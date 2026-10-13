use super::*;

pub(super) type Iv = (i128, i128);
pub(super) const FULL: Iv = (i64::MIN as i128, i64::MAX as i128);
const LEN_MAX: i128 = 1 << 47;

#[derive(Clone)]
pub(super) enum Fact {
    Range(String, Iv),
    Below(String, String),
    LenOf(String, String),
    ConstLen(String, i128),
}

#[derive(Default)]
pub(super) struct Know {
    pub facts: Vec<Fact>,
    pub inv: HashMap<String, Iv>,
    pub blocked: HashSet<String>,
}

pub(super) fn fits(iv: Iv) -> bool {
    iv.0 >= FULL.0 && iv.1 <= FULL.1
}

fn clamp(iv: Iv) -> Iv {
    (iv.0.max(FULL.0), iv.1.min(FULL.1))
}

fn join(a: Iv, b: Iv) -> Iv {
    (a.0.min(b.0), a.1.max(b.1))
}

fn excludes(iv: Iv, k: i128) -> bool {
    iv.0 > k || iv.1 < k
}

pub(super) fn raw_op(op: BinOp, a: Iv, b: Iv) -> Option<Iv> {
    Some(match op {
        BinOp::Add => (a.0 + b.0, a.1 + b.1),
        BinOp::Sub => (a.0 - b.1, a.1 - b.0),
        BinOp::Mul => {
            let c = [a.0 * b.0, a.0 * b.1, a.1 * b.0, a.1 * b.1];
            (*c.iter().min().unwrap(), *c.iter().max().unwrap())
        }
        BinOp::Div => {
            if excludes(b, 0) && (b.0 > 0 || b.1 < 0) && b.0 != 0 && b.1 != 0 {
                let c = [a.0 / b.0, a.0 / b.1, a.1 / b.0, a.1 / b.1];
                (*c.iter().min().unwrap(), *c.iter().max().unwrap())
            } else {
                let m = a.0.abs().max(a.1.abs());
                (-m, m)
            }
        }
        BinOp::Rem => {
            let m = b.0.abs().max(b.1.abs()) - 1;
            let m = m.max(0);
            if a.0 >= 0 {
                (0, a.1.min(m))
            } else if a.1 <= 0 {
                (a.0.max(-m), 0)
            } else {
                (a.0.max(-m), a.1.min(m))
            }
        }
        _ => return None,
    })
}

fn negate(op: BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Ge,
        BinOp::Le => BinOp::Gt,
        BinOp::Gt => BinOp::Le,
        BinOp::Ge => BinOp::Lt,
        BinOp::Eq => BinOp::Ne,
        BinOp::Ne => BinOp::Eq,
        o => o,
    }
}

fn flip(op: BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Le => BinOp::Ge,
        BinOp::Gt => BinOp::Lt,
        BinOp::Ge => BinOp::Le,
        o => o,
    }
}

fn is_cmp(op: BinOp) -> bool {
    matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
}

fn plain_var(c: &str) -> bool {
    let c = c.strip_prefix("env_->").unwrap_or(c);
    !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
}

pub(super) fn pat_names(p: &Pat, out: &mut HashSet<String>) {
    let mut names = Vec::new();
    p.binds(&mut names);
    out.extend(names);
}

fn binders_expr(e: &Expr, out: &mut HashSet<String>) {
    visit::walk_expr(e, &mut |x| {
        match &x.kind {
            ExprKind::Lambda { params, .. } => out.extend(params.iter().cloned()),
            ExprKind::Match(_, arms, _) | ExprKind::Catch(_, arms) => arms.iter().for_each(|a| pat_names(&a.pat, out)),
            ExprKind::Block(stmts) => binders_stmts(stmts, out),
            ExprKind::Table(rows) => rows.iter().flat_map(|r| &r.cells).for_each(|c| {
                if let Cell::Pat(p) = c {
                    pat_names(p, out)
                }
            }),
            _ => {}
        }
        true
    });
}

