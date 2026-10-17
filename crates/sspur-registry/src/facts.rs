//! Package facts: what the registry checks need to know about a package's source, from the
//! parser and the checker. `sspur api --json`, `sspur publish` and the browser checker all build
//! them here, so the checks see the same facts wherever they run. Nothing here runs package code.

use serde_json::{json, Value as J};
use sspur_check::Diag;
use sspur_syntax::lexer::{lex, Tok};
use sspur_syntax::visit::walk_expr;
use sspur_syntax::*;
use std::collections::{BTreeMap, BTreeSet};

pub const FACTS_VERSION: u32 = 1;

/// The result of checking the source somewhere else (the CLI links the package with its
/// dependencies first). `diags` spans are offsets into the source; spans at or past its end
/// belong to dependencies.
pub struct Checked<'a> {
    pub diags: &'a [Diag],
    pub hashes: &'a BTreeMap<String, String>,
}

fn short_hash(s: &str) -> String {
    sspur_hash::base32(&blake3::hash(s.as_bytes()).as_bytes()[..16])
}

/// For each line, the byte offset where its code ends: everything after it is a comment or
/// blank, `None` for a line with no code. And for each line, whether it continues the
/// statement of the line before it (the lexer read no line break there).
fn code_ends(src: &str) -> Option<(Vec<Option<usize>>, Vec<bool>)> {
    let toks = lex(src).ok()?;
    let starts: Vec<usize> = std::iter::once(0).chain(src.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let line_of = |at: usize| starts.partition_point(|&x| x <= at) - 1;
    let mut ends = vec![None; starts.len()];
    let mut cont = vec![false; starts.len()];
    let mut prev_line = None;
    for t in toks {
        if matches!(t.tok, Tok::Newline(_)) {
            prev_line = None;
        }
        if matches!(t.tok, Tok::Newline(_) | Tok::Eof) || t.span.end <= t.span.start {
            continue;
        }
        let (s, e) = (t.span.start as usize, t.span.end as usize);
        let l = line_of(s);
        let le = starts.partition_point(|&x| x < e.max(1)) - 1;
        if prev_line.is_some_and(|p| l > p) {
            cont[l] = true;
        }
        prev_line = Some(le);
        for (i, slot) in ends.iter_mut().enumerate().take(le + 1).skip(l) {
            let line_end = starts.get(i + 1).map_or(src.len(), |&n| n - 1);
            let v = if i == le { e } else { line_end };
            *slot = Some(slot.map_or(v, |x: usize| x.max(v)));
        }
    }
    Some((ends, cont))
}

/// `a` with the continued line `b` joined on, the way the printer writes the statement on one
/// line: no space before `.name` or a closing bracket, or after an opening one, and no trailing
/// comma before a closing bracket (`sspur fmt` writes one after the last element of a broken
/// list, call or record).
fn join_continued(a: &mut String, b: &str) {
    if b.starts_with([')', ']', '}']) && a.ends_with(',') {
        a.pop();
    }
    if !((b.starts_with(['.', ')', ']', '}']) && !b.starts_with("..")) || a.ends_with(['(', '[', '{'])) {
        a.push(' ');
    }
    a.push_str(b);
}

fn indent(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

/// Whether the source is what `sspur fmt` prints, ignoring blank lines and where statements are
/// broken. A statement broken across lines (after an operator, before `.name`, inside brackets
/// with or without a trailing comma, an `if` chain with each `else` on its own line) reads as
/// the one line of the canonical text, so both what `sspur fmt` writes since ADR 0040 and the
/// one-line form of 0.13 pass. And match arms may be indented one level under their `match`
/// (with their bodies), as most SSPUR code is written, where the printer puts them at its column.
/// Comments count since `sspur fmt` keeps them (ADR 0042): each one where `sspur fmt` puts it,
/// on its own line or after code, in the same order among the statements, at any indent. A
/// source in the one-line form of 0.13 is compared without its comments.
pub fn fmt_check(src: &str, module: &Module) -> J {
    let wrapped = format_source(src).unwrap_or_else(|_| format_module(module));
    let r = fmt_against(src, &wrapped, true);
    if r["clean"] == true {
        return r;
    }
    let canon = print_module(module);
    if canon != format_module(module) && fmt_against(src, &canon, false)["clean"] == true {
        return json!({"clean": true});
    }
    r
}

/// The statements of `src`, each with its first line number: lines the lexer reads as one
/// statement joined, blank lines dropped, and comments dropped or, with `comments`, each one an
/// entry of its own after the statement whose line it ends.
fn statements(src: &str, comments: bool) -> Option<Vec<(usize, String)>> {
    let (ends, cont) = code_ends(src)?;
    let starts: Vec<usize> = std::iter::once(0).chain(src.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let mut have: Vec<(usize, String)> = Vec::new();
    for (i, e) in ends.iter().enumerate() {
        let Some(e) = *e else { continue };
        let line = src[starts[i]..e].trim_end();
        if line.trim().is_empty() {
            continue;
        }
        match have.last_mut() {
            Some((_, h)) if cont[i] => join_continued(h, line.trim_start()),
            _ => have.push((i + 1, line.to_string())),
        }
    }
    if comments {
        let (_, cs) = sspur_syntax::lexer::lex_comments(src).ok()?;
        let mut all: Vec<(usize, u8, String)> = have.into_iter().map(|(l, t)| (l, 0, t)).collect();
        for c in cs {
            let line = starts.partition_point(|&x| x <= c.span.start as usize);
            all.push((line, 1 + u8::from(c.own_line), src[c.span.start as usize..c.span.end as usize].trim_end().to_string()));
        }
        all.sort_by_key(|x| (x.0, x.1));
        have = all.into_iter().map(|x| (x.0, x.2)).collect();
    }
    Some(join_variants(have))
}

/// A sum type with each variant on its own line after `|` (how `sspur fmt` breaks a long one)
/// as the one line `type T = A | B`; comments among the variants come after it.
fn join_variants(st: Vec<(usize, String)>) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut it = st.into_iter().peekable();
    while let Some((line, mut s)) = it.next() {
        let head = s.trim_start_matches("pub ").trim_start_matches("res ");
        if !(indent(&s) == 0 && head.starts_with("type ") && s.contains("= | ")) {
            out.push((line, s));
            continue;
        }
        s = s.replacen("= | ", "= ", 1);
        let mut notes = Vec::new();
        while let Some((l, n)) = it.peek() {
            let t = n.trim_start();
            if t.starts_with("| ") && indent(n) > 0 {
                s.push(' ');
                s.push_str(t);
            } else if t.starts_with("//") {
                notes.push((*l, n.clone()));
            } else {
                break;
            }
            it.next();
        }
        out.push((line, s));
        out.extend(notes);
    }
    out
}

fn fmt_against(src: &str, want_text: &str, comments: bool) -> J {
    let Some(have) = statements(src, comments) else { return json!({"clean": false, "line": 1, "have": "", "want": ""}) };
    let want: Vec<String> = statements(want_text, comments).unwrap_or_default().into_iter().map(|x| x.1).collect();
    // The arms indented past where the printer puts them: each one's indent in the source.
    let mut arms: Vec<usize> = Vec::new();
    let mut j = 0;
    for w in &want {
        let Some((line, h)) = have.get(j).cloned() else {
            return json!({"clean": false, "line": have.last().map_or(1, |x| x.0), "have": "", "want": w});
        };
        if w.trim_start().starts_with("//") || h.trim_start().starts_with("//") {
            if h.trim_start() != w.trim_start() {
                return json!({"clean": false, "line": line, "have": h.trim_start(), "want": w.trim_start()});
            }
            j += 1;
            continue;
        }
        let mut h = h;
        let mut used = 1;
        while h.trim_start() != w.trim_start() {
            match have.get(j + used) {
                Some((_, n)) if n.trim_start().starts_with("else ") && w.trim_start().starts_with(&format!("{} {}", h.trim_start(), n.trim_start())) => {
                    h = format!("{h} {}", n.trim_start());
                    used += 1;
                }
                _ => return json!({"clean": false, "line": line, "have": h, "want": w}),
            }
        }
        let (hi, arm) = (indent(&h), h.trim_start().starts_with('|'));
        while arms.last().is_some_and(|&a| hi < a || (hi == a && !arm)) {
            arms.pop();
        }
        let expect = indent(w) + 2 * arms.len();
        if hi == expect + 2 && arm && arms.last() != Some(&hi) {
            arms.push(hi);
        } else if hi != expect {
            return json!({"clean": false, "line": line, "have": h, "want": w});
        }
        j += used;
    }
    if let Some((line, h)) = have.get(j) {
        return json!({"clean": false, "line": line, "have": h, "want": ""});
    }
    json!({"clean": true})
}

/// The `//` lines directly above line `line` (1-based), without the slashes.
fn doc_above(lines: &[&str], line: usize) -> String {
    let mut i = line.saturating_sub(1);
    let mut out = Vec::new();
    while i > 0 {
        let t = lines[i - 1].trim_start();
        let Some(rest) = t.strip_prefix("//") else { break };
        out.push(rest.strip_prefix(' ').unwrap_or(rest).trim_end());
        i -= 1;
    }
    out.reverse();
    out.join("\n").trim().to_string()
}

fn has_fn_type(t: &Ty) -> bool {
    match t {
        Ty::Fn { .. } => true,
        Ty::Named { args, .. } => args.iter().any(has_fn_type),
        Ty::Tuple(ts) => ts.iter().any(has_fn_type),
    }
}

fn names_in(e: &Expr, out: &mut BTreeSet<String>) {
    walk_expr(e, &mut |x| {
        match &x.kind {
            ExprKind::Name(n) => {
                out.insert(n.clone());
            }
            ExprKind::Method { name, .. } => {
                out.insert(name.clone());
            }
            ExprKind::Record { ctor: Some(c), .. } => {
                out.insert(c.clone());
            }
            _ => {}
        }
        true
    });
}

fn fn_exprs(f: &FnDef) -> impl Iterator<Item = &Expr> {
    f.pres.iter().chain(&f.posts).chain(&f.examples).chain(std::iter::once(&f.body))
}

fn def_exprs(d: &Def) -> Vec<&Expr> {
    match d {
        Def::Fn(f) => fn_exprs(f).collect(),
        Def::Test(t) => vec![&t.body],
        Def::Impl(i) => i.fns.iter().flat_map(fn_exprs).collect(),
        Def::Trait(t) => t.methods.iter().filter(|m| m.default).map(|m| &m.sig.body).collect(),
        _ => vec![],
    }
}

fn type_kind(t: &TypeDef) -> &'static str {
    match &t.body {
        TypeBody::Record(_) => "record",
        TypeBody::Sum(_) => "sum",
        TypeBody::Alias(_, Some(_)) => "refined",
        TypeBody::Alias(..) => "alias",
        TypeBody::New(_) => "new",
    }
}

fn type_has_fn(t: &TypeDef) -> bool {
    match &t.body {
        TypeBody::Record(fs) => fs.iter().any(|f| has_fn_type(&f.ty)),
        TypeBody::Sum(vs) => vs.iter().flat_map(|v| v.fields.iter().flatten()).any(|f| has_fn_type(&f.ty)),
        TypeBody::Alias(t, _) | TypeBody::New(t) => has_fn_type(t),
    }
}

fn def_kind(d: &Def) -> &'static str {
    match d {
        Def::Type(_) => "type",
        Def::Fn(f) if f.ext.is_some() => "extern",
        Def::Fn(_) => "fn",
        Def::Test(_) => "test",
        Def::Effect(_) => "effect",
        Def::Store(_) => "store",
        Def::Svc(_) => "svc",
        Def::Static(_) => "static",
        Def::Use(_) => "use",
        Def::Trait(_) => "trait",
        Def::Impl(_) => "impl",
    }
}

