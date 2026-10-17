//! Elaboration of traits into plain definitions (ADR 0027). Every trait method call and
//! overloaded operator becomes a call of its impl function or the built-in operation, every
//! bounded generic function is specialized per instantiation, and the result is printed, parsed
//! and checked again as an ordinary program, which the interpreter and native code then run.

use super::*;

const MAX_SPECS: usize = 500;

enum Target {
    Fn(String),
    Builtin,
}

enum EqShape {
    Tuple(Vec<Type>),
    Opt(Type),
    List(Type),
    Res(Type, Type),
    Record(Vec<(String, Type)>),
    Sum(Vec<(String, Vec<(String, Type)>)>),
    New(Type),
}

struct Elab<'a> {
    t: &'a TraitOut,
    c: &'a CheckOutput,
    /// The definition being rewritten, where generated helpers go.
    cur: usize,
    eq_done: HashSet<String>,
    templates: HashMap<String, (usize, FnDef)>,
    specs: HashMap<String, String>,
    taken: HashSet<String>,
    queue: Vec<(String, String, HashMap<String, Type>)>,
    out: BTreeMap<usize, Vec<FnDef>>,
    err: Option<String>,
}

/// The trait-free program for `m`, with its check output, or `None` when `m` uses no traits.
pub fn lower(m: &Module, out: &CheckOutput) -> Result<Option<(Module, CheckOutput)>, String> {
    if out.has_errors() {
        return Ok(None);
    }
    if let Some(n) = crate::elab_net::lower(m, out)? {
        let c = check(&n);
        if let Some(d) = c.diags.iter().find(|d| d.is_error()) {
            let name = d.def.clone().unwrap_or_default();
            return Err(format!("internal: the elaborated network program does not check ({name}): {} {}", d.code, d.msg));
        }
        return Ok(Some(match lower_traits(&n, &c)? {
            Some(x) => x,
            None => (n, c),
        }));
    }
    lower_traits(m, out)
}

fn lower_traits(m: &Module, out: &CheckOutput) -> Result<Option<(Module, CheckOutput)>, String> {
    if !out.traits.active || out.has_errors() {
        return Ok(None);
    }
    let mut m2 = elaborate(m, out)?;
    let mut c2 = check(&m2);
    for _ in 0..2 {
        if !c2.traits.active || c2.has_errors() {
            break;
        }
        m2 = elaborate(&m2, &c2)?;
        c2 = check(&m2);
    }
    if let Some(d) = c2.diags.iter().find(|d| d.is_error()) {
        let name = d.def.clone().unwrap_or_default();
        return Err(format!("internal: the elaborated program does not check ({name}): {} {}", d.code, d.msg));
    }
    Ok(Some((m2, c2)))
}

pub(crate) fn free_params(t: &Type, out: &mut Vec<String>) {
    match t {
        Type::Param(p) => {
            if !out.contains(p) {
                out.push(p.clone());
            }
        }
        Type::Con(_, a) | Type::Tuple(a) => a.iter().for_each(|x| free_params(x, out)),
        Type::Fn(ps, r, _) => {
            ps.iter().for_each(|x| free_params(x, out));
            free_params(r, out);
        }
        Type::Var(_) => {}
    }
}

fn ty_of(t: &Type) -> Ty {
    let src = format!("fn t_(x: {t})\n= ()");
    match parse(&src) {
        Ok(m) => match &m.defs[0] {
            Def::Fn(f) => f.params[0].ty.clone(),
            _ => unreachable!(),
        },
        Err(_) => Ty::Named { name: "Unit".into(), args: vec![], span: Span::default() },
    }
}

fn subst_ty(t: &mut Ty, map: &HashMap<String, Ty>) {
    match t {
        Ty::Named { name, args, .. } if args.is_empty() && map.contains_key(name.as_str()) => *t = map[name.as_str()].clone(),
        Ty::Named { args, .. } => args.iter_mut().for_each(|a| subst_ty(a, map)),
        Ty::Tuple(xs) => xs.iter_mut().for_each(|a| subst_ty(a, map)),
        Ty::Fn { params, ret, effects } => {
            params.iter_mut().for_each(|a| subst_ty(a, map));
            subst_ty(ret, map);
            effects.iter_mut().flat_map(|e| e.args.iter_mut()).for_each(|a| subst_ty(a, map));
        }
    }
}

