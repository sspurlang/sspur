use super::*;
use sspur_check::kernel::{has_barrier, AOp, KExpr, KFn, KIntr, KStmt, KTy, Kernel};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tgt {
    C,
    Msl,
    Ocl,
}

pub(super) struct Em<'a> {
    k: &'a Kernel,
    t: Tgt,
    out: String,
    n: usize,
    ind: usize,
    mask: bool,
    narrow: bool,
    sync: bool,
    lane: bool,
}

const FLT_TINY: &str = "2.3509887e-38f";

fn lit(t: KTy, bits: u64, tgt: Tgt) -> String {
    match t {
        KTy::F32 => {
            let x = f32::from_bits(bits as u32);
            if x.is_nan() {
                return "(0.0f/0.0f)".into();
            }
            if x.is_infinite() {
                return if x > 0.0 { "(1.0f/0.0f)".into() } else { "(-1.0f/0.0f)".into() };
            }
            format!("{x:e}f")
        }
        KTy::F64 => {
            let x = f64::from_bits(bits);
            if x.is_nan() {
                return "(0.0/0.0)".into();
            }
            if x.is_infinite() {
                return if x > 0.0 { "(1.0/0.0)".into() } else { "(-1.0/0.0)".into() };
            }
            format!("{x:e}")
        }
        KTy::Bool => if bits != 0 { "1" } else { "0" }.into(),
        _ => {
            let v = bits as i64;
            match (v, tgt) {
                (i64::MIN, Tgt::C) => "INT64_MIN".into(),
                (i64::MIN, _) => "(-9223372036854775807L - 1L)".into(),
                (v, Tgt::C) => format!("{v}LL"),
                (v, _) => format!("{v}L"),
            }
        }
    }
}

pub fn elem_c(t: KTy, tgt: Tgt) -> &'static str {
    match (t, tgt) {
        (KTy::F32, _) => "float",
        (KTy::F64, _) => "double",
        (KTy::I32, Tgt::C) => "int32_t",
        (KTy::U32, Tgt::C) => "uint32_t",
        (KTy::I32, _) => "int",
        (KTy::U32, _) => "uint",
        (KTy::Int | KTy::Bool, Tgt::C) => "int64_t",
        (KTy::Int | KTy::Bool, _) => "long",
    }
}

impl<'a> Em<'a> {
    pub(super) fn new(k: &'a Kernel, t: Tgt, ind: usize) -> Self {
        Em { k, t, out: String::new(), n: 0, ind, mask: false, narrow: false, sync: false, lane: false }
    }

    pub(super) fn take(&mut self) -> String {
        std::mem::take(&mut self.out)
    }

