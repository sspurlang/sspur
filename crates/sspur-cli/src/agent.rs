use serde_json::{json, Value as Json};
use sspur_check::Diag;
use sspur_eval::Interp;
use sspur_store::{query::Ctx, Change, Loaded, Store, Tx, TxResult};
use sspur_syntax::{ast::Def, line_col, printer::print_def};
use std::collections::BTreeSet;

/// Ends a rejected edit: weak models otherwise retry one definition per call.
pub const RESEND: &str = "fix these and resend the whole edit in one call";

pub fn diag_lines(src: &str, diags: &[Diag], only: Option<&BTreeSet<String>>) -> Vec<String> {
    let defs: Vec<(String, u32)> = sspur_syntax::parse(src).map(|m| m.defs.iter().map(|d| (d.name().to_string(), d.span().start)).collect()).unwrap_or_default();
    let mut out = Vec::new();
    for d in diags {
        if d.severity != "error" && only.is_some_and(|s| !d.def.as_ref().is_some_and(|n| s.contains(n))) {
            continue;
        }
        let off = d.span[0];
        let start = d
            .def
            .as_ref()
            .and_then(|n| defs.iter().find(|(m, _)| m == n))
            .or_else(|| defs.iter().filter(|(_, s)| *s <= off).max_by_key(|(_, s)| *s))
            .filter(|_| !src.is_empty() && d.span != [0, 0]);
        let sev = if d.severity == "error" { String::new() } else { format!("{} ", d.severity) };
        let at = match start {
            Some((name, s)) => {
                let (l, c) = line_col(src, off);
                let (dl, _) = line_col(src, *s);
                format!("{name}:{}:{c} ", l + 1 - dl)
            }
            None => d.def.as_ref().map(|n| format!("{n}: ")).unwrap_or_default(),
        };
        out.push(format!("{sev}{at}{} {}", d.code, d.msg));
        if let Some(h) = &d.hint {
            out.push(format!("  hint: {h}"));
        }
        out.extend(crate::fill::fix_lines(d));
    }
    out
}

pub fn change_text(c: &Change) -> String {
    match (&c.old, &c.new, &c.renamed_from) {
        (_, _, Some(f)) => format!("{f}->{}", c.path),
        (None, Some(_), _) => format!("+{}", c.path),
        (Some(_), None, _) => format!("-{}", c.path),
        _ => format!("~{}", c.path),
    }
}

pub fn tx_text(r: &TxResult) -> String {
    tx_text_for(r, None)
}

fn tx_text_for(r: &TxResult, named: Option<&BTreeSet<String>>) -> String {
    let src = r.src.as_deref().unwrap_or("");
    if !r.ok {
        let mut lines = vec!["rejected, nothing changed".to_string()];
        let errors: Vec<Diag> = r.diags.iter().filter(|d| d.is_error()).cloned().collect();
        lines.extend(diag_lines(src, &errors, None));
        for c in &r.conflicts {
            if let Some(t) = &c.theirs {
                lines.push(format!("theirs {}:", c.theirs_name.as_deref().unwrap_or(&c.path)));
                lines.extend(t.lines().map(|l| format!("  {l}")));
            }
        }
        if !r.conflicts.is_empty()
            && let Some(b) = r.rebase.as_ref().and_then(|b| b["base"].as_str()) {
                lines.push(format!("HEAD is now {b}"));
            }
        if r.conflicts.is_empty() && !errors.is_empty() {
            lines.push(RESEND.to_string());
        }
        return lines.join("\n");
    }
    let shown: Vec<&Change> = r.changes.iter().filter(|c| named.is_none_or(|n| c.old.is_none() || c.new.is_none() || c.renamed_from.is_some() || n.contains(&c.path))).collect();
    let touched: BTreeSet<String> = shown.iter().map(|c| c.path.clone()).collect();
    let mut lines = vec![if shown.is_empty() { "ok, no changes".to_string() } else { format!("ok {}", shown.iter().map(|c| change_text(c)).collect::<Vec<_>>().join(" ")) }];
    if !r.notes.is_empty() {
        const SHOWN_NOTES: usize = 3;
        let more = r.notes.len().saturating_sub(SHOWN_NOTES);
        let tail = if more > 0 { format!(", and {more} more") } else { String::new() };
        lines.push(format!("stored as: {}{tail}", r.notes[..r.notes.len().min(SHOWN_NOTES)].join(", ")));
    }
    lines.extend(diag_lines(src, &r.diags, Some(&touched)));
    lines.join("\n")
}

