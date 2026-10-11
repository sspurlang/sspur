//! Typed hole filling (ADR 0030): enumerate small well-typed expressions bottom-up by size, keep
//! one per class of expressions that agree on every observed input, and rank the survivors by
//! the program's tests and contracts.
use crate::value::{Env, Value};
use crate::{fuzz, Ctrl, Interp, R};
use sspur_check::{CheckOutput, HoleCtx, HoleSite, Sig, SigKind, Type};
use sspur_syntax::*;
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

/// Spans of synthesized nodes start here, past any source position.
pub(crate) const SPAN_BASE: u32 = 0xF000_0000;
const MAX_ENVS: usize = 16;
const MAX_TERMS: usize = 30_000;
const MAX_TUPLES: usize = 4_000;
const EVAL_FUEL: u64 = 20_000;
const RUN_FUEL: u64 = 2_000_000;

#[derive(Default)]
pub(crate) struct HoleMode {
    fills: Vec<(Span, Rc<Expr>)>,
    probe: Option<Probe>,
}

struct Probe {
    span: Span,
    names: Vec<String>,
    seen: Vec<Vec<(String, Value)>>,
    hit: bool,
}

impl Interp {
    pub(crate) fn on_hole(&self, e: &Expr, env: &Rc<Env>) -> R {
        let fill = self.hole.borrow().fills.iter().find(|(s, _)| *s == e.span).map(|(_, x)| x.clone());
        if let Some(x) = fill {
            return self.eval(&x, env);
        }
        if let Some(p) = self.hole.borrow_mut().probe.as_mut().filter(|p| p.span == e.span) {
            if !p.hit && p.seen.len() < MAX_ENVS {
                p.seen.push(p.names.iter().filter_map(|n| env.get(n).map(|v| (n.clone(), v))).collect());
            }
            p.hit = true;
        }
        crate::trap("reached a typed hole")
    }
}

pub struct Opts {
    /// Time for one hole, in milliseconds.
    pub budget_ms: u64,
    /// The largest candidate, in AST nodes (`_` in a lambda is free).
    pub max_size: usize,
    pub lambda_size: usize,
    /// How many candidates to return.
    pub top: usize,
    /// Fuzzed inputs of the enclosing function, from its `pre` and `where` clauses.
    pub fuzz: usize,
    pub seed: u64,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { budget_ms: 200, max_size: 7, lambda_size: 3, top: 3, fuzz: 24, seed: 7 }
    }
}

#[derive(Clone, Debug)]
pub struct Cand {
    pub text: String,
    pub expr: Expr,
    pub size: usize,
    pub fns: usize,
    /// Tests and examples that reach the hole: passed and failed.
    pub passed: usize,
    pub failed: usize,
    /// Fuzzed inputs that reach the hole: contract violations and other traps.
    pub inputs: usize,
    pub broke: usize,
    pub traps: usize,
    /// What the enclosing function promises about its result: `post`, `where` for a refined result type.
    pub contract: Option<&'static str>,
    /// Set by a caller that proved the contracts with Z3.
    pub proved: bool,
    uses_input: bool,
}

impl Cand {
    pub fn ok(&self) -> bool {
        self.failed == 0 && self.broke == 0 && self.traps == 0
    }

    /// What the candidate was checked against, as `passes 3 tests, post`.
    pub fn evidence(&self) -> String {
        let total = self.passed + self.failed;
        let mut parts = vec![match (total, self.failed) {
            (0, _) => "no tests".to_string(),
            (n, 0) => format!("passes {n} test{}", if n == 1 { "" } else { "s" }),
            (n, f) => format!("fails {f} of {n} tests"),
        }];
        if let Some(k) = self.contract {
            if self.broke > 0 {
                parts.push(format!("breaks {k} on {} input{}", self.broke, if self.broke == 1 { "" } else { "s" }));
            } else if self.proved {
                parts.push(format!("{k} proved"));
            } else if self.inputs + total > 0 {
                parts.push(k.into());
            }
        }
        if self.traps > 0 {
            parts.push(format!("traps on {} input{}", self.traps, if self.traps == 1 { "" } else { "s" }));
        }
        parts.join(", ")
    }
}

pub struct Filled {
    pub site: HoleSite,
    pub cands: Vec<Cand>,
    /// Tests and fuzzed inputs that reached the hole.
    pub tests: usize,
    pub inputs: usize,
    pub explored: usize,
    pub classes: usize,
    pub ms: f64,
    pub timed_out: bool,
}

impl Filled {
    /// `?total: items.map(_.price * _.qty).sum (passes 3 tests, post)`, one per candidate.
    pub fn lines(&self) -> Vec<String> {
        self.cands.iter().map(|c| format!("{}: {} ({})", self.site.label(), c.text, c.evidence())).collect()
    }
}

/// Whether `want` (`?name`, `name`, `?` or the enclosing definition's name) picks this hole.
pub fn picks(site: &HoleSite, want: &str) -> bool {
    let w = want.strip_prefix('?').unwrap_or(want);
    match &site.name {
        Some(n) => n == w || site.def == w,
        None => w.is_empty() || site.def == w,
    }
}

/// Fills the holes of `m` (the checked source, `check` its check) using `it`, an interpreter for
/// the same program. Earlier holes are filled with their best candidate while later ones are searched.
pub fn fill(m: &Module, check: &CheckOutput, ctx: &HoleCtx, it: &Interp, opts: &Opts, only: Option<&str>) -> Vec<Filled> {
    let saved = it.output.replace(Some(vec![]));
    let mut fills: Vec<(Span, Rc<Expr>)> = vec![];
    let mut out = Vec::new();
    let span_ctr = Rc::new(Cell::new(0u32));
    for site in &ctx.holes {
        if only.is_some_and(|w| !picks(site, w)) {
            continue;
        }
        let f = fill_one(m, check, ctx, it, site, opts, &fills, &span_ctr);
        if let Some(best) = f.cands.first().filter(|c| c.ok()) {
            fills.push((site.span, Rc::new(best.expr.clone())));
        }
        out.push(f);
    }
    *it.hole.borrow_mut() = HoleMode::default();
    it.synth_float.borrow_mut().clear();
    it.fuel.set(u64::MAX);
    *it.output.borrow_mut() = saved;
    out
}

