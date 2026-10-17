use sspur_check::{CheckOutput, Type};
use sspur_syntax::visit::{strip_spans, walk_expr};
use sspur_syntax::*;
use std::collections::{BTreeMap, HashMap, HashSet};

struct Op {
    eparams: Vec<String>,
    params: Vec<Ty>,
    ret: Option<Ty>,
}

#[derive(Clone)]
enum Ev {
    Param(String),
    Inline(Vec<Pat>, Expr),
    Host,
}

type Sc = HashMap<String, Ev>;
type Ctor = (String, Option<Vec<String>>);

struct Lower<'a> {
    check: &'a CheckOutput,
    ops: HashMap<String, Op>,
    effects: HashMap<String, Vec<String>>,
    sigs: HashMap<String, Vec<(String, Ty)>>,
    fresh: usize,
    log_only: HashSet<String>,
    log_fns: HashSet<String>,
    reenter: HashSet<String>,
    cur: String,
    types: Vec<(String, String, String)>,
    n_types: usize,
    no_ev: HashSet<String>,
    lam_ok: bool,
    sums: HashMap<String, Vec<Ctor>>,
    in_local: usize,
}

const EAGER: &[&str] = &["map", "flat_map", "filter", "fold", "any", "all", "find", "sort_by", "each", "count", "take_while", "drop_while", "scan", "partition", "group_by", "min_by", "max_by", "find_index", "sum_by", "zip_with"];

fn concrete(t: &Type) -> bool {
    match t {
        Type::Con(_, a) | Type::Tuple(a) => a.iter().all(concrete),
        _ => false,
    }
}

struct Post {
    caps: Vec<String>,
    pat: Pat,
    rest: Vec<Stmt>,
}

fn resume_arg(x: &Expr) -> Expr {
    match &x.kind {
        ExprKind::Call(_, args) => args.first().cloned().unwrap_or_else(|| ex(ExprKind::Unit)),
        _ => ex(ExprKind::Unit),
    }
}

const SUFFIX: &str = "__ev";

pub(super) fn original_name(n: &str) -> &str {
    n.strip_suffix(SUFFIX).unwrap_or(n)
}

fn sp() -> Span {
    Span::default()
}

fn ex(kind: ExprKind) -> Expr {
    Expr::new(kind, sp())
}

fn named(n: &str) -> Ty {
    Ty::Named { name: n.into(), args: vec![], span: sp() }
}

fn subst_ty(t: &Ty, map: &HashMap<&str, &Ty>) -> Ty {
    match t {
        Ty::Named { name, args, .. } if args.is_empty() && map.contains_key(name.as_str()) => map[name.as_str()].clone(),
        Ty::Named { name, args, span } => Ty::Named { name: name.clone(), args: args.iter().map(|a| subst_ty(a, map)).collect(), span: *span },
        Ty::Tuple(xs) => Ty::Tuple(xs.iter().map(|a| subst_ty(a, map)).collect()),
        Ty::Fn { params, ret, effects } => Ty::Fn { params: params.iter().map(|a| subst_ty(a, map)).collect(), ret: Box::new(subst_ty(ret, map)), effects: effects.clone() },
    }
}

fn irrefutable(p: &Pat) -> bool {
    match p {
        Pat::Wild | Pat::Bind(_) => true,
        Pat::Tuple(xs) => xs.iter().all(irrefutable),
        Pat::Or(xs) => xs.iter().any(irrefutable),
        Pat::List { head, rest: Some(_), tail } => head.is_empty() && tail.is_empty(),
        _ => false,
    }
}

fn is_resume(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Call(f, _) if matches!(&f.kind, ExprKind::Name(n) if n == "resume"))
}

fn tail_resumes(e: &Expr) -> bool {
    match &e.kind {
        _ if is_resume(e) => true,
        ExprKind::Block(stmts) => matches!(stmts.last(), Some(Stmt::Expr(x)) if tail_resumes(x)) && !stmts[..stmts.len() - 1].iter().any(|s| matches!(s, Stmt::Expr(x) | Stmt::Let(_, x) if is_resume(x))),
        ExprKind::If(_, t, Some(f)) => tail_resumes(t) && tail_resumes(f),
        ExprKind::Match(_, arms, _) => arms.iter().all(|a| tail_resumes(&a.body)),
        _ => false,
    }
}

fn has_post(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Block(stmts) => stmts.iter().enumerate().any(|(i, s)| match s {
            Stmt::Let(_, x) => is_resume(x),
            Stmt::Expr(x) if i + 1 < stmts.len() => is_resume(x),
            Stmt::Expr(x) => has_post(x),
            _ => false,
        }),
        ExprKind::If(_, t, f) => has_post(t) || f.as_ref().is_some_and(|f| has_post(f)),
        ExprKind::Match(_, arms, _) => arms.iter().any(|a| has_post(&a.body)),
        _ => false,
    }
}

fn strip_resume(e: &Expr) -> Expr {
    if let ExprKind::Call(_, args) = &e.kind
        && is_resume(e) {
            return args.first().cloned().unwrap_or_else(|| ex(ExprKind::Unit));
        }
    let mut e = e.clone();
    match &mut e.kind {
        ExprKind::Block(stmts) => {
            if let Some(Stmt::Expr(x)) = stmts.last_mut() {
                *x = strip_resume(x);
            }
        }
        ExprKind::If(_, t, Some(f)) => {
            **t = strip_resume(t);
            **f = strip_resume(f);
        }
        ExprKind::Match(_, arms, _) => arms.iter_mut().for_each(|a| a.body = strip_resume(&a.body)),
        _ => {}
    }
    e
}

fn pat_binds(p: &Pat, out: &mut HashSet<String>) {
    let mut names = Vec::new();
    p.binds(&mut names);
    out.extend(names);
}