fn diag_json(src: &str, path: &str, d: &Diag) -> J {
    if d.span[0] as usize > src.len() || (d.span == [0, 0] && d.code.starts_with("E_DEP")) {
        return json!({"code": d.code, "severity": d.severity, "file": "(dependency)", "line": 0, "col": 0, "msg": d.msg, "hint": d.hint});
    }
    let (line, col) = line_col(src, d.span[0]);
    json!({"code": d.code, "severity": d.severity, "file": path, "line": line, "col": col, "msg": d.msg, "hint": d.hint})
}

/// How the facts get their diagnostics and hashes.
pub enum Check<'a> {
    /// Check the source here, on its own: a package without dependencies.
    Here,
    /// The caller checked it, linked with its dependencies.
    Given(Checked<'a>),
    /// Don't typecheck (the browser can't fetch dependencies); `checked` is false.
    Skip,
}

/// The facts of one package source.
pub fn facts(path: &str, src: &str, mode: Check) -> J {
    let module = match parse(src) {
        Ok(m) => m,
        Err(e) => {
            let d = sspur_check::syntax_diag_in(src, &e);
            return json!({"facts": FACTS_VERSION, "source": path, "parse": false, "checked": false, "profile": null, "diags": [diag_json(src, path, &d)], "fmt": {"clean": false, "line": line_col(src, d.span[0]).0, "have": "", "want": ""}, "defs": [], "holes": [], "run_cmd": [], "tests": 0, "tested": []});
        }
    };
    let own_check;
    let own_hashes: BTreeMap<String, String>;
    let none = BTreeMap::new();
    let mut lowered = None;
    let (diags, hashes): (&[Diag], &BTreeMap<String, String>) = match &mode {
        Check::Given(c) => (c.diags, c.hashes),
        Check::Skip => (&[], &none),
        Check::Here => {
            own_check = sspur_check::check(&module);
            let res = sspur_hash::Resolution { user_methods: Some(&own_check.user_methods), record_types: Some(&own_check.record_types) };
            own_hashes = sspur_hash::hash_module_with(&module, &res).into_iter().collect();
            if !own_check.has_errors() {
                lowered = sspur_check::elab::lower(&module, &own_check).err();
            }
            (&own_check.diags, &own_hashes)
        }
    };
    let mut diags_out: Vec<J> = diags.iter().filter(|d| d.code != "E_HOLE").map(|d| diag_json(src, path, d)).collect();
    if let Some(e) = lowered {
        diags_out.push(json!({"code": "E_LOWER", "severity": "error", "file": path, "line": 0, "col": 0, "msg": e, "hint": null}));
    }
    let lines: Vec<&str> = src.lines().collect();
    let mut defs = Vec::new();
    let mut holes = Vec::new();
    let mut run_cmd = Vec::new();
    let mut tests = 0;
    let mut tested: BTreeSet<String> = BTreeSet::new();
    for d in &module.defs {
        let (line, _) = line_col(src, d.span().start);
        for e in def_exprs(d) {
            walk_expr(e, &mut |x| {
                match &x.kind {
                    ExprKind::Hole(n) => holes.push(json!({"name": n.clone().map_or("?".to_string(), |n| format!("?{n}")), "line": line_col(src, x.span.start).0, "def": d.name()})),
                    ExprKind::Call(f, _) if matches!(&f.kind, ExprKind::Name(n) if n == "run_cmd") => run_cmd.push(json!({"line": line_col(src, x.span.start).0, "def": d.name()})),
                    ExprKind::Method { name, .. } if name == "run_cmd" => run_cmd.push(json!({"line": line_col(src, x.span.start).0, "def": d.name()})),
                    _ => {}
                }
                true
            });
        }
        if let Def::Test(t) = d {
            tests += 1;
            names_in(&t.body, &mut tested);
        }
        let mut j = json!({
            "kind": def_kind(d),
            "name": d.name(),
            "public": d.is_pub(),
            "line": line,
            "doc": doc_above(&lines, line),
            "hash": hashes.get(d.name()).cloned().unwrap_or_default(),
        });
        let o = j.as_object_mut().unwrap();
        match d {
            Def::Fn(f) => {
                let sig = printer::print_sig(f);
                let pres: Vec<String> = f.pres.iter().map(|p| printer::expr(p, 2)).collect();
                o.insert("sig_hash".into(), json!(short_hash(&format!("{sig}\n{}", pres.join("\n")))));
                o.insert("sig".into(), json!(sig));
                o.insert("effects".into(), json!(f.effects.iter().map(printer::effect).collect::<Vec<_>>()));
                o.insert("pres".into(), json!(pres));
                o.insert("posts".into(), json!(f.posts.len()));
                o.insert("examples".into(), json!(f.examples.len()));
                o.insert("params".into(), json!(f.params.iter().map(|p| json!({"name": p.name, "ty": printer::ty(&p.ty), "refined": p.refine.is_some()})).collect::<Vec<_>>()));
                o.insert("tparams".into(), json!(f.tparams.iter().map(|p| p.name.clone()).collect::<Vec<_>>()));
                o.insert("trusted".into(), json!(f.trusted.is_some()));
            }
            Def::Type(t) => {
                let sig = printer::print_def(d);
                o.insert("sig_hash".into(), json!(short_hash(&sig)));
                o.insert("sig".into(), json!(sig));
                o.insert("type_kind".into(), json!(type_kind(t)));
                o.insert("derives".into(), json!(t.derives));
                o.insert("has_fn_fields".into(), json!(type_has_fn(t)));
                o.insert("generic".into(), json!(!t.params.is_empty()));
                let fields: Vec<String> = match &t.body {
                    TypeBody::Record(fs) => fs.iter().map(|f| f.name.clone()).collect(),
                    TypeBody::Sum(vs) => vs.iter().flat_map(|v| v.fields.iter().flatten().map(|f| f.name.clone())).collect(),
                    _ => vec![],
                };
                o.insert("fields".into(), json!(fields));
                o.insert("variants".into(), json!(match &t.body {
                    TypeBody::Sum(vs) => vs.iter().map(|v| v.name.clone()).collect::<Vec<_>>(),
                    _ => vec![],
                }));
            }
            Def::Effect(e) => {
                let sig = printer::print_def(d);
                o.insert("sig_hash".into(), json!(short_hash(&sig)));
                o.insert("sig".into(), json!(sig));
                o.insert("ops".into(), json!(e.ops.iter().map(|op| op.name.clone()).collect::<Vec<_>>()));
            }
            Def::Trait(t) => {
                let sig = printer::print_def(d);
                o.insert("sig_hash".into(), json!(short_hash(&sig)));
                o.insert("sig".into(), json!(sig));
                o.insert("ops".into(), json!(t.methods.iter().map(|m| m.sig.name.clone()).collect::<Vec<_>>()));
            }
            Def::Impl(i) => {
                let sig = format!("impl {} for {}", i.trait_name, printer::ty(&i.target));
                o.insert("sig_hash".into(), json!(short_hash(&sig)));
                o.insert("sig".into(), json!(sig));
            }
            _ => {}
        }
        defs.push(j);
    }
    for d in &module.defs {
        if let Def::Fn(f) = d
            && !f.examples.is_empty()
        {
            tested.insert(f.name.clone());
        }
    }
    json!({
        "facts": FACTS_VERSION,
        "source": path,
        "parse": true,
        "checked": !matches!(mode, Check::Skip),
        "profile": module.profile,
        "diags": diags_out,
        "fmt": fmt_check(src, &module),
        "defs": defs,
        "holes": holes,
        "run_cmd": run_cmd,
        "tests": tests,
        "tested": tested.into_iter().collect::<Vec<_>>(),
    })
}

