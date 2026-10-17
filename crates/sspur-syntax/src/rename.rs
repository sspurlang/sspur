use crate::ast::*;
use std::collections::{HashMap, HashSet};

/// Effect names that are built in, so an effect row's `log` or `fail[E]` is never a package's function of that name.
pub const BUILTIN_EFFECTS: &[&str] = &["log", "fail", "div", "fs", "io", "proc", "time", "env", "net", "db", "rand", "conc", "yield", "ffi", "dev", "unsafe", "mmio", "static"];

pub fn rename_module(m: &mut Module, from: &str, to: &str, user_methods: &HashSet<(u32, u32)>) {
    let map = HashMap::from([(from.to_string(), to.to_string())]);
    rename_defs(&mut m.defs, &map, user_methods);
}

/// Renames every top-level name in `map` at once, scope-aware.
pub fn rename_defs(defs: &mut [Def], map: &HashMap<String, String>, user_methods: &HashSet<(u32, u32)>) {
    let r = Renamer { map, user_methods };
    for d in defs {
        let tps: Vec<&str> = match &*d {
            Def::Type(t) => t.params.iter().map(|p| p.name.as_str()).collect(),
            Def::Fn(f) => f.tparams.iter().map(|p| p.name.as_str()).collect(),
            Def::Effect(e) => e.params.iter().map(|p| p.name.as_str()).collect(),
            Def::Trait(t) => t.params.iter().map(|p| p.name.as_str()).collect(),
            Def::Impl(i) => i.tparams.iter().map(|p| p.name.as_str()).collect(),
            _ => vec![],
        };
        if map.len() > 1 && tps.iter().any(|p| map.contains_key(*p)) {
            let sub: HashMap<String, String> = map.iter().filter(|(k, _)| !tps.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
            Renamer { map: &sub, user_methods }.def(d);
        } else {
            r.def(d);
        }
    }
}

struct Renamer<'a> {
    map: &'a HashMap<String, String>,
    user_methods: &'a HashSet<(u32, u32)>,
}

type Scope = Vec<HashSet<String>>;