    fn vty(&self, t: KTy) -> &'static str {
        match (t, self.t) {
            (KTy::F32, _) => "float",
            (KTy::F64, _) => "double",
            (KTy::Bool, Tgt::C) => "int",
            (KTy::Bool, _) => "bool",
            (_, Tgt::C) => "int64_t",
            _ if self.narrow => "int",
            _ => "long",
        }
    }

    fn ucast(&self) -> &'static str {
        match self.t {
            Tgt::C => "(uint64_t)",
            _ if self.narrow => "(uint)",
            _ => "(ulong)",
        }
    }

    fn lv(&self, slot: usize) -> String {
        if self.lane { format!("v{slot}[ss_t]") } else { format!("v{slot}") }
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.ind {
            self.out.push_str("  ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn fresh(&mut self) -> String {
        self.n += 1;
        format!("t{}", self.n)
    }

    fn fail(&mut self, cond: &str, code: i64, clause: &str, value: &str) {
        match self.t {
            Tgt::C if self.sync => self.line(&format!("if (UNLIKELY({cond})) {{ ss_q_sync(st); TRAPV({code}, {clause}, {value}); }}")),
            Tgt::C => self.line(&format!("if (UNLIKELY({cond})) TRAPV({code}, {clause}, {value});")),
            _ => self.line(&format!("bad |= ({cond});")),
        }
    }

    fn flag(&self) -> &'static str {
        match (self.t, self.mask) {
            (Tgt::Msl, true) => "atomic_fetch_or_explicit(&ss_mask[ss_lin >> 5], 1u << (uint)(ss_lin & 31), memory_order_relaxed); atomic_store_explicit(ss_flag, 1u, memory_order_relaxed);",
            (Tgt::Msl, false) => "atomic_store_explicit(ss_flag, 1u, memory_order_relaxed);",
            (_, true) => "SS_OR(&ss_mask[ss_lin >> 5], 1u << (uint)(ss_lin & 31)); *ss_flag = 1u;",
            _ => "*ss_flag = 1u;",
        }
    }

    fn safe_index(&self, i: &str, p: usize) -> String {
        if self.t == Tgt::C {
            return i.to_string();
        }
        let cast = self.ucast();
        format!("({cast}{i} < {cast}l{p} ? {i} : 0)")
    }

    fn range(&mut self, t: KTy, v: &str) {
        if self.narrow {
            return;
        }
        match t {
            KTy::I32 => self.fail(&format!("{v} < -2147483648LL || {v} > 2147483647LL").replace("LL", if self.t == Tgt::C { "LL" } else { "L" }), T_OVERFLOW, "0", "0"),
            KTy::U32 => self.fail(&format!("{v} < 0 || {v} > 4294967295LL").replace("LL", if self.t == Tgt::C { "LL" } else { "L" }), T_OVERFLOW, "0", "0"),
            _ => {}
        }
    }

    fn tiny(&mut self, r: &str, cond: &str) {
        if self.t != Tgt::C {
            self.fail(&format!("fabs({r}) < {FLT_TINY} && ({cond})"), 0, "0", "0");
        }
    }

    fn min_i(&self) -> &'static str {
        if self.t == Tgt::C { "INT64_MIN" } else { "(-9223372036854775807L - 1L)" }
    }

    pub(super) fn expr(&mut self, e: &KExpr) -> String {
        match e {
            KExpr::Lit(t, b) if self.narrow && t.is_int() => format!("{}", *b as i64),
            KExpr::Lit(t, b) => lit(*t, *b, self.t),
            KExpr::Local(s) => self.lv(*s),
            KExpr::Param(p) => match self.k.params[*p].ty {
                KTy::Bool if self.t != Tgt::C => format!("(p{p} != 0)"),
                t if self.narrow && t.is_int() => format!("((int)p{p})"),
                _ => format!("p{p}"),
            },
            KExpr::Len(p) if self.narrow => format!("((int)l{p})"),
            KExpr::Len(p) => format!("l{p}"),
            KExpr::Intr(i) => match i {
                KIntr::Gid => "gid".into(),
                KIntr::Lid => format!("(gid % {})", self.k.group),
                KIntr::GroupId => format!("(gid / {})", self.k.group),
                KIntr::GroupSize => format!("{}", self.k.group),
                KIntr::GridSize if self.narrow => "((int)ss_n)".into(),
                KIntr::GridSize => "ss_n".into(),
                KIntr::GidY => "gid_y".into(),
                KIntr::LidY => format!("(gid_y % {})", self.k.group_y),
                KIntr::GroupIdY => format!("(gid_y / {})", self.k.group_y),
                KIntr::GroupSizeY => format!("{}", self.k.group_y),
                KIntr::GridSizeY if self.narrow => "((int)ss_ny)".into(),
                KIntr::GridSizeY => "ss_ny".into(),
            },
            KExpr::Load(p, i) => {
                let i = self.expr(i);
                let t = self.fresh();
                let et = self.k.params[*p].ty;
                let cast = self.ucast();
                self.fail(&format!("{cast}{i} >= {cast}l{p}"), T_INDEX, &format!("l{p}"), &i);
                let vt = self.vty(et);
                let ix = self.safe_index(&i, *p);
                self.line(&format!("{vt} {t} = ({vt})p{p}[{ix}];"));
                if et == KTy::F32 {
                    match self.t {
                        Tgt::Msl => self.fail(&format!("ss_sub(as_type<uint>({t}))"), 0, "0", "0"),
                        Tgt::Ocl => self.fail(&format!("ss_sub(as_uint({t}))"), 0, "0", "0"),
                        Tgt::C => {}
                    }
                }
                t
            }
            KExpr::SLoad(a, i) => {
                let i = self.expr(i);
                let t = self.fresh();
                let (et, len) = (self.k.shared[*a].1, self.k.shared[*a].2);
                let cast = self.ucast();
                self.fail(&format!("{cast}{i} >= {cast}{len}"), T_INDEX, &len.to_string(), &i);
                let vt = self.vty(et);
                let ix = if self.t == Tgt::C { i } else { format!("({cast}{i} < {cast}{len} ? {i} : 0)") };
                self.line(&format!("{vt} {t} = ({vt})s{a}[{ix}];"));
                t
            }
            KExpr::Bin(op @ (BinOp::And | BinOp::Or), _, a, b) => {
                let a = self.expr(a);
                let t = self.fresh();
                let bt = self.vty(KTy::Bool);
                self.line(&format!("{bt} {t} = {a};"));
                self.line(&format!("if ({}{t}) {{", if *op == BinOp::And { "" } else { "!" }));
                self.ind += 1;
                let b = self.expr(b);
                self.line(&format!("{t} = {b};"));
                self.ind -= 1;
                self.line("}");
                t
            }
            KExpr::Bin(op, ty, a, b) => {
                let a = self.expr(a);
                let b = self.expr(b);
                self.bin(*op, *ty, &a, &b)
            }
            KExpr::Neg(ty, a) => {
                let a = self.expr(a);
                let t = self.fresh();
                let vt = self.vty(*ty);
                if ty.is_int() && !self.narrow {
                    let min = self.min_i();
                    self.fail(&format!("{a} == {min}"), T_OVERFLOW, "0", "0");
                }
                self.line(&format!("{vt} {t} = -{a};"));
                self.range(*ty, &t);
                t
            }
            KExpr::Not(a) => {
                let a = self.expr(a);
                format!("(!{a})")
            }
            KExpr::Cast(to, from, a) => {
                let a = self.expr(a);
                let t = self.fresh();
                let vt = self.vty(*to);
                match (to, from) {
                    (KTy::F32 | KTy::F64, _) => self.line(&format!("{vt} {t} = ({vt}){a};")),
                    _ => {
                        self.line(&format!("{vt} {t} = {a};"));
                        self.range(*to, &t);
                    }
                }
                t
            }
            KExpr::Call(f, ty, xs) => {
                let a = self.expr(&xs[0]);
                let b = xs.get(1).map(|x| self.expr(x));
                let t = self.fresh();
                let vt = self.vty(*ty);
                let f32 = *ty == KTy::F32 && self.t == Tgt::C;
                let fun = |n: &str| if f32 { format!("{n}f") } else { n.to_string() };
                match f {
                    KFn::Min => self.line(&format!("{vt} {t} = {a} < {} ? {a} : {};", b.clone().unwrap(), b.unwrap())),
                    KFn::Max => self.line(&format!("{vt} {t} = {a} > {} ? {a} : {};", b.clone().unwrap(), b.unwrap())),
                    KFn::Abs if ty.is_int() => {
                        let min = self.min_i();
                        if !self.narrow {
                            self.fail(&format!("{a} == {min}"), T_OVERFLOW, "0", "0");
                        }
                        self.line(&format!("{vt} {t} = {a} < 0 ? -{a} : {a};"));
                        self.range(*ty, &t);
                    }
                    KFn::Abs => self.line(&format!("{vt} {t} = {}({a});", fun("fabs"))),
                    KFn::Sqrt => self.line(&format!("{vt} {t} = {}({a});", fun("sqrt"))),
                    KFn::Floor => self.line(&format!("{vt} {t} = {}({a});", fun("floor"))),
                    KFn::Ceil => self.line(&format!("{vt} {t} = {}({a});", fun("ceil"))),
                    KFn::Round => self.line(&format!("{vt} {t} = {}({a});", fun("round"))),
                    KFn::Shl | KFn::Shr => {
                        let b = b.unwrap();
                        let (l, u) = if self.t == Tgt::C { ("int64_t", "uint64_t") } else { ("long", "ulong") };
                        let op = if *f == KFn::Shl { "<<" } else { ">>" };
                        self.line(&format!("{vt} {t} = ({vt})(({l}){b} >= 0 && ({l}){b} < 64 ? ({l})(({u})({l}){a} {op} ({u}){b}) : 0);"));
                        if self.t == Tgt::C {
                            match ty {
                                KTy::I32 => self.fail(&format!("{t} < -2147483648LL || {t} > 2147483647LL"), T_OVERFLOW, "0", "0"),
                                KTy::U32 => self.fail(&format!("{t} < 0 || {t} > 4294967295LL"), T_OVERFLOW, "0", "0"),
                                _ => {}
                            }
                        } else {
                            self.range(*ty, &t);
                        }
                    }
                    KFn::Band | KFn::Bor | KFn::Bxor => {
                        let op = match f {
                            KFn::Band => "&",
                            KFn::Bor => "|",
                            _ => "^",
                        };
                        self.line(&format!("{vt} {t} = {a} {op} {};", b.unwrap()));
                    }
                }
                t
            }
            KExpr::If(c, a, b) => {
                let c = self.expr(c);
                let t = self.fresh();
                let ty = self.expr_ty(e);
                let vt = self.vty(ty);
                self.line(&format!("{vt} {t};"));
                self.line(&format!("if ({c}) {{"));
                self.ind += 1;
                let a = self.expr(a);
                self.line(&format!("{t} = {a};"));
                self.ind -= 1;
                self.line("} else {");
                self.ind += 1;
                let b = self.expr(b);
                self.line(&format!("{t} = {b};"));
                self.ind -= 1;
                self.line("}");
                t
            }
        }
    }

    fn expr_ty(&self, e: &KExpr) -> KTy {
        match e {
            KExpr::Lit(t, _) => *t,
            KExpr::Local(s) => self.k.locals[*s],
            KExpr::Param(p) | KExpr::Load(p, _) => self.k.params[*p].ty,
            KExpr::Len(_) | KExpr::Intr(_) => KTy::Int,
            KExpr::Bin(op, t, ..) => {
                if matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or) {
                    KTy::Bool
                } else {
                    *t
                }
            }
            KExpr::Neg(t, _) | KExpr::Cast(t, _, _) | KExpr::Call(_, t, _) => *t,
            KExpr::Not(_) => KTy::Bool,
            KExpr::If(_, a, _) => self.expr_ty(a),
            KExpr::SLoad(a, _) => self.k.shared[*a].1,
        }
    }

    fn bin(&mut self, op: BinOp, ty: KTy, a: &str, b: &str) -> String {
        use BinOp::*;
        let t = self.fresh();
        if matches!(op, Eq | Ne | Lt | Le | Gt | Ge) {
            let bt = self.vty(KTy::Bool);
            self.line(&format!("{bt} {t} = {a} {} {b};", op.symbol()));
            return t;
        }
        let vt = self.vty(ty);
        if ty.is_float() {
            self.line(&format!("{vt} {t} = {a} {} {b};", op.symbol()));
            if ty == KTy::F32 {
                match op {
                    Add => self.tiny(&t, &format!("{a} != -{b}")),
                    Sub => self.tiny(&t, &format!("{a} != {b}")),
                    Mul => self.tiny(&t, &format!("{a} != 0.0f && {b} != 0.0f")),
                    _ => self.tiny(&t, &format!("{a} != 0.0f && !isinf({b})")),
                }
            }
            return t;
        }
        let min = self.min_i();
        let wide = ty == KTy::Int;
        match op {
            Div | Rem => {
                self.fail(&format!("{b} == 0"), T_DIV_ZERO, "0", "0");
                if !self.narrow {
                    self.fail(&format!("{a} == {min} && {b} == -1"), T_OVERFLOW, "0", "0");
                }
                if self.t == Tgt::C {
                    self.line(&format!("{vt} {t} = {a} {} {b};", op.symbol()));
                } else {
                    self.line(&format!("{vt} {t} = {a} {} ({b} == 0 ? 1 : {b});", op.symbol()));
                }
            }
            _ if !wide || self.narrow => self.line(&format!("{vt} {t} = {a} {} {b};", op.symbol())),
            _ if self.t == Tgt::C => {
                let f = match op {
                    Add => "add",
                    Sub => "sub",
                    _ => "mul",
                };
                self.line(&format!("{vt} {t};"));
                self.fail(&format!("__builtin_{f}_overflow({a}, {b}, &{t})"), T_OVERFLOW, "0", "0");
            }
            Add => {
                self.line(&format!("{vt} {t} = (long)((ulong){a} + (ulong){b});"));
                self.fail(&format!("(({a} ^ {t}) & ({b} ^ {t})) < 0"), T_OVERFLOW, "0", "0");
            }
            Sub => {
                self.line(&format!("{vt} {t} = (long)((ulong){a} - (ulong){b});"));
                self.fail(&format!("(({a} ^ {b}) & ({a} ^ {t})) < 0"), T_OVERFLOW, "0", "0");
            }
            _ => {
                self.line(&format!("{vt} {t} = (long)((ulong){a} * (ulong){b});"));
                self.fail(&format!("SS_MULHI({a}, {b}) != ({t} >> 63)"), T_OVERFLOW, "0", "0");
            }
        }
        self.range(ty, &t);
        t
    }

    fn commit(&mut self, group: &[KStmt]) {
        let mut done = Vec::new();
        for st in group {
            let KStmt::Store(p, i, v) = st else { continue };
            let i = self.expr(i);
            let v = self.expr(v);
            done.push((*p, i, v));
        }
        let cast = self.ucast();
        for (p, i, _) in &done {
            self.fail(&format!("{cast}{i} >= {cast}l{p}"), T_INDEX, &format!("l{p}"), i);
        }
        if self.t != Tgt::C {
            let f = self.flag();
            self.line(&format!("if (bad) {{ {f} return; }}"));
        }
        for (p, i, v) in &done {
            let et = elem_c(self.k.params[*p].ty, self.t);
            self.line(&format!("p{p}[{i}] = ({et}){v};"));
        }
    }

    fn lanes(&mut self, b: &[KStmt]) {
        let (g, gy) = (self.k.group, self.k.group_y);
        let n = g * gy;
        let base = format!("int64_t ss_t = 0, gid = ss_bx * {g}, gid_y = ss_by * {gy}; (void)ss_t; (void)gid; (void)gid_y;");
        for s in b {
            if !has_barrier(s) {
                self.line(&format!("for (int64_t ss_t = 0; ss_t < {n}; ss_t++) {{"));
                self.ind += 1;
                self.line(&format!("int64_t gid = ss_bx * {g} + ss_t % {g}, gid_y = ss_by * {gy} + ss_t / {g}; (void)gid; (void)gid_y;"));
                self.stmt(s);
                self.ind -= 1;
                self.line("}");
                continue;
            }
            match s {
                KStmt::For(slot, a, b, body) => {
                    self.line("{");
                    self.ind += 1;
                    self.line(&base);
                    let lo = self.expr(a);
                    let hi = self.expr(b);
                    let (l, h) = (self.fresh(), self.fresh());
                    self.line(&format!("int64_t {l} = {lo}, {h} = {hi};"));
                    self.line(&format!("for (int64_t ss_i = {l}; ss_i < {h}; ss_i++) {{"));
                    self.ind += 1;
                    self.line(&format!("for (int64_t ss_u = 0; ss_u < {n}; ss_u++) v{slot}[ss_u] = ss_i;"));
                    self.lanes(body);
                    self.ind -= 1;
                    self.line("}");
                    self.ind -= 1;
                    self.line("}");
                }
                KStmt::If(c, t, f) => {
                    self.line("{");
                    self.ind += 1;
                    self.line(&base);
                    let c = self.expr(c);
                    self.line(&format!("if ({c}) {{"));
                    self.ind += 1;
                    self.lanes(t);
                    self.ind -= 1;
                    self.line("} else {");
                    self.ind += 1;
                    self.lanes(f);
                    self.ind -= 1;
                    self.line("}");
                    self.ind -= 1;
                    self.line("}");
                }
                _ => {}
            }
        }
    }

    pub(super) fn block(&mut self, b: &[KStmt]) {
        for s in b {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &KStmt) {
        match s {
            KStmt::Let(slot, e) => {
                let v = self.expr(e);
                let vt = self.vty(self.k.locals[*slot]);
                if self.lane {
                    self.line(&format!("v{slot}[ss_t] = {v};"));
                } else {
                    self.line(&format!("{vt} v{slot} = {v};"));
                }
            }
            KStmt::Set(slot, e) => {
                let v = self.expr(e);
                let l = self.lv(*slot);
                self.line(&format!("{l} = {v};"));
            }
            KStmt::SStore(a, i, v) => {
                let i = self.expr(i);
                let v = self.expr(v);
                let (et, len) = (self.k.shared[*a].1, self.k.shared[*a].2);
                let cast = self.ucast();
                self.fail(&format!("{cast}{i} >= {cast}{len}"), T_INDEX, &len.to_string(), &i);
                let et = elem_c(et, self.t);
                let guard = if self.t == Tgt::C { "" } else { "if (!bad) " };
                self.line(&format!("{guard}s{a}[{i}] = ({et}){v};"));
            }
            KStmt::Atomic(op, shared, w, i, v, v2) => {
                let i = self.expr(i);
                let v = self.expr(v);
                let v2 = v2.as_ref().map(|x| self.expr(x));
                let (et, base, len) = if *shared { (self.k.shared[*w].1, format!("s{w}"), self.k.shared[*w].2.to_string()) } else { (self.k.params[*w].ty, format!("p{w}"), format!("l{w}")) };
                let cast = self.ucast();
                let clause = if *shared { len.clone() } else { format!("l{w}") };
                self.fail(&format!("{cast}{i} >= {cast}{len}"), T_INDEX, &clause, &i);
                let e = elem_c(et, self.t);
                let p = format!("{base}[{i}]");
                if self.t == Tgt::C {
                    let d = v2.clone().unwrap_or_default();
                    self.line(&match (op, et) {
                        (AOp::Add, KTy::F32) => format!("{p} = {p} + {v};"),
                        (AOp::Add, KTy::I32) => format!("{p} = (int32_t)((uint32_t){p} + (uint32_t){v});"),
                        (AOp::Add, _) => format!("{p} = (uint32_t)((uint32_t){p} + (uint32_t){v});"),
                        (AOp::Min, _) => format!("if ({v} < {p}) {p} = ({e}){v};"),
                        (AOp::Max, _) => format!("if ({v} > {p}) {p} = ({e}){v};"),
                        (AOp::Cas, _) => format!("if ({p} == {v}) {p} = ({e}){d};"),
                    });
                    return;
                }
                self.line("if (!bad) {");
                self.ind += 1;
                let msl = self.t == Tgt::Msl;
                let space = match (msl, *shared) {
                    (true, true) => "threadgroup ",
                    (true, false) => "device ",
                    (false, true) => "volatile __local ",
                    (false, false) => "volatile __global ",
                };
                let ut = "uint";
                let at = |t: &str| if msl { format!("atomic_{t}") } else { t.to_string() };
                let ptr = |t: &str| format!("({space}{}*)&{p}", at(t));
                let rel = "memory_order_relaxed";
                match (*op, et) {
                    (AOp::Add, KTy::F32) => {
                        let a = self.fresh();
                        let o = self.fresh();
                        let of = self.fresh();
                        self.line(&format!("{space}{}* {a} = {};", at(ut), ptr(ut)));
                        if msl {
                            self.line(&format!("uint {o} = atomic_load_explicit({a}, {rel}); float {of};"));
                            self.line(&format!("while (true) {{ {of} = as_type<float>({o}); if (atomic_compare_exchange_weak_explicit({a}, &{o}, as_type<uint>({of} + {v}), {rel}, {rel})) break; }}"));
                        } else {
                            self.line(&format!("uint {o} = *{a}; float {of};"));
                            self.line(&format!("while (1) {{ {of} = as_float({o}); uint n_ = SS_ACAS({a}, {o}, as_uint({of} + {v})); if (n_ == {o}) break; {o} = n_; }}"));
                        }
                        self.line(&format!("bad |= !({v} >= 0.0f && {v} <= 16777216.0f && floor({v}) == {v} && floor({of}) == {of} && fabs({of}) <= 16777216.0f && fabs({of} + {v}) <= 16777216.0f);"));
                    }
                    (op, t) => {
                        let it = if t == KTy::I32 { "int" } else { "uint" };
                        let u = if t == KTy::U32 { "U" } else { "" };
                        if msl {
                            let f = match op {
                                AOp::Add => "add",
                                AOp::Min => "min",
                                AOp::Max => "max",
                                AOp::Cas => "",
                            };
                            if op == AOp::Cas {
                                let x = self.fresh();
                                let d = v2.clone().unwrap_or_default();
                                self.line(&format!("while (true) {{ {it} {x} = ({it}){v}; if (atomic_compare_exchange_weak_explicit({}, &{x}, ({it}){d}, {rel}, {rel}) || {x} != ({it}){v}) break; }}", ptr(it)));
                            } else {
                                self.line(&format!("atomic_fetch_{f}_explicit({}, ({it}){v}, {rel});", ptr(it)));
                            }
                        } else {
                            match op {
                                AOp::Add => self.line(&format!("SS_AADD(&{p}, ({it}){v});")),
                                AOp::Min => self.line(&format!("SS_AMIN{u}(&{p}, ({it}){v});")),
                                AOp::Max => self.line(&format!("SS_AMAX{u}(&{p}, ({it}){v});")),
                                AOp::Cas => self.line(&format!("SS_ACAS(&{p}, ({it}){v}, ({it}){});", v2.clone().unwrap_or_default())),
                            }
                        }
                    }
                }
                self.ind -= 1;
                self.line("}");
            }
            KStmt::Barrier => match self.t {
                Tgt::Msl => self.line("threadgroup_barrier(mem_flags::mem_threadgroup);"),
                Tgt::Ocl => self.line("barrier(CLK_LOCAL_MEM_FENCE);"),
                Tgt::C => {}
            },
            KStmt::Store(p, i, v) => {
                let i = self.expr(i);
                let v = self.expr(v);
                let cast = self.ucast();
                self.fail(&format!("{cast}{i} >= {cast}l{p}"), T_INDEX, &format!("l{p}"), &i);
                let et = elem_c(self.k.params[*p].ty, self.t);
                let guard = if self.t == Tgt::C { "" } else { "if (!bad) " };
                self.line(&format!("{guard}p{p}[{i}] = ({et}){v};"));
            }
            KStmt::For(slot, a, b, body) => {
                let lo = self.expr(a);
                let hi = self.expr(b);
                let it = self.vty(KTy::Int);
                let h = self.fresh();
                self.line(&format!("{it} {h} = {hi};"));
                let stop = if self.t == Tgt::C || body.iter().any(has_barrier) { "" } else { " && !bad" };
                let l = self.lv(*slot);
                let decl = if self.lane { String::new() } else { format!("{it} ") };
                self.line(&format!("for ({decl}{l} = {lo}; {l} < {h}{stop}; {l}++) {{"));

                self.ind += 1;
                self.block(body);
                self.ind -= 1;
                self.line("}");
            }
            KStmt::If(c, t, f) => {
                let c = self.expr(c);
                self.line(&format!("if ({c}) {{"));
                self.ind += 1;
                self.block(t);
                self.ind -= 1;
                if f.is_empty() {
                    self.line("}");
                } else {
                    self.line("} else {");
                    self.ind += 1;
                    self.block(f);
                    self.ind -= 1;
                    self.line("}");
                }
            }
            KStmt::Eval(e) => {
                let v = self.expr(e);
                self.line(&format!("(void){v};"));
            }
        }
    }
}

