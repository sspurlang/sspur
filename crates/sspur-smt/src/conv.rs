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
    /// Each conversion, its unit and the index of its definition.
    convs: Vec<(Conv<'a>, Option<usize>, usize)>,
    cur: usize,
    calls: Vec<Call<'a>>,
    /// Own functions used other than by calling them.
    values: HashSet<&'a str>,
}

impl<'a> Scan<'a> {
    fn walk(&mut self, x: &'a Expr, def: &str, unit: Option<usize>, check: &CheckOutput, fns: &HashSet<&str>) {
        if let Some((recv, method)) = conv_of(x, check) {
            self.convs.push((Conv { def: def.to_string(), e: x, recv, method }, unit, self.cur));
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
    report(outcomes(m, check, own, None))
}

#[derive(Clone, Copy, PartialEq)]
enum Outcome {
    Proved,
    /// A warning; whether the value is at least proved finite.
    Warn(bool),
    Unchecked,
}

fn report(os: Vec<(usize, Conv, Outcome)>) -> Report {
    let mut out = Report::default();
    for (_, c, o) in os {
        match o {
            Outcome::Proved => {}
            Outcome::Warn(finite) => out.diags.push(diag(&c, finite)),
            Outcome::Unchecked => out.unchecked += 1,
        }
    }
    out
}

/// Every conversion of the definitions `only` lets through (all without it), with its definition's
/// index and what the proof found.
fn outcomes<'a>(m: &'a Module, check: &CheckOutput, own: usize, only: Option<&[bool]>) -> Vec<(usize, Conv<'a>, Outcome)> {
    let defs = &m.defs[..own.min(m.defs.len())];
    let fns: HashSet<&str> = defs.iter().filter_map(|d| if let Def::Fn(f) = d { Some(f.name.as_str()) } else { None }).collect();
    let mut units = Vec::new();
    let mut sc = Scan::default();
    let mut open: HashSet<String> = ["main", "core_main", "on_trap"].map(String::from).into();
    for (di, d) in defs.iter().enumerate() {
        sc.cur = di;
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
    if let Some(o) = only {
        sc.convs.retain(|c| o[c.2]);
    }
    if sc.convs.is_empty() {
        return vec![];
    }
    if matches!(std::env::var("SSPUR_SMT").as_deref(), Ok("off" | "0")) {
        return sc.convs.into_iter().map(|(c, _, d)| (d, c, Outcome::Unchecked)).collect();
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
    for (_, u, _) in &sc.convs {
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
    let mut z3 = None;
    let mut out = Vec::new();
    for (c, u, d) in &sc.convs {
        let mut cov = Cover { solver: &mut solver, scripts: &scripts, closed: &closed, callers: &callers, call_unit: &call_unit, fn_of: &fn_of };
        let o = if cov.safe(c.e.span, &mut vec![], fn_of(*u), 0) {
            Outcome::Proved
        } else if scripts(c.e.span, "conv").iter().flatten().any(|s| !solver.is_cached(s)) && !*z3.get_or_insert_with(|| solver.available()) {
            Outcome::Unchecked
        } else {
            Outcome::Warn(all_unsat(&mut solver, &scripts(c.e.span, "conv-finite")))
        };
        out.push((*d, Conv { def: c.def.clone(), e: c.e, recv: c.recv, method: c.method }, o));
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

/// The conversions of one definition in the order `outcomes` finds them.
fn def_convs<'a>(d: &'a Def, check: &CheckOutput) -> Vec<Conv<'a>> {
    fn walk<'a>(x: &'a Expr, def: &str, check: &CheckOutput, out: &mut Vec<Conv<'a>>) {
        if let Some((recv, method)) = conv_of(x, check) {
            out.push(Conv { def: def.to_string(), e: x, recv, method });
        }
        for c in visit::children(x) {
            walk(c, def, check, out);
        }
    }
    let mut out = Vec::new();
    match d {
        Def::Impl(im) => im.fns.iter().for_each(|f| fn_exprs(f).for_each(|x| walk(x, &f.name, check, &mut out))),
        Def::Trait(t) => t.methods.iter().filter(|m| m.default).for_each(|m| fn_exprs(&m.sig).for_each(|x| walk(x, &m.sig.name, check, &mut out))),
        _ => def_exprs(d).into_iter().for_each(|x| walk(x, d.name(), check, &mut out)),
    }
    out
}

/// Bumped whenever the encoding or the proof changes what it can prove.
const PROOF_VERSION: &str = "1";

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// What a definition's conversion proofs read, as a cache key: its content hash, which covers
/// everything it calls and the types it uses, and, for a function that isn't `pub`, the hashes
/// of every definition whose text names it, three callers out (a superset of the callers whose
/// context `outcomes` may use). `None` when a hash is missing.
fn proof_key(defs: &[Def], i: usize, src: &str, spans: &[(usize, usize, usize)], hashes: &BTreeMap<String, String>, timeout: &str) -> Option<String> {
    let mut rel: BTreeSet<usize> = BTreeSet::new();
    let mut frontier = vec![i];
    for _ in 0..DEPTH {
        let mut next = vec![];
        for f in frontier {
            let Def::Fn(fd) = &defs[f] else { continue };
            if fd.public {
                continue;
            }
            let name = fd.name.rsplit("__").next().unwrap_or(&fd.name);
            let b = src.as_bytes();
            for (at, _) in src.match_indices(name) {
                let end = at + name.len();
                if (at > 0 && is_ident(b[at - 1])) || (end < b.len() && is_ident(b[end])) {
                    continue;
                }
                let k = spans.partition_point(|s| s.0 <= at);
                if k > 0 && at < spans[k - 1].1 && rel.insert(spans[k - 1].2) {
                    next.push(spans[k - 1].2);
                }
            }
        }
        frontier = next;
    }
    let mut h = blake3::Hasher::new();
    for t in [PROOF_VERSION, env!("CARGO_PKG_VERSION"), timeout, defs[i].name(), hashes.get(defs[i].name())?] {
        h.update(t.as_bytes());
        h.update(b"\n");
    }
    for r in rel {
        let n = defs[r].name();
        h.update(n.as_bytes());
        h.update(b"\0");
        h.update(hashes.get(n)?.as_bytes());
        h.update(b"\n");
    }
    Some(h.finalize().to_hex()[..32].to_string())
}

/// A cached result: `Some(vec![])` when every conversion of the definition is proved, else the
/// ordinal of each one that isn't and whether it is at least finite.
fn cached(dir: &std::path::Path, key: &str) -> Option<Vec<(usize, bool)>> {
    let d = dir.join(&key[..2]);
    if d.join(format!("{key}.ok")).exists() {
        return Some(vec![]);
    }
    let text = std::fs::read_to_string(d.join(format!("{key}.w"))).ok()?;
    text.lines().map(|l| l.split_once(' ').and_then(|(a, b)| Some((a.parse().ok()?, b == "1")))).collect()
}

fn store(dir: &std::path::Path, key: &str, warns: &[(usize, bool)]) {
    let d = dir.join(&key[..2]);
    if std::fs::create_dir_all(&d).is_err() {
        return;
    }
    if warns.is_empty() {
        let _ = std::fs::File::create(d.join(format!("{key}.ok")));
        return;
    }
    let text: String = warns.iter().map(|(o, f)| format!("{o} {}\n", u8::from(*f))).collect();
    let tmp = d.join(format!("{key}.{}.tmp", std::process::id()));
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, d.join(format!("{key}.w")));
    }
}

/// `warnings`, with each definition's result cached under `proof_key` in `conv/` of the cache
/// directory: a definition whose proof inputs are unchanged costs a stat, and only the others are
/// encoded and proved. `src` holds the definitions' text at their spans.
pub fn warnings_cached(m: &Module, check: &CheckOutput, own: usize, src: &str, hashes: &BTreeMap<String, String>) -> Report {
    let defs = &m.defs[..own.min(m.defs.len())];
    let mut spans: Vec<(usize, usize, usize)> = defs.iter().enumerate().map(|(i, d)| (d.span().start as usize, d.span().end as usize, i)).collect();
    spans.sort_unstable();
    let cands = candidates(defs, src, &spans);
    if cands.is_empty() {
        return Report::default();
    }
    let Some(dir) = sspur_syntax::cache_root().map(|c| c.join("conv")) else { return warnings(m, check, own) };
    let timeout = std::env::var("SSPUR_SMT_TIMEOUT").unwrap_or_default();
    let mut per: Vec<Vec<Diag>> = (0..defs.len()).map(|_| vec![]).collect();
    let mut miss = vec![false; defs.len()];
    let mut keys: Vec<(usize, String)> = vec![];
    for &i in &cands {
        let key = proof_key(defs, i, src, &spans, hashes, &timeout);
        match key.as_deref().and_then(|k| cached(&dir, k)) {
            Some(ws) if ws.is_empty() => {}
            Some(ws) => {
                let cs = def_convs(&defs[i], check);
                match ws.iter().map(|&(o, finite)| cs.get(o).map(|c| diag(c, finite))).collect::<Option<Vec<_>>>() {
                    Some(ds) => per[i] = ds,
                    None => miss[i] = true,
                }
            }
            None => miss[i] = true,
        }
        if let Some(k) = key.filter(|_| miss[i]) {
            keys.push((i, k));
        }
    }
    let mut unchecked = 0;
    if miss.contains(&true) {
        let os = outcomes(m, check, own, Some(&miss));
        for (i, k) in keys {
            let mine: Vec<Outcome> = os.iter().filter(|o| o.0 == i).map(|o| o.2).collect();
            if !mine.contains(&Outcome::Unchecked) && mine.len() == def_convs(&defs[i], check).len() {
                store(&dir, &k, &mine.iter().enumerate().filter_map(|(n, o)| if let Outcome::Warn(f) = o { Some((n, *f)) } else { None }).collect::<Vec<_>>());
            }
        }
        for (i, c, o) in os {
            match o {
                Outcome::Proved => {}
                Outcome::Warn(finite) => per[i].push(diag(&c, finite)),
                Outcome::Unchecked => unchecked += 1,
            }
        }
    }
    Report { diags: per.into_iter().flatten().collect(), unchecked }
}

/// The definitions that may have a conversion: their text has `.round`, `.floor`, `.ceil` or
/// `.trunc` (space after the dot allowed), found in one pass over the source, or their span isn't
/// in it.
fn candidates(defs: &[Def], src: &str, spans: &[(usize, usize, usize)]) -> Vec<usize> {
    let mut out: Vec<usize> = spans.iter().filter(|s| s.0 >= s.1 || s.1 > src.len()).map(|s| s.2).collect();
    let end = spans.iter().map(|s| s.1).filter(|&e| e <= src.len()).max().unwrap_or(0);
    let b = &src.as_bytes()[..end];
    for (at, _) in src[..end].match_indices('.') {
        let mut j = at + 1;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let r = &b[j..];
        if !float::CONV.iter().any(|n| r.starts_with(n.as_bytes()) && r.get(n.len()).is_none_or(|&c| !is_ident(c))) {
            continue;
        }
        let k = spans.partition_point(|s| s.0 <= at);
        if k > 0 && at < spans[k - 1].1 && !out.contains(&spans[k - 1].2) {
            out.push(spans[k - 1].2);
        }
    }
    out.retain(|&i| !matches!(defs[i], Def::Use(_) | Def::Effect(_) | Def::Store(_) | Def::Svc(_)));
    out.sort_unstable();
    out
}

/// Whether `src` has `.round`, `.floor`, `.ceil` or `.trunc` (space after the dot allowed).
fn conv_text(src: &str) -> bool {
    let b = src.as_bytes();
    src.match_indices('.').any(|(at, _)| {
        let mut j = at + 1;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let r = &b[j..];
        float::CONV.iter().any(|n| r.starts_with(n.as_bytes()) && r.get(n.len()).is_none_or(|&c| !is_ident(c)))
    })
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match (c, c == '\\') {
            (_, true) => match it.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(x) => out.push(x),
                None => {}
            },
            _ => out.push(c),
        }
    }
    out
}

