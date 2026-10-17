use super::prove::{fits, pat_names, raw_op, Iv, FULL};
use sspur_check::{expr_key, CheckOutput, ExprKey, Type};
use sspur_syntax::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct Rewrite {
    pub func: String,
    pub rule: &'static str,
    pub before: String,
    pub after: String,
    pub proof: String,
    pub cost: String,
}

pub struct Optimized {
    pub module: Module,
    pub check: CheckOutput,
    pub rewrites: Vec<Rewrite>,
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Trap {
    Overflow,
    DivZero,
    Other,
}

type Traps = Option<BTreeSet<Trap>>;

#[derive(Clone, Debug)]
enum Av {
    Int(Iv),
    Rec(Vec<(String, Av)>),
    Any,
}

const SYN: u32 = 0xF000_0000;
const CALL: f64 = 8.0;
const ALLOC: f64 = 24.0;
const ALLOC_ELEM: f64 = 6.0;
const SMALL: f64 = 40.0;
const N_UNKNOWN: f64 = 1000.0;
pub(super) const PURE_METHODS: &[&str] = &[
    "map", "filter", "sum", "len", "is_empty", "to_f64", "to_int", "to_str", "upper", "lower", "trim", "words", "split", "join", "contains", "starts_with", "ends_with", "get", "or", "keys", "values", "items", "put", "push", "concat", "take", "drop",
    "reverse", "sort", "sort_by", "unique", "fold", "any", "all", "count", "min", "max", "abs", "sqrt", "floor", "ceil", "round", "first", "last", "zip", "enumerate", "find", "chars", "counts", "is_some", "is_none", "has", "remove", "size",
];

pub struct Profile {
    pub calls: HashMap<String, u64>,
}

pub fn profile_path(m: &Module) -> Option<std::path::PathBuf> {
    let base = sspur_syntax::cache_root()?;
    let key = blake3::hash(printer::print_module(m).as_bytes()).to_hex();
    Some(base.join("profile").join(format!("{}.json", &key[..32])))
}

fn load_profile(m: &Module) -> Option<Profile> {
    let text = std::fs::read_to_string(profile_path(m)?).ok()?;
    let mut calls = HashMap::new();
    for line in text.lines() {
        let (n, c) = line.split_once(' ')?;
        calls.insert(n.to_string(), c.trim().parse().ok()?);
    }
    Some(Profile { calls })
}

pub fn save_profile(m: &Module, calls: &BTreeMap<String, u64>) -> Option<std::path::PathBuf> {
    let p = profile_path(m)?;
    std::fs::create_dir_all(p.parent()?).ok()?;
    let text: String = calls.iter().map(|(n, c)| format!("{n} {c}\n")).collect();
    std::fs::write(&p, text).ok()?;
    Some(p)
}

fn is_ty(t: &Type, n: &str) -> bool {
    matches!(t, Type::Con(c, a) if c == n && a.is_empty())
}

fn list_elem(t: &Type) -> Option<&Type> {
    match t {
        Type::Con(c, a) if c == "List" && a.len() == 1 => Some(&a[0]),
        _ => None,
    }
}

fn one_line(e: &Expr) -> String {
    let s = printer::expr(e, 0).split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > 110 { format!("{}...", s.chars().take(107).collect::<String>()) } else { s }
}

fn map_stmt(s: &Stmt, f: &mut dyn FnMut(&Expr) -> Expr) -> Stmt {
    match s {
        Stmt::Let(p, x) => Stmt::Let(p.clone(), f(x)),
        Stmt::Var(n, x) => Stmt::Var(n.clone(), f(x)),
        Stmt::Assign(n, x, sp) => Stmt::Assign(n.clone(), f(x), *sp),
        Stmt::Expr(x) => Stmt::Expr(f(x)),
        Stmt::For(p, a, b) => Stmt::For(p.clone(), f(a), f(b)),
        Stmt::While(a, b) => Stmt::While(f(a), f(b)),
        Stmt::Fn(d) => {
            let mut d = (**d).clone();
            d.body = f(&d.body);
            Stmt::Fn(Box::new(d))
        }
    }
}

fn map_arms(arms: &[Arm], f: &mut dyn FnMut(&Expr) -> Expr) -> Vec<Arm> {
    arms.iter().map(|a| Arm { pat: a.pat.clone(), guard: a.guard.as_ref().map(&mut *f), body: f(&a.body) }).collect()
}

fn map_kids(e: &Expr, f: &mut dyn FnMut(&Expr) -> Expr) -> Expr {
    let kind = match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Unit | ExprKind::Name(_) | ExprKind::Hole(_) | ExprKind::Placeholder => e.kind.clone(),
        ExprKind::Str(parts) => ExprKind::Str(
            parts
                .iter()
                .map(|p| match p {
                    StrPart::Expr(x) => StrPart::Expr(f(x)),
                    l => l.clone(),
                })
                .collect(),
        ),
        ExprKind::Field(x, n) => ExprKind::Field(Box::new(f(x)), n.clone()),
        ExprKind::Unary(o, x) => ExprKind::Unary(*o, Box::new(f(x))),
        ExprKind::Raise(x) => ExprKind::Raise(Box::new(f(x))),
        ExprKind::Return(x) => ExprKind::Return(Box::new(f(x))),
        ExprKind::Lambda { params, body, implicit, .. } => ExprKind::Lambda { params: params.clone(), body: Box::new(f(body)), implicit: *implicit, tys: vec![] },
        ExprKind::Call(g, args) => ExprKind::Call(Box::new(f(g)), args.iter().map(&mut *f).collect()),
        ExprKind::Method { recv, name, targs, args } => ExprKind::Method { recv: Box::new(f(recv)), name: name.clone(), targs: targs.clone(), args: args.iter().map(&mut *f).collect() },
        ExprKind::Index(a, b) => ExprKind::Index(Box::new(f(a)), Box::new(f(b))),
        ExprKind::Binary(o, a, b) => ExprKind::Binary(*o, Box::new(f(a)), Box::new(f(b))),
        ExprKind::Range(a, b) => ExprKind::Range(Box::new(f(a)), Box::new(f(b))),
        ExprKind::If(c, t, x) => ExprKind::If(Box::new(f(c)), Box::new(f(t)), x.as_ref().map(|x| Box::new(f(x)))),
        ExprKind::Match(s, arms, k) => ExprKind::Match(Box::new(f(s)), map_arms(arms, f), *k),
        ExprKind::Catch(s, arms) => ExprKind::Catch(Box::new(f(s)), map_arms(arms, f)),
        ExprKind::Handle(s, arms) => ExprKind::Handle(Box::new(f(s)), map_arms(arms, f)),
        ExprKind::Block(stmts) => ExprKind::Block(stmts.iter().map(|s| map_stmt(s, f)).collect()),
        ExprKind::Record { ctor, fields } => ExprKind::Record { ctor: ctor.clone(), fields: fields.iter().map(|(n, x)| (n.clone(), f(x))).collect() },
        ExprKind::List(xs) => ExprKind::List(xs.iter().map(&mut *f).collect()),
        ExprKind::Tuple(xs) => ExprKind::Tuple(xs.iter().map(&mut *f).collect()),
        ExprKind::Par(xs) => ExprKind::Par(xs.iter().map(&mut *f).collect()),
        ExprKind::Table(rows) => ExprKind::Table(
            rows.iter()
                .map(|r| TableRow {
                    cells: r
                        .cells
                        .iter()
                        .map(|c| match c {
                            Cell::Cond(x) => Cell::Cond(f(x)),
                            c => c.clone(),
                        })
                        .collect(),
                    out: f(&r.out),
                })
                .collect(),
        ),
        ExprKind::With(b, ups) => ExprKind::With(
            Box::new(f(b)),
            ups.iter()
                .map(|(p, v)| {
                    let p = p
                        .iter()
                        .map(|s| match s {
                            PathSeg::Index(i) => PathSeg::Index(f(i)),
                            s => s.clone(),
                        })
                        .collect();
                    (p, f(v))
                })
                .collect(),
        ),
    };
    Expr::new(kind, e.span)
}

fn any_node(e: &Expr, pred: &mut dyn FnMut(&Expr) -> bool) -> bool {
    let mut hit = false;
    visit::walk_expr(e, &mut |x| {
        if hit {
            return false;
        }
        if pred(x) {
            hit = true;
            return false;
        }
        true
    });
    hit
}

fn stmt_fn(e: &Expr) -> bool {
    any_node(e, &mut |x| matches!(&x.kind, ExprKind::Block(st) if st.iter().any(|s| matches!(s, Stmt::Fn(_)))))
}

fn names(e: &Expr) -> HashSet<String> {
    let mut out = HashSet::new();
    visit::walk_expr(e, &mut |x| {
        match &x.kind {
            ExprKind::Name(n) => {
                out.insert(n.clone());
            }
            ExprKind::Block(st) => {
                for s in st {
                    if let Stmt::Assign(n, _, _) = s {
                        out.insert(n.clone());
                    }
                }
            }
            _ => {}
        }
        true
    });
    out
}

fn binders(e: &Expr) -> HashSet<String> {
    let mut out = HashSet::new();
    visit::walk_expr(e, &mut |x| {
        match &x.kind {
            ExprKind::Lambda { params, .. } => out.extend(params.iter().cloned()),
            ExprKind::Match(_, arms, _) | ExprKind::Catch(_, arms) | ExprKind::Handle(_, arms) => arms.iter().for_each(|a| pat_names(&a.pat, &mut out)),
            ExprKind::Block(st) => {
                for s in st {
                    match s {
                        Stmt::Let(p, _) | Stmt::For(p, _, _) => pat_names(p, &mut out),
                        Stmt::Var(n, _) => {
                            out.insert(n.clone());
                        }
                        Stmt::Fn(f) => {
                            out.insert(f.name.clone());
                            out.extend(f.params.iter().map(|p| p.name.clone()));
                        }
                        _ => {}
                    }
                }
            }
            ExprKind::Table(rows) => rows.iter().flat_map(|r| &r.cells).for_each(|c| {
                if let Cell::Pat(p) = c {
                    pat_names(p, &mut out)
                }
            }),
            _ => {}
        }
        true
    });
    out
}

fn pat_binds(p: &Pat, x: &str) -> bool {
    let mut s = HashSet::new();
    pat_names(p, &mut s);
    s.contains(x)
}

