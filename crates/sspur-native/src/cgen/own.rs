use super::*;

pub(super) type Spans = HashSet<(u32, u32)>;

fn key(e: &Expr) -> (u32, u32) {
    (e.span.start, e.span.end)
}

fn contains_ty(t: &Type, target: &Type) -> bool {
    t == target
        || match t {
            Type::Con(_, a) => a.iter().any(|x| contains_ty(x, target)),
            Type::Tuple(xs) => xs.iter().any(|x| contains_ty(x, target)),
            Type::Fn(ps, r, _) => ps.iter().any(|x| contains_ty(x, target)) || contains_ty(r, target),
            _ => false,
        }
}

fn count(e: &Expr, n: &str) -> usize {
    let mut c = 0;
    visit::walk_expr(e, &mut |x| {
        if matches!(&x.kind, ExprKind::Name(m) if m == n) {
            c += 1;
        }
        true
    });
    c
}

pub(super) fn has_catch(e: &Expr) -> bool {
    let mut found = false;
    visit::walk_expr(e, &mut |x| {
        if matches!(x.kind, ExprKind::Catch(..)) {
            found = true;
        }
        !found
    });
    found
}

struct Plan<'p> {
    fname: &'p str,
    p0: &'p str,
    ptrs: HashSet<String>,
    ctor: String,
    spans: Spans,
}

impl Plan<'_> {
    fn free(&self, e: &Expr) -> bool {
        count(e, self.p0) == 0 && self.ptrs.iter().all(|p| count(e, p) == 0)
    }

    fn uses_ok(&self, e: &Expr) -> bool {
        count(e, self.p0) == 0 && self.ptrs.iter().all(|p| count(e, p) <= 1)
    }

    fn owned_arg(&mut self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Name(n) if self.ptrs.contains(n) => true,
            ExprKind::Call(f, args) if is_name(f, self.fname) => self.self_call(args),
            ExprKind::Record { ctor: Some(_), fields } => fields.iter().all(|(_, x)| self.owned_arg(x)),
            _ => self.free(e),
        }
    }

    fn self_call(&mut self, args: &[Expr]) -> bool {
        let Some((a0, rest)) = args.split_first() else { return false };
        let first = match &a0.kind {
            ExprKind::Name(n) => self.ptrs.contains(n) || self.free(a0),
            _ => self.owned_arg(a0),
        };
        first && rest.iter().all(|a| self.free(a))
    }

    fn tail(&mut self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Name(n) if n == self.p0 || self.ptrs.contains(n) => true,
            ExprKind::If(c, a, Some(b)) => self.free(c) && self.tail(a) && self.tail(b),
            ExprKind::Call(f, args) if is_name(f, self.fname) => self.uses_ok(e) && self.self_call(args),
            ExprKind::Record { ctor: Some(c), fields } => {
                if !self.uses_ok(e) || !fields.iter().all(|(_, x)| self.owned_arg(x)) {
                    return false;
                }
                if *c == self.ctor {
                    self.spans.insert(key(e));
                }
                true
            }
            _ => self.free(e),
        }
    }
}