fn read_program(path: &std::path::Path) -> Option<Vec<Diag>> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let [def, a, b, msg, hint] = f.as_slice() else { return None };
            Some(Diag { code: "W_CONVERSION_MAY_TRAP".into(), severity: "warning", def: Some(unesc(def)), span: [a.parse().ok()?, b.parse().ok()?], msg: unesc(msg), hint: Some(unesc(hint)), fix: vec![] })
        })
        .collect()
}

/// `sspur check`'s conversion warnings, cached for the whole program under `conv/prog/` by a hash
/// of its linked source (which holds every dependency's): a program checked before costs one hash
/// of its text and one file read, with no encoding and no Z3.
pub fn check_program(m: &Module, check: &CheckOutput, own: usize, src: &str, hashes: &BTreeMap<String, String>) -> Report {
    let defs = &m.defs[..own.min(m.defs.len())];
    let end = defs.iter().map(|d| d.span().end as usize).filter(|&e| e <= src.len()).max().unwrap_or(0);
    let spans_ok = defs.iter().all(|d| d.span().end as usize <= src.len());
    if src.is_empty() || (spans_ok && !conv_text(&src[..end])) {
        return if src.is_empty() { warnings(m, check, own) } else { Report::default() };
    }
    let Some(dir) = sspur_syntax::cache_root().map(|c| c.join("conv").join("prog")) else { return warnings(m, check, own) };
    let mut h = blake3::Hasher::new();
    for t in [PROOF_VERSION, env!("CARGO_PKG_VERSION"), &std::env::var("SSPUR_SMT_TIMEOUT").unwrap_or_default(), &own.to_string()] {
        h.update(t.as_bytes());
        h.update(b"\n");
    }
    h.update(src.as_bytes());
    let key = h.finalize().to_hex()[..32].to_string();
    let path = dir.join(&key[..2]).join(&key);
    if let Some(diags) = read_program(&path) {
        return Report { diags, unchecked: 0 };
    }
    let r = warnings_cached(m, check, own, src, hashes);
    if r.unchecked == 0 && std::fs::create_dir_all(path.parent().unwrap()).is_ok() {
        let text: String = r.diags.iter().map(|d| format!("{}\t{}\t{}\t{}\t{}\n", esc(d.def.as_deref().unwrap_or("")), d.span[0], d.span[1], esc(&d.msg), esc(d.hint.as_deref().unwrap_or("")))).collect();
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
    r
}
