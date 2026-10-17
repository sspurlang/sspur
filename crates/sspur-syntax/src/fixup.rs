//! Rewrites of foreign spellings into the canonical form, located by span. The checker
//! finds them (it knows the types); `sspur edit` applies them and stores the result.

use crate::ast::*;
use crate::visit::walk_expr_mut;

#[derive(Clone, Debug, PartialEq)]
pub enum Fix {
    /// `x.length` -> `x.len`: the method or field at `span` gets the name `to`.
    Method { span: Span, to: String },
    /// `len(x)` -> `x.len`: the call at `span` becomes a method call on its first argument.
    CallToMethod { span: Span, to: String },
    /// `s.slice(a, b)` on a Str -> `s.drop(a).take(b - a)`.
    StrSlice { span: Span },
    /// `n.get` where n is not an Opt -> `n`.
    DropMethod { span: Span },
    /// `None` -> `none`, `True` -> `true`, ...: the name at `span`.
    Name { span: Span, to: String },
    /// `| None =>` -> `| none =>`, in every pattern of the definition.
    PatCtor { from: String, to: String },
    /// `f(g(_))` where g takes no function: the implicit lambda at `span` moves out to
    /// the nearest enclosing call argument, as an explicit lambda.
    Lift { span: Span },
    /// `catch f(x) | E => b` in a test, where f(x) is not a Bool: `catch do` + `_ = f(x)` +
    /// `false`, so the test fails when nothing is raised.
    CatchTest { span: Span },
}

fn def_exprs(d: &mut Def) -> Vec<&mut Expr> {
    match d {
        Def::Fn(f) => f.pres.iter_mut().chain(f.posts.iter_mut()).chain(f.examples.iter_mut()).chain(std::iter::once(&mut f.body)).collect(),
        Def::Test(t) => vec![&mut t.body],
        Def::Static(s) => vec![&mut s.init],
        _ => vec![],
    }
}

/// Applies `fix` to `d`; false if nothing in `d` matched.
pub fn apply(d: &mut Def, fix: &Fix) -> bool {
    let mut done = false;
    match fix {
        Fix::PatCtor { from, to } => {
            for e in def_exprs(d) {
                walk_expr_mut(e, &mut |x| match &mut x.kind {
                    ExprKind::Match(_, arms, _) | ExprKind::Catch(_, arms) => arms.iter_mut().for_each(|a| done |= rename_pat(&mut a.pat, from, to)),
                    ExprKind::Block(stmts) => {
                        for s in stmts {
                            if let Stmt::Let(p, _) | Stmt::For(p, _, _) = s {
                                done |= rename_pat(p, from, to);
                            }
                        }
                    }
                    _ => {}
                });
            }
        }
        Fix::Lift { span } => {
            let taken = taken_names(d);
            let x = ["x", "it", "v", "e", "a"].iter().map(|s| s.to_string()).chain((1..).map(|i| format!("x{i}"))).find(|n| !taken.contains(n)).unwrap();
            for e in def_exprs(d) {
                if !done {
                    done = lift(e, *span, &x) == Lift::Done;
                }
            }
        }
        _ => {
            for e in def_exprs(d) {
                walk_expr_mut(e, &mut |x| {
                    if !done && at(x, fix) {
                        done = rewrite(x, fix);
                    }
                });
            }
        }
    }
    done
}

fn at(x: &Expr, fix: &Fix) -> bool {
    match fix {
        Fix::Method { span, .. } | Fix::CallToMethod { span, .. } | Fix::StrSlice { span } | Fix::DropMethod { span } | Fix::Name { span, .. } | Fix::CatchTest { span } => x.span == *span,
        _ => false,
    }
}