fn binders_stmts(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Let(p, _) | Stmt::For(p, _, _) => pat_names(p, out),
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

fn stmt_exprs(s: &Stmt) -> Vec<&Expr> {
    match s {
        Stmt::Let(_, x) | Stmt::Var(_, x) | Stmt::Assign(_, x, _) | Stmt::Expr(x) => vec![x],
        Stmt::For(_, a, b) | Stmt::While(a, b) => vec![a, b],
        Stmt::Fn(f) => f.pres.iter().chain(f.posts.iter()).chain([&f.body]).collect(),
    }
}

fn assigns(stmts: &[Stmt], name: &str, out: &mut Vec<Expr>) {
    for s in stmts {
        if let Stmt::Assign(n, x, _) = s
            && n == name {
                out.push(x.clone());
            }
        for x in stmt_exprs(s) {
            visit::walk_expr(x, &mut |e| {
                if let ExprKind::Block(inner) = &e.kind {
                    for s in inner {
                        if let Stmt::Assign(n, x, _) = s
                            && n == name {
                                out.push(x.clone());
                            }
                    }
                }
                true
            });
        }
    }
}

impl Cx<'_> {
    fn var_of(&self, e: &Expr) -> Option<String> {
        let n = match &e.kind {
            ExprKind::Name(n) => n.as_str(),
            ExprKind::Placeholder => "_",
            _ => return None,
        };
        if self.know.blocked.contains(n) {
            return None;
        }
        let (c, _) = self.lookup(n)?;
        plain_var(&c).then_some(c)
    }

    fn fixed_var(&self, e: &Expr) -> Option<String> {
        self.var_of(e).filter(|c| !self.mutable.contains(c))
    }

    fn list_of_len(&self, e: &Expr) -> Option<String> {
        let recv = match &e.kind {
            ExprKind::Field(r, f) if f == "len" => r,
            ExprKind::Method { recv, name, args, .. } if name == "len" && args.is_empty() => recv,
            ExprKind::Name(_) => {
                let c = self.fixed_var(e)?;
                return self.know.facts.iter().rev().find_map(|f| match f {
                    Fact::LenOf(v, l) if *v == c => Some(l.clone()),
                    _ => None,
                });
            }
            _ => return None,
        };
        let t = self.ty(recv).ok()?;
        if !is_list(&t) {
            return None;
        }
        self.fixed_var(recv)
    }

    pub(super) fn var_iv(&self, c: &str) -> Iv {
        let mut iv = self.know.inv.get(c).copied().unwrap_or(FULL);
        for f in &self.know.facts {
            if let Fact::Range(v, r) = f
                && v == c {
                    iv = (iv.0.max(r.0), iv.1.min(r.1));
                }
        }
        iv
    }

    pub(super) fn range(&mut self, e: &Expr) -> Iv {
        match &e.kind {
            ExprKind::Int(n) => (*n as i128, *n as i128),
            ExprKind::Name(_) | ExprKind::Placeholder => match self.var_of(e) {
                Some(c) => self.var_iv(&c),
                None => FULL,
            },
            ExprKind::Binary(op, a, b) if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem) => {
                let (x, y) = (self.range(a), self.range(b));
                raw_op(*op, x, y).map_or(FULL, clamp)
            }
            ExprKind::Unary(UnOp::Neg, x) => {
                let r = self.range(x);
                clamp((-r.1, -r.0))
            }
            ExprKind::Field(r, f) if f == "len" => self.len_iv(r),
            ExprKind::Method { recv, name, args, .. } if name == "len" && args.is_empty() => self.len_iv(recv),
            ExprKind::If(c, a, Some(b)) => {
                let m = self.know.facts.len();
                self.assume(c, true);
                let x = self.range(a);
                self.know.facts.truncate(m);
                self.assume(c, false);
                let y = self.range(b);
                self.know.facts.truncate(m);
                join(x, y)
            }
            ExprKind::Call(f, _) => match &f.kind {
                ExprKind::Name(n) if self.lookup(n).is_none() => self.post_iv(n),
                _ => FULL,
            },
            _ => FULL,
        }
    }

    fn len_iv(&self, r: &Expr) -> Iv {
        if let Some(n) = self.const_len(r) {
            return (n, n);
        }
        match self.ty(r) {
            Ok(Type::Con(n, _)) if matches!(n.as_str(), "List" | "Str" | "Map") => (0, LEN_MAX),
            _ => FULL,
        }
    }

    fn post_iv(&mut self, name: &str) -> Iv {
        let Some(f) = self.fn_def(name).cloned() else { return FULL };
        if !self.eligible.contains(name) || !self.check.fn_types.get(name).is_some_and(|(_, r)| is(r, "Int")) {
            return FULL;
        }
        let mut iv = FULL;
        for p in &f.posts {
            iv = meet_bound(iv, p, "r");
        }
        iv
    }

    pub(super) fn assume(&mut self, c: &Expr, pos: bool) {
        match &c.kind {
            ExprKind::Binary(BinOp::And, a, b) if pos => {
                self.assume(a, true);
                self.assume(b, true);
            }
            ExprKind::Binary(BinOp::Or, a, b) if !pos => {
                self.assume(a, false);
                self.assume(b, false);
            }
            ExprKind::Unary(UnOp::Not, x) => self.assume(x, !pos),
            ExprKind::Binary(op, a, b) if is_cmp(*op) => {
                let op = if pos { *op } else { negate(*op) };
                if !self.ty(a).is_ok_and(|t| is(&t, "Int")) {
                    return;
                }
                self.constrain(a, op, b);
                self.constrain(b, flip(op), a);
            }
            _ => {}
        }
    }

    fn constrain(&mut self, x: &Expr, op: BinOp, rhs: &Expr) {
        let Some(c) = self.fixed_var(x) else { return };
        if op == BinOp::Lt
            && let Some(l) = self.list_of_len(rhs) {
                self.know.facts.push(Fact::Below(c.clone(), l));
            }
        let r = self.range(rhs);
        let iv = match op {
            BinOp::Lt => (FULL.0, r.1 - 1),
            BinOp::Le => (FULL.0, r.1),
            BinOp::Gt => (r.0 + 1, FULL.1),
            BinOp::Ge => (r.0, FULL.1),
            BinOp::Eq => r,
            BinOp::Ne if r.0 == r.1 => {
                let cur = self.var_iv(&c);
                if cur.0 == r.0 {
                    (r.0 + 1, FULL.1)
                } else if cur.1 == r.0 {
                    (FULL.0, r.0 - 1)
                } else {
                    return;
                }
            }
            _ => return,
        };
        self.know.facts.push(Fact::Range(c, iv));
    }

    pub(super) fn holds(&mut self, c: &Expr) -> bool {
        match &c.kind {
            ExprKind::Bool(b) => *b,
            ExprKind::Binary(BinOp::And, a, b) => {
                if !self.holds(a) {
                    return false;
                }
                let m = self.know.facts.len();
                self.assume(a, true);
                let r = self.holds(b);
                self.know.facts.truncate(m);
                r
            }
            ExprKind::Binary(BinOp::Or, a, b) => self.holds(a) || self.holds(b),
            ExprKind::Binary(op, a, b) if is_cmp(*op) => {
                if !self.ty(a).is_ok_and(|t| is(&t, "Int")) {
                    return false;
                }
                if *op == BinOp::Lt && self.below(a, b) {
                    return true;
                }
                let (x, y) = (self.range(a), self.range(b));
                match op {
                    BinOp::Lt => x.1 < y.0,
                    BinOp::Le => x.1 <= y.0,
                    BinOp::Gt => x.0 > y.1,
                    BinOp::Ge => x.0 >= y.1,
                    BinOp::Eq => x.0 == x.1 && y.0 == y.1 && x.0 == y.0,
                    BinOp::Ne => x.1 < y.0 || x.0 > y.1,
                    _ => false,
                }
            }
            _ => false,
        }
    }

    pub(super) fn below(&self, i: &Expr, len: &Expr) -> bool {
        let (Some(c), Some(l)) = (self.fixed_var(i), self.list_of_len(len)) else { return false };
        self.below_var(&c, &l)
    }

    pub(super) fn below_var(&self, c: &str, l: &str) -> bool {
        self.know.facts.iter().any(|f| matches!(f, Fact::Below(v, x) if v == c && x == l))
    }

    pub(super) fn index_safe(&mut self, a: &Expr, i: &Expr) -> bool {
        if let Some(n) = self.const_len(a) {
            let r = self.range(i);
            if r.0 >= 0 && r.1 < n {
                return true;
            }
        }
        let (Some(l), Some(c)) = (self.fixed_var(a), self.fixed_var(i)) else { return false };
        self.var_iv(&c).0 >= 0 && self.below_var(&c, &l)
    }

    pub(super) fn bind_fact(&mut self, c: &str, e: &Expr) {
        if self.mutable.contains(c) || !plain_var(c) {
            return;
        }
        if let Some(l) = self.list_of_len(e) {
            self.know.facts.push(Fact::LenOf(c.to_string(), l));
        }
        if let ExprKind::List(xs) = &e.kind {
            self.know.facts.push(Fact::ConstLen(c.to_string(), xs.len() as i128));
        }
        let iv = self.range(e);
        if iv != FULL {
            self.know.facts.push(Fact::Range(c.to_string(), iv));
        }
    }

    pub(super) fn for_range_facts(&mut self, c: &str, a: &Expr, b: &Expr) {
        let (x, y) = (self.range(a), self.range(b));
        self.know.facts.push(Fact::Range(c.to_string(), (x.0, y.1 - 1)));
        if let Some(l) = self.list_of_len(b)
            && x.0 >= 0 {
                self.know.facts.push(Fact::Below(c.to_string(), l));
            }
    }

    pub(super) fn var_invariant(&mut self, name: &str, init: &Expr, rest: &[Stmt]) -> Option<Iv> {
        let mut bound = HashSet::new();
        binders_stmts(rest, &mut bound);
        for s in rest {
            for x in stmt_exprs(s) {
                binders_expr(x, &mut bound);
            }
        }
        if bound.contains(name) {
            return None;
        }
        let mut rhs = Vec::new();
        assigns(rest, name, &mut rhs);
        let mut iv = self.range(init);
        if !rhs.is_empty() && rhs.iter().all(|x| is_increment(x, name)) && iv.0 >= 0 && iv.1 <= 1 << 61 {
            return Some((iv.0, 1 << 62));
        }
        let key = self.fresh("inv");
        let saved_blocked = std::mem::replace(&mut self.know.blocked, bound);
        self.scopes.push(HashMap::from([(name.to_string(), (key.clone(), Type::int()))]));
        self.mutable.insert(key.clone());
        let mut result = None;
        for round in 0..4 {
            self.know.inv.insert(key.clone(), iv);
            let mut next = iv;
            for x in &rhs {
                next = join(next, self.range(x));
            }
            if next == iv {
                result = Some(iv);
                break;
            }
            if round >= 1 {
                if next.0 < iv.0 {
                    next.0 = FULL.0;
                }
                if next.1 > iv.1 {
                    next.1 = FULL.1;
                }
            }
            iv = next;
        }
        self.scopes.pop();
        self.know.inv.remove(&key);
        self.know.blocked = saved_blocked;
        result.filter(|iv| *iv != FULL)
    }
}