/// One module of a package: a path dependency inside the package's own directory, with its
/// own `sspur.toml` (as the web-app template's `models = { path = "models" }`). The package's
/// root is the module with the key "" and the directory "".
#[derive(Clone, Debug, PartialEq)]
pub struct PkgModule {
    /// The dependency key it is used by (its name in its sspur.toml).
    pub key: String,
    /// Its directory relative to the repository root, without a trailing slash.
    pub dir: String,
    pub manifest: String,
    /// Its source file relative to the repository root.
    pub src_path: String,
}

/// `a/b` joined with `../c`, lexically; `None` if it leaves the root.
pub fn join_rel(dir: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for p in path.split(['/', '\\']) {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            x => parts.push(x),
        }
    }
    Some(parts.join("/"))
}

fn in_dir(dir: &str, f: &str) -> String {
    if dir.is_empty() { f.to_string() } else { format!("{dir}/{f}") }
}

/// The source file of the module in `dir`: `src` from its manifest, else lib.ssp or main.ssp.
pub fn module_src(dir: &str, manifest: &J, exists: &dyn Fn(&str) -> bool) -> String {
    match manifest["src"].as_str().filter(|s| !s.is_empty()) {
        Some(s) => join_rel(dir, s).unwrap_or_else(|| in_dir(dir, s)),
        None => ["lib.ssp", "main.ssp"].iter().map(|f| in_dir(dir, f)).find(|p| exists(p)).unwrap_or_else(|| in_dir(dir, "lib.ssp")),
    }
}

