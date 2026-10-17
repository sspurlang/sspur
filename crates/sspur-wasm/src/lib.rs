//! SSPUR in the browser: the checker, the formatter and the interpreter behind a JSON API for the
//! playground. Every call takes the whole source and returns a JSON string.
use serde_json::{json, Value as J};
use sspur_check::{CheckOutput, Diag};
use sspur_eval::Interp;
use sspur_syntax::{Def, Module};
use sspur_ide as ide;
use std::cell::RefCell;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

thread_local! {
    /// The last source that parsed, with its check, for completions and hover.
    static LAST: RefCell<Option<Arc<ide::Analysis>>> = const { RefCell::new(None) };
}

/// Runs the interpreter so that deep recursion doesn't overflow the browser's stack.
fn drive<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(target_family = "wasm")]
    return sspur_eval::hop::drive(f);
    #[cfg(not(target_family = "wasm"))]
    f()
}

fn ctx() -> ide::Ctx<'static> {
    ide::Ctx { load: &load, last: LAST.with(|l| l.borrow().clone()), exports: Default::default() }
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Date, js_name = now)]
    fn date_now() -> f64;
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn perf_now() -> f64;
    #[wasm_bindgen(js_namespace = globalThis, js_name = sspurPanic)]
    fn report_panic(msg: &str);
}

/// A panic aborts the instance; the page shows the message and starts a fresh one.
#[cfg(target_family = "wasm")]
#[wasm_bindgen(start)]
pub fn init() {
    std::panic::set_hook(Box::new(|p| report_panic(&p.to_string())));
}

/// Diagnostics, like `sspur check --json`, with line and column and editor offsets added.
#[wasm_bindgen]
pub fn check(src: &str) -> String {
    let diags = match load(src) {
        Ok((m, c)) => {
            let mut d = c.diags.clone();
            if !c.has_errors()
                && let Err(e) = sspur_check::elab::lower(&m, &c)
            {
                d.push(lower_diag(e));
            }
            fill_holes(&m, &mut d);
            let j = ide::diags_json(src, &d, Some(&m));
            LAST.with(|l| *l.borrow_mut() = Some(Arc::new(ide::Analysis { src: src.to_string(), module: m, check: c })));
            j
        }
        Err(d) => ide::diags_json(src, &d, None),
    };
    json!({ "diags": diags }).to_string()
}

/// Runs `main` in the interpreter with `stdin` as its input: its output, then how it ended.
#[wasm_bindgen]
pub fn run(src: &str, stdin: &str) -> String {
    start();
    #[cfg(target_family = "wasm")]
    sspur_eval::web::set_stdin(stdin);
    #[cfg(not(target_family = "wasm"))]
    let _ = stdin;
    let (it, diags) = match prepare(src) {
        Ok(x) => x,
        Err(d) => return json!({ "status": "error", "diags": d, "output": [] }).to_string(),
    };
    *it.output.borrow_mut() = Some(vec![]);
    let r = drive(|| it.run_main());
    let output = lines(it.output.borrow_mut().take().unwrap_or_default());
    let end = match (exit_code(), r) {
        (Some(code), _) => json!({ "status": "exit", "code": code }),
        (None, Ok(())) => json!({ "status": "ok" }),
        (None, Err(e)) => json!({ "status": "trap", "message": format!("runtime error: {e}") }),
    };
    let mut out = json!({ "output": output, "diags": diags });
    if let (J::Object(o), J::Object(e)) = (&mut out, end) {
        o.extend(e);
    }
    out.to_string()
}

/// Runs the examples and tests, printed the way `sspur test` prints them.
#[wasm_bindgen]
pub fn test(src: &str) -> String {
    start();
    let (it, diags) = match prepare(src) {
        Ok(x) => x,
        Err(d) => return json!({ "status": "error", "diags": d, "tests": [] }).to_string(),
    };
    let results = drive(|| it.run_tests());
    let failed = results.iter().filter(|(_, r)| r.is_err()).count();
    let mut text: Vec<String> = results.iter().filter_map(|(n, r)| r.as_ref().err().map(|e| format!("FAIL  {n}: {e}"))).collect();
    text.push(format!("{} passed, {failed} failed", results.len() - failed));
    let tests: Vec<J> = results
        .iter()
        .map(|(n, r)| match r {
            Ok(()) => json!({ "name": n, "ok": true }),
            Err(e) => json!({ "name": n, "ok": false, "message": e }),
        })
        .collect();
    json!({ "status": if failed == 0 { "ok" } else { "failed" }, "tests": tests, "passed": results.len() - failed, "failed": failed, "text": text.join("\n"), "diags": diags }).to_string()
}