fn subst(e: &Expr, x: &str, r: &Expr) -> Expr {
    match &e.kind {
        ExprKind::Name(n) if n == x => r.clone(),
        ExprKind::Lambda { params, .. } if params.iter().any(|p| p == x) => e.clone(),
        ExprKind::Match(s, arms, _) | ExprKind::Catch(s, arms) | ExprKind::Handle(s, arms) => {
            let arms = arms.iter().map(|a| if pat_binds(&a.pat, x) { a.clone() } else { Arm { pat: a.pat.clone(), guard: a.guard.as_ref().map(|g| subst(g, x, r)), body: subst(&a.body, x, r) } }).collect();
            let s = Box::new(subst(s, x, r));
            let kind = match &e.kind {
                ExprKind::Match(_, _, k) => ExprKind::Match(s, arms, *k),
                ExprKind::Catch(..) => ExprKind::Catch(s, arms),
                _ => ExprKind::Handle(s, arms),
            };
            Expr::new(kind, e.span)
        }
        ExprKind::Block(stmts) => {
            let mut out = Vec::new();
            let mut live = true;
            for s in stmts {
                if !live {
                    out.push(s.clone());
                    continue;
                }
                let ns = match s {
                    Stmt::Let(p, v) => {
                        let ns = Stmt::Let(p.clone(), subst(v, x, r));
                        live = !pat_binds(p, x);
                        ns
                    }
                    Stmt::Var(n, v) => {
                        live = n != x;
                        Stmt::Var(n.clone(), subst(v, x, r))
                    }
                    Stmt::For(p, a, b) => Stmt::For(p.clone(), subst(a, x, r), if pat_binds(p, x) { b.clone() } else { subst(b, x, r) }),
                    Stmt::Fn(d) => {
                        if d.name == x {
                            live = false;
                            s.clone()
                        } else if d.params.iter().any(|p| p.name == x) {
                            s.clone()
                        } else {
                            map_stmt(s, &mut |y| subst(y, x, r))
                        }
                    }
                    s => map_stmt(s, &mut |y| subst(y, x, r)),
                };
                out.push(ns);
            }
            Expr::new(ExprKind::Block(out), e.span)
        }
        _ => map_kids(e, &mut |c| subst(c, x, r)),
    }
}

fn count_free(e: &Expr, x: &str) -> usize {
    let probe = Expr::new(ExprKind::Hole(Some("\u{1}probe".into())), Span::default());
    let s = subst(e, x, &probe);
    let mut n = 0;
    visit::walk_expr(&s, &mut |y| {
        if matches!(&y.kind, ExprKind::Hole(Some(h)) if h == "\u{1}probe") {
            n += 1;
        }
        true
    });
    n
}

fn head_is(e: &Expr, x: &str) -> bool {
    match &e.kind {
        ExprKind::Name(n) => n == x,
        ExprKind::Method { recv, .. } | ExprKind::Field(recv, _) | ExprKind::Index(recv, _) | ExprKind::Unary(_, recv) => head_is(recv, x),
        ExprKind::Binary(_, a, _) | ExprKind::Range(a, _) | ExprKind::If(a, _, _) | ExprKind::Match(a, _, _) => head_is(a, x),
        ExprKind::Call(f, args) => matches!(f.kind, ExprKind::Name(_)) && args.first().is_some_and(|a| head_is(a, x)),
        ExprKind::Record { fields, .. } => fields.first().is_some_and(|(_, v)| head_is(v, x)),
        ExprKind::List(xs) | ExprKind::Tuple(xs) => xs.first().is_some_and(|v| head_is(v, x)),
        ExprKind::Block(st) => match st.first() {
            Some(Stmt::Let(_, v) | Stmt::Var(_, v) | Stmt::Expr(v)) => head_is(v, x),
            _ => false,
        },
        _ => false,
    }
}

fn replace_ph(e: &Expr, v: &Expr) -> Expr {
    match &e.kind {
        ExprKind::Placeholder => v.clone(),
        ExprKind::Lambda { .. } => e.clone(),
        _ => map_kids(e, &mut |c| replace_ph(c, v)),
    }
}

#[allow(dead_code)]
fn size(e: &Expr) -> usize {
    let mut n = 0;
    visit::walk_expr(e, &mut |_| {
        n += 1;
        true
    });
    n
}

fn refine_iv(r: &Expr) -> Option<Iv> {
    match &r.kind {
        ExprKind::Binary(BinOp::And, a, b) => {
            let (x, y) = (refine_iv(a)?, refine_iv(b)?);
            Some((x.0.max(y.0), x.1.min(y.1)))
        }
        ExprKind::Binary(op, a, b) => {
            let (k, flip) = match (&a.kind, &b.kind) {
                (ExprKind::Placeholder, _) => (lit_int(b)?, false),
                (_, ExprKind::Placeholder) => (lit_int(a)?, true),
                _ => return None,
            };
            let op = if flip { flip_cmp(*op)? } else { *op };
            Some(match op {
                BinOp::Gt => (k + 1, FULL.1),
                BinOp::Ge => (k, FULL.1),
                BinOp::Lt => (FULL.0, k - 1),
                BinOp::Le => (FULL.0, k),
                BinOp::Eq => (k, k),
                _ => return None,
            })
        }
        _ => None,
    }
}

fn lit_int(e: &Expr) -> Option<i128> {
    match &e.kind {
        ExprKind::Int(n) => Some(*n as i128),
        ExprKind::Unary(UnOp::Neg, x) => lit_int(x).map(|v| -v),
        _ => None,
    }
}

fn flip_cmp(op: BinOp) -> Option<BinOp> {
    Some(match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Le => BinOp::Ge,
        BinOp::Gt => BinOp::Lt,
        BinOp::Ge => BinOp::Le,
        BinOp::Eq => BinOp::Eq,
        BinOp::Ne => BinOp::Ne,
        _ => return None,
    })
}

fn neg_cmp(op: BinOp) -> Option<BinOp> {
    Some(match op {
        BinOp::Lt => BinOp::Ge,
        BinOp::Le => BinOp::Gt,
        BinOp::Gt => BinOp::Le,
        BinOp::Ge => BinOp::Lt,
        BinOp::Eq => BinOp::Ne,
        BinOp::Ne => BinOp::Eq,
        _ => return None,
    })
}

fn meet(a: Iv, b: Iv) -> Iv {
    (a.0.max(b.0), a.1.min(b.1))
}

fn join(a: Iv, b: Iv) -> Iv {
    (a.0.min(b.0), a.1.max(b.1))
}

fn clamp(a: Iv) -> Iv {
    (a.0.max(FULL.0), a.1.min(FULL.1))
}

#[derive(Clone, Default)]
struct Facts {
    iv: HashMap<String, Iv>,
    ints: HashSet<String>,
    rel: Vec<(Expr, bool)>,
}

impl Facts {
    fn forget(&mut self, x: &str) {
        self.iv.remove(x);
        self.ints.remove(x);
        self.rel.retain(|(e, _)| !names(e).contains(x));
    }

    fn forget_all(&mut self, xs: &HashSet<String>) {
        for x in xs {
            self.forget(x);
        }
    }
}

struct Opt<'a> {
    check: &'a CheckOutput,
    fns: HashMap<String, FnDef>,
    recursive: HashSet<String>,
    alias: HashMap<String, (Ty, Expr)>,
    fields: HashMap<String, Vec<Field>>,
    used: HashSet<String>,
    syn: HashMap<ExprKey, Type>,
    next: u32,
    fresh_n: usize,
    log: Vec<Rewrite>,
    notes: Vec<String>,
    cur: String,
    scope: HashSet<String>,
    smt: Option<sspur_smt::Solver>,
    prof: Option<Profile>,
    iv_used: bool,
    budget: usize,
    fieldish: HashSet<(u32, u32)>,
}

fn call_graph(fns: &HashMap<String, FnDef>, check: &CheckOutput) -> HashMap<String, HashSet<String>> {
    let mut g = HashMap::new();
    for (n, f) in fns {
        let mut out = HashSet::new();
        let mut scan = |e: &Expr| {
            visit::walk_expr(e, &mut |x| {
                match &x.kind {
                    ExprKind::Name(m) if fns.contains_key(m) => {
                        out.insert(m.clone());
                    }
                    ExprKind::Method { name, .. } if check.user_methods.contains(&(x.span.start, x.span.end)) => {
                        out.insert(name.clone());
                    }
                    _ => {}
                }
                true
            })
        };
        scan(&f.body);
        f.pres.iter().chain(&f.posts).for_each(&mut scan);
        g.insert(n.clone(), out);
    }
    g
}

