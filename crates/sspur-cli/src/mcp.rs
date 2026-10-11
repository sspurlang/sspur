use crate::{agent, default_interp, fill, interp, AGENT_SPEC, AGENT_SPEC_MORE, REFERENCE};
use serde_json::{json, Value as Json};
use sspur_eval::fuzz::Options;
use sspur_store::{query::Ctx, Store, Tx};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const PROTOCOL: &str = "2025-06-18";

/// Kept under 2,048 characters: Claude Code truncates longer server instructions, so the spec
/// itself comes from the `start` tool.
const INSTRUCTIONS: &str = "SSPUR is a programming language whose code lives in a typechecked store (.sspur/), not in files. The language is new to you, so make your first call start, with the definition names your task mentions (names: 'a,b,c'): one call returns the language spec and this codebase, all of it if it is small, otherwise counts plus pack (each named definition with what it uses, its tests and callers). If you need more, use query (find, grep, body, callers, pack; body and pack take A,B,C). Then make every change in ONE edit call with test: true. An edit is atomic: if it is rejected nothing changed, so fix the listed errors (each has a hint) and resend the whole edit. Do not edit .ssp files or .sspur/ directly. The spec's CLI commands are these tools: `./sspur src` is src, `q X T` is query, `edit --test change.ssp` is edit with test: true and the definitions as src (no file needed).";

fn tool(name: &str, title: &str, desc: &str, props: Json, required: &[&str], read_only: bool) -> Json {
    json!({
        "name": name,
        "title": title,
        "description": desc,
        "inputSchema": {"type": "object", "properties": props, "required": required},
        "annotations": {"readOnlyHint": read_only, "destructiveHint": false, "idempotentHint": read_only, "openWorldHint": false}
    })
}

fn tools() -> Json {
    let none = json!({});
    json!([
        tool("start", "Spec and codebase overview", "Call this first. The core language spec (about 0.7k tokens) plus this codebase: all of it if small; otherwise counts, then pack of the definitions in names and find of other words in names.", json!({"names": {"type": "string", "description": "definition names or words from the task, comma-separated"}}), &[], true),
        tool("spec", "SSPUR language spec", "The core SSPUR spec (about 0.7k tokens; start includes it). more: true returns the rest of the builtins, full: true the long reference.", json!({"full": {"type": "boolean"}, "more": {"type": "boolean"}}), &[], true),
        tool("src", "Read all source", "The whole codebase as SSPUR source. Fine for small codebases; for large ones use query.", none.clone(), &[], true),
        tool("query", "Query the codebase", "Search and read code without printing all of it. query is one of: list (signatures by kind; target 'tests' lists tests), find (target 'a|b*': names matching, with signatures; or a type shape like 'List[Int] -> Int'), grep (target TEXT: definitions containing it, with the lines), body (target 'A' or 'A,B'), sig, callers, callees, effects, impact, pack (target with the context to edit it), why, holes, diag, log.", json!({"query": {"type": "string"}, "target": {"type": "string"}, "budget": {"type": "integer", "description": "token budget for pack (default 2000)"}, "json": {"type": "boolean"}}), &["query"], true),
        tool("edit", "Edit definitions", "Add or replace definitions by name: src holds SSPUR definitions (type, fn, test), optionally after lines 'rename OLD NEW' or 'remove NAME'. Put every change in one call. Atomic and typechecked; a rejected edit changes nothing and lists errors with fix hints; unambiguous spellings from other languages are stored in SSPUR form. test: true also runs all tests.", json!({"src": {"type": "string"}, "test": {"type": "boolean"}}), &["src"], false),
        tool("test", "Run tests", "Run every test. Prints failures and a pass/fail count.", none.clone(), &[], true),
        tool("check", "Typecheck", "Typecheck the codebase and list diagnostics with hints.", none.clone(), &[], true),
        tool("run", "Run main", "Run main() and return what it logged.", none.clone(), &[], true),
        tool("fuzz", "Property-test contracts", "Property-test every function from its pre, post and where clauses. Lists shrunk counterexamples.", json!({"cases": {"type": "integer"}}), &[], true),
        tool("fill", "Fill typed holes", "Synthesize an expression for each typed hole (? or ?name) from the values in scope and the functions it may call, checked against the tests and contracts. Prints the best candidate per hole with what it passed; apply: true writes the passing ones; all: true lists up to 3 per hole.", json!({"hole": {"type": "string", "description": "?name, or a function name for all its holes; default every hole"}, "apply": {"type": "boolean"}, "all": {"type": "boolean"}}), &[], false),
        tool("suggest", "Suggest contracts", "Run a function on test-derived and fuzzed inputs and propose pre/post clauses that held on every run (marked proved when z3 proves them), and a pre that rules out the inputs it traps on. apply: true adds them.", json!({"fn": {"type": "string", "description": "function name; default every function"}, "apply": {"type": "boolean"}}), &[], false),
        tool("apply", "Apply transaction ops", "Low-level: apply JSON ops (add, replace, rename, remove, refine, fill, attach) atomically. Prefer edit. Diagnostics include machine fix ops.", json!({"ops": {"type": "array", "items": {"type": "object"}}, "reason": {"type": "string"}, "agent": {"type": "string"}, "base": {"type": "string"}, "gate": {"type": "string", "enum": ["check", "tests"], "description": "'tests' rejects the transaction unless all tests pass"}, "test": {"type": "boolean", "description": "run all tests after applying"}, "json": {"type": "boolean"}}), &["ops"], false)
    ])
}

