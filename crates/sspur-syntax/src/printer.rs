use crate::ast::*;
use crate::layout::{self, mark};

pub fn print_module(m: &Module) -> String {
    let mut out = String::new();
    if let Some(p) = &m.profile {
        out.push_str(&format!("profile {p}\n\n"));
    }
    let defs: Vec<String> = m.defs.iter().map(print_def).collect();
    out.push_str(&defs.join("\n\n"));
    out.push('\n');
    out
}

pub fn print_def(d: &Def) -> String {
    if d.is_pub() {
        return format!("pub {}", print_def_inner(d));
    }
    print_def_inner(d)
}

fn print_def_inner(d: &Def) -> String {
    match d {
        Def::Type(t) => print_type_def(t),
        Def::Fn(f) => print_fn(f),
        Def::Test(t) => format!("test {} = {}", t.name, expr(&t.body, 0)),
        Def::Effect(e) => print_effect_def(e),
        Def::Store(st) => format!("store {} = {}[{}, {}]", st.name, st.kind, ty(&st.key), ty(&st.val)),
        Def::Svc(sv) => {
            let mut s = format!("svc {}", sv.name);
            #[cfg(not(target_family = "wasm"))]
            if !sv.params.is_empty() {
                let ps: Vec<String> = sv.params.iter().map(|p| format!("{}: {}", p.name, ty(&p.ty))).collect();
                s.push_str(&format!("({})", ps.join(", ")));
            }
            for ep in &sv.eps {
                s.push_str(&format!("\n  ep {} {:?} = {}", ep.method, ep.path, ep.handler));
            }
            s
        }
        Def::Static(s) => format!("static {}: {} = {}", s.name, ty(&s.ty), expr(&s.init, 0)),
        Def::Use(u) if u.names.is_empty() => format!("use {}", u.pkg),
        Def::Use(u) => format!("use {}.{{{}}}", u.pkg, layout::group(&u.names)),
        Def::Trait(t) => {
            let mut s = format!("trait {}{}", t.name, tparams(&t.params));
            for m in &t.methods {
                s.push_str("\n  ");
                if m.default {
                    s.push_str(&print_fn_at(&m.sig, 2));
                } else {
                    let mut f = m.sig.clone();
                    f.body = Expr::new(ExprKind::Unit, Span::default());
                    let full = print_fn_at(&f, 2);
                    s.push_str(full.rsplit_once("\n  = ").map_or(full.as_str(), |(h, _)| h));
                }
            }
            s
        }
        Def::Impl(i) => {
            let args = if i.trait_args.is_empty() { String::new() } else { format!("[{}]", i.trait_args.iter().map(ty).collect::<Vec<_>>().join(", ")) };
            let mut s = format!("impl{} {}{} for {}", tparams(&i.tparams), i.trait_name, args, ty(&i.target));
            for f in &i.fns {
                s.push_str("\n  ");
                s.push_str(&print_fn_at(f, 2));
            }
            s
        }
    }
}

fn op_sig(op: &OpSig) -> String {
    let params: Vec<String> = op.params.iter().map(|p| format!("{}: {}", p.name, ty(&p.ty))).collect();
    let mut s = format!("({})", params.join(", "));
    if let Some(r) = &op.ret {
        s.push_str(&format!(" -> {}", ty(r)));
    }
    s
}

fn print_effect_def(e: &EffectDef) -> String {
    let head = format!("effect {}{}", e.name, tparams(&e.params));
    if let [op] = e.ops.as_slice()
        && op.name == e.name {
            return format!("{head}{}", op_sig(op));
        }
    let mut s = head;
    for op in &e.ops {
        s.push_str(&format!("\n  {}{}", op.name, op_sig(op)));
    }
    s
}

fn tparams(ps: &[TParam]) -> String {
    if ps.is_empty() {
        return String::new();
    }
    let items: Vec<String> = ps
        .iter()
        .map(|p| {
            let mut s = p.name.clone();
            if let Some(k) = &p.kind {
                s.push_str(&format!(": {}", ty(k)));
            }
            if !p.bounds.is_empty() {
                s.push_str(&format!(": {}", p.bounds.iter().map(ty).collect::<Vec<_>>().join(" + ")));
            }
            if let Some(r) = &p.refine {
                s.push_str(&format!(" where {}", expr(r, 0)));
            }
            s
        })
        .collect();
    format!("[{}]", items.join(", "))
}