pub fn tests_text(results: &[(String, Result<(), String>)], full: bool) -> (String, bool) {
    let failed = results.iter().filter(|(_, r)| r.is_err()).count();
    let mut lines = Vec::new();
    for (name, r) in results {
        match r {
            Ok(()) if full => lines.push(format!("pass  {name}")),
            Ok(()) => {}
            Err(e) => lines.push(format!("FAIL  {name}: {e}")),
        }
    }
    lines.push(format!("{} passed, {failed} failed", results.len() - failed));
    (lines.join("\n"), failed == 0)
}

fn directive(line: &str) -> Option<Json> {
    let w: Vec<&str> = line.split_whitespace().collect();
    let ident = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_');
    let qualified = |s: &str| s.split_once('.').is_some_and(|(p, n)| ident(p) && ident(n));
    match w.as_slice() {
        ["rename", a, b] if ident(a) && ident(b) && !line.starts_with(' ') => Some(json!({"op": "rename", "from": a, "to": b})),
        ["remove", a] if ident(a) && !line.starts_with(' ') => Some(json!({"op": "remove", "path": a})),
        ["remove", "use", p] if ident(p) && !line.starts_with(' ') => Some(json!({"op": "remove", "path": format!("use {p}")})),
        ["remove", "impl", t, "for", ty] if !line.starts_with(' ') => Some(json!({"op": "remove", "path": format!("impl {t} for {ty}")})),
        ["remove", a] | ["rename", a, _] if qualified(a) && !line.starts_with(' ') => Some(json!({"error": format!("E_DEP_READONLY {a} belongs to dependency {}; dependency code can't be edited here", a.split('.').next().unwrap_or(""))})),
        _ => None,
    }
}

pub fn edit_ops(store: &Store, input: &str) -> Result<Vec<Json>, String> {
    let mut renames = Vec::new();
    let mut removes = Vec::new();
    let mut body = String::new();
    for line in input.lines() {
        match directive(line) {
            Some(op) if op.get("error").is_some() => return Err(format!("rejected, nothing changed\n{}", op["error"].as_str().unwrap_or(""))),
            Some(op) if op["op"] == "rename" => renames.push(op),
            Some(op) => removes.push(op),
            None => body.push_str(line),
        }
        body.push('\n');
    }
    let (module, said) = sspur_syntax::parse_all_noted(&body).map_err(|errs| {
        let mut lines = vec!["rejected, nothing changed".to_string()];
        for e in &errs {
            let (l, c) = line_col(&body, e.span.start);
            lines.push(format!("input:{l}:{c} {} {}", e.code, e.msg));
            if let Some(h) = sspur_syntax::syntax_hint(&body, e) {
                lines.push(format!("  hint: {h}"));
            }
        }
        lines.push(RESEND.to_string());
        lines.join("\n")
    })?;
    if module.defs.is_empty() && renames.is_empty() && removes.is_empty() {
        return Err("nothing to edit: give definitions, 'rename OLD NEW', or 'remove NAME'".into());
    }
    let mut names: BTreeSet<String> = store.head_root().map(|r| r.names.keys().cloned().collect()).unwrap_or_default();
    for r in &renames {
        let from = r["from"].as_str().unwrap_or_default();
        if names.remove(from) {
            names.insert(r["to"].as_str().unwrap_or_default().to_string());
        }
    }
    let mut ops = renames;
    for d in &module.defs {
        let kind = if names.insert(d.name().to_string()) { "add" } else { "replace" };
        ops.push(json!({"op": kind, "path": d.name(), "src": print_def(d)}));
    }
    ops.extend(removes);
    ops.extend(said.into_iter().map(|n| json!({"op": "note", "text": n})));
    Ok(ops)
}