/// Accepts the names from before 0.3 (`sspur_edit`, `sspur_export`, ...).
fn canonical(name: &str) -> &str {
    match name.strip_prefix("sspur_").unwrap_or(name) {
        "export" => "src",
        n => n,
    }
}

struct Server {
    dir: PathBuf,
    store: Option<Store>,
}

impl Server {
    fn store(&mut self, create: bool) -> Result<&Store, String> {
        if self.store.is_none() {
            self.store = Store::find(&self.dir);
        }
        if self.store.is_none() && create {
            self.store = Some(Store::init(&self.dir).map_err(|e| format!("cannot create .sspur in {}: {e}", self.dir.display()))?);
        }
        self.store.as_ref().ok_or_else(|| format!("no SSPUR codebase (.sspur/) in {} or above; an edit creates one, or start the server with 'sspur mcp --dir PATH'", self.dir.display()))
    }
}

fn call(srv: &mut Server, name: &str, a: &Json) -> (String, bool) {
    let flag = |k: &str| a.get(k).and_then(Json::as_bool).unwrap_or(false);
    let name = canonical(name);
    if name == "spec" {
        return ((if flag("full") { REFERENCE } else if flag("more") { AGENT_SPEC_MORE } else { AGENT_SPEC }).to_string(), false);
    }
    if name == "start" {
        let names: Vec<String> = a.get("names").and_then(Json::as_str).map(String::from).into_iter().collect();
        let _ = srv.store(false);
        return (format!("{}\n{}", AGENT_SPEC.trim_end(), agent::start_text(srv.store.as_ref(), &names)), false);
    }
    let store = match srv.store(matches!(name, "edit" | "apply")) {
        Ok(s) => s,
        Err(e) => return (e, true),
    };
    let make = |l: &sspur_store::Loaded| default_interp(l);
    let test: Option<&dyn Fn(&sspur_store::Loaded) -> sspur_eval::Interp> = if flag("test") { Some(&make) } else { None };
    if name == "edit" {
        return match agent::edit_ops(store, a.get("src").and_then(Json::as_str).unwrap_or("")) {
            Ok(ops) => {
                let (t, ok) = agent::run_edit(store, ops, "mcp", test);
                (t, !ok)
            }
            Err(e) => (e, true),
        };
    }
    if name == "apply" && !flag("json") && a.get("gate").is_none() && a.get("base").is_none() {
        let ops = a.get("ops").and_then(Json::as_array).cloned().unwrap_or_default();
        let (t, ok) = agent::run_edit(store, ops, a.get("agent").and_then(Json::as_str).unwrap_or("mcp"), test);
        return (t, !ok);
    }
    if name == "apply" {
        let tx = Tx {
            base: a.get("base").and_then(Json::as_str).map(String::from),
            agent: Some(a.get("agent").and_then(Json::as_str).unwrap_or("mcp").to_string()),
            reason: a.get("reason").and_then(Json::as_str).map(String::from),
            gate: a.get("gate").and_then(Json::as_str).map(String::from),
            merge: a.get("merge").and_then(Json::as_bool).unwrap_or(false),
            ops: a.get("ops").and_then(Json::as_array).cloned().unwrap_or_default(),
            normalize: false,
        };
        let r = store.apply(tx);
        return (serde_json::to_string(&r).unwrap(), !r.ok);
    }
    let loaded = match store.load_head() {
        Ok(l) => l,
        Err(d) => return (agent::diag_lines("", &d, None).join("\n"), true),
    };
    match name {
        "query" => {
            let q = a.get("query").and_then(Json::as_str).unwrap_or("list");
            let budget = a.get("budget").and_then(Json::as_u64).unwrap_or(2000) as usize;
            let out = Ctx::new(&loaded, Some(store)).run(q, a.get("target").and_then(Json::as_str), budget);
            let err = out.get("error").is_some();
            if flag("json") { (serde_json::to_string(&out).unwrap(), err) } else { (agent::query_text(q, &out, &loaded.src, a.get("target").and_then(Json::as_str).unwrap_or("")), err) }
        }
        "check" => {
            let mut diags = loaded.check.diags.clone();
            fill::enrich(&loaded, &mut diags, Some(fill::budget(400)));
            let lines = agent::diag_lines(&loaded.src, &diags, None);
            let head = if loaded.check.has_errors() { "errors".to_string() } else { format!("ok {} definitions", loaded.module.defs.len()) };
            (std::iter::once(head).chain(lines).collect::<Vec<_>>().join("\n"), loaded.check.has_errors())
        }
        "src" => (if loaded.src.trim().is_empty() { "(empty codebase)".to_string() } else { loaded.src.clone() }, false),
        _ if loaded.check.has_errors() => (agent::diag_lines(&loaded.src, &loaded.check.diags, None).join("\n"), true),
        "test" => {
            let (t, ok) = agent::tests_text(&default_interp(&loaded).run_tests(), false);
            (t, !ok)
        }
        "run" => {
            let it = default_interp(&loaded);
            *it.output.borrow_mut() = Some(vec![]);
            let r = it.run_main();
            let mut lines: Vec<String> = it.output.borrow_mut().take().unwrap_or_default();
            if let Err(e) = &r {
                lines.push(format!("error: {e}"));
            }
            (if lines.is_empty() { "ok, no output".into() } else { lines.join("\n") }, r.is_err())
        }
        "fuzz" => {
            let cases = a.get("cases").and_then(Json::as_u64).unwrap_or(200) as usize;
            let reports = interp(&loaded).fuzz(&Options { cases, seed: 7, edge: false });
            let mut lines = Vec::new();
            let mut failed = 0;
            for r in &reports {
                if let Some((args, msg)) = &r.failure {
                    failed += 1;
                    lines.push(format!("FAIL  {}({}): {msg}", r.name, args.join(", ")));
                }
            }
            lines.push(format!("{} functions, {failed} with counterexamples", reports.len()));
            (lines.join("\n"), failed > 0)
        }
        "fill" => {
            let opts = sspur_eval::synth::Opts { budget_ms: crate::fill::budget(1000), ..Default::default() };
            let (mut text, good, _, ops) = fill::fill_report(&loaded, &opts, a.get("hole").and_then(Json::as_str), flag("all"), false);
            if flag("apply") && !ops.is_empty() {
                let (t, ok) = agent::run_edit(store, ops, "mcp", None);
                text = format!("{text}\n{t}");
                return (text, !ok || !good);
            }
            (text, !good)
        }
        "suggest" => {
            let only = a.get("fn").and_then(Json::as_str);
            let (mut text, out) = fill::suggest_report(&loaded, only, 200, false);
            let ops: Vec<Json> = out.iter().flat_map(|s| s.sugg.iter().map(|x| json!({"op": "refine", "target": s.name, "contract": {x.kind: x.text}}))).collect();
            if flag("apply") && !ops.is_empty() {
                let (t, ok) = agent::run_edit(store, ops, "mcp", None);
                text = format!("{text}\n{t}");
                return (text, !ok);
            }
            (text, only.is_some_and(|f| !out.iter().any(|s| s.name == f)))
        }
        _ => (format!("unknown tool '{name}'; tools: start spec src query edit test check run fuzz apply fill suggest"), true),
    }
}