pub fn scalar_c(t: KTy) -> &'static str {
    match t {
        KTy::F32 => "float",
        KTy::F64 => "double",
        _ => "int64_t",
    }
}

fn has_store(s: &KStmt) -> bool {
    match s {
        KStmt::Store(..) | KStmt::Atomic(..) => true,
        KStmt::For(_, _, _, b) => b.iter().any(has_store),
        KStmt::If(_, t, f) => t.iter().chain(f).any(has_store),
        _ => false,
    }
}

struct Iv<'a> {
    k: &'a Kernel,
    out: String,
    n: usize,
    locals: HashMap<usize, (String, String)>,
    mutable: HashSet<usize>,
}

type IvR = Result<Option<(String, String)>, ()>;

fn set_slots(b: &[KStmt], out: &mut HashSet<usize>) {
    for s in b {
        match s {
            KStmt::Set(slot, _) => {
                out.insert(*slot);
            }
            KStmt::For(_, _, _, body) => set_slots(body, out),
            KStmt::If(_, t, f) => {
                set_slots(t, out);
                set_slots(f, out);
            }
            _ => {}
        }
    }
}

impl Iv<'_> {
    fn node(&mut self, t: KTy, lo: String, hi: String) -> (String, String) {
        self.n += 1;
        let (a, b) = (format!("lo{}", self.n), format!("hi{}", self.n));
        let (tlo, thi) = t.range();
        let (mn, mx) = (tlo.max(i32::MIN as i128), thi.min(i32::MAX as i128));
        writeln!(self.out, "    __int128 {a} = {lo}, {b} = {hi}; nar_ &= {a} >= {mn} && {b} <= {mx};").unwrap();
        (a, b)
    }

    fn expr(&mut self, e: &KExpr) -> IvR {
        let big = |v: i128| format!("((__int128){}LL)", v as i64);
        let ty = |t: KTy| t.is_int();
        Ok(match e {
            KExpr::Lit(t, b) if ty(*t) => Some(self.node(*t, big(*b as i64 as i128), big(*b as i64 as i128))),
            KExpr::Lit(..) => None,
            KExpr::Local(s) if ty(self.k.locals[*s]) => Some(self.locals.get(s).cloned().ok_or(())?),
            KExpr::Local(_) => None,
            KExpr::Param(p) if ty(self.k.params[*p].ty) => Some(self.node(self.k.params[*p].ty, format!("(__int128)p{p}"), format!("(__int128)p{p}"))),
            KExpr::Param(_) => None,
            KExpr::Len(p) => Some(self.node(KTy::Int, format!("(__int128)l{p}"), format!("(__int128)l{p}"))),
            KExpr::Intr(i) => {
                let g = self.k.group;
                let gy = self.k.group_y;
                let (lo, hi) = match i {
                    KIntr::Gid => ("0".to_string(), "(__int128)ss_n - 1".to_string()),
                    KIntr::Lid => ("0".into(), format!("{}", g - 1)),
                    KIntr::GroupId => ("0".into(), format!("((__int128)ss_n - 1) / {g}")),
                    KIntr::GroupSize => (format!("{g}"), format!("{g}")),
                    KIntr::GridSize => ("(__int128)ss_n".into(), "(__int128)ss_n".into()),
                    KIntr::GidY => ("0".to_string(), "(__int128)ss_ny - 1".to_string()),
                    KIntr::LidY => ("0".into(), format!("{}", gy - 1)),
                    KIntr::GroupIdY => ("0".into(), format!("((__int128)ss_ny - 1) / {gy}")),
                    KIntr::GroupSizeY => (format!("{gy}"), format!("{gy}")),
                    KIntr::GridSizeY => ("(__int128)ss_ny".into(), "(__int128)ss_ny".into()),
                };
                Some(self.node(KTy::Int, lo, hi))
            }
            KExpr::SLoad(a, i) => {
                self.expr(i)?;
                let et = self.k.shared[*a].1;
                if !ty(et) {
                    return Ok(None);
                }
                let (lo, hi) = et.range();
                Some(self.node(et, big(lo), if hi > i64::MAX as i128 { big(i64::MAX as i128) } else { format!("((__int128){hi}LL)") }))
            }
            KExpr::Load(p, i) => {
                self.expr(i)?;
                let et = self.k.params[*p].ty;
                if !ty(et) {
                    return Ok(None);
                }
                let (lo, hi) = et.range();
                Some(self.node(et, big(lo), if hi > i64::MAX as i128 { big(i64::MAX as i128) } else { format!("((__int128){hi}LL)") }))
            }
            KExpr::Bin(op, t, a, b) => {
                let x = self.expr(a)?;
                let y = self.expr(b)?;
                let (Some((al, ah)), Some((bl, bh))) = (x, y) else { return Ok(None) };
                if !ty(*t) || matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) {
                    return Ok(None);
                }
                let (lo, hi) = match op {
                    BinOp::Add => (format!("{al} + {bl}"), format!("{ah} + {bh}")),
                    BinOp::Sub => (format!("{al} - {bh}"), format!("{ah} - {bl}")),
                    BinOp::Mul => (format!("iv_min4({al} * {bl}, {al} * {bh}, {ah} * {bl}, {ah} * {bh})"), format!("iv_max4({al} * {bl}, {al} * {bh}, {ah} * {bl}, {ah} * {bh})")),
                    BinOp::Div => (format!("iv_div_lo({al}, {ah}, {bl}, {bh})"), format!("iv_div_hi({al}, {ah}, {bl}, {bh})")),
                    _ => (format!("iv_rem_lo({al}, {ah}, {bl}, {bh})"), format!("iv_rem_hi({al}, {ah}, {bl}, {bh})")),
                };
                Some(self.node(*t, lo, hi))
            }
            KExpr::Neg(t, a) => match self.expr(a)? {
                Some((lo, hi)) if ty(*t) => Some(self.node(*t, format!("-{hi}"), format!("-{lo}"))),
                _ => None,
            },
            KExpr::Not(a) => {
                self.expr(a)?;
                None
            }
            KExpr::Cast(to, _, a) => match self.expr(a)? {
                Some((lo, hi)) if ty(*to) => Some(self.node(*to, lo, hi)),
                _ => None,
            },
            KExpr::Call(f, t, xs) => {
                let a = self.expr(&xs[0])?;
                let b = match xs.get(1) {
                    Some(x) => self.expr(x)?,
                    None => None,
                };
                match (f, a, b) {
                    (KFn::Abs, Some((lo, hi)), _) if ty(*t) => Some(self.node(*t, "0".into(), format!("iv_mag({lo}, {hi})"))),
                    (KFn::Min, Some((al, ah)), Some((bl, bh))) => Some(self.node(*t, format!("iv_min4({al}, {bl}, {al}, {bl})"), format!("iv_min4({ah}, {bh}, {ah}, {bh})"))),
                    (KFn::Max, Some((al, ah)), Some((bl, bh))) => Some(self.node(*t, format!("iv_max4({al}, {bl}, {al}, {bl})"), format!("iv_max4({ah}, {bh}, {ah}, {bh})"))),
                    (KFn::Shl | KFn::Shr | KFn::Band | KFn::Bor | KFn::Bxor, Some((al, ah)), Some((bl, bh))) => {
                        let ok = format!("{al} >= 0 && {bl} >= 0");
                        let (lo, hi) = match f {
                            KFn::Shl => (format!("{al} << {bl}"), format!("{ah} << {bh}")),
                            KFn::Shr => (format!("iv_shr({al}, {bh})"), format!("iv_shr({ah}, {bl})")),
                            KFn::Band => ("0".into(), format!("iv_min4({ah}, {bh}, {ah}, {bh})")),
                            _ => ("0".into(), format!("iv_ones(iv_max4({ah}, {bh}, {ah}, {bh}))")),
                        };
                        let ok = if *f == KFn::Shl { format!("{ok} && {bh} < 62 && {ah} < ((__int128)1 << 62)") } else { ok };
                        Some(self.node(*t, format!("({ok} ? {lo} : IV_MIN)"), format!("({ok} ? {hi} : IV_MAX)")))
                    }
                    _ if t.is_int() => return Err(()),
                    _ => None,
                }
            }
            KExpr::If(c, a, b) => {
                self.expr(c)?;
                let x = self.expr(a)?;
                let y = self.expr(b)?;
                match (x, y) {
                    (Some((al, ah)), Some((bl, bh))) => {
                        let t = if let KExpr::Local(s) = &**a { self.k.locals[*s] } else { KTy::Int };
                        Some(self.node(t, format!("iv_min4({al}, {bl}, {al}, {bl})"), format!("iv_max4({ah}, {bh}, {ah}, {bh})")))
                    }
                    _ => None,
                }
            }
        })
    }

    fn block(&mut self, b: &[KStmt]) -> Result<(), ()> {
        for s in b {
            match s {
                KStmt::Let(slot, e) => {
                    let r = self.expr(e)?;
                    if self.k.locals[*slot].is_int() {
                        if self.mutable.contains(slot) {
                            return Err(());
                        }
                        self.locals.insert(*slot, r.ok_or(())?);
                    }
                }
                KStmt::Set(_, e) | KStmt::Eval(e) => {
                    self.expr(e)?;
                }
                KStmt::Store(_, i, v) | KStmt::SStore(_, i, v) => {
                    self.expr(i)?;
                    self.expr(v)?;
                }
                KStmt::Barrier => {}
                KStmt::Atomic(_, _, _, i, v, w) => {
                    self.expr(i)?;
                    self.expr(v)?;
                    if let Some(w) = w {
                        self.expr(w)?;
                    }
                }
                KStmt::For(slot, a, b, body) => {
                    let (lo, _) = self.expr(a)?.ok_or(())?;
                    let (_, hi) = self.expr(b)?.ok_or(())?;
                    let r = self.node(KTy::Int, lo.clone(), format!("iv_max4({hi} - 1, {lo}, {lo}, {lo})"));
                    self.locals.insert(*slot, r);
                    self.block(body)?;
                }
                KStmt::If(c, t, f) => {
                    self.expr(c)?;
                    self.block(t)?;
                    self.block(f)?;
                }
            }
        }
        Ok(())
    }
}

