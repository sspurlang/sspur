pub mod conv;
mod float;
mod z3;

pub use z3::{collecting, parse_model, solve_all, Answer, Solver};

use sspur_check::{expr_key, CheckOutput, Type};
use sspur_syntax::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

const MIN: &str = "(- 9223372036854775808)";
const MAX: &str = "9223372036854775807";
const LEN_MAX: &str = "140737488355328";
const GLOBALS: &[&str] = &["log", "some", "ok", "err", "empty_map", "min", "max", "secret", "pii", "untrusted", "guess"];
const PURE: &[&str] = &["len", "is_empty", "is_some", "is_none", "is_ok", "contains", "starts_with", "ends_with", "has", "abs", "to_f64", "str", "raw"];

pub type Key = (u32, u32, String);

#[derive(Clone, Debug)]
pub struct Query {
    pub fun: usize,
    upto: usize,
    extra: (usize, usize),
    path: Vec<String>,
    goal: String,
}

pub struct FnCtx {
    pub name: String,
    pub is_test: bool,
    pub params: Vec<(String, String, bool)>,
    decls: String,
    facts: Vec<String>,
    /// Loop invariants (float mode) every query of the function may assume.
    pub(crate) assumed: Vec<String>,
}

pub struct Site {
    pub func: String,
    pub usable: bool,
    pub queries: Vec<Option<Query>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClauseKind {
    Where(String),
    Alias(String, String),
    Pre,
    Post,
    Ret(String),
}

pub struct Obligation {
    pub caller: usize,
    pub at: u32,
    pub query: Option<Query>,
}

pub struct Clause {
    pub func: String,
    pub kind: ClauseKind,
    pub text: String,
    pub obligations: Vec<Obligation>,
}

#[derive(Default)]
pub struct Analysis {
    pub fns: Vec<FnCtx>,
    pub sites: HashMap<Key, Site>,
    pub clauses: Vec<Clause>,
    posts: HashMap<(String, usize), usize>,
    rets: HashMap<String, usize>,
    usable_fns: HashSet<String>,
    pub(crate) cands: Vec<float::Cand>,
}

impl Analysis {
    pub fn script(&self, q: &Query) -> String {
        let f = &self.fns[q.fun];
        let mut s = f.decls.clone();
        for t in f.facts[..q.upto].iter().chain(&f.facts[q.extra.0..q.extra.1]) {
            writeln!(s, "(assert {t})").unwrap();
        }
        for t in f.assumed.iter().chain(&q.path) {
            writeln!(s, "(assert {t})").unwrap();
        }
        writeln!(s, "(assert (not {}))", q.goal).unwrap();
        s
    }

    pub fn model_vars(&self, q: &Query) -> Vec<String> {
        self.fns[q.fun].params.iter().map(|(_, s, _)| s.clone()).collect()
    }

    pub fn post_clause(&self, f: &str, i: usize) -> Option<&Clause> {
        self.posts.get(&(f.to_string(), i)).map(|&c| &self.clauses[c])
    }

    pub fn ret_clause(&self, f: &str) -> Option<&Clause> {
        self.rets.get(f).map(|&c| &self.clauses[c])
    }

