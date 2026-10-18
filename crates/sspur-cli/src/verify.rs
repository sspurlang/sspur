use serde_json::json;
use sspur_eval::{Ctrl, Value};
use sspur_smt::{Analysis, Answer, Clause, ClauseKind, Query, Solver};
use sspur_store::Loaded;
use sspur_syntax::{line_col, Def, FnDef};
use std::collections::HashMap;
use std::process::ExitCode;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Status {
    Proved,
    Unknown,
    Counterexample,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Status::Proved => "proved",
            Status::Unknown => "unknown",
            Status::Counterexample => "counterexample",
        }
    }
}

struct Ctx<'a> {
    l: &'a Loaded,
    an: &'a Analysis,
    it: sspur_eval::Interp,
    tests: Option<HashMap<String, Result<(), String>>>,
}

fn label(c: &Clause) -> String {
    match &c.kind {
        ClauseKind::Where(p) => format!("{p} where {}", c.text),
        ClauseKind::Alias(t, p) => format!("{p}: {t} ({})", c.text),
        ClauseKind::Pre => format!("pre {}", c.text),
        ClauseKind::Post => format!("post {}", c.text),
        ClauseKind::Ret(t) => format!("result: {t} ({})", c.text),
    }
}

fn expected(c: &Clause) -> String {
    match &c.kind {
        ClauseKind::Where(p) => format!("parameter '{p}' of {} where {}", c.func, c.text),
        ClauseKind::Alias(t, p) => format!("type {t} in parameter '{p}' of {}", c.func),
        ClauseKind::Pre => format!("pre {} in {}", c.text, c.func),
        ClauseKind::Post => format!("post {} in {}", c.text, c.func),
        ClauseKind::Ret(t) => format!("type {t} in result of {}", c.func),
    }
}

impl Ctx<'_> {
    fn fn_def(&self, name: &str) -> Option<&FnDef> {
        self.l.module.defs.iter().find_map(|d| match d {
            Def::Fn(f) if f.name == name => Some(f),
            _ => None,
        })
    }

    fn confirm(&mut self, q: &Query, model: &[(String, String)], c: &Clause) -> Option<String> {
        let want = expected(c);
        let caller = &self.an.fns[q.fun];
        if caller.is_test {
            let name = caller.name.clone();
            let tests = self.tests.get_or_insert_with(|| {
                self.it.fuel.set(5_000_000);
                let r = self.it.run_tests().into_iter().collect();
                self.it.fuel.set(u64::MAX);
                r
            });
            return match tests.get(&name) {
                Some(Err(m)) if m.contains(&want) => Some(format!("test {name} fails: {m}")),
                _ => None,
            };
        }
        let f = self.fn_def(&caller.name)?.clone();
        let mut args = Vec::new();
        let mut shown = Vec::new();
        for p in &f.params {
            let (_, sym, is_bool) = caller.params.iter().find(|(n, _, _)| *n == p.name)?;
            let v = &model.iter().find(|(k, _)| k == sym)?.1;
            args.push(if *is_bool { Value::Bool(v == "true") } else { Value::Int(v.parse().ok()?) });
            shown.push(format!("{} = {v}", p.name));
        }
        self.it.fuel.set(5_000_000);
        let saved = self.it.output.replace(Some(vec![]));
        let r = self.it.call_fn(&f, args);
        self.it.output.replace(saved);
        self.it.fuel.set(u64::MAX);
        match r {
            Err(Ctrl::Trap(m)) if m.contains(&want) => Some(format!("{}({}): {m}", f.name, shown.join(", "))),
            _ => None,
        }
    }
}

fn small_model(solver: &mut Solver, an: &Analysis, q: &Query) -> Option<Vec<(String, String)>> {
    let mut s = an.script(q);
    for (_, sym, is_bool) in &an.fns[q.fun].params {
        if !is_bool {
            s.push_str(&format!("(assert (and (<= (- 1000) {sym}) (<= {sym} 1000)))\n"));
        }
    }
    match solver.check(&s, &an.model_vars(q)) {
        Answer::Sat(m) if !m.is_empty() => Some(m),
        _ => None,
    }
}

