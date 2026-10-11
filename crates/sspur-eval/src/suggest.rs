//! Contract inference (ADR 0030): run a function on test-derived and fuzzed inputs, keep the
//! candidate `post` clauses that held on every run, and the `pre` that separates the inputs it
//! traps on from the rest.
use crate::value::{Env, Quoted, Value};
use crate::{fuzz, Ctrl, Interp};
use sspur_syntax::*;
use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;

pub struct Opts {
    /// Fuzzed inputs, on top of the arguments the tests and examples pass.
    pub cases: usize,
    pub seed: u64,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { cases: 200, seed: 7 }
    }
}

#[derive(Clone, Debug)]
pub struct Suggestion {
    /// `pre` or `post`.
    pub kind: &'static str,
    pub text: String,
    pub expr: Expr,
    /// Why it is suggested: `held on 180 runs`, or for a `pre`, `traps on 23 of 200 inputs without it`.
    pub why: String,
    /// Set by a caller that proved it with Z3.
    pub proved: bool,
}

impl Suggestion {
    pub fn note(&self) -> String {
        if self.proved { "proved".into() } else { self.why.clone() }
    }
}

pub struct FnSuggest {
    pub name: String,
    pub runs: usize,
    pub traps: usize,
    pub skipped: Option<String>,
    pub sugg: Vec<Suggestion>,
}

#[derive(Clone, Copy, PartialEq)]
enum Num {
    Int,
    F64,
}

/// A numeric term over the parameters or the result, such as `xs.len` or `r.qty`.
struct NTerm {
    text: String,
    num: Num,
    /// `xs.len` is never negative, so `t >= 0` says nothing about it.
    nonneg: bool,
}

fn ty_name(t: &Ty) -> Option<(&str, &[Ty])> {
    match t {
        Ty::Named { name, args, .. } => Some((name, args)),
        _ => None,
    }
}

/// Up to `max` combinations taking one value from each part.
pub(crate) fn product(parts: &[Vec<Value>], max: usize) -> Vec<Vec<Value>> {
    let mut out: Vec<Vec<Value>> = vec![vec![]];
    for p in parts {
        let mut next = Vec::new();
        for prefix in &out {
            for v in p {
                if next.len() >= max {
                    break;
                }
                let mut x = prefix.clone();
                x.push(v.clone());
                next.push(x);
            }
        }
        out = next;
    }
    out
}

fn shift(e: &mut Expr, base: &mut u32) {
    visit::walk_expr_mut(e, &mut |x| {
        *base += 1;
        x.span = Span { start: crate::synth::SPAN_BASE + *base, end: crate::synth::SPAN_BASE + *base };
    });
}

impl Interp {
    /// A few small values of `t`, for exhaustive small-scope runs: `0`, `[]`, `[0]`, `""` and the like.
    pub(crate) fn small(&self, t: &Ty, depth: u32) -> Vec<Value> {
        let Some((n, args)) = ty_name(t) else {
            return match t {
                Ty::Tuple(xs) if depth < 2 => {
                    let parts: Vec<Vec<Value>> = xs.iter().map(|x| self.small(x, depth + 1).into_iter().take(3).collect()).collect();
                    product(&parts, 12).into_iter().map(|v| Value::Tuple(Rc::new(v))).collect()
                }
                _ => vec![],
            };
        };
        match n {
            "Int" => [0, 1, -1, 2, 3].map(Value::Int).to_vec(),
            "F64" => [0.0, 1.0, -1.0, 0.5].map(Value::Float).to_vec(),
            "Bool" => vec![Value::Bool(false), Value::Bool(true)],
            "Str" => ["", "a", " b ", "ab"].map(Value::str).to_vec(),
            "List" if depth < 2 => {
                let el: Vec<Value> = args.first().map(|a| self.small(a, depth + 1)).unwrap_or_default();
                let mut out = vec![Value::list(vec![])];
                out.extend(el.iter().take(3).map(|x| Value::list(vec![x.clone()])));
                if el.len() >= 2 {
                    out.push(Value::list(vec![el[0].clone(), el[0].clone()]));
                    out.push(Value::list(vec![el[1].clone(), el[0].clone()]));
                    out.push(Value::list(el.iter().take(3).cloned().collect()));
                }
                out
            }
            "Opt" if depth < 2 => {
                let mut out = vec![Value::Opt(None)];
                out.extend(args.first().map(|a| self.small(a, depth + 1)).unwrap_or_default().into_iter().take(2).map(Value::some));
                out
            }
            _ => match self.types.get(n).map(|d| d.body.clone()) {
                Some(TypeBody::Alias(base, refine)) => self.small(&base, depth).into_iter().filter(|v| refine.as_ref().is_none_or(|r| self.check_refine(r, v, "").is_ok())).collect(),
                Some(TypeBody::Record(fs)) if depth < 2 && self.types[n].params.is_empty() => {
                    let parts: Vec<Vec<Value>> = fs.iter().map(|f| self.small(&f.ty, depth + 1).into_iter().take(2).collect()).collect();
                    product(&parts, 8)
                        .into_iter()
                        .filter_map(|vals| {
                            let named: Vec<(Rc<str>, Value)> = fs.iter().map(|f| Rc::<str>::from(f.name.as_str())).zip(vals).collect();
                            self.build_record(n, named).ok().map(|f| Value::Record(n.into(), f))
                        })
                        .collect()
                }
                _ => vec![],
            },
        }
    }