fn rewrite(x: &mut Expr, fix: &Fix) -> bool {
    let kind = std::mem::replace(&mut x.kind, ExprKind::Unit);
    let (kind, ok) = match (kind, fix) {
        (ExprKind::Method { recv, targs, args, .. }, Fix::Method { to, .. }) => (ExprKind::Method { recv, name: to.clone(), targs, args }, true),
        (ExprKind::Field(recv, _), Fix::Method { to, .. }) => (ExprKind::Field(recv, to.clone()), true),
        (ExprKind::Call(_, mut args), Fix::CallToMethod { to, .. }) if !args.is_empty() => {
            let mut recv = args.remove(0);
            if let ExprKind::Lambda { body, implicit: true, .. } = recv.kind {
                recv = *body;
            }
            if args.is_empty() {
                (ExprKind::Field(Box::new(recv), to.clone()), true)
            } else {
                (ExprKind::Method { recv: Box::new(recv), name: to.clone(), targs: vec![], args }, true)
            }
        }
        (ExprKind::Method { recv, mut args, .. }, Fix::StrSlice { .. }) if (1..=2).contains(&args.len()) => {
            let sp = x.span;
            let m = |recv: Expr, name: &str, arg: Expr| Expr::new(ExprKind::Method { recv: Box::new(recv), name: name.into(), targs: vec![], args: vec![arg] }, sp);
            let b = (args.len() == 2).then(|| args.pop().unwrap());
            let a = args.pop().unwrap();
            let out = match b {
                None => m(*recv, "drop", a),
                Some(b) if a.kind == ExprKind::Int(0) => m(*recv, "take", b),
                Some(b) => {
                    let len = match (&a.kind, &b.kind) {
                        (ExprKind::Int(i), ExprKind::Int(j)) => Expr::new(ExprKind::Int(j - i), b.span),
                        _ => Expr::new(ExprKind::Binary(BinOp::Sub, Box::new(b), Box::new(a.clone())), sp),
                    };
                    m(m(*recv, "drop", a), "take", len)
                }
            };
            (out.kind, true)
        }
        (ExprKind::Method { recv, args, .. }, Fix::DropMethod { .. }) if args.is_empty() => (recv.kind, true),
        (ExprKind::Field(recv, _), Fix::DropMethod { .. }) => (recv.kind, true),
        (ExprKind::Catch(body, arms), Fix::CatchTest { .. }) => {
            let sp = body.span;
            // The test expects an error when an arm says so; when every arm is `false`, it expects
            // `e` to finish without one (`catch require(cfg) | Missing{..} => false`).
            let expects_success = !arms.is_empty() && arms.iter().all(|a| matches!(a.body.kind, ExprKind::Bool(false)));
            let block = Expr::new(ExprKind::Block(vec![Stmt::Let(Pat::Wild, *body), Stmt::Expr(Expr::new(ExprKind::Bool(expects_success), sp))]), sp);
            (ExprKind::Catch(Box::new(block), arms), true)
        }
        (ExprKind::Name(_), Fix::Name { to, .. }) => (
            match to.as_str() {
                "true" => ExprKind::Bool(true),
                "false" => ExprKind::Bool(false),
                n => ExprKind::Name(n.into()),
            },
            true,
        ),
        (k, _) => (k, false),
    };
    x.kind = kind;
    ok
}

fn rename_pat(p: &mut Pat, from: &str, to: &str) -> bool {
    match p {
        Pat::Ctor { name, args } => {
            let mut hit = false;
            if name == from {
                match to {
                    "true" | "false" => {
                        *p = Pat::Bool(to == "true");
                        return true;
                    }
                    _ => *name = to.to_string(),
                }
                hit = true;
            }
            match args {
                CtorArgs::Positional(xs) => xs.iter_mut().for_each(|x| hit |= rename_pat(x, from, to)),
                CtorArgs::Record(fs) => fs.iter_mut().for_each(|(_, x)| hit |= rename_pat(x, from, to)),
                CtorArgs::None => {}
            }
            hit
        }
        Pat::Tuple(xs) | Pat::Or(xs) => xs.iter_mut().fold(false, |h, x| rename_pat(x, from, to) | h),
        Pat::List { head, tail, .. } => head.iter_mut().chain(tail.iter_mut()).fold(false, |h, x| rename_pat(x, from, to) | h),
        _ => false,
    }
}

fn taken_names(d: &mut Def) -> Vec<String> {
    let mut out = Vec::new();
    for e in def_exprs(d) {
        walk_expr_mut(e, &mut |x| match &x.kind {
            ExprKind::Name(n) => out.push(n.clone()),
            ExprKind::Lambda { params, .. } => out.extend(params.iter().cloned()),
            ExprKind::Block(stmts) => {
                for s in stmts {
                    match s {
                        Stmt::Var(n, _) | Stmt::Assign(n, _, _) => out.push(n.clone()),
                        Stmt::Let(p, _) | Stmt::For(p, _, _) => pat_names(p, &mut out),
                        _ => {}
                    }
                }
            }
            ExprKind::Match(_, arms, _) | ExprKind::Catch(_, arms) | ExprKind::Handle(_, arms) => arms.iter().for_each(|a| pat_names(&a.pat, &mut out)),
            _ => {}
        });
    }
    if let Def::Fn(f) = d {
        out.extend(f.params.iter().map(|p| p.name.clone()));
    }
    out
}

fn pat_names(p: &Pat, out: &mut Vec<String>) {
    p.binds(out)
}

#[derive(PartialEq)]
enum Lift {
    No,
    Found,
    Done,
}