impl<'a> Opt<'a> {
    fn new(m: &Module, check: &'a CheckOutput) -> Self {
        let fns: HashMap<String, FnDef> = m.defs.iter().filter_map(|d| if let Def::Fn(f) = d { Some((f.name.clone(), f.clone())) } else { None }).collect();
        let g = call_graph(&fns, check);
        let mut recursive = HashSet::new();
        for n in fns.keys() {
            let mut stack: Vec<&String> = g[n].iter().collect();
            let mut seen = HashSet::new();
            while let Some(x) = stack.pop() {
                if x == n {
                    recursive.insert(n.clone());
                    break;
                }
                if seen.insert(x) {
                    stack.extend(g.get(x).into_iter().flatten());
                }
            }
        }
        let mut alias = HashMap::new();
        let mut fields = HashMap::new();
        for d in &m.defs {
            if let Def::Type(t) = d {
                match &t.body {
                    TypeBody::Alias(ty, Some(r)) => {
                        alias.insert(t.name.clone(), (ty.clone(), r.clone()));
                    }
                    TypeBody::Record(fs) => {
                        fields.insert(t.name.clone(), fs.clone());
                    }
                    TypeBody::Sum(vs) => {
                        for v in vs {
                            if let Some(fs) = &v.fields {
                                fields.insert(v.name.clone(), fs.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut used = HashSet::new();
        for d in &m.defs {
            used.insert(d.name().to_string());
            if let Def::Fn(f) = d {
                used.extend(names(&f.body));
                used.extend(binders(&f.body));
                used.extend(f.params.iter().map(|p| p.name.clone()));
            }
            if let Def::Test(t) = d {
                used.extend(names(&t.body));
            }
        }
        let smt = match std::env::var("SSPUR_SMT").as_deref() {
            Ok("off") | Ok("0") => None,
            _ => Some(sspur_smt::Solver::new()),
        };
        let mut o = Opt { check, fns, recursive, alias, fields, used, syn: HashMap::new(), next: 0, fresh_n: 0, log: vec![], notes: vec![], cur: String::new(), scope: HashSet::new(), smt, prof: load_profile(m), iv_used: false, budget: 0, fieldish: HashSet::new() };
        let names: Vec<String> = o.fns.keys().cloned().collect();
        for n in names {
            let b = o.fns[&n].body.clone();
            let b = o.methodize(&b);
            o.fns.get_mut(&n).unwrap().body = b;
        }
        o
    }

    fn fresh(&mut self, base: &str) -> String {
        loop {
            self.fresh_n += 1;
            let n = format!("{base}{}_", self.fresh_n);
            if !self.used.contains(&n) {
                self.used.insert(n.clone());
                return n;
            }
        }
    }

    fn mk(&mut self, kind: ExprKind, t: Option<Type>) -> Expr {
        self.next += 1;
        let e = Expr::new(kind, Span { start: SYN + self.next, end: SYN + self.next });
        if let Some(t) = t {
            self.syn.insert(expr_key(&e), t);
        }
        e
    }

    fn ty(&self, e: &Expr) -> Option<Type> {
        let k = expr_key(e);
        match self.syn.get(&k) {
            Some(t) => Some(t.clone()),
            None if e.span.start >= SYN => None,
            None => self.check.expr_types.get(&k).cloned(),
        }
    }

    fn methodize(&mut self, e: &Expr) -> Expr {
        let e = map_kids(e, &mut |c| self.methodize(c));
        if let ExprKind::Field(r, n) = &e.kind {
            let method = match self.ty(r) {
                Some(Type::Con(t, _)) => !self.check.records.get(&t).is_some_and(|(_, fs)| fs.iter().any(|(f, _)| f == n)),
                Some(Type::Tuple(_)) | None => false,
                Some(_) => true,
            };
            if method {
                let m = Expr::new(ExprKind::Method { recv: r.clone(), name: n.clone(), targs: vec![], args: vec![] }, e.span);
                if let Some(t) = self.ty(&e) {
                    self.syn.insert(expr_key(&m), t);
                }
                self.fieldish.insert((e.span.start, e.span.end));
                return m;
            }
        }
        e
    }

    fn fieldize(&self, e: &Expr) -> Expr {
        let e = map_kids(e, &mut |c| self.fieldize(c));
        if let ExprKind::Method { recv, name, args, targs } = &e.kind
            && args.is_empty() && targs.is_empty() && self.fieldish.contains(&(e.span.start, e.span.end)) {
                return Expr::new(ExprKind::Field(recv.clone(), name.clone()), e.span);
            }
        e
    }

    fn user_method(&self, e: &Expr) -> bool {
        e.span.start < SYN && self.check.user_methods.contains(&(e.span.start, e.span.end))
    }

    fn global_fn(&self, n: &str) -> Option<&FnDef> {
        if self.scope.contains(n) { None } else { self.fns.get(n) }
    }

    fn record(&mut self, rule: &'static str, before: &Expr, after: &Expr, proof: String, cost: String) {
        let (b, a) = (self.fieldize(before), self.fieldize(after));
        self.log.push(Rewrite { func: self.cur.clone(), rule, before: one_line(&b), after: one_line(&a), proof, cost });
    }

    // purity: no effects anywhere in e (lambda bodies included)
    fn pure(&self, e: &Expr) -> bool {
        !any_node(e, &mut |x| match &x.kind {
            ExprKind::Call(f, _) => match &f.kind {
                ExprKind::Name(n) if self.scope.contains(n) => true,
                ExprKind::Name(n) => match self.fns.get(n) {
                    Some(g) => !g.effects.is_empty() || g.ext.is_some(),
                    None => !(n.starts_with(|c: char| c.is_ascii_uppercase()) || matches!(n.as_str(), "some" | "empty_map" | "ok" | "err")),
                },
                _ => true,
            },
            ExprKind::Method { name, .. } => {
                if self.user_method(x) {
                    self.fns.get(name).is_none_or(|g| !g.effects.is_empty())
                } else {
                    !PURE_METHODS.contains(&name.as_str())
                }
            }
            ExprKind::Raise(_) | ExprKind::Return(_) | ExprKind::Handle(..) | ExprKind::Hole(_) | ExprKind::Par(_) | ExprKind::Catch(..) => true,
            ExprKind::Block(st) => st.iter().any(|s| matches!(s, Stmt::Fn(_))),
            _ => false,
        })
    }

    fn field_refine(&self, f: &Field) -> Option<Expr> {
        if f.refine.is_some() {
            return f.refine.clone();
        }
        match &f.ty {
            Ty::Named { name, .. } => self.alias.get(name).map(|(_, r)| r.clone()),
            _ => None,
        }
    }

    fn field_is_int(&self, f: &Field) -> bool {
        match &f.ty {
            Ty::Named { name, args, .. } if args.is_empty() => name == "Int" || self.alias.get(name).is_some_and(|(t, _)| matches!(t, Ty::Named { name, .. } if name == "Int")),
            _ => false,
        }
    }

    fn av_of_type(&self, t: &Type) -> Av {
        match t {
            _ if is_ty(t, "Int") => Av::Int(FULL),
            Type::Con(n, a) if a.is_empty() && self.alias.contains_key(n) => {
                let (base, r) = &self.alias[n];
                if matches!(base, Ty::Named { name, .. } if name == "Int") {
                    Av::Int(meet(FULL, refine_iv(r).unwrap_or(FULL)))
                } else {
                    Av::Any
                }
            }
            Type::Con(n, _) if self.fields.contains_key(n) => {
                let fs = self.fields[n].iter().map(|f| (f.name.clone(), if self.field_is_int(f) { Av::Int(meet(FULL, self.field_refine(f).and_then(|r| refine_iv(&r)).unwrap_or(FULL))) } else { Av::Any })).collect();
                Av::Rec(fs)
            }
            _ => Av::Any,
        }
    }

    fn rec_name(&self, e: &Expr, ctor: &Option<String>) -> Option<String> {
        if let Some(c) = ctor {
            return Some(c.clone());
        }
        if e.span.start < SYN
            && let Some(n) = self.check.record_types.get(&(e.span.start, e.span.end)) {
                return Some(n.clone());
            }
        match self.ty(e)? {
            Type::Con(n, _) => Some(n),
            _ => None,
        }
    }

    fn av(&self, e: &Expr, env: &HashMap<String, Av>) -> Av {
        match &e.kind {
            ExprKind::Int(n) => Av::Int((*n as i128, *n as i128)),
            ExprKind::Name(n) => env.get(n).cloned().unwrap_or_else(|| self.ty(e).map_or(Av::Any, |t| self.av_of_type(&t))),
            ExprKind::Placeholder => env.get("_").cloned().unwrap_or(Av::Any),
            ExprKind::Unary(UnOp::Neg, x) => match self.av(x, env) {
                Av::Int(i) => Av::Int(clamp((-i.1, -i.0))),
                _ => Av::Any,
            },
            ExprKind::Binary(op @ (BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem), a, b) => match (self.av(a, env), self.av(b, env)) {
                (Av::Int(x), Av::Int(y)) => raw_op(*op, x, y).map_or(Av::Any, |r| Av::Int(clamp(r))),
                _ => Av::Any,
            },
            ExprKind::Field(x, f) => match self.av(x, env) {
                Av::Rec(fs) => fs.iter().find(|(n, _)| n == f).map_or(Av::Any, |(_, a)| a.clone()),
                _ => self.ty(e).map_or(Av::Any, |t| self.av_of_type(&t)),
            },
            ExprKind::Record { ctor, fields } => match self.rec_name(e, ctor) {
                Some(_) => Av::Rec(fields.iter().map(|(f, v)| (f.clone(), self.av(v, env))).collect()),
                None => Av::Any,
            },
            ExprKind::If(_, t, Some(f)) => match (self.av(t, env), self.av(f, env)) {
                (Av::Int(x), Av::Int(y)) => Av::Int(join(x, y)),
                _ => Av::Any,
            },
            ExprKind::Method { recv: _, name, args, .. } if name == "len" && args.is_empty() => Av::Int((0, 1 << 47)),
            ExprKind::Block(st) => {
                let mut env = env.clone();
                for s in &st[..st.len().saturating_sub(1)] {
                    if let Stmt::Let(Pat::Bind(x), v) = s {
                        let a = self.av(v, &env);
                        env.insert(x.clone(), a);
                    } else {
                        return Av::Any;
                    }
                }
                match st.last() {
                    Some(Stmt::Expr(x)) => self.av(x, &env),
                    _ => Av::Any,
                }
            }
            _ => self.ty(e).map_or(Av::Any, |t| self.av_of_type(&t)),
        }
    }

    fn int_iv(&self, e: &Expr, env: &HashMap<String, Av>) -> Iv {
        match self.av(e, env) {
            Av::Int(i) => i,
            _ => FULL,
        }
    }

    // trap kinds evaluation of e may raise; None when unknown (calls, partial ops, possible divergence)
    fn traps(&mut self, e: &Expr, env: &HashMap<String, Av>, depth: usize) -> Traps {
        let mut out = BTreeSet::new();
        let kids = |s: &mut Self, xs: &[&Expr], out: &mut BTreeSet<Trap>| -> Option<()> {
            for x in xs {
                out.extend(s.traps(x, env, depth)?);
            }
            Some(())
        };
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Unit | ExprKind::Name(_) | ExprKind::Placeholder | ExprKind::Lambda { .. } => {}
            ExprKind::Str(parts) => {
                let xs: Vec<&Expr> = parts.iter().filter_map(|p| if let StrPart::Expr(x) = p { Some(x) } else { None }).collect();
                kids(self, &xs, &mut out)?;
            }
            ExprKind::Field(x, _) => kids(self, &[x], &mut out)?,
            ExprKind::Tuple(xs) | ExprKind::List(xs) => kids(self, &xs.iter().collect::<Vec<_>>(), &mut out)?,
            ExprKind::If(c, t, f) => {
                let mut xs = vec![&**c, &**t];
                if let Some(f) = f {
                    xs.push(f);
                }
                kids(self, &xs, &mut out)?;
            }
            ExprKind::Unary(UnOp::Not, x) => kids(self, &[x], &mut out)?,
            ExprKind::Unary(UnOp::Neg, x) => {
                kids(self, &[x], &mut out)?;
                if self.ty(e).is_some_and(|t| is_ty(&t, "Int")) || matches!(self.av(x, env), Av::Int(_)) {
                    if self.int_iv(x, env).0 > FULL.0 {
                        self.iv_used = true;
                    } else {
                        out.insert(Trap::Overflow);
                    }
                } else if !self.ty(e).is_some_and(|t| is_ty(&t, "F64")) {
                    return None;
                }
            }
            ExprKind::Binary(op, a, b) => {
                kids(self, &[a, b], &mut out)?;
                let t = self.ty(e).or_else(|| match (self.av(a, env), self.av(b, env)) {
                    (Av::Int(_), Av::Int(_)) => Some(Type::con("Int")),
                    _ => None,
                });
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul => match t {
                        Some(t) if is_ty(&t, "Int") => {
                            let r = raw_op(*op, self.int_iv(a, env), self.int_iv(b, env));
                            if r.is_some_and(fits) {
                                self.iv_used = true;
                            } else {
                                out.insert(Trap::Overflow);
                            }
                        }
                        Some(t) if is_ty(&t, "F64") || is_ty(&t, "Str") => {}
                        _ => return None,
                    },
                    BinOp::Div | BinOp::Rem => match t {
                        Some(t) if is_ty(&t, "Int") => {
                            let (x, y) = (self.int_iv(a, env), self.int_iv(b, env));
                            if y.0 > 0 || y.1 < 0 {
                                self.iv_used = true;
                            } else {
                                out.insert(Trap::DivZero);
                            }
                            if x.0 == FULL.0 && y.0 <= -1 && y.1 >= -1 {
                                out.insert(Trap::Overflow);
                            }
                        }
                        _ => return None,
                    },
                    BinOp::Pow => return None,
                    _ => {}
                }
            }
            ExprKind::Record { ctor, fields } => {
                kids(self, &fields.iter().map(|(_, v)| v).collect::<Vec<_>>(), &mut out)?;
                let name = self.rec_name(e, ctor)?;
                let defs = self.fields.get(&name).cloned().unwrap_or_default();
                for f in &defs {
                    let Some(r) = self.field_refine(f) else { continue };
                    let (_, v) = fields.iter().find(|(n, _)| *n == f.name)?;
                    let ok = refine_iv(&r).is_some_and(|want| {
                        let have = self.int_iv(v, env);
                        have.0 >= want.0 && have.1 <= want.1
                    });
                    if ok {
                        self.iv_used = true;
                    } else {
                        out.insert(Trap::Other);
                    }
                }
            }
            ExprKind::Method { recv, name, args, .. } if args.is_empty() && matches!(name.as_str(), "len" | "is_empty" | "to_f64" | "to_str") && !self.user_method(e) => kids(self, &[recv], &mut out)?,
            ExprKind::Method { recv, name, args, .. } if args.is_empty() && name == "sum" && !self.user_method(e) => {
                kids(self, &[recv], &mut out)?;
                match self.ty(e) {
                    Some(t) if is_ty(&t, "Int") => {
                        out.insert(Trap::Overflow);
                    }
                    Some(t) if is_ty(&t, "F64") => {}
                    _ => return None,
                }
            }
            ExprKind::Call(f, args) => {
                let ExprKind::Name(n) = &f.kind else { return None };
                let g = self.global_fn(n)?.clone();
                if depth > 3 || self.recursive.contains(n) || !g.effects.is_empty() || !g.tparams.is_empty() || g.ext.is_some() || !g.pres.is_empty() || !g.posts.is_empty() || g.params.iter().any(|p| p.refine.is_some()) || g.params.len() != args.len() {
                    return None;
                }
                kids(self, &args.iter().collect::<Vec<_>>(), &mut out)?;
                let mut inner = HashMap::new();
                for (p, a) in g.params.iter().zip(args) {
                    inner.insert(p.name.clone(), self.av(a, env));
                }
                let saved = std::mem::take(&mut self.scope);
                let r = self.traps(&g.body, &inner, depth + 1);
                self.scope = saved;
                out.extend(r?);
            }
            ExprKind::Block(st) => {
                let mut env = env.clone();
                for s in st {
                    match s {
                        Stmt::Let(Pat::Bind(x), v) => {
                            out.extend(self.traps(v, &env, depth)?);
                            let a = self.av(v, &env);
                            env.insert(x.clone(), a);
                        }
                        Stmt::Expr(v) => out.extend(self.traps(v, &env, depth)?),
                        _ => return None,
                    }
                }
            }
            _ => return None,
        }
        Some(out)
    }
}

fn order_safe(ts: &[(String, Traps)]) -> Option<String> {
    let risky: Vec<&(String, Traps)> = ts.iter().filter(|(_, t)| t.as_ref().is_none_or(|t| !t.is_empty())).collect();
    if risky.len() <= 1 {
        let what = risky.first().map_or("no stage can trap".to_string(), |(n, _)| format!("only {n} can trap"));
        return Some(what);
    }
    let mut all = BTreeSet::new();
    for (_, t) in &risky {
        all.extend(t.as_ref()?.iter().copied());
    }
    match all.iter().collect::<Vec<_>>().as_slice() {
        [Trap::Overflow] => Some("every trap is 'integer overflow', whose message does not depend on the element".into()),
        [Trap::DivZero] => Some("every trap is 'division by zero', whose message does not depend on the element".into()),
        _ => None,
    }
}

fn traps_text(t: &Traps) -> String {
    match t {
        None => "unknown".into(),
        Some(s) if s.is_empty() => "none".into(),
        Some(s) => s.iter().map(|k| format!("{k:?}").to_lowercase()).collect::<Vec<_>>().join("+"),
    }
}

impl Opt<'_> {
    // estimated instructions for one evaluation; allocations weighted as ALLOC
    fn est(&self, e: &Expr, depth: usize) -> f64 {
        let kids = |s: &Self, e: &Expr| -> f64 { visit::children(e).iter().map(|c| s.est(c, depth)).sum() };
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Unit | ExprKind::Name(_) | ExprKind::Placeholder => 1.0,
            ExprKind::Lambda { .. } => 2.0,
            ExprKind::Binary(op, ..) => kids(self, e) + if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem) { 2.0 } else { 1.0 },
            ExprKind::Record { .. } | ExprKind::List(_) | ExprKind::Tuple(_) | ExprKind::Str(_) => kids(self, e) + ALLOC,
            ExprKind::If(c, t, f) => self.est(c, depth) + self.est(t, depth).max(f.as_ref().map_or(0.0, |f| self.est(f, depth))),
            ExprKind::Call(f, args) => {
                let a: f64 = args.iter().map(|x| self.est(x, depth)).sum();
                let body = match &f.kind {
                    ExprKind::Name(n) => match self.global_fn(n) {
                        Some(g) if depth < 3 && !self.recursive.contains(n) => self.est(&g.body, depth + 1),
                        _ => 50.0,
                    },
                    _ => 30.0,
                };
                a + CALL + body
            }
            ExprKind::Method { recv, name, args, .. } => {
                let n = self.count(recv);
                let per = args.iter().map(|x| self.lambda_cost(x, depth)).sum::<f64>();
                self.est(recv, depth)
                    + match name.as_str() {
                        "map" => n * (per + ALLOC_ELEM) + ALLOC,
                        "filter" => n * (per + ALLOC_ELEM / 2.0) + ALLOC,
                        "sum" | "len" => n,
                        _ => 20.0 + per,
                    }
            }
            ExprKind::Block(st) => st
                .iter()
                .map(|s| match s {
                    Stmt::For(_, a, b) => self.est(a, depth) + self.count(a) * self.est(b, depth),
                    s => map_stmt_cost(self, s, depth),
                })
                .sum(),
            _ => kids(self, e) + 1.0,
        }
    }

    fn lambda_cost(&self, f: &Expr, depth: usize) -> f64 {
        match &f.kind {
            ExprKind::Lambda { body, .. } => self.est(body, depth),
            ExprKind::Name(n) => self.global_fn(n).map_or(30.0, |g| CALL + self.est(&g.body, depth + 1)),
            _ => self.est(f, depth),
        }
    }

    fn count(&self, e: &Expr) -> f64 {
        match &e.kind {
            ExprKind::Range(a, b) => match (lit_int(a), lit_int(b)) {
                (Some(x), Some(y)) => ((y - x).max(0)) as f64,
                _ => N_UNKNOWN,
            },
            ExprKind::Method { recv, name, .. } if matches!(name.as_str(), "map" | "filter") => self.count(recv),
            ExprKind::List(xs) => xs.len() as f64,
            _ => N_UNKNOWN,
        }
    }

    fn calls_of(&self, n: &str) -> Option<u64> {
        self.prof.as_ref().map(|p| p.calls.get(n).copied().unwrap_or(0))
    }
}

fn map_stmt_cost(o: &Opt, s: &Stmt, depth: usize) -> f64 {
    match s {
        Stmt::Let(_, x) | Stmt::Var(_, x) | Stmt::Assign(_, x, _) | Stmt::Expr(x) => o.est(x, depth),
        Stmt::For(_, a, b) | Stmt::While(a, b) => o.est(a, depth) + N_UNKNOWN * o.est(b, depth),
        Stmt::Fn(_) => 0.0,
    }
}

fn is_chain(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Method { recv, name, args, .. } if matches!(name.as_str(), "map" | "filter") && args.len() == 1 => matches!(recv.kind, ExprKind::Range(..)) || is_chain(recv) || matches!(recv.kind, ExprKind::Name(_) | ExprKind::Call(..)),
        _ => false,
    }
}

// inlining
impl Opt<'_> {
    fn inline_ok(&self, g: &FnDef) -> Result<(), &'static str> {
        if !g.tparams.is_empty() {
            return Err("generic");
        }
        if !g.effects.is_empty() {
            return Err("has effects");
        }
        if !g.pres.is_empty() || !g.posts.is_empty() || g.params.iter().any(|p| p.refine.is_some()) {
            return Err("has contracts");
        }
        let refined = |t: &Ty| matches!(t, Ty::Named { name, .. } if self.alias.contains_key(name) || name.starts_with('&'));
        if g.params.iter().any(|p| refined(&p.ty)) || g.ret.as_ref().is_some_and(refined) {
            return Err("has refined parameter or result types");
        }
        if g.ext.is_some() || g.kernel.is_some() || g.interrupt.is_some() || g.trusted.is_some() {
            return Err("is extern, kernel, interrupt or trusted");
        }
        if self.recursive.contains(&g.name) {
            return Err("is recursive");
        }
        if any_node(&g.body, &mut |x| matches!(x.kind, ExprKind::Return(_) | ExprKind::Raise(_) | ExprKind::Handle(..) | ExprKind::Hole(_) | ExprKind::Table(_) | ExprKind::Par(_) | ExprKind::Catch(..))) || stmt_fn(&g.body) {
            return Err("uses return, raise, handlers, catch, tables or local functions");
        }
        Ok(())
    }

    fn want_inline(&self, g: &FnDef, in_chain: bool, arg_chain: bool) -> Result<String, String> {
        let body = self.est(&g.body, 1);
        let calls = self.calls_of(&g.name);
        if calls == Some(0) {
            return Err("profile: never called".into());
        }
        let n = self.count(&g.body);
        let producer = in_chain && is_chain(&g.body);
        let consumer = arg_chain && g.params.first().is_some_and(|p| head_is(&g.body, &p.name)) && matches!(&g.body.kind, ExprKind::Method { name, .. } if matches!(name.as_str(), "map" | "filter" | "sum" | "len"));
        if producer || consumer {
            return Ok(format!("enables fusion: saves ~{n} list writes ({:.0} instr) per call, body ~{body:.0} instr", n * ALLOC_ELEM));
        }
        if body <= SMALL {
            return Ok(format!("body ~{body:.0} instr <= {SMALL:.0}; saves a call ({CALL:.0} instr)"));
        }
        if let Some(c) = calls.filter(|c| *c >= 10_000)
            && body <= 4.0 * SMALL {
                return Ok(format!("profile: {c} calls, body ~{body:.0} instr"));
            }
        Err(format!("body ~{body:.0} instr > {SMALL:.0}"))
    }

    fn inline_pass(&mut self, e: &Expr, in_chain: bool, stmt_pos: bool) -> Expr {
        let e = match &e.kind {
            ExprKind::Method { recv, name, targs, args } => {
                let chain = matches!(name.as_str(), "map" | "filter" | "sum" | "len") && !self.user_method(e);
                let r = self.inline_pass(recv, chain, false);
                let args = args.iter().map(|a| self.inline_pass(a, false, matches!(a.kind, ExprKind::Lambda { .. }))).collect();
                Expr::new(ExprKind::Method { recv: Box::new(r), name: name.clone(), targs: targs.clone(), args }, e.span)
            }
            ExprKind::Block(st) => {
                let st = st.iter().map(|s| map_stmt(s, &mut |x| self.inline_pass(x, false, true))).collect();
                Expr::new(ExprKind::Block(st), e.span)
            }
            ExprKind::If(c, t, f) => {
                let c = self.inline_pass(c, false, false);
                let t = self.inline_pass(t, false, stmt_pos);
                let f = f.as_ref().map(|f| Box::new(self.inline_pass(f, false, stmt_pos)));
                Expr::new(ExprKind::If(Box::new(c), Box::new(t), f), e.span)
            }
            ExprKind::Lambda { params, body, implicit, .. } => Expr::new(ExprKind::Lambda { params: params.clone(), body: Box::new(self.inline_pass(body, false, !implicit)), implicit: *implicit, tys: vec![] }, e.span),
            _ => map_kids(e, &mut |c| self.inline_pass(c, false, false)),
        };
        let ExprKind::Call(f, args) = &e.kind else { return e };
        let ExprKind::Name(gname) = &f.kind else { return e };
        let Some(g) = self.global_fn(gname).cloned() else { return e };
        if g.params.len() != args.len() || g.name == self.cur || self.budget == 0 {
            return e;
        }
        if let Err(why) = self.inline_ok(&g) {
            self.notes.push(format!("{}: not inlining {gname}: {why}", self.cur));
            return e;
        }
        let arg_chain = args.first().is_some_and(|a| is_chain(a) || matches!(&a.kind, ExprKind::Call(h, _) if matches!(&h.kind, ExprKind::Name(h) if self.global_fn(h).is_some_and(|h| is_chain(&h.body)))));
        let why = match self.want_inline(&g, in_chain, arg_chain) {
            Ok(w) => w,
            Err(w) => {
                self.notes.push(format!("{}: not inlining {gname}: cost: {w}", self.cur));
                return e;
            }
        };
        let params: HashSet<String> = g.params.iter().map(|p| p.name.clone()).collect();
        let inner = binders(&g.body);
        let free: HashSet<String> = names(&g.body).into_iter().filter(|n| !params.contains(n) && !inner.contains(n)).collect();
        if let Some(c) = free.iter().find(|n| self.scope.contains(*n)) {
            self.notes.push(format!("{}: not inlining {gname}: '{c}' would be captured", self.cur));
            return e;
        }
        let trivial = |a: &Expr| matches!(&a.kind, ExprKind::Name(_) | ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Float(_) | ExprKind::Unit) || matches!(&a.kind, ExprKind::Str(p) if p.iter().all(|x| matches!(x, StrPart::Lit(_))));
        let nontrivial = args.iter().filter(|a| !trivial(a)).count();
        let mut body = g.body.clone();
        let mut lets = Vec::new();
        for (p, a) in g.params.iter().zip(args) {
            let captured = names(a).iter().any(|n| inner.contains(n));
            let direct = !captured && (trivial(a) || (nontrivial == 1 && count_free(&body, &p.name) == 1 && head_is(&body, &p.name)));
            if direct {
                body = subst(&body, &p.name, a);
            } else {
                let v = self.fresh(&p.name);
                lets.push(Stmt::Let(Pat::Bind(v.clone()), a.clone()));
                let r = Expr::new(ExprKind::Name(v), Span::default());
                body = subst(&body, &p.name, &r);
            }
        }
        if !lets.is_empty() && !stmt_pos {
            self.notes.push(format!("{}: not inlining {gname}: needs let bindings outside a statement position", self.cur));
            return e;
        }
        let out = if lets.is_empty() {
            body
        } else {
            match body.kind {
                ExprKind::Block(st) => {
                    lets.extend(st);
                    self.mk(ExprKind::Block(lets), self.ty(&e))
                }
                _ => {
                    lets.push(Stmt::Expr(body));
                    self.mk(ExprKind::Block(lets), self.ty(&e))
                }
            }
        };
        self.budget -= 1;
        self.record("inline", &e, &out, format!("construction: {gname} is pure, non-recursive, has no contracts; arguments are bound once in call order"), why);
        out
    }
}