pub fn narrow_pred(k: &Kernel) -> Option<String> {
    let mut mutable = HashSet::new();
    set_slots(&k.body, &mut mutable);
    let mut iv = Iv { k, out: String::new(), n: 0, locals: HashMap::new(), mutable };
    iv.block(&k.body).ok()?;
    Some(iv.out)
}

pub fn commit_split(k: &Kernel) -> Option<usize> {
    if k.grouped {
        return None;
    }
    let b = &k.body;
    let mut split = b.len();
    while split > 0 && matches!(b[split - 1], KStmt::Store(..)) {
        split -= 1;
    }
    if b[..split].iter().any(has_store) {
        return None;
    }
    let mut stored = Vec::new();
    for s in &b[split..] {
        let KStmt::Store(p, i, v) = s else { continue };
        let reads = |e: &KExpr| sspur_check::kernel::expr_has(e, &|x| matches!(x, KExpr::Load(q, _) if stored.contains(q)));
        if reads(i) || reads(v) {
            return None;
        }
        stored.push(*p);
    }
    Some(split)
}

pub fn cpu_kernel(k: &Kernel, fidx: usize) -> String {
    let mut sig = Vec::new();
    for (i, p) in k.params.iter().enumerate() {
        match p.slice {
            None => sig.push(format!("{} p{i}", scalar_c(p.ty))),
            Some(m) => {
                sig.push(format!("{}{}* p{i}", if m { "" } else { "const " }, elem_c(p.ty, Tgt::C)));
                sig.push(format!("int64_t l{i}"));
            }
        }
    }
    if k.grouped {
        let g = k.group_threads() as usize;
        let mut decls = String::new();
        let mut off = 0usize;
        for (i, t) in k.locals.iter().enumerate() {
            let vt = match t {
                KTy::F32 => "float",
                KTy::F64 => "double",
                KTy::Bool => "int",
                _ => "int64_t",
            };
            writeln!(decls, "  {vt}* v{i} = ({vt}*)(ss_mem + {off});").unwrap();
            off += g * 8;
        }
        let mut zero = String::new();
        for (i, (_, t, len)) in k.shared.iter().enumerate() {
            let e = elem_c(*t, Tgt::C);
            writeln!(decls, "  {e}* s{i} = ({e}*)(ss_mem + {off});").unwrap();
            writeln!(zero, "    memset(s{i}, 0, {});", *len as usize * es(*t)).unwrap();
            off += (*len as usize * es(*t)).div_ceil(8) * 8;
        }
        let mut em = Em::new(k, Tgt::C, 2);
        em.lane = true;
        em.lanes(&k.body);
        let body = em.take();
        return format!(
            "#define FIDX {fidx}\nstatic void kc_{}({}, int64_t ss_n, int64_t ss_ny, const uint32_t* ss_mask, Status* st) {{\n#pragma clang fp contract(off)\n  (void)ss_mask;\n  char* ss_mem = (char*)malloc({});\n{decls}  for (int64_t ss_by = 0; ss_by < ss_ny / {}; ss_by++) for (int64_t ss_bx = 0; ss_bx < ss_n / {}; ss_bx++) {{\n{zero}{body}  }}\n  free(ss_mem);\n}}\n#undef FIDX\n",
            k.name,
            sig.join(", "),
            off.max(8),
            k.group_y,
            k.group
        );
    }
    let mut em = Em::new(k, Tgt::C, 2);
    em.block(&k.body);
    let body = em.take();
    format!(
        "#define FIDX {fidx}\nstatic void kc_{}({}, int64_t ss_n, int64_t ss_ny, const uint32_t* ss_mask, Status* st) {{\n#pragma clang fp contract(off)\n  for (int64_t gid_y = 0; gid_y < ss_ny; gid_y++) for (int64_t gid = 0; gid < ss_n; gid++) {{\n    int64_t ss_lin = gid_y * ss_n + gid; (void)ss_lin; (void)gid_y;\n    if (ss_mask && !((ss_mask[ss_lin >> 5] >> (ss_lin & 31)) & 1u)) continue;\n{body}  }}\n}}\n#undef FIDX\n",
        k.name,
        sig.join(", ")
    )
}

pub const MSL_PRELUDE: &str = "#include <metal_stdlib>\nusing namespace metal;\n#pragma METAL fp math_mode(safe)\n#pragma METAL fp contract(off)\n#define SS_MULHI(a, b) mulhi((long)(a), (long)(b))\nstatic inline bool ss_sub(uint b) { return (b & 0x7f800000u) == 0u && (b & 0x007fffffu) != 0u; }\n";

pub const OCL_PRELUDE: &str = "#ifdef __NVPTX__\n#define get_global_id(d) ((d) == 0 ? (size_t)__nvvm_read_ptx_sreg_ctaid_x() * __nvvm_read_ptx_sreg_ntid_x() + __nvvm_read_ptx_sreg_tid_x() : (size_t)__nvvm_read_ptx_sreg_ctaid_y() * __nvvm_read_ptx_sreg_ntid_y() + __nvvm_read_ptx_sreg_tid_y())
\n#define SS_MULHI(a, b) __nvvm_mulhi_ll((a), (b))\n#define fabs(x) __builtin_fabs(x)\n#define sqrt(x) __builtin_sqrt(x)\n#define floor(x) __builtin_floor(x)\n#define ceil(x) __builtin_ceil(x)\n#define round(x) __builtin_round(x)\n#define isinf(x) __builtin_isinf(x)\n#define SS_OR(p, v) __nvvm_atom_or_gen_i((volatile int*)(p), (int)(v))\n#define barrier(f) __nvvm_bar_sync(0)\n#define SS_AADD(p, v) __nvvm_atom_add_gen_i((volatile int*)(p), (int)(v))\n#define SS_AMIN(p, v) __nvvm_atom_min_gen_i((volatile int*)(p), (int)(v))\n#define SS_AMAX(p, v) __nvvm_atom_max_gen_i((volatile int*)(p), (int)(v))\n#define SS_AMINU(p, v) __nvvm_atom_min_gen_ui((volatile unsigned int*)(p), (unsigned int)(v))\n#define SS_AMAXU(p, v) __nvvm_atom_max_gen_ui((volatile unsigned int*)(p), (unsigned int)(v))\n#define SS_ACAS(p, e, d) (uint)__nvvm_atom_cas_gen_i((volatile int*)(p), (int)(e), (int)(d))\n#else\n#define SS_MULHI(a, b) mul_hi((long)(a), (long)(b))\n#define SS_OR(p, v) atomic_or((p), (v))\n#define SS_AADD(p, v) atomic_add((p), (v))\n#define SS_AMIN(p, v) atomic_min((p), (v))\n#define SS_AMAX(p, v) atomic_max((p), (v))\n#define SS_AMINU(p, v) atomic_min((p), (v))\n#define SS_AMAXU(p, v) atomic_max((p), (v))\n#define SS_ACAS(p, e, d) atomic_cmpxchg((p), (e), (d))\n#endif
\n#pragma OPENCL EXTENSION cl_khr_fp64 : enable\n#pragma OPENCL FP_CONTRACT OFF\nstatic inline bool ss_sub(uint b) { return (b & 0x7f800000u) == 0u && (b & 0x007fffffu) != 0u; }\n";

