//! Drops at last use (ADR 0033). A list buffer or string block that the compiler proves is
//! uniquely owned is freed right after its last use; everything else stays with the collector.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Rk {
    List,
    Str,
    /// A list of strings that each belong to it alone: freeing it frees them too.
    Strs,
}

/// An owned local: `v` is bound at statement `at`, is only borrowed afterwards, and dies after
/// statement `last` (or is the block's result when `moved`).
pub(super) struct RcPlan {
    pub v: String,
    pub k: Rk,
    pub last: usize,
    pub moved: bool,
    pub var: bool,
    /// The key expression of the map `put` that takes `v` over: the put frees it if the key was
    /// already present, so no drop follows.
    pub consumed: Option<usize>,
}

/// Uses of a list whose strings it owns that never copy a string out of it.
const DEEP_READS: &[&str] = &["len", "is_empty", "join", "contains"];

const LIST_READS: &[&str] = &[
    "len", "is_empty", "get", "first", "last", "sum", "contains", "map", "flat_map", "filter", "fold", "any", "all", "find", "min", "max", "sort", "sort_by", "unique", "enumerate", "zip", "counts", "reverse",
];
const LIST_FRESH: &[&str] = &["map", "filter", "reverse", "sort", "sort_by", "unique", "enumerate", "zip", "flat_map", "counts"];
const STR_READS: &[&str] = &["len", "is_empty", "byte_len", "byte", "lower", "upper", "contains", "starts_with", "ends_with", "replace", "repeat", "to_int", "is_alpha", "compare_ci"];
const STR_FRESH: &[&str] = &["lower", "upper", "replace", "repeat", "reverse"];
const STR_LIST_FRESH: &[&str] = &["split", "words", "chars"];
/// Call names the code generator handles itself even when a user function has the same name.
const BUILTIN_CALLS: &[&str] = &["log", "some", "min", "max", "__cmp", "__hash", "empty_map", "secret", "pii", "untrusted", "guess", "drop", "leak", "alloc", "free", "null"];

pub(super) fn rk_of(t: &Type) -> Option<Rk> {
    if is(t, "Str") {
        Some(Rk::Str)
    } else if elem(t, "List").is_some() {
        Some(Rk::List)
    } else {
        None
    }
}

pub(super) fn drop_code(k: Rk, c: &str) -> String {
    match k {
        Rk::List => format!("ss_drop_l({c}); "),
        Rk::Str => format!("ss_drop_s({c}); "),
        Rk::Strs => format!("ss_drop_ls({c}); "),
    }
}

fn key(e: &Expr) -> usize {
    e as *const Expr as usize
}

/// Mentions of `v` as a name or as an assignment target.
fn touches(e: &Expr, v: &str) -> bool {
    let mut hit = false;
    visit::walk_expr(e, &mut |x| {
        match &x.kind {
            ExprKind::Name(n) if n == v => hit = true,
            ExprKind::Block(stmts) if stmts.iter().any(|s| matches!(s, Stmt::Assign(n, _, _) if n == v)) => hit = true,
            _ => {}
        }
        !hit
    });
    hit
}

fn assigns(e: &Expr, v: &str) -> bool {
    let mut hit = false;
    visit::walk_expr(e, &mut |x| {
        if let ExprKind::Block(stmts) = &x.kind
            && stmts.iter().any(|s| matches!(s, Stmt::Assign(n, _, _) if n == v))
        {
            hit = true;
        }
        !hit
    });
    hit
}

fn pat_names(p: &Pat) -> Vec<String> {
    let mut out = Vec::new();
    p.binds(&mut out);
    out
}

fn has_return(e: &Expr) -> bool {
    let mut hit = false;
    visit::walk_expr(e, &mut |x| {
        if matches!(x.kind, ExprKind::Return(_)) {
            hit = true;
        }
        !hit && !matches!(x.kind, ExprKind::Lambda { .. })
    });
    hit
}