fn is_list(t: &Type) -> bool {
    matches!(t, Type::Con(n, _) if n == "List")
}

fn meet_bound(iv: Iv, c: &Expr, name: &str) -> Iv {
    let lit = |e: &Expr| match &e.kind {
        ExprKind::Int(n) => Some(*n as i128),
        ExprKind::Unary(UnOp::Neg, x) => match x.kind {
            ExprKind::Int(n) => Some(-(n as i128)),
            _ => None,
        },
        _ => None,
    };
    let is_name = |e: &Expr| matches!(&e.kind, ExprKind::Name(n) if n == name);
    match &c.kind {
        ExprKind::Binary(BinOp::And, a, b) => meet_bound(meet_bound(iv, a, name), b, name),
        ExprKind::Binary(op, a, b) if is_cmp(*op) => {
            let (op, k) = if is_name(a) {
                match lit(b) {
                    Some(k) => (*op, k),
                    None => return iv,
                }
            } else if is_name(b) {
                match lit(a) {
                    Some(k) => (flip(*op), k),
                    None => return iv,
                }
            } else {
                return iv;
            };
            match op {
                BinOp::Lt => (iv.0, iv.1.min(k - 1)),
                BinOp::Le => (iv.0, iv.1.min(k)),
                BinOp::Gt => (iv.0.max(k + 1), iv.1),
                BinOp::Ge => (iv.0.max(k), iv.1),
                BinOp::Eq => (iv.0.max(k), iv.1.min(k)),
                _ => iv,
            }
        }
        _ => iv,
    }
}