pub fn device_kernel(k: &Kernel, tgt: Tgt, narrow: bool) -> String {
    let mut sig = Vec::new();
    let mut slot = 0;
    let mut buf = |s: String, sig: &mut Vec<String>| {
        if tgt == Tgt::Msl {
            sig.push(format!("{s} [[buffer({slot})]]"));
        } else {
            sig.push(s);
        }
        slot += 1;
    };
    let (dev, con) = if tgt == Tgt::Msl { ("device ", "constant ") } else { ("__global ", "") };
    for (i, p) in k.params.iter().enumerate() {
        let r = if tgt == Tgt::Msl { "&" } else { "" };
        match p.slice {
            None => {
                let t = match p.ty {
                    KTy::F32 => "float",
                    KTy::F64 => "double",
                    _ => "long",
                };
                buf(format!("{con}{t}{r} p{i}"), &mut sig);
            }
            Some(m) => {
                buf(format!("{dev}{}{}* p{i}", if m { "" } else { "const " }, elem_c(p.ty, tgt)), &mut sig);
                buf(format!("{con}long{r} l{i}"), &mut sig);
            }
        }
    }
    let r = if tgt == Tgt::Msl { "&" } else { "" };
    buf(format!("{con}long{r} ss_n"), &mut sig);
    let two = k.grid_y.is_some();
    if two {
        buf(format!("{con}long{r} ss_ny"), &mut sig);
    }
    let flag = if tgt == Tgt::Msl { "device atomic_uint* ss_flag" } else { "__global uint* ss_flag" };
    buf(flag.to_string(), &mut sig);
    let split = commit_split(k);
    if split.is_some() {
        let mask = if tgt == Tgt::Msl { "device atomic_uint* ss_mask" } else { "__global uint* ss_mask" };
        buf(mask.to_string(), &mut sig);
    }
    let mut em = Em::new(k, tgt, 1);
    em.mask = split.is_some();
    em.narrow = narrow;
    em.line(if tgt == Tgt::Msl { "bool bad = false;" } else { "uint bad = 0u;" });
    for (i, (_, t, len)) in k.shared.iter().enumerate() {
        let e = elem_c(*t, tgt);
        let space = if tgt == Tgt::Msl { "threadgroup" } else { "__local" };
        let (g, gy) = (k.group, k.group_y);
        em.line(&format!("{space} {e} s{i}[{len}];"));
        if !k.shared_zero[i] {
            continue;
        }
        let start = if two { format!("(int)((gid_y % {gy}) * {g} + gid % {g})") } else { format!("(int)(gid % {g})") };
        em.line(&format!("for (int ss_z = {start}; ss_z < {len}; ss_z += {}) s{i}[ss_z] = 0;", g * gy));
    }
    if k.shared_zero.contains(&true) {
        em.stmt(&KStmt::Barrier);
    }

    match split {
        Some(at) => {
            em.block(&k.body[..at]);
            em.commit(&k.body[at..]);
        }
        None => {
            em.block(&k.body);
            let f = em.flag();
            em.line(&format!("if (bad) {{ {f} }}"));
        }
    }
    let body = em.take();
    let (name, sig) = (&k.name, sig.join(", "));
    match (tgt, two) {
        (Tgt::Msl, false) if narrow => format!("kernel void k_{name}_n({sig}, uint ss_t [[thread_position_in_grid]]) {{\n  int gid = (int)ss_t;\n  if ((long)gid >= ss_n) return;\n  int ss_lin = gid; (void)ss_lin;\n{body}}}\n"),
        (Tgt::Msl, false) => format!("kernel void k_{name}({sig}, uint ss_t [[thread_position_in_grid]]) {{\n  long gid = (long)ss_t;\n  if (gid >= ss_n) return;\n  long ss_lin = gid; (void)ss_lin;\n{body}}}\n"),
        (Tgt::Msl, true) if narrow => format!("kernel void k_{name}_n({sig}, uint2 ss_t [[thread_position_in_grid]]) {{\n  int gid = (int)ss_t.x, gid_y = (int)ss_t.y;\n  if ((long)gid >= ss_n || (long)gid_y >= ss_ny) return;\n  int ss_lin = gid_y * (int)ss_n + gid; (void)ss_lin;\n{body}}}\n"),
        (Tgt::Msl, true) => format!("kernel void k_{name}({sig}, uint2 ss_t [[thread_position_in_grid]]) {{\n  long gid = (long)ss_t.x, gid_y = (long)ss_t.y;\n  if (gid >= ss_n || gid_y >= ss_ny) return;\n  long ss_lin = gid_y * ss_n + gid; (void)ss_lin;\n{body}}}\n"),
        (_, false) => format!("__kernel void k_{name}({sig}) {{\n  long gid = (long)get_global_id(0);\n  if (gid >= ss_n) return;\n  long ss_lin = gid;\n{body}}}\n"),
        (_, true) => format!("__kernel void k_{name}({sig}) {{\n  long gid = (long)get_global_id(0), gid_y = (long)get_global_id(1);\n  if (gid >= ss_n || gid_y >= ss_ny) return;\n  long ss_lin = gid_y * ss_n + gid;\n{body}}}\n"),
    }
}

pub fn msl_source(ks: &[&Kernel]) -> String {
    let mut s = String::from(MSL_PRELUDE);
    for k in ks {
        s.push_str(&device_kernel(k, Tgt::Msl, false));
        if narrow_pred(k).is_some() {
            s.push_str(&device_kernel(k, Tgt::Msl, true));
        }
    }
    s
}

pub fn opencl_source(ks: &[&Kernel]) -> String {
    let mut s = String::from(OCL_PRELUDE);
    for k in ks {
        s.push_str(&device_kernel(k, Tgt::Ocl, false));
    }
    s
}

pub const EMIT_TARGETS: &[&str] = &["metal", "opencl", "spirv", "ptx"];

fn device_cc(backend: &str) -> Result<String, String> {
    let mut cands: Vec<String> = std::env::var("SSPUR_GPU_CC").into_iter().collect();
    cands.extend(["/opt/homebrew/opt/llvm/bin/clang", "/usr/local/opt/llvm/bin/clang", "clang-22", "clang-21", "clang-20", "clang-19", "clang"].map(String::from));
    cands
        .into_iter()
        .find(|c| Command::new(c).arg("-print-targets").output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim_start().starts_with(backend))))
        .ok_or_else(|| format!("no clang with the {backend} backend (install LLVM, e.g. 'brew install llvm', or set SSPUR_GPU_CC)"))
}

pub fn emit(check: &CheckOutput, target: &str) -> Result<Vec<u8>, String> {
    let ks: Vec<&Kernel> = check.kernels.values().collect();
    if ks.is_empty() {
        return Err("the program has no kernel fns".into());
    }
    let (backend, args): (&str, &[&str]) = match target {
        "metal" => {
            let metal: Vec<&Kernel> = ks.iter().copied().filter(|k| !k.uses_f64()).collect();
            return Ok(msl_source(&metal).into_bytes());
        }
        "opencl" => return Ok(opencl_source(&ks).into_bytes()),
        "spirv" => ("spirv64", &["-target", "spirv64", "-cl-std=CL2.0", "-c"]),
        "ptx" => ("nvptx64", &["--target=nvptx64-nvidia-cuda", "-march=sm_70", "-nogpulib", "-cl-std=CL2.0", "-S"]),
        _ => return Err(format!("unknown GPU target '{target}' (use {})", EMIT_TARGETS.join(", "))),
    };
    let cc = device_cc(backend)?;
    let src = opencl_source(&ks);
    let key = blake3::hash(format!("{target}\n{src}").as_bytes()).to_hex().to_string();
    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let uniq = super::tmp_suffix();
    let cl = dir.join(format!("{}.{uniq}.cl", &key[..24]));
    let out = dir.join(format!("{}.{uniq}.{target}", &key[..24]));
    std::fs::write(&cl, &src).map_err(|e| e.to_string())?;
    let mut failure = String::new();
    let mut crashed = false;
    for opt in if target == "ptx" { ["-O3", "-O1"] } else { ["-O2", "-O1"] } {
        let o = Command::new(&cc).args(["-x", "cl", opt]).args(args).arg(&cl).arg("-o").arg(&out).output().map_err(|e| format!("cannot run {cc}: {e}"))?;
        if o.status.success() {
            failure.clear();
            break;
        }
        let err = String::from_utf8_lossy(&o.stderr);
        crashed = err.contains("PLEASE submit a bug report") || o.status.code().is_none();
        failure = format!("{cc} failed for {target}: {}", err.lines().take(8).collect::<Vec<_>>().join(" | "));
    }
    if crashed {
        return Err(format!("no clang with a working {backend} backend ({cc} crashed on valid OpenCL; install a newer LLVM or set SSPUR_GPU_CC)"));
    }
    if !failure.is_empty() {
        return Err(failure);
    }
    let bytes = std::fs::read(&out).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&cl);
    Ok(bytes)
}

pub(super) const SHIM_LINK: &str = "@sspur-gpu";
const SHIM_SRC: &str = include_str!("gpu_rt.m");

pub fn shim() -> Result<PathBuf, String> {
    let key = blake3::hash(SHIM_SRC.as_bytes()).to_hex().to_string();
    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let lib = dir.join(format!("libsspur_gpu_{}.dylib", &key[..16]));
    if lib.exists() {
        return Ok(lib);
    }
    let uniq = super::tmp_suffix();
    let src = dir.join(format!("sspur_gpu_{}.{uniq}.m", &key[..16]));
    std::fs::write(&src, SHIM_SRC).map_err(|e| e.to_string())?;
    let tmp = lib.with_extension(format!("{uniq}.tmp"));
    let cc = super::flags::cc();
    let install = format!("-Wl,-install_name,{}", lib.display());
    let metal = Command::new(&cc).args(["-O2", "-fobjc-arc", "-shared", "-fPIC", "-w", &install, "-framework", "Metal", "-framework", "Foundation", "-o"]).arg(&tmp).arg(&src).output();
    if !metal.as_ref().is_ok_and(|o| o.status.success()) {
        let stub = Command::new(&cc).args(["-O2", "-x", "c", "-shared", "-fPIC", "-w", &install, "-o"]).arg(&tmp).arg(&src).output().map_err(|e| format!("cannot run {cc}: {e}"))?;
        if !stub.status.success() {
            return Err(format!("{cc} failed to build the GPU runtime: {}", String::from_utf8_lossy(&stub.stderr).lines().take(4).collect::<Vec<_>>().join(" | ")));
        }
    }
    let _ = std::fs::remove_file(&src);
    super::flags::publish(&tmp, &lib)?;
    Ok(lib)
}

pub(super) const HOST_PRELUDE: &str = r#"typedef struct { int32_t kind, pad; const void* ptr; int64_t bytes; void* dev; } SsGpuArg;
#ifdef __APPLE__
int ss_gpu_ready(void);
int ss_gpu_run(const char* src, const char* entry, int32_t nargs, const SsGpuArg* args, int64_t nx, int64_t ny, int64_t gx, int64_t gy, int32_t masked, const uint32_t** mask);
void* ss_gpu_alloc(int64_t bytes, void** host);
void ss_gpu_free(void* dev);
int ss_gpu_enqueue(const char* src, const char* entry, int32_t nargs, const SsGpuArg* args, int64_t nx, int64_t ny, int64_t gx, int64_t gy, int32_t opts, int32_t slot);
int ss_gpu_wait(void);
void ss_gpu_commit(void);
uint32_t ss_gpu_flag(int32_t slot);
const uint32_t* ss_gpu_mask(int32_t slot);
#else
static int ss_gpu_enqueue(const char* src, const char* entry, int32_t nargs, const SsGpuArg* args, int64_t nx, int64_t ny, int64_t gx, int64_t gy, int32_t opts, int32_t slot) { (void)src; (void)entry; (void)nargs; (void)args; (void)nx; (void)ny; (void)gx; (void)gy; (void)opts; (void)slot; return 2; }
static int ss_gpu_wait(void) { return 0; }
static void ss_gpu_commit(void) {}
static uint32_t ss_gpu_flag(int32_t slot) { (void)slot; return 1; }
static const uint32_t* ss_gpu_mask(int32_t slot) { (void)slot; return 0; }
static int ss_gpu_ready(void) { return 0; }
static int ss_gpu_run(const char* src, const char* entry, int32_t nargs, const SsGpuArg* args, int64_t nx, int64_t ny, int64_t gx, int64_t gy, int32_t masked, const uint32_t** mask) { (void)src; (void)entry; (void)nargs; (void)args; (void)nx; (void)ny; (void)gx; (void)gy; (void)masked; (void)mask; return 2; }

