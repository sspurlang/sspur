use super::prove::Fact;
use super::*;

pub(super) const BLOCK: i64 = 64;
const K: i128 = 1 << 30;
const SMALL: i128 = 1 << 56;

pub(super) struct Vecx {
    flag: String,
    src: String,
    lo: String,
    hi: String,
    pre: Vec<String>,
    flagged: bool,
    bad: bool,
    assumed: (usize, usize),
    spec: bool,
}

pub(super) struct VBody {
    pub body: String,
    pub xf: String,
    pub pre: Vec<String>,
    pub flagged: bool,
    pub small: bool,
}

pub(super) struct VLoop<'s> {
    pub i: &'s str,
    pub head: &'s str,
    pub seq: &'s str,
    pub vb: &'s VBody,
    pub init: String,
    pub step: String,
    pub commit: String,
    pub guard: String,
    pub direct: Option<String>,
}

impl VLoop<'_> {
    pub(super) fn emit(&self, lo: &str, hi: &str) -> String {
        let VLoop { i, head, seq, vb, .. } = self;
        let body = &vb.body;
        if let Some(d) = &self.direct {
            return format!("for (int64_t {i} = {lo}; {i} < {hi}; {i}++) {{ uint64_t vf_ = 0; {head}{body}{d}(void)vf_; }} ");
        }
        let mut conds = vec![format!("vm_ < 8 + ((vb_ - ({lo})) >> 9)")];
        if !self.guard.is_empty() {
            conds.push(self.guard.clone());
        }
        conds.extend(vb.pre.iter().cloned());
        format!(
            "for (int64_t vb_ = {lo}; vb_ < {hi}; vb_ += {BLOCK}) {{ int64_t ve_ = {hi} - vb_ > {BLOCK} ? vb_ + {BLOCK} : {hi}; \
if (LIKELY_({})) {{ uint64_t vf_ = 0; {}for (int64_t {i} = vb_; {i} < ve_; {i}++) {{ {head}{body}{}}} if (LIKELY_(!vf_)) {{ {}continue; }} vm_++; }} \
for (int64_t {i} = vb_; {i} < ve_; {i}++) {{ {head}{seq}}} }} ",
            conds.join(" && "),
            self.init,
            self.step,
            self.commit
        )
    }
}

fn mul_range(c: i128) -> (i128, i128) {
    let (min, max) = (i64::MIN as i128, i64::MAX as i128);
    if c > 0 {
        (-(-min / c), max / c)
    } else {
        let d = -c;
        (-(max / d), (-min / d).min(max))
    }
}

fn narrow(r: Iv) -> bool {
    r.0 >= i32::MIN as i128 && r.1 <= i32::MAX as i128
}

fn within(r: Iv, k: i128) -> bool {
    r.0 >= -k && r.1 < k
}