/// The package's modules, root first and then in the order they are reached, read with
/// `read(path)` (paths relative to the repository root), and the errors of path dependencies
/// that are not modules: outside the package's directory, or without a sspur.toml.
pub fn modules(root_manifest: &str, read: &dyn Fn(&str) -> Option<String>) -> (Vec<PkgModule>, Vec<J>) {
    let exists = |p: &str| read(p).is_some();
    let root = PkgModule { key: String::new(), dir: String::new(), manifest: root_manifest.to_string(), src_path: module_src("", &manifest(root_manifest), &exists) };
    let mut out = vec![root];
    let mut errors = Vec::new();
    let mut i = 0;
    while i < out.len() {
        let (dir, mt) = (out[i].dir.clone(), manifest(&out[i].manifest));
        let at = in_dir(&dir, "sspur.toml");
        for d in mt["deps"].as_array().into_iter().flatten().filter(|d| d["kind"] == "path") {
            let key = d["name"].as_str().unwrap_or_default();
            let path = d["req"].as_str().unwrap_or_default();
            let err = |msg: String| json!({"code": "E_MODULE", "severity": "error", "file": at, "line": 0, "col": 0, "msg": msg, "hint": null});
            let Some(target) = join_rel(&dir, path).filter(|t| !t.is_empty()) else {
                errors.push(err(format!("dependency {key} = {{ path = \"{path}\" }} is outside the package's directory")));
                continue;
            };
            if out.iter().any(|m| m.dir == target) {
                continue;
            }
            match read(&in_dir(&target, "sspur.toml")) {
                Some(text) => {
                    let mm = manifest(&text);
                    if mm["name"].as_str() != Some(key) {
                        errors.push(err(format!("the module at {target} is named {}, not {key}", mm["name"].as_str().unwrap_or("nothing"))));
                        continue;
                    }
                    let src_path = module_src(&target, &mm, &exists);
                    out.push(PkgModule { key: key.to_string(), dir: target, manifest: text, src_path });
                }
                None => errors.push(err(format!("dependency {key} = {{ path = \"{path}\" }}: {target} has no sspur.toml"))),
            }
        }
        i += 1;
    }
    (out, errors)
}