/// The source as `sspur fmt` prints it.
#[wasm_bindgen]
pub fn fmt(src: &str) -> String {
    match sspur_syntax::format_source(src) {
        Ok(text) => json!({ "ok": true, "text": text }).to_string(),
        Err(e) => json!({ "ok": false, "diags": ide::diags_json(src, &[sspur_check::syntax_diag_in(src, &e)], None) }).to_string(),
    }
}

/// The facts the registry checks read, for the Publish page. `input` is
/// `{"manifest": "<sspur.toml>", "files": {"lib.ssp": "<text>", "models/sspur.toml": ...}}`,
/// paths relative to the repository root. Every module (a path dependency inside the package)
/// is parsed and formatted here; one with dependencies is typechecked only by the registry
/// workflow.
#[cfg(feature = "registry")]
#[wasm_bindgen]
pub fn pkg_facts(input: &str) -> String {
    use sspur_registry::facts;
    let v: J = serde_json::from_str(input).unwrap_or(J::Null);
    let files = &v["files"];
    let read = |p: &str| files.get(p).and_then(J::as_str).map(String::from);
    let root_manifest = v["manifest"].as_str().unwrap_or("");
    let (mods, errs) = facts::modules(root_manifest, &read);
    let one = |path: &str, mf: &J| -> J {
        let has_deps = mf["deps"].as_array().is_some_and(|d| !d.is_empty());
        match files[path].as_str() {
            Some(src) => facts::facts(path, src, if has_deps { facts::Check::Skip } else { facts::Check::Here }),
            None => json!({"source": path, "parse": false, "checked": false, "diags": [{"code": "E_NO_SOURCE", "severity": "error", "file": path, "line": 0, "col": 0, "msg": format!("no {path} in the repository"), "hint": "Set src in sspur.toml, or add lib.ssp."}], "fmt": {"clean": false}, "defs": [], "holes": [], "run_cmd": [], "tests": 0, "tested": []}),
        }
    };
    let mf = facts::manifest(root_manifest);
    let mut f = one(&mods[0].src_path, &mf);
    let units: Vec<J> = mods[1..]
        .iter()
        .map(|m| {
            let mm = facts::manifest(&m.manifest);
            let mut u = one(&m.src_path, &mm);
            u["key"] = json!(m.key);
            u["dir"] = json!(m.dir);
            u["manifest"] = mm;
            u
        })
        .collect();
    f["manifest"] = mf;
    f["modules"] = J::Array(units);
    f["module_errors"] = J::Array(errs);
    f.to_string()
}

/// A rough token count: about four characters per token. The token benchmark uses real
/// tokenizers, which are too big to ship to the page.
#[wasm_bindgen]
pub fn tokens(src: &str) -> u32 {
    src.split_whitespace().map(|w| w.chars().count().div_ceil(4)).sum::<usize>() as u32
}

/// Completions at `pos` (a UTF-16 offset): names in scope, fields and methods after a dot.
#[wasm_bindgen]
pub fn complete(src: &str, pos: u32) -> String {
    ide::complete(&ctx(), src, pos as usize).to_string()
}

/// The type or signature of the name under `pos`.
#[wasm_bindgen]
pub fn hover(src: &str, pos: u32) -> String {
    ide::hover(&ctx(), src, pos as usize).to_string()
}

/// The signature of the call at `pos`, with the argument the cursor is at, or null.
#[wasm_bindgen]
pub fn signature(src: &str, pos: u32) -> String {
    ide::sig::signature_help(&ctx(), src, pos as usize).to_string()
}

/// Inlay hints for `src`: `[{at, label, kind}]` with `kind` "type" or "param". `opts` is JSON like
/// `{"types": true, "lambdas": true, "params": false}`; a missing key is on.
#[wasm_bindgen]
pub fn inlay_hints(src: &str, opts: &str) -> String {
    let opts = ide::hints::Opts::from_json(&serde_json::from_str(opts).unwrap_or(J::Null));
    let cx = ctx();
    match cx.last.clone().filter(|a| a.src == src).or_else(|| cx.analyze(src)) {
        Some(a) => ide::hints::hints_json(&a, &opts).to_string(),
        None => "[]".into(),
    }
}

/// Builtin functions, methods, types, keywords and effects with signatures and short docs.
#[wasm_bindgen]
pub fn builtins() -> String {
    ide::builtins_json().to_string()
}

fn start() {
    #[cfg(target_family = "wasm")]
    {
        sspur_eval::web::set_clock(|| (date_now(), perf_now()));
        sspur_eval::web::reset();
    }
}

fn exit_code() -> Option<i64> {
    #[cfg(target_family = "wasm")]
    return sspur_eval::web::exit_code();
    #[cfg(not(target_family = "wasm"))]
    None
}