fn fields(fs: &[Field]) -> String {
    let items: Vec<String> = fs
        .iter()
        .map(|f| match &f.refine {
            Some(r) => format!("{}: {} where {}", f.name, ty(&f.ty), expr(r, 0)),
            None => format!("{}: {}", f.name, ty(&f.ty)),
        })
        .collect();
    format!("{{{}}}", layout::group(&items))
}

fn print_type_def(t: &TypeDef) -> String {
    let body = match &t.body {
        TypeBody::Record(fs) => fields(fs),
        TypeBody::Sum(vs) => {
            let items: Vec<String> = vs
                .iter()
                .map(|v| match &v.fields {
                    Some(fs) => format!("{}{}", v.name, fields(fs)),
                    None => v.name.clone(),
                })
                .collect();
            // Broken, each variant goes on its own line after `|`.
            let m = if items.len() > 1 { mark(layout::SUM) } else { "" };
            format!("{m}{}", items.join(&format!(" {m}| ")))
        }
        TypeBody::Alias(t, None) => ty(t),
        TypeBody::Alias(t, Some(r)) => format!("{} where {}", ty(t), expr(r, 0)),
        TypeBody::New(t) => format!("new {}", ty(t)),
    };
    let res = if t.res { "res " } else { "" };
    let mut s = format!("{res}type {}{} = {}", t.name, tparams(&t.params), body);
    if !t.derives.is_empty() {
        s.push_str(&format!(" derive {}", t.derives.join(", ")));
    }
    if let Some(d) = &t.drop {
        s.push_str(&format!(" drop {d}"));
    }
    s
}

pub fn print_sig(f: &FnDef) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| match &p.refine {
            Some(r) => format!("{}: {} where {}", p.name, ty(&p.ty), expr(r, 0)),
            None => cat(&[&p.name, ": ", &ty(&p.ty)]),
        })
        .collect();
    let kw = if f.ext.is_some() {
        "extern fn"
    } else if f.kernel.is_some() {
        "kernel fn"
    } else {
        "fn"
    };
    let mut s = cat(&[kw, " ", &f.name, &tparams(&f.tparams), "(", &layout::group(&params), ")"]);
    if let Some(r) = &f.ret {
        s = ext(s, &[" -> ", &ty(r)]);
    }
    if !f.effects.is_empty() {
        s = ext(s, &[" ! ", &effects(&f.effects)]);
    }
    if let Some(k) = &f.kernel {
        match &k.y {
            Some((h, gh)) => s.push_str(&format!(" @grid2({}, {}, {}, {})", expr(&k.grid, 0), expr(h, 0), expr(&k.group, 0), expr(gh, 0))),
            None => s.push_str(&format!(" @grid({}, {})", expr(&k.grid, 0), expr(&k.group, 0))),
        }
    }
    if let Some(x) = &f.ext {
        if let Some(l) = &x.lib {
            s.push_str(&format!(" from {l:?}"));
        }
        if x.symbol != f.name {
            s.push_str(&format!(" as {:?}", x.symbol));
        }
    }
    s
}

fn print_fn(f: &FnDef) -> String {
    print_fn_at(f, 0)
}

fn print_fn_at(f: &FnDef, ind: usize) -> String {
    let mut s = print_sig(f);
    if f.ext.is_some() {
        return s;
    }
    let c = pad(ind + 2);
    if let Some(r) = &f.trusted {
        s.push_str(&format!("\n{c}unsafe {}", expr(&Expr::new(ExprKind::Str(vec![StrPart::Lit(r.clone())]), Span::default()), 0)));
    }
    if let Some(v) = &f.interrupt {
        s.push_str(&format!("\n{c}interrupt {v}"));
    }
    for p in &f.pres {
        s.push_str(&format!("\n{c}pre {}", expr(p, ind + 2)));
    }
    for p in &f.posts {
        s.push_str(&format!("\n{c}post {}", expr(p, ind + 2)));
    }
    for p in &f.examples {
        s.push_str(&format!("\n{c}ex {}", expr(p, ind + 2)));
    }
    if let ExprKind::Table(rows) = &f.body.kind {
        s.replace_range(..2, "rule");
        for r in rows {
            let cells: Vec<String> = r
                .cells
                .iter()
                .map(|c| match c {
                    Cell::Any => "_".to_string(),
                    Cell::Pat(p) => pat(p),
                    Cell::Cond(e) => expr(e, ind + 2),
                })
                .collect();
            s.push_str(&format!("\n{c}| {} => {}", cells.join(", "), expr(&r.out, ind + 2)));
        }
        return s;
    }
    s.push('\n');
    s.extend(std::iter::repeat_n(' ', ind));
    ext(s, &["= ", &expr(&f.body, ind)])
}

