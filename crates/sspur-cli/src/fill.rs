//! `sspur fill` and `sspur suggest`, and the hole candidates `sspur check` adds to E_HOLE (ADR 0030).
use crate::{agent, exec_or, Args};
use serde_json::{json, Value as Json};
use sspur_check::Diag;
use sspur_eval::{suggest, synth, Interp};
use sspur_smt::{Answer, Solver};
use sspur_store::{pkg, Loaded, Store};
use sspur_syntax::{printer, Def, Expr, Module, Span};
use std::process::ExitCode;

/// A hole's span, the candidate's text and its expression.
type Edit = (Span, String, Expr);

/// The interpreter the filler runs candidates in: no ownership moves, since a hole is not a use.
fn plain_interp(l: &Loaded) -> Interp {
    let mut tmp = None;
    let l = exec_or(l, &mut tmp);
    let mut it = Interp::new(&l.module, l.check.record_types.clone(), l.check.user_methods.clone(), l.check.gen_loops.clone());
    it.foreign = l.module.defs[l.own..].iter().map(|d| d.name().to_string()).collect();
    it.set_check(&l.check);
    it.float_sums = l
        .check
        .expr_types
        .iter()
        .filter(|((_, _, tag), t)| matches!(tag, 4 | 5) && matches!(t, sspur_check::Type::Con(n, a) if n == "F64" && a.is_empty()))
        .map(|((s, e, _), _)| (*s, *e))
        .collect();
    it
}

pub fn fill_holes(l: &Loaded, opts: &synth::Opts, only: Option<&str>) -> Vec<synth::Filled> {
    let (check, ctx) = sspur_check::check_holes(&l.module);
    if ctx.holes.is_empty() || check.has_errors() {
        return vec![];
    }
    let it = plain_interp(l);
    synth::fill(&l.module, &check, &ctx, &it, opts, only)
}

/// True when Z3 proves every `post` of `def` in `m`.
fn posts_proved(m: &Module, def: &str, solver: &mut Solver) -> bool {
    let check = sspur_check::check(m);
    if check.has_errors() {
        return false;
    }
    let n = m.defs.iter().find_map(|d| match d {
        Def::Fn(f) if f.name == def => Some(f.posts.len()),
        _ => None,
    });
    let Some(n) = n.filter(|n| *n > 0) else { return false };
    let an = sspur_smt::analyze(m, &check);
    (0..n).all(|i| {
        an.post_clause(def, i).is_some_and(|c| !c.obligations.is_empty() && c.obligations.iter().all(|o| o.query.as_ref().is_some_and(|q| solver.check(&an.script(q), &[]) == Answer::Unsat)))
    })
}

fn fill_op(f: &synth::Filled, c: &synth::Cand, m: &Module) -> Json {
    let mut op = json!({"op": "fill", "hole": f.site.label(), "expr": c.text, "evidence": c.evidence(), "ok": c.ok()});
    if m.defs.iter().any(|d| d.name() == f.site.def) {
        op["target"] = json!(f.site.def);
    }
    op
}

/// Adds the ranked candidates of each hole to its E_HOLE diagnostic as `fill` fixes.
pub fn enrich(l: &Loaded, diags: &mut [Diag], args: &Args) {
    if args.has("--no-fill") || !diags.iter().any(|d| d.code == "E_HOLE") || diags.iter().any(Diag::is_error) {
        return;
    }
    let opts = synth::Opts { budget_ms: args.num("--fill-ms", 200) as u64, ..Default::default() };
    for f in fill_holes(l, &opts, None) {
        if let Some(d) = diags.iter_mut().find(|d| d.code == "E_HOLE" && d.span == [f.site.span.start, f.site.span.end]) {
            d.fix = f.cands.iter().map(|c| fill_op(&f, c, &l.module)).collect();
        }
    }
}

/// The candidate lines of a diagnostic's `fill` fixes, for text output.
pub fn fix_lines(d: &Diag) -> Vec<String> {
    d.fix.iter().filter(|o| o["op"] == "fill" && o.get("evidence").is_some()).map(|o| format!("  {}: {} ({})", o["hole"].as_str().unwrap_or("?"), o["expr"].as_str().unwrap_or(""), o["evidence"].as_str().unwrap_or(""))).collect()
}