// constant folding
impl Opt<'_> {
    fn fold(&mut self, e: &Expr) -> Expr {
        let e = map_kids(e, &mut |c| self.fold(c));
        let int = |x: &Expr| if let ExprKind::Int(n) = x.kind { Some(n as i128) } else { None };
        let lit = |k: ExprKind| Expr::new(k, e.span);
        let new: Option<(Expr, String)> = match &e.kind {
            ExprKind::Binary(op, a, b) => match (int(a), int(b), op) {
                (Some(x), Some(y), BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem) => {
                    let r = match op {
                        BinOp::Add => Some(x + y),
                        BinOp::Sub => Some(x - y),
                        BinOp::Mul => Some(x * y),
                        BinOp::Div if y != 0 => Some(x / y),
                        BinOp::Rem if y != 0 => Some(x % y),
                        _ => None,
                    };
                    r.filter(|r| *r > FULL.0 && *r <= FULL.1).map(|r| (lit(ExprKind::Int(r as i64)), format!("construction: {x} {} {y} = {r} exactly, within Int range, so no overflow or division trap is removed", op.symbol())))
                }
                (Some(x), Some(y), BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne) => {
                    let r = match op {
                        BinOp::Lt => x < y,
                        BinOp::Le => x <= y,
                        BinOp::Gt => x > y,
                        BinOp::Ge => x >= y,
                        BinOp::Eq => x == y,
                        _ => x != y,
                    };
                    Some((lit(ExprKind::Bool(r)), "construction: comparison of literals".into()))
                }
                _ => match (&a.kind, &b.kind, op) {
                    (ExprKind::Bool(true), _, BinOp::And) | (ExprKind::Bool(false), _, BinOp::Or) => Some(((**b).clone(), "construction: neutral left operand; the right operand is evaluated either way".into())),
                    (ExprKind::Bool(false), _, BinOp::And) | (ExprKind::Bool(true), _, BinOp::Or) => Some(((**a).clone(), "construction: short-circuit; the right operand was never evaluated".into())),
                    (_, ExprKind::Bool(true), BinOp::And) | (_, ExprKind::Bool(false), BinOp::Or) => Some(((**a).clone(), "construction: x and true = x, x or false = x".into())),
                    _ if self.ty(&e).is_some_and(|t| is_ty(&t, "Int")) => match (int(a), int(b), op) {
                        (_, Some(0), BinOp::Add | BinOp::Sub) | (_, Some(1), BinOp::Mul | BinOp::Div) => Some(((**a).clone(), format!("construction: x {} {} = x for Int and cannot trap", op.symbol(), int(b).unwrap()))),
                        (Some(0), _, BinOp::Add) | (Some(1), _, BinOp::Mul) => Some(((**b).clone(), format!("construction: {} {} x = x for Int and cannot trap", int(a).unwrap(), op.symbol()))),
                        _ => None,
                    },
                    _ => None,
                },
            },
            ExprKind::Unary(UnOp::Not, x) => match x.kind {
                ExprKind::Bool(v) => Some((lit(ExprKind::Bool(!v)), "construction: not of a literal".into())),
                _ => None,
            },
            ExprKind::If(c, t, f) => match c.kind {
                ExprKind::Bool(true) => Some(((**t).clone(), "construction: literal condition".into())),
                ExprKind::Bool(false) => Some((f.as_ref().map_or_else(|| lit(ExprKind::Unit), |f| (**f).clone()), "construction: literal condition".into())),
                _ => None,
            },
            _ => None,
        };
        match new {
            Some((out, proof)) => {
                self.record("fold", &e, &out, proof, "fewer operations".into());
                out
            }
            None => e,
        }
    }
}