pub fn effects(es: &[Effect]) -> String {
    es.iter().map(effect).collect::<Vec<_>>().join(", ")
}

pub fn effect(e: &Effect) -> String {
    if e.args.is_empty() {
        e.name.clone()
    } else {
        let sep = if e.name == "fail" { " | " } else { ", " };
        format!("{}[{}]", e.name, e.args.iter().map(ty).collect::<Vec<_>>().join(sep))
    }
}

pub fn ty(t: &Ty) -> String {
    match t {
        Ty::Named { name, args, .. } if args.is_empty() => name.clone(),
        Ty::Named { name, args, .. } if name == "&" => format!("&{}", ty(&args[0])),
        Ty::Named { name, args, .. } if name == "[]" => format!("[{}]", ty(&args[0])),
        Ty::Named { name, args, .. } if matches!(name.as_str(), "&mut" | "own") => format!("{name} {}", ty(&args[0])),
        Ty::Named { name, args, .. } => format!("{}[{}]", name, args.iter().map(ty).collect::<Vec<_>>().join(", ")),
        Ty::Tuple(xs) => format!("({})", xs.iter().map(ty).collect::<Vec<_>>().join(", ")),
        Ty::Fn { params, ret, effects: es } => {
            let ps = if params.len() == 1 && !matches!(params[0], Ty::Fn { .. } | Ty::Tuple(_)) {
                ty(&params[0])
            } else {
                format!("({})", params.iter().map(ty).collect::<Vec<_>>().join(", "))
            };
            let mut s = format!("{} -> {}", ps, ty(ret));
            if !es.is_empty() {
                s.push_str(&format!(" ! {}", effects(es)));
            }
            s
        }
    }
}

/// String text as it is written between quotes: `{` as `\\{`, and characters that are hard to
/// see or that the formatter uses itself as `\\u{...}`.
fn escape_into(s: &mut String, l: &str) {
    s.reserve(l.len());
    for c in l.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\t' => s.push_str("\\t"),
            '\r' => s.push_str("\\r"),
            '{' => s.push_str("\\{"),
            c if (c as u32) < 0x20 || c == '\u{7f}' || ('\u{e000}'..='\u{e0ff}').contains(&c) => s.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => s.push(c),
        }
    }
}

fn prec_of(e: &Expr) -> u8 {
    match &e.kind {
        ExprKind::Binary(op, ..) => op.prec(),
        ExprKind::Range(..) => 5,
        ExprKind::Unary(UnOp::Not, _) => 3,
        ExprKind::Unary(UnOp::Neg | UnOp::Ref | UnOp::RefMut, _) => 9,
        ExprKind::Int(n) if *n < 0 => 9,
        ExprKind::Float(n) if n.is_sign_negative() => 9,
        ExprKind::Lambda { implicit: false, .. }
        | ExprKind::If(..)
        | ExprKind::Match(..)
        | ExprKind::Catch(..)
        | ExprKind::Handle(..)
        | ExprKind::Block(_)
        | ExprKind::Raise(_)
        | ExprKind::Return(_)
        | ExprKind::With(..)
        | ExprKind::Table(_) => 0,
        _ => 10,
    }
}

fn operand(e: &Expr, min: u8, ind: usize) -> String {
    let s = expr(e, ind);
    if prec_of(e) < min { cat(&["(", &s, ")"]) } else { s }
}

fn cat(xs: &[&str]) -> String {
    let mut s = String::with_capacity(xs.iter().map(|x| x.len()).sum());
    xs.iter().for_each(|x| s.push_str(x));
    s
}

/// `s` with `xs` appended.
fn ext(mut s: String, xs: &[&str]) -> String {
    s.reserve(xs.iter().map(|x| x.len()).sum());
    xs.iter().for_each(|x| s.push_str(x));
    s
}

fn pad(n: usize) -> String {
    " ".repeat(n)
}