fn now_ms() -> f64 {
    crate::sys::mono_ns() as f64 / 1e6
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Obs {
    V(Value),
    Raise(Value),
    Trap,
}

#[derive(Clone)]
struct Term {
    show: Expr,
    run: Expr,
    ty: Type,
    size: usize,
    fns: Vec<String>,
    obs: Rc<Vec<Obs>>,
    local: bool,
    var: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Call,
    Method,
    Trait(String),
    Record(Vec<String>),
    Field(String),
    Bin(BinOp),
    Un(UnOp),
}

#[derive(Clone, Debug)]
struct Comp {
    name: String,
    kind: Kind,
    tparams: Vec<String>,
    params: Vec<Type>,
    ret: Type,
    effects: BTreeSet<String>,
    self_types: Vec<String>,
    commutes: bool,
}

type Binds = BTreeMap<String, Type>;
type Pool = Rc<Vec<Rc<Term>>>;

fn bind(p: &Type, a: &Type, tp: &[String], b: &mut Binds) -> bool {
    match (p, a) {
        (_, Type::Param(w)) if w == WILD => true,
        (Type::Param(n), _) if tp.contains(n) => match b.get(n) {
            Some(x) => x == a,
            None => {
                b.insert(n.clone(), a.clone());
                true
            }
        },
        (Type::Con(x, xs), Type::Con(y, ys)) => x == y && xs.len() == ys.len() && xs.iter().zip(ys).all(|(p, a)| bind(p, a, tp, b)),
        (Type::Tuple(xs), Type::Tuple(ys)) => xs.len() == ys.len() && xs.iter().zip(ys).all(|(p, a)| bind(p, a, tp, b)),
        (Type::Fn(ps, r, _), Type::Fn(qs, s, _)) => ps.len() == qs.len() && ps.iter().zip(qs).all(|(p, a)| bind(p, a, tp, b)) && bind(r, s, tp, b),
        _ => p == a,
    }
}

fn subst(t: &Type, b: &Binds) -> Type {
    match t {
        Type::Param(n) => b.get(n).cloned().unwrap_or_else(|| t.clone()),
        Type::Con(n, xs) => Type::Con(n.clone(), xs.iter().map(|x| subst(x, b)).collect()),
        Type::Tuple(xs) => Type::Tuple(xs.iter().map(|x| subst(x, b)).collect()),
        Type::Fn(ps, r, row) => Type::Fn(ps.iter().map(|x| subst(x, b)).collect(), Box::new(subst(r, b)), row.clone()),
        Type::Var(_) => t.clone(),
    }
}

fn open(t: &Type, tp: &[String]) -> bool {
    match t {
        Type::Param(n) => tp.contains(n),
        Type::Con(_, xs) | Type::Tuple(xs) => xs.iter().any(|x| open(x, tp)),
        Type::Fn(ps, r, _) => ps.iter().any(|x| open(x, tp)) || open(r, tp),
        Type::Var(_) => true,
    }
}

fn has_var(t: &Type) -> bool {
    match t {
        Type::Var(_) => true,
        Type::Con(_, xs) | Type::Tuple(xs) => xs.iter().any(has_var),
        Type::Fn(ps, r, _) => ps.iter().any(has_var) || has_var(r),
        Type::Param(_) => false,
    }
}

/// The outer constructor of a type, `List` for `List[Int]`, for indexing terms.
fn head(t: &Type) -> String {
    match t {
        Type::Con(n, _) => n.clone(),
        Type::Tuple(xs) => format!("({})", xs.len()),
        Type::Fn(..) => "->".into(),
        Type::Param(n) => format!("'{n}"),
        Type::Var(_) => "?".into(),
    }
}

fn con(t: &Type) -> Option<(&str, &[Type])> {
    match t {
        Type::Con(n, a) => Some((n, a)),
        _ => None,
    }
}

/// The builtins the search composes: the everyday methods of lists, strings, options, numbers,
/// maps and sets, and a few globals. The rest of the tables would mostly add noise and time.
const METHODS: &[(&str, &[&str])] = &[
    ("List", &["len", "sum", "map", "filter", "find", "any", "all", "first", "last", "reverse", "sort", "sort_by", "unique", "contains", "take", "drop", "push", "min", "max", "is_empty", "join", "index_of", "flat_map"]),
    ("Str", &["len", "trim", "lower", "upper", "split", "contains", "starts_with", "ends_with", "replace", "take", "drop", "to_int", "is_empty", "chars", "words", "reverse"]),
    ("Opt", &["or", "is_some", "is_none", "map"]),
    ("Int", &["abs", "to_f64"]),
    ("F64", &["abs", "round", "floor", "sqrt"]),
    ("Map", &["get", "has", "keys", "values", "len"]),
    ("#Set", &["has", "len", "items"]),
];
const GLOBALS: &[&str] = &["min", "max", "some", "ok", "err", "clamp"];

fn admitted(s: &Sig) -> bool {
    match &s.kind {
        SigKind::Method(recv) => METHODS.iter().any(|(r, ms)| r == recv && ms.contains(&s.name.as_str())),
        SigKind::Global => GLOBALS.contains(&s.name.as_str()),
        SigKind::Fn | SigKind::Trait => true,
    }
}

fn components(ctx: &HoleCtx, check: &CheckOutput, site: &HoleSite) -> Vec<Comp> {
    let mut out = Vec::new();
    let mut shapes: Vec<String> = vec![site.ty.to_string()];
    shapes.extend(ctx.sigs.iter().filter(|s| s.kind == SigKind::Fn).flat_map(|s| s.params.iter().map(|t| t.to_string())));
    shapes.extend(check.records.values().flat_map(|(_, fs)| fs.iter().map(|(_, t)| t.to_string())));
    shapes.extend(check.sums.values().flat_map(|(_, vs)| vs.iter().flat_map(|(_, fs)| fs.iter().flatten().map(|(_, t)| t.to_string()))));
    let wraps = |w: &str| shapes.iter().any(|t| t.contains(&format!("{w}[")));
    let allowed = |s: &Sig| s.effects.iter().all(|e| site.effects.contains(e) || s.tparams.iter().any(|p| e.contains(&format!("[{p}]"))));
    for s in &ctx.sigs {
        let wrap = match (&s.kind, s.name.as_str()) {
            (SigKind::Global, "some") => wraps("Opt"),
            (SigKind::Global, "ok" | "err") => wraps("Res"),
            _ => true,
        };
        if !wrap || s.params.len() > 4 || !allowed(s) || !admitted(s) || s.params.is_empty() && !matches!(s.kind, SigKind::Fn) || s.name == site.def {
            continue;
        }
        let kind = match &s.kind {
            SigKind::Fn | SigKind::Global => Kind::Call,
            SigKind::Method(_) => Kind::Method,
            SigKind::Trait => Kind::Trait(s.owner.clone()),
        };
        out.push(Comp { name: s.name.clone(), kind, tparams: s.tparams.clone(), params: s.params.clone(), ret: s.ret.clone(), effects: s.effects.clone(), self_types: s.self_types.clone(), commutes: matches!(s.name.as_str(), "min" | "max" | "gcd" | "lcm") });
    }
    let mut records: Vec<_> = check.records.iter().filter(|(n, _)| !n.starts_with('#')).collect();
    records.sort_by_key(|(n, _)| *n);
    for (name, (params, fields)) in records {
        let ty = Type::Con(name.clone(), params.iter().map(|p| Type::Param(p.clone())).collect());
        for (f, ft) in fields {
            out.push(Comp { name: f.clone(), kind: Kind::Field(f.clone()), tparams: params.clone(), params: vec![ty.clone()], ret: ft.clone(), effects: BTreeSet::new(), self_types: vec![], commutes: false });
        }
        if !fields.is_empty() && fields.len() <= 4 {
            out.push(Comp { name: name.clone(), kind: Kind::Record(fields.iter().map(|(f, _)| f.clone()).collect()), tparams: params.clone(), params: fields.iter().map(|(_, t)| t.clone()).collect(), ret: ty.clone(), effects: BTreeSet::new(), self_types: vec![], commutes: false });
        }
    }
    let mut sums: Vec<_> = check.sums.iter().collect();
    sums.sort_by_key(|(n, _)| *n);
    for (name, (params, variants)) in sums {
        let ty = Type::Con(name.clone(), params.iter().map(|p| Type::Param(p.clone())).collect());
        for (v, fields) in variants {
            let fields = fields.clone().unwrap_or_default();
            if fields.len() <= 4 {
                out.push(Comp { name: v.clone(), kind: Kind::Record(fields.iter().map(|(f, _)| f.clone()).collect()), tparams: params.clone(), params: fields.iter().map(|(_, t)| t.clone()).collect(), ret: ty.clone(), effects: BTreeSet::new(), self_types: vec![], commutes: false });
            }
        }
    }
    let a = || vec!["A".to_string()];
    let pa = || Type::Param("A".into());
    for t in ["Int", "F64"] {
        let t = Type::con(t);
        for op in [BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::Div, BinOp::Rem] {
            if op == BinOp::Rem && t != Type::int() {
                continue;
            }
            out.push(Comp { name: op.symbol().into(), kind: Kind::Bin(op), tparams: vec![], params: vec![t.clone(), t.clone()], ret: t.clone(), effects: BTreeSet::new(), self_types: vec![], commutes: matches!(op, BinOp::Add | BinOp::Mul) });
        }
        out.push(Comp { name: "-".into(), kind: Kind::Un(UnOp::Neg), tparams: vec![], params: vec![t.clone()], ret: t.clone(), effects: BTreeSet::new(), self_types: vec![], commutes: false });
    }
    for t in ["Int", "F64", "Str"] {
        let t = Type::con(t);
        for op in [BinOp::Lt, BinOp::Le, BinOp::Gt, BinOp::Ge] {
            out.push(Comp { name: op.symbol().into(), kind: Kind::Bin(op), tparams: vec![], params: vec![t.clone(), t.clone()], ret: Type::bool(), effects: BTreeSet::new(), self_types: vec![], commutes: false });
        }
    }
    for op in [BinOp::Eq, BinOp::Ne] {
        out.push(Comp { name: op.symbol().into(), kind: Kind::Bin(op), tparams: a(), params: vec![pa(), pa()], ret: Type::bool(), effects: BTreeSet::new(), self_types: vec![], commutes: true });
    }
    for op in [BinOp::And, BinOp::Or] {
        out.push(Comp { name: op.symbol().into(), kind: Kind::Bin(op), tparams: vec![], params: vec![Type::bool(), Type::bool()], ret: Type::bool(), effects: BTreeSet::new(), self_types: vec![], commutes: true });
    }
    out.push(Comp { name: "not".into(), kind: Kind::Un(UnOp::Not), tparams: vec![], params: vec![Type::bool()], ret: Type::bool(), effects: BTreeSet::new(), self_types: vec![], commutes: false });
    out.push(Comp { name: "+".into(), kind: Kind::Bin(BinOp::Add), tparams: vec![], params: vec![Type::str(), Type::str()], ret: Type::str(), effects: BTreeSet::new(), self_types: vec![], commutes: false });
    out.push(Comp { name: "+".into(), kind: Kind::Bin(BinOp::Add), tparams: a(), params: vec![Type::list(pa()), Type::list(pa())], ret: Type::list(pa()), effects: BTreeSet::new(), self_types: vec![], commutes: false });
    out
}

struct En<'a> {
    it: &'a Interp,
    check: &'a CheckOutput,
    envs: Vec<Rc<Env>>,
    terms: Vec<Term>,
    by_size: Vec<Vec<usize>>,
    by_ty: HashMap<(usize, String), Vec<usize>>,
    by_head: HashMap<(usize, String), Vec<usize>>,
    seen: HashSet<String>,
    seen_obs: BTreeMap<(String, Rc<Vec<Obs>>), usize>,
    comps: Rc<Vec<Comp>>,
    lambda: bool,
    lambda_size: usize,
    deadline: f64,
    explored: usize,
    spans: Rc<Cell<u32>>,
    /// Per element type: how many samples it was built from, the term count then, and the bodies.
    lambdas: HashMap<String, (usize, usize, Pool)>,
    fn_values: Vec<(String, Type, Type)>,
    out_of_time: bool,
    steps: u64,
    /// The hole's type and the size bound: terms that can't grow into the type in time are dropped.
    target: Option<Type>,
    max: usize,
    memo: HashMap<(String, usize), bool>,
    /// One child of each input's environment with the argument slots, reused for every evaluation.
    scratch: Vec<Rc<Env>>,
}