impl Cx<'_> {
    pub(super) fn rc_on(&self) -> bool {
        !self.rc_off && !self.sys && !self.bare && self.target.is_none() && !self.par_mode && self.own_fn.is_none() && self.vec.is_none()
    }

    fn user_call(&self, e: &Expr) -> bool {
        self.check.user_methods.contains(&(e.span.start, e.span.end))
    }

    /// `n` names a top-level function that a call compiles to, not a local closure or a builtin.
    fn global_fn(&self, n: &str, loc: &[String]) -> bool {
        !loc.iter().any(|l| l == n)
            && self.lookup(n).is_none()
            && self.check.fn_types.contains_key(n)
            && !BUILTIN_CALLS.contains(&n)
            && !self.layouts.newtypes.contains_key(n)
            && self.fn_def(n).is_some_and(|f| f.ext.is_none() && f.kernel.is_none())
    }

    pub(super) fn builtin_reader(&self, n: &str, loc: &[String]) -> bool {
        !loc.iter().any(|l| l == n) && self.lookup(n).is_none() && !self.check.fn_types.contains_key(n) && matches!(n, "log" | "eprint" | "dev_f32" | "dev_f64" | "dev_i32" | "dev_u32" | "dev_int")
    }

    /// The value of `e` is a newly allocated list buffer or string block that nothing else references.
    pub(super) fn rc_fresh(&self, e: &Expr, loc: &mut Vec<String>) -> bool {
        match &e.kind {
            ExprKind::List(_) | ExprKind::Range(..) => true,
            ExprKind::Str(_) => true,
            ExprKind::Method { recv, name, .. } => self.fresh_method(e, recv, name, loc),
            ExprKind::Field(x, f) => {
                let Ok(xt) = self.ty(x) else { return false };
                if matches!(xt, Type::Tuple(_)) || matches!(&xt, Type::Con(n, a) if self.layouts.record_fields(n, a).is_some()) {
                    return false;
                }
                self.fresh_method(e, x, f, loc)
            }
            ExprKind::Call(f, _) => matches!(&f.kind, ExprKind::Name(n) if self.global_fn(n, loc) && self.rc_ret.contains(n)),
            ExprKind::If(_, a, Some(b)) => self.rc_fresh(a, loc) && self.rc_fresh(b, loc),
            ExprKind::Match(_, arms, _) => {
                !arms.is_empty()
                    && arms.iter().all(|a| {
                        let n0 = loc.len();
                        loc.extend(pat_names(&a.pat));
                        let ok = self.rc_fresh(&a.body, loc);
                        loc.truncate(n0);
                        ok
                    })
            }
            ExprKind::Block(stmts) => {
                let Some(Stmt::Expr(last)) = stmts.last() else { return false };
                let n0 = loc.len();
                let ok = match &last.kind {
                    ExprKind::Name(v) => match stmts.iter().rposition(|s| matches!(s, Stmt::Let(Pat::Bind(n), _) | Stmt::Var(n, _) if n == v)) {
                        Some(i) => {
                            for s in &stmts[..i] {
                                bind_names(s, loc);
                            }
                            self.rc_plan_in(stmts, i, loc).is_some_and(|p| p.moved)
                        }
                        None => false,
                    },
                    _ => {
                        for s in &stmts[..stmts.len() - 1] {
                            bind_names(s, loc);
                        }
                        self.rc_fresh(last, loc)
                    }
                };
                loc.truncate(n0);
                ok
            }
            _ => false,
        }
    }

    fn fresh_method(&self, e: &Expr, recv: &Expr, name: &str, loc: &[String]) -> bool {
        if self.user_call(e) {
            return self.global_fn(name, loc) && self.rc_ret.contains(name);
        }
        if name == "map" && matches!(recv.kind, ExprKind::Range(..)) {
            return true;
        }
        let Ok(rt) = self.ty(recv) else { return false };
        match rk_of(&rt) {
            Some(Rk::List | Rk::Strs) => LIST_FRESH.contains(&name) || (name == "join" && elem(&rt, "List").is_some_and(|t| is(&t, "Str"))),
            Some(Rk::Str) => STR_FRESH.contains(&name) || STR_LIST_FRESH.contains(&name),
            None => name == "to_list" && matches!(&rt, Type::Con(n, _) if n == "#DevBuf"),
        }
    }

    /// `e` is a fresh list of strings that are each fresh: a literal of fresh strings, or a map
    /// whose function returns a fresh string.
    pub(super) fn rc_deep_init(&self, e: &Expr, loc: &mut Vec<String>) -> bool {
        if !self.ty(e).ok().and_then(|t| elem(&t, "List")).is_some_and(|t| is(&t, "Str")) {
            return false;
        }
        match &e.kind {
            ExprKind::List(xs) => xs.iter().all(|x| self.rc_fresh(x, loc)),
            ExprKind::Method { recv, name, args, .. } if name == "map" && args.len() == 1 && !self.user_call(e) && (matches!(recv.kind, ExprKind::Range(..)) || self.ty(recv).ok().and_then(|t| elem(&t, "List")).is_some()) => match &args[0].kind {
                ExprKind::Lambda { params, body, .. } => {
                    let n0 = loc.len();
                    loc.extend(params.iter().cloned());
                    let ok = self.rc_fresh(body, loc);
                    loc.truncate(n0);
                    ok
                }
                ExprKind::Name(f) => self.global_fn(f, loc) && self.rc_ret.contains(f),
                _ => false,
            },
            _ => false,
        }
    }

    /// Every use of `v` (of kind `k`) in `e` only reads it. With `var`, assignments to `v` must be
    /// a fresh value or a push onto `v` itself.
    fn rc_reads(&self, e: &Expr, v: &str, k: Rk, var: bool, loc: &mut Vec<String>) -> bool {
        if !touches(e, v) {
            return true;
        }
        let me = |x: &Expr| is_name(x, v);
        match &e.kind {
            ExprKind::Name(n) => n != v,
            ExprKind::Lambda { params, .. } => params.iter().any(|p| p == v),
            ExprKind::Method { recv, name, args, .. } => {
                if self.user_call(e) {
                    return self.global_fn(name, loc) && self.call_reads(name, std::iter::once(&**recv).chain(args.iter()).collect(), v, k, var, loc);
                }
                if me(recv) {
                    let reads = match k {
                        _ if self.rc_deep.get() => DEEP_READS.contains(&name.as_str()),
                        Rk::List | Rk::Strs => LIST_READS.contains(&name.as_str()) || (name == "join" && self.ty(recv).ok().and_then(|t| elem(&t, "List")).is_some_and(|t| is(&t, "Str"))),
                        Rk::Str => STR_READS.contains(&name.as_str()),
                    };
                    return reads && args.iter().all(|a| !assigns(a, v) && !me(a) && self.rc_reads(a, v, k, var, loc));
                }
                if !self.rc_reads(recv, v, k, var, loc) {
                    return false;
                }
                let rt = self.ty(recv).ok();
                let deep = self.rc_deep.get();
                let arg_ok = |i: usize| -> bool {
                    let Some(rt) = &rt else { return false };
                    if deep {
                        return false;
                    }
                    match (rk_of(rt), name.as_str(), i, k) {
                        (Some(Rk::Str), "contains" | "starts_with" | "ends_with" | "split" | "compare_ci", 0, Rk::Str) => true,
                        (Some(Rk::Str), "replace", 0 | 1, Rk::Str) => true,
                        (Some(Rk::List), "join", 0, Rk::Str) => true,
                        (Some(Rk::List), "zip" | "concat", 0, Rk::List) => true,
                        (Some(Rk::List), "contains", 0, _) => true,
                        (None, "get" | "has", 0, _) => matches!(rt, Type::Con(m, _) if m == "Map"),
                        (None, "put", 0, _) => matches!(rt, Type::Con(m, _) if m == "Map") && self.rc_take.get() == Some(key(&args[0])),
                        _ => false,
                    }
                };
                args.iter().enumerate().all(|(i, a)| if me(a) { arg_ok(i) && !assigns(recv, v) && args.iter().all(|b| !assigns(b, v)) } else { self.rc_reads(a, v, k, var, loc) })
            }
            ExprKind::Field(x, f) => {
                if me(x) {
                    return !self.user_call(e)
                        && match k {
                            _ if self.rc_deep.get() => f == "len" || f == "is_empty",
                            Rk::List | Rk::Strs => LIST_READS.contains(&f.as_str()),
                            Rk::Str => STR_READS.contains(&f.as_str()),
                        };
                }
                self.rc_reads(x, v, k, var, loc)
            }
            ExprKind::Index(a, i) => (if me(a) { !self.rc_deep.get() && !assigns(i, v) } else { self.rc_reads(a, v, k, var, loc) }) && self.rc_reads(i, v, k, var, loc),
            ExprKind::Binary(op, a, b) => {
                let cmp = matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge);
                let list_add = *op == BinOp::Add && k == Rk::List && !self.rc_deep.get();
                let mut side = |x: &Expr, other: &Expr, right: bool| if me(x) { (cmp || (list_add && right)) && !assigns(other, v) } else { self.rc_reads(x, v, k, var, loc) };
                side(a, b, false) && side(b, a, true)
            }
            ExprKind::Str(parts) => {
                let exprs: Vec<&Expr> = parts.iter().filter_map(|p| if let StrPart::Expr(x) = p { Some(x) } else { None }).collect();
                exprs.iter().all(|x| if me(x) { exprs.iter().all(|y| !assigns(y, v)) } else { self.rc_reads(x, v, k, var, loc) })
            }
            ExprKind::Call(f, args) => {
                if let ExprKind::Name(n) = &f.kind {
                    if self.global_fn(n, loc) {
                        return self.call_reads(n, args.iter().collect(), v, k, var, loc);
                    }
                    if self.builtin_reader(n, loc) && !self.rc_deep.get() {
                        return args.iter().all(|a| if me(a) { args.iter().all(|b| !assigns(b, v)) } else { self.rc_reads(a, v, k, var, loc) });
                    }
                }
                !touches(f, v) && args.iter().all(|a| !me(a) && self.rc_reads(a, v, k, var, loc))
            }
            ExprKind::Unary(UnOp::Ref | UnOp::RefMut, _) => false,
            ExprKind::Unary(_, x) | ExprKind::Raise(x) | ExprKind::Return(x) => !me(x) && self.rc_reads(x, v, k, var, loc),
            ExprKind::Range(a, b) => self.rc_reads(a, v, k, var, loc) && self.rc_reads(b, v, k, var, loc),
            ExprKind::If(c, a, b) => self.rc_reads(c, v, k, var, loc) && self.rc_reads(a, v, k, var, loc) && b.as_ref().is_none_or(|b| self.rc_reads(b, v, k, var, loc)),
            ExprKind::Match(s, arms, _) | ExprKind::Catch(s, arms) => {
                !me(s)
                    && self.rc_reads(s, v, k, var, loc)
                    && arms.iter().all(|a| {
                        let names = pat_names(&a.pat);
                        if names.iter().any(|n| n == v) {
                            return true;
                        }
                        let n0 = loc.len();
                        loc.extend(names);
                        let ok = a.guard.as_ref().is_none_or(|g| self.rc_reads(g, v, k, var, loc)) && self.rc_reads(&a.body, v, k, var, loc);
                        loc.truncate(n0);
                        ok
                    })
            }
            ExprKind::Block(stmts) => self.rc_reads_stmts(stmts, stmts, v, k, var, loc),
            ExprKind::Record { fields, .. } => fields.iter().all(|(_, x)| !me(x) && self.rc_reads(x, v, k, var, loc)),
            ExprKind::List(xs) | ExprKind::Tuple(xs) => xs.iter().all(|x| !me(x) && self.rc_reads(x, v, k, var, loc)),
            _ => false,
        }
    }

    fn call_reads(&self, n: &str, args: Vec<&Expr>, v: &str, k: Rk, var: bool, loc: &mut Vec<String>) -> bool {
        args.iter().enumerate().all(|(i, a)| {
            if is_name(a, v) {
                !self.rc_deep.get() && self.rc_bor.contains(&(n.to_string(), i)) && args.iter().all(|b| !assigns(b, v))
            } else {
                self.rc_reads(a, v, k, var, loc)
            }
        })
    }

    /// `stmts` is a statement list in which `v` may be used; `scope` is the whole block, for the
    /// names of its local functions.
    fn rc_reads_stmts(&self, stmts: &[Stmt], scope: &[Stmt], v: &str, k: Rk, var: bool, loc: &mut Vec<String>) -> bool {
        let n0 = loc.len();
        for s in scope {
            if let Stmt::Fn(f) = s {
                loc.push(f.name.clone());
            }
        }
        let mut ok = true;
        for s in stmts {
            match s {
                Stmt::Let(p, x) => {
                    ok = self.rc_reads(x, v, k, var, loc);
                    let names = pat_names(p);
                    if !ok || names.iter().any(|n| n == v) {
                        break;
                    }
                    loc.extend(names);
                }
                Stmt::Var(n, x) => {
                    ok = self.rc_reads(x, v, k, var, loc);
                    if !ok || n == v {
                        break;
                    }
                    loc.push(n.clone());
                }
                Stmt::Assign(n, x, _) if n == v => ok = var && self.rc_assign_ok(x, v, k, loc),
                Stmt::Assign(_, x, _) | Stmt::Expr(x) => ok = self.rc_reads(x, v, k, var, loc),
                Stmt::For(p, it, body) => {
                    let names = pat_names(p);
                    ok = (if is_name(it, v) { !self.rc_deep.get() && !assigns(body, v) } else { self.rc_reads(it, v, k, var, loc) }) && {
                        if names.iter().any(|n| n == v) {
                            true
                        } else {
                            let m = loc.len();
                            loc.extend(names);
                            let r = self.rc_reads(body, v, k, var, loc);
                            loc.truncate(m);
                            r
                        }
                    };
                }
                Stmt::While(c, b) => ok = self.rc_reads(c, v, k, var, loc) && self.rc_reads(b, v, k, var, loc),
                Stmt::Fn(f) => ok = f.params.iter().any(|p| p.name == v) || (!touches(&f.body, v) && f.pres.iter().chain(f.posts.iter()).all(|x| !touches(x, v))),
            }
            if !ok {
                break;
            }
        }
        loc.truncate(n0);
        ok
    }

    /// `v := x` keeps `v` uniquely owned: `x` is fresh (and may read the old `v`), or pushes onto `v`.
    fn rc_assign_ok(&self, x: &Expr, v: &str, k: Rk, loc: &mut Vec<String>) -> bool {
        if push_chain(x, v, self) {
            if self.rc_deep.get() {
                return only_pushes(x, v) && push_args(x, v).iter().all(|a| !touches(a, v) && self.ty(a).is_ok_and(|t| is(&t, "Str")) && self.rc_fresh(a, loc));
            }
            return push_args(x, v).iter().all(|a| !touches(a, v));
        }
        if self.rc_deep.get() {
            return false;
        }
        self.rc_fresh(x, loc) && self.rc_reads(x, v, k, false, loc)
    }

    /// The plan for the binding at `stmts[i]`, if its value can be freed at its last use.
    pub(super) fn rc_plan(&self, stmts: &[Stmt], i: usize) -> Option<RcPlan> {
        let mut loc = Vec::new();
        for s in &stmts[..i] {
            bind_names(s, &mut loc);
        }
        self.rc_plan_in(stmts, i, &mut loc)
    }

    fn rc_plan_in(&self, stmts: &[Stmt], i: usize, loc: &mut Vec<String>) -> Option<RcPlan> {
        let (v, init, var) = match &stmts[i] {
            Stmt::Let(Pat::Bind(v), init) => (v, init, false),
            Stmt::Var(v, init) => (v, init, true),
            _ => return None,
        };
        let k = rk_of(&self.ty(init).ok()?)?;
        if var && self.box_names.contains(v) {
            return None;
        }
        let n0 = loc.len();
        for s in stmts {
            if let Stmt::Fn(f) = s {
                loc.push(f.name.clone());
            }
        }
        let saved = (self.rc_deep.replace(false), self.rc_take.replace(None));
        let plan = (|| {
            if !self.rc_fresh(init, loc) {
                return None;
            }
            let rest = &stmts[i + 1..];
            if rest.iter().any(|s| matches!(s, Stmt::Let(p, _) if pat_names(p).iter().any(|n| n == v)) || matches!(s, Stmt::Var(n, _) if n == v)) {
                return None;
            }
            let moved = matches!(rest.last(), Some(Stmt::Expr(x)) if is_name(x, v));
            let check = if moved { &rest[..rest.len() - 1] } else { rest };
            let deep = !moved && self.rc_deep_init(init, loc) && {
                self.rc_deep.set(true);
                let ok = self.rc_reads_stmts(check, stmts, v, k, var, loc);
                self.rc_deep.set(false);
                ok
            };
            let last = rest.iter().rposition(|s| stmt_mentions(s, v)).map_or(i, |j| i + 1 + j);
            if deep {
                return Some(RcPlan { v: v.clone(), k: Rk::Strs, last, moved, var, consumed: None });
            }
            if self.rc_reads_stmts(check, stmts, v, k, var, loc) {
                return Some(RcPlan { v: v.clone(), k, last, moved, var, consumed: None });
            }
            let put = if moved || last == i { None } else { put_key(&stmts[last], v) }?;
            self.rc_take.set(Some(key(put)));
            let ok = self.rc_reads_stmts(check, stmts, v, k, var, loc);
            self.rc_take.set(None);
            ok.then(|| RcPlan { v: v.clone(), k, last, moved, var, consumed: Some(key(put)) })
        })();
        self.rc_deep.set(saved.0);
        self.rc_take.set(saved.1);
        loc.truncate(n0);
        plan
    }

    /// Function summaries: which functions return a fresh block, and which list and string
    /// parameters are only read (so the caller may free a fresh argument after the call).
    pub(super) fn rc_summaries(&mut self) {
        self.rc_ret.clear();
        self.rc_bor.clear();
        if self.rc_off {
            return;
        }
        let fns: Vec<FnDef> = self.all_fns.values().filter(|f| f.ext.is_none() && f.kernel.is_none() && !matches!(f.body.kind, ExprKind::Table(_))).cloned().collect();
        for f in &fns {
            let Some((ps, r)) = self.check.fn_types.get(&f.name) else { continue };
            if rk_of(r).is_some() && !has_return(&f.body) {
                self.rc_ret.insert(f.name.clone());
            }
            for (i, (p, t)) in f.params.iter().zip(ps).enumerate() {
                if rk_of(t).is_some() && !matches!(&p.ty, Ty::Named { name, .. } if name == "&" || name == "&mut") {
                    self.rc_bor.insert((f.name.clone(), i));
                }
            }
        }
        loop {
            let mut changed = false;
            for f in &fns {
                let mut loc: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
                if self.rc_ret.contains(&f.name) && !self.rc_fresh(&f.body, &mut loc) {
                    self.rc_ret.remove(&f.name);
                    changed = true;
                }
                let ps = &self.check.fn_types[&f.name].0;
                for (i, p) in f.params.iter().enumerate() {
                    let key = (f.name.clone(), i);
                    if !self.rc_bor.contains(&key) {
                        continue;
                    }
                    let Some(k) = rk_of(&ps[i]) else { continue };
                    let mut loc: Vec<String> = f.params.iter().filter(|q| q.name != p.name).map(|q| q.name.clone()).collect();
                    let ok = !f.params[..i].iter().any(|q| q.name == p.name)
                        && f.pres.iter().chain(f.posts.iter()).all(|x| self.rc_reads(x, &p.name, k, false, &mut loc))
                        && self.rc_reads(&f.body, &p.name, k, false, &mut loc);
                    if !ok {
                        self.rc_bor.remove(&key);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// A fresh list or string temporary that this expression's consumer may free afterwards.
    pub(super) fn rc_temp(&self, e: &Expr) -> Option<Rk> {
        if !self.rc_on() || self.rc_subst.contains_key(&key(e)) {
            return None;
        }
        if matches!(&e.kind, ExprKind::Str(parts) if matches!(parts.as_slice(), [StrPart::Lit(_)])) {
            return None;
        }
        let k = rk_of(&self.ty(e).ok()?)?;
        self.rc_fresh(e, &mut Vec::new()).then_some(k)
    }

    /// Evaluates `subs` in order into temporaries, generates the consumer with those temporaries
    /// in place of the expressions, then frees the ones with a kind.
    pub(super) fn rc_hoist(&mut self, subs: &[(&Expr, Option<Rk>)], make: impl Fn(&mut Self) -> G) -> G {
        let mut temps = Vec::new();
        for (x, k) in subs {
            let c = self.expr(x)?;
            let tv = self.fresh("rt");
            temps.push((key(x), tv, c, *k));
        }
        for (kx, tv, _, _) in &temps {
            self.rc_subst.insert(*kx, tv.clone());
        }
        let r = make(self);
        for (kx, _, _, _) in &temps {
            self.rc_subst.remove(kx);
        }
        let code = r?;
        if !temps.iter().all(|(_, tv, _, _)| uses_name(&code, tv)) {
            return make(self);
        }
        let (mut decls, mut drops) = (String::new(), String::new());
        for (_, tv, c, k) in &temps {
            write!(decls, "__auto_type {tv} = {c}; ").unwrap();
            if let Some(k) = k {
                drops.push_str(&drop_code(*k, tv));
            }
        }
        let mr = self.fresh("mr");
        Ok(format!("({{ {decls}__auto_type {mr} = {code}; {drops}{mr}; }})"))
    }

    /// For a call of `n` with `args`: the prefix of arguments to evaluate first, marking the fresh
    /// ones passed to parameters `n` only reads.
    pub(super) fn rc_call_temps<'e>(&self, n: &str, args: &[&'e Expr]) -> Option<Vec<(&'e Expr, Option<Rk>)>> {
        let picks: Vec<Option<Rk>> = args.iter().enumerate().map(|(i, a)| if self.rc_bor.contains(&(n.to_string(), i)) { self.rc_temp(a) } else { None }).collect();
        let last = picks.iter().rposition(Option::is_some)?;
        Some(args[..=last].iter().zip(&picks).map(|(a, k)| (*a, *k)).collect())
    }

    pub(super) fn rc_lookup(&self, e: &Expr) -> Option<String> {
        if self.rc_subst.is_empty() {
            return None;
        }
        self.rc_subst.get(&key(e)).cloned()
    }

    /// Code for `var := e` when `var` is an owned local: the old value is freed.
    pub(super) fn rc_assign(&mut self, var: &str, k: Rk, x: &Expr, v: &str, code: &str) -> String {
        let t = self.fresh("ov");
        if push_chain(x, v, self) {
            format!("{{ __auto_type {t} = {var}; {var} = {code}; if ({t}.hdr != {var}.hdr) ss_drop_l({t}); }} ")
        } else {
            format!("{{ __auto_type {t} = {code}; {}{var} = {t}; }} ", drop_code(k, var))
        }
    }
}

/// `x` is `v.push(..)`, `v.concat(..)` or `v + ..` on a list, possibly chained.
fn push_chain(x: &Expr, v: &str, cx: &Cx) -> bool {
    match &x.kind {
        ExprKind::Method { recv, name, args, .. } if (name == "push" || name == "concat") && args.len() == 1 && !cx.user_call(x) => is_name(recv, v) || push_chain(recv, v, cx),
        ExprKind::Binary(BinOp::Add, a, _) if cx.ty(a).ok().and_then(|t| elem(&t, "List")).is_some() => is_name(a, v) || push_chain(a, v, cx),
        _ => false,
    }
}

fn push_args<'e>(x: &'e Expr, v: &str) -> Vec<&'e Expr> {
    match &x.kind {
        ExprKind::Method { recv, args, .. } => {
            let mut out: Vec<&Expr> = args.iter().collect();
            if !is_name(recv, v) {
                out.extend(push_args(recv, v));
            }
            out
        }
        ExprKind::Binary(_, a, b) => {
            let mut out = vec![&**b];
            if !is_name(a, v) {
                out.extend(push_args(a, v));
            }
            out
        }
        _ => vec![],
    }
}