impl Cx<'_> {
    pub(super) fn own_plan(&self, name: &str) -> Option<Spans> {
        if self.generics.contains_key(name) || !self.eligible.contains(name) {
            return None;
        }
        let f = self.fn_def(name)?;
        let (pts, rt) = self.check.fn_types.get(name)?;
        if !f.posts.is_empty() || f.params.is_empty() || pts.first()? != rt || has_catch(&f.body) {
            return None;
        }
        let Type::Con(sum, targs) = rt else { return None };
        let vs = self.layouts.sum_variants(sum, targs)?;
        if pts[1..].iter().any(|t| contains_ty(t, rt)) {
            return None;
        }
        let p0 = f.params[0].name.as_str();
        let ExprKind::Match(s, arms, _) = &f.body.kind else { return None };
        if !is_name(s, p0) {
            return None;
        }
        let mut spans = Spans::new();
        for arm in arms {
            let mut ptrs = HashSet::new();
            let ctor = match &arm.pat {
                Pat::Wild => String::new(),
                Pat::Ctor { name: c, args } => {
                    let fields = vs.iter().find(|(v, _)| v == c)?.1.clone().unwrap_or_default();
                    let binds: Vec<(String, &Pat)> = match args {
                        CtorArgs::None => Vec::new(),
                        CtorArgs::Record(fs) => fs.iter().map(|(n, p)| (n.clone(), p)).collect(),
                        CtorArgs::Positional(ps) => fields.iter().zip(ps).map(|((n, _), p)| (n.clone(), p)).collect(),
                    };
                    for (fname, p) in binds {
                        let ft = &fields.iter().find(|(n, _)| *n == fname)?.1;
                        match p {
                            Pat::Wild => {}
                            Pat::Bind(b) if ft == rt => {
                                ptrs.insert(b.clone());
                            }
                            Pat::Bind(_) if !contains_ty(ft, rt) => {}
                            _ => return None,
                        }
                    }
                    c.clone()
                }
                _ => return None,
            };
            if ptrs.contains(p0) {
                return None;
            }
            let mut plan = Plan { fname: name, p0, ptrs, ctor, spans: Spans::new() };
            if arm.guard.as_ref().is_some_and(|g| !plan.free(g)) || !plan.tail(&arm.body) {
                return None;
            }
            spans.extend(plan.spans);
        }
        Some(spans)
    }

    fn own_stmt(&self, s: &Stmt, v: &str) -> Option<bool> {
        match s {
            Stmt::Assign(n, x, _) if n == v => self.own_assign(v, x).then_some(true),
            Stmt::Let(p, x) => (!pat_binds(p, v) && !mentions(x, v)).then_some(false),
            Stmt::Var(n, x) => (n != v && !mentions(x, v)).then_some(false),
            Stmt::Assign(_, x, _) => (!mentions(x, v)).then_some(false),
            Stmt::Expr(x) => self.own_expr(x, v),
            Stmt::While(c, b) => {
                if mentions(c, v) {
                    return None;
                }
                self.own_expr(b, v)
            }
            Stmt::For(p, it, b) => {
                if pat_binds(p, v) || mentions(it, v) {
                    return None;
                }
                self.own_expr(b, v)
            }
            Stmt::Fn(f) => (!mentions(&f.body, v)).then_some(false),
        }
    }

    fn own_expr(&self, e: &Expr, v: &str) -> Option<bool> {
        if !mentions(e, v) {
            return Some(false);
        }
        match &e.kind {
            ExprKind::Block(stmts) => {
                let mut any = false;
                for s in stmts {
                    any |= self.own_stmt(s, v)?;
                }
                Some(any)
            }
            ExprKind::If(c, a, b) if !mentions(c, v) => {
                let x = self.own_expr(a, v)?;
                let y = match b {
                    Some(b) => self.own_expr(b, v)?,
                    None => false,
                };
                Some(x || y)
            }
            _ => None,
        }
    }

    pub(super) fn own_assign(&self, v: &str, x: &Expr) -> bool {
        match &x.kind {
            ExprKind::Call(f, args) => {
                let ExprKind::Name(fname) = &f.kind else { return false };
                self.lookup(fname).is_none()
                    && args.first().is_some_and(|a| is_name(a, v))
                    && args[1..].iter().all(|a| !mentions(a, v))
                    && self.own_plan(fname).is_some()
            }
            ExprKind::Method { recv, name, args, .. } if name == "map" && is_name(recv, v) && args.len() == 1 => {
                !mentions(&args[0], v)
                    && matches!(&args[0].kind, ExprKind::Lambda { .. } | ExprKind::Name(_))
                    && self.ty(recv).ok().zip(self.ty(x).ok()).is_some_and(|(a, b)| a == b)
            }
            _ => false,
        }
    }

    pub(super) fn owned_var(&self, init: &Expr, rest: &[Stmt], v: &str) -> bool {
        let fresh = match &init.kind {
            ExprKind::Name(n) => self.lookup(n).is_none() && !self.check.fn_types.contains_key(n),
            ExprKind::Range(..) | ExprKind::List(_) => true,
            ExprKind::Method { name, .. } => name == "map",
            _ => false,
        };
        if !fresh {
            return false;
        }
        let mut last = None;
        let mut flags = Vec::new();
        for (i, s) in rest.iter().enumerate() {
            match self.own_stmt(s, v) {
                Some(true) => {
                    last = Some(i);
                    flags.push(true);
                }
                Some(false) => flags.push(true),
                None => flags.push(false),
            }
        }
        let Some(last) = last else { return false };
        flags[..=last].iter().all(|ok| *ok)
    }
}

