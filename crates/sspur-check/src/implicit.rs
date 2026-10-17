//! Generic functions over data: a function whose body decodes a type parameter
//! (`json.decode[T]`, `db.query[T]`, `csv.read[T]`) gets an implicit bound on it, so it is
//! specialized per instantiation like a bounded function, and each instantiation is checked to be
//! decodable at the call. A generic function called with explicit type arguments, `f[Int](x)`,
//! is specialized the same way.
use super::*;

/// The implicit bound for decoding a parameter with `ns.op[T]`.
fn data_bound(ns: &str, op: &str) -> Option<&'static str> {
    match (ns, op) {
        ("json", "decode") => Some(DECODE),
        ("db", "query" | "one" | "opt") => Some(ROW),
        ("csv", "decode" | "read" | "fold") => Some(CSV),
        _ => None,
    }
}

pub(crate) const DECODE: &str = "#Decode";
pub(crate) const ROW: &str = "#Row";
pub(crate) const CSV: &str = "#Csv";
pub(crate) const INST: &str = "#Inst";

/// How a diagnostic names an implicit bound.
pub(crate) fn describe(tr: &str) -> Option<&'static str> {
    match tr {
        DECODE => Some("decodable from JSON"),
        ROW => Some("readable from SQL rows"),
        CSV => Some("readable from CSV"),
        INST => Some("a type"),
        _ => None,
    }
}

fn mentions(t: &Ty, p: &str) -> bool {
    match t {
        Ty::Named { name, args, .. } => name == p || args.iter().any(|a| mentions(a, p)),
        Ty::Tuple(xs) => xs.iter().any(|a| mentions(a, p)),
        Ty::Fn { params, ret, .. } => params.iter().any(|a| mentions(a, p)) || mentions(ret, p),
    }
}

/// `List[Int]` written as an index expression, `f[List[Int]](..)`, as a type.
pub(crate) fn expr_ty(e: &Expr) -> Option<Ty> {
    match &e.kind {
        ExprKind::Name(n) if n.starts_with(|c: char| c.is_ascii_uppercase()) || n.contains('.') => Some(Ty::Named { name: n.clone(), args: vec![], span: e.span }),
        ExprKind::Index(b, a) => {
            let Ty::Named { name, args, span } = expr_ty(b)? else { return None };
            if !args.is_empty() {
                return None;
            }
            let args = match &a.kind {
                ExprKind::Tuple(xs) => xs.iter().map(expr_ty).collect::<Option<Vec<_>>>()?,
                _ => vec![expr_ty(a)?],
            };
            Some(Ty::Named { name, args, span })
        }
        ExprKind::Tuple(xs) => Some(Ty::Tuple(xs.iter().map(expr_ty).collect::<Option<Vec<_>>>()?)),
        _ => None,
    }
}

/// The explicit type argument of a call `f[T](args)` of the generic function `f`: one, the
/// first type parameter (`f[(Int, Str)]` is a tuple).
pub(crate) fn explicit_call<'a>(f: &'a Expr, generic: &dyn Fn(&str) -> bool) -> Option<(&'a str, Vec<Ty>)> {
    let ExprKind::Index(g, i) = &f.kind else { return None };
    let ExprKind::Name(n) = &g.kind else { return None };
    if !generic(n) {
        return None;
    }
    Some((n.as_str(), vec![expr_ty(i)?]))
}

/// The implicit bounds of every generic function of `m`.
pub(crate) fn bounds(m: &Module) -> HashMap<String, Vec<traits::Bound>> {
    let generic: HashMap<&str, Vec<String>> = m
        .defs
        .iter()
        .filter_map(|d| match d {
            Def::Fn(f) => {
                let ps: Vec<String> = f.tparams.iter().map(|p| p.name.clone()).filter(|n| n.starts_with(|c: char| c.is_ascii_uppercase())).collect();
                (!ps.is_empty()).then_some((f.name.as_str(), ps))
            }
            _ => None,
        })
        .collect();
    let mut out: HashMap<String, Vec<traits::Bound>> = HashMap::new();
    let mut calls: Vec<(String, String, Vec<Ty>)> = Vec::new();
    let add = |out: &mut HashMap<String, Vec<traits::Bound>>, f: &str, p: &str, tr: &str| -> bool {
        let bs = out.entry(f.to_string()).or_default();
        if bs.iter().any(|(bp, bt, _)| bp == p && bt == tr) {
            return false;
        }
        bs.push((p.to_string(), tr.to_string(), vec![]));
        true
    };
    for d in &m.defs {
        let roots: Vec<(&str, &Expr)> = match d {
            Def::Fn(f) => std::iter::once(&f.body).chain(&f.pres).chain(&f.posts).chain(&f.examples).map(|e| (f.name.as_str(), e)).collect(),
            Def::Test(t) => vec![(t.name.as_str(), &t.body)],
            Def::Static(s) => vec![(s.name.as_str(), &s.init)],
            Def::Impl(i) => i.fns.iter().map(|f| (f.name.as_str(), &f.body)).collect(),
            _ => vec![],
        };
        for (fname, root) in roots {
            scan(fname, root, generic.get(fname), &generic, &mut out, &mut calls);
        }
    }
    fn scan(fname: &str, root: &Expr, mine: Option<&Vec<String>>, generic: &HashMap<&str, Vec<String>>, out: &mut HashMap<String, Vec<traits::Bound>>, calls: &mut Vec<(String, String, Vec<Ty>)>) {
        let add = |out: &mut HashMap<String, Vec<traits::Bound>>, f: &str, p: &str, tr: &str| {
            let bs = out.entry(f.to_string()).or_default();
            if !bs.iter().any(|(bp, bt, _)| bp == p && bt == tr) {
                bs.push((p.to_string(), tr.to_string(), vec![]));
            }
        };
        let f = fname;
        let mut visit = |e: &Expr| {
            match &e.kind {
                ExprKind::Method { recv, name, targs, .. } if !targs.is_empty() => {
                    let tr = match &recv.kind {
                        ExprKind::Name(ns) => data_bound(ns, name),
                        _ => None,
                    }
                    .or((name == "json").then_some(DECODE));
                    if let (Some(tr), Some(ps)) = (tr, mine) {
                        for p in ps.iter().filter(|p| mentions(&targs[0], p)) {
                            add(out, f, p, tr);
                        }
                    }
                }
                ExprKind::Call(callee, _) => {
                    if let Some((g, tys)) = explicit_call(callee, &|n| generic.contains_key(n)) {
                        add(out, g, &generic[g][0], INST);
                        calls.push((f.to_string(), g.to_string(), tys));
                    }
                }
                _ => {}
            }
            true
        };
        visit::walk_expr(root, &mut visit);
    }
    let mut changed = true;
    while changed {
        changed = false;
        for (f, g, tys) in &calls {
            let Some(mine) = generic.get(f.as_str()) else { continue };
            let theirs: Vec<(String, String)> = out.get(g).into_iter().flatten().filter(|(_, tr, _)| tr != INST).map(|(p, tr, _)| (p.clone(), tr.clone())).collect();
            for (q, tr) in theirs {
                let Some(i) = generic[g.as_str()].iter().position(|x| *x == q) else { continue };
                let Some(t) = tys.get(i) else { continue };
                for p in mine.iter().filter(|p| mentions(t, p)) {
                    changed |= add(&mut out, f, p, &tr);
                }
            }
        }
    }
    out
}