static void* ss_gpu_alloc(int64_t bytes, void** host) { (void)bytes; (void)host; return 0; }
static void ss_gpu_free(void* dev) { (void)dev; }
#endif
static int ss_trace_on = -1;
static void ss_trace(const char* k, const char* how, int64_t n) {
    if (ss_trace_on < 0) { const char* t = getenv("SSPUR_GPU_TRACE"); ss_trace_on = t && *t && *t != '0'; }
    if (ss_trace_on) fprintf(stderr, "gpu %s: %s, %lld threads\n", k, how, (long long)n);
}
static void __attribute__((noinline, cold, noreturn)) dev_fail(Status* st, int64_t f, const char* a, int64_t j, const char* b, int64_t v) {
    char* p = (char*)malloc(strlen(a) + (b ? strlen(b) : 0) + 64);
    if (j >= 0) sprintf(p, "%s%lld%s%lld)", a, (long long)j, b, (long long)v); else if (b) sprintf(p, "%s%s%lld)", a, b, (long long)v); else strcpy(p, a);
    st->rbuf = (int64_t*)p; st->rlen = (int64_t)strlen(p);
    sspur_trap(st, 12, f, 0, 0);
}
static inline int ss_sub32(float x) { return x != 0.0f && fabsf(x) < 1.17549435e-38f; }
static inline double ss_canon(double x) { return x != x ? NAN : x; }
static inline __int128 iv_min4(__int128 a, __int128 b, __int128 c, __int128 d) { __int128 m = a < b ? a : b; m = m < c ? m : c; return m < d ? m : d; }
static inline __int128 iv_max4(__int128 a, __int128 b, __int128 c, __int128 d) { __int128 m = a > b ? a : b; m = m > c ? m : c; return m > d ? m : d; }
static inline __int128 iv_mag(__int128 a, __int128 b) { a = a < 0 ? -a : a; b = b < 0 ? -b : b; return a > b ? a : b; }
static inline __int128 iv_div_lo(__int128 al, __int128 ah, __int128 bl, __int128 bh) { if (bl <= 0 && bh >= 0) return -iv_mag(al, ah); return iv_min4(al / bl, al / bh, ah / bl, ah / bh); }
static inline __int128 iv_div_hi(__int128 al, __int128 ah, __int128 bl, __int128 bh) { if (bl <= 0 && bh >= 0) return iv_mag(al, ah); return iv_max4(al / bl, al / bh, ah / bl, ah / bh); }
static inline __int128 iv_rem_m(__int128 al, __int128 ah, __int128 bl, __int128 bh) { __int128 m = iv_mag(bl, bh) - 1, a = iv_mag(al, ah); if (m < 0) m = 0; return m < a ? m : a; }
static inline __int128 iv_rem_lo(__int128 al, __int128 ah, __int128 bl, __int128 bh) { return al >= 0 ? 0 : -iv_rem_m(al, ah, bl, bh); }
#define IV_MIN (-(__int128)9223372036854775807LL - 1)
#define IV_MAX ((__int128)9223372036854775807LL)
static inline __int128 iv_shr(__int128 a, __int128 s) { return s >= 64 ? 0 : a >> (int)s; }
static inline __int128 iv_ones(__int128 a) { __int128 m = 0; while (m < a) m = m * 2 + 1; return m; }
static inline __int128 iv_rem_hi(
__int128 al, __int128 ah, __int128 bl, __int128 bh) { return ah <= 0 ? 0 : iv_rem_m(al, ah, bl, bh); }
struct SsDev { int64_t len, es; void* host; void* dev; struct SsDev* next; void* bk; uint64_t qb; int64_t qf; int32_t qbk; };
static struct SsDev* ss_devs;
#define SS_QMAX 256
typedef struct SsLaunch { void (*cpu)(void*, const uint32_t*, Status*); void* rec; const char* name; const char* why; int64_t n, fidx; int32_t gpu, masked, narrow, nw, nr; struct SsDev* w[16]; struct SsDev* r[16]; } SsLaunch;
static SsLaunch ss_q[SS_QMAX];
static struct SsDev* ss_q_pend[SS_QMAX * 16];
static int ss_qn, ss_q_tail, ss_q_gpus, ss_q_npend, ss_q_incb;
static uint64_t ss_qbatch = 1;
static pthread_mutex_t ss_q_mu = PTHREAD_MUTEX_INITIALIZER;
static void ss_q_drop(void) {
    for (int i = 0; i < ss_qn; i++) free(ss_q[i].rec);
    ss_qn = 0; ss_q_tail = 0; ss_q_gpus = 0; ss_q_npend = 0; ss_q_incb = 0; ss_qbatch++;
}
static void ss_q_backup(void) {
    for (int k = 0; k < ss_q_npend; k++) {
        struct SsDev* d = ss_q_pend[k];
        if (d->qbk) continue;
        if (!d->bk) d->bk = malloc((size_t)(d->len ? d->len : 1) * (size_t)d->es);
        if (d->len) memcpy(d->bk, d->host, (size_t)(d->len * d->es));
        d->qbk = 1;
    }
    ss_q_npend = 0;
}
static void ss_q_cpu(SsLaunch* L, const uint32_t* m, Status* st) {
    jmp_buf jb; jmp_buf* saved = sspur_jb; sspur_jb = &jb;
    if (setjmp(jb)) { sspur_jb = saved; ss_q_drop(); pthread_mutex_unlock(&ss_q_mu); longjmp(*saved, 1); }
    L->cpu(L->rec, m, st);
    sspur_jb = saved;
}
static int ss_q_has(struct SsDev* const* xs, int n, const struct SsDev* d) {
    for (int i = 0; i < n; i++) if (xs[i] == d) return 1;
    return 0;
}
static int ss_q_alone(int i) {
    SsLaunch* L = &ss_q[i];
    for (int j = i + 1; j < ss_qn; j++) {
        SsLaunch* M = &ss_q[j];
        for (int k = 0; k < M->nw; k++) if (ss_q_has(L->w, L->nw, M->w[k]) || ss_q_has(L->r, L->nr, M->w[k])) return 0;
        for (int k = 0; k < M->nr; k++) if (ss_q_has(L->w, L->nw, M->r[k])) return 0;
    }
    return 1;
}
static void ss_q_sync_locked(Status* st) {
    int n = ss_qn;
    if (!n) return;
    if (n > 1 || !ss_q[0].masked) ss_q_backup();
    int ok = !ss_q_gpus || ss_gpu_wait() == 0;
    for (int i = 0; i < n; i++) {
        SsLaunch* L = &ss_q[i];
        if (!L->gpu) { ss_trace(L->name, L->why, L->n); ss_q_cpu(L, 0, st); continue; }
        if (ok && !ss_gpu_flag(i)) { ss_trace(L->name, L->narrow ? "metal, 32-bit indices" : "metal", L->n); continue; }
        if (ok && L->masked && ss_q_alone(i)) { ss_trace(L->name, "cpu rerun of flagged threads", L->n); ss_q_cpu(L, ss_gpu_mask(i), st); continue; }
        int s = i;
        for (;;) {
            int s2 = s;
            for (int j = s; j < n; j++) for (int k = 0; k < ss_q[j].nw; k++) if (ss_q[j].w[k]->qf < s2) s2 = (int)ss_q[j].w[k]->qf;
            if (s2 == s) break;
            s = s2;
        }
        for (int j = s; j < n; j++) for (int k = 0; k < ss_q[j].nw; k++) {
            struct SsDev* d = ss_q[j].w[k];
            if (d->qbk) { if (d->len) memcpy(d->host, d->bk, (size_t)(d->len * d->es)); continue; }
            if (ss_q[d->qf].gpu) {
                SsLaunch F = ss_q[d->qf];
                char* m = (char*)malloc(strlen(F.name) + 64);
                sprintf(m, "dev: the GPU failed while running %s", F.name);
                ss_q_drop(); pthread_mutex_unlock(&ss_q_mu);
                dev_fail(st, F.fidx, m, -1, 0, 0);
            }
        }
        for (int j = s; j < n; j++) {
            ss_trace(ss_q[j].name, !ss_q[j].gpu ? ss_q[j].why : !ok ? "cpu rerun (GPU failure)" : j < i ? "cpu rerun (before a flagged launch)" : j > i ? "cpu rerun (after a flagged launch)" : "cpu rerun (trap or subnormal)", ss_q[j].n);
            ss_q_cpu(&ss_q[j], 0, st);
        }
        break;
    }
    ss_q_drop();
}
static void ss_q_sync(Status* st) {
    if (!ss_qn) return;
    pthread_mutex_lock(&ss_q_mu);
    ss_q_sync_locked(st);
    pthread_mutex_unlock(&ss_q_mu);
}
static void ss_q_push(Status* st, SsLaunch* L, const char* src, const char* entry, int32_t nargs, const SsGpuArg* g, int64_t nx, int64_t ny, int64_t gx, int64_t gy, int32_t opts) {
    pthread_mutex_lock(&ss_q_mu);
    ss_sync_hook = ss_q_sync;
    if (ss_qn == SS_QMAX || (entry && ss_q_tail)) ss_q_sync_locked(st);
    int i = ss_qn;
    L->gpu = entry && ss_gpu_enqueue(src, entry, nargs, g, nx, ny, gx, gy, opts, i) == 0;
    for (int k = 0; k < L->nw; k++) {
        struct SsDev* d = L->w[k];
        if (d->qb == ss_qbatch) continue;
        d->qb = ss_qbatch; d->qf = i; d->qbk = 0;
        if (L->gpu) ss_q_pend[ss_q_npend++] = d;
    }
    if (L->gpu) ss_q_gpus++; else ss_q_tail = 1;
    ss_q[i] = *L;
    ss_qn++;
    if (L->gpu && ++ss_q_incb >= 64) { ss_q_backup(); ss_gpu_commit(); ss_q_incb = 0; }
    pthread_mutex_unlock(&ss_q_mu);
}
static void ss_dev_reset(void) {
    pthread_mutex_lock(&ss_q_mu);
    if (ss_q_gpus) ss_gpu_wait();
    ss_q_drop();
    pthread_mutex_unlock(&ss_q_mu);
    while (ss_devs) { struct SsDev* d = ss_devs; ss_devs = d->next; if (d->dev) ss_gpu_free(d->dev); else free(d->host); free(d->bk); free(d); }
}
static pthread_mutex_t ss_dev_mu = PTHREAD_MUTEX_INITIALIZER;
static struct SsDev* ss_dev_alloc(int64_t n, int64_t es) {
    struct SsDev* d = (struct SsDev*)calloc(1, sizeof(struct SsDev));
    d->len = n; d->es = es;
    if (ss_gpu_ready()) d->dev = ss_gpu_alloc(n * es, &d->host);
    if (!d->dev) d->host = calloc((size_t)(n ? n : 1), (size_t)es);
    pthread_mutex_lock(&ss_dev_mu);
    d->next = ss_devs; ss_devs = d;
    ss_reset_hook = ss_dev_reset;
    pthread_mutex_unlock(&ss_dev_mu);
    return d;
}
"#;