    pub fn usable(&self, f: &str) -> bool {
        self.usable_fns.contains(f)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Val {
    Int(String),
    Bool(String),
    Rec(usize),
    Tup(Vec<Val>),
    List(String),
    /// An `F64` in float mode: class and real (see `float.rs`).
    F(String, String),
    Other,
    Never,
}

#[derive(Clone)]
enum Bind {
    Val(Val),
    Lazy,
    Mutable,
    /// A `var` in float mode, with its value until the next assignment or loop.
    Var(Option<Val>),
}

struct Goal {
    base: usize,
    obls: Vec<String>,
    ok: bool,
}

struct Obj {
    owner: Option<String>,
    fields: HashMap<String, Val>,
}

type Conds<'a> = Vec<(Option<usize>, &'a Expr)>;

struct Enc<'a> {
    check: &'a CheckOutput,
    fns: HashMap<&'a str, &'a FnDef>,
    aliases: HashMap<&'a str, &'a Expr>,
    frefs: HashMap<String, HashMap<String, Vec<&'a Expr>>>,
    dups: HashSet<(u32, u32, u8)>,
    an: Analysis,
    entry_clause: HashMap<(String, usize), usize>,
    fidx: usize,
    fname: String,
    usable: bool,
    floats: bool,
    /// Float operations by operands, so the same expression rounds the same way twice.
    fmemo: HashMap<String, Val>,
    /// The `to_f64` results so far (Int term, real), for monotonicity.
    fints: Vec<(String, String)>,
    /// Real literals made by `f_lit`, by term, for constant folding.
    flits: HashMap<String, f64>,
    /// The loops being walked (float mode): their `F64` vars and invariant candidates.
    frames: Vec<HashMap<String, Vec<usize>>>,
    roots: Vec<&'a Expr>,
    /// The calls being inlined (float mode): callee names, and the call spans as a site-kind suffix.
    inlined: Vec<String>,
    chain: String,
    budget: usize,
    decls: String,
    facts: Vec<String>,
    path: Vec<String>,
    env: Vec<HashMap<String, Bind>>,
    objs: Vec<Obj>,
    n: usize,
    goals: Vec<Goal>,
    exits: Vec<(usize, Vec<String>, Val)>,
    returns: usize,
}

fn count_returns(e: &Expr) -> usize {
    let mut n = 0;
    visit::walk_expr(e, &mut |x| {
        n += usize::from(matches!(x.kind, ExprKind::Return(_)));
        true
    });
    n
}

fn lit(n: i128) -> String {
    if n < 0 { format!("(- {})", -n) } else { n.to_string() }
}

fn int_lit(t: &str) -> Option<i64> {
    match t.strip_prefix("(- ").and_then(|x| x.strip_suffix(')')) {
        Some(x) => x.parse::<i64>().ok().map(|k| -k),
        None => t.parse().ok(),
    }
}

fn in_range(t: &str) -> String {
    format!("(and (<= {MIN} {t}) (<= {t} {MAX}))")
}

fn and(ts: &[String]) -> String {
    match ts {
        [] => "true".into(),
        [t] => t.clone(),
        _ => format!("(and {})", ts.join(" ")),
    }
}

fn is_ty(t: Option<&Type>, name: &str) -> bool {
    matches!(t, Some(Type::Con(n, a)) if n == name && a.is_empty())
}

fn ty_name(t: &Ty) -> Option<&str> {
    match t {
        Ty::Named { name, .. } => Some(name),
        _ => None,
    }
}

fn kind_tag(k: &ExprKind) -> u8 {
    match k {
        ExprKind::Int(_) => 10,
        ExprKind::Float(_) => 11,
        ExprKind::Str(_) => 12,
        ExprKind::Bool(_) => 13,
        ExprKind::Unit => 14,
        ExprKind::Name(_) => 15,
        ExprKind::Hole(_) => 16,
        ExprKind::Placeholder => 17,
        ExprKind::Field(..) => 18,
        ExprKind::Call(..) => 19,
        ExprKind::Method { .. } => 20,
        ExprKind::Index(..) => 21,
        ExprKind::Lambda { .. } => 22,
        ExprKind::Binary(..) => 23,
        ExprKind::Unary(..) => 24,
        ExprKind::Range(..) => 25,
        ExprKind::If(..) => 26,
        ExprKind::Match(..) => 27,
        ExprKind::Catch(..) => 28,
        ExprKind::Block(_) => 29,
        ExprKind::Record { .. } => 30,
        ExprKind::List(_) => 31,
        ExprKind::Tuple(_) => 32,
        ExprKind::Par(_) => 33,
        ExprKind::Raise(_) => 34,
        ExprKind::Return(_) => 35,
        ExprKind::With(..) => 36,
        ExprKind::Table(_) => 37,
        ExprKind::Handle(..) => 38,
    }
}

fn count_spans(m: &Module) -> HashSet<(u32, u32, u8)> {
    let mut seen = HashSet::new();
    let mut dups = HashSet::new();
    let mut visit = |e: &Expr| {
        visit::walk_expr(e, &mut |x| {
            let k = (x.span.start, x.span.end, kind_tag(&x.kind));
            if !seen.insert(k) {
                dups.insert(k);
            }
            true
        })
    };
    for d in &m.defs {
        match d {
            Def::Fn(f) => {
                for p in &f.params {
                    if let Some(r) = &p.refine {
                        visit(r);
                    }
                }
                f.pres.iter().chain(&f.posts).chain(&f.examples).chain([&f.body]).for_each(&mut visit);
            }
            Def::Test(t) => visit(&t.body),
            Def::Effect(_) | Def::Store(_) | Def::Svc(_) | Def::Static(_) | Def::Use(_) | Def::Trait(_) | Def::Impl(_) => {}
            Def::Type(t) => match &t.body {
                TypeBody::Alias(_, Some(r)) => visit(r),
                TypeBody::Record(fs) => fs.iter().filter_map(|f| f.refine.as_ref()).for_each(&mut visit),
                TypeBody::Sum(vs) => vs.iter().flat_map(|v| v.fields.iter().flatten()).filter_map(|f| f.refine.as_ref()).for_each(&mut visit),
                _ => {}
            },
        }
    }
    dups
}

pub fn analyze(m: &Module, check: &CheckOutput) -> Analysis {
    let (mut enc, defs) = encoder(m, check, false);
    for f in &defs {
        enc.function(f);
    }
    for f in &defs {
        for (i, ex) in f.examples.iter().enumerate() {
            enc.test(&format!("{}.ex{}", f.name, i + 1), ex);
        }
    }
    for d in &m.defs {
        if let Def::Test(t) = d {
            enc.test(&t.name, &t.body);
        }
    }
    enc.an
}

fn encoder<'a>(m: &'a Module, check: &'a CheckOutput, floats: bool) -> (Enc<'a>, Vec<&'a FnDef>) {
    let mut enc = Enc {
        check,
        fns: HashMap::new(),
        aliases: HashMap::new(),
        frefs: HashMap::new(),
        dups: if floats { HashSet::new() } else { count_spans(m) },
        an: Analysis::default(),
        entry_clause: HashMap::new(),
        fidx: 0,
        fname: String::new(),
        usable: false,
        floats,
        fmemo: HashMap::new(),
        fints: vec![],
        flits: HashMap::new(),
        frames: vec![],
        roots: vec![],
        inlined: vec![],
        chain: String::new(),
        budget: 0,
        decls: String::new(),
        facts: vec![],
        path: vec![],
        env: vec![],
        objs: vec![],
        n: 0,
        goals: vec![],
        exits: vec![],
        returns: 0,
    };
    for d in &m.defs {
        match d {
            Def::Fn(f) => {
                enc.fns.insert(&f.name, f);
            }
            Def::Type(TypeDef { name, body: TypeBody::Alias(_, Some(r)), .. }) => {
                enc.aliases.insert(name, r);
            }
            _ => {}
        }
    }
    for d in &m.defs {
        let Def::Type(t) = d else { continue };
        let owners: Vec<(&String, &Vec<Field>)> = match &t.body {
            TypeBody::Record(fs) => vec![(&t.name, fs)],
            TypeBody::Sum(vs) => vs.iter().filter_map(|v| v.fields.as_ref().map(|fs| (&v.name, fs))).collect(),
            _ => vec![],
        };
        for (owner, fs) in owners {
            let mut map = HashMap::new();
            for f in fs {
                let conds: Vec<&Expr> = f.refine.iter().chain(ty_name(&f.ty).and_then(|n| enc.aliases.get(n).copied())).collect();
                if !conds.is_empty() {
                    map.insert(f.name.clone(), conds);
                }
            }
            enc.frefs.insert(owner.clone(), map);
        }
    }
    let defs: Vec<&FnDef> = m.defs.iter().filter_map(|d| if let Def::Fn(f) = d { Some(f) } else { None }).collect();
    for f in &defs {
        for (j, (i, c)) in enc.entry_conds(f).into_iter().enumerate() {
            let kind = match i {
                Some(i) => {
                    let p = &f.params[i];
                    match &p.refine {
                        Some(r) if std::ptr::eq(r, c) => ClauseKind::Where(p.name.clone()),
                        _ => ClauseKind::Alias(ty_name(&p.ty).unwrap_or("").to_string(), p.name.clone()),
                    }
                }
                None => ClauseKind::Pre,
            };
            enc.entry_clause.insert((f.name.clone(), j), enc.an.clauses.len());
            enc.an.clauses.push(Clause { func: f.name.clone(), kind, text: if floats { String::new() } else { printer::expr(c, 0) }, obligations: vec![] });
        }
        for (i, p) in f.posts.iter().enumerate() {
            enc.an.posts.insert((f.name.clone(), i), enc.an.clauses.len());
            enc.an.clauses.push(Clause { func: f.name.clone(), kind: ClauseKind::Post, text: if floats { String::new() } else { printer::expr(p, 0) }, obligations: vec![] });
        }
        if let Some(rt) = &f.ret
            && let Some(n) = ty_name(rt)
            && let Some(r) = enc.aliases.get(n) {
                enc.an.rets.insert(f.name.clone(), enc.an.clauses.len());
                enc.an.clauses.push(Clause { func: f.name.clone(), kind: ClauseKind::Ret(n.to_string()), text: if floats { String::new() } else { printer::expr(r, 0) }, obligations: vec![] });
            }
    }
    (enc, defs)
}