pub fn serve(dir: Option<&String>) -> ExitCode {
    let dir = dir.map(PathBuf::from).or_else(|| std::env::var_os("SSPUR_DIR").map(PathBuf::from)).unwrap_or_else(|| std::env::current_dir().unwrap());
    let mut srv = Server { dir, store: None };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Json = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                let resp = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}});
                let _ = writeln!(stdout, "{resp}");
                let _ = stdout.flush();
                continue;
            }
        };
        let Some(id) = msg.get("id").cloned() else { continue };
        let method = msg.get("method").and_then(Json::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params.get("protocolVersion").and_then(Json::as_str).unwrap_or(PROTOCOL),
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "sspur", "title": "SSPUR", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "resources/list" => Ok(json!({"resources": []})),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "prompts/list" => Ok(json!({"prompts": []})),
            "tools/call" => {
                let name = params.get("name").and_then(Json::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let (text, is_error) = call(&mut srv, name, &args);
                Ok(json!({"content": [{"type": "text", "text": text}], "isError": is_error}))
            }
            m => Err(json!({"code": -32601, "message": format!("method not found: {m}")})),
        };
        let resp = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
        };
        if writeln!(stdout, "{resp}").is_err() {
            break;
        }
        let _ = stdout.flush();
    }
    ExitCode::SUCCESS
}