fn es(t: KTy) -> usize {
    if matches!(t, KTy::F32 | KTy::I32 | KTy::U32) { 4 } else { 8 }
}

fn dev_elem(t: &Type) -> KTy {
    match t {
        Type::Con(_, a) => match a.first() {
            Some(Type::Con(e, _)) => KTy::from_name(e).unwrap_or(KTy::Int),
            _ => KTy::Int,
        },
        _ => KTy::Int,
    }
}

impl Cx<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn kernel_fn(&mut self, f: &FnDef, params: &[Type], ret: &Type, fidx: usize, k: &Kernel, devs: &[bool], cname: &str) -> G {
        let rr = self.rr(ret)?;
        let dev = |i: usize| devs.get(i).copied().unwrap_or(false);
        let all_dev = k.params.iter().enumerate().all(|(i, p)| p.slice.is_none() || dev(i));
        let mut sig = Vec::new();
        for (i, (t, p)) in params.iter().zip(&f.params).enumerate() {
            if dev(i) {
                sig.push(format!("SsDev* a{i}"));
                continue;
            }
            let star = if is_mut_borrow(&p.ty) { "*" } else { "" };
            sig.push(format!("{}{star} a{i}", self.cty(t)?));
        }
        sig.push("Status* st".into());
        sig.push("int64_t depth".into());
        let name = &f.name;
        let mut s = format!("#define FIDX {fidx}\nstatic {rr} f_{cname}({}) {{\n  (void)depth;\n", sig.join(", "));
        if !all_dev {
            s.push_str("  ss_q_sync(st);\n");
        }
        for (j, q) in k.params.iter().enumerate() {
            for (i, p) in k.params.iter().enumerate().take(j) {
                if dev(i) && dev(j) && (p.slice == Some(true) || q.slice == Some(true)) {
                    let m = c_lit(&format!("dev: arguments {} and {} of {name} are the same buffer", p.name, q.name));
                    writeln!(s, "  if (UNLIKELY(a{i} == a{j})) {{ ss_q_sync(st); dev_fail(st, FIDX, {m}, -1, 0, 0); }}").unwrap();
                }
            }
        }
        let mut gargs = Vec::new();
        let mut cargs = Vec::new();
        let mut back = String::new();
        let mut save = String::new();
        let mut restore = String::new();
        let mut rec = Vec::new();
        let mut qargs = Vec::new();
        let mut fill = String::new();
        for (i, (p, t)) in k.params.iter().zip(params).enumerate() {
            let src = if is_mut_borrow(&f.params[i].ty) { format!("(*a{i})") } else { format!("a{i}") };
            let Some(m) = p.slice else {
                let c = scalar_c(p.ty);
                if let Some((lo, hi)) = matches!(p.ty, KTy::I32 | KTy::U32).then(|| p.ty.range()) {
                    writeln!(s, "  if (UNLIKELY({src} < {lo}LL || {src} > {hi}LL)) {{ ss_q_sync(st); dev_fail(st, FIDX, \"dev: argument {} of {name} is out of range for {} (value = \", -1, \"\", {src}); }}", p.name, p.ty.name()).unwrap();
                }
                writeln!(s, "  {c} p{i} = ({c}){src};").unwrap();
                let bytes = if p.ty == KTy::F32 { 4 } else { 8 };
                gargs.push(format!("{{0, 0, &p{i}, {bytes}, 0}}"));
                cargs.push(format!("p{i}"));
                rec.push(format!("{c} p{i};"));
                qargs.push(format!("q_->p{i}"));
                writeln!(fill, "    q_->p{i} = p{i};").unwrap();
                continue;
            };
            let e = elem_c(p.ty, Tgt::C);
            let es = es(p.ty);
            if dev(i) {
                writeln!(s, "  int64_t l{i} = a{i}->len; {e}* p{i} = ({e}*)a{i}->host;").unwrap();
                if all_dev {
                    gargs.push(format!("{{3, 0, p{i}, l{i} * {es}, a{i}->dev}}"));
                } else {
                    gargs.push(format!("a{i}->dev ? (SsGpuArg){{3, 0, p{i}, l{i} * {es}, a{i}->dev}} : (SsGpuArg){{{}, 0, p{i}, l{i} * {es}, 0}}", if m { 2 } else { 1 }));
                }
                rec.push(format!("struct SsDev* d{i};"));
                qargs.push(format!("({}{e}*)q_->d{i}->host, q_->d{i}->len", if m { "" } else { "const " }));
                writeln!(fill, "    q_->d{i} = a{i};").unwrap();
                if m {
                    writeln!(save, "      void* bk{i} = 0; if (a{i}->dev) {{ bk{i} = sspur_alloc_atomic((size_t)(l{i} ? l{i} : 1) * {es}); memcpy(bk{i}, p{i}, (size_t)l{i} * {es}); }}").unwrap();
                    writeln!(restore, "      if (r_ && bk{i}) memcpy(p{i}, bk{i}, (size_t)l{i} * {es});").unwrap();
                    writeln!(fill, "    L_.w[L_.nw++] = a{i};").unwrap();
                } else {
                    writeln!(fill, "    L_.r[L_.nr++] = a{i};").unwrap();
                }
            } else {
                writeln!(s, "  int64_t l{i} = {src}.len;").unwrap();
                match p.ty {
                    KTy::F64 | KTy::Int if !m => writeln!(s, "  const {e}* p{i} = (const {e}*){src}.data;").unwrap(),
                    KTy::F64 | KTy::Int => writeln!(s, "  {e}* p{i} = ({e}*)sspur_alloc_atomic((size_t)(l{i} ? l{i} : 1) * 8); if (l{i}) memcpy(p{i}, {src}.data, (size_t)l{i} * 8);").unwrap(),
                    KTy::F32 => writeln!(s, "  float* p{i} = (float*)sspur_alloc_atomic((size_t)(l{i} ? l{i} : 1) * 4); for (int64_t j_ = 0; j_ < l{i}; j_++) p{i}[j_] = (float){src}.data[j_];").unwrap(),
                    _ => {
                        let (lo, hi) = p.ty.range();
                        writeln!(s, "  {e}* p{i} = ({e}*)sspur_alloc_atomic((size_t)(l{i} ? l{i} : 1) * 4); for (int64_t j_ = 0; j_ < l{i}; j_++) {{ int64_t v_ = {src}.data[j_]; if (UNLIKELY(v_ < {lo}LL || v_ > {hi}LL)) dev_fail(st, FIDX, \"dev: element \", j_, \" of {} is out of range for {} (value = \", v_); p{i}[j_] = ({e})v_; }}", p.name, p.ty.name()).unwrap();
                    }
                }
                gargs.push(format!("{{{}, 0, p{i}, l{i} * {es}, 0}}", if m { 2 } else { 1 }));
                if m {
                    let lc = self.cty(t)?;
                    let ec = if p.ty.is_float() { "double" } else { "int64_t" };
                    let conv = if p.ty.is_float() { format!("ss_canon((double)p{i}[j_])") } else { format!("(int64_t)p{i}[j_]") };
                    writeln!(back, "  {{ RawL r_ = raw_alloc_a(l{i}, 8, 1); {ec}* d_ = ({ec}*)r_.data; for (int64_t j_ = 0; j_ < l{i}; j_++) d_[j_] = {conv}; r_.hdr[1] = l{i}; *a{i} = ({lc}){{l{i}, d_, r_.hdr}}; }}").unwrap();
                }
            }
            gargs.push(format!("{{0, 0, &l{i}, 8, 0}}"));
            cargs.push(format!("p{i}"));
            cargs.push(format!("l{i}"));
        }
        let mut em = Em::new(k, Tgt::C, 1);
        em.sync = all_dev;
        for (i, pre) in k.pres.iter().enumerate() {
            let c = em.expr(pre);
            em.fail(&format!("!({c})"), T_PRE, &i.to_string(), "0");
        }
        let n = em.expr(&k.grid);
        let ny = k.grid_y.as_ref().map(|g| em.expr(g)).unwrap_or_else(|| "1".into());
        s.push_str(&em.take());
        writeln!(s, "  int64_t ss_n = {n}, ss_ny = {ny};").unwrap();
        let (gx, gy) = (k.group, k.group_y);
        if k.grouped && k.grid_y.is_none() {
            let m = c_lit(&format!("dev: the thread count of {name} is not a multiple of its group size {gx} (count = "));
            writeln!(s, "  if (UNLIKELY(ss_n > 0 && ss_n % {gx} != 0)) {{ ss_q_sync(st); dev_fail(st, FIDX, {m}, -1, \"\", ss_n); }}").unwrap();
        } else if k.grouped {
            let m = c_lit(&format!("dev: the grid width of {name} is not a multiple of its group width {gx} (width = "));
            writeln!(s, "  if (UNLIKELY(ss_n > 0 && ss_ny > 0 && ss_n % {gx} != 0)) {{ ss_q_sync(st); dev_fail(st, FIDX, {m}, -1, \"\", ss_n); }}").unwrap();
            let m = c_lit(&format!("dev: the grid height of {name} is not a multiple of its group height {gy} (height = "));
            writeln!(s, "  if (UNLIKELY(ss_n > 0 && ss_ny > 0 && ss_ny % {gy} != 0)) {{ ss_q_sync(st); dev_fail(st, FIDX, {m}, -1, \"\", ss_ny); }}").unwrap();
        }
        let exact = if k.grouped { 2 } else { 0 };
        let sub: Vec<String> = k.params.iter().enumerate().filter(|(_, p)| p.slice.is_none() && p.ty == KTy::F32).map(|(i, _)| format!(" && !ss_sub32(p{i})")).collect();
        gargs.push("{0, 0, &ss_n, 8, 0}".into());
        if k.grid_y.is_some() {
            gargs.push("{0, 0, &ss_ny, 8, 0}".into());
        }
        let commit = commit_split(k).is_some();
        let narrow = narrow_pred(k);
        let entry = |s: &mut String| match &narrow {
            Some(code) => {
                let lens: String = k.params.iter().enumerate().filter(|(_, p)| p.slice.is_some()).map(|(i, _)| format!(" && l{i} <= 2147483647LL")).collect();
                writeln!(s, "      nar_ = ss_n <= 2147483647LL && ss_ny <= 2147483647LL && ss_n * ss_ny <= 2147483647LL{lens};\n      if (nar_) {{\n{code}      }}").unwrap();
                format!("nar_ ? \"k_{name}_n\" : \"k_{name}\"")
            }
            None => format!("\"k_{name}\""),
        };
        writeln!(s, "  if (ss_n > 0 && ss_ny > 0) {{").unwrap();
        if all_dev {
            writeln!(self.protos, "typedef struct {{ {} int64_t ss_n, ss_ny; }} KQ_{cname};\nstatic void kq_{cname}(void* r_, const uint32_t* m_, Status* st);", rec.join(" ")).unwrap();
            writeln!(s, "    KQ_{cname}* q_ = (KQ_{cname}*)malloc(sizeof(KQ_{cname}));\n    q_->ss_n = ss_n; q_->ss_ny = ss_ny;").unwrap();
            writeln!(s, "    SsLaunch L_ = {{0}}; L_.cpu = kq_{cname}; L_.rec = q_; L_.name = \"{name}\"; L_.fidx = FIDX; L_.n = ss_n * ss_ny; L_.masked = {};", i32::from(commit)).unwrap();
            s.push_str(&fill);
            let why = if k.uses_f64() { "cpu (F64 is not available on the GPU)" } else { "cpu" };
            writeln!(s, "    L_.why = \"{why}\";\n    const char* e_ = 0; int nar_ = 0; (void)nar_;").unwrap();
            let devs_ok: String = k.params.iter().enumerate().filter(|(_, p)| p.slice.is_some()).map(|(i, _)| format!(" && a{i}->dev")).collect();
            if !k.uses_f64() {
                writeln!(s, "    if (ss_gpu_ready(){}{devs_ok}) {{", sub.concat()).unwrap();
                let e = entry(&mut s);
                writeln!(s, "      e_ = {e};\n    }}\n    L_.narrow = nar_;").unwrap();
            }
            writeln!(s, "    SsGpuArg g_[] = {{{}}};", gargs.join(", ")).unwrap();
            writeln!(s, "    ss_q_push(st, &L_, ss_msl, e_, {}, g_, ss_n, ss_ny, {gx}, {gy}, {});", gargs.len(), i32::from(commit) | exact).unwrap();
        } else {
            let masked = commit && !k.params.iter().enumerate().any(|(i, p)| p.slice == Some(true) && !dev(i));
            if k.uses_f64() {
                writeln!(s, "    ss_trace(\"{name}\", \"cpu (F64 is not available on the GPU)\", ss_n * ss_ny);").unwrap();
                writeln!(s, "    kc_{name}({}, ss_n, ss_ny, 0, st);", cargs.join(", ")).unwrap();
            } else {
                writeln!(s, "    int r_ = 2, nar_ = 0; const uint32_t* m_ = 0; (void)m_; (void)nar_;").unwrap();
                writeln!(s, "    if (ss_gpu_ready(){}) {{", sub.concat()).unwrap();
                let e = entry(&mut s);
                if !masked {
                    s.push_str(&save);
                }
                writeln!(s, "      SsGpuArg g_[] = {{{}}};", gargs.join(", ")).unwrap();
                writeln!(s, "      r_ = ss_gpu_run(ss_msl, {e}, {}, g_, ss_n, ss_ny, {gx}, {gy}, {}, &m_);", gargs.len(), i32::from(masked) + i32::from(commit) + 2 * exact).unwrap();
                if !masked {
                    s.push_str(&restore);
                }
                writeln!(s, "    }}").unwrap();
                let rerun = if masked { "cpu rerun of flagged threads" } else { "cpu rerun (trap or subnormal)" };
                writeln!(s, "    ss_trace(\"{name}\", r_ == 0 ? (nar_ ? \"metal, 32-bit indices\" : \"metal\") : r_ == 1 ? \"{rerun}\" : \"cpu\", ss_n * ss_ny);").unwrap();
                if masked {
                    let m = c_lit(&format!("dev: the GPU failed while running {name}"));
                    writeln!(s, "    if (r_ == 3) dev_fail(st, FIDX, {m}, -1, 0, 0);").unwrap();
                    writeln!(s, "    if (r_) kc_{name}({}, ss_n, ss_ny, r_ == 1 ? m_ : 0, st);", cargs.join(", ")).unwrap();
                    writeln!(s, "    if (m_) free((void*)m_);").unwrap();
                } else {
                    writeln!(s, "    if (r_) kc_{name}({}, ss_n, ss_ny, 0, st);", cargs.join(", ")).unwrap();
                }
            }
        }
        writeln!(s, "  }}").unwrap();
        s.push_str(&back);
        writeln!(s, "  return ({rr}){{0, 0}};\n}}\n#undef FIDX").unwrap();
        if all_dev {
            let call: String = qargs.iter().map(|a| format!("{a}, ")).collect();
            writeln!(s, "static void kq_{cname}(void* r_, const uint32_t* m_, Status* st) {{\n  KQ_{cname}* q_ = (KQ_{cname}*)r_;\n  kc_{name}({call}q_->ss_n, q_->ss_ny, m_, st);\n}}").unwrap();
        }
        Ok(s)
    }

    pub(super) fn kernel_call(&mut self, name: &str, args: &[Expr]) -> G {
        let k = self.check.kernels.get(name).cloned().ok_or("unknown kernel")?;
        let f = self.fn_def(name).cloned().ok_or("unknown kernel")?;
        let mut vals = Vec::new();
        let mut devs = Vec::new();
        for (a, p) in args.iter().zip(&k.params) {
            let place = match &a.kind {
                ExprKind::Unary(UnOp::Ref | UnOp::RefMut, x) if p.slice.is_some() => &**x,
                _ => a,
            };
            let dev = p.slice.is_some() && matches!(self.ty(place)?, Type::Con(n, _) if n == "#DevBuf");
            devs.push(dev);
            let v = self.expr(place)?;
            vals.push(if p.slice == Some(true) && !dev { format!("&({v})") } else { v });
        }
        let cname = if devs.contains(&true) { format!("{name}__{}", devs.iter().map(|d| if *d { 'D' } else { 'L' }).collect::<String>()) } else { name.to_string() };
        if cname != name && self.helpers_done.insert(format!("kernel {cname}")) {
            let (params, ret) = self.check.fn_types.get(name).cloned().ok_or("kernel has no type")?;
            let fidx = *self.fn_index.get(name).ok_or("kernel has no index")?;
            let code = self.kernel_fn(&f, &params, &ret, fidx, &k, &devs, &cname)?;
            let proto = code.lines().nth(1).ok_or("empty kernel wrapper")?.trim_end_matches(" {").to_string();
            writeln!(self.protos, "{proto};").unwrap();
            self.helpers_late.push_str(&code);
        }
        let mut s = String::from("({ ");
        let mut names = Vec::new();
        for v in vals {
            let t = self.fresh("ka");
            write!(s, "__auto_type {t} = {v}; ").unwrap();
            names.push(t);
        }
        let call: String = names.iter().map(|n| format!("{n}, ")).collect();
        write!(s, "f_{cname}({call}st, depth).v; }})").unwrap();
        Ok(s)
    }

    pub(super) fn dev_new(&mut self, n: &str, list: &str, t: &Type) -> G {
        let kt = dev_elem(t);
        let e = elem_c(kt, Tgt::C);
        let es = es(kt);
        let fill = match kt {
            KTy::F32 => "h_[j_] = (float)l_.data[j_];".to_string(),
            KTy::F64 | KTy::Int => format!("h_[j_] = ({e})l_.data[j_];"),
            _ => {
                let (lo, hi) = kt.range();
                format!("int64_t v_ = l_.data[j_]; if (UNLIKELY(v_ < {lo}LL || v_ > {hi}LL)) dev_fail(st, FIDX, \"dev: element \", j_, \" of {n} is out of range for {} (value = \", v_); h_[j_] = ({e})v_;", kt.name())
            }
        };
        Ok(format!("({{ __auto_type l_ = {list}; SsDev* d_ = ss_dev_alloc(l_.len, {es}); {e}* h_ = ({e}*)d_->host; for (int64_t j_ = 0; j_ < l_.len; j_++) {{ {fill} }} d_; }})"))
    }

    /// `dev_f32((a..b).map(f))` and the other `dev_*` constructors of a mapped range: each value
    /// goes straight into the device buffer instead of a host list first. An element out of range
    /// for a 32-bit buffer is reported after the last `f`, where the unfused upload would find it.
    pub(super) fn dev_new_range(&mut self, n: &str, a: &Expr, b: &Expr, f: &Expr, t: &Type) -> G {
        let kt = dev_elem(t);
        let e = elem_c(kt, Tgt::C);
        let es = es(kt);
        let (av, bv) = (self.expr(a)?, self.expr(b)?);
        let (i, x) = (self.fresh("i"), self.fresh("x"));
        let m = self.know.facts.len();
        self.for_range_facts(&x, a, b);
        let body = self.apply(f, vec![(x.clone(), Type::int())]);
        self.know.facts.truncate(m);
        let body = body?;
        let (fill, check) = match kt {
            KTy::F32 => (format!("h_[{i}] = (float)({body});"), String::new()),
            KTy::F64 | KTy::Int => (format!("h_[{i}] = ({e})({body});"), String::new()),
            _ => {
                let (lo, hi) = kt.range();
                (
                    format!("int64_t v_ = {body}; if (UNLIKELY((v_ < {lo}LL || v_ > {hi}LL) && bad_ < 0)) {{ bad_ = {i}; bv_ = v_; }} h_[{i}] = ({e})v_;"),
                    format!("if (UNLIKELY(bad_ >= 0)) dev_fail(st, FIDX, \"dev: element \", bad_, \" of {n} is out of range for {} (value = \", bv_); ", kt.name()),
                )
            }
        };
        let bad = if check.is_empty() { "" } else { "int64_t bad_ = -1, bv_ = 0; " };
        Ok(format!("({{ int64_t s_ = {av}; int64_t re_ = {bv}; int64_t n_ = re_ > s_ ? re_ - s_ : 0; if (UNLIKELY(n_ > (((int64_t)1 << 31) / 8))) sspur_trap((Status*)gc_root_ptr, 15, 0, 0, 0); SsDev* d_ = ss_dev_alloc(n_, {es}); {e}* h_ = ({e}*)d_->host; {bad}for (int64_t {i} = 0; {i} < n_; {i}++) {{ int64_t {x} = s_ + {i}; {fill} }} {check}d_; }})"))
    }

    /// `d.to_list.sum` without the list: the same sequential sum over the converted elements.
    pub(super) fn dev_sum(&mut self, r: &str, rt: &Type) -> String {
        let kt = dev_elem(rt);
        let e = elem_c(kt, Tgt::C);
        let head = format!("SsDev* d_ = {r}; ss_q_sync(st); {e}* h_ = ({e}*)d_->host; ");
        if kt.is_float() {
            format!("({{ {head}double s_ = -0.0; for (int64_t j_ = 0; j_ < d_->len; j_++) s_ += ss_canon((double)h_[j_]); d_->len ? s_ : 0.0; }})")
        } else {
            format!("({{ {head}int64_t s_ = 0; for (int64_t j_ = 0; j_ < d_->len; j_++) if (UNLIKELY(__builtin_add_overflow(s_, (int64_t)h_[j_], &s_))) TRAPV({T_OVERFLOW}, 0, 0); s_; }})")
        }
    }

    pub(super) fn dev_method(&mut self, r: &str, rt: &Type, name: &str, t: &Type) -> G {
        match name {
            "len" => Ok(format!("(({r})->len)")),
            "to_list" => {
                let kt = dev_elem(rt);
                let lc = self.cty(t)?;
                let e = elem_c(kt, Tgt::C);
                let (oc, conv) = if kt.is_float() { ("double", "ss_canon((double)h_[j_])") } else { ("int64_t", "(int64_t)h_[j_]") };
                Ok(format!("({{ SsDev* d_ = {r}; ss_q_sync(st); {e}* h_ = ({e}*)d_->host; RawL r_ = raw_alloc_a(d_->len, 8, 1); {oc}* o_ = ({oc}*)r_.data; for (int64_t j_ = 0; j_ < d_->len; j_++) o_[j_] = {conv}; r_.hdr[1] = d_->len; ({lc}){{d_->len, o_, r_.hdr}}; }})"))
            }
            _ => Err(format!("uses DevBuf.{name}")),
        }
    }
}