enum Target {
    File(String, String),
    Head(Store),
}

fn load(args: &Args) -> Result<(Target, Loaded, Vec<String>), ExitCode> {
    let rest: Vec<String> = args.pos[1..].to_vec();
    let (file, rest) = match rest.first() {
        Some(p) if p.ends_with(".ssp") || std::path::Path::new(p).is_file() => (Some(p.clone()), rest[1..].to_vec()),
        _ => (None, rest),
    };
    match file {
        Some(path) => {
            let src = std::fs::read_to_string(&path).map_err(|e| {
                eprintln!("cannot read {path}: {e}");
                ExitCode::from(2)
            })?;
            let env = crate::file_env(&path).map_err(|e| {
                eprintln!("{e}");
                ExitCode::FAILURE
            })?;
            match pkg::load(src.clone(), &env, false) {
                Ok(l) => Ok((Target::File(path, src), l, rest)),
                Err(d) => Err(crate::fail_diags(&src, &path, &d, args.has("--json"))),
            }
        }
        None => {
            let Some(store) = crate::cwd_store() else { return Err(crate::no_store()) };
            match store.load_head() {
                Ok(l) => Ok((Target::Head(store), l, rest)),
                Err(d) => Err(crate::fail_diags("", "HEAD", &d, args.has("--json"))),
            }
        }
    }
}

/// Replaces each hole's text with its candidate, in parentheses where the bare text would parse differently.
fn apply_text(src: &str, mut edits: Vec<Edit>) -> Result<String, String> {
    edits.sort_by_key(|e| std::cmp::Reverse(e.0.start));
    let mut out = src.to_string();
    for (span, text, expr) in edits {
        let m = sspur_syntax::parse(&out).map_err(|e| e.msg.clone())?;
        let want = sspur_syntax::print_module(&synth::substitute(&m, span, &expr));
        let (a, b) = (span.start as usize, span.end as usize);
        let plain = format!("{}{text}{}", &out[..a], &out[b..]);
        out = match sspur_syntax::parse(&plain) {
            Ok(m2) if sspur_syntax::print_module(&m2) == want => plain,
            _ => format!("{}({text}){}", &out[..a], &out[b..]),
        };
    }
    Ok(out)
}

fn write_file(path: &str, text: &str) -> bool {
    match std::fs::write(path, text) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("cannot write {path}: {e}");
            false
        }
    }
}

/// Fills the holes and returns the text to print, whether every hole got a passing candidate,
/// and the `fill` ops of the best candidates.
pub fn fill_report(l: &Loaded, opts: &synth::Opts, only: Option<&str>, all: bool, json_out: bool) -> (String, bool, Vec<Edit>, Vec<Json>) {
    let filled = fill_holes(l, opts, only);
    let mut solver: Option<Solver> = None;
    let mut lines = Vec::new();
    let mut holes = Vec::new();
    let mut edits = Vec::new();
    let mut ops = Vec::new();
    let mut good = !filled.is_empty();
    for mut f in filled {
        if let Some(c) = f.cands.first_mut().filter(|c| c.ok() && c.contract) {
            let z3 = solver.get_or_insert_with(Solver::new);
            if z3.available() {
                c.proved = posts_proved(&synth::substitute(&l.module, f.site.span, &c.expr), &f.site.def, z3);
            }
        }
        match f.cands.first() {
            Some(best) if best.ok() => {
                edits.push((f.site.span, best.text.clone(), best.expr.clone()));
                ops.push(fill_op(&f, best, &l.module));
            }
            _ => good = false,
        }
        if f.cands.is_empty() {
            lines.push(format!("{}: no candidate ({} expressions in {:.0} ms)", f.site.label(), f.explored, f.ms));
        } else {
            let shown = f.lines();
            lines.extend(if all { shown } else { shown.into_iter().take(1).collect() });
        }
        if json_out {
            holes.push(json!({
                "hole": f.site.label(), "def": f.site.def, "type": f.site.ty.to_string(), "ms": (f.ms * 10.0).round() / 10.0,
                "explored": f.explored, "classes": f.classes, "tests": f.tests, "inputs": f.inputs, "timed_out": f.timed_out,
                "candidates": f.cands.iter().map(|c| json!({"expr": c.text, "size": c.size, "evidence": c.evidence(), "ok": c.ok()})).collect::<Vec<_>>(),
                "fix": f.cands.iter().map(|c| fill_op(&f, c, &l.module)).collect::<Vec<_>>(),
            }));
        }
    }
    if lines.is_empty() {
        lines.push(match only {
            Some(h) => format!("no hole {h}"),
            None => "no holes".into(),
        });
    }
    let text = if json_out { serde_json::to_string(&json!({"holes": holes})).unwrap() } else { lines.join("\n") };
    (text, good, edits, ops)
}