    fn num_terms(&self, name: &str, t: &Ty, out: &mut Vec<NTerm>) {
        let Some((n, _)) = ty_name(t) else { return };
        let resolved = match self.types.get(n).map(|d| &d.body) {
            Some(TypeBody::Alias(base, _)) => ty_name(base).map(|(b, _)| b.to_string()),
            _ => Some(n.to_string()),
        };
        match resolved.as_deref() {
            Some("Int") => out.push(NTerm { text: name.into(), num: Num::Int, nonneg: false }),
            Some("F64") => out.push(NTerm { text: name.into(), num: Num::F64, nonneg: false }),
            Some("List" | "Str") => out.push(NTerm { text: format!("{name}.len"), num: Num::Int, nonneg: true }),
            _ => {
                if let Some(TypeDef { body: TypeBody::Record(fs), params, .. }) = self.types.get(n)
                    && params.is_empty()
                {
                    for f in fs {
                        if let Some((fty, _)) = ty_name(&f.ty) {
                            let num = match fty {
                                "Int" => Num::Int,
                                "F64" => Num::F64,
                                _ => continue,
                            };
                            out.push(NTerm { text: format!("{name}.{}", f.name), num, nonneg: false });
                        }
                    }
                }
            }
        }
    }

    /// Suggests contracts for `only`, or for every function of the program that can be fuzzed.
    pub fn suggest(&self, only: Option<&str>, opts: &Opts) -> Vec<FnSuggest> {
        let mut names: Vec<String> = self.fns.keys().filter(|n| !self.foreign.contains(*n) && only.is_none_or(|o| o == n.as_str()) && n.as_str() != "main").cloned().collect();
        names.sort();
        let saved = self.output.replace(Some(vec![]));
        let out = names.iter().map(|n| self.suggest_fn(&self.fns[n].clone(), opts)).collect();
        *self.output.borrow_mut() = saved;
        self.fuel.set(u64::MAX);
        out
    }