pub fn expr(e: &Expr, ind: usize) -> String {
    match &e.kind {
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Float(n) => {
            let s = format!("{n:?}");
            if s.contains(['.', 'e']) { s } else { format!("{s}.0") }
        }
        ExprKind::Str(parts) => {
            let mut s = cat(&[mark(layout::STR_S), "\""]);
            let was = layout::marking(false);
            let flat = layout::flattening(true);
            for p in parts {
                match p {
                    StrPart::Lit(l) => escape_into(&mut s, l),
                    StrPart::Expr(x) => {
                        let t = expr(x, ind);
                        // `{3}`, `{"a"}` and `{{a}}` would read as text (see the lexer).
                        let lone = matches!(x.kind, ExprKind::Int(_) | ExprKind::Float(_))
                            || (matches!(&x.kind, ExprKind::Str(p) if p.iter().all(|q| matches!(q, StrPart::Lit(_)))) && crate::lexer::code_follows(&t[t.find('"').map_or(0, |i| i + 1)..]));
                        let paren = lone || t.starts_with('{');
                        s.push_str(if paren { "{(" } else { "{" });
                        s.push_str(&t);
                        s.push_str(if paren { ")}" } else { "}" });
                    }
                }
            }
            s.push('"');
            layout::flattening(flat);
            layout::marking(was);
            s.push_str(mark(layout::STR_E));
            s
        }
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Unit => "()".into(),
        ExprKind::Name(n) => n.clone(),
        ExprKind::Hole(None) => "?".into(),
        ExprKind::Hole(Some(n)) => format!("?{n}"),
        ExprKind::Placeholder => "_".into(),
        // A line can't start with `.0`, so a tuple element is never where a chain breaks.
        ExprKind::Field(x, f) => ext(operand(x, 10, ind), &[if f.starts_with(|c: char| c.is_ascii_digit()) { "" } else { mark(layout::CHAIN_F) }, ".", f]),
        ExprKind::Call(f, args) => ext(operand(f, 10, ind), &["(", &list(args, ind), ")"]),
        ExprKind::Method { recv, name, targs, args } if name == "mmio" && targs.len() == 1 => {
            let rest: String = args.iter().map(|a| format!(", {}", expr(a, ind))).collect();
            format!("mmio[{}]({}{rest})", ty(&targs[0]), expr(recv, ind))
        }
        ExprKind::Method { recv, name, targs, .. } if name == "#array" && targs.len() == 1 => format!("[{}; {}]", expr(recv, ind), ty(&targs[0])),
        ExprKind::Method { recv, name, .. } if name == ASCRIBE => expr(recv, ind),
        ExprKind::Method { recv, name, targs, args } if name == "asm" && args.len() == 3 => {
            let tmpl = match &recv.kind {
                ExprKind::Str(p) => p.iter().map(|x| if let StrPart::Lit(s) = x { s.as_str() } else { "" }).collect::<String>(),
                _ => String::new(),
            };
            let named = |rest: &str| rest.find('}').is_some_and(|j| j > 0 && rest[..j].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !rest.as_bytes()[0].is_ascii_digit());
            let esc: String = tmpl
                .char_indices()
                .map(|(i, c)| match c {
                    '"' => "\\\"".to_string(),
                    '\\' => "\\\\".to_string(),
                    '\n' => "\\n".to_string(),
                    '\t' => "\\t".to_string(),
                    '{' if !named(&tmpl[i + 1..]) => "\\{".to_string(),
                    c => c.to_string(),
                })
                .collect();
            let mut s = format!("asm \"{esc}\"");
            let fields = |a: &Expr| if let ExprKind::Record { fields, .. } = &a.kind { fields.clone() } else { vec![] };
            let ins = fields(&args[0]);
            if !ins.is_empty() {
                s.push_str(&format!(" in({})", ins.iter().map(|(n, e)| format!("{n}: {}", expr(e, ind))).collect::<Vec<_>>().join(", ")));
            }
            let outs = fields(&args[1]);
            if !outs.is_empty() {
                s.push_str(&format!(" out({})", outs.iter().zip(targs).map(|((n, _), t)| format!("{n}: {}", ty(t))).collect::<Vec<_>>().join(", ")));
            }
            let cl = fields(&args[2]);
            if !cl.is_empty() {
                s.push_str(&format!(" clobber({})", cl.iter().map(|(c, _)| format!("\"{c}\"")).collect::<Vec<_>>().join(", ")));
            }
            s
        }
        ExprKind::Method { recv, name, targs, args } => {
            let mut s = ext(operand(recv, 10, ind), &[mark(layout::CHAIN_M), ".", name]);
            if !targs.is_empty() {
                s = ext(s, &["[", &targs.iter().map(ty).collect::<Vec<_>>().join(", "), "]"]);
            }
            if !args.is_empty() || targs.is_empty() {
                s = ext(s, &["(", &list(args, ind), ")"]);
            }
            s
        }
        ExprKind::Index(a, i) => ext(operand(a, 10, ind), &["[", &expr(i, ind), "]"]),
        ExprKind::Lambda { body, implicit: true, .. } => expr(body, ind),
        ExprKind::Lambda { params, body, tys, .. } => {
            let ps = if tys.iter().any(Option::is_some) {
                let each: Vec<String> = params.iter().zip(tys).map(|(p, t)| match t {
                    Some(t) => format!("{p}: {}", ty(t)),
                    None => p.clone(),
                }).collect();
                format!("({})", each.join(", "))
            } else if params.len() == 1 {
                params[0].clone()
            } else {
                format!("({})", params.join(", "))
            };
            let b = expr(body, ind);
            if b.contains('\n') && !matches!(body.kind, ExprKind::Block(_) | ExprKind::Match(_, _, MatchForm::LetElse)) {
                return format!("{ps} => do\n{}{}", pad(ind + 2), expr(body, ind + 2));
            }
            format!("{ps} => {b}")
        }
        ExprKind::Binary(op, l, r) => {
            let p = op.prec();
            let (lmin, rmin) = if *op == BinOp::Pow { (p + 1, p) } else { (p, p + 1) };
            let ls = if *op == BinOp::Or && matches!(&l.kind, ExprKind::Match(_, arms, MatchForm::Is) if arms[0].guard.is_none()) { expr(l, ind) } else { operand(l, lmin, ind) };
            ext(ls, &[" ", op.symbol(), layout::bin(p), " ", &operand(r, rmin, ind)])
        }
        ExprKind::Unary(UnOp::Neg, x) => format!("-{}", operand(x, 9, ind)),
        ExprKind::Unary(UnOp::Not, x) => format!("not {}", operand(x, 4, ind)),
        ExprKind::Unary(UnOp::Ref, x) => format!("&{}", operand(x, 10, ind)),
        ExprKind::Unary(UnOp::RefMut, x) => format!("&mut {}", operand(x, 10, ind)),
        ExprKind::Range(a, b) => format!("{}..{}", operand(a, 6, ind), operand(b, 6, ind)),
        ExprKind::If(c, t, f) => if_form(&expr(c, ind), t, f.as_deref(), ind),
        ExprKind::Match(s, arms, MatchForm::IfIs) if arms.len() == 2 && arms[1].pat == Pat::Wild && arms[1].guard.is_none() => {
            let c = is_test(s, &arms[0], ind);
            let f = (arms[1].body.kind != ExprKind::Unit).then_some(&arms[1].body);
            if_form(&c, &arms[0].body, f, ind)
        }
        ExprKind::Match(s, arms, MatchForm::Is) if arms.len() == 2 => is_test(s, &arms[0], ind),
        ExprKind::Match(inner, arms, MatchForm::IfChain) if arms.len() == 2 && chain_parts(inner, ind).is_some() => {
            let (tests, t) = chain_parts(inner, ind).unwrap();
            let f = (arms[1].body.kind != ExprKind::Unit).then_some(&arms[1].body);
            if_form(&tests.join(&format!(" and{} ", layout::bin(2))), t, f, ind)
        }
        ExprKind::Match(_, _, MatchForm::LetElse) => {
            let mut s = String::from("do");
            for l in guard_lines(e, ind + 2) {
                s.push_str(&format!("\n{}{l}", pad(ind + 2)));
            }
            s
        }
        ExprKind::Match(s, arms, _) => format!("match {}{}", arms_head(s, ind), print_arms(arms, ind)),
        ExprKind::Catch(s, arms) => format!("catch {}{}", arms_head(s, ind), print_arms(arms, ind)),
        ExprKind::Handle(s, arms) => format!("handle {}{}", arms_head(s, ind), print_arms(arms, ind)),
        ExprKind::Block(stmts) => {
            let mut s = String::from("do");
            for l in stmt_lines(stmts, ind + 2) {
                s.push('\n');
                s.extend(std::iter::repeat_n(' ', ind + 2));
                s.push_str(&l);
            }
            s
        }
        ExprKind::Record { ctor, fields } => {
            // A value that prints as the field's name is that variable: write the shorthand.
            let items: Vec<String> = fields
                .iter()
                .map(|(n, v)| {
                    let s = expr(v, ind);
                    if s == *n { s } else { format!("{n}: {s}") }
                })
                .collect();
            format!("{}{{{}}}", ctor.as_deref().unwrap_or(""), layout::group(&items))
        }
        ExprKind::List(xs) => format!("[{}]", list(xs, ind)),
        ExprKind::Tuple(xs) => format!("({})", list(xs, ind)),
        ExprKind::Par(xs) => format!("par({})", list(xs, ind)),
        ExprKind::Raise(x) => format!("raise {}", expr(x, ind)),
        ExprKind::Return(x) => format!("return {}", expr(x, ind)),
        ExprKind::Table(_) => "<rule table>".into(),
        ExprKind::With(base, ups) => {
            let items: Vec<String> = ups.iter().map(|(p, v)| format!("{} := {}", path(p, ind), expr(v, ind))).collect();
            format!("{} with {}", operand(base, 5, ind), layout::updates(&items))
        }
    }
}