pub fn fill_cmd(args: &Args) -> ExitCode {
    let (target, l, rest) = match load(args) {
        Ok(x) => x,
        Err(c) => return c,
    };
    if l.check.has_errors() {
        let (src, label) = match &target {
            Target::File(p, s) => (s.clone(), p.clone()),
            Target::Head(_) => (l.src.clone(), "HEAD".into()),
        };
        return crate::fail_diags(&src, &label, &l.check.diags, args.has("--json"));
    }
    let opts = synth::Opts { budget_ms: args.num("--fill-ms", 1000) as u64, ..Default::default() };
    let (text, good, edits, ops) = fill_report(&l, &opts, rest.first().map(String::as_str), args.has("--all"), args.has("--json"));
    println!("{text}");
    if !args.has("--apply") || edits.is_empty() {
        return if good { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    match target {
        Target::File(path, src) => match apply_text(&src, edits) {
            Ok(out) if write_file(&path, &out) => {
                if !args.has("--json") {
                    println!("filled {} hole{} in {path}", ops.len(), if ops.len() == 1 { "" } else { "s" });
                }
                if good { ExitCode::SUCCESS } else { ExitCode::FAILURE }
            }
            Ok(_) => ExitCode::FAILURE,
            Err(e) => {
                eprintln!("cannot apply: {e}");
                ExitCode::FAILURE
            }
        },
        Target::Head(store) => {
            let (t, ok) = agent::run_edit(&store, ops, "fill", None);
            println!("{t}");
            if ok && good { ExitCode::SUCCESS } else { ExitCode::FAILURE }
        }
    }
}

/// Suggested contracts for `only` (or every function), with the ones Z3 proves marked.
pub fn suggestions(l: &Loaded, only: Option<&str>, cases: usize) -> Vec<suggest::FnSuggest> {
    let it = plain_interp(l);
    let mut out = it.suggest(only, &suggest::Opts { cases, ..Default::default() });
    let mut solver = Solver::new();
    if out.iter().any(|s| s.sugg.iter().any(|x| x.kind == "post")) && solver.available() {
        for s in &mut out {
            let Some(f) = l.module.defs.iter().find_map(|d| match d {
                Def::Fn(f) if f.name == s.name => Some(f.clone()),
                _ => None,
            }) else {
                continue;
            };
            let pres: Vec<Expr> = s.sugg.iter().filter(|x| x.kind == "pre").map(|x| x.expr.clone()).collect();
            for x in s.sugg.iter_mut().filter(|x| x.kind == "post") {
                let mut g = f.clone();
                g.pres.extend(pres.iter().cloned());
                g.posts = vec![x.expr.clone()];
                let m = with_fn(&l.module, g);
                x.proved = posts_proved(&m, &s.name, &mut solver);
            }
            s.sugg.sort_by_key(|x| (x.kind == "post", !x.proved));
        }
    }
    out
}

fn with_fn(m: &Module, f: sspur_syntax::FnDef) -> Module {
    let mut m = m.clone();
    for d in m.defs.iter_mut() {
        if let Def::Fn(g) = d
            && g.name == f.name
        {
            *g = f;
            break;
        }
    }
    m
}

fn refine_op(f: &str, s: &suggest::Suggestion) -> Json {
    json!({"op": "refine", "target": f, "contract": {s.kind: s.text}, "evidence": s.note()})
}

pub fn suggest_report(l: &Loaded, only: Option<&str>, cases: usize, json_out: bool) -> (String, Vec<suggest::FnSuggest>) {
    let out = suggestions(l, only, cases);
    if json_out {
        let fns: Vec<Json> = out
            .iter()
            .filter(|s| !s.sugg.is_empty() || only.is_some())
            .map(|s| {
                let mut o = json!({
                    "fn": l.show(&s.name), "runs": s.runs, "traps": s.traps,
                    "suggestions": s.sugg.iter().map(|x| json!({"kind": x.kind, "expr": x.text, "proved": x.proved, "note": x.note()})).collect::<Vec<_>>(),
                    "fix": s.sugg.iter().map(|x| refine_op(&s.name, x)).collect::<Vec<_>>(),
                });
                if let Some(why) = &s.skipped {
                    o["skipped"] = json!(why);
                }
                o
            })
            .collect();
        return (serde_json::to_string(&json!({"fns": fns})).unwrap(), out);
    }
    let mut lines = Vec::new();
    for s in &out {
        if let Some(why) = &s.skipped {
            if only.is_some() {
                lines.push(format!("{}: skipped ({why})", l.show(&s.name)));
            }
            continue;
        }
        if s.sugg.is_empty() {
            if only.is_some() {
                lines.push(format!("{}: nothing held on all {} runs", l.show(&s.name), s.runs));
            }
            continue;
        }
        lines.push(l.show(&s.name));
        for x in &s.sugg {
            lines.push(format!("  {} {}  // {}", x.kind, x.text, x.note()));
        }
    }
    if lines.is_empty() {
        lines.push("no suggestions".into());
    }
    (lines.join("\n"), out)
}

pub fn suggest_cmd(args: &Args) -> ExitCode {
    let (target, l, rest) = match load(args) {
        Ok(x) => x,
        Err(c) => return c,
    };
    if l.check.has_errors() {
        return crate::fail_diags(&l.src, "HEAD", &l.check.diags, args.has("--json"));
    }
    let only = rest.first().map(String::as_str);
    let (text, out) = suggest_report(&l, only, args.num("--cases", 200), args.has("--json"));
    println!("{text}");
    if !args.has("--apply") {
        return ExitCode::SUCCESS;
    }
    let ops: Vec<Json> = out.iter().flat_map(|s| s.sugg.iter().map(|x| refine_op(&s.name, x))).collect();
    if ops.is_empty() {
        return ExitCode::SUCCESS;
    }
    match target {
        Target::File(path, src) => {
            let mut text = src.clone();
            let mut m = match sspur_syntax::parse(&src) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("cannot apply: {}", e.msg);
                    return ExitCode::FAILURE;
                }
            };
            let mut defs: Vec<(Span, String)> = Vec::new();
            for d in m.defs.iter_mut() {
                let Def::Fn(f) = d else { continue };
                let Some(s) = out.iter().find(|s| s.name == f.name && !s.sugg.is_empty()) else { continue };
                for x in &s.sugg {
                    if x.kind == "pre" { f.pres.push(x.expr.clone()) } else { f.posts.push(x.expr.clone()) }
                }
                defs.push((f.span, printer::print_def(d)));
            }
            defs.sort_by_key(|d| std::cmp::Reverse(d.0.start));
            for (span, def) in defs {
                let end = (span.end as usize).min(text.len());
                text = format!("{}{}{}", &text[..span.start as usize], def.trim_end(), &text[end..]);
            }
            if sspur_syntax::parse(&text).map(|m| sspur_check::check(&m).has_errors()).unwrap_or(true) {
                eprintln!("cannot apply: the file would not typecheck");
                return ExitCode::FAILURE;
            }
            if !write_file(&path, &text) {
                return ExitCode::FAILURE;
            }
            println!("added {} clause{} to {path}", ops.len(), if ops.len() == 1 { "" } else { "s" });
            ExitCode::SUCCESS
        }
        Target::Head(store) => {
            let (t, ok) = agent::run_edit(&store, ops, "suggest", None);
            println!("{t}");
            if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
        }
    }
}
