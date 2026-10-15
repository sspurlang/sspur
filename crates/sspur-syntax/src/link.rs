//! Name resolution across packages: `pkg.f`, `pkg.T` and names imported with `use pkg.{a, b}`
//! become the mangled names of the dependency's definitions (`pkg__f`, `Pkg__T`).

use crate::ast::*;
use crate::SyntaxError;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Fn,
    Type,
    Ctor,
    Effect,
    Op,
    Trait,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub kind: Kind,
    pub mangled: String,
    pub public: bool,
    pub arity: usize,
}

/// Everything a package can name in its dependencies.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    pub pkgs: BTreeMap<String, BTreeMap<String, Item>>,
    pub imported: BTreeMap<String, Item>,
    /// Packages that are only dependencies of dependencies.
    pub indirect: Vec<String>,
}

pub fn mangle(pkg: &str, name: &str) -> String {
    if name.starts_with(|c: char| c.is_ascii_uppercase()) {
        let mut p = pkg.to_string();
        p[..1].make_ascii_uppercase();
        format!("{p}__{name}")
    } else {
        format!("{pkg}__{name}")
    }
}

/// `pkg.name` for a mangled name of one of `pkgs`, else the name itself.
pub fn demangle_name(n: &str, pkgs: &impl Fn(&str) -> bool) -> Option<String> {
    let i = n.find("__")?;
    let p = n[..i].to_ascii_lowercase();
    let rest = &n[i + 2..];
    (!rest.is_empty() && pkgs(&p)).then(|| format!("{p}.{rest}"))
}

/// Rewrites every identifier in `text` that is a mangled name of one of `pkgs`.
pub fn demangle(text: &str, pkgs: &impl Fn(&str) -> bool) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let w = &text[s..i];
            match demangle_name(w, pkgs) {
                Some(d) => out.push_str(&d),
                None => out.push_str(w),
            }
        } else {
            let s = i;
            i += 1;
            while i < b.len() && !b[i].is_ascii_alphabetic() && b[i] != b'_' {
                i += 1;
            }
            out.push_str(&text[s..i]);
        }
    }
    out
}

impl Scope {
    pub fn exports(&self, pkg: &str) -> String {
        let names: Vec<&str> = self.pkgs.get(pkg).into_iter().flatten().filter(|(_, it)| it.public && it.kind != Kind::Ctor && it.kind != Kind::Op).map(|(n, _)| n.as_str()).collect();
        if names.is_empty() { "nothing".into() } else { names.join(", ") }
    }

    pub fn lookup(&self, pkg: &str, name: &str, span: Span) -> Result<&Item, SyntaxError> {
        let Some(items) = self.pkgs.get(pkg) else {
            return Err(SyntaxError::new("E_PKG_UNKNOWN", format!("no dependency '{pkg}'; add it with 'sspur add <path|git-url>'"), span));
        };
        match items.get(name) {
            Some(it) if it.public => Ok(it),
            Some(_) => Err(SyntaxError::new("E_PKG_PRIVATE", format!("{pkg}.{name} is private to {pkg}; it exports {}", self.exports(pkg)), span)),
            None => Err(SyntaxError::new("E_PKG_NAME", format!("{pkg} has no '{name}'; it exports {}", self.exports(pkg)), span)),
        }
    }
}

type Locals = Vec<HashSet<String>>;

struct R<'a> {
    s: &'a Scope,
    errs: Vec<SyntaxError>,
    tps: Vec<String>,
}

pub fn resolve(defs: &mut [Def], scope: &Scope) -> Vec<SyntaxError> {
    let mut r = R { s: scope, errs: vec![], tps: vec![] };
    for d in defs {
        r.def(d);
    }
    r.errs
}

fn bound(l: &Locals, n: &str) -> bool {
    l.iter().any(|s| s.contains(n))
}