fn scratch(envs: &[Rc<Env>]) -> Vec<Rc<Env>> {
    envs.iter()
        .map(|e| {
            let env = Env::child(e);
            for s in SLOTS {
                env.define(s, Value::Unit);
            }
            env
        })
        .collect()
}

fn mk(kind: ExprKind, span: Span) -> Expr {
    Expr { kind, span }
}

impl<'a> En<'a> {
    fn span(&self) -> Span {
        let n = self.spans.get() + 1;
        self.spans.set(n);
        Span { start: SPAN_BASE + n, end: SPAN_BASE + n }
    }

    fn time_up(&mut self) -> bool {
        self.steps += 1;
        if !self.out_of_time && (self.steps.is_multiple_of(64) && now_ms() > self.deadline || self.terms.len() >= MAX_TERMS) {
            self.out_of_time = true;
        }
        self.out_of_time
    }

    fn observe(&self, run: &Expr) -> Vec<Obs> {
        self.envs
            .iter()
            .map(|env| {
                self.it.fuel.set(EVAL_FUEL);
                self.it.depth.set(0);
                match self.it.eval(run, env) {
                    Ok(v) => Obs::V(v),
                    Err(Ctrl::Raise(v)) => Obs::Raise(v),
                    Err(_) => Obs::Trap,
                }
            })
            .collect()
    }

    /// Adds a term unless an equal or smaller one already gives the same values on every input.
    fn add(&mut self, t: Term) -> Option<usize> {
        self.add_with(t, None)
    }

    fn add_with(&mut self, mut t: Term, obs: Option<Vec<Obs>>) -> Option<usize> {
        self.explored += 1;
        let key = t.ty.to_string();
        if matches!(t.ty, Type::Fn(..)) || self.envs.is_empty() {
            if !self.seen.insert(format!("{key}|{}", printer::expr(&t.show, 0))) {
                return None;
            }
        } else {
            let obs = obs.unwrap_or_else(|| self.observe(&t.run));
            if obs.iter().all(|o| *o == Obs::Trap) {
                return None;
            }
            let obs = Rc::new(obs);
            if let Some(&id) = self.seen_obs.get(&(key.clone(), obs.clone())) {
                // Same values as a term found before: keep the simpler of two of the same size.
                let old = &self.terms[id];
                let (a, b) = ((t.fns.len(), !t.local), (old.fns.len(), !old.local));
                if old.size == t.size && (a < b || a == b && simpler(&t.show, &old.show)) {
                    t.obs = obs;
                    self.terms[id] = t;
                }
                return None;
            }
            self.seen_obs.insert((key.clone(), obs.clone()), self.terms.len());
            t.obs = obs;
        }
        let id = self.terms.len();
        while self.by_size.len() <= t.size {
            self.by_size.push(vec![]);
        }
        self.by_size[t.size].push(id);
        self.by_ty.entry((t.size, key)).or_default().push(id);
        self.by_head.entry((t.size, head(&t.ty))).or_default().push(id);
        self.terms.push(t);
        Some(id)
    }