fn binders(e: &Expr) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_expr(e, &mut |x| {
        match &x.kind {
            ExprKind::Lambda { params, .. } => out.extend(params.iter().cloned()),
            ExprKind::Match(_, arms, _) | ExprKind::Catch(_, arms) | ExprKind::Handle(_, arms) => arms.iter().for_each(|a| pat_binds(&a.pat, &mut out)),
            ExprKind::Block(stmts) => {
                for s in stmts {
                    match s {
                        Stmt::Let(p, _) | Stmt::For(p, _, _) => pat_binds(p, &mut out),
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
            _ => {}
        }
        true
    });
    out
}

fn names(e: &Expr) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_expr(e, &mut |x| {
        match &x.kind {
            ExprKind::Name(n) => {
                out.insert(n.clone());
            }
            ExprKind::Block(stmts) => {
                for s in stmts {
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

fn free_in(e: &Expr, bound: &mut Vec<String>, out: &mut HashSet<String>) {
    let mark = bound.len();
    match &e.kind {
        ExprKind::Name(n) => {
            if !bound.contains(n) {
                out.insert(n.clone());
            }
        }
        ExprKind::Lambda { params, body, .. } => {
            bound.extend(params.iter().cloned());
            free_in(body, bound, out);
        }
        ExprKind::Match(x, arms, _) | ExprKind::Catch(x, arms) | ExprKind::Handle(x, arms) => {
            free_in(x, bound, out);
            for a in arms {
                let m = bound.len();
                let mut h = HashSet::new();
                pat_binds(&a.pat, &mut h);
                bound.extend(h);
                if let Some(g) = &a.guard {
                    free_in(g, bound, out);
                }
                free_in(&a.body, bound, out);
                bound.truncate(m);
            }
        }
        ExprKind::Block(stmts) => {
            for st in stmts {
                if let Stmt::Fn(f) = st {
                    bound.push(f.name.clone());
                }
            }
            for st in stmts {
                match st {
                    Stmt::Let(p, x) => {
                        free_in(x, bound, out);
                        let mut h = HashSet::new();
                        pat_binds(p, &mut h);
                        bound.extend(h);
                    }
                    Stmt::Var(n, x) => {
                        free_in(x, bound, out);
                        bound.push(n.clone());
                    }
                    Stmt::Assign(n, x, _) => {
                        if !bound.contains(n) {
                            out.insert(n.clone());
                        }
                        free_in(x, bound, out);
                    }
                    Stmt::Expr(x) => free_in(x, bound, out),
                    Stmt::For(p, it, body) => {
                        free_in(it, bound, out);
                        let m = bound.len();
                        let mut h = HashSet::new();
                        pat_binds(p, &mut h);
                        bound.extend(h);
                        free_in(body, bound, out);
                        bound.truncate(m);
                    }
                    Stmt::While(c, body) => {
                        free_in(c, bound, out);
                        free_in(body, bound, out);
                    }
                    Stmt::Fn(f) => {
                        let m = bound.len();
                        bound.extend(f.params.iter().map(|p| p.name.clone()));
                        free_in(&f.body, bound, out);
                        bound.truncate(m);
                    }
                }
            }
        }
        _ => {
            for c in sspur_syntax::visit::children(e) {
                free_in(c, bound, out);
            }
        }
    }
    bound.truncate(mark);
}

fn arm_free(a: &Arm) -> HashSet<String> {
    let mut bound = Vec::new();
    let mut h = HashSet::new();
    pat_binds(&a.pat, &mut h);
    bound.extend(h);
    let mut out = HashSet::new();
    free_in(&a.body, &mut bound, &mut out);
    out
}

fn has_return(e: &Expr) -> bool {
    let mut found = false;
    walk_expr(e, &mut |x| {
        if matches!(x.kind, ExprKind::Return(_)) {
            found = true;
        }
        !found
    });
    found
}

fn ty_mentions(t: &Ty, user: &HashMap<String, Vec<String>>) -> bool {
    match t {
        Ty::Named { args, .. } => args.iter().any(|a| ty_mentions(a, user)),
        Ty::Tuple(xs) => xs.iter().any(|a| ty_mentions(a, user)),
        Ty::Fn { params, ret, effects } => effects.iter().any(|e| user.contains_key(&e.name)) || params.iter().any(|a| ty_mentions(a, user)) || ty_mentions(ret, user),
    }
}

fn single(stmts: Vec<Stmt>) -> Expr {
    match stmts.as_slice() {
        [Stmt::Expr(_)] => {
            let Some(Stmt::Expr(e)) = stmts.into_iter().next() else { unreachable!() };
            e
        }
        _ => ex(ExprKind::Block(stmts)),
    }
}

impl Lower<'_> {
    fn fresh(&mut self, base: &str) -> String {
        self.fresh += 1;
        format!("{base}__{}", self.fresh)
    }

    fn user_atom(&self, a: &str) -> bool {
        self.effects.contains_key(a.split('[').next().unwrap_or(a))
    }

    fn ev_params(&self, f: &FnDef) -> Vec<(String, Ty)> {
        let mut out = Vec::new();
        for e in f.effects.iter().filter(|e| self.effects.contains_key(&e.name)) {
            for op in &self.effects[&e.name] {
                let info = &self.ops[op];
                let map: HashMap<&str, &Ty> = info.eparams.iter().map(String::as_str).zip(e.args.iter()).collect();
                let params = info.params.iter().map(|t| subst_ty(t, &map)).collect();
                let ret = info.ret.as_ref().map_or_else(|| named("Unit"), |t| subst_ty(t, &map));
                out.push((op.clone(), Ty::Fn { params, ret: Box::new(ret), effects: vec![] }));
            }
        }
        out
    }

    fn local_ok(&self, f: &FnDef, effectful: &HashSet<String>) -> Result<(), String> {
        let mut user = self.effects.clone();
        user.remove("log");
        if f.params.iter().any(|p| ty_mentions(&p.ty, &user)) {
            return Err("takes a function with a declared effect".into());
        }
        let bound = binders(&f.body);
        let taken: HashSet<&String> = self.ops.keys().chain(effectful.iter()).collect();
        if bound.iter().chain(f.params.iter().map(|p| &p.name)).any(|n| taken.contains(n) || n.contains("__")) {
            return Err("shadows an operation".into());
        }
        let mut callees = HashSet::new();
        walk_expr(&f.body, &mut |x| {
            if let ExprKind::Call(g, _) = &x.kind
                && matches!(g.kind, ExprKind::Name(_)) {
                    callees.insert((g.span.start, g.span.end));
                }
            true
        });
        let mut err = None;
        walk_expr(&f.body, &mut |x| {
            let why = match &x.kind {
                ExprKind::Name(n) => ((self.ops.contains_key(n) || (effectful.contains(n) && !self.log_only.contains(n))) && !callees.contains(&(x.span.start, x.span.end))).then_some("passes an effectful function as a value"),
                ExprKind::Handle(_, arms) => self.handle_why(arms),
                ExprKind::Block(stmts) => stmts.iter().find_map(|s| match s {
                    Stmt::Fn(g) if g.effects.iter().any(|e| e.name != "log" && self.effects.contains_key(&e.name)) => Some("has a local function with a declared effect"),
                    _ => None,
                }),
                _ => None,
            };
            if err.is_none() {
                err = why;
            }
            err.is_none()
        });
        err.map_or(Ok(()), |e| Err(e.into()))
    }

    fn raises(&self, body: &Expr) -> bool {
        self.check.clause_effects.get(&(body.span.start, body.span.end)).is_none_or(|fx| fx.iter().any(|a| a.starts_with("fail[")))
    }

    fn handle_why(&self, arms: &[Arm]) -> Option<&'static str> {
        arms.iter().find_map(|a| match &a.pat {
            Pat::Ctor { name, args: CtorArgs::Positional(ps) } if name == "return" => (!ps.iter().all(irrefutable)).then_some("has a refutable return arm"),
            Pat::Ctor { name, args: CtorArgs::Positional(ps) } if self.ops.contains_key(name) && ps.iter().all(irrefutable) => None,
            _ => Some("uses an unsupported handler arm"),
        })
    }

    fn host(&self, sc: &Sc) -> bool {
        matches!(sc.get("log"), None | Some(Ev::Host))
    }

    fn lambda_of(&mut self, pats: &[Pat], body: &Expr) -> Expr {
        let mut params = Vec::new();
        let mut lets = Vec::new();
        for p in pats {
            match p {
                Pat::Bind(n) => params.push(n.clone()),
                _ => {
                    let t = self.fresh("a");
                    lets.push(Stmt::Let(p.clone(), ex(ExprKind::Name(t.clone()))));
                    params.push(t);
                }
            }
        }
        lets.push(Stmt::Expr(body.clone()));
        ex(ExprKind::Lambda { params, body: Box::new(single(lets)), implicit: false, tys: vec![] })
    }

    fn arm_body(&mut self, e: &Expr, ab: &str, k: &str, bound: &[String], post: &mut Option<Post>, used: &mut bool) -> Result<Expr, String> {
        if is_resume(e) {
            return Ok(resume_arg(e));
        }
        let mut e = e.clone();
        match &mut e.kind {
            ExprKind::Block(stmts) => {
                let n = stmts.len();
                let at = stmts.iter().enumerate().position(|(i, s)| match s {
                    Stmt::Let(_, x) => is_resume(x),
                    Stmt::Expr(x) => i + 1 < n && is_resume(x),
                    _ => false,
                });
                let upto = at.unwrap_or(n.saturating_sub(1));
                let mut b = bound.to_vec();
                for st in &stmts[..upto] {
                    match st {
                        Stmt::Let(p, _) => {
                            let mut h = HashSet::new();
                            pat_binds(p, &mut h);
                            b.extend(h);
                        }
                        Stmt::Var(v, _) => b.push(v.clone()),
                        Stmt::Fn(f) => b.push(f.name.clone()),
                        _ => {}
                    }
                }
                if let Some(i) = at {
                    if post.is_some() {
                        return Err("a handler resumes and runs code after it in more than one place".into());
                    }
                    let (pat, v) = match &stmts[i] {
                        Stmt::Let(p, x) => (p.clone(), resume_arg(x)),
                        Stmt::Expr(x) => (Pat::Wild, resume_arg(x)),
                        _ => unreachable!(),
                    };
                    let rest: Vec<Stmt> = stmts[i + 1..].to_vec();
                    stmts.truncate(i);
                    let used_names = names(&ex(ExprKind::Block(rest.clone())));
                    let mut caps: Vec<String> = Vec::new();
                    for x in b.iter().rev() {
                        if used_names.contains(x) && !caps.contains(x) {
                            caps.push(x.clone());
                        }
                    }
                    let tuple = match caps.as_slice() {
                        [] => ex(ExprKind::Unit),
                        [one] => ex(ExprKind::Name(one.clone())),
                        many => ex(ExprKind::Tuple(many.iter().map(|c| ex(ExprKind::Name(c.clone()))).collect())),
                    };
                    let push = ex(ExprKind::Method { recv: Box::new(ex(ExprKind::Name(k.into()))), name: "push".into(), targs: vec![], args: vec![tuple] });
                    stmts.push(Stmt::Assign(k.into(), push, sp()));
                    stmts.push(Stmt::Expr(v));
                    *post = Some(Post { caps, pat, rest });
                } else if let Some(Stmt::Expr(x)) = stmts.last_mut() {
                    *x = self.arm_body(x, ab, k, &b, post, used)?;
                } else {
                    stmts.push(Stmt::Expr(self.abort(ex(ExprKind::Unit), ab, used)));
                }
            }
            ExprKind::If(_, t, f) => {
                **t = self.arm_body(t, ab, k, bound, post, used)?;
                let fe = match f.take() {
                    Some(f) => self.arm_body(&f, ab, k, bound, post, used)?,
                    None => self.abort(ex(ExprKind::Unit), ab, used),
                };
                *f = Some(Box::new(fe));
            }
            ExprKind::Match(_, arms, _) => {
                for a in arms.iter_mut() {
                    let mut h = HashSet::new();
                    pat_binds(&a.pat, &mut h);
                    let mut b = bound.to_vec();
                    b.extend(h);
                    a.body = self.arm_body(&a.body, ab, k, &b, post, used)?;
                }
            }
            _ => return Ok(self.abort(e, ab, used)),
        }
        Ok(e)
    }

    fn abort(&mut self, v: Expr, ab: &str, used: &mut bool) -> Expr {
        *used = true;
        ex(ExprKind::Raise(Box::new(ex(ExprKind::Record { ctor: Some(format!("{ab}v")), fields: vec![("v".into(), v)] }))))
    }

    fn arm_errors(&self, body: &Expr) -> Result<Vec<String>, String> {
        let Some(fx) = self.check.clause_effects.get(&(body.span.start, body.span.end)) else { return Err("a handler arm may raise an unknown error".into()) };
        Ok(fx.iter().filter_map(|a| a.strip_prefix("fail[").and_then(|x| x.strip_suffix(']'))).map(String::from).collect())
    }

    fn wrap_errors(&mut self, mut b: Expr, orig: &Expr, ab: &str, errs: &mut Vec<String>) -> Result<Expr, String> {
        if !self.raises(orig) {
            return Ok(b);
        }
        for et in self.arm_errors(orig)? {
            let i = match errs.iter().position(|x| *x == et) {
                Some(i) => i,
                None => {
                    errs.push(et);
                    errs.len() - 1
                }
            };
            let vs = self.sums.get(&errs[i]).cloned().ok_or("a handler arm raises an error that is not a sum type")?;
            let mut carms = Vec::new();
            for (vn, fs) in vs {
                let (pat, val) = match fs {
                    None => (Pat::Ctor { name: vn.clone(), args: CtorArgs::None }, ex(ExprKind::Name(vn.clone()))),
                    Some(fs) => {
                        let ys: Vec<String> = fs.iter().map(|_| self.fresh("y")).collect();
                        let pat = Pat::Ctor { name: vn.clone(), args: CtorArgs::Record(fs.iter().zip(&ys).map(|(f, y)| (f.clone(), Pat::Bind(y.clone()))).collect()) };
                        (pat, ex(ExprKind::Record { ctor: Some(vn.clone()), fields: fs.iter().zip(&ys).map(|(f, y)| (f.clone(), ex(ExprKind::Name(y.clone())))).collect() }))
                    }
                };
                let wrap = ex(ExprKind::Raise(Box::new(ex(ExprKind::Record { ctor: Some(format!("{ab}e{i}")), fields: vec![("e".into(), val)] }))));
                carms.push(Arm { pat, guard: None, body: wrap });
            }
            b = ex(ExprKind::Catch(Box::new(b), carms));
        }
        Ok(b)
    }

    fn ab_catch(&mut self, ab: &str, v: Option<(String, bool)>, errs: &[String], out: Expr) -> Result<Expr, String> {
        if self.reenter.contains(&self.cur) || self.in_local > 0 {
            return Err("has an aborting handler in a function that may re-enter itself".into());
        }
        let mut vs = Vec::new();
        let mut carms = Vec::new();
        let y = self.fresh("y");
        if let Some((t, ret)) = v {
            vs.push(format!("{ab}v{{v: {t}}}"));
            let body = if ret { ex(ExprKind::Return(Box::new(ex(ExprKind::Name(y.clone()))))) } else { ex(ExprKind::Name(y.clone())) };
            carms.push(Arm { pat: Pat::Ctor { name: format!("{ab}v"), args: CtorArgs::Record(vec![("v".into(), Pat::Bind(y.clone()))]) }, guard: None, body });
        }
        for (i, et) in errs.iter().enumerate() {
            vs.push(format!("{ab}e{i}{{e: {et}}}"));
            carms.push(Arm { pat: Pat::Ctor { name: format!("{ab}e{i}"), args: CtorArgs::Record(vec![("e".into(), Pat::Bind(y.clone()))]) }, guard: None, body: ex(ExprKind::Raise(Box::new(ex(ExprKind::Name(y.clone()))))) });
        }
        self.types.push((ab.to_string(), format!("type {ab} = {}", vs.join(" | ")), self.cur.clone()));
        Ok(ex(ExprKind::Catch(Box::new(out), carms)))
    }

    fn returns_to_raises(&self, e: &mut Expr, ab: &str) -> Result<(), String> {
        let mut nested = false;
        walk_expr(e, &mut |x| {
            match &x.kind {
                ExprKind::Lambda { body, .. } => nested |= has_return(body),
                ExprKind::Block(stmts) => nested |= stmts.iter().any(|s| matches!(s, Stmt::Fn(f) if has_return(&f.body))),
                _ => {}
            }
            true
        });
        if nested {
            return Err("returns from a lambda inside a generator loop".into());
        }
        sspur_syntax::visit::walk_expr_mut(e, &mut |x| {
            if let ExprKind::Return(v) = &x.kind {
                let v = (**v).clone();
                *x = ex(ExprKind::Raise(Box::new(ex(ExprKind::Record { ctor: Some(format!("{ab}v")), fields: vec![("v".into(), v)] }))));
            }
        });
        Ok(())
    }

    fn handle(&mut self, e: &Expr, body: &Expr, arms: &[Arm], sc: &Sc) -> Result<Expr, String> {
        let ab = format!("Ab__{}", self.n_types);
        self.n_types += 1;
        let k = self.fresh("k");
        let mut used = false;
        let mut post = None;
        let mut errs: Vec<String> = Vec::new();
        let mut inner = sc.clone();
        let mut ret = None;
        let mut lowered = Vec::new();
        for a in arms {
            let Pat::Ctor { name, args: CtorArgs::Positional(ps) } = &a.pat else { return Err("bad arm".into()) };
            if name == "return" {
                ret = Some((ps[0].clone(), self.tr(&a.body, sc)?));
                continue;
            }
            let t = self.tr(&a.body, sc)?;
            let mut b = if tail_resumes(&a.body) && !has_post(&a.body) {
                strip_resume(&t)
            } else {
                let mut bound = Vec::new();
                for p in ps {
                    let mut h = HashSet::new();
                    pat_binds(p, &mut h);
                    bound.extend(h);
                }
                self.arm_body(&t, &ab, &k, &bound, &mut post, &mut used)?
            };
            b = self.wrap_errors(b, &a.body, &ab, &mut errs)?;
            lowered.push((name.clone(), ps.clone(), b));
        }
        let arm_names: HashSet<String> = arms.iter().flat_map(arm_free).collect();
        let bound = !binders(body).is_disjoint(&arm_names);
        let mut stmts = Vec::new();
        if post.is_some() {
            stmts.push(Stmt::Var(k.clone(), ex(ExprKind::List(vec![]))));
        }
        for (name, ps, b) in lowered {
            if bound {
                let h = self.fresh("h");
                let lam = self.lambda_of(&ps, &b);
                stmts.push(Stmt::Let(Pat::Bind(h.clone()), lam));
                inner.insert(name, Ev::Param(h));
            } else {
                inner.insert(name, Ev::Inline(ps, b));
            }
        }
        let body = self.tr(body, &inner)?;
        let mut out = match ret {
            Some((p, r)) => single(vec![Stmt::Let(p, body), Stmt::Expr(r)]),
            None => body,
        };
        if used || !errs.is_empty() {
            let t = self.check.expr_types.get(&sspur_check::expr_key(e)).filter(|t| concrete(t)).ok_or("has an aborting handler with a generic result type")?.to_string();
            out = self.ab_catch(&ab, Some((t, false)), &errs, out)?;
        }
        if let Some(Post { caps, pat, rest }) = post {
            let (x, acc, t) = (self.fresh("x"), self.fresh("acc"), self.fresh("t"));
            stmts.push(Stmt::Let(Pat::Bind(x.clone()), out));
            let mut inner = Vec::new();
            match caps.len() {
                0 => {}
                1 => inner.push(Stmt::Let(Pat::Bind(caps[0].clone()), ex(ExprKind::Name(t.clone())))),
                _ => inner.push(Stmt::Let(Pat::Tuple(caps.iter().map(|c| Pat::Bind(c.clone())).collect()), ex(ExprKind::Name(t.clone())))),
            }
            if !matches!(pat, Pat::Wild) {
                inner.push(Stmt::Let(pat, ex(ExprKind::Name(acc.clone()))));
            }
            if rest.is_empty() {
                inner.push(Stmt::Expr(ex(ExprKind::Unit)));
            } else {
                inner.extend(rest);
            }
            let lam = ex(ExprKind::Lambda { params: vec![acc, t], body: Box::new(ex(ExprKind::Block(inner))), implicit: false, tys: vec![] });
            let rev = ex(ExprKind::Field(Box::new(ex(ExprKind::Name(k))), "reverse".into()));
            stmts.push(Stmt::Expr(ex(ExprKind::Method { recv: Box::new(rev), name: "fold".into(), targs: vec![], args: vec![ex(ExprKind::Name(x)), lam] })));
        } else {
            stmts.push(Stmt::Expr(out));
        }
        Ok(single(stmts))
    }

    fn evidence(&mut self, g: &str, sc: &Sc) -> Result<Vec<Expr>, String> {
        let mut out = Vec::new();
        for (op, _) in self.sigs[g].clone() {
            out.push(match sc.get(&op).cloned() {
                Some(Ev::Param(v)) => ex(ExprKind::Name(v)),
                Some(Ev::Inline(pats, body)) => self.lambda_of(&pats, &body),
                Some(Ev::Host) => {
                    let m = self.fresh("m");
                    ex(ExprKind::Lambda { params: vec![m.clone()], body: Box::new(ex(ExprKind::Call(Box::new(ex(ExprKind::Name(op.clone()))), vec![ex(ExprKind::Name(m))]))), implicit: false, tys: vec![] })
                }
                None if op == "log" => {
                    let m = self.fresh("m");
                    ex(ExprKind::Lambda { params: vec![m.clone()], body: Box::new(ex(ExprKind::Call(Box::new(ex(ExprKind::Name(op.clone()))), vec![ex(ExprKind::Name(m))]))), implicit: false, tys: vec![] })
                }
                None => return Err(format!("no handler for '{op}'")),
            });
        }
        Ok(out)
    }

    fn inline(&mut self, pats: &[Pat], args: Vec<Expr>, body: &Expr) -> Expr {
        let mut stmts = Vec::new();
        let mut temps = Vec::new();
        for a in args {
            let t = self.fresh("a");
            stmts.push(Stmt::Let(Pat::Bind(t.clone()), a));
            temps.push(t);
        }
        for (p, t) in pats.iter().zip(temps) {
            stmts.push(Stmt::Let(p.clone(), ex(ExprKind::Name(t))));
        }
        stmts.push(Stmt::Expr(body.clone()));
        single(stmts)
    }

    fn list(&mut self, xs: &[Expr], sc: &Sc) -> Result<Vec<Expr>, String> {
        xs.iter().map(|x| self.tr(x, sc)).collect()
    }

    fn tr(&mut self, e: &Expr, sc: &Sc) -> Result<Expr, String> {
        let user_method = self.check.user_methods.contains(&(e.span.start, e.span.end));
        let kind = match &e.kind {
            ExprKind::Call(f, args) if matches!(&f.kind, ExprKind::Name(n) if self.ops.contains_key(n)) => {
                let ExprKind::Name(n) = &f.kind else { unreachable!() };
                let args = self.list(args, sc)?;
                return match sc.get(n).cloned() {
                    Some(Ev::Param(v)) => Ok(Expr::new(ExprKind::Call(Box::new(ex(ExprKind::Name(v))), args), e.span)),
                    Some(Ev::Inline(pats, body)) => Ok(self.inline(&pats, args, &body)),
                    Some(Ev::Host) => Ok(Expr::new(ExprKind::Call(f.clone(), args), e.span)),
                    None if n == "log" => Ok(Expr::new(ExprKind::Call(f.clone(), args), e.span)),
                    None => Err(format!("no handler for '{n}'")),
                };
            }
            ExprKind::Call(f, args) if matches!(&f.kind, ExprKind::Name(n) if self.sigs.contains_key(n)) => {
                let ExprKind::Name(n) = &f.kind else { unreachable!() };
                let mut args = self.list(args, sc)?;
                if self.log_only.contains(n) && self.host(sc) {
                    ExprKind::Call(f.clone(), args)
                } else {
                    args.extend(self.evidence(n, sc)?);
                    ExprKind::Call(Box::new(Expr::new(ExprKind::Name(format!("{n}{SUFFIX}")), f.span)), args)
                }
            }
            ExprKind::Method { recv, name, targs, args } if user_method && self.sigs.contains_key(name) => {
                let mut args = self.list(args, sc)?;
                if self.log_only.contains(name) && self.host(sc) {
                    ExprKind::Method { recv: Box::new(self.tr(recv, sc)?), name: name.clone(), targs: targs.clone(), args }
                } else {
                    args.extend(self.evidence(name, sc)?);
                    ExprKind::Method { recv: Box::new(self.tr(recv, sc)?), name: format!("{name}{SUFFIX}"), targs: targs.clone(), args }
                }
            }
            ExprKind::Field(x, name) if user_method && self.sigs.contains_key(name) => {
                if self.log_only.contains(name) && self.host(sc) {
                    ExprKind::Field(Box::new(self.tr(x, sc)?), name.clone())
                } else {
                    let args = self.evidence(name, sc)?;
                    ExprKind::Method { recv: Box::new(self.tr(x, sc)?), name: format!("{name}{SUFFIX}"), targs: vec![], args }
                }
            }
            ExprKind::Method { name, .. } | ExprKind::Field(_, name) if user_method && self.log_fns.contains(name) && !self.host(sc) => return Err(format!("calls '{name}' under a log handler, which is not native")),
            ExprKind::Call(f, _) if !self.host(sc) && matches!(&f.kind, ExprKind::Name(n) if self.log_fns.contains(n)) => {
                let ExprKind::Name(n) = &f.kind else { unreachable!() };
                return Err(format!("calls '{n}' under a log handler, which is not native"));
            }
            ExprKind::Name(_) if !self.host(sc) && matches!(self.check.expr_types.get(&sspur_check::expr_key(e)), Some(Type::Fn(_, _, row)) if row.atoms.contains("log")) => {
                return Err("passes a logging function into a log handler".into());
            }
            ExprKind::Handle(body, arms) => return self.handle(e, body, arms, sc),
            ExprKind::Block(stmts) => {
                let mut out = Vec::new();
                for s in stmts {
                    self.stmt(s, sc, &mut out)?;
                }
                return Ok(match out.as_slice() {
                    [Stmt::Expr(_)] => single(out),
                    _ => Expr::new(ExprKind::Block(out), e.span),
                });
            }
            ExprKind::Str(parts) => ExprKind::Str(
                parts
                    .iter()
                    .map(|p| match p {
                        StrPart::Lit(l) => Ok(StrPart::Lit(l.clone())),
                        StrPart::Expr(x) => Ok(StrPart::Expr(self.tr(x, sc)?)),
                    })
                    .collect::<Result<_, String>>()?,
            ),
            ExprKind::Field(x, f) => ExprKind::Field(Box::new(self.tr(x, sc)?), f.clone()),
            ExprKind::Call(f, args) => ExprKind::Call(Box::new(self.tr(f, sc)?), self.list(args, sc)?),
            ExprKind::Method { recv, name, targs, args } => {
                let eager = !user_method && EAGER.contains(&name.as_str()) && matches!(self.check.expr_types.get(&sspur_check::expr_key(recv)), Some(Type::Con(c, _)) if c == "List" || c == "Opt");
                let mut out = Vec::new();
                for a in args {
                    self.lam_ok = eager && matches!(a.kind, ExprKind::Lambda { .. });
                    out.push(self.tr(a, sc)?);
                }
                ExprKind::Method { recv: Box::new(self.tr(recv, sc)?), name: name.clone(), targs: targs.clone(), args: out }
            }
            ExprKind::Index(a, b) => ExprKind::Index(Box::new(self.tr(a, sc)?), Box::new(self.tr(b, sc)?)),
            ExprKind::Lambda { params, body, implicit, .. } => {
                let ok = std::mem::take(&mut self.lam_ok);
                let ty = self.check.expr_types.get(&(e.span.start, e.span.end, 1));
                if !matches!(ty, Some(Type::Fn(..))) {
                    return Err("has a lambda of unknown type".into());
                }
                if !ok && let Some(Type::Fn(_, _, row)) = ty {
                    if row.atoms.iter().any(|a| self.user_atom(a) && !a.starts_with("log")) {
                        return Err("has a lambda that performs a declared effect".into());
                    }
                    if row.atoms.contains("log") && !self.host(sc) {
                        return Err("has a lambda that logs under a log handler".into());
                    }
                }
                ExprKind::Lambda { params: params.clone(), body: Box::new(self.tr(body, sc)?), implicit: *implicit, tys: vec![] }
            }
            ExprKind::Binary(op, a, b) => ExprKind::Binary(*op, Box::new(self.tr(a, sc)?), Box::new(self.tr(b, sc)?)),
            ExprKind::Unary(op, a) => ExprKind::Unary(*op, Box::new(self.tr(a, sc)?)),
            ExprKind::Range(a, b) => ExprKind::Range(Box::new(self.tr(a, sc)?), Box::new(self.tr(b, sc)?)),
            ExprKind::If(c, t, f) => ExprKind::If(Box::new(self.tr(c, sc)?), Box::new(self.tr(t, sc)?), match f {
                Some(f) => Some(Box::new(self.tr(f, sc)?)),
                None => None,
            }),
            ExprKind::Match(s, arms, _) | ExprKind::Catch(s, arms) => {
                let s2 = Box::new(self.tr(s, sc)?);
                let mut out = Vec::new();
                for a in arms {
                    let guard = match &a.guard {
                        Some(g) => Some(self.tr(g, sc)?),
                        None => None,
                    };
                    out.push(Arm { pat: a.pat.clone(), guard, body: self.tr(&a.body, sc)? });
                }
                if matches!(e.kind, ExprKind::Match(..)) { ExprKind::Match(s2, out, MatchForm::Arms) } else { ExprKind::Catch(s2, out) }
            }
            ExprKind::Record { ctor, fields } => ExprKind::Record { ctor: ctor.clone(), fields: fields.iter().map(|(n, x)| Ok((n.clone(), self.tr(x, sc)?))).collect::<Result<_, String>>()? },
            ExprKind::List(xs) => ExprKind::List(self.list(xs, sc)?),
            ExprKind::Tuple(xs) => ExprKind::Tuple(self.list(xs, sc)?),
            ExprKind::Par(xs) => ExprKind::Par(self.list(xs, sc)?),
            ExprKind::Raise(x) => ExprKind::Raise(Box::new(self.tr(x, sc)?)),
            ExprKind::Return(x) => ExprKind::Return(Box::new(self.tr(x, sc)?)),
            ExprKind::With(base, ups) => {
                let mut out = Vec::new();
                for (path, v) in ups {
                    let mut p2 = Vec::new();
                    for seg in path {
                        p2.push(match seg {
                            PathSeg::Field(f) => PathSeg::Field(f.clone()),
                            PathSeg::Index(i) => PathSeg::Index(self.tr(i, sc)?),
                        });
                    }
                    out.push((p2, self.tr(v, sc)?));
                }
                ExprKind::With(Box::new(self.tr(base, sc)?), out)
            }
            ExprKind::Table(rows) => {
                let mut out = Vec::new();
                for r in rows {
                    let mut cells = Vec::new();
                    for c in &r.cells {
                        cells.push(match c {
                            Cell::Cond(x) => Cell::Cond(self.tr(x, sc)?),
                            c => c.clone(),
                        });
                    }
                    out.push(TableRow { cells, out: self.tr(&r.out, sc)? });
                }
                ExprKind::Table(out)
            }
            k => k.clone(),
        };
        Ok(Expr::new(kind, e.span))
    }

    fn stmt(&mut self, s: &Stmt, sc: &Sc, out: &mut Vec<Stmt>) -> Result<(), String> {
        let st = match s {
            Stmt::For(p, it, orig) if self.check.gen_loops.contains(&(it.span.start, it.span.end)) => {
                let ab = format!("Ab__{}", self.n_types);
                self.n_types += 1;
                let mut body = self.tr(orig, sc)?;
                let ret = has_return(&body);
                if ret {
                    self.returns_to_raises(&mut body, &ab)?;
                }
                let mut errs = Vec::new();
                let body = self.wrap_errors(body, orig, &ab, &mut errs)?;
                let ev = if irrefutable(p) {
                    Ev::Inline(vec![p.clone()], body)
                } else {
                    let t = self.fresh("x");
                    let arms = vec![Arm { pat: p.clone(), guard: None, body }, Arm { pat: Pat::Wild, guard: None, body: ex(ExprKind::Unit) }];
                    Ev::Inline(vec![Pat::Bind(t.clone())], ex(ExprKind::Match(Box::new(ex(ExprKind::Name(t))), arms, MatchForm::Arms)))
                };
                let ev = match ev {
                    Ev::Inline(ps, b) if !binders(it).is_disjoint(&arm_free(&Arm { pat: p.clone(), guard: None, body: orig.clone() })) => {
                        let h = self.fresh("h");
                        let lam = self.lambda_of(&ps, &b);
                        out.push(Stmt::Let(Pat::Bind(h.clone()), lam));
                        Ev::Param(h)
                    }
                    ev => ev,
                };
                let mut inner = sc.clone();
                inner.insert("yield".into(), ev);
                let mut lp = self.tr(it, &inner)?;
                if ret || !errs.is_empty() {
                    let v = if ret {
                        let t = self.check.fn_types.get(&self.cur).map(|(_, r)| r.clone()).filter(concrete).ok_or("returns from a generator loop in a generic function")?;
                        Some((t.to_string(), true))
                    } else {
                        None
                    };
                    lp = self.ab_catch(&ab, v, &errs, single(vec![Stmt::Expr(lp), Stmt::Expr(ex(ExprKind::Unit))]))?;
                }
                Stmt::Expr(lp)
            }
            Stmt::For(p, it, body) => Stmt::For(p.clone(), self.tr(it, sc)?, self.tr(body, sc)?),
            Stmt::Let(p, x) => Stmt::Let(p.clone(), self.tr(x, sc)?),
            Stmt::Var(n, x) => Stmt::Var(n.clone(), self.tr(x, sc)?),
            Stmt::Assign(n, x, span) => Stmt::Assign(n.clone(), self.tr(x, sc)?, *span),
            Stmt::Expr(x) => Stmt::Expr(self.tr(x, sc)?),
            Stmt::While(c, body) => Stmt::While(self.tr(c, sc)?, self.tr(body, sc)?),
            Stmt::Fn(f) => {
                let mut g = (**f).clone();
                self.in_local += 1;
                let b = self.tr(&f.body, sc);
                self.in_local -= 1;
                g.body = b?;
                Stmt::Fn(Box::new(g))
            }
        };
        out.push(st);
        Ok(())
    }

    fn lower_fn(&mut self, f: &FnDef) -> Result<Vec<FnDef>, String> {
        self.cur = f.name.clone();
        let mut base = Sc::new();
        if self.effects.contains_key("log") {
            base.insert("log".into(), Ev::Host);
        }
        let evs = self.ev_params(f);
        if evs.is_empty() {
            let mut g = f.clone();
            g.body = self.tr(&f.body, &base)?;
            return Ok(vec![g]);
        }
        let orig = if self.log_only.contains(&f.name) {
            let mut o = f.clone();
            o.body = self.tr(&f.body, &base)?;
            o
        } else {
            f.clone()
        };
        if self.no_ev.contains(&f.name) {
            return Ok(vec![orig]);
        }
        let mut g = f.clone();
        let mut sc = Sc::new();
        g.effects.retain(|e| !self.effects.contains_key(&e.name));
        g.examples.clear();
        for (i, (op, ty)) in evs.into_iter().enumerate() {
            let row = format!("e{i}__");
            let Ty::Fn { params, ret, .. } = ty else { unreachable!() };
            let pname = format!("ev__{op}");
            g.params.push(Param { name: pname.clone(), ty: Ty::Fn { params, ret, effects: vec![Effect { name: row.clone(), args: vec![], span: sp() }] }, refine: None });
            g.tparams.push(TParam { name: row.clone(), kind: None, refine: None, bounds: vec![] });
            g.effects.push(Effect { name: row, args: vec![], span: sp() });
            sc.insert(op, Ev::Param(pname));
        }
        g.name = format!("{}{SUFFIX}", f.name);
        match self.tr(&f.body, &sc) {
            Ok(b) => {
                g.body = b;
                Ok(vec![orig, g])
            }
            Err(_) if self.log_only.contains(&f.name) => {
                self.no_ev.insert(f.name.clone());
                Ok(vec![orig])
            }
            Err(e) => Err(e),
        }
    }
}

pub(super) type Lowered = (Module, CheckOutput, BTreeMap<String, String>);

fn give_up(why: &str) -> Option<Lowered> {
    if std::env::var_os("SSPUR_LOWER_DEBUG").is_some() {
        eprintln!("lower: giving up: {why}");
    }
    None
}

fn ty_has_fn(t: &Ty) -> bool {
    match t {
        Ty::Fn { .. } => true,
        Ty::Named { args, .. } | Ty::Tuple(args) => args.iter().any(ty_has_fn),
    }
}

fn reentrant(fns: &[&FnDef], check: &CheckOutput) -> HashSet<String> {
    let names: HashSet<&str> = fns.iter().map(|f| f.name.as_str()).collect();
    let mut edges: HashMap<&str, HashSet<String>> = HashMap::new();
    let mut open: HashSet<&str> = HashSet::new();
    for f in fns {
        let mut out = HashSet::new();
        walk_expr(&f.body, &mut |x| {
            match &x.kind {
                ExprKind::Name(n) if names.contains(n.as_str()) => {
                    out.insert(n.clone());
                }
                ExprKind::Method { name, .. } | ExprKind::Field(_, name) if check.user_methods.contains(&(x.span.start, x.span.end)) => {
                    out.insert(name.clone());
                }
                _ => {}
            }
            true
        });
        if f.params.iter().any(|p| ty_has_fn(&p.ty)) {
            open.insert(f.name.as_str());
        }
        edges.insert(f.name.as_str(), out);
    }
    let mut out = HashSet::new();
    for f in fns {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut work: Vec<&str> = edges[f.name.as_str()].iter().map(String::as_str).collect();
        let mut hit = open.contains(f.name.as_str());
        while let Some(n) = work.pop() {
            if n == f.name {
                hit = true;
                break;
            }
            if seen.insert(n)
                && let Some(es) = edges.get(n)
            {
                work.extend(es.iter().map(String::as_str));
            }
        }
        if hit {
            out.insert(f.name.clone());
        }
    }
    out
}

pub(super) fn lower(m: &Module, check: &CheckOutput) -> Option<Lowered> {
    let mut ops = HashMap::new();
    let mut effects: HashMap<String, Vec<String>> = HashMap::new();
    for d in &m.defs {
        if let Def::Effect(e) = d {
            for op in &e.ops {
                ops.insert(op.name.clone(), Op { eparams: e.params.iter().map(|p| p.name.clone()).collect(), params: op.params.iter().map(|p| p.ty.clone()).collect(), ret: op.ret.clone() });
            }
            effects.insert(e.name.clone(), e.ops.iter().map(|o| o.name.clone()).collect());
        }
    }
    let fns: Vec<&FnDef> = m.defs.iter().filter_map(|d| if let Def::Fn(f) = d { Some(f) } else { None }).collect();
    let mut log_mode = false;
    let mut log_fns: HashSet<String> = HashSet::new();
    for f in &fns {
        if f.effects.iter().any(|e| e.name == "log") {
            log_fns.insert(f.name.clone());
        }
        walk_expr(&f.body, &mut |x| {
            match &x.kind {
                ExprKind::Handle(_, arms) => log_mode |= arms.iter().any(|a| matches!(&a.pat, Pat::Ctor { name, .. } if name == "log")),
                ExprKind::Block(stmts) => {
                    for s in stmts {
                        if let Stmt::Fn(g) = s
                            && g.effects.iter().any(|e| e.name == "log")
                        {
                            log_fns.insert(g.name.clone());
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }
    if effects.is_empty() && check.gen_loops.is_empty() && !log_mode {
        return None;
    }
    ops.insert("yield".into(), Op { eparams: vec!["T".into()], params: vec![named("T")], ret: None });
    effects.insert("yield".into(), vec!["yield".into()]);
    if log_mode {
        ops.insert("log".into(), Op { eparams: vec![], params: vec![named("Str")], ret: None });
        effects.insert("log".into(), vec!["log".into()]);
    } else {
        log_fns.clear();
    }
    let mut lw = Lower { check, ops, effects, sigs: HashMap::new(), fresh: 0, log_only: HashSet::new(), log_fns, reenter: HashSet::new(), cur: String::new(), types: vec![], n_types: 0, no_ev: HashSet::new(), lam_ok: false, sums: HashMap::new(), in_local: 0 };
    for d in &m.defs {
        if let Def::Type(TypeDef { name, params, body: TypeBody::Sum(vs), .. }) = d
            && params.is_empty()
        {
            lw.sums.insert(name.clone(), vs.iter().map(|v| (v.name.clone(), v.fields.as_ref().map(|fs| fs.iter().map(|f| f.name.clone()).collect()))).collect());
        }
    }
    let effectful: HashSet<String> = fns.iter().filter(|f| !lw.ev_params(f).is_empty()).map(|f| f.name.clone()).collect();
    lw.log_only = fns.iter().filter(|f| lw.ev_params(f).iter().all(|(op, _)| op == "log") && effectful.contains(&f.name)).map(|f| f.name.clone()).collect();
    lw.reenter = reentrant(&fns, check);
    if fns.iter().any(|f| f.name.ends_with(SUFFIX)) {
        return None;
    }
    let mut why: BTreeMap<String, String> = BTreeMap::new();
    let mut ok: HashSet<String> = HashSet::new();
    for f in &fns {
        match lw.local_ok(f, &effectful) {
            Ok(()) => {
                ok.insert(f.name.clone());
            }
            Err(e) => {
                why.insert(f.name.clone(), e);
            }
        }
    }
    for _ in 0..24 {
        loop {
            let before = ok.len();
            for f in &fns {
                let mut calls_bad = None;
                walk_expr(&f.body, &mut |x| {
                    let callee = match &x.kind {
                        ExprKind::Call(g, _) => match &g.kind {
                            ExprKind::Name(n) => Some(n),
                            _ => None,
                        },
                        ExprKind::Method { name, .. } | ExprKind::Field(_, name) if check.user_methods.contains(&(x.span.start, x.span.end)) => Some(name),
                        _ => None,
                    };
                    if let Some(n) = callee.filter(|n| effectful.contains(*n) && !ok.contains(*n) && !lw.log_only.contains(*n)) {
                        calls_bad.get_or_insert_with(|| n.clone());
                    }
                    true
                });
                if let Some(n) = calls_bad
                    && ok.remove(&f.name) {
                        why.insert(f.name.clone(), format!("calls '{n}', which is not native"));
                    }
            }
            if ok.len() == before {
                break;
            }
        }
        lw.sigs = fns.iter().filter(|f| ok.contains(&f.name) && effectful.contains(&f.name) && !lw.no_ev.contains(&f.name)).map(|f| (f.name.clone(), lw.ev_params(f))).collect();
        let no_ev = lw.no_ev.len();
        lw.types.clear();
        lw.n_types = 0;
        let mut defs = Vec::new();
        let mut origin: BTreeMap<String, String> = BTreeMap::new();
        let mut failed = None;
        for d in &m.defs {
            match d {
                Def::Fn(f) if ok.contains(&f.name) => {
                    lw.fresh = 0;
                    match lw.lower_fn(f) {
                        Ok(gs) => {
                            for g in gs {
                                origin.insert(g.name.clone(), f.name.clone());
                                defs.push(Def::Fn(g));
                            }
                        }
                        Err(e) => {
                            why.insert(f.name.clone(), e);
                            failed = Some(f.name.clone());
                            break;
                        }
                    }
                }
                Def::Test(_) => {}
                d => defs.push(d.clone()),
            }
        }
        if let Some(n) = failed {
            ok.remove(&n);
            continue;
        }
        if lw.no_ev.len() != no_ev {
            continue;
        }
        let mut extra = Vec::new();
        for (name, text, owner) in &lw.types {
            let Ok(tm) = parse(text) else { return give_up("abort type") };
            origin.insert(name.clone(), owner.clone());
            extra.extend(tm.defs);
        }
        extra.extend(defs);
        let defs = extra;
        let lowered = Module { profile: m.profile.clone(), defs, own: m.own };
        let text = printer::print_module(&lowered);
        let Ok(reparsed) = parse(&text) else { return give_up(&text) };
        let (mut a, mut b) = (lowered.clone(), reparsed.clone());
        strip_spans(&mut a);
        strip_spans(&mut b);
        if a.defs.len() != b.defs.len() {
            return give_up("def count");
        }
        let differ: Vec<&Def> = a.defs.iter().zip(&b.defs).filter(|(x, y)| x != y).map(|(x, _)| x).collect();
        if let Some(d) = differ.iter().find(|d| !origin.contains_key(d.name())) {
            return give_up(d.name());
        }
        let bad: Vec<String> = differ.iter().filter_map(|d| origin.get(d.name()).cloned()).collect();
        if !bad.is_empty() {
            for n in bad {
                ok.remove(&n);
            }
            continue;
        }
        let c2 = sspur_check::check(&reparsed);
        let bad_ev: Vec<String> = c2.diags.iter().filter(|d| d.is_error()).filter_map(|d| d.def.as_deref()).filter(|n| n.ends_with(SUFFIX) && lw.log_only.contains(original_name(n))).map(|n| original_name(n).to_string()).collect();
        let errs: Vec<String> = c2.diags.iter().filter(|d| d.is_error() && !d.def.as_deref().is_some_and(|n| n.ends_with(SUFFIX) && lw.log_only.contains(original_name(n)))).filter_map(|d| d.def.as_ref().and_then(|n| origin.get(n)).cloned()).collect();
        if c2.has_errors() && errs.is_empty() && bad_ev.is_empty() {
            return give_up(&format!("{:?}", c2.diags.iter().find(|d| d.is_error())));
        }
        lw.no_ev.extend(bad_ev);
        if c2.has_errors() {
            if std::env::var_os("SSPUR_LOWER_DEBUG").is_some() {
                eprintln!("{text}");
                for d in c2.diags.iter().filter(|d| d.is_error()) {
                    eprintln!("lower: {:?}: {}", d.def, d.msg);
                }
            }
            for d in c2.diags.iter().filter(|d| d.is_error()) {
                if let Some(o) = d.def.as_ref().and_then(|n| origin.get(n)) {
                    why.entry(o.clone()).or_insert_with(|| format!("does not check after lowering ({})", d.msg.chars().take(80).collect::<String>()));
                }
            }
            for n in errs {
                ok.remove(&n);
            }
            continue;
        }
        why.retain(|n, _| !ok.contains(n));
        return Some((reparsed, c2, why));
    }
    give_up("too many rounds")
}
