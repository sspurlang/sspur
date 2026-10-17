//! Last uses of local variables that receive an appending method (`xs.push(x)`, `b.add(s)`,
//! `m.put(k, v)`), so the interpreter can hand the method the variable's only reference and
//! append in place instead of copying. Names are compared as text, which is conservative under
//! shadowing: a use of any variable with the same name keeps the receiver live.

use sspur_syntax::visit::children;
use sspur_syntax::{Expr, ExprKind, FnDef, Pat, Stmt};
use std::collections::{HashMap, HashSet};

pub(crate) const MOVE_METHODS: &[&str] = &["push", "concat", "add", "put"];

/// A set of a function's candidate variables, one bit each.
type Live = u64;

#[derive(Default)]
pub(crate) struct LastUse {
    marks: HashMap<(u32, u32), bool>,
}

fn binds(p: &Pat) -> Vec<String> {
    let mut out = vec![];
    p.binds(&mut out);
    out
}

/// The statements' expressions in order, with the local functions' bodies when `fns`.
fn stmt_exprs(s: &Stmt, fns: bool) -> Vec<&Expr> {
    match s {
        Stmt::Fn(f) if fns => f.pres.iter().chain(&f.posts).chain(std::iter::once(&f.body)).collect(),
        Stmt::Fn(_) => vec![],
        Stmt::Let(_, x) | Stmt::Var(_, x) | Stmt::Assign(_, x, _) | Stmt::Expr(x) => vec![x],
        Stmt::For(_, a, b) | Stmt::While(a, b) => vec![a, b],
    }
}

/// Receivers of moving methods and names bound in `e`, outside nested closures.
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
                    _ => {}
                }
                stmt_exprs(s, false).into_iter().for_each(|x| scan(x, recv, bound));
            }
            return;
        }
        _ => {}
    }
    for c in children(e) {
        scan(c, recv, bound);
    }
}