    fn atom(&mut self, show: ExprKind, ty: Type, local: bool) {
        let e = mk(show, self.span());
        self.add(Term { run: e.clone(), show: e, ty, size: 1, fns: vec![], obs: Rc::new(vec![]), local, var: false });
    }

    fn seed(&mut self, site: &HoleSite, extra: &[(String, Type)]) {
        if self.lambda {
            let e = mk(ExprKind::Placeholder, self.span());
            let ty = extra[0].1.clone();
            self.add(Term { run: e.clone(), show: e, ty, size: 0, fns: vec![], obs: Rc::new(vec![]), local: true, var: true });
        }
        for (n, t) in &site.locals {
            if !has_var(t) {
                self.atom(ExprKind::Name(n.clone()), t.clone(), true);
            }
        }
        let mut types: Vec<Type> = vec![site.ty.clone()];
        types.extend(site.locals.iter().map(|(_, t)| t.clone()));
        types.extend(extra.iter().map(|(_, t)| t.clone()));
        for (ps, fs) in self.check.records.values() {
            if ps.is_empty() {
                types.extend(fs.iter().map(|(_, t)| t.clone()));
            }
        }
        let text: String = types.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(" ");
        self.atom(ExprKind::Int(0), Type::int(), false);
        self.atom(ExprKind::Int(1), Type::int(), false);
        if text.contains("F64") {
            self.atom(ExprKind::Float(0.0), Type::con("F64"), false);
            self.atom(ExprKind::Float(1.0), Type::con("F64"), false);
        }
        self.atom(ExprKind::Bool(true), Type::bool(), false);
        self.atom(ExprKind::Bool(false), Type::bool(), false);
        if text.contains("Str") {
            self.atom(ExprKind::Str(vec![]), Type::str(), false);
        }
        if site.ty == Type::unit() {
            self.atom(ExprKind::Unit, Type::unit(), false);
        }
        if self.lambda {
            return;
        }
        let mut shells = BTreeMap::new();
        for t in &types {
            collect_shells(t, &mut shells);
        }
        for t in shells.into_values() {
            let kind = if con(&t).is_some_and(|(n, _)| n == "List") { ExprKind::List(vec![]) } else { ExprKind::Name("none".into()) };
            self.atom(kind, t, false);
        }
        let comps = self.comps.clone();
        for c in comps.iter().filter(|c| c.params.is_empty()) {
            let mut b = Binds::new();
            if open(&c.ret, &c.tparams) && !bind(&c.ret, &site.ty, &c.tparams, &mut b) {
                continue;
            }
            let ty = subst(&c.ret, &b);
            if open(&ty, &c.tparams) {
                continue;
            }
            let show = match &c.kind {
                Kind::Record(_) => mk(ExprKind::Name(c.name.clone()), self.span()),
                _ => mk(ExprKind::Call(Box::new(mk(ExprKind::Name(c.name.clone()), self.span())), vec![]), self.span()),
            };
            let fns = if matches!(c.kind, Kind::Call) { vec![c.name.clone()] } else { vec![] };
            self.add(Term { run: show.clone(), show, ty, size: 1, fns, obs: Rc::new(vec![]), local: false, var: false });
        }
    }

    fn grow(&mut self, max: usize, site: &HoleSite) {
        for s in 1..=max {
            self.grow_level(s, site);
        }
    }

    fn grow_level(&mut self, s: usize, site: &HoleSite) {
        if self.out_of_time {
            return;
        }
        let comps = self.comps.clone();
        for c in comps.iter().filter(|c| !c.params.is_empty()) {
            if self.lambda && (matches!(c.kind, Kind::Trait(_) | Kind::Record(_)) || c.params.iter().any(|p| matches!(p, Type::Fn(..)))) {
                continue;
            }
            let mut found = Vec::new();
            self.args(c, 0, s - 1, &mut Vec::new(), &mut Binds::new(), site, &mut found);
            for (ids, b) in found {
                if self.time_up() {
                    return;
                }
                self.build(c, &ids, &b, site);
            }
            if self.time_up() {
                return;
            }
        }
        self.tuple_fields(s);
    }