/// The manifest as the checks see it: the known fields, every key as `section.key`, and the
/// dependencies with their kind (`registry`, `path` or `git`).
pub fn manifest(text: &str) -> J {
    use crate::toml::{parse_toml, TVal};
    let secs = match parse_toml(text) {
        Ok(s) => s,
        Err(e) => return json!({"error": e, "name": "", "version": "", "src": "", "description": "", "license": "", "repository": "", "keywords": [], "keys": [], "deps": []}),
    };
    let mut get = BTreeMap::new();
    let mut keys = Vec::new();
    let mut deps = Vec::new();
    let mut keywords: Vec<String> = Vec::new();
    for s in &secs {
        let sec = if s.name.is_empty() { "package" } else { s.name.as_str() };
        if s.kv.is_empty() && !s.name.is_empty() {
            keys.push(format!("{sec}."));
        }
        for (k, v) in &s.kv {
            keys.push(format!("{}{sec}.{k}", if s.array { "[]" } else { "" }));
            match (sec, v) {
                ("package", TVal::Str(x)) => {
                    get.insert(k.clone(), x.clone());
                }
                ("package", TVal::Arr(xs)) if k == "keywords" => keywords = xs.clone(),
                ("deps" | "dependencies", v) => {
                    let (kind, req) = match v {
                        TVal::Str(p) => ("path", p.clone()),
                        TVal::Table(t) if t.contains_key("version") => ("registry", t.get("version").and_then(TVal::str).unwrap_or_default().to_string()),
                        TVal::Table(t) if t.contains_key("git") => ("git", t.get("git").and_then(TVal::str).unwrap_or_default().to_string()),
                        TVal::Table(t) => ("path", t.get("path").and_then(TVal::str).unwrap_or_default().to_string()),
                        TVal::Arr(_) => ("invalid", String::new()),
                    };
                    let package = match v {
                        TVal::Table(t) => t.get("package").and_then(TVal::str).unwrap_or(k).to_string(),
                        _ => k.clone(),
                    };
                    let mut d = json!({"name": k, "kind": kind, "req": req, "package": package});
                    if matches!(v, TVal::Table(t) if t.get("public").and_then(TVal::str) == Some("true")) {
                        d["public"] = json!(true);
                    }
                    deps.push(d);
                }
                _ => {}
            }
        }
    }
    let f = |k: &str| get.get(k).cloned().unwrap_or_default();
    json!({"name": f("name"), "version": f("version"), "src": f("src"), "description": f("description"), "license": f("license"), "repository": f("repository"), "keywords": keywords, "keys": keys, "deps": deps})
}