fn is_target(e: &Expr, span: Span) -> bool {
    e.span == span && matches!(e.kind, ExprKind::Lambda { implicit: true, .. })
}

/// Finds the implicit lambda at `span`; the nearest call argument strictly around it
/// becomes `x => ...` with that lambda's `_` (and the argument's own) as `x`.
fn lift(e: &mut Expr, span: Span, x: &str) -> Lift {
    if is_target(e, span) {
        return Lift::Found;
    }
    let first_arg = match e.kind {
        ExprKind::Call(..) | ExprKind::Method { .. } => 1,
        ExprKind::List(_) => 0,
        _ => usize::MAX,
    };
    let mut kids: Vec<&mut Expr> = Vec::new();
    collect_children(e, &mut kids);
    let mut found_here = false;
    for (i, k) in kids.into_iter().enumerate() {
        match lift(k, span, x) {
            Lift::Done => return Lift::Done,
            Lift::Found if i >= first_arg && !is_target(k, span) => {
                wrap_arg(k, span, x);
                return Lift::Done;
            }
            Lift::Found => found_here = true,
            Lift::No => {}
        }
    }
    if found_here { Lift::Found } else { Lift::No }
}

fn collect_children<'a>(e: &'a mut Expr, out: &mut Vec<&'a mut Expr>) {
    match &mut e.kind {
        ExprKind::Call(g, args) => {
            out.push(g);
            out.extend(args.iter_mut());
        }
        ExprKind::Method { recv, args, .. } => {
            out.push(recv);
            out.extend(args.iter_mut());
        }
        ExprKind::Str(parts) => out.extend(parts.iter_mut().filter_map(|p| if let StrPart::Expr(x) = p { Some(x) } else { None })),
        ExprKind::Field(x, _) | ExprKind::Unary(_, x) | ExprKind::Raise(x) | ExprKind::Return(x) => out.push(x),
        ExprKind::Lambda { body, .. } => out.push(body),
        ExprKind::Index(a, b) | ExprKind::Binary(_, a, b) | ExprKind::Range(a, b) => {
            out.push(a);
            out.push(b);
        }
        ExprKind::If(c, t, f) => {
            out.push(c);
            out.push(t);
            if let Some(f) = f {
                out.push(f);
            }
        }
        ExprKind::Match(s, arms, _) | ExprKind::Catch(s, arms) | ExprKind::Handle(s, arms) => {
            out.push(s);
            for a in arms {
                if let Some(g) = &mut a.guard {
                    out.push(g);
                }
                out.push(&mut a.body);
            }
        }
        ExprKind::Block(stmts) => {
            for s in stmts {
                match s {
                    Stmt::Let(_, v) | Stmt::Var(_, v) | Stmt::Assign(_, v, _) | Stmt::Expr(v) => out.push(v),
                    Stmt::For(_, a, b) | Stmt::While(a, b) => {
                        out.push(a);
                        out.push(b);
                    }
                    Stmt::Fn(f) => out.push(&mut f.body),
                }
            }
        }
        ExprKind::Record { fields, .. } => out.extend(fields.iter_mut().map(|(_, v)| v)),
        ExprKind::List(xs) | ExprKind::Tuple(xs) | ExprKind::Par(xs) => out.extend(xs.iter_mut()),
        ExprKind::With(b, ups) => {
            out.push(b);
            out.extend(ups.iter_mut().map(|(_, v)| v));
        }
        _ => {}
    }
}

fn wrap_arg(arg: &mut Expr, span: Span, x: &str) {
    walk_expr_mut(arg, &mut |e| {
        if is_target(e, span)
            && let ExprKind::Lambda { body, .. } = std::mem::replace(&mut e.kind, ExprKind::Unit) {
                *e = *body;
            }
    });
    let sp = arg.span;
    let body = match std::mem::replace(&mut arg.kind, ExprKind::Unit) {
        ExprKind::Lambda { body, implicit: true, .. } => *body,
        k => Expr::new(k, sp),
    };
    let mut body = body;
    name_placeholders(&mut body, x);
    arg.kind = ExprKind::Lambda { params: vec![x.to_string()], body: Box::new(body), implicit: false, tys: vec![] };
}

/// Replaces the `_` that belong to this level (not those under another implicit lambda).
fn name_placeholders(e: &mut Expr, x: &str) {
    if matches!(e.kind, ExprKind::Placeholder) {
        e.kind = ExprKind::Name(x.to_string());
        return;
    }
    if matches!(e.kind, ExprKind::Lambda { implicit: true, .. }) {
        return;
    }
    let mut kids = Vec::new();
    collect_children(e, &mut kids);
    for k in kids {
        name_placeholders(k, x);
    }
}
