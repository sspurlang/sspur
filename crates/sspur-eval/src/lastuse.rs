//! Last uses of local variables that receive an appending method (`xs.push(x)`, `b.add(s)`,
//! `m.put(k, v)`), so the interpreter can hand the method the variable's only reference and
//! append in place instead of copying. Names are compared as text, which is conservative under
//! shadowing: a use of any variable with the same name keeps the receiver live.

use sspur_syntax::visit::children;
use sspur_syntax::{Expr, ExprKind, FnDef, Pat, Stmt};
use std::collections::{HashMap, HashSet};

pub(crate) const MOVE_METHODS: &[&str] = &["push", "concat", "add", "put", "remove"];

type Live = HashSet<String>;

#[derive(Default)]
pub(crate) struct LastUse {
    marks: HashMap<(u32, u32), bool>,
}

fn binds(p: &Pat) -> Vec<String> {
    let mut out = vec![];
    p.binds(&mut out);
    out
}

/// Every name inside lambdas and local functions nested in `e`.
fn captured(e: &Expr, out: &mut HashSet<String>, inside: bool) {
    match &e.kind {
        ExprKind::Name(n) if inside => {
            out.insert(n.clone());
        }
        ExprKind::Lambda { body, .. } => return captured(body, out, true),
        ExprKind::Block(stmts) => {
            for s in stmts {
                match s {
                    Stmt::Fn(f) => f.pres.iter().chain(&f.posts).chain(std::iter::once(&f.body)).for_each(|x| captured(x, out, true)),
                    Stmt::Let(_, x) | Stmt::Var(_, x) | Stmt::Assign(_, x, _) | Stmt::Expr(x) => captured(x, out, inside),
                    Stmt::For(_, a, b) | Stmt::While(a, b) => {
                        captured(a, out, inside);
                        captured(b, out, inside);
                    }
                }
            }
            return;
        }
        _ => {}
    }
    for c in children(e) {
        captured(c, out, inside);
    }
}

/// Receivers of moving methods outside nested closures, and names bound locally.
fn scan(e: &Expr, recv: &mut HashSet<String>, bound: &mut HashSet<String>) {
    match &e.kind {
        ExprKind::Lambda { .. } => return,
        ExprKind::Method { recv: r, name, .. } if MOVE_METHODS.contains(&name.as_str()) => {
            if let ExprKind::Name(n) = &r.kind {
                recv.insert(n.clone());
            }
        }
        ExprKind::Match(_, arms, _) | ExprKind::Catch(_, arms) | ExprKind::Handle(_, arms) => arms.iter().for_each(|a| bound.extend(binds(&a.pat))),
        ExprKind::Block(stmts) => {
            for s in stmts {
                match s {
                    Stmt::Let(p, _) | Stmt::For(p, _, _) => bound.extend(binds(p)),
                    Stmt::Var(n, _) => {
                        bound.insert(n.clone());
                    }
                    Stmt::Fn(_) => {}
                    _ => {}
                }
            }
            for s in stmts {
                match s {
                    Stmt::Fn(_) => {}
                    Stmt::Let(_, x) | Stmt::Var(_, x) | Stmt::Assign(_, x, _) | Stmt::Expr(x) => scan(x, recv, bound),
                    Stmt::For(_, a, b) | Stmt::While(a, b) => {
                        scan(a, recv, bound);
                        scan(b, recv, bound);
                    }
                }
            }
            return;
        }
        _ => {}
    }
    for c in children(e) {
        scan(c, recv, bound);
    }
}

/// Nested lambdas and local functions, each analyzed on its own.
fn nested<'a>(e: &'a Expr, out: &mut Vec<(Vec<String>, &'a Expr, bool)>) {
    match &e.kind {
        ExprKind::Lambda { params, body, .. } => {
            out.push((params.clone(), body, false));
            return;
        }
        ExprKind::Block(stmts) => {
            for s in stmts {
                match s {
                    Stmt::Fn(f) => out.push((f.params.iter().map(|p| p.name.clone()).collect(), &f.body, !f.posts.is_empty())),
                    Stmt::Let(_, x) | Stmt::Var(_, x) | Stmt::Assign(_, x, _) | Stmt::Expr(x) => nested(x, out),
                    Stmt::For(_, a, b) | Stmt::While(a, b) => {
                        nested(a, out);
                        nested(b, out);
                    }
                }
            }
            return;
        }
        _ => {}
    }
    for c in children(e) {
        nested(c, out);
    }
}

struct Fx<'a> {
    cand: HashSet<String>,
    marks: &'a mut HashMap<(u32, u32), bool>,
}