/// `e is p`, and ` and guard` when the arm has one.
fn is_test(s: &Expr, a: &Arm, ind: usize) -> String {
    let mut c = format!("{} is {}", operand(s, 4, ind), pat(&a.pat));
    if let Some(g) = &a.guard {
        let gs = if matches!(g.kind, ExprKind::Match(_, _, MatchForm::Is)) { expr(g, ind) } else { operand(g, 2, ind) };
        c.push_str(&format!(" and{} {gs}", layout::bin(2)));
    }
    c
}

/// The tests of an `IfChain`'s nested matches, and the then branch inside its `some(..)`.
fn chain_parts(mut e: &Expr, ind: usize) -> Option<(Vec<String>, &Expr)> {
    let mut tests = Vec::new();
    loop {
        let ExprKind::Match(s, arms, MatchForm::IfIs) = &e.kind else { return None };
        if arms.len() != 2 || !matches!(&arms[1].body.kind, ExprKind::Name(n) if n == "none") {
            return None;
        }
        tests.push(is_test(s, &arms[0], ind));
        match &arms[0].body.kind {
            ExprKind::Call(f, args) if matches!(&f.kind, ExprKind::Name(n) if n == "some") && args.len() == 1 => return Some((tests, &args[0])),
            _ if arms[0].guard.is_some() => return None,
            _ => e = &arms[0].body,
        }
    }
}

