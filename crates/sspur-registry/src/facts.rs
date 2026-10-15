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
/// blank. `None` for a line with no code.
fn code_ends(src: &str) -> Option<Vec<Option<usize>>> {
    let toks = lex(src).ok()?;
    let starts: Vec<usize> = std::iter::once(0).chain(src.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let mut ends = vec![None; starts.len()];
    for t in toks {
        if matches!(t.tok, Tok::Newline(_) | Tok::Eof) || t.span.end <= t.span.start {
            continue;
        }
        let (s, e) = (t.span.start as usize, t.span.end as usize);
        let l = starts.partition_point(|&x| x <= s) - 1;
        let le = starts.partition_point(|&x| x < e.max(1)) - 1;
        for (i, slot) in ends.iter_mut().enumerate().take(le + 1).skip(l) {
            let line_end = starts.get(i + 1).map_or(src.len(), |&n| n - 1);
            let v = if i == le { e } else { line_end };
            *slot = Some(slot.map_or(v, |x: usize| x.max(v)));
        }
    }
    Some(ends)
}

/// Whether the source is what `sspur fmt` prints, ignoring comments and blank lines. The
/// formatter drops comments, so they can't count against a package.
pub fn fmt_check(src: &str, module: &Module) -> J {
    let want_text = print_module(module);
    let Some(ends) = code_ends(src) else { return json!({"clean": false, "line": 1, "have": "", "want": ""}) };
    let starts: Vec<usize> = std::iter::once(0).chain(src.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let have: Vec<(usize, &str)> = ends.iter().enumerate().filter_map(|(i, e)| e.map(|e| (i + 1, src[starts[i]..e].trim_end()))).filter(|(_, l)| !l.trim().is_empty()).collect();
    let want: Vec<&str> = want_text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    for (k, w) in want.iter().enumerate() {
        match have.get(k) {
            Some((_, h)) if h == w => {}
            Some((line, h)) => return json!({"clean": false, "line": line, "have": h, "want": w}),
            None => return json!({"clean": false, "line": have.last().map_or(1, |x| x.0), "have": "", "want": w}),
        }
    }
    if let Some((line, h)) = have.get(want.len()) {
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
            return json!({"facts": FACTS_VERSION, "source": path, "parse": false, "checked": false, "diags": [diag_json(src, path, &d)], "fmt": {"clean": false}, "defs": [], "holes": [], "run_cmd": [], "tests": 0});
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
            "pub": d.is_pub(),
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
        assert_eq!(get("add")["pub"], true);
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
    fn fmt_ignores_comments_but_not_layout() {
        let ok = "// a comment\nfn f(x: Int) -> Int\n= x + 1  // why\n\n\n// section\n\ntest t = f(1) == 2\n";
        assert_eq!(fmt_check(ok, &parse(ok).unwrap())["clean"], true);
        let bad = "fn f(x: Int) -> Int = x+1\n";
        let r = fmt_check(bad, &parse(bad).unwrap());
        assert_eq!(r["clean"], false);
        assert_eq!(r["line"], 1);
        assert_eq!(r["want"], "fn f(x: Int) -> Int");
        let url = "fn u() -> Str\n= \"https://example.com/a\"\n";
        assert_eq!(fmt_check(url, &parse(url).unwrap())["clean"], true, "// inside a string is not a comment");
    }

    #[test]
    fn parse_errors_are_facts() {
        let f = facts("lib.ssp", "fn f( -> Int = 1\n", Check::Here);
        assert_eq!(f["parse"], false);
        assert_eq!(f["diags"][0]["line"], 1);
    }
}