impl Cx<'_> {
    pub(super) fn under<T>(&mut self, c: &Expr, pos: bool, f: impl FnOnce(&mut Self) -> T) -> T {
        let m = self.know.facts.len();
        self.assume(c, pos);
        let r = f(self);
        self.know.facts.truncate(m);
        r
    }

    fn alias_refine(&self, ty: &Ty) -> Option<Expr> {
        match ty {
            Ty::Named { name, .. } => self.alias_refines.get(name).cloned(),
            _ => None,
        }
    }

    pub(super) fn entry_checks(&self, f: &FnDef) -> bool {
        !f.pres.is_empty() || f.params.iter().any(|p| p.refine.is_some() || self.alias_refine(&p.ty).is_some())
    }

    fn with_underscore<T>(&mut self, c: &str, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(HashMap::from([("_".to_string(), (c.to_string(), Type::int()))]));
        let r = f(self);
        self.scopes.pop();
        r
    }

    fn entry_conds(&self, f: &FnDef) -> Vec<(Option<usize>, Expr)> {
        let mut out = Vec::new();
        for (i, p) in f.params.iter().enumerate() {
            out.extend(self.alias_refine(&p.ty).map(|r| (Some(i), r)));
            out.extend(p.refine.clone().map(|r| (Some(i), r)));
        }
        out.extend(f.pres.iter().map(|p| (None, p.clone())));
        out
    }

    pub(super) fn assume_entry(&mut self, f: &FnDef) {
        for (i, c) in self.entry_conds(f) {
            match i {
                Some(i) => self.with_underscore(&format!("a{i}"), |cx| cx.assume(&c, true)),
                None => self.assume(&c, true),
            }
        }
    }

    pub(super) fn callee_safe(&mut self, name: &str, args: &[Expr], site: &Expr) -> bool {
        if self.generics.contains_key(name) {
            return false;
        }
        let Some(f) = self.fn_def(name).cloned() else { return false };
        if !self.entry_checks(&f) || f.params.len() != args.len() {
            return false;
        }
        self.callee_safe_iv(&f, args) || self.smt.proves(site, "call")
    }

    fn callee_safe_iv(&mut self, f: &FnDef, args: &[Expr]) -> bool {
        let m = self.know.facts.len();
        let mut scope = HashMap::new();
        let mut keys = Vec::new();
        for (p, a) in f.params.iter().zip(args) {
            let c = match self.fixed_var(a) {
                Some(c) => c,
                None => {
                    let k = self.fresh("pa");
                    let iv = self.range(a);
                    self.know.facts.push(Fact::Range(k.clone(), iv));
                    k
                }
            };
            keys.push(c.clone());
            scope.insert(p.name.clone(), (c, Type::int()));
        }
        let saved = std::mem::replace(&mut self.scopes, vec![scope]);
        let mut ok = true;
        for (i, c) in self.entry_conds(f) {
            ok = match i {
                Some(i) => self.with_underscore(&keys[i], |cx| cx.holds(&c)),
                None => self.holds(&c),
            };
            if !ok {
                break;
            }
        }
        self.scopes = saved;
        self.know.facts.truncate(m);
        ok
    }
}