pub fn verify(l: &Loaded, label_path: &str, json: bool) -> ExitCode {
    let an = sspur_smt::analyze(&l.module, &l.check);
    let mut solver = Solver::new();
    let have = solver.available();
    if !have && !json {
        println!("z3 not found: install it (brew install z3) or set SSPUR_Z3 to its path; every clause is reported as unknown");
    }
    let mut cx = Ctx { l, an: &an, it: crate::interp(l), tests: None };
    let mut counts = [0usize; 3];
    let mut rows = Vec::new();
    let mut last_fn = String::new();
    for c in &an.clauses {
        let entry = matches!(c.kind, ClauseKind::Where(_) | ClauseKind::Alias(..) | ClauseKind::Pre);
        let mut status = Status::Proved;
        let mut detail = String::new();
        if entry && c.obligations.is_empty() {
            rows.push(json!({"fn": l.show(&c.func), "clause": label(c), "status": "no callers"}));
            if !json {
                if last_fn != c.func {
                    println!("fn {}", l.show(&c.func));
                    last_fn = c.func.clone();
                }
                println!("  {:<44} no callers", label(c));
            }
            continue;
        }
        for o in &c.obligations {
            let Some(q) = &o.query else {
                status = status.max(Status::Unknown);
                if detail.is_empty() {
                    detail = "outside the encoded fragment".into();
                }
                continue;
            };
            if !have {
                status = Status::Unknown;
                continue;
            }
            let at = if entry {
                let (line, _) = line_col(&l.src, o.at);
                format!(" (call in {} at {label_path}:{line})", an.fns[o.caller].name)
            } else {
                String::new()
            };
            match solver.check(&an.script(q), &an.model_vars(q)) {
                Answer::Unsat => {}
                Answer::Unknown => {
                    status = status.max(Status::Unknown);
                    detail = format!("solver gave up{at}");
                }
                Answer::Sat(model) => match small_model(&mut solver, &an, q).and_then(|m| cx.confirm(q, &m, c)).or_else(|| cx.confirm(q, &model, c)) {
                    Some(m) => {
                        status = Status::Counterexample;
                        detail = m;
                        break;
                    }
                    None => {
                        status = status.max(Status::Unknown);
                        detail = format!("possible violation not reproduced{at}");
                    }
                },
            }
        }
        if status == Status::Proved && entry {
            detail = format!("at {} call site{}", c.obligations.len(), if c.obligations.len() == 1 { "" } else { "s" });
        }
        counts[status as usize] += 1;
        rows.push(json!({"fn": l.show(&c.func), "clause": label(c), "status": status.name(), "detail": detail}));
        if !json {
            if last_fn != c.func {
                println!("fn {}", l.show(&c.func));
                last_fn = c.func.clone();
            }
            let d = if detail.is_empty() { String::new() } else { format!(": {detail}") };
            println!("  {:<44} {}{d}", label(c), status.name());
        }
    }
    let (mut safe, mut total) = (0, 0);
    if have {
        let mut keys: Vec<_> = an.sites.iter().filter(|(k, s)| s.usable && (matches!(k.2.as_str(), "arith" | "neg" | "index" | "call") || k.2.starts_with("field:"))).collect();
        keys.sort_by_key(|(k, _)| (*k).clone());
        for (_, s) in keys {
            total += 1;
            if s.queries.iter().all(|q| q.as_ref().is_some_and(|q| solver.check(&an.script(q), &[]) == Answer::Unsat)) {
                safe += 1;
            }
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&json!({"z3": have, "clauses": rows, "proved": counts[0], "unknown": counts[1], "counterexample": counts[2], "runtime_checks": total, "runtime_checks_proved": safe})).unwrap());
    } else {
        println!("{} proved, {} counterexample, {} unknown; {safe} of {total} runtime checks proved safe", counts[0], counts[2], counts[1]);
    }
    if counts[2] > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

/// Adds `W_CONVERSION_MAY_TRAP` to `diags` (ADR 0044), unless there are errors; returns how many
/// conversions went unchecked for want of Z3.
pub fn conversions(l: &Loaded, diags: &mut Vec<sspur_check::Diag>) -> usize {
    if diags.iter().any(sspur_check::Diag::is_error) {
        return 0;
    }
    let r = sspur_smt::conv::check_program(&l.module, &l.check, l.own, &l.src, &l.hashes);
    diags.extend(r.diags);
    r.unchecked
}