pub fn run_edit(store: &Store, ops: Vec<Json>, agent: &str, test: Option<&dyn Fn(&sspur_store::Loaded) -> Interp>) -> (String, bool) {
    let (said, ops): (Vec<Json>, Vec<Json>) = ops.into_iter().partition(|o| o["op"] == "note");
    let named: BTreeSet<String> = ops.iter().flat_map(|o| ["path", "target", "to"].map(|k| o.get(k).and_then(Json::as_str).map(String::from))).flatten().collect();
    let mut r = store.apply(Tx { base: None, agent: Some(agent.into()), reason: None, gate: None, merge: false, ops, normalize: true });
    if r.ok {
        let mut all: Vec<String> = said.iter().filter_map(|o| o["text"].as_str().map(String::from)).collect();
        all.append(&mut r.notes);
        r.notes = all;
    }
    let mut text = tx_text_for(&r, Some(&named));
    if !r.ok {
        return (text, false);
    }
    let Some(make) = test else { return (text, true) };
    match store.load_head() {
        Ok(l) => {
            let (t, ok) = tests_text(&make(&l).run_tests(), false);
            text.push('\n');
            text.push_str(&t);
            (text, ok)
        }
        Err(d) => (diag_lines("", &d, None).join("\n"), false),
    }
}

pub fn parse_tx(raw: &str) -> Result<Tx, String> {
    let v: Json = serde_json::from_str(raw).map_err(|e| format!("invalid transaction JSON: {e}"))?;
    let v = if v.is_array() { json!({"ops": v}) } else { v };
    serde_json::from_value(v).map_err(|e| format!("invalid transaction: {e}"))
}

fn names(v: &Json) -> String {
    let xs: Vec<&str> = v.as_array().map(|a| a.iter().filter_map(Json::as_str).collect()).unwrap_or_default();
    if xs.is_empty() { "(none)".into() } else { xs.join(" ") }
}