impl Renamer<'_> {
    fn hit(&self, n: &mut String) {
        if let Some(t) = self.map.get(n.as_str()) {
            *n = t.clone();
        }
    }

    fn hit_effect(&self, n: &mut String) {
        if !BUILTIN_EFFECTS.contains(&n.as_str()) {
            self.hit(n);
        }
    }

    fn shadows(&self, n: &str) -> bool {
        self.map.contains_key(n)
    }

    fn bound(scope: &Scope, n: &str) -> bool {
        scope.iter().any(|s| s.contains(n))
    }

    fn def(&self, d: &mut Def) {
        match d {
            Def::Type(t) => {
                self.hit(&mut t.name);
                if let Some(d) = &mut t.drop {
                    self.hit(d);
                }
                let tparams: Vec<String> = t.params.iter().map(|p| p.name.clone()).collect();
                let shadow = tparams.iter().any(|p| self.shadows(p));
                match &mut t.body {
                    TypeBody::Record(fs) => self.fields(fs, shadow),
                    TypeBody::Sum(vs) => {
                        for v in vs {
                            self.hit(&mut v.name);
                            if let Some(fs) = &mut v.fields {
                                self.fields(fs, shadow);
                            }
                        }
                    }
                    TypeBody::Alias(ty, r) => {
                        if !shadow {
                            self.ty(ty);
                        }
                        if let Some(r) = r {
                            self.contract(r, &["_"]);
                        }
                    }
                    TypeBody::New(ty) => {
                        if !shadow {
                            self.ty(ty);
                        }
                    }
                }
            }
            Def::Fn(f) => self.fn_def(f, true, false),
            Def::Trait(t) => {
                self.hit(&mut t.name);
                let shadow = t.params.iter().any(|p| self.shadows(&p.name));
                for m in &mut t.methods {
                    self.fn_def(&mut m.sig, true, shadow);
                }
            }
            Def::Impl(i) => {
                let own = self.map.contains_key(&i.trait_name);
                self.hit(&mut i.trait_name);
                let shadow = i.tparams.iter().any(|p| self.shadows(&p.name));
                if !shadow {
                    i.trait_args.iter_mut().for_each(|t| self.ty(t));
                    self.ty(&mut i.target);
                    for p in &mut i.tparams {
                        p.bounds.iter_mut().for_each(|t| self.ty(t));
                    }
                }
                for f in &mut i.fns {
                    self.fn_def(f, own, shadow);
                }
                i.refresh_key();
            }
            Def::Test(t) => {
                self.hit(&mut t.name);
                self.expr(&mut t.body, &mut vec![]);
            }
            Def::Store(st) => {
                self.hit(&mut st.name);
                self.ty(&mut st.key);
                self.ty(&mut st.val);
            }
            Def::Svc(sv) => {
                self.hit(&mut sv.name);
                #[cfg(not(target_family = "wasm"))]
                sv.params.iter_mut().for_each(|p| self.ty(&mut p.ty));
                sv.eps.iter_mut().for_each(|e| self.hit(&mut e.handler));
            }
            Def::Static(s) => {
                self.hit(&mut s.name);
                self.ty(&mut s.ty);
                self.expr(&mut s.init, &mut vec![]);
            }
            Def::Use(_) => {}
            Def::Effect(e) => {
                self.hit(&mut e.name);
                let shadow = e.params.iter().any(|p| self.shadows(&p.name));
                for op in &mut e.ops {
                    self.hit(&mut op.name);
                    if !shadow {
                        op.params.iter_mut().for_each(|p| self.ty(&mut p.ty));
                        if let Some(r) = &mut op.ret {
                            self.ty(r);
                        }
                    }
                }
            }
        }
    }

    fn fn_def(&self, f: &mut FnDef, rename: bool, outer_shadow: bool) {
        if rename {
            self.hit(&mut f.name);
        }
        let shadow = outer_shadow || f.tparams.iter().any(|p| self.shadows(&p.name));
        if !shadow {
            for p in &mut f.tparams {
                p.bounds.iter_mut().for_each(|t| self.ty(t));
            }
        }
        for p in &mut f.params {
            if !shadow {
                self.ty(&mut p.ty);
            }
            if let Some(r) = &mut p.refine {
                self.contract(r, &["_"]);
            }
        }
        if let (Some(t), false) = (&mut f.ret, shadow) {
            self.ty(t);
        }
        for e in &mut f.effects {
            self.hit_effect(&mut e.name);
        }
        if !shadow {
            for e in &mut f.effects {
                e.args.iter_mut().for_each(|t| self.ty(t));
            }
        }
        let params: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
        let names: Vec<&str> = params.iter().map(String::as_str).collect();
        for p in &mut f.pres {
            self.contract(p, &names);
        }
        for p in &mut f.examples {
            self.contract(p, &[]);
        }
        let mut with_r = names.clone();
        with_r.push("r");
        for p in &mut f.posts {
            self.contract(p, &with_r);
        }
        let mut scope = vec![params.into_iter().collect()];
        self.expr(&mut f.body, &mut scope);
    }

    fn fields(&self, fs: &mut [Field], shadow: bool) {
        for f in fs {
            if !shadow {
                self.ty(&mut f.ty);
            }
            if let Some(r) = &mut f.refine {
                self.contract(r, &["_"]);
            }
        }
    }

    fn contract(&self, e: &mut Expr, binds: &[&str]) {
        let mut scope = vec![binds.iter().map(|s| s.to_string()).collect()];
        self.expr(e, &mut scope);
    }

    fn ty(&self, t: &mut Ty) {
        match t {
            Ty::Named { name, args, .. } => {
                self.hit(name);
                args.iter_mut().for_each(|a| self.ty(a));
            }
            Ty::Tuple(xs) => xs.iter_mut().for_each(|a| self.ty(a)),
            Ty::Fn { params, ret, effects } => {
                params.iter_mut().for_each(|a| self.ty(a));
                self.ty(ret);
                for e in effects {
                    self.hit_effect(&mut e.name);
                    e.args.iter_mut().for_each(|a| self.ty(a));
                }
            }
        }
    }

    fn pat(&self, p: &mut Pat, binds: &mut HashSet<String>) {
        match p {
            Pat::Bind(n) => {
                binds.insert(n.clone());
            }
            Pat::Tuple(xs) => xs.iter_mut().for_each(|x| self.pat(x, binds)),
            Pat::Or(xs) => xs.iter_mut().for_each(|x| self.pat(x, binds)),
            Pat::List { head, rest, tail } => {
                if let Some(Some(r)) = rest {
                    binds.insert(r.clone());
                }
                head.iter_mut().chain(tail.iter_mut()).for_each(|x| self.pat(x, binds));
            }
            Pat::Ctor { name, args } => {
                self.hit(name);
                match args {
                    CtorArgs::Positional(xs) => xs.iter_mut().for_each(|x| self.pat(x, binds)),
                    CtorArgs::Record(fs) => fs.iter_mut().for_each(|(_, x)| self.pat(x, binds)),
                    CtorArgs::None => {}
                }
            }
            _ => {}
        }
    }

    fn expr(&self, e: &mut Expr, scope: &mut Scope) {
        let span = e.span;
        match &mut e.kind {
            ExprKind::Name(n) => {
                if !Self::bound(scope, n) {
                    self.hit(n);
                }
            }
            ExprKind::Field(x, f) => {
                self.expr(x, scope);
                if self.user_methods.contains(&(span.start, span.end)) {
                    self.hit(f);
                }
            }
            ExprKind::Method { recv, name, targs, args } => {
                self.expr(recv, scope);
                if self.user_methods.contains(&(span.start, span.end)) {
                    self.hit(name);
                }
                targs.iter_mut().for_each(|t| self.ty(t));
                args.iter_mut().for_each(|a| self.expr(a, scope));
            }
            ExprKind::Lambda { params, body, .. } => {
                scope.push(params.iter().cloned().collect());
                self.expr(body, scope);
                scope.pop();
            }
            ExprKind::Record { ctor, fields } => {
                if let Some(c) = ctor {
                    self.hit(c);
                }
                fields.iter_mut().for_each(|(_, x)| self.expr(x, scope));
            }
            ExprKind::Table(rows) => {
                for r in rows {
                    let mut binds = HashSet::new();
                    for c in &mut r.cells {
                        match c {
                            Cell::Pat(p) => self.pat(p, &mut binds),
                            Cell::Cond(e) => self.expr(e, scope),
                            Cell::Any => {}
                        }
                    }
                    scope.push(binds);
                    self.expr(&mut r.out, scope);
                    scope.pop();
                }
            }
            ExprKind::Match(s, arms, _) | ExprKind::Catch(s, arms) | ExprKind::Handle(s, arms) => {
                self.expr(s, scope);
                for a in arms {
                    let mut binds = HashSet::new();
                    self.pat(&mut a.pat, &mut binds);
                    scope.push(binds);
                    if let Some(g) = &mut a.guard {
                        self.expr(g, scope);
                    }
                    self.expr(&mut a.body, scope);
                    scope.pop();
                }
            }
            ExprKind::Block(stmts) => {
                let local_fns: HashSet<String> = stmts.iter().filter_map(|s| if let Stmt::Fn(f) = s { Some(f.name.clone()) } else { None }).collect();
                scope.push(local_fns);
                for s in stmts {
                    match s {
                        Stmt::Expr(x) => self.expr(x, scope),
                        Stmt::Let(p, x) => {
                            self.expr(x, scope);
                            let mut binds = HashSet::new();
                            self.pat(p, &mut binds);
                            scope.last_mut().unwrap().extend(binds);
                        }
                        Stmt::Var(n, x) => {
                            self.expr(x, scope);
                            scope.last_mut().unwrap().insert(n.clone());
                        }
                        Stmt::Assign(_, x, _) => self.expr(x, scope),
                        Stmt::While(c, body) => {
                            self.expr(c, scope);
                            self.expr(body, scope);
                        }
                        Stmt::Fn(f) => {
                            let shadow = f.tparams.iter().any(|p| self.shadows(&p.name));
                            for p in &mut f.params {
                                if !shadow {
                                    self.ty(&mut p.ty);
                                }
                            }
                            if let (Some(t), false) = (&mut f.ret, shadow) {
                                self.ty(t);
                            }
                            for e in &mut f.effects {
                                self.hit_effect(&mut e.name);
                                if !shadow {
                                    e.args.iter_mut().for_each(|t| self.ty(t));
                                }
                            }
                            scope.push(f.params.iter().map(|p| p.name.clone()).chain(["r".to_string()]).collect());
                            for x in f.pres.iter_mut().chain(f.posts.iter_mut()).chain(f.examples.iter_mut()) {
                                self.expr(x, scope);
                            }
                            self.expr(&mut f.body, scope);
                            scope.pop();
                        }
                        Stmt::For(p, it, body) => {
                            self.expr(it, scope);
                            let mut binds = HashSet::new();
                            self.pat(p, &mut binds);
                            scope.push(binds);
                            self.expr(body, scope);
                            scope.pop();
                        }
                    }
                }
                scope.pop();
            }
            _ => {
                for c in children_mut(e) {
                    self.expr(c, scope);
                }
            }
        }
    }
}