    /// Whether a term of type `t` can still become part of a term of the hole's type within `k` more nodes.
    fn useful(&mut self, t: &Type, k: usize) -> bool {
        let Some(target) = self.target.clone() else { return true };
        if wild_eq(t, &target) {
            return true;
        }
        if k == 0 {
            return false;
        }
        let key = (t.to_string(), k);
        if let Some(&b) = self.memo.get(&key) {
            return b;
        }
        self.memo.insert(key.clone(), false);
        let mut r = false;
        if let Type::Tuple(xs) = t {
            r = xs.clone().iter().any(|x| self.useful(x, k - 1));
        }
        let comps = self.comps.clone();
        'c: for c in comps.iter() {
            let n = c.params.len();
            if r || n == 0 || n > k {
                continue;
            }
            for i in 0..n {
                let mut b = Binds::new();
                if !bind(&c.params[i], t, &c.tparams, &mut b) {
                    continue;
                }
                let ret = wild(&subst(&c.ret, &b), &c.tparams);
                if self.useful(&ret, k - n) {
                    r = true;
                    break 'c;
                }
            }
        }
        self.memo.insert(key, r);
        r
    }

    fn tuple_fields(&mut self, s: usize) {
        let ids: Vec<usize> = self.by_size.get(s - 1).cloned().unwrap_or_default();
        for id in ids {
            let Type::Tuple(xs) = self.terms[id].ty.clone() else { continue };
            for (i, t) in xs.iter().enumerate() {
                let a = &self.terms[id];
                let (show, run) = (mk(ExprKind::Field(Box::new(a.show.clone()), i.to_string()), self.span()), mk(ExprKind::Field(Box::new(a.run.clone()), i.to_string()), self.span()));
                let t = Term { show, run, ty: t.clone(), size: s, fns: a.fns.clone(), obs: Rc::new(vec![]), local: a.local, var: a.var };
                self.add(t);
            }
        }
    }

    /// Argument tuples for `c` whose sizes add up to `left`, with the type parameters they bind.
    #[allow(clippy::too_many_arguments)]
    fn args(&mut self, c: &Comp, i: usize, left: usize, picked: &mut Vec<Arg>, b: &mut Binds, site: &HoleSite, out: &mut Vec<(Vec<Arg>, Binds)>) {
        let n = c.params.len();
        if i == n {
            if left == 0 {
                out.push((picked.clone(), b.clone()));
            }
            return;
        }
        if out.len() >= MAX_TUPLES || self.time_up() {
            return;
        }
        let rest = n - i - 1;
        if left < rest || left < 1 + rest && !(self.lambda && i == 0) {
            return;
        }
        let p = subst(&c.params[i], b);
        if let Type::Fn(ps, r, _) = &p {
            if self.lambda || ps.len() != 1 || open(&ps[0], &c.tparams) {
                return;
            }
            let pool = self.lambda_pool(&ps[0], site);
            for t in pool.iter() {
                if t.size > left || left - t.size < rest {
                    continue;
                }
                let mut b2 = b.clone();
                if !bind(r, &t.ty, &c.tparams, &mut b2) {
                    continue;
                }
                picked.push(Arg::Lambda(t.clone()));
                self.args(c, i + 1, left - t.size, picked, &mut b2, site, out);
                picked.pop();
            }
            if left > rest {
                for (fname, fp, fr) in self.fn_values.clone() {
                    let mut b2 = b.clone();
                    if fp == ps[0] && bind(r, &fr, &c.tparams, &mut b2) {
                        picked.push(Arg::FnValue(fname));
                        self.args(c, i + 1, left - 1, picked, &mut b2, site, out);
                        picked.pop();
                    }
                }
            }
            return;
        }
        let lo = if self.lambda && i == 0 { 0 } else { 1 };
        let closed = !open(&p, &c.tparams);
        let (key, hd) = (if closed { p.to_string() } else { String::new() }, head(&p));
        for sz in lo..=left - rest {
            let ids: Vec<usize> = if closed {
                self.by_ty.get(&(sz, key.clone())).cloned().unwrap_or_default()
            } else if matches!(&p, Type::Param(n) if c.tparams.contains(n)) {
                self.by_size.get(sz).cloned().unwrap_or_default()
            } else {
                self.by_head.get(&(sz, hd.clone())).cloned().unwrap_or_default()
            };
            for id in ids {
                let t = &self.terms[id];
                if self.lambda && t.var && i > 0 && !matches!(c.kind, Kind::Bin(_)) {
                    continue;
                }
                if c.commutes && i == 1 && matches!(picked[0], Arg::Term(a) if a > id) {
                    continue;
                }
                if closed {
                    picked.push(Arg::Term(id));
                    self.args(c, i + 1, left - sz, picked, b, site, out);
                    picked.pop();
                    continue;
                }
                let mut b2 = b.clone();
                if !bind(&p, &t.ty, &c.tparams, &mut b2) {
                    continue;
                }
                picked.push(Arg::Term(id));
                self.args(c, i + 1, left - sz, picked, &mut b2, site, out);
                picked.pop();
            }
        }
    }

    fn build(&mut self, c: &Comp, args: &[Arg], b: &Binds, site: &HoleSite) {
        let mut b = b.clone();
        if open(&c.ret, &c.tparams) {
            let mut b2 = b.clone();
            if bind(&c.ret, &site.ty, &c.tparams, &mut b2) {
                b = b2;
            }
        }
        let ty = subst(&c.ret, &b);
        if open(&ty, &c.tparams) {
            return;
        }
        if matches!(c.name.as_str(), "min" | "max" | "clamp") && matches!(c.kind, Kind::Call) && !b.get("A").is_some_and(|t| t.is_numeric() || *t == Type::str()) {
            return;
        }
        if matches!(c.kind, Kind::Bin(BinOp::Eq | BinOp::Ne)) && args.iter().any(|a| matches!(a, Arg::Term(id) if matches!(self.terms[*id].show.kind, ExprKind::Bool(_)))) {
            return;
        }
        if self.redundant(c, args) {
            return;
        }
        let size: usize = 1 + args.iter().map(|a| self.arg_size(a)).sum::<usize>();
        if size > self.max.max(self.lambda_size) || !self.lambda && !self.useful(&ty, self.max.saturating_sub(size)) {
            return;
        }
        for e in &c.effects {
            let mut e = e.clone();
            for (p, t) in &b {
                e = e.replace(&format!("[{p}]"), &format!("[{t}]"));
            }
            if !site.effects.contains(&e) {
                return;
            }
        }
        let mut quick = Vec::new();
        let mut size = 1;
        let mut fns: Vec<String> = vec![];
        let (mut local, mut var) = (false, false);
        for (i, a) in args.iter().enumerate() {
            match a {
                Arg::Term(id) => {
                    let t = &self.terms[*id];
                    quick.push(mk(ExprKind::Name(format!("${i}")), t.run.span));
                    size += t.size;
                    fns.extend(t.fns.iter().cloned());
                    local |= t.local;
                    var |= t.var;
                }
                Arg::Lambda(t) => {
                    quick.push(lambda(&t.run, t.run.span));
                    size += t.size;
                    fns.extend(t.fns.iter().cloned());
                    local |= t.local;
                }
                Arg::FnValue(f) => {
                    quick.push(mk(ExprKind::Name(f.clone()), Span::default()));
                    size += 1;
                    fns.push(f.clone());
                }
            }
        }
        let unit = match &c.kind {
            Kind::Trait(tr) => {
                let Some(Type::Con(self_ty, _)) = b.get("Self") else { return };
                if !c.self_types.contains(self_ty) {
                    return;
                }
                let key = (tr.clone(), self_ty.clone());
                let unit = self.check.traits.impls.get(&key).and_then(|i| i.fns.get(&c.name)).or_else(|| self.check.traits.defaults.get(&(tr.clone(), c.name.clone())));
                let Some(unit) = unit.cloned() else { return };
                Some(unit)
            }
            _ => None,
        };
        if !matches!(c.kind, Kind::Field(_) | Kind::Record(_) | Kind::Bin(_) | Kind::Un(_)) {
            fns.push(c.name.clone());
        }
        fns.sort();
        fns.dedup();
        let sp = self.span();
        if matches!(c.kind, Kind::Method) && c.name == "sum" && ty == Type::con("F64") {
            self.it.synth_float.borrow_mut().insert((sp.start, sp.end));
        }
        let callee = unit.as_deref().unwrap_or(&c.name).to_string();
        // The node over the values its arguments already have, so each step evaluates one node.
        let obs = (!self.envs.is_empty() && !matches!(ty, Type::Fn(..))).then(|| {
            let top = self.node(c, &callee, quick, sp);
            let vals: Vec<Option<&Rc<Vec<Obs>>>> = args.iter().map(|a| if let Arg::Term(id) = a { Some(&self.terms[*id].obs) } else { None }).collect();
            let out: Vec<Obs> = (0..self.envs.len())
                .map(|e| {
                    let env = &self.scratch[e];
                    for (i, v) in vals.iter().enumerate() {
                        match v.map(|o| &o[e]) {
                            Some(Obs::V(x)) => *env.cell(SLOTS[i]).unwrap().borrow_mut() = x.clone(),
                            Some(other) => return other.clone(),
                            None => {}
                        }
                    }
                    self.it.fuel.set(EVAL_FUEL);
                    self.it.depth.set(0);
                    match self.it.eval(&top, env) {
                        Ok(v) => Obs::V(v),
                        Err(Ctrl::Raise(v)) => Obs::Raise(v),
                        Err(_) => Obs::Trap,
                    }
                })
                .collect();
            Rc::new(out)
        });
        let key = ty.to_string();
        if let Some(o) = &obs {
            if o.iter().all(|x| *x == Obs::Trap) {
                self.explored += 1;
                return;
            }
            if let Some(&id) = self.seen_obs.get(&(key.clone(), o.clone())) {
                // Same values as a term found before: keep the simpler of two of the same size.
                self.explored += 1;
                let old = &self.terms[id];
                let (x, y) = ((fns.len(), !local), (old.fns.len(), !old.local));
                if old.size != size || x > y {
                    return;
                }
                let show = self.node(c, &c.name, self.arg_exprs(args, false), sp);
                if x == y && !simpler(&show, &old.show) {
                    return;
                }
                let run = self.node(c, &callee, self.arg_exprs(args, true), sp);
                self.terms[id] = Term { show, run, ty, size, fns, obs: o.clone(), local, var };
                return;
            }
        }
        let show = self.node(c, &c.name, self.arg_exprs(args, false), sp);
        let run = self.node(c, &callee, self.arg_exprs(args, true), sp);
        self.add_with(Term { show, run, ty, size, fns, obs: Rc::new(vec![]), local, var }, obs.map(|o| (*o).clone()));
    }

    fn arg_exprs(&self, args: &[Arg], run: bool) -> Vec<Expr> {
        args.iter()
            .map(|a| match a {
                Arg::Term(id) => {
                    let t = &self.terms[*id];
                    if run { t.run.clone() } else { t.show.clone() }
                }
                Arg::Lambda(t) => lambda(if run { &t.run } else { &t.show }, t.run.span),
                Arg::FnValue(f) => mk(ExprKind::Name(f.clone()), Span::default()),
            })
            .collect()
    }

    fn node(&self, c: &Comp, callee: &str, mut args: Vec<Expr>, sp: Span) -> Expr {
        let fresh = Span { start: sp.start, end: sp.end };
        let kind = match &c.kind {
            Kind::Call | Kind::Trait(_) => ExprKind::Call(Box::new(mk(ExprKind::Name(callee.to_string()), fresh)), args),
            Kind::Method if c.params.len() == 1 => ExprKind::Field(Box::new(args.remove(0)), c.name.clone()),
            Kind::Method => {
                let recv = args.remove(0);
                ExprKind::Method { recv: Box::new(recv), name: c.name.clone(), targs: vec![], args }
            }
            Kind::Field(f) => ExprKind::Field(Box::new(args.remove(0)), f.clone()),
            Kind::Record(fields) => ExprKind::Record { ctor: Some(c.name.clone()), fields: fields.iter().cloned().zip(args).collect() },
            Kind::Bin(op) => {
                let r = args.pop().unwrap();
                ExprKind::Binary(*op, Box::new(args.pop().unwrap()), Box::new(r))
            }
            Kind::Un(op) => ExprKind::Unary(*op, Box::new(args.remove(0))),
        };
        mk(kind, sp)
    }

    /// Terms that are certainly equal to a smaller one or a constant: a method of a literal,
    /// `xs.reverse.sum`, `x.abs.abs`, `xs.map(f).len`, `1 + 1`.
    fn redundant(&self, c: &Comp, args: &[Arg]) -> bool {
        let term = |a: &Arg| match a {
            Arg::Term(id) => Some(&self.terms[*id]),
            _ => None,
        };
        let constant = |a: &Arg| term(a).is_some_and(|t| !t.local && !t.var);
        match &c.kind {
            Kind::Bin(_) => args.iter().all(constant),
            Kind::Un(op) => constant(&args[0]) || term(&args[0]).is_some_and(|t| matches!(&t.show.kind, ExprKind::Unary(o, _) if o == op)),
            Kind::Method | Kind::Field(_) => {
                if constant(&args[0]) {
                    return true;
                }
                let Some(r) = term(&args[0]) else { return false };
                let inner = match &r.show.kind {
                    ExprKind::Field(_, n) => n.as_str(),
                    ExprKind::Method { name, .. } => name.as_str(),
                    _ => return false,
                };
                let outer = c.name.as_str();
                let idem = ["abs", "sort", "unique", "trim", "lower", "upper", "reverse"];
                let blind = ["sum", "len", "min", "max", "contains", "any", "all", "is_empty", "sort", "unique"];
                inner == outer && idem.contains(&outer) || blind.contains(&outer) && matches!(inner, "reverse" | "sort" | "sort_by") || outer == "len" && matches!(inner, "map" | "sort_by" | "reverse" | "sort")
            }
            _ => false,
        }
    }

    /// Lambda bodies over `_: p`, enumerated against elements of the lists seen so far.
    fn lambda_pool(&mut self, p: &Type, site: &HoleSite) -> Pool {
        let key = p.to_string();
        if let Some((n, at, x)) = self.lambdas.get(&key)
            && (*n > 0 || *at == self.terms.len())
        {
            return x.clone();
        }
        let mut samples: Vec<Rc<Env>> = Vec::new();
        let list = Type::list(p.clone());
        let opt = Type::opt(p.clone());
        'outer: for t in self.terms.iter().filter(|t| t.ty == list || t.ty == opt) {
            for (i, o) in t.obs.iter().enumerate() {
                let elems: Vec<Value> = match o {
                    Obs::V(Value::List(xs)) => xs.iter().take(4).cloned().collect(),
                    Obs::V(Value::Opt(Some(x))) => vec![(**x).clone()],
                    _ => vec![],
                };
                for v in elems {
                    let env = Env::child(&self.envs[i]);
                    env.define("_", v);
                    samples.push(env);
                    if samples.len() >= MAX_ENVS {
                        break 'outer;
                    }
                }
            }
        }
        let mut sub = En {
            it: self.it,
            check: self.check,
            envs: samples,
            terms: vec![],
            by_size: vec![],
            by_ty: HashMap::new(),
            by_head: HashMap::new(),
            seen: HashSet::new(),
            seen_obs: BTreeMap::new(),
            comps: self.comps.clone(),
            lambda: true,
            lambda_size: self.lambda_size,
            deadline: self.deadline,
            explored: 0,
            spans: self.spans.clone(),
            lambdas: HashMap::new(),
            fn_values: vec![],
            out_of_time: false,
            steps: 0,
            target: None,
            max: self.lambda_size,
            memo: HashMap::new(),
            scratch: vec![],
        };
        sub.scratch = scratch(&sub.envs);
        let n = sub.envs.len();
        sub.seed(site, &[("_".into(), p.clone())]);
        sub.grow(if n == 0 { 1 } else { self.lambda_size }, site);
        self.explored += sub.explored;
        let pool: Vec<Rc<Term>> = sub.terms.into_iter().filter(|t| t.var && t.size > 0).map(Rc::new).collect();
        let pool = Rc::new(pool);
        self.lambdas.insert(key, (n, self.terms.len(), pool.clone()));
        pool
    }
}