impl<'a> Enc<'a> {
    fn entry_conds(&self, f: &'a FnDef) -> Conds<'a> {
        let mut out = Vec::new();
        for (i, p) in f.params.iter().enumerate() {
            if let Some(r) = ty_name(&p.ty).and_then(|n| self.aliases.get(n)) {
                out.push((Some(i), *r));
            }
            if let Some(r) = &p.refine {
                out.push((Some(i), r));
            }
        }
        out.extend(f.pres.iter().map(|p| (None, p)));
        out
    }

    fn reset(&mut self, name: &str, usable: bool) {
        self.fidx = self.an.fns.len();
        self.fname = name.to_string();
        self.usable = usable;
        self.decls.clear();
        if self.floats {
            self.decls.push_str(&float::prelude());
            self.fmemo.clear();
            self.fints.clear();
            self.budget = float::INLINE_BUDGET;
        }
        self.facts.clear();
        self.path.clear();
        self.env.clear();
        self.objs.clear();
        self.goals.clear();
        self.exits.clear();
        self.returns = 0;
    }

    fn finish(&mut self, is_test: bool, params: Vec<(String, String, bool)>) {
        self.an.fns.push(FnCtx { name: self.fname.clone(), is_test, params, decls: std::mem::take(&mut self.decls), facts: std::mem::take(&mut self.facts), assumed: vec![] });
    }

    fn test(&mut self, name: &str, body: &'a Expr) {
        self.reset(name, false);
        self.roots = vec![body];
        self.env.push(HashMap::new());
        self.ex(body);
        self.finish(true, vec![]);
    }

    fn function(&mut self, f: &'a FnDef) {
        let usable = f.tparams.is_empty() && !matches!(f.body.kind, ExprKind::Table(_));
        if usable {
            self.an.usable_fns.insert(f.name.clone());
        }
        self.reset(&f.name, usable);
        let ptys = self.check.fn_types.get(&f.name).map(|(p, _)| p.clone()).unwrap_or_default();
        let mut scope = HashMap::new();
        let mut vals = Vec::new();
        let mut model = Vec::new();
        for (i, p) in f.params.iter().enumerate() {
            let v = self.fresh_of(ptys.get(i));
            match &v {
                Val::Int(s) => model.push((p.name.clone(), s.clone(), false)),
                Val::Bool(s) => model.push((p.name.clone(), s.clone(), true)),
                _ => {}
            }
            scope.insert(p.name.clone(), Bind::Val(v.clone()));
            vals.push((p.name.clone(), v));
        }
        self.env.push(scope);
        for (i, c) in self.entry_conds(f) {
            let mut binds = vals.clone();
            if let Some(i) = i {
                binds.push(("_".into(), vals[i].1.clone()));
            }
            if let Some(t) = self.goal_with(binds, c) {
                self.facts.push(t);
            }
        }
        self.roots = vec![&f.body];
        let v = match &f.body.kind {
            ExprKind::Table(_) => Val::Other,
            _ => self.ex(&f.body),
        };
        self.exits.push((self.facts.len(), vec![], v));
        let hidden = self.returns != count_returns(&f.body);
        let exits: Vec<_> = self.exits.iter().filter(|x| x.2 != Val::Never).cloned().collect();
        for (i, post) in f.posts.iter().enumerate() {
            let ci = self.an.posts[&(f.name.clone(), i)];
            for (k, path, v) in &exits {
                let mut binds = vals.clone();
                binds.push(("r".into(), v.clone()));
                let g0 = self.facts.len();
                let goal = self.goal_with(binds, post);
                let q = goal.filter(|_| !hidden).map(|goal| Query { fun: self.fidx, upto: *k, extra: (g0, self.facts.len()), path: path.clone(), goal });
                self.an.clauses[ci].obligations.push(Obligation { caller: self.fidx, at: f.span.start, query: q });
            }
        }
        if let Some(&ci) = self.an.rets.get(&f.name) {
            let r = self.aliases[ty_name(f.ret.as_ref().unwrap()).unwrap()];
            for (k, path, v) in &exits {
                let g0 = self.facts.len();
                let goal = self.goal_with(vec![("_".into(), v.clone())], r);
                let q = goal.filter(|_| !hidden).map(|goal| Query { fun: self.fidx, upto: *k, extra: (g0, self.facts.len()), path: path.clone(), goal });
                self.an.clauses[ci].obligations.push(Obligation { caller: self.fidx, at: f.span.start, query: q });
            }
        }
        self.finish(false, model);
    }

    fn sym(&mut self, sort: &str) -> String {
        self.n += 1;
        let s = format!("k{}", self.n);
        writeln!(self.decls, "(declare-const {s} {sort})").unwrap();
        s
    }

    fn int_sym(&mut self, lo: &str, hi: &str) -> String {
        let s = self.sym("Int");
        self.facts.push(format!("(and (<= {lo} {s}) (<= {s} {hi}))"));
        s
    }

    fn define(&mut self, sort: &str, t: String) -> String {
        let s = self.sym(sort);
        self.facts.push(format!("(= {s} {t})"));
        s
    }

    fn path_from(&self, base: usize) -> String {
        and(&self.path[base..])
    }

    fn guarded(&self, t: &str) -> String {
        if self.path.is_empty() { t.to_string() } else { format!("(=> {} {t})", self.path_from(0)) }
    }

    fn assume(&mut self, t: &str) {
        if self.goals.is_empty() {
            let g = self.guarded(t);
            self.facts.push(g);
        }
    }

    fn fail(&mut self) {
        if let Some(g) = self.goals.last_mut() {
            g.ok = false;
        }
    }

    fn checkpoint(&mut self, e: &Expr, kind: &str, cond: Option<String>) {
        if let Some(g) = self.goals.last() {
            let base = g.base;
            match cond {
                Some(c) => {
                    let o = if self.path.len() > base { format!("(=> {} {c})", self.path_from(base)) } else { c };
                    self.goals.last_mut().unwrap().obls.push(o);
                }
                None => self.fail(),
            }
            return;
        }
        let usable = self.usable && !self.dups.contains(&(e.span.start, e.span.end, kind_tag(&e.kind)));
        let q = cond.map(|goal| Query { fun: self.fidx, upto: self.facts.len(), extra: (0, 0), path: self.path.clone(), goal });
        let site = self.an.sites.entry((e.span.start, e.span.end, format!("{kind}{}", self.chain))).or_insert(Site { func: self.fname.clone(), usable, queries: vec![] });
        site.usable &= usable;
        site.queries.push(q);
    }

    fn goal_with(&mut self, binds: Vec<(String, Val)>, c: &'a Expr) -> Option<String> {
        let scope: HashMap<String, Bind> = binds.into_iter().map(|(k, v)| (k, Bind::Val(v))).collect();
        let saved = std::mem::replace(&mut self.env, vec![scope]);
        self.goals.push(Goal { base: self.path.len(), obls: vec![], ok: true });
        let v = self.ex(c);
        let g = self.goals.pop().unwrap();
        self.env = saved;
        match (g.ok, v) {
            (true, Val::Bool(t)) => {
                let mut all = g.obls;
                all.push(t);
                Some(and(&all))
            }
            _ => None,
        }
    }

    fn ty(&self, e: &Expr) -> Option<&'a Type> {
        self.check.expr_types.get(&expr_key(e))
    }