/// A block's statements, one per line; a trailing `p = e else b` brings the statements after it.
fn stmt_lines(stmts: &[Stmt], ind: usize) -> Vec<String> {
    let mut out = Vec::new();
    for (i, st) in stmts.iter().enumerate() {
        match st {
            Stmt::Expr(e @ Expr { kind: ExprKind::Match(_, _, MatchForm::LetElse), .. }) if i + 1 == stmts.len() => out.extend(guard_lines(e, ind)),
            _ => out.push(stmt(st, ind)),
        }
    }
    out
}

/// `p = e else b` and the lines after it, from `match e | p => rest | _ => b`.
fn guard_lines(e: &Expr, ind: usize) -> Vec<String> {
    let ExprKind::Match(s, arms, MatchForm::LetElse) = &e.kind else { return vec![expr(e, ind)] };
    let mut out = vec![format!("{} = {} else {}", pat(&arms[0].pat), expr(s, ind), branch(&arms[1].body, ind))];
    match &arms[0].body.kind {
        ExprKind::Unit => {}
        ExprKind::Block(stmts) => out.extend(stmt_lines(stmts, ind)),
        ExprKind::Match(_, _, MatchForm::LetElse) => out.extend(guard_lines(&arms[0].body, ind)),
        _ => out.push(expr(&arms[0].body, ind)),
    }
    out
}