impl Cx<'_> {
    pub(super) fn refine_holds(&mut self, r: &Expr, e: &Expr) -> bool {
        if !self.ty(e).is_ok_and(|t| is(&t, "Int")) {
            return false;
        }
        let iv = self.range(e);
        if iv == FULL {
            return false;
        }
        let k = self.fresh("rk");
        let m = self.know.facts.len();
        self.know.facts.push(Fact::Range(k.clone(), iv));
        let ok = self.with_underscore(&k, |cx| cx.holds(r));
        self.know.facts.truncate(m);
        ok
    }
}

impl Cx<'_> {
    pub(super) fn hit_var(&self, map: &str, key: &Expr) -> Option<String> {
        if !self.hit_ok || !self.inplace.contains(map) || !map.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$') {
            return None;
        }
        let k = self.fixed_var(key)?;
        k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$').then(|| format!("hit_{map}_{k}"))
    }
}

fn is_increment(e: &Expr, name: &str) -> bool {
    let is_v = |x: &Expr| matches!(&x.kind, ExprKind::Name(n) if n == name);
    let one = |x: &Expr| matches!(x.kind, ExprKind::Int(1));
    matches!(&e.kind, ExprKind::Binary(BinOp::Add, a, b) if (is_v(a) && one(b)) || (one(a) && is_v(b)))
}

impl Cx<'_> {
    pub(super) fn const_len(&self, a: &Expr) -> Option<i128> {
        let c = self.fixed_var(a)?;
        self.know.facts.iter().rev().find_map(|f| match f {
            Fact::ConstLen(v, n) if *v == c => Some(*n),
            _ => None,
        })
    }
}