impl Fx<'_> {
    fn reads(&self, e: &Expr, out: &mut Live) {
        if let ExprKind::Name(n) = &e.kind
            && self.cand.contains(n)
        {
            out.insert(n.clone());
        }
        for c in children(e) {
            self.reads(c, out);
        }
    }

    fn reads_of(&self, e: &Expr) -> Live {
        let mut out = Live::new();
        self.reads(e, &mut out);
        out
    }

    fn mark(&mut self, e: &Expr, ok: bool) {
        let k = (e.span.start, e.span.end);
        if k.0 >= k.1 {
            return;
        }
        let m = self.marks.entry(k).or_insert(ok);
        *m = *m && ok;
    }

    /// The names live before `e`, given those live after it; marks the receivers it can move.
    fn expr(&mut self, e: &Expr, live: &Live, blk: &HashSet<String>) -> Live {
        match &e.kind {
            ExprKind::Name(n) => {
                let mut l = live.clone();
                if self.cand.contains(n) {
                    l.insert(n.clone());
                }
                l
            }
            ExprKind::Lambda { .. } => live.clone(),
            ExprKind::Method { recv, name, args, .. } => {
                if let ExprKind::Name(n) = &recv.kind
                    && MOVE_METHODS.contains(&name.as_str())
                    && self.cand.contains(n)
                {
                    let ok = !live.contains(n) && !blk.contains(n);
                    self.mark(recv, ok);
                }
                let mut l = live.clone();
                for a in args.iter().rev() {
                    l = self.expr(a, &l, blk);
                }
                self.expr(recv, &l, blk)
            }
            ExprKind::If(c, t, f) => {
                let mut l = self.expr(t, live, blk);
                l.extend(match f {
                    Some(f) => self.expr(f, live, blk),
                    None => live.clone(),
                });
                self.expr(c, &l, blk)
            }
            ExprKind::Match(s, arms, _) => {
                let mut u = Live::new();
                for a in arms {
                    let mut la = self.expr(&a.body, live, blk);
                    if let Some(g) = &a.guard {
                        la = self.expr(g, &la, blk);
                    }
                    for n in binds(&a.pat) {
                        if !live.contains(&n) {
                            la.remove(&n);
                        }
                    }
                    u.extend(la);
                }
                self.expr(s, &u, blk)
            }
            ExprKind::Catch(s, arms) | ExprKind::Handle(s, arms) => {
                let mut u = live.clone();
                for a in arms {
                    self.reads(&a.body, &mut u);
                    if let Some(g) = &a.guard {
                        self.reads(g, &mut u);
                    }
                }
                let mut b2 = blk.clone();
                b2.extend(u.iter().cloned());
                let mut arm_blk = blk.clone();
                arm_blk.extend(self.reads_of(s));
                for a in arms {
                    let la = self.expr(&a.body, live, &arm_blk);
                    if let Some(g) = &a.guard {
                        self.expr(g, &la, &arm_blk);
                    }
                }
                let mut l = self.expr(s, &u, &b2);
                l.extend(u);
                l
            }
            ExprKind::Par(xs) => {
                let mut all = live.clone();
                for x in xs {
                    self.reads(x, &mut all);
                }
                let mut b2 = blk.clone();
                b2.extend(all.iter().cloned());
                for x in xs {
                    self.expr(x, &all, &b2);
                }
                all
            }
            ExprKind::Block(stmts) => self.block(stmts, live, blk),
            _ => {
                let mut l = live.clone();
                for c in children(e).into_iter().rev() {
                    l = self.expr(c, &l, blk);
                }
                l
            }
        }
    }

    fn block(&mut self, stmts: &[Stmt], out: &Live, blk: &HashSet<String>) -> Live {
        let mut l = out.clone();
        let restore = |l: &mut Live, names: Vec<String>| {
            for n in names {
                if !out.contains(&n) {
                    l.remove(&n);
                }
            }
        };
        for s in stmts.iter().rev() {
            match s {
                Stmt::Expr(e) => l = self.expr(e, &l, blk),
                Stmt::Let(p, e) => {
                    restore(&mut l, binds(p));
                    l = self.expr(e, &l, blk);
                }
                Stmt::Var(n, e) => {
                    restore(&mut l, vec![n.clone()]);
                    l = self.expr(e, &l, blk);
                }
                Stmt::Assign(n, e, _) => {
                    l.remove(n);
                    l = self.expr(e, &l, blk);
                }
                Stmt::For(_, it, body) => {
                    let mut lb = l.clone();
                    self.reads(body, &mut lb);
                    let inner = self.expr(body, &lb, blk);
                    l.extend(inner);
                    l = self.expr(it, &l, blk);
                }
                Stmt::While(c, body) => {
                    let mut lb = l.clone();
                    self.reads(body, &mut lb);
                    self.reads(c, &mut lb);
                    let inner = self.expr(body, &lb, blk);
                    l.extend(inner);
                    l.extend(lb);
                    l = self.expr(c, &l, blk);
                }
                Stmt::Fn(_) => {}
            }
        }
        l
    }
}

impl LastUse {
    pub(crate) fn func(&mut self, params: &[String], body: &Expr, contracts: bool) {
        let mut subs = vec![];
        nested(body, &mut subs);
        for (ps, b, c) in subs {
            self.func(&ps, b, c);
        }
        if contracts {
            return;
        }
        let (mut recv, mut bound) = (HashSet::new(), params.iter().cloned().collect::<HashSet<_>>());
        scan(body, &mut recv, &mut bound);
        let cand: HashSet<String> = recv.intersection(&bound).cloned().collect();
        if cand.is_empty() {
            return;
        }
        let mut blk = HashSet::new();
        captured(body, &mut blk, false);
        let mut fx = Fx { cand, marks: &mut self.marks };
        fx.expr(body, &Live::new(), &blk);
    }

    pub(crate) fn def(&mut self, f: &FnDef) {
        let ps: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
        self.func(&ps, &f.body, !f.posts.is_empty());
    }

    pub(crate) fn finish(self) -> HashSet<(u32, u32)> {
        self.marks.into_iter().filter(|(_, ok)| *ok).map(|(k, _)| k).collect()
    }
}