    fn fresh_of(&mut self, t: Option<&Type>) -> Val {
        match t {
            Some(Type::Con(n, a)) if a.is_empty() && n == "Int" => Val::Int(self.int_sym(MIN, MAX)),
            Some(Type::Con(n, a)) if a.is_empty() && n == "Bool" => Val::Bool(self.sym("Bool")),
            Some(Type::Con(n, a)) if a.is_empty() && n == "F64" && self.floats => self.f_fresh(),
            Some(Type::Con(n, a)) if a.len() == 1 && n == "List" => Val::List(self.int_sym("0", LEN_MAX)),
            Some(Type::Tuple(ts)) => Val::Tup(ts.iter().map(|t| self.fresh_of(Some(t))).collect()),
            Some(Type::Con(n, _)) if self.check.records.contains_key(n) => {
                self.objs.push(Obj { owner: Some(n.clone()), fields: HashMap::new() });
                Val::Rec(self.objs.len() - 1)
            }
            _ => Val::Other,
        }
    }

    fn fresh_e(&mut self, e: &Expr) -> Val {
        let t = self.ty(e);
        self.fresh_of(t)
    }

    fn bool_of(&mut self, v: Val) -> String {
        match v {
            Val::Bool(t) => t,
            _ => self.sym("Bool"),
        }
    }

    fn bound(&self, n: &str) -> bool {
        self.env.iter().any(|s| s.contains_key(n))
    }

    fn name(&mut self, n: &str, e: &Expr) -> Val {
        let Some(i) = self.env.iter().rposition(|s| s.contains_key(n)) else { return self.fresh_e(e) };
        match self.env[i][n].clone() {
            Bind::Val(v) => v,
            Bind::Mutable => self.fresh_e(e),
            Bind::Var(Some(v)) => v,
            Bind::Var(None) => {
                let v = if self.tracked(n) { self.f_invariant(n) } else { self.fresh_e(e) };
                self.env[i].insert(n.to_string(), Bind::Var(Some(v.clone())));
                v
            }
            Bind::Lazy => {
                let v = self.fresh_e(e);
                self.env[i].insert(n.to_string(), Bind::Val(v.clone()));
                v
            }
        }
    }

    fn bind_pat(&mut self, p: &Pat, v: Option<Val>) {
        match (p, v) {
            (Pat::Bind(n), Some(v)) => {
                self.env.last_mut().unwrap().insert(n.clone(), Bind::Val(v));
            }
            (Pat::Bind(n), None) => {
                self.env.last_mut().unwrap().insert(n.clone(), Bind::Lazy);
            }
            (Pat::Tuple(ps), Some(Val::Tup(vs))) if ps.len() == vs.len() => {
                for (p, v) in ps.iter().zip(vs) {
                    self.bind_pat(p, Some(v));
                }
            }
            (Pat::Tuple(ps), _) => ps.iter().for_each(|p| self.bind_pat(p, None)),
            (Pat::Ctor { args: CtorArgs::Positional(ps), .. }, _) => ps.iter().for_each(|p| self.bind_pat(p, None)),
            (Pat::Ctor { args: CtorArgs::Record(fs), .. }, _) => fs.iter().for_each(|(_, p)| self.bind_pat(p, None)),
            (p @ (Pat::Or(_) | Pat::List { .. }), _) => {
                let mut names = Vec::new();
                p.binds(&mut names);
                for n in names {
                    self.env.last_mut().unwrap().insert(n, Bind::Lazy);
                }
            }
            _ => {}
        }
    }

    fn read_field(&mut self, id: usize, f: &str, e: &Expr) -> Val {
        if let Some(v) = self.objs[id].fields.get(f) {
            return v.clone();
        }
        let v = self.fresh_e(e);
        self.objs[id].fields.insert(f.to_string(), v.clone());
        let conds = self.objs[id].owner.as_ref().and_then(|o| self.frefs.get(o)).and_then(|m| m.get(f)).cloned().unwrap_or_default();
        for c in conds {
            if let Some(t) = self.goal_with(vec![("_".into(), v.clone())], c) {
                self.facts.push(t);
            }
        }
        v
    }

    fn merge(&mut self, c: &str, a: Val, b: Val, e: &Expr) -> Val {
        match (a, b) {
            (a, b) if a == b => a,
            (Val::Never, x) | (x, Val::Never) => x,
            (Val::Int(x), Val::Int(y)) => Val::Int(self.define("Int", format!("(ite {c} {x} {y})"))),
            (Val::Bool(x), Val::Bool(y)) => Val::Bool(self.define("Bool", format!("(ite {c} {x} {y})"))),
            (Val::List(x), Val::List(y)) => Val::List(self.define("Int", format!("(ite {c} {x} {y})"))),
            (Val::F(cx, rx), Val::F(cy, ry)) => Val::F(self.define("Int", format!("(ite {c} {cx} {cy})")), self.define("Real", format!("(ite {c} {rx} {ry})"))),
            (Val::Tup(xs), Val::Tup(ys)) if xs.len() == ys.len() => Val::Tup(xs.into_iter().zip(ys).map(|(x, y)| self.merge(c, x, y, e)).collect()),
            _ => self.fresh_e(e),
        }
    }

    fn under<T>(&mut self, c: String, f: impl FnOnce(&mut Self) -> T) -> T {
        self.path.push(c);
        let r = f(self);
        self.path.pop();
        r
    }