impl R<'_> {
    fn qual(&mut self, n: &mut String, span: Span, kinds: &[Kind]) {
        if let Some((p, x)) = n.split_once('.') {
            if !self.s.pkgs.contains_key(p) && !x.starts_with(|c: char| c.is_ascii_uppercase()) {
                return;
            }
            match self.s.lookup(p, x, span) {
                Ok(it) if kinds.contains(&it.kind) => *n = it.mangled.clone(),
                Ok(_) => self.errs.push(SyntaxError::new("E_PKG_KIND", format!("{n} can't be used here"), span)),
                Err(e) => self.errs.push(e),
            }
        } else if let Some(it) = self.s.imported.get(n.as_str()).filter(|it| kinds.contains(&it.kind)) {
            *n = it.mangled.clone();
        }
    }

    fn def(&mut self, d: &mut Def) {
        let span = d.span();
        match d {
            Def::Type(t) => {
                self.tps = t.params.iter().map(|p| p.name.clone()).collect();
                match &mut t.body {
                    TypeBody::Record(fs) => self.fields(fs),
                    TypeBody::Sum(vs) => vs.iter_mut().filter_map(|v| v.fields.as_mut()).for_each(|fs| self.fields(fs)),
                    TypeBody::Alias(ty, r) => {
                        self.ty(ty, span);
                        if let Some(r) = r {
                            self.expr(r, &mut vec![HashSet::from(["_".to_string()])]);
                        }
                    }
                    TypeBody::New(ty) => self.ty(ty, span),
                }
            }
            Def::Fn(f) => {
                self.tps = f.tparams.iter().map(|p| p.name.clone()).collect();
                self.bounds(&mut f.tparams, span);
                self.fn_def(f, &mut vec![]);
            }
            Def::Trait(t) => {
                self.tps = t.params.iter().map(|p| p.name.clone()).chain(["Self".to_string()]).collect();
                self.bounds(&mut t.params, span);
                for m in &mut t.methods {
                    let saved = self.tps.clone();
                    self.tps.extend(m.sig.tparams.iter().map(|p| p.name.clone()));
                    self.bounds(&mut m.sig.tparams, span);
                    self.fn_def(&mut m.sig, &mut vec![]);
                    self.tps = saved;
                }
            }
            Def::Impl(i) => {
                self.tps = i.tparams.iter().map(|p| p.name.clone()).chain(["Self".to_string()]).collect();
                self.bounds(&mut i.tparams, span);
                let before = i.trait_name.clone();
                self.qual(&mut i.trait_name, span, &[Kind::Trait]);
                let pkg = (i.trait_name != before).then(|| i.trait_name.split_once("__").map(|(p, _)| p.to_ascii_lowercase())).flatten();
                i.trait_args.iter_mut().for_each(|t| self.ty(t, span));
                self.ty(&mut i.target, span);
                for f in &mut i.fns {
                    if let Some(p) = &pkg {
                        f.name = mangle(p, &f.name);
                    }
                    let saved = self.tps.clone();
                    self.tps.extend(f.tparams.iter().map(|p| p.name.clone()));
                    self.fn_def(f, &mut vec![]);
                    self.tps = saved;
                }
                i.refresh_key();
            }
            Def::Test(t) => {
                self.tps.clear();
                self.expr(&mut t.body, &mut vec![]);
            }
            Def::Store(st) => {
                self.tps.clear();
                self.ty(&mut st.key, span);
                self.ty(&mut st.val, span);
            }
            Def::Svc(sv) => {
                self.tps.clear();
                for p in &mut sv.params {
                    self.ty(&mut p.ty, span);
                }
                for e in &mut sv.eps {
                    self.qual(&mut e.handler, e.span, &[Kind::Fn]);
                }
            }
            Def::Static(s) => {
                self.tps.clear();
                self.ty(&mut s.ty, span);
                self.expr(&mut s.init, &mut vec![]);
            }
            Def::Effect(e) => {
                self.tps = e.params.iter().map(|p| p.name.clone()).collect();
                for op in &mut e.ops {
                    op.params.iter_mut().for_each(|p| self.ty(&mut p.ty, span));
                    if let Some(r) = &mut op.ret {
                        self.ty(r, span);
                    }
                }
            }
            Def::Use(_) => {}
        }
    }

    fn fn_def(&mut self, f: &mut FnDef, l: &mut Locals) {
        let span = f.sig_span;
        for p in &mut f.params {
            self.ty(&mut p.ty, span);
            if let Some(r) = &mut p.refine {
                l.push(HashSet::from(["_".to_string()]));
                self.expr(r, l);
                l.pop();
            }
        }
        if let Some(t) = &mut f.ret {
            self.ty(t, span);
        }
        self.effects(&mut f.effects);
        let mut ps: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
        l.push(ps.clone());
        for p in &mut f.pres {
            self.expr(p, l);
        }
        l.pop();
        l.push(HashSet::new());
        for p in &mut f.examples {
            self.expr(p, l);
        }
        l.pop();
        ps.insert("r".into());
        l.push(ps);
        for p in &mut f.posts {
            self.expr(p, l);
        }
        l.pop();
        l.push(f.params.iter().map(|p| p.name.clone()).collect());
        self.expr(&mut f.body, l);
        l.pop();
    }

    fn bounds(&mut self, ps: &mut [TParam], span: Span) {
        for p in ps {
            for b in &mut p.bounds {
                if let Ty::Named { name, args, span: s } = b {
                    let sp = if *s == Span::default() { span } else { *s };
                    self.qual(name, sp, &[Kind::Trait]);
                    args.iter_mut().for_each(|a| self.ty(a, span));
                }
            }
        }
    }

    fn effects(&mut self, es: &mut [Effect]) {
        for e in es {
            self.qual(&mut e.name, e.span, &[Kind::Effect]);
            let sp = e.span;
            e.args.iter_mut().for_each(|t| self.ty(t, sp));
        }
    }

    fn fields(&mut self, fs: &mut [Field]) {
        for f in fs {
            self.ty(&mut f.ty, Span::default());
            if let Some(r) = &mut f.refine {
                self.expr(r, &mut vec![HashSet::from(["_".to_string()])]);
            }
        }
    }

    fn ty(&mut self, t: &mut Ty, outer: Span) {
        match t {
            Ty::Named { name, args, span } => {
                if !self.tps.contains(name) {
                    let sp = if *span == Span::default() { outer } else { *span };
                    self.qual(name, sp, &[Kind::Type]);
                }
                args.iter_mut().for_each(|a| self.ty(a, outer));
            }
            Ty::Tuple(xs) => xs.iter_mut().for_each(|a| self.ty(a, outer)),
            Ty::Fn { params, ret, effects } => {
                params.iter_mut().for_each(|a| self.ty(a, outer));
                self.ty(ret, outer);
                self.effects(effects);
            }
        }
    }

    fn pat(&mut self, p: &mut Pat, binds: &mut HashSet<String>, span: Span) {
        match p {
            Pat::Bind(n) => {
                binds.insert(n.clone());
            }
            Pat::Tuple(xs) | Pat::Or(xs) => xs.iter_mut().for_each(|x| self.pat(x, binds, span)),
            Pat::List { head, rest, tail } => {
                if let Some(Some(r)) = rest {
                    binds.insert(r.clone());
                }
                head.iter_mut().chain(tail.iter_mut()).for_each(|x| self.pat(x, binds, span));
            }
            Pat::Ctor { name, args } => {
                self.qual(name, span, &[Kind::Ctor, Kind::Type, Kind::Op, Kind::Effect]);
                match args {
                    CtorArgs::Positional(xs) => xs.iter_mut().for_each(|x| self.pat(x, binds, span)),
                    CtorArgs::Record(fs) => fs.iter_mut().for_each(|(_, x)| self.pat(x, binds, span)),
                    CtorArgs::None => {}
                }
            }
            _ => {}
        }
    }

    fn pkg_of(&mut self, e: &Expr, l: &Locals) -> Option<String> {
        match &e.kind {
            ExprKind::Name(p) if self.s.pkgs.contains_key(p) && !bound(l, p) => Some(p.clone()),
            ExprKind::Name(p) if self.s.indirect.contains(p) && !bound(l, p) => {
                self.errs.push(SyntaxError::new("E_PKG_UNKNOWN", format!("no dependency '{p}': it is only a dependency of a dependency; add it with 'sspur add' to use it here"), e.span));
                None
            }
            _ => None,
        }
    }

    fn imported_fn(&self, n: &str, l: &Locals) -> Option<Item> {
        self.s.imported.get(n).filter(|it| matches!(it.kind, Kind::Fn | Kind::Op) && !bound(l, n)).cloned()
    }

    fn expr(&mut self, e: &mut Expr, l: &mut Locals) {
        let span = e.span;
        match &mut e.kind {
            ExprKind::Name(n) => {
                if !bound(l, n) {
                    self.qual(n, span, &[Kind::Fn, Kind::Ctor, Kind::Type, Kind::Op, Kind::Effect]);
                }
            }
            ExprKind::Field(x, f) => {
                if let Some(p) = self.pkg_of(x, l) {
                    match self.s.lookup(&p, f, span) {
                        Ok(it) if it.kind == Kind::Fn && it.arity == 0 => e.kind = ExprKind::Call(Box::new(Expr::new(ExprKind::Name(it.mangled.clone()), span)), vec![]),
                        Ok(it) => e.kind = ExprKind::Name(it.mangled.clone()),
                        Err(err) => self.errs.push(err),
                    }
                    return;
                }
                self.expr(x, l);
                if let Some(it) = self.imported_fn(f, l).filter(|it| it.arity == 1) {
                    let recv = std::mem::replace(&mut **x, Expr::new(ExprKind::Unit, span));
                    e.kind = ExprKind::Call(Box::new(Expr::new(ExprKind::Name(it.mangled), span)), vec![recv]);
                }
            }
            ExprKind::Method { recv, name, targs, args } => {
                targs.iter_mut().for_each(|t| self.ty(t, span));
                args.iter_mut().for_each(|a| self.expr(a, l));
                if let Some(p) = self.pkg_of(recv, l) {
                    match self.s.lookup(&p, name, span) {
                        Ok(it) if matches!(it.kind, Kind::Fn | Kind::Op | Kind::Effect) => {
                            if !targs.is_empty() {
                                self.errs.push(SyntaxError::new("E_PKG_TARGS", format!("{p}.{name} takes no type arguments here; annotate the result instead"), span));
                            }
                            let f = Expr::new(ExprKind::Name(it.mangled.clone()), span);
                            e.kind = ExprKind::Call(Box::new(f), std::mem::take(args));
                        }
                        Ok(_) => self.errs.push(SyntaxError::new("E_PKG_KIND", format!("{p}.{name} is not a function"), span)),
                        Err(err) => self.errs.push(err),
                    }
                    return;
                }
                self.expr(recv, l);
                if let Some(it) = self.imported_fn(name, l) {
                    let recv = std::mem::replace(&mut **recv, Expr::new(ExprKind::Unit, span));
                    let mut all = vec![recv];
                    all.append(args);
                    e.kind = ExprKind::Call(Box::new(Expr::new(ExprKind::Name(it.mangled), span)), all);
                }
            }
            ExprKind::Lambda { params, body, .. } => {
                l.push(params.iter().cloned().collect());
                self.expr(body, l);
                l.pop();
            }
            ExprKind::Record { ctor, fields } => {
                if let Some(c) = ctor {
                    self.qual(c, span, &[Kind::Ctor, Kind::Type]);
                }
                fields.iter_mut().for_each(|(_, x)| self.expr(x, l));
            }
            ExprKind::Table(rows) => {
                for r in rows {
                    let mut binds = HashSet::new();
                    for c in &mut r.cells {
                        match c {
                            Cell::Pat(p) => self.pat(p, &mut binds, span),
                            Cell::Cond(x) => self.expr(x, l),
                            Cell::Any => {}
                        }
                    }
                    l.push(binds);
                    self.expr(&mut r.out, l);
                    l.pop();
                }
            }
            ExprKind::Match(s, arms, _) | ExprKind::Catch(s, arms) | ExprKind::Handle(s, arms) => {
                self.expr(s, l);
                for a in arms {
                    let mut binds = HashSet::new();
                    self.pat(&mut a.pat, &mut binds, a.body.span);
                    l.push(binds);
                    if let Some(g) = &mut a.guard {
                        self.expr(g, l);
                    }
                    self.expr(&mut a.body, l);
                    l.pop();
                }
            }
            ExprKind::Block(stmts) => {
                l.push(stmts.iter().filter_map(|s| if let Stmt::Fn(f) = s { Some(f.name.clone()) } else { None }).collect());
                for s in stmts {
                    match s {
                        Stmt::Expr(x) => self.expr(x, l),
                        Stmt::Let(p, x) => {
                            self.expr(x, l);
                            let mut binds = HashSet::new();
                            let sp = x.span;
                            self.pat(p, &mut binds, sp);
                            l.last_mut().unwrap().extend(binds);
                        }
                        Stmt::Var(n, x) => {
                            self.expr(x, l);
                            l.last_mut().unwrap().insert(n.clone());
                        }
                        Stmt::Assign(_, x, _) => self.expr(x, l),
                        Stmt::While(c, body) => {
                            self.expr(c, l);
                            self.expr(body, l);
                        }
                        Stmt::Fn(f) => {
                            let saved = std::mem::take(&mut self.tps);
                            self.tps = saved.iter().cloned().chain(f.tparams.iter().map(|p| p.name.clone())).collect();
                            self.fn_def(f, l);
                            self.tps = saved;
                        }
                        Stmt::For(p, it, body) => {
                            self.expr(it, l);
                            let mut binds = HashSet::new();
                            let sp = it.span;
                            self.pat(p, &mut binds, sp);
                            l.push(binds);
                            self.expr(body, l);
                            l.pop();
                        }
                    }
                }
                l.pop();
            }
            _ => {
                for c in crate::rename::children_mut(e) {
                    self.expr(c, l);
                }
            }
        }
    }
}