/// `_ => body` in the implicit form the parser gives `xs.map(_.f)`.
fn lambda(body: &Expr, span: Span) -> Expr {
    mk(ExprKind::Lambda { params: vec!["_".into()], body: Box::new(body.clone()), implicit: true }, span)
}

/// The names compositional evaluation binds the argument values to.
const SLOTS: [&str; 6] = ["$0", "$1", "$2", "$3", "$4", "$5"];

/// Of two equally big expressions, the shorter text, then the alphabetically first.
fn simpler(a: &Expr, b: &Expr) -> bool {
    let (a, b) = (printer::expr(a, 0), printer::expr(b, 0));
    (a.len(), &a) < (b.len(), &b)
}

impl En<'_> {
    fn arg_size(&self, a: &Arg) -> usize {
        match a {
            Arg::Term(id) => self.terms[*id].size,
            Arg::Lambda(t) => t.size,
            Arg::FnValue(_) => 1,
        }
    }
}

const WILD: &str = "?";

/// `t` with the type parameters still open replaced by a wildcard.
fn wild(t: &Type, tp: &[String]) -> Type {
    match t {
        Type::Param(n) if tp.contains(n) => Type::Param(WILD.into()),
        Type::Con(n, xs) => Type::Con(n.clone(), xs.iter().map(|x| wild(x, tp)).collect()),
        Type::Tuple(xs) => Type::Tuple(xs.iter().map(|x| wild(x, tp)).collect()),
        Type::Fn(ps, r, row) => Type::Fn(ps.iter().map(|x| wild(x, tp)).collect(), Box::new(wild(r, tp)), row.clone()),
        _ => t.clone(),
    }
}