fn simple_var(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn names(e: &Expr, out: &mut Vec<String>) {
    let mut bound = Vec::new();
    visit::walk_expr(e, &mut |x| {
        match &x.kind {
            ExprKind::Name(n) => out.push(n.clone()),
            ExprKind::Block(stmts) => bound.extend(stmts.iter().filter_map(|s| if let Stmt::Let(Pat::Bind(n), _) = s { Some(n.clone()) } else { None })),
            _ => {}
        }
        true
    });
    out.retain(|n| !bound.contains(n));
}

impl Cx<'_> {
    pub(super) fn vstage(&self, f: &Expr) -> bool {
        matches!(&f.kind, ExprKind::Lambda { params, body, .. } if params.len() == 1 && self.vshape(body))
    }

    fn vshape(&self, e: &Expr) -> bool {
        if self.check.user_methods.contains(&(e.span.start, e.span.end)) || !self.ty(e).is_ok_and(|t| scalar(&t)) {
            return false;
        }
        let list = |b: &Expr| matches!(b.kind, ExprKind::Name(_)) && self.ty(b).is_ok_and(|t| elem(&t, "List").is_some_and(|e| scalar(&e)));
        let builtin = |recv: &Expr, name: &str| {
            let rt = self.ty(recv);
            (name == "len" && list(recv))
                || (name == "to_f64" && rt.as_ref().is_ok_and(|t| is(t, "Int")) && self.vshape(recv))
                || (matches!(name, "abs" | "sqrt" | "floor" | "round") && rt.as_ref().is_ok_and(|t| is(t, "F64")) && self.vshape(recv))
        };
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Unit | ExprKind::Name(_) | ExprKind::Placeholder => true,
            ExprKind::Binary(op, a, b) => *op != BinOp::Pow && self.vshape(a) && self.vshape(b),
            ExprKind::Unary(_, x) => self.vshape(x),
            ExprKind::If(c, a, b) => self.vshape(c) && self.vshape(a) && b.as_ref().is_some_and(|b| self.vshape(b)),
            ExprKind::Index(b, i) => list(b) && self.vshape(i),
            ExprKind::Block(stmts) => {
                let n = stmts.len();
                stmts.iter().enumerate().all(|(k, s)| match s {
                    Stmt::Let(Pat::Bind(_), x) => k + 1 < n && self.vshape(x),
                    Stmt::Expr(x) => k + 1 == n && self.vshape(x),
                    _ => false,
                })
            }
            ExprKind::Field(recv, name) => builtin(recv, name),
            ExprKind::Method { recv, name, args, .. } if args.is_empty() => builtin(recv, name),
            _ => false,
        }
    }

    pub(super) fn vchecked(&mut self, op: BinOp, x: &str, y: &str, ra: Iv, rb: Iv) -> Option<String> {
        let v = self.vec.as_mut()?;
        let f = v.flag.clone();
        let point = |r: Iv| (r.0 == r.1).then_some(r.0);
        let wrap = |s: &str| format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; (int64_t)((uint64_t)a_ {s} (uint64_t)b_); }})");
        if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) && iv_safe(op, ra, rb) {
            return Some(match op {
                BinOp::Mul if narrow(ra) && narrow(rb) => format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; (int64_t)(int32_t)a_ * (int64_t)(int32_t)b_; }})"),
                _ => wrap(op.symbol()),
            });
        }
        let s = match op {
            BinOp::Add => format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; int64_t r_ = (int64_t)((uint64_t)a_ + (uint64_t)b_); {f} |= (uint64_t)((a_ ^ r_) & (b_ ^ r_)) >> 63; r_; }})"),
            BinOp::Sub => format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; int64_t r_ = (int64_t)((uint64_t)a_ - (uint64_t)b_); {f} |= (uint64_t)((a_ ^ b_) & (a_ ^ r_)) >> 63; r_; }})"),
            BinOp::Mul => match point(rb).filter(|k| *k != 0).map(|k| (x, y, k)).or_else(|| point(ra).filter(|k| *k != 0).map(|k| (y, x, k))) {
                Some((a, b, k)) => {
                    let (lo, hi) = mul_range(k);
                    let r = if point(rb).is_some() { ra } else { rb };
                    if r.0 >= lo && r.1 > hi {
                        format!("({{ int64_t a_ = {a}; int64_t b_ = {b}; {f} |= a_ > {}; (int64_t)((uint64_t)a_ * (uint64_t)b_); }})", lit(hi as i64))
                    } else if r.1 <= hi && r.0 < lo {
                        format!("({{ int64_t a_ = {a}; int64_t b_ = {b}; {f} |= a_ < {}; (int64_t)((uint64_t)a_ * (uint64_t)b_); }})", lit(lo as i64))
                    } else {
                        format!(
                            "({{ int64_t a_ = {a}; int64_t b_ = {b}; {f} |= (uint64_t)a_ - (uint64_t){} > {}ULL; (int64_t)((uint64_t)a_ * (uint64_t)b_); }})",
                            lit(lo as i64),
                            (hi - lo) as u64
                        )
                    }
                }
                None if v.spec => {
                    v.bad = true;
                    return None;
                }
                None => {
                    let mut cs = Vec::new();
                    if !narrow(ra) {
                        cs.push("((uint64_t)a_ + 0x80000000ULL > 0xFFFFFFFFULL)");
                    }
                    if !narrow(rb) {
                        cs.push("((uint64_t)b_ + 0x80000000ULL > 0xFFFFFFFFULL)");
                    }
                    format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; {f} |= {}; (int64_t)(int32_t)a_ * (int64_t)(int32_t)b_; }})", cs.join(" | "))
                }
            },
            BinOp::Div | BinOp::Rem if point(rb).is_some_and(|k| k != 0 && k != -1) => {
                if let Some(u) = unsigned_div(op, x, y, ra, rb) {
                    return Some(u);
                }
                return Some(format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; a_ {} b_; }})", op.symbol()));
            }
            _ => {
                v.bad = true;
                return None;
            }
        };
        v.flagged = true;
        Some(s)
    }

    pub(super) fn spec_proven(&self) -> bool {
        self.vec.as_ref().is_some_and(|v| v.spec)
    }

    pub(super) fn vneg(&mut self, x: &str, proven: bool) -> Option<String> {
        let v = self.vec.as_mut()?;
        if proven {
            return Some(format!("((int64_t)(0ULL - (uint64_t)({x})))"));
        }
        v.flagged = true;
        Some(format!("({{ int64_t t_ = {x}; {} |= t_ == INT64_MIN; (int64_t)(0ULL - (uint64_t)t_); }})", v.flag))
    }

    pub(super) fn vindex(&mut self, a: &Expr, i: &Expr, av: &str, iv: &str) -> Option<String> {
        let (a0, a1) = self.vec.as_ref()?.assumed;
        let saved = self.know.facts.clone();
        self.know.facts.drain(a0..a1.min(self.know.facts.len()));
        let safe = self.index_safe(a, i);
        self.know.facts = saved;
        if safe {
            return Some(format!("(({av}).data[{iv}])"));
        }
        let v = self.vec.as_mut()?;
        if iv != v.src || !simple_var(av.strip_prefix("pc_->").unwrap_or(av)) {
            v.bad = true;
            return None;
        }
        let p = format!("({}) >= 0 && ({}) < {av}.len", v.lo, v.hi);
        if !v.pre.contains(&p) {
            v.pre.push(p);
        }
        Some(format!("({av}.data[{iv}])"))
    }

    fn stage_iv(&mut self, f: &Expr, x: &str, et: &Type) -> Iv {
        let ExprKind::Lambda { params, body, .. } = &f.kind else { return FULL };
        let mut scope = HashMap::new();
        scope.insert(params[0].clone(), (x.to_string(), et.clone()));
        self.scopes.push(scope);
        let r = self.range(body);
        self.scopes.pop();
        r
    }

    fn vstage_body(&mut self, stages: &[(&str, &Expr, &Expr)], mut x: String, mut et: Type, assume: bool) -> G<(String, String)> {
        let mut body = String::from("uint64_t mk_ = 1; ");
        let mut caps = Vec::new();
        for (_, f, _) in stages {
            if let ExprKind::Lambda { params, body, .. } = &f.kind {
                let mut ns = Vec::new();
                names(body, &mut ns);
                caps.extend(ns.into_iter().filter(|n| !params.contains(n)));
            }
        }
        if assume {
            if is(&et, "Int") && !within(self.var_iv(&x), K) {
                self.know.facts.push(Fact::Range(x.clone(), (-K, K - 1)));
                let v = self.vec.as_mut().ok_or("no vec")?;
                if v.src == x {
                    v.pre.push(format!("({}) >= {} && ({}) < {}", v.lo, -K, v.hi, K));
                } else {
                    v.flagged = true;
                    write!(body, "vf_ |= (uint64_t){x} + {K}ULL >= {}ULL; ", 2 * K).unwrap();
                }
            }
            let mut seen = HashSet::new();
            for n in caps {
                let Some((c, t)) = self.lookup(&n) else { continue };
                if !is(&t, "Int") || !simple_var(&c) || !seen.insert(c.clone()) || within(self.var_iv(&c), K) {
                    continue;
                }
                self.know.facts.push(Fact::Range(c.clone(), (-K, K - 1)));
                self.vec.as_mut().ok_or("no vec")?.pre.push(format!("{c} >= {} && {c} < {K}", -K));
            }
        }
        for (kind, f, stage) in stages {
            let fl = self.fresh("vf");
            let top = self.know.facts.len();
            if let Some(v) = self.vec.as_mut() {
                v.flag = fl.clone();
                v.assumed.1 = top;
            }
            let v = self.apply(f, vec![(x.clone(), et.clone())])?;
            if *kind == "map" {
                let nx = self.fresh("x");
                write!(body, "uint64_t {fl} = 0; __auto_type {nx} = {v}; vf_ |= {fl} & mk_; ").unwrap();
                let st = self.ty(stage).ok().and_then(|st| elem(&st, "List")).ok_or("bad fused map type")?;
                if is(&st, "Int") {
                    let iv = self.stage_iv(f, &x, &et);
                    if iv != FULL {
                        self.know.facts.push(Fact::Range(nx.clone(), iv));
                    }
                }
                x = nx;
                et = st;
            } else {
                let p = self.fresh("vp");
                write!(body, "uint64_t {fl} = 0; int64_t {p} = {v}; vf_ |= {fl} & mk_; mk_ &= {p} != 0; ").unwrap();
            }
        }
        Ok((body, x))
    }

    #[allow(clippy::too_many_arguments)]
    fn vec_try(&mut self, stages: &[(&str, &Expr, &Expr)], x: &str, et0: &Type, src: &str, lo: &str, hi: &str, assume: bool) -> Option<VBody> {
        let snap = self.snapshot();
        let decls = self.fn_decls.len();
        let mark = self.know.facts.len();
        self.vec = Some(Vecx { flag: String::new(), src: src.into(), lo: lo.into(), hi: hi.into(), pre: Vec::new(), flagged: false, bad: false, assumed: (mark, mark), spec: false });
        let r = self.vstage_body(stages, x.to_string(), et0.clone(), assume);
        let small = r.as_ref().is_ok_and(|(_, xf)| within(self.var_iv(xf), SMALL));
        self.know.facts.truncate(mark);
        let v = self.vec.take()?;
        match r {
            Ok((body, xf)) if !v.bad && !body.contains("TRAPV") && !body.contains("sspur_") && self.fn_decls.len() == decls => Some(VBody { body, xf, pre: v.pre, flagged: v.flagged, small }),
            _ => {
                self.fn_decls.truncate(decls);
                self.vrestore(snap);
                None
            }
        }
    }

    fn vrestore(&mut self, snap: Snapshot) {
        let keep = (std::mem::take(&mut self.know), std::mem::take(&mut self.catch_stack), std::mem::take(&mut self.in_progress));
        self.restore(snap);
        (self.know, self.catch_stack, self.in_progress) = keep;
    }

    pub(super) fn vec_body(&mut self, stages: &[(&str, &Expr, &Expr)], x: &str, et0: &Type, src: &str, lo: &str, hi: &str) -> Option<VBody> {
        let snap = self.snapshot();
        let plain = self.vec_try(stages, x, et0, src, lo, hi, false);
        if plain.as_ref().is_some_and(|b| !b.flagged) {
            return plain;
        }
        if plain.is_some() {
            self.vrestore(snap.clone());
        }
        self.vec_try(stages, x, et0, src, lo, hi, true).or_else(|| self.vec_try(stages, x, et0, src, lo, hi, false))
    }
}