/// What the checks see of one file of the package: its size, whether it is text, its first
/// bytes in hex (for the binary check), and its text when it is text and at most 1 MiB.
pub fn file_info(path: &str, data: &[u8], mode: &str) -> J {
    let head: String = data.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let text = std::str::from_utf8(data).ok().filter(|t| !t[..t.len().min(8000)].contains('\0'));
    json!({"path": path, "size": data.len(), "binary": text.is_none(), "head": head, "mode": mode, "text": text.filter(|t| t.len() <= 1 << 20)})
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "// Money in cents.\npub type Money = {cents: Int} derive Eq, Show\n\n// Adds two amounts.\n// Never overflows in practice.\npub fn add(a: Money, b: Money) -> Money\n= Money{cents: a.cents + b.cents}  // trailing\n\n// HTTP\n\npub fn half(n: Int where _ > 0) -> Int ! log\n= do\n  log(\"half {n}\")\n  n / 2\n\nfn shell() -> Res[(Int, Str, Str), Str] ! proc\n= run_cmd(\"ls\", [], \"\")\n\nfn todo_later(x: Int) -> Int = ?later\n\ntest t_add = add(Money{cents: 1}, Money{cents: 2}) == Money{cents: 3}\n";

    #[test]
    fn facts_of_a_small_package() {
        let f = facts("lib.ssp", SRC, Check::Here);
        let defs = f["defs"].as_array().unwrap();
        let get = |n: &str| defs.iter().find(|d| d["name"] == n).unwrap().clone();
        assert_eq!(get("Money")["doc"], "Money in cents.");
        assert_eq!(get("Money")["line"], 2);
        assert_eq!(get("Money")["derives"], json!(["Eq", "Show"]));
        assert_eq!(get("add")["doc"], "Adds two amounts.\nNever overflows in practice.");
        assert_eq!(get("add")["public"], true);
        assert_eq!(get("half")["doc"], "", "a section comment followed by a blank line is not a doc");
        assert_eq!(get("half")["effects"], json!(["log"]));
        assert_eq!(get("half")["params"][0]["refined"], true);
        assert_eq!(get("shell")["effects"], json!(["proc"]));
        assert_eq!(f["run_cmd"][0]["line"], 17);
        assert_eq!(f["holes"][0]["name"], "?later");
        assert_eq!(f["holes"][0]["line"], 19);
        assert_eq!(f["tests"], 1);
        assert!(f["tested"].as_array().unwrap().contains(&json!("add")));
        assert!(get("add")["hash"].as_str().unwrap().len() > 10);
        assert_eq!(get("add")["sig"], "fn add(a: Money, b: Money) -> Money");
    }

    #[test]
    fn fmt_counts_comments_and_layout() {
        let ok = "// a comment\nfn f(x: Int) -> Int\n= x + 1  // why\n\n\n// section\n\ntest t = f(1) == 2\n";
        assert_eq!(fmt_check(ok, &parse(ok).unwrap())["clean"], true);
        let inside = "fn f() -> List[Int]\n= [\n    1, // one\n    // two next\n    2,\n  ]\n";
        assert_eq!(fmt_check(inside, &parse(inside).unwrap())["clean"], true);
        let late = "fn f(x: Int) -> Int\n= x+1 // why\n";
        let r = fmt_check(late, &parse(late).unwrap());
        assert_eq!((r["line"].clone(), r["want"].clone()), (json!(2), json!("= x + 1")));
        let bad = "fn f(x: Int) -> Int = x+1\n";
        let r = fmt_check(bad, &parse(bad).unwrap());
        assert_eq!(r["clean"], false);
        assert_eq!(r["line"], 1);
        assert_eq!(r["want"], "fn f(x: Int) -> Int");
        let url = "fn u() -> Str\n= \"https://example.com/a\"\n";
        assert_eq!(fmt_check(url, &parse(url).unwrap())["clean"], true, "// inside a string is not a comment");
    }

    #[test]
    fn fmt_ignores_where_a_statement_is_broken() {
        let ok = "fn f(xs: List[Int], r: R) -> Bool\n= xs.len > 0 and\n    r.ok  // why\n\nfn g(xs: List[Int]) -> List[Int]\n= xs\n    .map(_ + 1)\n    .filter(_ > 2)\n\nfn h(a: Int, b: Int) -> P\n= P{\n    a,\n    b: f(\n      [a], b)\n  }\n";
        assert_eq!(fmt_check(ok, &parse(ok).unwrap()), json!({"clean": true}));
        let bad = "fn f(a: Int, b: Int) -> Int\n= a+\n    b\n";
        let r = fmt_check(bad, &parse(bad).unwrap());
        assert_eq!((r["line"].clone(), r["have"].clone(), r["want"].clone()), (json!(2), json!("= a+ b"), json!("= a + b")));
        let arms = "fn f(x: Opt[Int], y: Int) -> Int\n= match x\n  | some(v) => do\n    w = v + 1\n    match y\n      | 0 => w\n      | _ => if w > 2 then 1\n        else if w > 1 then 2\n        else 3\n  | none => 0\n\nfn g(x: Int) -> Int\n= do\n  r = match x\n    | 0 => 1\n    | _ => 2\n  r + 1\n";
        assert_eq!(fmt_check(arms, &parse(arms).unwrap()), json!({"clean": true}));
        let printed = print_module(&parse(arms).unwrap());
        assert_eq!(fmt_check(&printed, &parse(&printed).unwrap()), json!({"clean": true}));
        let odd = "fn f(x: Int) -> Int\n= match x\n   | 0 => 1\n   | _ => 2\n";
        assert_eq!(fmt_check(odd, &parse(odd).unwrap())["line"], 3);
        let long = "fn f(xs: List[Int]) -> Int\n= xs.filter(x => x > 1000000).map(x => x * 1000 + 17).filter(x => x % 3 == 0).map(x => x / 7).filter(x => x > 0).sum\n\ntest t = f([10000000, 20000000, 30000000, 40000000, 50000000, 60000000, 70000000, 80000000]) > 0 and f([]) == 0\n";
        let wrapped = sspur_syntax::format_module(&parse(long).unwrap());
        assert!(wrapped.lines().count() > 6, "{wrapped}");
        assert_eq!(fmt_check(&wrapped, &parse(&wrapped).unwrap()), json!({"clean": true}), "{wrapped}");
        assert_eq!(fmt_check(long, &parse(long).unwrap()), json!({"clean": true}), "the one-line form of 0.13");
        let short = "fn h(a: Int) -> P\n= P{a: a, b: 1}\n";
        assert_eq!(fmt_check(short, &parse(short).unwrap())["want"], "= P{a, b: 1}");
    }

    #[test]
    fn manifests_and_files() {
        let m = manifest("[package]\nname = \"money\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\nkeywords = [\"cash\"]\nbuild = \"make\"\n\n[deps]\ntext = { version = \"^0.2\" }\nlocal = { path = \"../l\" }\n");
        assert_eq!(m["name"], "money");
        assert_eq!(m["keywords"], json!(["cash"]));
        assert!(m["keys"].as_array().unwrap().contains(&json!("package.build")));
        assert_eq!(m["deps"][1], json!({"name": "text", "kind": "registry", "req": "^0.2", "package": "text"}));
        assert_eq!(m["deps"][0]["kind"], "path");
        let open = manifest("[package]\nname = \"big\"\n\n[deps]\nmodels = { path = \"models\", public = true }\n");
        assert_eq!(open["deps"][0], json!({"name": "models", "kind": "path", "req": "models", "package": "models", "public": true}));
        let f = file_info("a.bin", &[0x7f, b'E', b'L', b'F', 0, 1], "100644");
        assert_eq!(f["binary"], true);
        assert_eq!(f["head"], "7f454c460001");
        assert_eq!(file_info("a.txt", b"hi", "100644")["text"], "hi");
    }

    #[test]
    fn modules_are_path_dependencies_inside_the_package() {
        let files: BTreeMap<&str, &str> = [
            ("sspur.toml", "[package]\nname = \"acme/big\"\nversion = \"1.0.0\"\n\n[deps]\nmodels = { path = \"models\" }\nviews = { path = \"./src/views\" }\nup = { path = \"../up\" }\ntext = { package = \"acme/text\", version = \"^1\" }\n"),
            ("lib.ssp", ""),
            ("models/sspur.toml", "[package]\nname = \"models\"\nsrc = \"m.ssp\"\n"),
            ("models/m.ssp", ""),
            ("src/views/sspur.toml", "[package]\nname = \"views\"\n\n[deps]\nmodels = { path = \"../../models\" }\n"),
            ("src/views/main.ssp", ""),
        ]
        .into_iter()
        .collect();
        let (ms, errs) = modules(files["sspur.toml"], &|p| files.get(p).map(|s| s.to_string()));
        let got: Vec<(&str, &str, &str)> = ms.iter().map(|m| (m.key.as_str(), m.dir.as_str(), m.src_path.as_str())).collect();
        assert_eq!(got, vec![("", "", "lib.ssp"), ("models", "models", "models/m.ssp"), ("views", "src/views", "src/views/main.ssp")]);
        assert_eq!(errs.len(), 1);
        assert!(errs[0]["msg"].as_str().unwrap().contains("outside the package's directory"));
        assert_eq!(manifest(files["sspur.toml"])["deps"][1]["package"], "acme/text");
        assert_eq!(join_rel("a/b", "../c"), Some("a/c".into()));
        assert_eq!(join_rel("", "../c"), None);
    }

    #[test]
    fn parse_errors_are_facts() {
        let f = facts("lib.ssp", "fn f( -> Int = 1\n", Check::Here);
        assert_eq!(f["parse"], false);
        assert_eq!(f["diags"][0]["line"], 1);
    }
}
