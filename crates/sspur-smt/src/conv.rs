//! `W_CONVERSION_MAY_TRAP`: an `F64` `round floor ceil trunc` that Z3 can't prove finite and in
//! `[-2^63, 2^63)` (ADR 0044).

use super::*;
use sspur_check::Diag;

struct Conv<'a> {
    def: String,
    e: &'a Expr,
    recv: &'a Expr,
    method: &'a str,
}

enum Unit<'a> {
    Fn(&'a FnDef),
    Plain(&'a FnDef),
    Ex(&'a FnDef, usize),
    Test(&'a TestDef),
}

/// What `warnings` found: the diagnostics, and how many conversions went unchecked because Z3
/// isn't there (or `SSPUR_SMT=off`).
#[derive(Default)]
pub struct Report {
    pub diags: Vec<Diag>,
    pub unchecked: usize,
}

fn conv_of<'a>(e: &'a Expr, check: &CheckOutput) -> Option<(&'a Expr, &'a str)> {
    let (recv, name) = match &e.kind {
        ExprKind::Field(r, n) => (&**r, n.as_str()),
        ExprKind::Method { recv, name, args, .. } if args.is_empty() => (&**recv, name.as_str()),
        _ => return None,
    };
    if !float::CONV.contains(&name) || check.user_methods.contains(&(e.span.start, e.span.end)) {
        return None;
    }
    let t = |x: &Expr| check.expr_types.get(&expr_key(x));
    (is_ty(t(recv), "F64") && is_ty(t(e), "Int")).then_some((recv, name))
}

fn has_conv(x: &Expr, check: &CheckOutput) -> bool {
    let mut any = false;
    visit::walk_expr(x, &mut |e| {
        any |= conv_of(e, check).is_some();
        !any
    });
    any
}

fn def_exprs(d: &Def) -> Vec<&Expr> {
    match d {
        Def::Fn(f) => fn_exprs(f).chain(&f.examples).collect(),
        Def::Test(t) => vec![&t.body],
        Def::Impl(im) => im.fns.iter().flat_map(fn_exprs).collect(),
        Def::Trait(t) => t.methods.iter().flat_map(|m| fn_exprs(&m.sig)).collect(),
        Def::Static(s) => vec![&s.init],
        Def::Type(t) => match &t.body {
            TypeBody::Alias(_, Some(r)) => vec![r],
            TypeBody::Record(fs) => fs.iter().filter_map(|f| f.refine.as_ref()).collect(),
            TypeBody::Sum(vs) => vs.iter().flat_map(|v| v.fields.iter().flatten()).filter_map(|f| f.refine.as_ref()).collect(),
            _ => vec![],
        },
        _ => vec![],
    }
}

fn fn_exprs(f: &FnDef) -> impl Iterator<Item = &Expr> {
    f.params.iter().filter_map(|p| p.refine.as_ref()).chain(&f.pres).chain(&f.posts).chain([&f.body])
}

fn atomic(e: &Expr) -> bool {
    matches!(e.kind, ExprKind::Name(_) | ExprKind::Field(..) | ExprKind::Method { .. } | ExprKind::Call(..) | ExprKind::Index(..) | ExprKind::Int(_) | ExprKind::Placeholder) || matches!(e.kind, ExprKind::Float(x) if x >= 0.0)
}

fn diag(c: &Conv, finite: bool) -> Diag {
    let t = printer::expr(c.recv, 0);
    let t = if atomic(c.recv) { t } else { format!("({t})") };
    let m = c.method;
    let (msg, guard) = if finite {
        (format!("'{t}.{m}' traps when '{t}' is at or above 2^63 or below -2^63, and that isn't ruled out here"), "x.abs < 9.2e18")
    } else {
        (format!("'{t}.{m}' traps when '{t}' is NaN, infinite or outside Int's range, and that isn't ruled out here"), "x.is_finite and x.abs < 9.2e18")
    };
    let lit = matches!(c.recv.kind, ExprKind::Float(_)) || matches!(&c.recv.kind, ExprKind::Unary(UnOp::Neg, x) if matches!(x.kind, ExprKind::Float(_)));
    let hint = if lit {
        format!("use '{t}.checked_{m}' (an Opt[Int]), or an Int literal")
    } else if t.len() <= 24 {
        let g = guard.replace('x', &t);
        format!("use '{t}.checked_{m}' (an Opt[Int]), or guard it: 'if {g} then {t}.{m} else ...'; a 'where' or 'pre' that bounds it works too")
    } else {
        format!("use '.checked_{m}' (an Opt[Int]), or name the value x and guard it: 'if {guard} then x.{m} else ...'; a 'where' or 'pre' that bounds it works too")
    };
    Diag { code: "W_CONVERSION_MAY_TRAP".into(), severity: "warning", def: Some(c.def.clone()), span: [c.e.span.start, c.e.span.end], msg, hint: Some(hint), fix: vec![] }
}

/// A call from unit `unit` (an index into the units, or `None` outside them) to `callee`.
struct Call<'a> {
    callee: &'a str,
    span: (u32, u32),
    unit: Option<usize>,
}

#[derive(Default)]
struct Scan<'a> {
    convs: Vec<(Conv<'a>, Option<usize>)>,
    calls: Vec<Call<'a>>,
    /// Own functions used other than by calling them.
    values: HashSet<&'a str>,
}

impl<'a> Scan<'a> {
    fn walk(&mut self, x: &'a Expr, def: &str, unit: Option<usize>, check: &CheckOutput, fns: &HashSet<&str>) {
        if let Some((recv, method)) = conv_of(x, check) {
            self.convs.push((Conv { def: def.to_string(), e: x, recv, method }, unit));
        }
        let span = (x.span.start, x.span.end);
        match &x.kind {
            ExprKind::Call(f, args) if matches!(&f.kind, ExprKind::Name(n) if fns.contains(n.as_str())) => {
                let ExprKind::Name(n) = &f.kind else { unreachable!() };
                self.calls.push(Call { callee: n, span, unit });
                args.iter().for_each(|a| self.walk(a, def, unit, check, fns));
                return;
            }
            ExprKind::Field(_, n) | ExprKind::Method { name: n, .. } if check.user_methods.contains(&span) => self.calls.push(Call { callee: n, span, unit }),
            ExprKind::Name(n) if fns.contains(n.as_str()) => {
                self.values.insert(n);
            }
            _ => {}
        }
        for c in visit::children(x) {
            self.walk(c, def, unit, check, fns);
        }
    }
}

const DEPTH: usize = 3;

/// The conversions in `m.defs[..own]` that may trap. Code without conversions costs one walk.
pub fn warnings(m: &Module, check: &CheckOutput, own: usize) -> Report {
    let defs = &m.defs[..own.min(m.defs.len())];
    if !defs.iter().any(|d| def_exprs(d).into_iter().any(|x| has_conv(x, check))) {
        return Report::default();
    }
    let fns: HashSet<&str> = defs.iter().filter_map(|d| if let Def::Fn(f) = d { Some(f.name.as_str()) } else { None }).collect();
    let mut units = Vec::new();
    let mut sc = Scan::default();
    let mut open: HashSet<String> = ["main", "core_main", "on_trap"].map(String::from).into();
    for d in defs {
        match d {
            Def::Fn(f) => {
                let u = units.len();
                units.push(Unit::Fn(f));
                fn_exprs(f).for_each(|x| sc.walk(x, &f.name, Some(u), check, &fns));
                for (i, ex) in f.examples.iter().enumerate() {
                    units.push(Unit::Ex(f, i));
                    sc.walk(ex, &f.name, Some(units.len() - 1), check, &fns);
                }
            }
            Def::Test(t) => {
                units.push(Unit::Test(t));
                sc.walk(&t.body, &t.name, Some(units.len() - 1), check, &fns);
            }
            Def::Impl(im) => {
                for f in &im.fns {
                    units.push(Unit::Plain(f));
                    fn_exprs(f).for_each(|x| sc.walk(x, &f.name, Some(units.len() - 1), check, &fns));
                }
            }
            Def::Trait(t) => {
                for f in t.methods.iter().filter(|m| m.default).map(|m| &m.sig) {
                    units.push(Unit::Plain(f));
                    fn_exprs(f).for_each(|x| sc.walk(x, &f.name, Some(units.len() - 1), check, &fns));
                }
            }
            Def::Static(_) | Def::Type(_) => def_exprs(d).into_iter().for_each(|x| sc.walk(x, d.name(), None, check, &fns)),
            Def::Svc(s) => open.extend(s.eps.iter().map(|e| e.handler.clone())),
            _ => {}
        }
    }
    if sc.convs.is_empty() {
        return Report::default();
    }
    if matches!(std::env::var("SSPUR_SMT").as_deref(), Ok("off" | "0")) {
        return Report { diags: vec![], unchecked: sc.convs.len() };
    }
    let fn_of = |u: Option<usize>| match u.map(|u| &units[u]) {
        Some(Unit::Fn(f)) => Some(*f),
        _ => None,
    };
    // A function whose every call is in this module, which isn't C-exportable (ADR 0014), so
    // that its conversions can be proved at its calls.
    let closed = |f: &FnDef| {
        let simple = |t: &Type| ["Int", "F64", "Bool", "Str"].iter().any(|n| is_ty(Some(t), n));
        let exportable = f.tparams.is_empty() && check.fn_types.get(&f.name).is_none_or(|(ps, r)| ps.iter().all(simple) && (simple(r) || is_ty(Some(r), "Unit")));
        !f.public && !exportable && f.ext.is_none() && f.kernel.is_none() && f.interrupt.is_none() && !open.contains(&f.name) && !sc.values.contains(f.name.as_str())
    };
    let mut callers: HashMap<&str, Vec<&Call>> = HashMap::new();
    for c in &sc.calls {
        callers.entry(c.callee).or_default().push(c);
    }
    let mut need: Vec<bool> = vec![false; units.len()];
    let mut frontier: Vec<&FnDef> = vec![];
    for (_, u) in &sc.convs {
        if let Some(u) = *u {
            need[u] = true;
            frontier.extend(fn_of(Some(u)));
        }
    }
    for _ in 0..DEPTH {
        let mut next = vec![];
        for f in frontier.into_iter().filter(|f| closed(f)) {
            for c in callers.get(f.name.as_str()).into_iter().flatten() {
                if let Some(u) = c.unit
                    && !need[u]
                {
                    need[u] = true;
                    next.extend(fn_of(Some(u)));
                }
            }
        }
        frontier = next;
    }
    let (mut enc, _) = encoder(m, check, true);
    for (_, unit) in units.iter().enumerate().filter(|(u, _)| need[*u]) {
        match unit {
            Unit::Fn(f) => enc.function(f),
            Unit::Plain(f) => enc.plain_fn(f),
            Unit::Ex(f, i) => enc.test(&format!("{}.ex{}", f.name, i + 1), &f.examples[*i]),
            Unit::Test(t) => enc.test(&t.name, &t.body),
        }
    }
    let mut an = enc.an;
    let mut solver = Solver::new();
    houdini(&mut an, &mut solver);
    let scripts = |span: Span, kind: &str| -> Option<Vec<String>> {
        let s = an.sites.get(&(span.start, span.end, kind.to_string()))?;
        s.queries.iter().map(|q| q.as_ref().map(|q| an.script(q))).collect::<Option<Vec<_>>>().filter(|v| !v.is_empty())
    };
    let qs = an.sites.iter().filter(|(k, _)| k.2.starts_with("conv") && !k.2.starts_with("conv-")).flat_map(|(_, s)| s.queries.iter().flatten());
    solve_new(&mut solver, qs.map(|q| an.script(q)));
    let call_unit: HashMap<(u32, u32), Option<usize>> = sc.calls.iter().map(|c| (c.span, c.unit)).collect();
    let mut out = Report::default();
    let mut z3 = None;
    for (c, u) in &sc.convs {
        let mut cov = Cover { solver: &mut solver, scripts: &scripts, closed: &closed, callers: &callers, call_unit: &call_unit, fn_of: &fn_of };
        if cov.safe(c.e.span, &mut vec![], fn_of(*u), 0) {
            continue;
        }
        let ss = scripts(c.e.span, "conv");
        if ss.iter().flatten().any(|s| !solver.is_cached(s)) && !*z3.get_or_insert_with(|| solver.available()) {
            out.unchecked += 1;
            continue;
        }
        let finite = all_unsat(&mut solver, &scripts(c.e.span, "conv-finite"));
        out.diags.push(diag(c, finite));
    }
    out
}

type Scripts<'s> = dyn Fn(Span, &str) -> Option<Vec<String>> + 's;

struct Cover<'s, 'a> {
    solver: &'s mut Solver,
    scripts: &'s Scripts<'s>,
    closed: &'s dyn Fn(&FnDef) -> bool,
    callers: &'s HashMap<&'a str, Vec<&'a Call<'a>>>,
    call_unit: &'s HashMap<(u32, u32), Option<usize>>,
    fn_of: &'s dyn Fn(Option<usize>) -> Option<&'a FnDef>,
}

impl Cover<'_, '_> {
    /// Whether the site at `span`, reached through the calls `chain` (outermost first) from
    /// function `outer`, can't trap: proved there, or `outer` is closed and it is proved at
    /// every call of `outer`.
    fn safe(&mut self, span: Span, chain: &mut Vec<(u32, u32)>, outer: Option<&FnDef>, depth: usize) -> bool {
        let kind: String = std::iter::once("conv".to_string()).chain(chain.iter().map(|(s, e)| format!("@{s}-{e}"))).collect();
        if all_unsat(self.solver, &(self.scripts)(span, &kind)) {
            return true;
        }
        let Some(f) = outer.filter(|f| depth < DEPTH && (self.closed)(f)) else { return false };
        let calls: Vec<(u32, u32)> = self.callers.get(f.name.as_str()).into_iter().flatten().map(|c| c.span).collect();
        !calls.is_empty()
            && calls.into_iter().all(|c| {
                let outer = self.call_unit.get(&c).copied().flatten();
                chain.insert(0, c);
                let ok = self.safe(span, chain, (self.fn_of)(outer), depth + 1);
                chain.remove(0);
                ok
            })
    }
}

fn all_unsat(solver: &mut Solver, ss: &Option<Vec<String>>) -> bool {
    ss.as_ref().is_some_and(|v| v.iter().all(|s| solver.check(s, &[]) == Answer::Unsat))
}

/// Keeps the loop invariant candidates that hold at loop entry and after every assignment,
/// assuming the others kept (Houdini), and lets every query of their function assume them.
fn houdini(an: &mut Analysis, solver: &mut Solver) {
    let mut alive = vec![true; an.cands.len()];
    loop {
        for f in &mut an.fns {
            f.assumed.clear();
        }
        for (c, _) in an.cands.iter().zip(&alive).filter(|(_, a)| **a) {
            an.fns[c.fun].assumed.push(c.b.clone());
        }
        let live: Vec<usize> = (0..an.cands.len()).filter(|&i| alive[i]).collect();
        solve_new(solver, live.iter().flat_map(|&i| an.cands[i].obls.iter().flatten()).map(|q| an.script(q)));
        let mut changed = false;
        for i in live {
            let ok = an.cands[i].obls.iter().all(|q| q.as_ref().is_some_and(|q| solver.check(&an.script(q), &[]) == Answer::Unsat));
            if !ok {
                alive[i] = false;
                changed = true;
            }
        }
        if !changed {
            return;
        }
    }
}

/// Answers the scripts the cache doesn't have yet, on every core.
fn solve_new(solver: &mut Solver, scripts: impl Iterator<Item = String>) {
    let miss: Vec<String> = scripts.filter(|s| !solver.is_cached(s)).collect();
    if miss.is_empty() {
        return;
    }
    let (_, put_off) = collecting(1, || miss.iter().for_each(|s| drop(solver.check(s, &[]))));
    if !put_off.is_empty() {
        solve_all(&put_off, std::thread::available_parallelism().map_or(4, |n| n.get()));
    }
}
