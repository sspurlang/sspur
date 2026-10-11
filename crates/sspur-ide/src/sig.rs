//! Signature help: the signature of the call the cursor is in, with the argument it is at. The
//! call is found in the text, which is usually incomplete while it is being typed; its callee is
//! typed by checking the text with the call closed, or else by the last check that parsed.
use crate::{docs, globals, method_sig, methods_of, recv_key, shown, split_sig, u16_of, word_before, Analysis, Ctx};
use serde_json::{json, Value as J};
use sspur_check::Type;
use sspur_syntax::{printer, visit, Def, Expr, ExprKind, FnDef, Stmt};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Frame {
    Paren { at: usize, commas: usize },
    Open(u8),
    Str,
    Interp,
}

/// The innermost open `(` before `pos`, the number of commas directly inside it, and the text
/// that would close everything still open at `pos`.
pub fn enclosing_call(src: &str, pos: usize) -> Option<(usize, usize, String)> {
    let pos = pos.min(src.len());
    let b = src.as_bytes();
    let mut stack: Vec<Frame> = Vec::new();
    let mut i = 0;
    let mut line_begin = true;
    while i < pos {
        let c = b[i];
        let in_str = stack.last() == Some(&Frame::Str);
        if c == b'\n' {
            if let Some(k) = stack.iter().position(|f| *f == Frame::Str) {
                stack.truncate(k);
            }
            line_begin = true;
            i += 1;
            continue;
        }
        if line_begin && !in_str && c.is_ascii_alphabetic() {
            // A line that starts at the margin starts a new definition.
            stack.clear();
        }
        line_begin = false;
        if in_str {
            match c {
                b'\\' => i += 1,
                b'"' => {
                    stack.pop();
                }
                b'{' => stack.push(Frame::Interp),
                _ => {}
            }
            i += 1;
            continue;
        }
        match c {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < pos && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'"' => stack.push(Frame::Str),
            b'(' => stack.push(Frame::Paren { at: i, commas: 0 }),
            b'[' | b'{' => stack.push(Frame::Open(c)),
            b')' | b']' | b'}' => {
                let want = |f: &Frame| matches!((f, c), (Frame::Paren { .. }, b')') | (Frame::Open(b'['), b']') | (Frame::Open(b'{') | Frame::Interp, b'}'));
                if let Some(k) = stack.iter().rposition(want) {
                    stack.truncate(k);
                }
            }
            b',' => {
                if let Some(Frame::Paren { commas, .. }) = stack.last_mut() {
                    *commas += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let (at, commas) = stack.iter().rev().find_map(|f| match f {
        Frame::Paren { at, commas } => Some((*at, *commas)),
        _ => None,
    })?;
    let close: String = stack
        .iter()
        .rev()
        .map(|f| match f {
            Frame::Paren { .. } => ')',
            Frame::Open(b'[') => ']',
            Frame::Open(_) | Frame::Interp => '}',
            Frame::Str => '"',
        })
        .collect();
    Some((at, commas, close))
}

/// `name(p1, p2) -> ret` with the UTF-16 range of each parameter in it.
pub fn compose(name: &str, params: &[String], ret: &str) -> (String, Vec<[usize; 2]>) {
    let mut label = format!("{name}(");
    let mut ranges = Vec::new();
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            label.push_str(", ");
        }
        let a = label.encode_utf16().count();
        label.push_str(p);
        ranges.push([a, label.encode_utf16().count()]);
    }
    label.push(')');
    if !ret.is_empty() {
        label.push_str(&format!(" -> {ret}"));
    }
    (label, ranges)
}

struct Found {
    name: String,
    params: Vec<String>,
    ret: String,
    doc: Option<String>,
}

fn of_sig(name: String, sig: &str, skip: usize, doc: Option<String>) -> Option<Found> {
    let (_, ps, ret) = split_sig(sig.trim_start_matches("pub ").trim_start_matches("fn "))?;
    Some(Found { name, params: ps.iter().skip(skip).map(|p| p.to_string()).collect(), ret: ret.to_string(), doc })
}

/// Signature help at `pos16` (a UTF-16 offset): `{name, params, ret, label, ranges, active, doc,
/// from}`, where `ranges` are the parameters' UTF-16 ranges in `label` and `from` is the `(`.
pub fn signature_help(cx: &Ctx, src: &str, pos16: usize) -> J {
    let pos = crate::byte_of(src, pos16);
    let Some((open, active, close)) = enclosing_call(src, pos) else { return J::Null };
    let Some(f) = resolve(cx, src, pos, open, &close) else { return J::Null };
    let (label, ranges) = compose(&f.name, &f.params, &f.ret);
    json!({ "name": f.name, "params": f.params, "ret": f.ret, "label": label, "ranges": ranges, "active": active, "doc": f.doc, "from": u16_of(src, open) })
}

fn resolve(cx: &Ctx, src: &str, pos: usize, open: usize, close: &str) -> Option<Found> {
    let b = src.as_bytes();
    let mut end = open;
    if end > 0 && b[end - 1] == b']' {
        let mut depth = 0;
        while end > 0 {
            end -= 1;
            match b[end] {
                b']' => depth += 1,
                b'[' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    let word = word_before(src, end);
    if word.is_empty() || word.as_bytes()[0].is_ascii_digit() || sspur_syntax::lexer::KEYWORDS.contains(&word) {
        return None;
    }
    let start = end - word.len();
    let dot = (start > 0 && b[start - 1] == b'.' && !(start > 1 && b[start - 2] == b'.')).then(|| start - 1);
    let fresh = cx.last.clone().filter(|a| a.src == src).or_else(|| cx.analyze(src)).or_else(|| {
        let line_end = src[pos..].find('\n').map_or(src.len(), |k| pos + k);
        let before = src[..pos].trim_end();
        let hole = if before.ends_with(|c: char| "(,+-*/%<>=!&|[{:".contains(c)) { "?" } else { "" };
        cx.analyze(&format!("{}{hole}{close}{}", &src[..pos], &src[line_end..]))
    });
    let a = fresh.clone().or_else(|| cx.last.clone())?;
    let fresh = fresh.is_some();
    match dot {
        Some(dot) => method(&a, fresh, src, dot, word),
        None => function(&a, fresh, start, word),
    }
}

/// The type of the receiver that ends at `dot`: the checker's when the analysis is of this text,
/// else the type the last check gave a variable of that name.
fn receiver(a: &Analysis, fresh: bool, src: &str, dot: usize) -> Option<Type> {
    if fresh {
        return a.check.expr_types.iter().filter(|((_, e, _), _)| *e as usize == dot).max_by_key(|((s, e, _), _)| e - s).map(|(_, t)| t.clone());
    }
    let recv = word_before(src, dot);
    a.check.expr_types.iter().filter(|((s, e, tag), _)| *tag == 8 && a.src.get(*s as usize..*e as usize) == Some(recv)).min_by_key(|((s, _, _), _)| (*s as i64 - dot as i64).abs()).map(|(_, t)| t.clone())
}

fn method(a: &Analysis, fresh: bool, src: &str, dot: usize, name: &str) -> Option<Found> {
    let d = docs();
    if let Some(t) = receiver(a, fresh, src, dot).filter(|t| !t.to_string().contains('?'))
        && let Some(key) = recv_key(&t)
    {
        if let Some(sig) = methods_of(&key).find(|s| split_sig(s).is_some_and(|(n, _, _)| n == name)) {
            let doc = d.by_method.get(&(shown(&key).to_string(), name.to_string())).cloned();
            return of_sig(format!("{}.{name}", shown(&key)), &method_sig(sig), 0, doc);
        }
        if let Some((ps, _)) = a.check.fn_types.get(name)
            && matches!(ps.first(), Some(Type::Con(n, _)) if *n == key)
            && let Some(sig) = a.check.sigs.get(name)
        {
            return of_sig(name.to_string(), sig, 1, user_doc(a, name));
        }
        return None;
    }
    let pkg = word_before(src, dot);
    if pkg.is_empty() {
        return None;
    }
    let m = sspur_syntax::link::mangle(pkg, name);
    let sig = a.check.sigs.get(&m)?;
    of_sig(m.clone(), sig, 0, None)
}

fn function(a: &Analysis, fresh: bool, start: usize, name: &str) -> Option<Found> {
    if let Some(sig) = a.check.sigs.get(name) {
        return of_sig(name.to_string(), sig, 0, user_doc(a, name));
    }
    for def in &a.module.defs {
        if let Def::Effect(e) = def
            && let Some(op) = e.ops.iter().find(|o| o.name == name)
        {
            let params = op.params.iter().map(|p| format!("{}: {}", p.name, printer::ty(&p.ty))).collect();
            let ret = format!("{} ! {}", op.ret.as_ref().map_or("Unit".to_string(), printer::ty), e.name);
            return Some(Found { name: name.to_string(), params, ret, doc: None });
        }
    }
    if let Some(f) = local_fn(a, name, start as u32) {
        return of_sig(name.to_string(), &printer::print_sig(f), 0, None);
    }
    if let Some(sig) = globals().find(|s| split_sig(s).is_some_and(|(n, _, _)| n == name)) {
        return of_sig(name.to_string(), sig, 0, docs().by_name.get(name).cloned());
    }
    if fresh
        && let Some(Type::Fn(ps, r, row)) = a.check.expr_types.get(&(start as u32, (start + name.len()) as u32, 8))
    {
        let ret = Type::Fn(vec![], r.clone(), row.clone()).to_string();
        let ret = ret.trim_start_matches("()").trim_start().trim_start_matches("->").trim();
        return Some(Found { name: name.to_string(), params: ps.iter().map(Type::to_string).collect(), ret: ret.to_string(), doc: None });
    }
    None
}

/// A function defined inside the definition that contains `at`.
fn local_fn<'a>(a: &'a Analysis, name: &str, at: u32) -> Option<&'a FnDef> {
    fn find<'a>(e: &'a Expr, name: &str) -> Option<&'a FnDef> {
        if let ExprKind::Block(stmts) = &e.kind {
            for s in stmts {
                if let Stmt::Fn(f) = s
                    && f.name == name
                {
                    return Some(f);
                }
            }
        }
        visit::children(e).into_iter().find_map(|c| find(c, name))
    }
    let (def, _) = crate::locals_at(&a.module, at.min(a.src.len() as u32));
    match def? {
        Def::Fn(f) => find(&f.body, name),
        Def::Test(t) => find(&t.body, name),
        Def::Impl(i) => i.fns.iter().find_map(|f| find(&f.body, name)),
        _ => None,
    }
}

/// The comment lines just above a definition, joined into one line.
fn user_doc(a: &Analysis, name: &str) -> Option<String> {
    let d = a.module.defs.iter().find(|d| d.name() == name)?;
    let at = (d.span().start as usize).min(a.src.len());
    let mut lines: Vec<&str> = Vec::new();
    for l in a.src[..a.src[..at].rfind('\n').map_or(0, |i| i + 1)].lines().rev() {
        match l.trim_start().strip_prefix("//") {
            Some(c) => lines.push(c.trim()),
            None => break,
        }
    }
    lines.reverse();
    let doc = lines.join(" ");
    (!doc.is_empty()).then_some(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{analysis, load};
    use std::collections::BTreeSet;

    const SRC: &str = "type P = {x: Int, y: Int}

// Scales n by k.
fn scale(n: Int, k: Int) -> Int
= n * k

fn main() -> Unit ! log
= do
  xs = [1, 2, 3]
  log(scale(1, 2).str)
";

    fn at(src: &str, marker: &str) -> (String, usize) {
        let i = src.find(marker).unwrap();
        let text = src.replacen(marker, "", 1);
        (text.clone(), u16_of(&text, i))
    }

    #[test]
    fn finds_the_enclosing_call() {
        assert_eq!(enclosing_call("f(a, g(b, c", 11), Some((6, 1, "))".to_string())));
        assert_eq!(enclosing_call("f(a, \"x, (y\", ", 14).map(|(a, n, _)| (a, n)), Some((1, 2)));
        assert_eq!(enclosing_call("f(a, [1, 2], ", 13).map(|(a, n, _)| (a, n)), Some((1, 2)));
        assert_eq!(enclosing_call("f(a)\ng(", 7).map(|(a, n, _)| (a, n)), Some((6, 0)));
        assert_eq!(enclosing_call("f(a)", 4), None);
        assert_eq!(enclosing_call("f(a,\nfn g", 10), None);
    }

    #[test]
    fn helps_while_typing() {
        let good = analysis(SRC);
        let cx = Ctx { load: &load, last: Some(good), exports: BTreeSet::new() };
        let typing = SRC.replace("  log(scale(1, 2).str)\n", "  y = scale(1, |\n  log(\"hi\")\n");
        let (text, pos) = at(&typing, "|");
        let h = signature_help(&cx, &text, pos);
        assert_eq!(h["label"], "scale(n: Int, k: Int) -> Int", "{h}");
        assert_eq!(h["active"], 1);
        assert_eq!(h["doc"], "Scales n by k.");
        let r = h["ranges"][1].as_array().unwrap();
        assert_eq!(&h["label"].as_str().unwrap()[r[0].as_u64().unwrap() as usize..r[1].as_u64().unwrap() as usize], "k: Int");

        let (text, pos) = at(&SRC.replace("  log(scale(1, 2).str)\n", "  ys = xs.map(|\n"), "|");
        let h = signature_help(&cx, &text, pos);
        assert_eq!(h["name"], "List.map", "{h}");
        assert_eq!(h["active"], 0);

        let (text, pos) = at(&SRC.replace("scale(1, 2)", "scale(1, min(3, |))"), "|");
        let h = signature_help(&cx, &text, pos);
        assert_eq!(h["name"], "min", "{h}");
        assert_eq!(h["active"], 1);

        let (text, pos) = at(&SRC.replace("scale(1, 2)", "scale(|1, 2)"), "|");
        assert_eq!(signature_help(&cx, &text, pos)["active"], 0);
        let (text, pos) = at(&SRC.replace("[1, 2, 3]", "[1, |2, 3]"), "|");
        assert!(signature_help(&cx, &text, pos).is_null());
    }

    #[test]
    fn composes_labels() {
        let (l, r) = compose("f", &["a: Int".into(), "b: Str".into()], "Bool");
        assert_eq!(l, "f(a: Int, b: Str) -> Bool");
        assert_eq!(r, vec![[2, 8], [10, 16]]);
    }
}