fn subst_fn(f: &mut FnDef, map: &HashMap<String, Ty>) {
    for p in &mut f.params {
        subst_ty(&mut p.ty, map);
    }
    if let Some(r) = &mut f.ret {
        subst_ty(r, map);
    }
    f.effects.iter_mut().flat_map(|e| e.args.iter_mut()).for_each(|a| subst_ty(a, map));
    let mut body = std::mem::replace(&mut f.body, Expr::new(ExprKind::Unit, Span::default()));
    visit::walk_expr_mut(&mut body, &mut |e| match &mut e.kind {
        ExprKind::Method { targs, .. } => targs.iter_mut().for_each(|a| subst_ty(a, map)),
        ExprKind::Lambda { tys, .. } => tys.iter_mut().flatten().for_each(|a| subst_ty(a, map)),
        ExprKind::Block(stmts) => {
            for s in stmts {
                if let Stmt::Fn(g) = s {
                    for p in &mut g.params {
                        subst_ty(&mut p.ty, map);
                    }
                    if let Some(r) = &mut g.ret {
                        subst_ty(r, map);
                    }
                    g.effects.iter_mut().flat_map(|e| e.args.iter_mut()).for_each(|a| subst_ty(a, map));
                }
            }
        }
        _ => {}
    });
    f.body = body;
}

/// `_` lambdas become explicit ones: a rewritten call would otherwise capture the `_`.
fn explicit(x: &mut Expr, ctr: &mut usize) {
    let ExprKind::Lambda { params, body, implicit, .. } = &mut x.kind else { return };
    if !*implicit {
        return;
    }
    *implicit = false;
    let n = format!("x__{ctr}");
    *ctr += 1;
    *params = vec![n.clone()];
    visit::walk_expr_mut(body, &mut |y| match &y.kind {
        ExprKind::Placeholder => y.kind = ExprKind::Name(n.clone()),
        ExprKind::Lambda { implicit: true, .. } => explicit(y, ctr),
        _ => {}
    });
}

fn sanitize(t: &Type) -> String {
    let mut s = String::new();
    for c in t.to_string().chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('_') {
            s.push('_');
        }
    }
    s.trim_matches('_').to_string()
}

fn name_expr(n: &str) -> Expr {
    Expr::new(ExprKind::Name(n.to_string()), Span::default())
}

fn call(n: &str, args: Vec<Expr>) -> ExprKind {
    ExprKind::Call(Box::new(name_expr(n)), args)
}

fn synth(k: ExprKind) -> Expr {
    Expr::new(k, Span::default())
}