/// `pattern` is the `find` or `grep` argument; definitions with exactly that name are printed first.
pub fn query_text(q: &str, out: &Json, src: &str, pattern: &str) -> String {
    if let Some(e) = out.get("error") {
        let mut s = format!("{} {}", e.as_str().unwrap_or("E_QUERY"), out["msg"].as_str().unwrap_or(""));
        if let Some(c) = out.get("close").filter(|c| c.as_array().is_some_and(|a| !a.is_empty())) {
            s.push_str(&format!(" (did you mean: {})", names(c)));
        }
        if let Some(qs) = out.get("queries") {
            s.push_str(&format!("\nqueries: {}", names(qs)));
        }
        return s;
    }
    if let Some(t) = out.get("text").and_then(Json::as_str) {
        return t.to_string();
    }
    let arr = || out.as_array().cloned().unwrap_or_default();
    match q {
        "list" => list_text(&arr()),
        "find" | "grep" if out.get("hits").is_some() => hits_text(q, out, pattern),
        "sig" => out["sig"].as_str().unwrap_or("").to_string(),
        "body" => out["src"].as_str().unwrap_or("").to_string(),
        "callers" | "callees" | "effects" => names(out),
        "find" => arr().iter().map(|h| format!("{}: {}", h["name"].as_str().unwrap_or(""), h["type"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n"),
        "impact" => format!("dependents: {}\ntests: {}", names(&out["dependents"]), names(&out["tests"])),
        "holes" | "diag" => {
            let diags: Vec<Diag> = arr().iter().map(diag_of_json).collect();
            let lines = diag_lines(src, &diags, None);
            if lines.is_empty() { "(none)".into() } else { lines.join("\n") }
        }
        "log" => arr().iter().map(|r| format!("{} {} {} [{}]", r["at"].as_str().unwrap_or(""), r["agent"].as_str().unwrap_or(""), r["reason"].as_str().unwrap_or(""), names(&r["changes"]))).collect::<Vec<_>>().join("\n"),
        "why" => {
            let p = &out["prov"];
            let mut lines = vec![format!("{} by {} at {}: {}", out["name"].as_str().unwrap_or(""), p["agent"].as_str().unwrap_or("?"), p["at"].as_str().unwrap_or("?"), p["reason"].as_str().unwrap_or(""))];
            if let Some(reqs) = p.get("reqs").filter(|r| r.as_array().is_some_and(|a| !a.is_empty())) {
                lines.push(format!("reqs: {}", names(reqs)));
            }
            for h in out["history"].as_array().into_iter().flatten() {
                lines.push(format!("  {} {} {}", h["at"].as_str().unwrap_or(""), h["agent"].as_str().unwrap_or(""), h["reason"].as_str().unwrap_or("")));
            }
            lines.join("\n")
        }
        _ => out.to_string(),
    }
}

/// Tests listed by name on one line when there are at most this many; otherwise only counted.
const INLINE_TESTS: usize = 20;

fn list_text(items: &[Json]) -> String {
    let kinds = [("use", "uses"), ("type", "types"), ("effect", "effects"), ("trait", "traits"), ("impl", "impls"), ("store", "stores"), ("static", "statics"), ("svc", "svcs"), ("fn", "fns"), ("test", "tests")];
    let only_tests = !items.is_empty() && items.iter().all(|d| d["kind"] == "test");
    let mut lines = Vec::new();
    for (k, plural) in kinds {
        let group: Vec<&Json> = items.iter().filter(|d| d["kind"] == k).collect();
        if group.is_empty() {
            continue;
        }
        let names = || group.iter().filter_map(|d| d["name"].as_str()).collect::<Vec<_>>();
        if k != "test" {
            lines.push(format!("# {plural} {}", group.len()));
            lines.extend(group.iter().map(|d| d["sig"].as_str().unwrap_or("").to_string()));
        } else if only_tests {
            lines.push(format!("# tests {}", group.len()));
            lines.extend(names().into_iter().map(String::from));
        } else if group.len() <= INLINE_TESTS {
            lines.push(format!("# tests {}: {}", group.len(), names().join(" ")));
        } else {
            lines.push(format!("# tests {} (q list tests, or q find WORD)", group.len()));
        }
    }
    if lines.is_empty() { "(none)".into() } else { lines.join("\n") }
}

/// `find` prints this many signatures and `grep` this many matching definitions; then only names.
const FIND_HITS: usize = 25;
const GREP_HITS: usize = 12;
const TEXT_NAMES: usize = 40;

fn hits_text(q: &str, out: &Json, pattern: &str) -> String {
    let mut hits = out["hits"].as_array().cloned().unwrap_or_default();
    hits.sort_by_key(|h| !pattern.split('|').any(|a| h["name"].as_str().is_some_and(|n| a.trim().trim_start_matches('^').trim_end_matches('$').eq_ignore_ascii_case(n))));
    let total = out["total"].as_u64().unwrap_or(hits.len() as u64) as usize;
    let mut lines = Vec::new();
    let mut tests = Vec::new();
    let mut rest = Vec::new();
    let mut shown = 0;
    for h in &hits {
        let name = h["name"].as_str().unwrap_or("");
        if q == "find" && h["kind"] == "test" {
            tests.push(name.to_string());
            continue;
        }
        if shown == if q == "grep" { GREP_HITS } else { FIND_HITS } {
            rest.push(name);
            continue;
        }
        shown += 1;
        lines.push(h["sig"].as_str().unwrap_or("").to_string());
        for l in h["lines"].as_array().into_iter().flatten() {
            lines.push(format!("  {}", l.as_str().unwrap_or("").trim()));
        }
        if let Some(n) = h["more"].as_u64().filter(|n| *n > 0) {
            lines.push(format!("  .. {n} more lines (q body {name})"));
        }
    }
    if !tests.is_empty() {
        lines.push(format!("tests: {}", tests.join(" ")));
    }
    let more = rest.len() + total.saturating_sub(hits.len());
    if more > 0 {
        let listed = &rest[..rest.len().min(TEXT_NAMES)];
        let unlisted = if more > listed.len() { format!(" (+{}; narrow the pattern)", more - listed.len()) } else { String::new() };
        lines.push(format!("-- {more} more: {}{unlisted}", listed.join(" ")));
    }
    if lines.is_empty() { "(none)".into() } else { lines.join("\n") }
}

fn diag_of_json(d: &Json) -> Diag {
    let severity = match d["severity"].as_str() {
        Some("error") => "error",
        Some("warning") => "warning",
        Some("hole") => "hole",
        _ => "note",
    };
    let span = [d["span"][0].as_u64().unwrap_or(0) as u32, d["span"][1].as_u64().unwrap_or(0) as u32];
    let s = |k: &str| d[k].as_str().map(String::from);
    Diag { code: s("code").unwrap_or_default(), severity, def: s("def"), span, msg: s("msg").unwrap_or_default(), hint: s("hint"), fix: vec![] }
}

/// `start` prints a codebase of at most this many bytes of source (about 3.7k tokens) whole.
pub const START_SRC: usize = 12_000;

fn counts_line(l: &Loaded) -> String {
    let kinds = [("use", "uses"), ("type", "types"), ("effect", "effects"), ("store", "stores"), ("static", "statics"), ("svc", "svcs"), ("fn", "fns"), ("test", "tests")];
    let kind = |d: &Def| match d {
        Def::Use(_) => "use",
        Def::Type(_) => "type",
        Def::Effect(_) => "effect",
        Def::Store(_) => "store",
        Def::Static(_) => "static",
        Def::Svc(_) => "svc",
        Def::Fn(_) => "fn",
        Def::Test(_) => "test",
        Def::Trait(_) => "trait",
        Def::Impl(_) => "impl",
    };
    let parts: Vec<String> = kinds
        .iter()
        .filter_map(|(k, plural)| {
            let n = l.own_defs().iter().filter(|d| kind(d) == *k).count();
            (n > 0).then(|| format!("{n} {}", if n == 1 { k } else { plural }))
        })
        .collect();
    if parts.is_empty() { "empty".into() } else { parts.join(", ") }
}

/// One query as text under a `## q ...` heading.
fn query_section(l: &Loaded, store: &Store, q: &str, target: Option<&str>) -> String {
    let out = Ctx::new(l, Some(store)).run(q, target, 2000);
    let head = match target {
        Some(t) if t.contains(|c: char| " |*^$".contains(c)) => format!("## q {q} '{t}'"),
        Some(t) => format!("## q {q} {t}"),
        None => format!("## q {q}"),
    };
    format!("{head}\n{}", query_text(q, &out, &l.src, target.unwrap_or("")))
}

/// Queries that `spec` runs without a target.
const BARE_QUERIES: [&str; 4] = ["list", "holes", "diag", "log"];

/// `spec [src] [QUERY TARGET]...`: the text to print after the spec, so one call reads the spec and searches.
pub fn spec_queries(store: Option<&Store>, args: &[String]) -> Result<String, String> {
    let Some(store) = store else { return Err("no .sspur codebase here; run 'sspur init'".into()) };
    let l = store.load_head().map_err(|d| diag_lines("", &d, None).join("\n"))?;
    let mut parts = Vec::new();
    let mut it = args.iter();
    while let Some(q) = it.next() {
        if q == "src" {
            parts.push(format!("## src\n{}", l.src.trim_end()));
        } else if BARE_QUERIES.contains(&q.as_str()) {
            parts.push(query_section(&l, store, q, None));
        } else {
            let t = it.next().ok_or_else(|| format!("query '{q}' needs a target: spec {q} TARGET"))?;
            parts.push(query_section(&l, store, q, Some(t)));
        }
    }
    Ok(parts.join("\n\n"))
}

/// `start [NAME|PATTERN]...`: what an agent needs before its first edit, printed after the spec.
/// A small codebase is printed whole; a large one gets its counts, `q pack` of the arguments
/// that name a definition and `q find` of the others.
pub fn start_text(store: Option<&Store>, terms: &[String]) -> String {
    let Some(store) = store else {
        return "## This codebase\nnone yet (no .sspur/ here or above): `./sspur init file.ssp` imports a file, or the first edit creates it.".into();
    };
    let l = match store.load_head() {
        Ok(l) => l,
        Err(d) => return format!("## This codebase does not load\n{}", diag_lines("", &d, None).join("\n")),
    };
    let src = l.src.trim_end();
    if src.len() <= START_SRC {
        return format!("## This codebase: {}, all of it\n{src}", counts_line(&l));
    }
    let mut out = vec![format!("## This codebase: {}, about {}k tokens of source, too much to print. Search it with `q find|grep|body|callers|pack`.", counts_line(&l), (src.len() * 10 / 32).div_ceil(1000))];
    let words: Vec<&str> = terms.iter().flat_map(|t| t.split([',', ' '])).map(str::trim).filter(|t| !t.is_empty()).collect();
    let (names, patterns): (Vec<&str>, Vec<&str>) = words.iter().partition(|w| l.own_defs().iter().any(|d| d.name() == **w));
    if !names.is_empty() {
        out.push(query_section(&l, store, "pack", Some(&names.join(","))));
    }
    if !patterns.is_empty() {
        out.push(query_section(&l, store, "find", Some(&patterns.join("|"))));
    }
    if words.is_empty() {
        out.push("`./sspur start NAME...` also packs the named definitions (and finds other words) in this output.".into());
    }
    out.join("\n\n")
}