fn if_form(c: &str, t: &Expr, f: Option<&Expr>, ind: usize) -> String {
    let c = cat(&[c, mark(layout::THEN)]);
    let nested = matches!(t.kind, ExprKind::If(..) | ExprKind::Match(_, _, MatchForm::IfIs | MatchForm::IfChain) | ExprKind::Lambda { .. });
    let Some(f) = f else { return cat(&["if ", &c, " then ", &branch(t, ind)]) };
    let block = matches!(&t.kind, ExprKind::Match(_, _, MatchForm::LetElse)) || matches!(&t.kind, ExprKind::Block(st) if !matches!(st.as_slice(), [Stmt::Assign(..)]));
    // `sspur fmt` lines `else` up with the `if` and indents each block once (ADR 0040).
    if block && layout::natural() {
        return format!("if {c} then {}\n{}else {}", branch(t, ind), pad(ind), branch(f, ind));
    }
    if block {
        return format!("if {c} then {}\n{}else {}", branch(t, ind + 2), pad(ind + 2), branch(f, ind + 2));
    }
    let wide = nested.then(|| expr(t, ind + 4));
    if let Some(te) = wide.as_ref().filter(|te| !te.contains('\n')) {
        return format!("if {c} then ({te}){} else {}", mark(layout::ELSE), branch(f, ind));
    }
    let arms = nested || matches!(t.kind, ExprKind::Match(..) | ExprKind::Catch(..) | ExprKind::Handle(..));
    if arms && layout::natural() {
        return format!("if {c} then do\n{}{}\n{}else {}", pad(ind + 2), expr(t, ind + 2), pad(ind), branch(f, ind));
    }
    if arms {
        return format!("if {c} then do\n{}{}\n{}else {}", pad(ind + 4), wide.unwrap_or_else(|| expr(t, ind + 4)), pad(ind + 2), branch(f, ind + 2));
    }
    format!("if {c} then {}{} else {}", branch(t, ind), mark(layout::ELSE), branch(f, ind))
}

fn iter_expr(e: &Expr, ind: usize) -> String {
    match e.kind {
        ExprKind::Block(_) | ExprKind::Match(_, _, MatchForm::LetElse) => expr(e, ind + 2),
        _ => expr(e, ind),
    }
}

fn arms_head(e: &Expr, ind: usize) -> String {
    match e.kind {
        ExprKind::Match(..) | ExprKind::Catch(..) | ExprKind::Handle(..) if layout::flat() => format!("({})", expr(e, ind)),
        ExprKind::Match(..) | ExprKind::Catch(..) | ExprKind::Handle(..) => format!("do\n{}{}", pad(ind + 2), expr(e, ind + 2)),
        _ => expr(e, ind),
    }
}

fn path(p: &[PathSeg], ind: usize) -> String {
    let mut s = String::new();
    for (i, seg) in p.iter().enumerate() {
        match seg {
            PathSeg::Field(f) if i == 0 => s.push_str(f),
            PathSeg::Field(f) => s.push_str(&format!(".{f}")),
            PathSeg::Index(e) => s.push_str(&format!("[{}]", expr(e, ind))),
        }
    }
    s
}

fn branch(e: &Expr, ind: usize) -> String {
    match &e.kind {
        ExprKind::Block(stmts) if matches!(stmts.as_slice(), [Stmt::Assign(..)]) => stmt(&stmts[0], ind),
        _ => expr(e, ind),
    }
}

fn list(xs: &[Expr], ind: usize) -> String {
    layout::group(&xs.iter().map(|x| expr(x, ind)).collect::<Vec<_>>())
}

fn print_arms(arms: &[Arm], ind: usize) -> String {
    let mut s = String::new();
    let lead = if layout::flat() { " | ".to_string() } else { format!("\n{}| ", pad(ind)) };
    for (k, a) in arms.iter().enumerate() {
        s.push_str(&format!("{lead}{}", pat(&a.pat)));
        if let Some(g) = &a.guard {
            s.push_str(&format!(" if {}{}", expr(g, ind), mark(layout::THEN)));
        }
        let b = branch(&a.body, ind);
        // A `match`, `catch` or `handle` whose arms would print where these are takes the arms
        // below it; a block one level in ends before them.
        if k + 1 < arms.len() && b.contains(&lead) && layout::flat() {
            s.push_str(&format!(" => ({b})"));
        } else if k + 1 < arms.len() && b.contains(&lead) {
            s.push_str(&format!(" => do\n{}{}", pad(ind + 2), branch(&a.body, ind + 2)));
        } else {
            s.push_str(&format!(" => {b}"));
        }
    }
    s
}