impl Elab<'_> {
    fn spec(&mut self, template: &str, map: HashMap<String, Type>) -> String {
        let Some((_, f)) = self.templates.get(template) else { return template.to_string() };
        let args: Vec<Type> = f.tparams.iter().filter_map(|p| map.get(&p.name).cloned()).collect();
        let key = format!("{template}[{}]", args.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", "));
        if let Some(n) = self.specs.get(&key) {
            return n.clone();
        }
        let tail = args.iter().map(sanitize).collect::<Vec<_>>().join("__");
        let base = if tail.is_empty() { format!("{template}__") } else { format!("{template}__{tail}") };
        let mut name = base.clone();
        let mut k = 2;
        while self.taken.contains(&name) {
            name = format!("{base}_{k}");
            k += 1;
        }
        self.taken.insert(name.clone());
        self.specs.insert(key, name.clone());
        if self.specs.len() > MAX_SPECS {
            self.err.get_or_insert_with(|| format!("E_TRAIT_RECURSION {template} is specialized more than {MAX_SPECS} times: a bounded generic calls itself at a growing type; make the recursive helper unbounded or fix its type"));
        } else {
            self.queue.push((name.clone(), template.to_string(), map));
        }
        name
    }

    fn user_eq(&self, t: &Type) -> bool {
        matches!(t, Type::Con(c, _) if self.t.impls.get(&("Eq".to_string(), c.clone())).is_some_and(|i| !i.derived))
    }

    /// The parts compared when `==` on `t` is structural: the elements of `Opt`, `List`, `Res`
    /// and tuples, and the fields of a type that derives `Eq`.
    fn eq_parts(&self, t: &Type) -> Option<EqShape> {
        match t {
            Type::Tuple(xs) => Some(EqShape::Tuple(xs.clone())),
            Type::Con(c, a) if c == "Opt" || c == "List" => Some(if c == "Opt" { EqShape::Opt(a[0].clone()) } else { EqShape::List(a[0].clone()) }),
            Type::Con(c, a) if c == "Res" => Some(EqShape::Res(a[0].clone(), a[1].clone())),
            Type::Con(c, a) if self.t.impls.get(&("Eq".to_string(), c.clone())).is_some_and(|i| i.derived) => {
                if let Some((ps, fs)) = self.c.records.get(c) {
                    let map: HashMap<String, Type> = ps.iter().cloned().zip(a.iter().cloned()).collect();
                    return Some(EqShape::Record(fs.iter().map(|(f, ft)| (f.clone(), subst_params(ft, &map))).collect()));
                }
                if let Some((ps, vs)) = self.c.sums.get(c) {
                    let map: HashMap<String, Type> = ps.iter().cloned().zip(a.iter().cloned()).collect();
                    return Some(EqShape::Sum(vs.iter().map(|(v, fs)| (v.clone(), fs.iter().flatten().map(|(f, ft)| (f.clone(), subst_params(ft, &map))).collect())).collect()));
                }
                self.c.newtypes.get(c).filter(|_| a.is_empty()).map(|i| EqShape::New(i.clone()))
            }
            _ => None,
        }
    }

    /// Whether `==` on `t` reaches a hand-written `Eq` impl.
    fn eq_needs(&self, t: &Type, seen: &mut Vec<Type>) -> bool {
        if self.user_eq(t) {
            return true;
        }
        if seen.contains(t) {
            return false;
        }
        seen.push(t.clone());
        let parts: Vec<Type> = match self.eq_parts(t) {
            Some(EqShape::Tuple(xs)) => xs,
            Some(EqShape::Opt(x) | EqShape::List(x) | EqShape::New(x)) => vec![x],
            Some(EqShape::Res(a, b)) => vec![a, b],
            Some(EqShape::Record(fs)) => fs.into_iter().map(|(_, t)| t).collect(),
            Some(EqShape::Sum(vs)) => vs.into_iter().flat_map(|(_, fs)| fs.into_iter().map(|(_, t)| t)).collect(),
            None => vec![],
        };
        parts.iter().any(|p| self.eq_needs(p, seen))
    }

    /// `x == y` at type `t`, through the impls it reaches.
    fn eq_call(&mut self, t: &Type, x: &str, y: &str) -> String {
        if self.user_eq(t) {
            if let Some(Target::Fn(f)) = self.target("Eq", "eq", t) {
                return format!("{f}({x}, {y})");
            }
        } else if self.eq_needs(t, &mut vec![]) {
            return format!("{}({x}, {y})", self.eq_helper(t));
        }
        format!("{x} == {y}")
    }

    /// A function comparing two `t`s part by part, for an `==` that reaches an `Eq` impl.
    fn eq_helper(&mut self, t: &Type) -> String {
        let name = format!("__eq__{}", sanitize(t));
        if !self.eq_done.insert(name.clone()) {
            return name;
        }
        let and = |xs: Vec<String>| if xs.is_empty() { "true".to_string() } else { xs.join(" and ") };
        let body = match self.eq_parts(t) {
            Some(EqShape::Tuple(xs)) => {
                let cs = xs.iter().enumerate().map(|(i, x)| self.eq_call(x, &format!("a.{i}"), &format!("b.{i}"))).collect();
                and(cs)
            }
            Some(EqShape::Opt(x)) => format!("match (a, b)\n  | (some(x), some(y)) => {}\n  | (none, none) => true\n  | _ => false", self.eq_call(&x, "x", "y")),
            Some(EqShape::List(x)) => format!("a.len == b.len and (0..a.len).all(i => {})", self.eq_call(&x, "a[i]", "b[i]")),
            Some(EqShape::Res(o, e)) => format!("match (a, b)\n  | (ok(x), ok(y)) => {}\n  | (err(x), err(y)) => {}\n  | _ => false", self.eq_call(&o, "x", "y"), self.eq_call(&e, "x", "y")),
            Some(EqShape::Record(fs)) => {
                let cs = fs.iter().map(|(f, ft)| self.eq_call(ft, &format!("a.{f}"), &format!("b.{f}"))).collect();
                and(cs)
            }
            Some(EqShape::New(inner)) => self.eq_call(&inner, "a.raw", "b.raw"),
            Some(EqShape::Sum(vs)) => {
                let mut arms = String::new();
                for (v, fs) in &vs {
                    let pa: Vec<String> = fs.iter().enumerate().map(|(i, (f, _))| format!("{f}: x{i}")).collect();
                    let pb: Vec<String> = fs.iter().enumerate().map(|(i, (f, _))| format!("{f}: y{i}")).collect();
                    let cs = fs.iter().enumerate().map(|(i, (_, ft))| self.eq_call(ft, &format!("x{i}"), &format!("y{i}"))).collect();
                    let (pa, pb) = if fs.is_empty() { (v.clone(), v.clone()) } else { (format!("{v}{{{}}}", pa.join(", ")), format!("{v}{{{}}}", pb.join(", "))) };
                    arms.push_str(&format!("\n  | ({pa}, {pb}) => {}", and(cs)));
                }
                if vs.len() > 1 {
                    arms.push_str("\n  | _ => false");
                }
                format!("match (a, b){arms}")
            }
            None => "a == b".to_string(),
        };
        let src = format!("fn {name}(a: {t}, b: {t}) -> Bool\n= {body}\n");
        match parse(&src) {
            Ok(m) => {
                if let Some(Def::Fn(f)) = m.defs.into_iter().next() {
                    self.taken.insert(name.clone());
                    self.out.entry(self.cur).or_default().push(f);
                }
            }
            Err(e) => {
                self.err.get_or_insert_with(|| format!("internal: the comparison of {t} does not parse: {}\n{src}", e.msg));
            }
        }
        name
    }

    fn target(&mut self, tr: &str, m: &str, st: &Type) -> Option<Target> {
        if tr == "Eq" && !self.user_eq(st) && self.eq_needs(st, &mut vec![]) {
            return Some(Target::Fn(self.eq_helper(st)));
        }
        let Type::Con(c, args) = st else {
            return match st {
                Type::Tuple(_) => Some(Target::Builtin),
                _ => None,
            };
        };
        let Some(imp) = self.t.impls.get(&(tr.to_string(), c.clone())) else { return Some(Target::Builtin) };
        if imp.derived {
            return Some(Target::Builtin);
        }
        let pmap: HashMap<String, Type> = imp.tparams.iter().cloned().zip(args.iter().cloned()).collect();
        if let Some(unit) = imp.fns.get(m).cloned() {
            if self.t.bounded.contains(&unit) {
                return Some(Target::Fn(self.spec(&unit, pmap)));
            }
            return Some(Target::Fn(unit));
        }
        let unit = self.t.defaults.get(&(tr.to_string(), m.to_string()))?.clone();
        let mut map = HashMap::from([("Self".to_string(), st.clone())]);
        for (p, a) in self.t.trait_params.get(tr).into_iter().flatten().zip(&imp.targs) {
            map.insert(p.clone(), subst_params(a, &pmap));
        }
        Some(Target::Fn(self.spec(&unit, map)))
    }

    fn method_of(tr: &str) -> &'static str {
        BUILTIN_TRAITS.iter().find(|(n, _)| *n == tr).and_then(|(_, ms)| ms.first().copied()).unwrap_or("")
    }

    fn builtin_call(m: &str, recv: Expr, mut args: Vec<Expr>) -> ExprKind {
        let b = |x: Expr| Box::new(x);
        let mut a0 = || if args.is_empty() { synth(ExprKind::Unit) } else { args.remove(0) };
        match m {
            "eq" => ExprKind::Binary(BinOp::Eq, b(recv), b(a0())),
            "cmp" => call("__cmp", vec![recv, a0()]),
            "show" => ExprKind::Field(b(recv), "str".into()),
            "hash" => call("__hash", vec![synth(ExprKind::Field(b(recv), "str".into()))]),
            "to_json" => ExprKind::Method { recv: b(name_expr("json")), name: "encode".into(), targs: vec![], args: vec![recv] },
            "add" => ExprKind::Binary(BinOp::Add, b(recv), b(a0())),
            "sub" => ExprKind::Binary(BinOp::Sub, b(recv), b(a0())),
            "mul" => ExprKind::Binary(BinOp::Mul, b(recv), b(a0())),
            "div" => ExprKind::Binary(BinOp::Div, b(recv), b(a0())),
            "neg" => ExprKind::Unary(UnOp::Neg, b(recv)),
            "index" => ExprKind::Index(b(recv), b(a0())),
            _ => ExprKind::Unit,
        }
    }

    fn rewrite(&mut self, e: &mut Expr, mono: &HashMap<String, Type>) {
        let mut ctr = 0;
        visit::walk_expr_mut(e, &mut |x| explicit(x, &mut ctr));
        visit::walk_expr_mut(e, &mut |x| self.node(x, mono));
    }

    fn node(&mut self, x: &mut Expr, mono: &HashMap<String, Type>) {
        let key = expr_key(x);
        if let ExprKind::Name(n) = &x.kind
            && let Some(&arity) = self.t.method_values.get(&key)
        {
            let params: Vec<String> = (0..arity).map(|i| format!("m__{i}")).collect();
            let recv = Box::new(name_expr(&params[0]));
            let mut body = Expr::new(ExprKind::Method { recv, name: n.clone(), targs: vec![], args: params[1..].iter().map(|p| name_expr(p)).collect() }, x.span);
            self.node(&mut body, mono);
            x.kind = ExprKind::Lambda { params, body: Box::new(body), implicit: false, tys: vec![] };
            return;
        }
        if let Some(targs) = self.t.insts.get(&key).cloned() {
            let name = match &x.kind {
                ExprKind::Name(n) => Some(n.clone()),
                ExprKind::Call(f, _) => match &f.kind {
                    ExprKind::Name(n) => Some(n.clone()),
                    _ => None,
                },
                ExprKind::Method { name, .. } | ExprKind::Field(_, name) => Some(name.clone()),
                _ => None,
            };
            if let Some(n) = name
                && let Some((_, f)) = self.templates.get(&n)
            {
                let map: HashMap<String, Type> = f.tparams.iter().filter(|p| p.name.starts_with(|c: char| c.is_ascii_uppercase())).map(|p| p.name.clone()).zip(targs.iter().map(|t| subst_params(t, mono))).collect();
                let s = self.spec(&n, map);
                match &mut x.kind {
                    ExprKind::Name(n) => *n = s,
                    ExprKind::Call(f, _) => f.kind = ExprKind::Name(s),
                    ExprKind::Method { name, .. } | ExprKind::Field(_, name) => *name = s,
                    _ => {}
                }
            }
            return;
        }
        let Some((tr, st)) = self.t.sites.get(&key).cloned() else { return };
        let st = subst_params(&st, mono);
        let kind = std::mem::replace(&mut x.kind, ExprKind::Unit);
        x.kind = match kind {
            ExprKind::Method { recv, name, args, .. } => self.method_site(&tr, &name, *recv, args, &st),
            ExprKind::Field(recv, name) => self.method_site(&tr, &name, *recv, vec![], &st),
            ExprKind::Call(f, mut args) if !args.is_empty() => {
                let name = match &f.kind {
                    ExprKind::Name(n) => n.clone(),
                    _ => String::new(),
                };
                let recv = args.remove(0);
                self.method_site(&tr, &name, recv, args, &st)
            }
            ExprKind::Binary(op, l, r) => {
                let m = Self::method_of(&tr);
                match self.target(&tr, m, &st) {
                    Some(Target::Fn(f)) => match op {
                        BinOp::Ne => ExprKind::Unary(UnOp::Not, Box::new(synth(call(&f, vec![*l, *r])))),
                        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => ExprKind::Binary(op, Box::new(synth(call(&f, vec![*l, *r]))), Box::new(synth(ExprKind::Int(0)))),
                        _ => call(&f, vec![*l, *r]),
                    },
                    _ if tr == "Ord" && !traits::native_lt(&st) => ExprKind::Binary(op, Box::new(synth(call("__cmp", vec![*l, *r]))), Box::new(synth(ExprKind::Int(0)))),
                    _ => ExprKind::Binary(op, l, r),
                }
            }
            ExprKind::Unary(UnOp::Neg, a) => match self.target(&tr, "neg", &st) {
                Some(Target::Fn(f)) => call(&f, vec![*a]),
                _ => ExprKind::Unary(UnOp::Neg, a),
            },
            ExprKind::Index(a, i) => match self.target(&tr, "index", &st) {
                Some(Target::Fn(f)) => call(&f, vec![*a, *i]),
                _ => ExprKind::Index(a, i),
            },
            other => other,
        };
    }

    fn method_site(&mut self, tr: &str, name: &str, recv: Expr, args: Vec<Expr>, st: &Type) -> ExprKind {
        let full = self.t.trait_methods.get(tr).and_then(|ms| ms.iter().find(|m| *m == name || m.rsplit("__").next() == Some(name))).cloned().unwrap_or_else(|| name.to_string());
        let name = full.as_str();
        match self.target(tr, name, st) {
            Some(Target::Fn(f)) => {
                let mut all = vec![recv];
                all.extend(args);
                call(&f, all)
            }
            Some(Target::Builtin) => Self::builtin_call(name.rsplit("__").next().unwrap_or(name), recv, args),
            None => {
                self.err.get_or_insert_with(|| format!("internal: no impl of {tr} for {st}"));
                ExprKind::Unit
            }
        }
    }

    fn rewrite_fn(&mut self, f: &mut FnDef, mono: &HashMap<String, Type>) {
        for p in &mut f.params {
            if let Some(r) = &mut p.refine {
                self.rewrite(r, mono);
            }
        }
        for x in f.pres.iter_mut().chain(f.posts.iter_mut()).chain(f.examples.iter_mut()) {
            self.rewrite(x, mono);
        }
        self.rewrite(&mut f.body, mono);
    }

    fn specialize(&mut self, name: &str, template: &str, map: &HashMap<String, Type>) -> Option<(usize, FnDef)> {
        let (origin, f) = self.templates.get(template).cloned()?;
        self.cur = origin;
        let mut g = f.clone();
        g.name = name.to_string();
        g.examples.clear();
        g.public = false;
        self.rewrite_fn(&mut g, map);
        let mut free = Vec::new();
        for t in map.values() {
            free_params(t, &mut free);
        }
        free.sort();
        let tymap: HashMap<String, Ty> = map.iter().map(|(p, t)| (p.clone(), ty_of(t))).collect();
        subst_fn(&mut g, &tymap);
        let kept: Vec<TParam> = f.tparams.iter().filter(|p| !map.contains_key(&p.name) && !free.contains(&p.name)).map(|p| TParam { bounds: vec![], ..p.clone() }).collect();
        g.tparams = free.into_iter().map(|n| TParam { name: n, kind: None, refine: None, bounds: vec![] }).chain(kept).collect();
        Some((origin, g))
    }
}