fn bind_names(s: &Stmt, loc: &mut Vec<String>) {
    match s {
        Stmt::Let(p, _) => loc.extend(pat_names(p)),
        Stmt::Var(n, _) => loc.push(n.clone()),
        Stmt::Fn(f) => loc.push(f.name.clone()),
        _ => {}
    }
}

/// `code` contains the identifier `name` (not just a longer identifier starting with it).
fn uses_name(code: &str, name: &str) -> bool {
    code.match_indices(name).any(|(i, _)| !code[i + name.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$'))
}

/// Methods on a list or string receiver that only read it.
pub(super) fn reads_recv(k: Rk, rt: &Type, name: &str) -> bool {
    match k {
        Rk::List => LIST_READS.contains(&name) || (name == "join" && elem(rt, "List").is_some_and(|t| is(&t, "Str"))),
        Rk::Str => STR_READS.contains(&name),
        Rk::Strs => DEEP_READS.contains(&name),
    }
}

/// The single mention of `v` in `s`, when it is the key of a `put` that runs at most once each
/// time `s` does (not inside a loop body, a lambda or a local function).
fn put_key<'e>(s: &'e Stmt, v: &str) -> Option<&'e Expr> {
    let e = match s {
        Stmt::Let(_, x) | Stmt::Var(_, x) | Stmt::Expr(x) => x,
        Stmt::Assign(n, x, _) if n != v => x,
        _ => return None,
    };
    let mut count = 0;
    visit::walk_expr(e, &mut |x| {
        match &x.kind {
            ExprKind::Name(n) if n == v => count += 1,
            ExprKind::Block(stmts) => count += stmts.iter().filter(|s| matches!(s, Stmt::Assign(n, _, _) if n == v)).count(),
            _ => {}
        }
        true
    });
    if count != 1 {
        return None;
    }
    fn find<'e>(e: &'e Expr, v: &str) -> Option<&'e Expr> {
        match &e.kind {
            ExprKind::Method { name, args, .. } if name == "put" && args.len() == 2 && is_name(&args[0], v) => Some(&args[0]),
            ExprKind::Lambda { .. } => None,
            ExprKind::Block(stmts) => stmts.iter().find_map(|s| match s {
                Stmt::Let(_, x) | Stmt::Var(_, x) | Stmt::Assign(_, x, _) | Stmt::Expr(x) => find(x, v),
                Stmt::For(_, it, _) => find(it, v),
                Stmt::While(..) | Stmt::Fn(_) => None,
            }),
            _ => visit::children(e).into_iter().find_map(|c| find(c, v)),
        }
    }
    find(e, v)
}