fn wild_eq(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Param(w), _) | (_, Type::Param(w)) if w == WILD => true,
        (Type::Con(x, xs), Type::Con(y, ys)) => x == y && xs.len() == ys.len() && xs.iter().zip(ys).all(|(a, b)| wild_eq(a, b)),
        (Type::Tuple(xs), Type::Tuple(ys)) => xs.len() == ys.len() && xs.iter().zip(ys).all(|(a, b)| wild_eq(a, b)),
        _ => a == b,
    }
}

#[derive(Clone)]
enum Arg {
    Term(usize),
    Lambda(Rc<Term>),
    FnValue(String),
}

fn collect_shells(t: &Type, out: &mut BTreeMap<String, Type>) {
    if let Type::Con(n, a) = t {
        if (n == "List" || n == "Opt") && !has_var(t) {
            out.insert(t.to_string(), t.clone());
        }
        for x in a {
            collect_shells(x, out);
        }
    }
}

fn contract_of(it: &Interp, f: &FnDef) -> Option<&'static str> {
    if !f.posts.is_empty() {
        return Some("post");
    }
    f.ret.as_ref().is_some_and(|t| matches!(t, Ty::Named { name, .. } if it.alias_refines.contains_key(name))).then_some("where")
}

#[allow(clippy::too_many_arguments)]
fn fill_one(m: &Module, check: &CheckOutput, ctx: &HoleCtx, it: &Interp, site: &HoleSite, opts: &Opts, prior: &[(Span, Rc<Expr>)], spans: &Rc<Cell<u32>>) -> Filled {
    let t0 = now_ms();
    let mut filled = Filled { site: site.clone(), cands: vec![], tests: 0, inputs: 0, explored: 0, classes: 0, ms: 0.0, timed_out: false };
    if has_var(&site.ty) {
        return filled;
    }
    // Observe: the values in scope each time a test, an example or a fuzzed call reaches the hole.
    let names: Vec<String> = site.locals.iter().map(|(n, _)| n.clone()).collect();
    *it.hole.borrow_mut() = HoleMode { fills: prior.to_vec(), probe: Some(Probe { span: site.span, names, seen: vec![], hit: false }) };
    let hit = || {
        let mut h = it.hole.borrow_mut();
        let p = h.probe.as_mut().unwrap();
        std::mem::replace(&mut p.hit, false)
    };
    let cases = it.test_cases();
    let mut reach: Vec<(usize, usize)> = Vec::new();
    for (i, (_, body)) in cases.iter().enumerate() {
        it.fuel.set(RUN_FUEL);
        it.depth.set(0);
        let _ = it.run_case(body);
        if hit() {
            reach.push((i, reach.len()));
        }
    }
    let f = if site.in_test { None } else { it.fns.get(&site.def).cloned() };
    let mut inputs: Vec<(Vec<Value>, usize)> = Vec::new();
    if let Some(f) = f.as_ref().filter(|f| it.skip_reason(f).is_none()) {
        let fo = fuzz::Options { cases: opts.fuzz, seed: opts.seed, edge: false };
        let mut rng = fuzz::Rng(opts.seed.max(1));
        let parts: Vec<Vec<Value>> = f.params.iter().map(|p| it.small(&p.ty, 0)).collect();
        let mut small = if parts.iter().all(|p| !p.is_empty()) { crate::suggest::product(&parts, opts.fuzz) } else { vec![] }.into_iter();
        let mut attempts = 0;
        while inputs.len() < opts.fuzz * 2 && attempts < opts.fuzz * 10 && now_ms() < t0 + opts.budget_ms as f64 * 0.25 {
            attempts += 1;
            let args = match small.next() {
                Some(a) => a,
                None => match f.params.iter().map(|p| it.generate(&p.ty, &mut rng, &fo, 0)).collect::<Option<Vec<_>>>() {
                    Some(a) => a,
                    None => continue,
                },
            };
            *it.output.borrow_mut() = Some(vec![]);
            it.fuel.set(fuzz::FUEL);
            it.depth.set(0);
            let _ = it.call_fn(f, args.clone());
            if hit() {
                inputs.push((args, reach.len() + inputs.len()));
            }
        }
    }
    let seen = it.hole.borrow_mut().probe.take().map(|p| p.seen).unwrap_or_default();
    let envs: Vec<Rc<Env>> = seen
        .into_iter()
        .map(|vars| {
            let env = Env::child(&it.globals);
            for (n, v) in vars {
                env.define(&n, v);
            }
            env
        })
        .collect();
    let trace = std::env::var_os("SSPUR_SYNTH_TRACE").is_some();
    if trace {
        eprintln!("probe: {} tests, {} inputs, {} envs, {:.1} ms", reach.len(), inputs.len(), envs.len(), now_ms() - t0);
    }
    filled.tests = reach.len();
    filled.inputs = inputs.len();
    // Enumerate.
    let comps = Rc::new(components(ctx, check, site));
    let fn_values: Vec<(String, Type, Type)> = ctx
        .sigs
        .iter()
        .filter(|s| s.kind == SigKind::Fn && s.params.len() == 1 && s.tparams.is_empty() && s.effects.is_empty() && s.name != site.def)
        .map(|s| (s.name.clone(), s.params[0].clone(), s.ret.clone()))
        .collect();
    let mut en = En {
        it,
        check,
        envs,
        terms: vec![],
        by_size: vec![],
        by_ty: HashMap::new(),
        by_head: HashMap::new(),
        seen: HashSet::new(),
        seen_obs: BTreeMap::new(),
        comps,
        lambda: false,
        lambda_size: opts.lambda_size,
        deadline: t0 + opts.budget_ms as f64 * 0.6,
        explored: 0,
        spans: spans.clone(),
        lambdas: HashMap::new(),
        fn_values,
        out_of_time: false,
        steps: 0,
        target: Some(site.ty.clone()),
        max: opts.max_size,
        memo: HashMap::new(),
        scratch: vec![],
    };
    en.scratch = scratch(&en.envs);
    *it.hole.borrow_mut() = HoleMode::default();
    let no_evidence = reach.is_empty() && inputs.is_empty();
    let contract = f.as_ref().and_then(|f| contract_of(it, f));
    let mut cands: Vec<Cand> = Vec::new();
    let mut ok = 0;
    let mut done = 0;
    en.deadline = t0 + opts.budget_ms as f64 * 0.9;
    // Grow one size at a time and validate the new terms of the hole's type before growing further:
    // once enough candidates pass, nothing larger can outrank them.
    for s in 1..=opts.max_size {
        if s == 1 {
            en.seed(site, &[]);
        } else {
            en.grow_level(s, site);
        }
        if trace {
            eprintln!("size {s}: {} classes, {} explored, {:.1} ms", en.terms.len(), en.explored, now_ms() - t0);
        }
        let mut level: Vec<usize> = (done..en.terms.len()).filter(|&i| en.terms[i].ty == site.ty && !en.terms[i].var).collect();
        done = en.terms.len();
        level.sort_by_key(|&i| (no_evidence && !en.terms[i].local, en.terms[i].size, en.terms[i].fns.len()));
        for i in level {
            if cands.len() >= 400 || now_ms() > en.deadline {
                en.out_of_time = true;
                break;
            }
            let c = validate(it, &en.terms[i], site, prior, &reach, &inputs, &cases, f.as_deref(), contract);
            if c.ok() && (!no_evidence || c.uses_input) {
                ok += 1;
            }
            cands.push(c);
        }
        if ok >= opts.top || en.out_of_time {
            break;
        }
    }
    filled.explored = en.explored;
    filled.classes = en.terms.len();
    filled.timed_out = en.out_of_time && ok < opts.top;
    *it.hole.borrow_mut() = HoleMode::default();
    cands.sort_by(|a, b| rank(a, no_evidence).cmp(&rank(b, no_evidence)).then_with(|| a.text.cmp(&b.text)));
    // The passing candidates, or else the one that comes closest.
    let any_ok = cands.iter().any(Cand::ok);
    for c in cands {
        if filled.cands.len() >= if any_ok { opts.top } else { 1 } {
            break;
        }
        if any_ok && !c.ok() || !any_ok && c.passed == 0 {
            continue;
        }
        if typechecks(m, site, &c.expr) {
            filled.cands.push(c);
        }
    }
    filled.ms = now_ms() - t0;
    filled
}