fn lines(out: Vec<String>) -> Vec<J> {
    out.into_iter()
        .map(|l| match l.strip_prefix('\u{1}') {
            Some(e) => json!({ "stream": "err", "text": e }),
            None => json!({ "stream": "out", "text": l }),
        })
        .collect()
}

/// Parses and checks one file. Packages need a manifest and a cache, which a page doesn't have.
fn load(src: &str) -> Result<(Module, CheckOutput), Vec<Diag>> {
    let module = sspur_syntax::parse(src).map_err(|e| vec![sspur_check::syntax_diag_in(src, &e)])?;
    if let Some(Def::Use(u)) = module.defs.iter().find(|d| matches!(d, Def::Use(_))) {
        let e = sspur_syntax::SyntaxError::new("E_PKG_UNKNOWN", format!("no dependency '{}': packages are not available in the playground", u.pkg), u.span);
        return Err(vec![sspur_check::syntax_diag(&e)]);
    }
    let check = sspur_check::check(&module);
    Ok((module, check))
}

/// Traits elaborated into plain definitions, as `Loaded::executable` does.
fn executable(m: Module, c: CheckOutput) -> Result<(Module, CheckOutput), Vec<Diag>> {
    match sspur_check::elab::lower(&m, &c) {
        Ok(None) => Ok((m, c)),
        Ok(Some(x)) => Ok(x),
        Err(e) => Err(vec![lower_diag(e)]),
    }
}

/// An elaboration error as a diagnostic, the way `pkg::dep_diag` reports it.
fn lower_diag(e: String) -> Diag {
    let (code, msg) = match e.split_once(' ') {
        Some((c, r)) if c.starts_with("E_") && c.chars().all(|x| x.is_ascii_uppercase() || x == '_') => (c.to_string(), r.to_string()),
        _ => ("E_DEP".to_string(), e),
    };
    Diag { code, severity: "error", def: None, span: [0, 0], msg, hint: None, fix: vec![] }
}

fn prepare(src: &str) -> Result<(Interp, Vec<J>), Vec<J>> {
    let (m, c) = load(src).map_err(|d| ide::diags_json(src, &d, None))?;
    let diags = ide::diags_json(src, &c.diags, Some(&m));
    if c.has_errors() {
        return Err(diags);
    }
    let (m, c) = executable(m.clone(), c).map_err(|d| ide::diags_json(src, &d, Some(&m)))?;
    Ok((interp(&m, &c), diags))
}

static FILL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(400);

/// The time hole filling may spend per hole, in ms (400 by default). Tests set it high so their
/// results depend on the search, not on how fast the machine is.
#[wasm_bindgen]
pub fn set_fill_budget(ms: u32) {
    FILL_MS.store(u64::from(ms), std::sync::atomic::Ordering::Relaxed);
}

/// Ranked candidates for each typed hole, as the `fill` fixes `sspur check` gives, on a smaller budget.
fn fill_holes(m: &Module, diags: &mut [Diag]) {
    if !diags.iter().any(|d| d.code == "E_HOLE") || diags.iter().any(Diag::is_error) {
        return;
    }
    start();
    let (check, ctx) = sspur_check::check_holes(m);
    let Ok((xm, xc)) = executable(m.clone(), sspur_check::check(m)) else { return };
    let mut it = Interp::new(&xm, xc.record_types.clone(), xc.user_methods.clone(), xc.gen_loops.clone());
    it.set_check(&xc);
    let opts = sspur_eval::synth::Opts { budget_ms: FILL_MS.load(std::sync::atomic::Ordering::Relaxed), fuzz: 12, envs: 8, ..Default::default() };
    for f in sspur_eval::synth::fill(m, &check, &ctx, &it, &opts, None) {
        if let Some(d) = diags.iter_mut().find(|d| d.code == "E_HOLE" && d.span == [f.site.span.start, f.site.span.end]) {
            d.fix = f.cands.iter().map(|c| json!({"op": "fill", "hole": f.site.label(), "expr": c.text, "evidence": c.evidence(), "ok": c.ok()})).collect();
        }
    }
}

/// The interpreter set up as `sspur run --interp` sets it up.
fn interp(m: &Module, c: &CheckOutput) -> Interp {
    let own = m.own.unwrap_or(m.defs.len());
    let mut it = Interp::new(m, c.record_types.clone(), c.user_methods.clone(), c.gen_loops.clone());
    it.foreign = m.defs[own..].iter().map(|d| d.name().to_string()).collect();
    it.set_check(c);
    it.set_ownership(c.own.moves.clone(), c.own.inplace.clone());
    it.float_sums = c
        .expr_types
        .iter()
        .filter(|((_, _, tag), t)| matches!(tag, 4 | 5) && matches!(t, sspur_check::Type::Con(n, a) if n == "F64" && a.is_empty()))
        .map(|((s, e, _), _)| (*s, *e))
        .collect();
    it
}