impl Cx<'_> {
    pub(super) fn own_update(&mut self, var: &str, e: &Expr) -> G {
        match &e.kind {
            ExprKind::Call(f, args) => {
                let ExprKind::Name(fname) = &f.kind else { unreachable!() };
                let mut vals = Vec::new();
                for a in args {
                    vals.push((self.expr(a)?, self.ty(a)?));
                }
                let t = self.ty(e)?;
                self.call_suffix = Some("__own".into());
                let call = self.call_user(fname, vals, Some(&t), Some((e, args)));
                self.call_suffix = None;
                Ok(format!("{var} = {}; ", call?))
            }
            ExprKind::Method { recv, args, .. } => {
                let lt = self.ty(recv)?;
                let et = elem(&lt, "List").ok_or("in-place map over a non-list")?;
                let (l, i, x) = (self.fresh("l"), self.fresh("i"), self.fresh("x"));
                let body = self.apply(&args[0], vec![(x.clone(), et)])?;
                Ok(format!("{{ __auto_type {l} = {var}; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {x} = {l}.data[{i}]; {l}.data[{i}] = {body}; }} }} "))
            }
            _ => unreachable!(),
        }
    }

    pub(super) fn range_map(&mut self, a: &Expr, b: &Expr, f: &Expr, t: &Type) -> G {
        let ot = elem(t, "List").ok_or("bad map type")?;
        let oc = self.cty(&ot)?;
        let out = self.cty(t)?;
        let (av, bv) = (self.expr(a)?, self.expr(b)?);
        let (i, x) = (self.fresh("i"), self.fresh("x"));
        let m = self.know.facts.len();
        self.for_range_facts(&x, a, b);
        let body = self.apply(f, vec![(x.clone(), Type::int())]);
        self.know.facts.truncate(m);
        let body = body?;
        Ok(format!("({{ int64_t s_ = {av}; int64_t re_ = {bv}; int64_t n_ = re_ > s_ ? re_ - s_ : 0; RawL r_ = raw_alloc(n_, sizeof({oc})); {oc}* d_ = ({oc}*)r_.data; for (int64_t {i} = 0; {i} < n_; {i}++) {{ int64_t {x} = s_ + {i}; d_[{i}] = {body}; }} r_.hdr[1] = n_; ({out}){{n_, d_, r_.hdr}}; }})"))
    }

    pub(super) fn proven_fields(&mut self, e: &Expr, owner: &str, fields: &[(String, Expr)]) -> HashSet<String> {
        let mut out = HashSet::new();
        let Some(decl) = self.field_refines.get(owner).cloned() else { return out };
        for (fname, refine, tyname) in decl {
            let Some((_, fe)) = fields.iter().find(|(n, _)| *n == fname) else { continue };
            let alias = tyname.as_ref().and_then(|n| self.alias_refines.get(n)).cloned();
            let conds: Vec<Expr> = refine.into_iter().chain(alias).collect();
            if !conds.is_empty() && (conds.iter().all(|c| self.refine_holds(c, fe)) || self.smt.proves(e, &format!("field:{fname}"))) {
                out.insert(fname);
            }
        }
        out
    }
}

pub(super) fn tail_calls(e: &Expr, name: &str, out: &mut Spans) {
    match &e.kind {
        ExprKind::Call(f, _) if is_name(f, name) => {
            out.insert(key(e));
        }
        ExprKind::If(_, a, b) => {
            tail_calls(a, name, out);
            if let Some(b) = b {
                tail_calls(b, name, out);
            }
        }
        ExprKind::Match(_, arms, _) => arms.iter().for_each(|a| tail_calls(&a.body, name, out)),
        ExprKind::Block(stmts) => {
            if let Some(Stmt::Expr(x)) = stmts.last() {
                tail_calls(x, name, out);
            }
        }
        _ => {}
    }
}

impl Cx<'_> {
    pub(super) fn inner_capture(&mut self, i: usize, cv: &str, outer: &HashMap<String, (String, String)>) -> String {
        match outer.get(cv) {
            Some((_, ct)) => {
                let cell = format!("(*env_->c{i})");
                self.mutable.insert(cell.clone());
                self.boxed.insert(cell.clone(), (format!("env_->c{i}"), ct.clone()));
                cell
            }
            None => format!("env_->c{i}"),
        }
    }
}

impl Cx<'_> {
    pub(super) fn push_reserve(&mut self, body: &Expr, s: &str, e: &str) -> G {
        let ExprKind::Block(stmts) = &body.kind else { return Ok(String::new()) };
        let mut out = String::new();
        let mut seen = HashSet::new();
        for st in stmts {
            let Stmt::Assign(v, x, _) = st else { continue };
            let ExprKind::Method { recv, name, args, .. } = &x.kind else { continue };
            if name != "push" || args.len() != 1 || !is_name(recv, v) || !seen.insert(v.clone()) {
                continue;
            }
            let Some((var, lt)) = self.lookup(v) else { continue };
            let Some(et) = elem(&lt, "List") else { continue };
            if self.sb_vars.contains_key(&var) {
                continue;
            }
            let ec = self.decl(&et)?;
            let lc = self.cty(&lt)?;
            let free_old = if self.rc_vars.contains_key(&var) { "if (o_.hdr != r_.hdr) ss_free(o_.hdr); " } else { "" };
            write!(out, "{{ RawL o_ = TO_RAW({var}); RawL r_ = raw_reserve_exact(o_, {e} - {s}, sizeof({ec})); {var} = ({lc}){{r_.len, ({ec}*)r_.data, r_.hdr}}; {free_old}}} ").unwrap();
        }
        Ok(out)
    }
}