fn elaborate(m: &Module, out: &CheckOutput) -> Result<Module, String> {
    let t = &out.traits;
    let own = m.own.unwrap_or(m.defs.len());
    let index: HashMap<&str, usize> = m.defs.iter().enumerate().map(|(i, d)| (d.name(), i)).collect();
    let mut e = Elab { t, c: out, cur: 0, eq_done: HashSet::new(), templates: HashMap::new(), specs: HashMap::new(), taken: m.defs.iter().map(|d| d.name().to_string()).collect(), queue: vec![], out: BTreeMap::new(), err: None };
    let mut plain_units: HashMap<usize, Vec<FnDef>> = HashMap::new();
    for (i, d) in m.defs.iter().enumerate() {
        if let Def::Fn(f) = d
            && t.bounded.contains(&f.name)
        {
            e.templates.insert(f.name.clone(), (i, f.clone()));
        }
    }
    for (origin, f) in &t.units {
        let i = index.get(origin.as_str()).copied().unwrap_or(0);
        e.taken.insert(f.name.clone());
        if t.bounded.contains(&f.name) {
            e.templates.insert(f.name.clone(), (i, f.clone()));
        } else {
            plain_units.entry(i).or_default().push(f.clone());
        }
    }
    let none = HashMap::new();
    let mut emitted: Vec<(usize, Def)> = Vec::new();
    let mut ex_tests: Vec<(String, String)> = Vec::new();
    for (i, d) in m.defs.iter().enumerate() {
        e.cur = i;
        match d {
            Def::Fn(f) if t.bounded.contains(&f.name) => {
                if i < own {
                    for (k, x) in f.examples.iter().enumerate() {
                        let mut body = x.clone();
                        e.rewrite(&mut body, &none);
                        let tn = format!("{}__ex{}", f.name, k + 1);
                        ex_tests.push((tn.clone(), format!("{}.ex{}", f.name, k + 1)));
                        emitted.push((i, Def::Test(TestDef { name: tn, body, span: f.span })));
                    }
                }
            }
            Def::Fn(f) => {
                let mut g = f.clone();
                e.rewrite_fn(&mut g, &none);
                emitted.push((i, Def::Fn(g)));
            }
            Def::Test(x) => {
                let mut x = x.clone();
                e.rewrite(&mut x.body, &none);
                emitted.push((i, Def::Test(x)));
            }
            Def::Static(s) => {
                let mut s = s.clone();
                e.rewrite(&mut s.init, &none);
                emitted.push((i, Def::Static(s)));
            }
            Def::Type(ty) => {
                let mut ty = ty.clone();
                ty.derives.clear();
                emitted.push((i, Def::Type(ty)));
            }
            Def::Trait(_) => {}
            Def::Impl(_) => {
                for mut f in plain_units.remove(&i).unwrap_or_default() {
                    e.rewrite_fn(&mut f, &none);
                    emitted.push((i, Def::Fn(f)));
                }
            }
            other => emitted.push((i, other.clone())),
        }
    }
    while let Some((name, template, map)) = e.queue.pop() {
        if let Some((origin, g)) = e.specialize(&name, &template, &map) {
            e.out.entry(origin).or_default().push(g);
        }
    }
    if let Some(err) = e.err {
        return Err(err);
    }
    let mut defs = Vec::new();
    let mut own2 = 0;
    let mut by_origin: BTreeMap<usize, Vec<Def>> = BTreeMap::new();
    for (i, d) in emitted {
        by_origin.entry(i).or_default().push(d);
    }
    for (i, fs) in std::mem::take(&mut e.out) {
        by_origin.entry(i).or_default().extend(fs.into_iter().map(Def::Fn));
    }
    for (i, ds) in by_origin {
        if i < own {
            own2 += ds.len();
        }
        defs.extend(ds);
    }
    let text = printer::print_module(&Module { profile: m.profile.clone(), defs, own: None });
    if std::env::var_os("SSPUR_ELAB_DUMP").is_some() {
        eprintln!("{text}");
    }
    let mut m2 = parse(&text).map_err(|err| format!("internal: the elaborated program does not parse: {} at {}", err.msg, err.span.start))?;
    for d in &mut m2.defs {
        if let Def::Test(x) = d
            && let Some((_, real)) = ex_tests.iter().find(|(tn, _)| *tn == x.name)
        {
            x.name = real.clone();
        }
    }
    m2.own = Some(own2);
    Ok(m2)
}