    fn suggest_fn(&self, f: &FnDef, opts: &Opts) -> FnSuggest {
        let mut rep = FnSuggest { name: f.name.clone(), runs: 0, traps: 0, skipped: self.skip_reason(f), sugg: vec![] };
        if rep.skipped.is_some() || f.ret.is_none() {
            rep.skipped.get_or_insert_with(|| "returns Unit".into());
            return rep;
        }
        // Inputs: the arguments of every call the tests and examples make, then fuzzed ones.
        let mut inputs: Vec<Vec<Value>> = Vec::new();
        let mut seen = HashSet::new();
        *self.record.borrow_mut() = (f.name.clone(), vec![]);
        self.recording.set(true);
        for (_, body) in self.test_cases() {
            self.fuel.set(2_000_000);
            self.depth.set(0);
            let _ = self.run_case(&body);
        }
        self.recording.set(false);
        let recorded = std::mem::take(&mut self.record.borrow_mut().1);
        let mut rng = fuzz::Rng(opts.seed.max(1));
        let fo = fuzz::Options { cases: opts.cases, seed: opts.seed, edge: false };
        let mut fuzzed = Vec::new();
        for _ in 0..opts.cases * 3 {
            if fuzzed.len() >= opts.cases {
                break;
            }
            if let Some(args) = f.params.iter().map(|p| self.generate(&p.ty, &mut rng, &fo, 0)).collect::<Option<Vec<Value>>>() {
                fuzzed.push(args);
            }
        }
        let parts: Vec<Vec<Value>> = f.params.iter().map(|p| self.small(&p.ty, 0)).collect();
        let small = if parts.iter().all(|p| !p.is_empty()) { product(&parts, 200) } else { vec![] };
        for args in recorded.into_iter().chain(small).chain(fuzzed) {
            let key: Vec<String> = args.iter().map(|a| Quoted(a).to_string()).collect();
            if seen.insert(key) {
                inputs.push(args);
            }
        }
        // Run them.
        let mut ok: Vec<(Vec<Value>, Value)> = Vec::new();
        let mut fine: Vec<Vec<Value>> = Vec::new();
        let mut trapped: Vec<Vec<Value>> = Vec::new();
        for args in inputs {
            *self.output.borrow_mut() = Some(vec![]);
            self.fuel.set(fuzz::FUEL);
            self.depth.set(0);
            match self.call_fn(f, args.clone()) {
                Ok(v) | Err(Ctrl::Return(v)) => {
                    fine.push(args.clone());
                    ok.push((args, v));
                }
                Err(Ctrl::Raise(_)) => fine.push(args),
                Err(Ctrl::Trap(m)) if m == crate::OUT_OF_FUEL || fuzz::is_input_error(&m) || m.starts_with("stack overflow") => {}
                Err(Ctrl::Trap(_)) => trapped.push(args),
                Err(_) => {}
            }
        }
        rep.runs = fine.len() + trapped.len();
        rep.traps = trapped.len();
        let mut base = 0u32;
        let parse = |src: &str, base: &mut u32| {
            let mut e = parse_expr(src).ok()?;
            shift(&mut e, base);
            Some(e)
        };
        let env_of = |args: &[Value], r: Option<&Value>| {
            let env = Env::child(&self.globals);
            for (p, v) in f.params.iter().zip(args) {
                env.define(&p.name, v.clone());
            }
            if let Some(r) = r {
                env.define("r", r.clone());
            }
            env
        };
        let holds = |e: &Expr, env: &Rc<Env>| {
            self.fuel.set(20_000);
            self.depth.set(0);
            matches!(self.eval(e, env), Ok(Value::Bool(true)))
        };
        let existing: BTreeSet<String> = f.pres.iter().chain(&f.posts).map(|e| printer::expr(e, 0)).collect();
        let mut pterms = Vec::new();
        for p in &f.params {
            self.num_terms(&p.name, &p.ty, &mut pterms);
        }
        // A precondition: the simplest clause true on every input that ran and false on every one that trapped.
        if !trapped.is_empty() && !fine.is_empty() {
            let mut cands: Vec<String> = Vec::new();
            for t in pterms.iter().filter(|t| t.num == Num::Int) {
                if !t.nonneg {
                    cands.push(format!("{} >= 0", t.text));
                    cands.push(format!("{} != 0", t.text));
                }
                cands.push(format!("{} > 0", t.text));
            }
            for a in pterms.iter().filter(|t| t.num == Num::Int && !t.nonneg) {
                for b in pterms.iter().filter(|t| t.num == Num::Int && t.text != a.text) {
                    cands.push(format!("{} <= {}", a.text, b.text));
                    cands.push(format!("{} < {}", a.text, b.text));
                }
            }
            for p in &f.params {
                match ty_name(&p.ty).map(|x| x.0) {
                    Some("Bool") => cands.extend([p.name.clone(), format!("not {}", p.name)]),
                    Some("Opt") => cands.push(format!("{}.is_some", p.name)),
                    _ => {}
                }
            }
            let fine_envs: Vec<Rc<Env>> = fine.iter().map(|a| env_of(a, None)).collect();
            let trap_envs: Vec<Rc<Env>> = trapped.iter().map(|a| env_of(a, None)).collect();
            let parsed: Vec<(String, Expr)> = cands.into_iter().filter(|c| !existing.contains(c)).filter_map(|c| parse(&c, &mut base).map(|e| (c, e))).collect();
            let valid: Vec<&(String, Expr)> = parsed.iter().filter(|(_, e)| fine_envs.iter().all(|env| holds(e, env))).collect();
            let falsified = |e: &Expr| -> Vec<bool> { trap_envs.iter().map(|env| !holds(e, env)).collect() };
            let why = format!("traps on {} of {} inputs without it", trapped.len(), rep.runs);
            if let Some((t, e)) = valid.iter().find(|(_, e)| falsified(e).iter().all(|x| *x)) {
                rep.sugg.push(Suggestion { kind: "pre", text: t.clone(), expr: e.clone(), why, proved: false });
            } else {
                let cover: Vec<Vec<bool>> = valid.iter().map(|(_, e)| falsified(e)).collect();
                'pairs: for i in 0..valid.len() {
                    for j in i + 1..valid.len() {
                        if cover[i].iter().zip(&cover[j]).all(|(a, b)| *a || *b) && cover[i].iter().any(|x| *x) && cover[j].iter().any(|x| *x) {
                            for k in [i, j] {
                                rep.sugg.push(Suggestion { kind: "pre", text: valid[k].0.clone(), expr: valid[k].1.clone(), why: why.clone(), proved: false });
                            }
                            break 'pairs;
                        }
                    }
                }
            }
        }
        if ok.len() < 5 {
            return rep;
        }
        // Postconditions: every clause of a fixed grammar that held on every run.
        let ret = f.ret.clone().unwrap();
        let mut rterms = Vec::new();
        self.num_terms("r", &ret, &mut rterms);
        let mut cands: Vec<(String, u8)> = Vec::new();
        let distinct = |text: &str| -> usize {
            let Some(e) = parse(text, &mut 0) else { return 0 };
            let vals: BTreeSet<String> = ok.iter().take(60).filter_map(|(a, r)| {
                self.fuel.set(20_000);
                self.eval(&e, &env_of(a, Some(r))).ok().map(|v| Quoted(&v).to_string())
            }).collect();
            vals.len()
        };
        for r in &rterms {
            let zero = if r.num == Num::F64 { "0.0" } else { "0" };
            cands.push((format!("{} > {zero}", r.text), 3));
            if !r.nonneg {
                cands.push((format!("{} >= {zero}", r.text), 4));
            }
            for p in pterms.iter().filter(|p| p.num == r.num && distinct(&p.text) > 1) {
                cands.push((format!("{} == {}", r.text, p.text), 0));
                cands.push((format!("{} < {}", r.text, p.text), 1));
                cands.push((format!("{} > {}", r.text, p.text), 1));
                cands.push((format!("{} <= {}", r.text, p.text), 2));
                cands.push((format!("{} >= {}", r.text, p.text), 2));
            }
        }
        for p in &f.params {
            if printer::ty(&p.ty) == printer::ty(&ret) {
                cands.push((format!("r == {}", p.name), 0));
            }
        }
        if let Some(("List", [elem])) = ty_name(&ret)
            && matches!(ty_name(elem).map(|x| x.0), Some("Int" | "Str" | "F64"))
        {
            cands.push(("r == r.sort".into(), 1));
        }
        let envs: Vec<Rc<Env>> = ok.iter().map(|(a, r)| env_of(a, Some(r))).collect();
        let mut held: Vec<(String, u8, Expr)> = Vec::new();
        for (c, rank) in cands {
            if existing.contains(&c) {
                continue;
            }
            let Some(e) = parse(&c, &mut base) else { continue };
            if envs.iter().all(|env| holds(&e, env)) {
                held.push((c, rank, e));
            }
        }
        let texts: HashSet<String> = held.iter().map(|h| h.0.clone()).collect();
        let has = |t: String| texts.contains(&t);
        let whole: Vec<String> = f.params.iter().filter(|p| has(format!("r == {}", p.name))).map(|p| p.name.clone()).collect();
        let kept: Vec<&(String, u8, Expr)> = held
            .iter()
            .filter(|(c, _, _)| {
                let parts: Vec<&str> = c.splitn(3, ' ').collect();
                let [l, op, rt] = parts[..] else { return true };
                if l != "r" && whole.iter().any(|p| rt.strip_prefix(p.as_str()).is_some_and(|s| l.strip_prefix('r') == Some(s))) {
                    return false;
                }
                match op {
                    "<=" | ">=" if has(format!("{l} == {rt}")) || has(format!("{l} {} {rt}", &op[..1])) => false,
                    ">=" if (rt == "0" || rt == "0.0") && (has(format!("{l} > {rt}")) || pterms.iter().any(|p| p.nonneg && (has(format!("{l} >= {}", p.text)) || has(format!("{l} == {}", p.text)) || has(format!("{l} > {}", p.text))))) => false,
                    ">" if (rt == "0" || rt == "0.0") && pterms.iter().any(|p| p.nonneg && has(format!("{l} > {}", p.text))) => false,
                    _ => true,
                }
            })
            .collect();
        let mut kept: Vec<&(String, u8, Expr)> = kept;
        kept.sort_by_key(|(c, rank, _)| (*rank, c.clone()));
        let why = format!("held on {} runs", ok.len());
        for (c, _, e) in kept.into_iter().take(6) {
            rep.sugg.push(Suggestion { kind: "post", text: c.clone(), expr: e.clone(), why: why.clone(), proved: false });
        }
        rep
    }
}