/// A chain of `push` calls only (no `concat` or `+`, which would copy strings the list doesn't own).
fn only_pushes(x: &Expr, v: &str) -> bool {
    match &x.kind {
        ExprKind::Method { recv, name, .. } if name == "push" => is_name(recv, v) || only_pushes(recv, v),
        _ => false,
    }
}

/// A `var v = []` of strings that is only pushed onto and then joined once with a literal
/// separator: the string is built directly and the list never exists.
struct SbScan<'a> {
    v: &'a str,
    sep: Option<String>,
    joined: bool,
    ok: bool,
}

impl SbScan<'_> {
    fn push_of<'e>(&self, x: &'e Expr, cx: &Cx) -> Option<&'e Expr> {
        match &x.kind {
            ExprKind::Method { recv, name, args, .. } if name == "push" && args.len() == 1 && is_name(recv, self.v) && !cx.user_call(x) && !touches(&args[0], self.v) => Some(&args[0]),
            _ => None,
        }
    }

    fn expr(&mut self, e: &Expr, cx: &Cx, in_loop: bool) {
        if !self.ok || !touches(e, self.v) {
            return;
        }
        match &e.kind {
            ExprKind::Method { recv, name, args, .. } if name == "join" && is_name(recv, self.v) && !cx.user_call(e) => match args.as_slice() {
                [a] if !in_loop && !self.joined => match &a.kind {
                    ExprKind::Str(parts) => match parts.as_slice() {
                        [StrPart::Lit(l)] => {
                            self.sep = Some(l.clone());
                            self.joined = true;
                        }
                        [] => {
                            self.sep = Some(String::new());
                            self.joined = true;
                        }
                        _ => self.ok = false,
                    },
                    _ => self.ok = false,
                },
                _ => self.ok = false,
            },
            ExprKind::Block(stmts) => {
                for s in stmts {
                    self.stmt(s, cx, in_loop);
                }
            }
            ExprKind::Lambda { .. } | ExprKind::Name(_) => self.ok = false,
            ExprKind::Match(s, arms, _) | ExprKind::Catch(s, arms) => {
                self.expr(s, cx, in_loop);
                for a in arms {
                    if pat_names(&a.pat).iter().any(|n| n == self.v) {
                        self.ok = false;
                    }
                    if let Some(g) = &a.guard {
                        self.expr(g, cx, in_loop);
                    }
                    self.expr(&a.body, cx, in_loop);
                }
            }
            _ => {
                for c in visit::children(e) {
                    self.expr(c, cx, in_loop);
                }
            }
        }
    }

    fn stmt(&mut self, s: &Stmt, cx: &Cx, in_loop: bool) {
        match s {
            Stmt::Let(p, x) => {
                self.expr(x, cx, in_loop);
                if pat_names(p).iter().any(|n| n == self.v) {
                    self.ok = false;
                }
            }
            Stmt::Var(n, x) => {
                self.expr(x, cx, in_loop);
                if n == self.v {
                    self.ok = false;
                }
            }
            Stmt::Assign(n, x, _) if n == self.v => match self.push_of(x, cx) {
                Some(a) if !self.joined => self.expr(a, cx, in_loop),
                _ => self.ok = false,
            },
            Stmt::Assign(_, x, _) | Stmt::Expr(x) => self.expr(x, cx, in_loop),
            Stmt::For(p, it, b) => {
                self.expr(it, cx, in_loop);
                if pat_names(p).iter().any(|n| n == self.v) {
                    self.ok = false;
                }
                self.expr(b, cx, true);
            }
            Stmt::While(c, b) => {
                self.expr(c, cx, true);
                self.expr(b, cx, true);
            }
            Stmt::Fn(f) => {
                if touches(&f.body, self.v) || f.params.iter().any(|p| p.name == self.v) {
                    self.ok = false;
                }
            }
        }
    }
}

impl Cx<'_> {
    /// The separator, when `stmts[i]` is `var v = []` of strings that the rest of the block only
    /// pushes onto and then joins once (outside any loop) with a literal separator.
    pub(super) fn sb_plan(&self, stmts: &[Stmt], i: usize) -> Option<String> {
        let Stmt::Var(v, init) = &stmts[i] else { return None };
        if !matches!(&init.kind, ExprKind::List(xs) if xs.is_empty()) || self.box_names.contains(v) || !self.rc_on() {
            return None;
        }
        if !self.ty(init).ok().and_then(|t| elem(&t, "List")).is_some_and(|t| is(&t, "Str")) {
            return None;
        }
        let mut scan = SbScan { v, sep: None, joined: false, ok: true };
        for s in &stmts[i + 1..] {
            scan.stmt(s, self, false);
        }
        (scan.ok && scan.joined).then_some(scan.sep).flatten()
    }
}