fn stmt(s: &Stmt, ind: usize) -> String {
    match s {
        Stmt::Let(p, e) => match e.ascription() {
            Some((v, t)) => format!("{}: {} = {}", pat(p), ty(t), expr(v, ind)),
            None => format!("{} = {}", pat(p), expr(e, ind)),
        },
        Stmt::Var(n, e) => match e.ascription() {
            Some((v, t)) => format!("var {n}: {} = {}", ty(t), expr(v, ind)),
            None => format!("var {} = {}", n, expr(e, ind)),
        },
        Stmt::Assign(n, e, _) => match &e.kind {
            ExprKind::With(base, ups) if matches!(&base.kind, ExprKind::Name(b) if b == n) && ups.len() == 1 && !ups[0].0.is_empty() => {
                let mut p = n.clone();
                for seg in &ups[0].0 {
                    match seg {
                        PathSeg::Field(f) => p.push_str(&format!(".{f}")),
                        PathSeg::Index(e) => p.push_str(&format!("[{}]", expr(e, ind))),
                    }
                }
                format!("{p} := {}", expr(&ups[0].1, ind))
            }
            _ => format!("{} := {}", n, expr(e, ind)),
        },
        Stmt::Expr(e) => expr(e, ind),
        Stmt::For(p, it, body) => loop_lines(&format!("for {} in {}", pat(p), iter_expr(it, ind)), body, ind),
        Stmt::While(c, body) => {
            let (c, body) = while_is(c, body, ind).unwrap_or_else(|| (expr(c, ind), body));
            loop_lines(&format!("while {c}"), body, ind)
        }
        Stmt::Fn(f) => print_fn_at(f, ind),
    }
}

/// The tests and the body of `while e is p ...` (see `while_is_body`).
fn while_is<'a>(c: &'a Expr, body: &Expr, ind: usize) -> Option<(String, &'a Expr)> {
    let b = while_is_body(c, body)?;
    let tests = match &c.kind {
        ExprKind::Match(inner, _, MatchForm::IfChain) => chain_parts(inner, ind)?.0.join(&format!(" and{} ", layout::bin(2))),
        ExprKind::Match(s, arms, _) if arms[1].pat == Pat::Wild && arms[1].guard.is_none() => is_test(s, &arms[0], ind),
        _ => return None,
    };
    Some((tests, b))
}

fn loop_lines(head: &str, body: &Expr, ind: usize) -> String {
    match &body.kind {
        ExprKind::Block(stmts) => {
            let mut s = head.to_string();
            for l in stmt_lines(stmts, ind + 2) {
                s.push_str(&format!("\n{}{l}", pad(ind + 2)));
            }
            s
        }
        ExprKind::Match(_, _, MatchForm::LetElse) => {
            let mut s = head.to_string();
            for l in guard_lines(body, ind + 2) {
                s.push_str(&format!("\n{}{l}", pad(ind + 2)));
            }
            s
        }
        _ => format!("{head}\n{}{}", pad(ind + 2), branch(body, ind + 2)),
    }
}

pub fn pat(p: &Pat) -> String {
    match p {
        Pat::Wild => "_".into(),
        Pat::Bind(n) => n.clone(),
        Pat::Int(n) => n.to_string(),
        Pat::Str(s) => {
            let mut t = String::from("\"");
            escape_into(&mut t, s);
            format!("{}{t}\"{}", mark(layout::STR_S), mark(layout::STR_E))
        }
        Pat::Bool(b) => b.to_string(),
        Pat::Tuple(xs) => format!("({})", layout::group(&xs.iter().map(pat).collect::<Vec<_>>())),
        Pat::Ctor { name, args: CtorArgs::None } => name.clone(),
        Pat::Ctor { name, args: CtorArgs::Positional(xs) } => {
            format!("{}({})", name, layout::group(&xs.iter().map(pat).collect::<Vec<_>>()))
        }
        Pat::Ctor { name, args: CtorArgs::Record(fs) } => {
            let items: Vec<String> = fs
                .iter()
                .map(|(f, p)| match p {
                    Pat::Bind(b) if b == f => f.clone(),
                    _ => format!("{}: {}", f, pat(p)),
                })
                .collect();
            format!("{}{{{}}}", name, layout::group(&items))
        }
        Pat::Or(alts) => alts.iter().map(|a| match a {
            Pat::Or(_) => format!("({})", pat(a)),
            _ => pat(a),
        }).collect::<Vec<_>>().join(" | "),
        Pat::List { head, rest, tail } => {
            let mut items: Vec<String> = head.iter().map(pat).collect();
            if let Some(r) = rest {
                items.push(format!("..{}", r.as_deref().unwrap_or("")));
                items.extend(tail.iter().map(pat));
            }
            format!("[{}]", layout::group(&items))
        }
    }
}