/// Nested lambdas and local functions, each analyzed on its own: parameters, body, contracts.
fn nested<'a>(e: &'a Expr, out: &mut Vec<(Vec<String>, &'a Expr, bool)>) {
    match &e.kind {
        ExprKind::Lambda { params, body, .. } => return out.push((params.clone(), body, false)),
        ExprKind::Block(stmts) => {
            for s in stmts {
                if let Stmt::Fn(f) = s {
                    out.push((f.params.iter().map(|p| p.name.clone()).collect(), &f.body, !f.posts.is_empty()));
                }
                stmt_exprs(s, false).into_iter().for_each(|x| nested(x, out));
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
    cand: Vec<String>,
    marks: &'a mut HashMap<(u32, u32), bool>,
}

impl Fx<'_> {
    fn bit(&self, n: &str) -> Live {
        self.cand.iter().position(|c| c == n).map_or(0, |i| 1 << i)
    }

    fn bits(&self, names: &[String]) -> Live {
        names.iter().fold(0, |m, n| m | self.bit(n))
    }

    /// Every candidate named in `e`; with `inside`, only in the closures nested in it.
    fn reads(&self, e: &Expr, inside: bool) -> Live {
        let mut m = match &e.kind {
            ExprKind::Name(n) if !inside => self.bit(n),
            ExprKind::Lambda { body, .. } if inside => return self.reads(body, false),
            ExprKind::Block(stmts) if inside => return stmts.iter().fold(0, |m, s| m | stmt_exprs(s, true).iter().fold(0, |m, x| m | self.reads(x, !matches!(s, Stmt::Fn(_))))),
            _ => 0,
        };
        for c in children(e) {
            m |= self.reads(c, inside);
        }
        m
    }

    /// The candidates live before `e`, given those live after it; marks the receivers it can move.
    fn expr(&mut self, e: &Expr, live: Live, blk: Live) -> Live {
        match &e.kind {
            ExprKind::Name(n) => live | self.bit(n),
            ExprKind::Lambda { .. } => live,
            ExprKind::Method { recv, name, args, .. } => {
                if let ExprKind::Name(n) = &recv.kind
                    && MOVE_METHODS.contains(&name.as_str())
                    && self.bit(n) != 0
                {
                    let k = (recv.span.start, recv.span.end);
                    if k.0 < k.1 {
                        let ok = (live | blk) & self.bit(n) == 0;
                        let m = self.marks.entry(k).or_insert(ok);
                        *m = *m && ok;
                    }
                }
                let l = args.iter().rev().fold(live, |l, a| self.expr(a, l, blk));
                self.expr(recv, l, blk)
            }
            ExprKind::If(c, t, f) => {
                let l = self.expr(t, live, blk) | f.as_ref().map_or(live, |f| self.expr(f, live, blk));
                self.expr(c, l, blk)
            }
            ExprKind::Match(s, arms, _) => {
                let mut u = 0;
                for a in arms {
                    let mut la = self.expr(&a.body, live, blk);
                    if let Some(g) = &a.guard {
                        la = self.expr(g, la, blk);
                    }
                    u |= la & !(self.bits(&binds(&a.pat)) & !live);
                }
                self.expr(s, u, blk)
            }
            ExprKind::Catch(s, arms) | ExprKind::Handle(s, arms) => {
                let u = arms.iter().fold(live, |m, a| m | self.reads(&a.body, false) | a.guard.as_ref().map_or(0, |g| self.reads(g, false)));
                let arm_blk = blk | self.reads(s, false);
                for a in arms {
                    let la = self.expr(&a.body, live, arm_blk);
                    if let Some(g) = &a.guard {
                        self.expr(g, la, arm_blk);
                    }
                }
                self.expr(s, u, blk | u) | u
            }
            ExprKind::Par(xs) => {
                let all = xs.iter().fold(live, |m, x| m | self.reads(x, false));
                for x in xs {
                    self.expr(x, all, blk | all);
                }
                all
            }
            ExprKind::Block(stmts) => self.block(stmts, live, blk),
            _ => children(e).into_iter().rev().fold(live, |l, c| self.expr(c, l, blk)),
        }
    }

    fn block(&mut self, stmts: &[Stmt], out: Live, blk: Live) -> Live {
        let mut l = out;
        for s in stmts.iter().rev() {
            l = match s {
                Stmt::Expr(e) => self.expr(e, l, blk),
                Stmt::Let(p, e) => {
                    let b = self.bits(&binds(p));
                    self.expr(e, l & !(b & !out), blk)
                }
                Stmt::Var(n, e) => {
                    let b = self.bit(n);
                    self.expr(e, l & !(b & !out), blk)
                }
                Stmt::Assign(n, e, _) => self.expr(e, l & !self.bit(n), blk),
                Stmt::For(_, it, body) => {
                    let lb = l | self.reads(body, false);
                    let inner = self.expr(body, lb, blk);
                    self.expr(it, l | inner, blk)
                }
                Stmt::While(c, body) => {
                    let lb = l | self.reads(body, false) | self.reads(c, false);
                    let inner = self.expr(body, lb, blk);
                    self.expr(c, l | inner | lb, blk)
                }
                Stmt::Fn(_) => l,
            };
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
        let cand: Vec<String> = recv.intersection(&bound).take(64).cloned().collect();
        if cand.is_empty() {
            return;
        }
        let mut fx = Fx { cand, marks: &mut self.marks };
        let blk = fx.reads(body, true);
        fx.expr(body, 0, blk);
    }

    pub(crate) fn def(&mut self, f: &FnDef) {
        let ps: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
        self.func(&ps, &f.body, !f.posts.is_empty());
    }

    pub(crate) fn finish(self) -> HashSet<(u32, u32)> {
        self.marks.into_iter().filter(|(_, ok)| *ok).map(|(k, _)| k).collect()
    }
}