    fn guard<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let g = self.sym("Bool");
        self.under(g, f)
    }

    fn dead(&mut self) {
        if self.goals.is_empty() {
            let p = self.path_from(0);
            self.facts.push(format!("(not {p})"));
        }
    }

    fn ex(&mut self, e: &'a Expr) -> Val {
        match &e.kind {
            ExprKind::Int(n) => {
                if self.ty(e).is_none_or(|t| is_ty(Some(t), "Int")) {
                    Val::Int(lit(*n as i128))
                } else {
                    Val::Other
                }
            }
            ExprKind::Bool(b) => Val::Bool(b.to_string()),
            ExprKind::Float(x) if self.floats => self.f_lit(*x),
            ExprKind::Float(_) | ExprKind::Unit => Val::Other,
            ExprKind::Str(parts) => {
                for p in parts {
                    if let StrPart::Expr(x) = p {
                        self.ex(x);
                    }
                }
                Val::Other
            }
            ExprKind::Hole(_) => {
                self.fail();
                self.fresh_e(e)
            }
            ExprKind::Name(n) => self.name(n, e),
            ExprKind::Placeholder => self.name("_", e),
            ExprKind::Lambda { params, body, .. } => {
                if self.floats && self.goals.is_empty() {
                    let ps = match self.ty(e) {
                        Some(Type::Fn(ps, ..)) => ps.clone(),
                        _ => vec![],
                    };
                    let scope = params.iter().enumerate().map(|(i, p)| (p.clone(), Bind::Val(self.fresh_of(ps.get(i))))).collect();
                    self.forget_vars();
                    self.env.push(scope);
                    self.guard(|s| s.ex(body));
                    self.env.pop();
                    self.forget_vars();
                }
                Val::Other
            }
            ExprKind::Field(x, f) => self.field(e, x, f),
            ExprKind::Call(f, args) => self.call_expr(e, f, args),
            ExprKind::Method { recv, name, args, .. } => self.method(e, recv, name, args),
            ExprKind::Index(a, i) => {
                let (av, iv) = (self.ex(a), self.ex(i));
                match (av, iv) {
                    (Val::List(l), Val::Int(i)) => {
                        let c = format!("(and (<= 0 {i}) (< {i} {l}))");
                        self.checkpoint(e, "index", Some(c.clone()));
                        self.assume(&c);
                    }
                    _ => self.fail(),
                }
                self.fresh_e(e)
            }
            ExprKind::Binary(op @ (BinOp::And | BinOp::Or), a, b) => {
                let av = self.ex(a);
                let x = self.bool_of(av);
                let c = if *op == BinOp::And { x.clone() } else { format!("(not {x})") };
                let bv = self.under(c, |s| s.ex(b));
                let y = self.bool_of(bv);
                Val::Bool(format!("({} {x} {y})", if *op == BinOp::And { "and" } else { "or" }))
            }
            ExprKind::Binary(op, a, b) => {
                let (av, bv) = (self.ex(a), self.ex(b));
                self.binary(e, *op, av, bv)
            }
            ExprKind::Unary(UnOp::Not, x) => {
                let v = self.ex(x);
                let t = self.bool_of(v);
                Val::Bool(format!("(not {t})"))
            }
            ExprKind::Unary(UnOp::Ref | UnOp::RefMut, x) => self.ex(x),
            ExprKind::Unary(UnOp::Neg, x) => match self.ex(x) {
                Val::F(c, r) => match self.flits.get(&r).filter(|_| c == "0") {
                    Some(&v) => self.f_lit(-v),
                    None => self.f_neg(&c, &r),
                },
                Val::Int(t) if is_ty(self.ty(e), "Int") => {
                    let c = format!("(not (= {t} {MIN}))");
                    let v = self.define("Int", format!("(- {t})"));
                    self.checkpoint(e, "neg", Some(c.clone()));
                    self.assume(&c);
                    Val::Int(v)
                }
                _ => {
                    if is_ty(self.ty(e), "Int") {
                        self.fail();
                    }
                    self.fresh_e(e)
                }
            },
            ExprKind::Range(a, b) => {
                let (av, bv) = (self.ex(a), self.ex(b));
                match (av, bv) {
                    (Val::Int(x), Val::Int(y)) => Val::List(self.define("Int", format!("(ite (> {y} {x}) (- {y} {x}) 0)"))),
                    _ => self.fresh_e(e),
                }
            }
            ExprKind::If(c, a, b) => {
                let cv = self.ex(c);
                let ct = self.bool_of(cv);
                let av = self.under(ct.clone(), |s| s.ex(a));
                let bv = match b {
                    Some(b) => self.under(format!("(not {ct})"), |s| s.ex(b)),
                    None => Val::Other,
                };
                self.merge(&ct, av, bv, e)
            }
            ExprKind::Match(s, arms, _) => {
                if !self.goals.is_empty() {
                    self.fail();
                    return self.fresh_e(e);
                }
                let sv = self.ex(s);
                let r = self.fresh_e(e);
                let mut gs = Vec::new();
                for arm in arms {
                    let g = self.sym("Bool");
                    gs.push(g.clone());
                    let mut conds = vec![g];
                    match (&arm.pat, &sv) {
                        (Pat::Int(k), Val::Int(t)) => conds.push(format!("(= {t} {})", lit(*k as i128))),
                        (Pat::Bool(b), Val::Bool(t)) => conds.push(format!("(= {t} {b})")),
                        _ => {}
                    }
                    self.env.push(HashMap::new());
                    let pv = matches!(arm.pat, Pat::Bind(_) | Pat::Tuple(_)).then(|| sv.clone());
                    self.bind_pat(&arm.pat, pv);
                    let depth = self.path.len();
                    self.path.push(and(&conds));
                    if let Some(gd) = &arm.guard {
                        let gv = self.ex(gd);
                        let gt = self.bool_of(gv);
                        self.path.push(gt);
                    }
                    let v = self.ex(&arm.body);
                    match (&r, v) {
                        (Val::Int(x), Val::Int(y)) | (Val::Bool(x), Val::Bool(y)) => {
                            let f = format!("(= {x} {y})");
                            self.assume(&f);
                        }
                        (Val::F(cx, rx), Val::F(cy, ry)) => {
                            let f = format!("(and (= {cx} {cy}) (= {rx} {ry}))");
                            self.assume(&f);
                        }
                        _ => {}
                    }
                    self.path.truncate(depth);
                    self.env.pop();
                }
                let any = format!("(or false {})", gs.join(" "));
                self.assume(&any);
                r
            }
            ExprKind::Catch(b, arms) | ExprKind::Handle(b, arms) if self.floats && self.goals.is_empty() => {
                self.guard(|s| s.ex(b));
                for arm in arms {
                    self.env.push(HashMap::new());
                    self.bind_pat(&arm.pat, None);
                    self.guard(|s| {
                        if let Some(g) = &arm.guard {
                            s.ex(g);
                        }
                        s.ex(&arm.body)
                    });
                    self.env.pop();
                }
                self.fresh_e(e)
            }
            ExprKind::Catch(..) | ExprKind::Table(_) | ExprKind::Handle(..) => {
                self.fail();
                self.fresh_e(e)
            }
            ExprKind::Block(stmts) => {
                if !self.goals.is_empty() {
                    self.fail();
                    return self.fresh_e(e);
                }
                self.block(stmts)
            }
            ExprKind::Record { ctor, fields } => self.record(e, ctor, fields),
            ExprKind::List(xs) => {
                for x in xs {
                    self.ex(x);
                }
                Val::List(xs.len().to_string())
            }
            ExprKind::Tuple(xs) => Val::Tup(xs.iter().map(|x| self.ex(x)).collect()),
            ExprKind::Par(xs) => {
                for x in xs {
                    self.guard(|s| s.ex(x));
                }
                self.fresh_e(e)
            }
            ExprKind::Raise(x) | ExprKind::Return(x) => {
                if !self.goals.is_empty() {
                    self.fail();
                    return self.fresh_e(e);
                }
                let v = self.ex(x);
                if matches!(e.kind, ExprKind::Return(_)) {
                    self.returns += 1;
                    self.exits.push((self.facts.len(), self.path.clone(), v));
                }
                self.dead();
                Val::Never
            }
            ExprKind::With(base, ups) => {
                self.fail();
                self.ex(base);
                for (segs, v) in ups {
                    self.guard(|s| {
                        for seg in segs {
                            if let PathSeg::Index(i) = seg {
                                s.ex(i);
                            }
                        }
                        s.ex(v);
                    });
                }
                self.fresh_e(e)
            }
        }
    }

    fn binary(&mut self, e: &Expr, op: BinOp, av: Val, bv: Val) -> Val {
        let cmp = match op {
            BinOp::Lt => Some("<"),
            BinOp::Le => Some("<="),
            BinOp::Gt => Some(">"),
            BinOp::Ge => Some(">="),
            BinOp::Eq | BinOp::Ne => Some("="),
            _ => None,
        };
        if let (Val::F(ca, ra), Val::F(cb, rb)) = (&av, &bv) {
            if cmp.is_some() {
                return Val::Bool(self.f_cmp(op, (ca, ra), (cb, rb)));
            }
            if is_ty(self.ty(e), "F64") {
                return self.f_arith(op, (ca.clone(), ra.clone()), (cb.clone(), rb.clone()));
            }
        }
        if let Some(sym) = cmp {
            let t = match (&av, &bv) {
                (Val::Int(x), Val::Int(y)) => format!("({sym} {x} {y})"),
                (Val::Bool(x), Val::Bool(y)) if sym == "=" => format!("(= {x} {y})"),
                _ => return Val::Bool(self.sym("Bool")),
            };
            return Val::Bool(if op == BinOp::Ne { format!("(not {t})") } else { t });
        }
        let (Val::Int(x), Val::Int(y), true) = (&av, &bv, is_ty(self.ty(e), "Int")) else {
            if is_ty(self.ty(e), "Int") {
                self.fail();
            }
            return self.fresh_e(e);
        };
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul => {
                let o = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    _ => "*",
                };
                let v = self.define("Int", format!("({o} {x} {y})"));
                let c = in_range(&v);
                self.checkpoint(e, "arith", Some(c.clone()));
                self.assume(&c);
                Val::Int(v)
            }
            BinOp::Div | BinOp::Rem => {
                let c = format!("(and (not (= {y} 0)) (not (and (= {x} {MIN}) (= {y} (- 1)))))");
                let q = self.define("Int", format!("(ite (or (>= {x} 0) (= (mod {x} {y}) 0)) (div {x} {y}) (ite (> {y} 0) (+ (div {x} {y}) 1) (- (div {x} {y}) 1)))"));
                let v = if op == BinOp::Div { q } else { self.define("Int", format!("(- {x} (* {y} {q}))")) };
                self.checkpoint(e, "arith", Some(c.clone()));
                self.assume(&c);
                Val::Int(v)
            }
            _ => {
                self.fail();
                Val::Int(self.int_sym(MIN, MAX))
            }
        }
    }

    fn field(&mut self, e: &'a Expr, x: &'a Expr, f: &str) -> Val {
        if self.check.user_methods.contains(&(e.span.start, e.span.end)) {
            let v = self.ex(x);
            return match self.fns.get(f).copied() {
                Some(g) => self.user_call(e, g, vec![v]),
                None => {
                    self.fail();
                    self.fresh_e(e)
                }
            };
        }
        let xv = self.ex(x);
        match (&xv, f) {
            (Val::Rec(id), _) if self.check.records.get(self.objs[*id].owner.as_deref().unwrap_or("")).is_some_and(|(_, fs)| fs.iter().any(|(n, _)| n == f)) => self.read_field(*id, f, e),
            (Val::Tup(vs), _) if f.parse::<usize>().is_ok_and(|i| i < vs.len()) => vs[f.parse::<usize>().unwrap()].clone(),
            _ => self.builtin_method(e, x, xv, f, vec![]),
        }
    }

    fn builtin_method(&mut self, e: &Expr, recv: &Expr, rv: Val, name: &str, args: Vec<Val>) -> Val {
        let rt = self.ty(recv);
        let is_field = matches!(rt, Some(Type::Con(n, _)) if self.check.records.get(n).is_some_and(|(_, fs)| fs.iter().any(|(f, _)| f == name)));
        if !is_field && !PURE.contains(&name) && !(self.floats && float::FPURE.contains(&name)) && name.parse::<usize>().is_err() {
            self.fail();
        }
        if self.floats && !is_field {
            match (&rv, name) {
                (Val::F(c, r), _) if is_ty(rt, "F64") => {
                    let (c, r) = (c.clone(), r.clone());
                    if let Some(v) = self.f_method(e, name, &c, &r, &args) {
                        return v;
                    }
                }
                (Val::Int(n), "to_f64") if args.is_empty() => {
                    let n = n.clone();
                    return match int_lit(&n) {
                        Some(k) => self.f_lit(k as f64),
                        None => self.f_of_int(&n),
                    };
                }
                _ if float::CONV.contains(&name) && args.is_empty() && is_ty(rt, "F64") => self.checkpoint(e, "conv", None),
                _ => {}
            }
        }
        let list_result = matches!(self.ty(e), Some(Type::Con(n, a)) if n == "List" && a.len() == 1);
        match (name, args.as_slice(), rv) {
            ("len", [], Val::List(l)) => Val::Int(l),
            ("is_empty", [], Val::List(l)) => Val::Bool(format!("(= {l} 0)")),
            ("map" | "sort" | "sort_by" | "reverse" | "enumerate", _, Val::List(l)) if list_result => Val::List(l),
            ("push", [_], Val::List(l)) if list_result => Val::List(self.define("Int", format!("(+ {l} 1)"))),
            ("concat", [Val::List(m)], Val::List(l)) if list_result => Val::List(self.define("Int", format!("(+ {l} {m})"))),
            ("take" | "drop", [Val::Int(n)], Val::List(l)) if list_result => {
                let k = format!("(ite (< {n} 0) 0 (ite (> {n} {l}) {l} {n}))");
                Val::List(self.define("Int", if name == "take" { k } else { format!("(- {l} {k})") }))
            }
            ("filter" | "unique", _, Val::List(l)) if list_result => Val::List(self.int_sym("0", &l)),
            ("len", [], _) if matches!(rt, Some(Type::Con(n, _)) if matches!(n.as_str(), "Str" | "Map" | "List")) => Val::Int(self.int_sym("0", LEN_MAX)),
            ("abs", [], Val::Int(x)) if is_ty(self.ty(e), "Int") => {
                let v = self.define("Int", format!("(ite (< {x} 0) (- {x}) {x})"));
                let c = in_range(&v);
                self.checkpoint(e, "abs", Some(c.clone()));
                self.assume(&c);
                Val::Int(v)
            }
            _ => self.fresh_e(e),
        }
    }

    fn method(&mut self, e: &'a Expr, recv: &'a Expr, name: &str, args: &'a [Expr]) -> Val {
        if self.check.user_methods.contains(&(e.span.start, e.span.end)) {
            let mut vals = vec![self.ex(recv)];
            vals.extend(args.iter().map(|a| self.ex(a)));
            return match self.fns.get(name).copied() {
                Some(g) => self.user_call(e, g, vals),
                None => {
                    self.fail();
                    self.fresh_e(e)
                }
            };
        }
        if self.floats
            && let Some(v) = self.elem_lambda(e, recv, name, args)
        {
            return v;
        }
        let rv = self.ex(recv);
        let avs = args.iter().map(|a| self.guard(|s| s.ex(a))).collect();
        self.builtin_method(e, recv, rv, name, avs)
    }

    fn call_expr(&mut self, e: &'a Expr, f: &'a Expr, args: &'a [Expr]) -> Val {
        if let ExprKind::Name(g) = &f.kind
            && !self.bound(g) {
                if self.floats && matches!(g.as_str(), "min" | "max" | "clamp" | "inf" | "nan" | "pi" | "rand_f64") && !self.fns.contains_key(g.as_str()) {
                    let vals: Vec<Val> = args.iter().map(|a| self.ex(a)).collect();
                    if let Some(v) = self.f_global(g, &vals, e) {
                        return v;
                    }
                    if (g == "min" || g == "max")
                        && is_ty(self.ty(e), "Int")
                        && let [Val::Int(x), Val::Int(y)] = vals.as_slice()
                    {
                        let o = if g == "min" { "<" } else { ">" };
                        return Val::Int(self.define("Int", format!("(ite ({o} {y} {x}) {y} {x})")));
                    }
                    return self.fresh_e(e);
                }
                if GLOBALS.contains(&g.as_str()) {
                    let vals: Vec<Val> = args.iter().map(|a| self.ex(a)).collect();
                    if (g == "min" || g == "max") && is_ty(self.ty(e), "Int")
                        && let [Val::Int(x), Val::Int(y)] = vals.as_slice() {
                            let o = if g == "min" { "<" } else { ">" };
                            return Val::Int(self.define("Int", format!("(ite ({o} {y} {x}) {y} {x})")));
                        }
                    if !matches!(g.as_str(), "some" | "ok" | "err" | "min" | "max" | "empty_map") {
                        self.fail();
                    }
                    return self.fresh_e(e);
                }
                if let Some(def) = self.fns.get(g.as_str()).copied() {
                    let vals: Vec<Val> = args.iter().map(|a| self.ex(a)).collect();
                    return self.user_call(e, def, vals);
                }
                for a in args {
                    self.ex(a);
                }
                return self.fresh_e(e);
            }
        self.fail();
        if !matches!(f.kind, ExprKind::Lambda { .. }) {
            self.ex(f);
        }
        for a in args {
            self.ex(a);
        }
        self.fresh_e(e)
    }

    fn user_call(&mut self, e: &Expr, g: &'a FnDef, vals: Vec<Val>) -> Val {
        if !self.goals.is_empty() || vals.len() != g.params.len() {
            self.fail();
            return self.fresh_e(e);
        }
        let binds: Vec<(String, Val)> = g.params.iter().map(|p| p.name.clone()).zip(vals.iter().cloned()).collect();
        let g0 = self.facts.len();
        let mut terms = Vec::new();
        let mut all = true;
        let conds = self.entry_conds(g);
        let has_conds = !conds.is_empty();
        for (j, (i, c)) in conds.into_iter().enumerate() {
            let mut b = binds.clone();
            if let Some(i) = i {
                b.push(("_".into(), vals[i].clone()));
            }
            let t = self.goal_with(b, c);
            let ci = self.entry_clause[&(g.name.clone(), j)];
            let q = t.clone().map(|goal| Query { fun: self.fidx, upto: g0, extra: (g0, self.facts.len()), path: self.path.clone(), goal });
            self.an.clauses[ci].obligations.push(Obligation { caller: self.fidx, at: e.span.start, query: q });
            match t {
                Some(t) => terms.push(t),
                None => all = false,
            }
        }
        if has_conds {
            let goal = and(&terms);
            let q = all.then(|| Query { fun: self.fidx, upto: g0, extra: (g0, self.facts.len()), path: self.path.clone(), goal: goal.clone() });
            let usable = self.usable && !self.dups.contains(&(e.span.start, e.span.end, kind_tag(&e.kind)));
            let site = self.an.sites.entry((e.span.start, e.span.end, "call".to_string())).or_insert(Site { func: self.fname.clone(), usable, queries: vec![] });
            site.usable &= usable;
            site.queries.push(q);
            self.assume(&goal);
        }
        let r = match self.inline(e, g, &binds) {
            Some(v) => v,
            None => self.fresh_e(e),
        };
        let mut rb = binds;
        rb.push(("r".into(), r.clone()));
        for post in &g.posts {
            if let Some(t) = self.goal_with(rb.clone(), post) {
                self.assume(&t);
            }
        }
        if let Some(c) = g.ret.as_ref().and_then(ty_name).and_then(|n| self.aliases.get(n).copied())
            && let Some(t) = self.goal_with(vec![("_".into(), r.clone())], c) {
                self.assume(&t);
            }
        r
    }

    fn record(&mut self, e: &'a Expr, ctor: &Option<String>, fields: &'a [(String, Expr)]) -> Val {
        let owner = ctor.clone().or_else(|| self.check.record_types.get(&(e.span.start, e.span.end)).cloned());
        let vals: Vec<(String, Val)> = fields.iter().map(|(n, x)| (n.clone(), self.ex(x))).collect();
        let Some(owner) = owner else {
            self.fail();
            return self.fresh_e(e);
        };
        let refs = self.frefs.get(&owner).cloned().unwrap_or_default();
        for (n, v) in &vals {
            let Some(conds) = refs.get(n) else { continue };
            let mut ts = Vec::new();
            let mut ok = true;
            for c in conds {
                match self.goal_with(vec![("_".into(), v.clone())], c) {
                    Some(t) => ts.push(t),
                    None => ok = false,
                }
            }
            let t = and(&ts);
            self.checkpoint(e, &format!("field:{n}"), ok.then(|| t.clone()));
            self.assume(&t);
        }
        if self.check.records.contains_key(&owner) {
            self.objs.push(Obj { owner: Some(owner), fields: vals.into_iter().collect() });
            Val::Rec(self.objs.len() - 1)
        } else {
            self.fresh_e(e)
        }
    }

    fn block(&mut self, stmts: &'a [Stmt]) -> Val {
        self.env.push(HashMap::new());
        let mut last = Val::Other;
        for s in stmts {
            last = Val::Other;
            match s {
                Stmt::Let(p, x) => {
                    let v = self.ex(x);
                    self.bind_pat(p, Some(v));
                }
                Stmt::Var(n, x) => {
                    let v = self.ex(x);
                    let b = if self.floats && v != Val::Never && !self.closure_assigned(n) { Bind::Var(Some(v)) } else { Bind::Mutable };
                    self.env.last_mut().unwrap().insert(n.clone(), b);
                }
                Stmt::Assign(n, x, _) => {
                    let v = self.ex(x);
                    if self.floats {
                        self.assigned(n, v);
                    }
                }
                Stmt::Expr(x) => last = self.ex(x),
                Stmt::Fn(f) => {
                    self.env.last_mut().unwrap().insert(f.name.clone(), Bind::Mutable);
                    if self.floats {
                        self.local_fn(f);
                    }
                }
                Stmt::For(p, it, body) => {
                    let tracked = self.enter_loop(&[body]);
                    let range = match (p, &it.kind) {
                        (Pat::Bind(i), ExprKind::Range(a, b)) => Some((i, a, b)),
                        _ => None,
                    };
                    if let Some((i, a, b)) = range {
                        let (av, bv) = (self.ex(a), self.ex(b));
                        self.env.push(HashMap::new());
                        let iv = self.int_sym(MIN, MAX);
                        self.env.last_mut().unwrap().insert(i.clone(), Bind::Val(Val::Int(iv.clone())));
                        let c = match (av, bv) {
                            (Val::Int(x), Val::Int(y)) => format!("(and (<= {x} {iv}) (< {iv} {y}))"),
                            _ => self.sym("Bool"),
                        };
                        self.under(c, |s| s.ex(body));
                        self.env.pop();
                    } else {
                        self.ex(it);
                        self.env.push(HashMap::new());
                        self.bind_pat(p, None);
                        self.guard(|s| s.ex(body));
                        self.env.pop();
                    }
                    self.leave_loop(tracked);
                }
                Stmt::While(c, body) => {
                    let tracked = self.enter_loop(&[c, body]);
                    self.guard(|s| {
                        let cv = s.ex(c);
                        let ct = s.bool_of(cv);
                        s.under(ct, |s| s.ex(body));
                    });
                    self.leave_loop(tracked);
                }
            }
        }
        self.env.pop();
        last
    }
}