#[allow(clippy::too_many_arguments)]
fn validate(it: &Interp, t: &Term, site: &HoleSite, prior: &[(Span, Rc<Expr>)], reach: &[(usize, usize)], inputs: &[(Vec<Value>, usize)], cases: &[(String, Expr)], f: Option<&FnDef>, contract: Option<&'static str>) -> Cand {
    let mut c = Cand { text: printer::expr(&t.show, 0), expr: t.show.clone(), size: t.size, fns: t.fns.len(), passed: 0, failed: 0, inputs: inputs.len(), broke: 0, traps: 0, contract, proved: false, uses_input: t.local };
    let mut fills = prior.to_vec();
    fills.push((site.span, Rc::new(t.run.clone())));
    *it.hole.borrow_mut() = HoleMode { fills, probe: None };
    for &(case, env) in reach {
        if t.obs.get(env) == Some(&Obs::Trap) {
            c.failed += 1;
            continue;
        }
        it.fuel.set(RUN_FUEL);
        it.depth.set(0);
        match it.run_case(&cases[case].1) {
            Ok(()) => c.passed += 1,
            Err(_) => c.failed += 1,
        }
    }
    if let Some(f) = f.filter(|_| c.failed == 0) {
        for (args, env) in inputs {
            if t.obs.get(*env) == Some(&Obs::Trap) {
                c.traps += 1;
                continue;
            }
            *it.output.borrow_mut() = Some(vec![]);
            it.fuel.set(fuzz::FUEL);
            it.depth.set(0);
            match it.call_fn(f, args.clone()) {
                Err(Ctrl::Trap(m)) if m == crate::OUT_OF_FUEL => {}
                Err(Ctrl::Trap(m)) if m.starts_with("contract violated") || m.starts_with("result of") => c.broke += 1,
                Err(Ctrl::Trap(_)) => c.traps += 1,
                _ => {}
            }
        }
    }
    *it.output.borrow_mut() = Some(vec![]);
    *it.hole.borrow_mut() = HoleMode::default();
    c
}

fn rank(c: &Cand, no_evidence: bool) -> (bool, usize, usize, bool, usize, usize, bool) {
    (!c.ok(), c.failed, c.broke + c.traps, no_evidence && !c.uses_input, c.size, c.fns, !c.uses_input)
}

/// The module with the hole at `span` replaced by `e`.
pub fn substitute(m: &Module, span: Span, e: &Expr) -> Module {
    let mut m = m.clone();
    for d in m.defs.iter_mut() {
        let exprs: Vec<&mut Expr> = match d {
            Def::Fn(f) => vec![&mut f.body],
            Def::Test(t) => vec![&mut t.body],
            Def::Static(s) => vec![&mut s.init],
            Def::Impl(i) => i.fns.iter_mut().map(|f| &mut f.body).collect(),
            Def::Trait(t) => t.methods.iter_mut().map(|m| &mut m.sig.body).collect(),
            _ => vec![],
        };
        for x in exprs {
            visit::walk_expr_mut(x, &mut |x| {
                if x.span == span && matches!(x.kind, ExprKind::Hole(_)) {
                    *x = e.clone();
                }
            });
        }
    }
    m
}

/// What to write in place of the hole at `span` in `src`: `text`, or `(text)` where the bare
/// text would parse differently from the expression.
pub fn fill_text(src: &str, span: Span, text: &str, expr: &Expr) -> String {
    let (a, b) = (span.start as usize, (span.end as usize).min(src.len()));
    let Ok(m) = parse(src) else { return text.to_string() };
    let want = print_module(&substitute(&m, span, expr));
    match parse(&format!("{}{text}{}", &src[..a], &src[b..])) {
        Ok(m2) if print_module(&m2) == want => text.to_string(),
        _ => format!("({text})"),
    }
}

fn typechecks(m: &Module, site: &HoleSite, e: &Expr) -> bool {
    let m2 = substitute(m, site.span, e);
    let skip: HashSet<String> = m2.defs.iter().filter(|d| matches!(d, Def::Fn(_) | Def::Test(_)) && d.name() != site.def).map(|d| d.name().to_string()).collect();
    let out = sspur_check::check_skipping(&m2, &skip);
    !out.has_errors()
}