const SPEC_OPS: usize = 6;

impl Cx<'_> {
    fn spec_shape(&self, e: &Expr, ops: &mut usize) -> bool {
        let int = |x: &Expr| self.ty(x).is_ok_and(|t| is(&t, "Int"));
        match &e.kind {
            ExprKind::Int(_) => true,
            ExprKind::Name(n) => self.lookup(n).is_some() && int(e),
            ExprKind::Binary(op @ (BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem), a, b) => {
                *ops += 1;
                let k = |x: &Expr| matches!(x.kind, ExprKind::Int(k) if k != 0 && k != -1);
                (*op != BinOp::Mul || k(a) || k(b)) && (!matches!(op, BinOp::Div | BinOp::Rem) || k(b)) && int(a) && int(b) && self.spec_shape(a, ops) && self.spec_shape(b, ops)
            }
            ExprKind::Unary(UnOp::Neg, x) => {
                *ops += 1;
                int(x) && self.spec_shape(x, ops)
            }
            _ => false,
        }
    }

    /// A small `if` over Int with overflow checks as a select: both arms wrap and set exact flags,
    /// and only the chosen arm's flag traps (every flagged check is an overflow).
    pub(super) fn spec_select(&mut self, c: &Expr, a: &Expr, b: &Expr, cv: &str) -> Option<String> {
        if self.vec.is_some() || !self.ty(a).is_ok_and(|t| is(&t, "Int")) {
            return None;
        }
        let mut ops = 0;
        if !self.spec_shape(a, &mut ops) || !self.spec_shape(b, &mut ops) || ops > SPEC_OPS {
            return None;
        }
        let mark = self.know.facts.len();
        let arm = |cx: &mut Self, x: &Expr, pos: bool| -> Option<(String, String, bool)> {
            let fl = cx.fresh("sf");
            cx.vec = Some(Vecx { flag: fl.clone(), src: String::new(), lo: String::new(), hi: String::new(), pre: Vec::new(), flagged: false, bad: false, assumed: (mark, mark), spec: true });
            let r = cx.under(c, pos, |cx| cx.expr(x));
            let v = cx.vec.take()?;
            match r {
                Ok(s) if !v.bad && v.pre.is_empty() && !s.contains("TRAPV") && !s.contains("sspur_") => Some((fl, s, v.flagged)),
                _ => None,
            }
        };
        let (fa, av, ga) = arm(self, a, true)?;
        let (fb, bv, gb) = arm(self, b, false)?;
        if !ga && !gb {
            return None;
        }
        let (sc, sa, sb) = (self.fresh("sc"), self.fresh("sa"), self.fresh("sb"));
        Some(format!(
            "({{ int64_t {sc} = {cv}; uint64_t {fa} = 0; uint64_t {fb} = 0; int64_t {sa} = {av}; int64_t {sb} = {bv}; if (UNLIKELY(({sc} ? {fa} : {fb}) != 0)) TRAPV({T_OVERFLOW}, 0, 0); {sc} ? {sa} : {sb}; }})"
        ))
    }
}