pub(crate) fn children_mut(e: &mut Expr) -> Vec<&mut Expr> {
    let mut out = Vec::new();
    match &mut e.kind {
        ExprKind::Str(parts) => {
            for p in parts {
                if let StrPart::Expr(x) = p {
                    out.push(x);
                }
            }
        }
        ExprKind::Unary(_, x) | ExprKind::Raise(x) | ExprKind::Return(x) => out.push(x),
        ExprKind::Call(f, args) => {
            out.push(&mut **f);
            out.extend(args.iter_mut());
        }
        ExprKind::Index(a, b) | ExprKind::Binary(_, a, b) | ExprKind::Range(a, b) => {
            out.push(&mut **a);
            out.push(&mut **b);
        }
        ExprKind::If(c, t, f) => {
            out.push(&mut **c);
            out.push(&mut **t);
            if let Some(f) = f {
                out.push(&mut **f);
            }
        }
        ExprKind::List(xs) | ExprKind::Tuple(xs) | ExprKind::Par(xs) => out.extend(xs.iter_mut()),
        ExprKind::With(base, ups) => {
            out.push(&mut **base);
            for (p, v) in ups {
                for seg in p {
                    if let PathSeg::Index(i) = seg {
                        out.push(i);
                    }
                }
                out.push(v);
            }
        }
        _ => {}
    }
    out
}