pub struct Oracle {
    an: Analysis,
    solver: RefCell<Solver>,
    memo: RefCell<HashMap<Key, bool>>,
    enabled: bool,
}

impl Oracle {
    pub fn new(m: &Module, check: &CheckOutput) -> Self {
        let enabled = !matches!(std::env::var("SSPUR_SMT").as_deref(), Ok("off" | "0"));
        let an = if enabled { analyze(m, check) } else { Analysis::default() };
        Oracle { an, solver: RefCell::new(Solver::new()), memo: RefCell::new(HashMap::new()), enabled }
    }

    pub fn disabled() -> Self {
        Oracle { an: Analysis::default(), solver: RefCell::new(Solver::new()), memo: RefCell::new(HashMap::new()), enabled: false }
    }

    fn all_unsat(&self, qs: &[Option<Query>]) -> bool {
        !qs.is_empty()
            && qs.iter().all(|q| match q {
                Some(q) => self.solver.borrow_mut().check(&self.an.script(q), &[]) == Answer::Unsat,
                None => false,
            })
    }

    pub fn proves(&self, e: &Expr, kind: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let key = (e.span.start, e.span.end, kind.to_string());
        if let Some(&b) = self.memo.borrow().get(&key) {
            return b;
        }
        let r = match self.an.sites.get(&key) {
            Some(s) if s.usable => self.all_unsat(&s.queries),
            _ => false,
        };
        self.memo.borrow_mut().insert(key, r);
        r
    }

    fn clause_proved(&self, c: Option<&Clause>, f: &str, key: Key) -> bool {
        if !self.enabled || !self.an.usable(f) {
            return false;
        }
        let Some(c) = c else { return false };
        if let Some(&b) = self.memo.borrow().get(&key) {
            return b;
        }
        let qs: Vec<Option<Query>> = c.obligations.iter().map(|o| o.query.clone()).collect();
        let r = self.all_unsat(&qs);
        self.memo.borrow_mut().insert(key, r);
        r
    }

    pub fn post_proved(&self, f: &str, i: usize) -> bool {
        self.clause_proved(self.an.post_clause(f, i), f, (0, i as u32, format!("post:{f}")))
    }

    pub fn ret_proved(&self, f: &str) -> bool {
        self.clause_proved(self.an.ret_clause(f), f, (0, 0, format!("ret:{f}")))
    }
}

#[cfg(test)]
mod tests;