// dead-branch elimination
impl Opt<'_> {
    fn assume(&self, c: &Expr, pos: bool, env: &mut Facts) {
        match &c.kind {
            ExprKind::Binary(BinOp::And, a, b) if pos => {
                self.assume(a, true, env);
                self.assume(b, true, env);
                return;
            }
            ExprKind::Binary(BinOp::Or, a, b) if !pos => {
                self.assume(a, false, env);
                self.assume(b, false, env);
                return;
            }
            ExprKind::Unary(UnOp::Not, a) => {
                self.assume(a, !pos, env);
                return;
            }
            ExprKind::Binary(op, a, b) => {
                let op = if pos { Some(*op) } else { neg_cmp(*op) };
                let one = |env: &mut Facts, x: &str, op: BinOp, k: i128| {
                    let cur = env.iv.get(x).copied().unwrap_or(FULL);
                    let n = match op {
                        BinOp::Lt => (FULL.0, k - 1),
                        BinOp::Le => (FULL.0, k),
                        BinOp::Gt => (k + 1, FULL.1),
                        BinOp::Ge => (k, FULL.1),
                        BinOp::Eq => (k, k),
                        _ => FULL,
                    };
                    env.iv.insert(x.to_string(), meet(cur, n));
                };
                if let Some(op) = op {
                    match (&a.kind, &b.kind) {
                        (ExprKind::Name(x), _) if env.ints.contains(x) && lit_int(b).is_some() => one(env, x, op, lit_int(b).unwrap()),
                        (_, ExprKind::Name(x)) if env.ints.contains(x) && lit_int(a).is_some() => {
                            if let Some(f) = flip_cmp(op) {
                                one(env, x, f, lit_int(a).unwrap())
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        env.rel.push((c.clone(), pos));
    }

    fn eval_iv(&self, e: &Expr, env: &Facts) -> Option<Iv> {
        match &e.kind {
            ExprKind::Int(n) => Some((*n as i128, *n as i128)),
            ExprKind::Name(x) if env.ints.contains(x) => Some(env.iv.get(x).copied().unwrap_or(FULL)),
            ExprKind::Unary(UnOp::Neg, x) => self.eval_iv(x, env).map(|i| (-i.1, -i.0)),
            ExprKind::Binary(op @ (BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem), a, b) => raw_op(*op, self.eval_iv(a, env)?, self.eval_iv(b, env)?),
            ExprKind::Method { name, args, .. } if name == "len" && args.is_empty() && !self.user_method(e) => Some((0, 1 << 47)),
            _ => None,
        }
    }

    fn safe_iv(&self, e: &Expr, env: &Facts) -> bool {
        match &e.kind {
            ExprKind::Binary(op, a, b) if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem) => {
                let (x, y) = match (self.eval_iv(a, env), self.eval_iv(b, env)) {
                    (Some(x), Some(y)) => (x, y),
                    _ => return false,
                };
                let ok = match op {
                    BinOp::Div | BinOp::Rem => (y.0 > 0 || y.1 < 0) && !(x.0 == FULL.0 && y.0 <= -1 && y.1 >= -1),
                    _ => raw_op(*op, x, y).is_some_and(fits),
                };
                ok && self.safe_iv(a, env) && self.safe_iv(b, env)
            }
            ExprKind::Binary(_, a, b) => self.safe_iv(a, env) && self.safe_iv(b, env),
            ExprKind::Unary(UnOp::Neg, x) => self.eval_iv(x, env).is_some_and(|i| i.0 > FULL.0) && self.safe_iv(x, env),
            ExprKind::Unary(UnOp::Not, x) => self.safe_iv(x, env),
            ExprKind::Int(_) | ExprKind::Bool(_) => true,
            ExprKind::Name(x) => env.ints.contains(x) || self.ty(e).is_some_and(|t| is_ty(&t, "Bool")),
            ExprKind::Method { recv, name, args, .. } => name == "len" && args.is_empty() && !self.user_method(e) && matches!(recv.kind, ExprKind::Name(_)),
            _ => false,
        }
    }

    fn eval_bool(&self, c: &Expr, env: &Facts) -> Option<bool> {
        match &c.kind {
            ExprKind::Bool(b) => Some(*b),
            ExprKind::Unary(UnOp::Not, x) => self.eval_bool(x, env).map(|b| !b),
            ExprKind::Binary(BinOp::And, a, b) => match (self.eval_bool(a, env), self.eval_bool(b, env)) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) => Some(true),
                _ => None,
            },
            ExprKind::Binary(BinOp::Or, a, b) => match (self.eval_bool(a, env), self.eval_bool(b, env)) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) => Some(false),
                _ => None,
            },
            ExprKind::Binary(op, a, b) => {
                let (x, y) = (self.eval_iv(a, env)?, self.eval_iv(b, env)?);
                match op {
                    BinOp::Lt if x.1 < y.0 => Some(true),
                    BinOp::Lt if x.0 >= y.1 => Some(false),
                    BinOp::Le if x.1 <= y.0 => Some(true),
                    BinOp::Le if x.0 > y.1 => Some(false),
                    BinOp::Gt if x.0 > y.1 => Some(true),
                    BinOp::Gt if x.1 <= y.0 => Some(false),
                    BinOp::Ge if x.0 >= y.1 => Some(true),
                    BinOp::Ge if x.1 < y.0 => Some(false),
                    BinOp::Eq if x.0 == x.1 && y.0 == y.1 && x.0 == y.0 => Some(true),
                    BinOp::Eq | BinOp::Ne if x.1 < y.0 || y.1 < x.0 => Some(*op == BinOp::Ne),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn smt_int(&self, e: &Expr, env: &Facts, vars: &mut BTreeSet<String>, ovf: &mut Vec<String>) -> Option<String> {
        Some(match &e.kind {
            ExprKind::Int(n) => {
                if *n < 0 {
                    format!("(- {})", -(*n as i128))
                } else {
                    n.to_string()
                }
            }
            ExprKind::Name(x) if env.ints.contains(x) => {
                vars.insert(format!("v_{x}"));
                format!("v_{x}")
            }
            ExprKind::Method { recv, name, args, .. } if name == "len" && args.is_empty() && !self.user_method(e) => match &recv.kind {
                ExprKind::Name(x) => {
                    vars.insert(format!("l_{x}"));
                    format!("l_{x}")
                }
                _ => return None,
            },
            ExprKind::Unary(UnOp::Neg, x) => {
                let t = format!("(- {})", self.smt_int(x, env, vars, ovf)?);
                ovf.push(format!("(> {t} {})", FULL.1));
                t
            }
            ExprKind::Binary(op @ (BinOp::Add | BinOp::Sub | BinOp::Mul), a, b) => {
                let t = format!("({} {} {})", op.symbol(), self.smt_int(a, env, vars, ovf)?, self.smt_int(b, env, vars, ovf)?);
                ovf.push(format!("(or (< {t} (- {})) (> {t} {}))", -FULL.0, FULL.1));
                t
            }
            _ => return None,
        })
    }

    fn smt_bool(&self, c: &Expr, env: &Facts, vars: &mut BTreeSet<String>, ovf: &mut Vec<String>) -> Option<String> {
        Some(match &c.kind {
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Unary(UnOp::Not, x) => format!("(not {})", self.smt_bool(x, env, vars, ovf)?),
            ExprKind::Binary(BinOp::And, a, b) => format!("(and {} {})", self.smt_bool(a, env, vars, ovf)?, self.smt_bool(b, env, vars, ovf)?),
            ExprKind::Binary(BinOp::Or, a, b) => format!("(or {} {})", self.smt_bool(a, env, vars, ovf)?, self.smt_bool(b, env, vars, ovf)?),
            ExprKind::Binary(op @ (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne), a, b) => {
                let (x, y) = (self.smt_int(a, env, vars, ovf)?, self.smt_int(b, env, vars, ovf)?);
                match op {
                    BinOp::Eq => format!("(= {x} {y})"),
                    BinOp::Ne => format!("(not (= {x} {y}))"),
                    _ => format!("({} {x} {y})", op.symbol()),
                }
            }
            _ => return None,
        })
    }

    fn smt_decide(&mut self, c: &Expr, env: &Facts) -> Option<bool> {
        let mut vars = BTreeSet::new();
        let mut covf = Vec::new();
        let goal = self.smt_bool(c, env, &mut vars, &mut covf)?;
        let mut facts = String::new();
        for (f, pos) in &env.rel {
            let mut fovf = Vec::new();
            if let Some(t) = self.smt_bool(f, env, &mut vars, &mut fovf) {
                facts.push_str(&format!("(assert {})\n", if *pos { t } else { format!("(not {t})") }));
                for o in fovf {
                    facts.push_str(&format!("(assert (not {o}))\n"));
                }
            }
        }
        let mut decls = String::new();
        for v in &vars {
            let (lo, hi) = if let Some(x) = v.strip_prefix("v_") { env.iv.get(x).copied().unwrap_or(FULL) } else { (0, 1 << 47) };
            let lo = lo.max(FULL.0);
            let hi = hi.min(FULL.1);
            decls.push_str(&format!("(declare-const {v} Int)\n(assert (<= {} {v} {hi}))\n", if lo < 0 { format!("(- {})", -lo) } else { lo.to_string() }));
        }
        let solver = self.smt.as_mut()?;
        let base = format!("{decls}{facts}");
        if !covf.is_empty() && solver.check(&format!("{base}(assert (or false {}))\n", covf.join(" ")), &[]) != sspur_smt::Answer::Unsat {
            return None;
        }
        if solver.check(&format!("{base}(assert (not {goal}))\n"), &[]) == sspur_smt::Answer::Unsat {
            return Some(true);
        }
        if solver.check(&format!("{base}(assert {goal})\n"), &[]) == sspur_smt::Answer::Unsat {
            return Some(false);
        }
        None
    }

    fn decide(&mut self, c: &Expr, env: &Facts) -> Option<(bool, &'static str)> {
        if matches!(c.kind, ExprKind::Bool(_)) || !self.pure(c) {
            return None;
        }
        if self.safe_iv(c, env)
            && let Some(b) = self.eval_bool(c, env) {
                return Some((b, "intervals"));
            }
        self.smt_decide(c, env).map(|b| (b, "z3"))
    }

    fn branch_pass(&mut self, e: &Expr, env: &Facts) -> Expr {
        match &e.kind {
            ExprKind::If(c, t, f) => {
                let c2 = self.branch_pass(c, env);
                if let Some((b, how)) = self.decide(&c2, env) {
                    let live = if b { Some((**t).clone()) } else { f.as_ref().map(|f| (**f).clone()) };
                    let out = match live {
                        Some(x) => self.branch_pass(&x, env),
                        None => Expr::new(ExprKind::Unit, e.span),
                    };
                    let facts: Vec<String> = env.rel.iter().map(|(x, p)| if *p { one_line(x) } else { format!("not ({})", one_line(x)) }).collect();
                    let ctx = if facts.is_empty() { String::new() } else { format!(" given {}", facts.join(", ")) };
                    let proof = format!("{how}: condition '{}' is always {b}{ctx}; it is pure and cannot trap", one_line(&c2));
                    let dead = Expr::new(ExprKind::If(Box::new(c2), t.clone(), f.clone()), e.span);
                    self.record("dead-branch", &dead, &out, proof, "removes a test and a branch".into());
                    return out;
                }
                let mut te = env.clone();
                self.assume(&c2, true, &mut te);
                let mut fe = env.clone();
                self.assume(&c2, false, &mut fe);
                let t2 = self.branch_pass(t, &te);
                let f2 = f.as_ref().map(|f| Box::new(self.branch_pass(f, &fe)));
                Expr::new(ExprKind::If(Box::new(c2), Box::new(t2), f2), e.span)
            }
            ExprKind::Lambda { params, body, implicit, .. } => {
                let mut inner = env.clone();
                inner.forget_all(&params.iter().cloned().collect());
                inner.forget_all(&binders(body));
                Expr::new(ExprKind::Lambda { params: params.clone(), body: Box::new(self.branch_pass(body, &inner)), implicit: *implicit, tys: vec![] }, e.span)
            }
            ExprKind::Match(..) | ExprKind::Catch(..) | ExprKind::Handle(..) | ExprKind::Table(_) => {
                let mut inner = env.clone();
                inner.forget_all(&binders(e));
                map_kids(e, &mut |c| self.branch_pass(c, &inner))
            }
            ExprKind::Block(st) => {
                let mut env = env.clone();
                let mut out = Vec::new();
                for s in st {
                    let ns = match s {
                        Stmt::Let(p, v) => {
                            let v = self.branch_pass(v, &env);
                            let mut bound = HashSet::new();
                            pat_names(p, &mut bound);
                            env.forget_all(&bound);
                            if let Pat::Bind(x) = p
                                && self.ty(&v).is_some_and(|t| is_ty(&t, "Int"))
                                    && self.safe_iv(&v, &env)
                                    && let Some(i) = self.eval_iv(&v, &env) {
                                        env.ints.insert(x.clone());
                                        env.iv.insert(x.clone(), clamp(i));
                                    }
                            Stmt::Let(p.clone(), v)
                        }
                        Stmt::Var(n, v) => {
                            let v = self.branch_pass(v, &env);
                            env.forget(n);
                            Stmt::Var(n.clone(), v)
                        }
                        Stmt::Assign(n, v, sp) => {
                            let v = self.branch_pass(v, &env);
                            env.forget(n);
                            Stmt::Assign(n.clone(), v, *sp)
                        }
                        Stmt::For(p, a, b) => {
                            let a = self.branch_pass(a, &env);
                            let mut inner = env.clone();
                            inner.forget_all(&binders(b));
                            let mut bound = HashSet::new();
                            pat_names(p, &mut bound);
                            inner.forget_all(&bound);
                            for (s, _) in assigned(b) {
                                inner.forget(&s);
                            }
                            if let (Pat::Bind(x), ExprKind::Range(lo, hi)) = (p, &a.kind)
                                && let (Some(l), Some(h)) = (self.eval_iv(lo, &inner), self.eval_iv(hi, &inner)) {
                                    inner.ints.insert(x.clone());
                                    inner.iv.insert(x.clone(), clamp((l.0, h.1 - 1)));
                                }
                            let b = self.branch_pass(b, &inner);
                            Stmt::For(p.clone(), a, b)
                        }
                        Stmt::While(c, b) => {
                            let mut inner = env.clone();
                            for (s, _) in assigned(b).into_iter().chain(assigned(c)) {
                                inner.forget(&s);
                            }
                            inner.forget_all(&binders(b));
                            Stmt::While(self.branch_pass(c, &inner), self.branch_pass(b, &inner))
                        }
                        Stmt::Expr(v) => Stmt::Expr(self.branch_pass(v, &env)),
                        Stmt::Fn(_) => s.clone(),
                    };
                    out.push(ns);
                }
                Expr::new(ExprKind::Block(out), e.span)
            }
            _ => map_kids(e, &mut |c| self.branch_pass(c, env)),
        }
    }
}

fn assigned(e: &Expr) -> Vec<(String, ())> {
    let mut out = Vec::new();
    visit::walk_expr(e, &mut |x| {
        if let ExprKind::Block(st) = &x.kind {
            for s in st {
                if let Stmt::Assign(n, _, _) | Stmt::Var(n, _) = s {
                    out.push((n.clone(), ()));
                }
            }
        }
        true
    });
    out
}

struct Stage {
    kind: &'static str,
    param: String,
    body: Expr,
}

// fusion
impl Opt<'_> {
    fn as_lambda(&mut self, f: &Expr) -> Option<(String, Expr)> {
        match &f.kind {
            ExprKind::Lambda { params, body, implicit: false, .. } if params.len() == 1 => Some((params[0].clone(), (**body).clone())),
            ExprKind::Lambda { params, body, implicit: true, .. } if params.len() == 1 => {
                let v = self.fresh("x");
                let r = Expr::new(ExprKind::Name(v.clone()), Span::default());
                Some((v, replace_ph(body, &r)))
            }
            ExprKind::Name(n) => {
                let g = self.global_fn(n)?;
                if g.params.len() != 1 || !g.tparams.is_empty() {
                    return None;
                }
                let v = self.fresh("x");
                let call = Expr::new(ExprKind::Call(Box::new(f.clone()), vec![Expr::new(ExprKind::Name(v.clone()), Span::default())]), Span::default());
                Some((v, call))
            }
            _ => None,
        }
    }

    fn stage_ok(&self, body: &Expr) -> bool {
        self.pure(body) && !any_node(body, &mut |x| matches!(x.kind, ExprKind::Table(_))) && !stmt_fn(body)
    }

    fn chain(&mut self, e: &Expr) -> Option<(Expr, Vec<(Stage, Expr)>)> {
        let mut stages = Vec::new();
        let mut cur = e;
        while let ExprKind::Method { recv, name, args, .. } = &cur.kind {
            if !matches!(name.as_str(), "map" | "filter") || args.len() != 1 || self.user_method(cur) {
                break;
            }
            if !self.ty(cur).is_some_and(|t| list_elem(&t).is_some()) {
                return None;
            }
            let (param, body) = self.as_lambda(&args[0])?;
            stages.push((Stage { kind: if name == "map" { "map" } else { "filter" }, param, body }, args[0].clone()));
            cur = recv;
        }
        stages.reverse();
        Some((cur.clone(), stages))
    }

    fn source_av(&self, src: &Expr) -> Av {
        match &src.kind {
            ExprKind::Range(a, b) => match (self.av(a, &HashMap::new()), self.av(b, &HashMap::new())) {
                (Av::Int(x), Av::Int(y)) => Av::Int(clamp((x.0, y.1 - 1))),
                _ => Av::Int(FULL),
            },
            _ => self.ty(src).as_ref().and_then(list_elem).map_or(Av::Any, |t| self.av_of_type(t)),
        }
    }

    fn analyze(&mut self, src: &Expr, stages: &[(Stage, Expr)], consumer: Option<&str>) -> Result<(Vec<(String, Traps)>, bool), String> {
        let mut av = self.source_av(src);
        let mut ts = Vec::new();
        self.iv_used = false;
        for (k, (s, _)) in stages.iter().enumerate() {
            if !self.stage_ok(&s.body) {
                return Err(format!("{} #{} is not pure", s.kind, k + 1));
            }
            let env = HashMap::from([(s.param.clone(), av.clone())]);
            let t = self.traps(&s.body, &env, 0);
            ts.push((format!("{} #{} ({})", s.kind, k + 1, traps_text(&t)), t));
            if s.kind == "map" {
                av = self.av(&s.body, &env);
            }
        }
        match consumer {
            Some("sum") => ts.push(("sum (overflow)".into(), Some(BTreeSet::from([Trap::Overflow])))),
            Some(c) => ts.push((format!("{c} (none)"), Some(BTreeSet::new()))),
            None => {}
        }
        Ok((ts, self.iv_used))
    }

    fn compose(&mut self, e: &Expr) -> Option<Expr> {
        let ExprKind::Method { recv, name, args, .. } = &e.kind else { return None };
        if name != "map" || args.len() != 1 || self.user_method(e) {
            return None;
        }
        let ExprKind::Method { recv: src, name: n2, args: a2, .. } = &recv.kind else { return None };
        if n2 != "map" || a2.len() != 1 || self.user_method(recv) || !self.ty(recv).is_some_and(|t| list_elem(&t).is_some()) {
            return None;
        }
        let (fp, fb) = self.as_lambda(&a2[0])?;
        let (gp, gb) = self.as_lambda(&args[0])?;
        let stages = vec![(Stage { kind: "map", param: fp.clone(), body: fb.clone() }, a2[0].clone()), (Stage { kind: "map", param: gp.clone(), body: gb.clone() }, args[0].clone())];
        if self.cgen_fuses(&stages, "map") {
            self.notes.push(format!("{}: keeping map.map: cost: native code fuses it in place", self.cur));
            return None;
        }
        let (ts, by_iv) = match self.analyze(src, &stages, None) {
            Ok(x) => x,
            Err(w) => {
                self.notes.push(format!("{}: not fusing maps: {w}", self.cur));
                return None;
            }
        };
        let Some(why) = order_safe(&ts) else {
            self.notes.push(format!("{}: not fusing maps: trap order could change ({})", self.cur, ts.iter().map(|t| t.0.clone()).collect::<Vec<_>>().join(", ")));
            return None;
        };
        let v = self.fresh("x");
        let vx = Expr::new(ExprKind::Name(v.clone()), Span::default());
        let f_at = subst(&fb, &fp, &vx);
        let captured = names(&f_at).iter().any(|n| binders(&gb).contains(n));
        let body = if !captured && count_free(&gb, &gp) == 1 && head_is(&gb, &gp) {
            subst(&gb, &gp, &f_at)
        } else {
            let y = self.fresh("y");
            let g_at = subst(&gb, &gp, &Expr::new(ExprKind::Name(y.clone()), Span::default()));
            Expr::new(ExprKind::Block(vec![Stmt::Let(Pat::Bind(y), f_at), Stmt::Expr(g_at)]), Span::default())
        };
        let lam = Expr::new(ExprKind::Lambda { params: vec![v], body: Box::new(body), implicit: false, tys: vec![] }, Span::default());
        let out = self.mk(ExprKind::Method { recv: src.clone(), name: "map".into(), targs: vec![], args: vec![lam] }, self.ty(e));
        let how = if by_iv { "intervals" } else { "construction" };
        self.record("map-map", e, &out, format!("{how}: both stages pure; {why} ({})", ts.iter().map(|t| t.0.clone()).collect::<Vec<_>>().join(", ")), format!("saves one intermediate list (~{:.0} instr per element)", ALLOC_ELEM));
        Some(out)
    }

    fn cgen_fuses(&self, stages: &[(Stage, Expr)], consumer: &str) -> bool {
        let mut codes = BTreeSet::new();
        if consumer == "sum" {
            codes.insert(crate::T_OVERFLOW);
        }
        for (_, orig) in stages {
            let ok = matches!(&orig.kind, ExprKind::Lambda { body, params, .. } if params.len() == 1 && super::fuse::traps(body, &mut codes));
            if !ok {
                return false;
            }
        }
        codes.len() <= 1
    }

    fn loop_fuse(&mut self, e: &Expr) -> Option<Expr> {
        let ExprKind::Method { recv, name, args, .. } = &e.kind else { return None };
        if !matches!(name.as_str(), "sum" | "len") || !args.is_empty() || self.user_method(e) {
            return None;
        }
        if name == "sum" && !self.ty(e).is_some_and(|t| is_ty(&t, "Int")) {
            return None;
        }
        let (src, stages) = self.chain(recv)?;
        if stages.is_empty() {
            return None;
        }
        if self.cgen_fuses(&stages, name) {
            self.notes.push(format!("{}: keeping a {}-stage pipeline: cost: native code fuses it in place (vector and parallel paths)", self.cur, stages.len() + 1));
            return None;
        }
        let n = self.count(&src);
        let per: f64 = stages.iter().map(|(s, _)| self.est(&s.body, 1)).sum();
        let unfused = n * (per + ALLOC_ELEM * stages.len() as f64) + ALLOC * stages.len() as f64;
        let fused = n * (per + 2.0);
        if fused >= unfused {
            return None;
        }
        let (ts, by_iv) = match self.analyze(&src, &stages, Some(name)) {
            Ok(x) => x,
            Err(w) => {
                self.notes.push(format!("{}: not fusing pipeline: {w}", self.cur));
                return None;
            }
        };
        let Some(why) = order_safe(&ts) else {
            self.notes.push(format!("{}: not fusing pipeline: trap order could change ({})", self.cur, ts.iter().map(|t| t.0.clone()).collect::<Vec<_>>().join(", ")));
            return None;
        };
        let acc = self.fresh("acc");
        let x0 = self.fresh("x");
        let mut vars = vec![x0.clone()];
        for (s, _) in &stages {
            if s.kind == "map" {
                let y = self.fresh("x");
                vars.push(y);
            } else {
                let last = vars.last().unwrap().clone();
                vars.push(last);
            }
        }
        let name_e = |n: &str| Expr::new(ExprKind::Name(n.to_string()), Span::default());
        let last = vars.last().unwrap().clone();
        let step = if name == "sum" { name_e(&last) } else { Expr::new(ExprKind::Int(1), Span::default()) };
        let mut inner = vec![Stmt::Assign(acc.clone(), Expr::new(ExprKind::Binary(BinOp::Add, Box::new(name_e(&acc)), Box::new(step)), Span::default()), Span::default())];
        for (k, (s, _)) in stages.iter().enumerate().rev() {
            let body = subst(&s.body, &s.param, &name_e(&vars[k]));
            if s.kind == "map" {
                let mut st = vec![Stmt::Let(Pat::Bind(vars[k + 1].clone()), body)];
                st.extend(inner);
                inner = st;
            } else {
                let blk = Expr::new(ExprKind::Block(inner), Span::default());
                inner = vec![Stmt::Expr(Expr::new(ExprKind::If(Box::new(body), Box::new(blk), None), Span::default()))];
            }
        }
        let lp = Stmt::For(Pat::Bind(x0), src.clone(), Expr::new(ExprKind::Block(inner), Span::default()));
        let out = self.mk(ExprKind::Block(vec![Stmt::Var(acc.clone(), Expr::new(ExprKind::Int(0), Span::default())), lp, Stmt::Expr(name_e(&acc))]), self.ty(e));
        let how = if by_iv { "intervals" } else { "construction" };
        self.record("loop-fusion", e, &out, format!("{how}: stages pure; {why} ({})", ts.iter().map(|t| t.0.clone()).collect::<Vec<_>>().join(", ")), format!("unfused ~{:.0} instr per element ({} lists built), fused ~{:.0}", unfused / n.max(1.0), stages.len(), fused / n.max(1.0)));
        Some(out)
    }

    fn fuse_pass(&mut self, e: &Expr, stmt_pos: bool) -> Expr {
        let e = match &e.kind {
            ExprKind::Block(st) => Expr::new(ExprKind::Block(st.iter().map(|s| map_stmt(s, &mut |x| self.fuse_pass(x, true))).collect()), e.span),
            ExprKind::If(c, t, f) => {
                let c = self.fuse_pass(c, false);
                let t = self.fuse_pass(t, stmt_pos);
                let f = f.as_ref().map(|f| Box::new(self.fuse_pass(f, stmt_pos)));
                Expr::new(ExprKind::If(Box::new(c), Box::new(t), f), e.span)
            }
            ExprKind::Lambda { params, body, implicit: false, .. } => Expr::new(ExprKind::Lambda { params: params.clone(), body: Box::new(self.fuse_pass(body, true)), implicit: false, tys: vec![] }, e.span),
            _ => map_kids(e, &mut |c| self.fuse_pass(c, false)),
        };
        if let Some(x) = self.compose(&e) {
            return x;
        }
        if stmt_pos
            && let Some(x) = self.loop_fuse(&e) {
                return x;
            }
        e
    }
}

fn eligible_fn(f: &FnDef) -> bool {
    f.ext.is_none() && f.kernel.is_none() && !sspur_check::kernel::is_device_fn(f) && f.interrupt.is_none() && f.trusted.is_none() && !any_node(&f.body, &mut |x| matches!(x.kind, ExprKind::Hole(_) | ExprKind::Handle(..)))
}

impl Opt<'_> {
    fn facts_of(&self, f: &FnDef) -> Facts {
        let mut env = Facts::default();
        for p in &f.params {
            let int = matches!(&p.ty, Ty::Named { name, args, .. } if args.is_empty() && (name == "Int" || self.alias.get(name).is_some_and(|(t, _)| matches!(t, Ty::Named { name, .. } if name == "Int"))));
            if !int {
                continue;
            }
            env.ints.insert(p.name.clone());
            let me = Expr::new(ExprKind::Name(p.name.clone()), Span::default());
            let mut rs = Vec::new();
            if let Some(r) = &p.refine {
                rs.push(r.clone());
            }
            if let Ty::Named { name, .. } = &p.ty
                && let Some((_, r)) = self.alias.get(name) {
                    rs.push(r.clone());
                }
            for r in rs {
                let c = replace_ph(&r, &me);
                self.assume(&c, true, &mut env);
            }
        }
        for pre in &f.pres {
            if self.pure(pre) {
                self.assume(pre, true, &mut env);
            }
        }
        env
    }

    fn function(&mut self, f: &FnDef) -> Option<Expr> {
        self.cur = f.name.clone();
        self.scope = binders(&f.body);
        self.scope.extend(f.params.iter().map(|p| p.name.clone()));
        self.budget = 16;
        let start = self.log.len();
        let mut body = self.fns[&f.name].body.clone();
        for _ in 0..3 {
            let before = self.log.len();
            body = self.inline_pass(&body, false, true);
            self.scope.extend(binders(&body));
            body = self.fold(&body);
            let env = self.facts_of(f);
            body = self.branch_pass(&body, &env);
            body = self.fuse_pass(&body, true);
            self.scope.extend(binders(&body));
            if self.log.len() == before {
                break;
            }
        }
        (self.log.len() > start).then(|| self.fieldize(&body))
    }
}

pub fn optimize(m: &Module, check: &CheckOutput) -> Option<Optimized> {
    run(m, check).ok()
}

pub fn explain(m: &Module, check: &CheckOutput) -> (Vec<Rewrite>, Vec<String>) {
    match run(m, check) {
        Ok(o) => (o.rewrites, o.notes),
        Err(n) => (vec![], n),
    }
}

fn run(m: &Module, check: &CheckOutput) -> Result<Optimized, Vec<String>> {
    if matches!(std::env::var("SSPUR_OPT").as_deref(), Ok("0") | Ok("off")) || matches!(m.profile.as_deref(), Some("sys" | "bare")) || check.has_errors() {
        return Err(vec!["optimizer off for this module (SSPUR_OPT=0, a sys or bare profile, or check errors)".into()]);
    }
    let mut skip: HashSet<String> = HashSet::new();
    let mut notes = Vec::new();
    for _ in 0..4 {
        let mut o = Opt::new(m, check);
        let mut out = m.clone();
        for d in out.defs.iter_mut() {
            let Def::Fn(f) = d else { continue };
            if skip.contains(&f.name) || !eligible_fn(f) {
                continue;
            }
            let start = o.log.len();
            if let Some(b) = o.function(f) {
                let mut g = f.clone();
                g.body = b;
                if sspur_syntax::parse(&printer::print_def(&Def::Fn(g.clone()))).is_ok() {
                    *f = g;
                } else {
                    o.notes.push(format!("{}: rewritten text does not parse; kept the original", f.name));
                    o.log.truncate(start);
                }
            }
        }
        if o.log.is_empty() {
            notes.extend(o.notes);
            return Err(notes);
        }
        let text = printer::print_module(&out);
        let parsed = match sspur_syntax::parse(&text) {
            Ok(p) => p,
            Err(_) => {
                notes.push("optimized module does not parse; optimizations dropped".to_string());
                return Err(notes);
            }
        };
        let rc = sspur_check::check(&parsed);
        let bad: HashSet<String> = rc.diags.iter().filter(|d| d.is_error()).map(|d| d.def.clone().unwrap_or_default()).collect();
        if bad.is_empty() {
            notes.extend(o.notes);
            let mut seen = HashSet::new();
            notes.retain(|n| seen.insert(n.clone()));
            return Ok(Optimized { module: parsed, check: rc, rewrites: o.log, notes });
        }
        let changed: HashSet<String> = o.log.iter().map(|r| r.func.clone()).collect();
        let culprits: Vec<String> = bad.iter().filter(|n| changed.contains(*n)).cloned().collect();
        if culprits.is_empty() {
            return Err(notes);
        }
        for c in culprits {
            notes.push(format!("{c}: rewritten body does not typecheck; kept the original"));
            skip.insert(c);
        }
    }
    Err(notes)
}
