//! `F64` in the encoding, used only for `W_CONVERSION_MAY_TRAP` (ADR 0044). A value is a class
//! (`-1` -inf, `0` finite, `1` +inf, `2` NaN, so that numbers order like SSPUR's total order) and
//! a real that means something only when the class is 0. Each rounded operation's result is
//! within a relative 2^-53 (plus 2^-64 for subnormals) of the exact one, rounds monotonically
//! past 0, 1 and 2^63, and overflows only past the largest double.

use super::*;

const FMAX: &str = "179769313486231570814527423731704356798070567525844996598917476803157260780028538760589558632766878171540458953514382464234321326889464182768467546703537516986049910576551282076245490090389328944075868508455133942304583236903222948165808559332123348274797826204144723168738177180919299881250404026184124858368.0";
pub(crate) const P63: &str = "9223372036854775808.0";

pub(crate) fn prelude() -> String {
    let fu = "(/ 1.0 9007199254740992.0)";
    let tiny = "(/ 1.0 18446744073709551616.0)";
    format!(
        "(define-fun fabs ((x Real)) Real (ite (>= x 0.0) x (- x)))
(define-fun fmax () Real {FMAX})
(define-fun fop ((c Int) (r Real) (e Real)) Bool (and (<= (- 1) c) (<= c 1) (=> (<= (fabs e) fmax) (= c 0)) (=> (= c 1) (> e fmax)) (=> (= c (- 1)) (< e (- fmax))) (=> (= c 0) (and (<= (fabs (- r e)) (+ (* {fu} (fabs e)) {tiny})) (<= (fabs r) fmax) (=> (>= e 0.0) (>= r 0.0)) (=> (<= e 0.0) (<= r 0.0)) (=> (<= (fabs e) 1.0) (<= (fabs r) 1.0)) (=> (<= (fabs e) {P63}) (<= (fabs r) {P63}))))))
(define-fun flt ((a Int) (x Real) (b Int) (y Real)) Bool (or (< a b) (and (= a 0) (= b 0) (< x y))))
(define-fun fle ((a Int) (x Real) (b Int) (y Real)) Bool (or (< a b) (and (= a b) (or (not (= a 0)) (<= x y)))))
(define-fun feq ((a Int) (x Real) (b Int) (y Real)) Bool (and (= a b) (or (not (= a 0)) (= x y))))
(define-fun fadd_c ((a Int) (b Int)) Int (ite (or (= a 2) (= b 2) (= (+ a b) 0)) 2 (ite (= a 0) b a)))
(define-fun fsgn ((a Int) (x Real)) Int (ite (= a 0) (ite (> x 0.0) 1 (ite (< x 0.0) (- 1) 0)) a))
(define-fun fmul_c ((a Int) (x Real) (b Int) (y Real)) Int (ite (or (= a 2) (= b 2) (= (fsgn a x) 0) (= (fsgn b y) 0)) 2 (ite (= (fsgn a x) (fsgn b y)) 1 (- 1))))
"
    )
}

/// The decimal digits of `m * 2^k`.
pub(crate) fn times_pow2(m: u64, k: u32) -> String {
    let mut limbs: Vec<u64> = vec![m % 1_000_000_000, m / 1_000_000_000 % 1_000_000_000, m / 1_000_000_000_000_000_000];
    for _ in 0..k {
        let mut carry = 0;
        for l in &mut limbs {
            let v = *l * 2 + carry;
            *l = v % 1_000_000_000;
            carry = v / 1_000_000_000;
        }
        if carry > 0 {
            limbs.push(carry);
        }
    }
    while limbs.len() > 1 && limbs.last() == Some(&0) {
        limbs.pop();
    }
    let mut s = limbs.last().unwrap().to_string();
    for l in limbs.iter().rev().skip(1) {
        write!(s, "{l:09}").unwrap();
    }
    s
}

/// The exact value of a finite double as an SMT-LIB real.
pub(crate) fn real_lit(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "(- 0.0)" } else { "0.0" }.into();
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1 << 52) - 1);
    let (mut m, mut e) = if exp == 0 { (frac, -1074) } else { (frac | (1 << 52), exp - 1075) };
    while m & 1 == 0 {
        m >>= 1;
        e += 1;
    }
    let s = if e >= 0 { format!("{}.0", times_pow2(m, e as u32)) } else { format!("(/ {m}.0 {}.0)", times_pow2(1, (-e) as u32)) };
    if x < 0.0 { format!("(- {s})") } else { s }
}

/// A real literal: no symbol in it.
fn is_lit(t: &str) -> bool {
    !t.contains('k')
}

pub(crate) const CONV: &[&str] = &["round", "floor", "ceil", "trunc"];
pub(crate) const FPURE: &[&str] = &["is_nan", "is_finite", "is_inf", "sqrt", "round", "floor", "ceil", "trunc", "log10", "log2", "ln", "exp", "pow", "cbrt", "asin", "acos", "tan", "sinh", "cosh", "hypot", "sin", "cos", "tanh", "atan", "atan2"];

impl<'a> Enc<'a> {
    pub(crate) fn f_lit(&mut self, x: f64) -> Val {
        if x.is_nan() {
            let r = self.sym("Real");
            Val::F("2".into(), r)
        } else if x.is_infinite() {
            let r = self.sym("Real");
            Val::F(if x > 0.0 { "1" } else { "(- 1)" }.into(), r)
        } else {
            let t = real_lit(x);
            self.flits.insert(t.clone(), x);
            Val::F("0".into(), t)
        }
    }

    fn lit_of(&self, c: &str, r: &str) -> Option<f64> {
        if c == "0" { self.flits.get(r).copied() } else { None }
    }

    /// A function of literals evaluated here: exactly for the correctly rounded `sqrt`, within
    /// 2^-50 for the others, whose last bits differ between C libraries.
    fn fold(&mut self, name: &str, x: f64, args: &[f64]) -> Option<Val> {
        let y = args.first().copied();
        let v = match (name, y) {
            ("abs", None) => return Some(self.f_lit(x.abs())),
            ("sqrt", None) => return Some(self.f_lit(x.sqrt())),
            ("sin", None) => x.sin(),
            ("cos", None) => x.cos(),
            ("tan", None) => x.tan(),
            ("asin", None) => x.asin(),
            ("acos", None) => x.acos(),
            ("atan", None) => x.atan(),
            ("sinh", None) => x.sinh(),
            ("cosh", None) => x.cosh(),
            ("tanh", None) => x.tanh(),
            ("exp", None) => x.exp(),
            ("ln", None) => x.ln(),
            ("log2", None) => x.log2(),
            ("log10", None) => x.log10(),
            ("cbrt", None) => x.cbrt(),
            ("pow", Some(y)) => x.powf(y),
            ("atan2", Some(y)) => x.atan2(y),
            ("hypot", Some(y)) => x.hypot(y),
            _ => return None,
        };
        if !v.is_finite() || v.abs() > 1e300 || args.len() > 1 || x.abs() > 1e9 || y.is_some_and(|y| y.abs() > 1e9) {
            return None;
        }
        let r = self.sym("Real");
        self.facts.push(format!("(<= (fabs (- {r} {})) (+ {} (/ 1.0 18446744073709551616.0)))", real_lit(v), real_lit(v.abs() / 1125899906842624.0)));
        Some(Val::F("0".into(), r))
    }

    pub(crate) fn f_fresh(&mut self) -> Val {
        let c = self.int_sym("(- 1)", "2");
        Val::F(c, self.sym("Real"))
    }

    fn f_pair(&mut self) -> (String, String) {
        let c = self.int_sym("(- 1)", "2");
        (c, self.sym("Real"))
    }

    pub(crate) fn f_cmp(&mut self, op: BinOp, a: (&str, &str), b: (&str, &str)) -> String {
        let t = |f: &str, x: (&str, &str), y: (&str, &str)| format!("({f} {} {} {} {})", x.0, x.1, y.0, y.1);
        match op {
            BinOp::Lt => t("flt", a, b),
            BinOp::Le => t("fle", a, b),
            BinOp::Gt => t("flt", b, a),
            BinOp::Ge => t("fle", b, a),
            BinOp::Eq => t("feq", a, b),
            _ => format!("(not {})", t("feq", a, b)),
        }
    }

    pub(crate) fn f_neg(&mut self, c: &str, r: &str) -> Val {
        let c = if c == "0" { c.to_string() } else { format!("(ite (= {c} 2) 2 (- {c}))") };
        Val::F(c, format!("(- {r})"))
    }

    fn memo(&mut self, key: String, f: impl FnOnce(&mut Self) -> Val) -> Val {
        if let Some(v) = self.fmemo.get(&key) {
            return v.clone();
        }
        let v = f(self);
        self.fmemo.insert(key, v.clone());
        v
    }

    pub(crate) fn f_arith(&mut self, op: BinOp, a: (String, String), b: (String, String)) -> Val {
        self.memo(format!("{op:?} {} {} {} {}", a.0, a.1, b.0, b.1), |s| s.f_arith_new(op, a, b))
    }

    fn f_arith_new(&mut self, op: BinOp, a: (String, String), b: (String, String)) -> Val {
        let ((ca, ra), (cb, rb)) = (a, b);
        if let (Some(x), Some(y)) = (self.lit_of(&ca, &ra), self.lit_of(&cb, &rb)) {
            match op {
                BinOp::Add => return self.f_lit(x + y),
                BinOp::Sub => return self.f_lit(x - y),
                BinOp::Mul => return self.f_lit(x * y),
                BinOp::Div => return self.f_lit(x / y),
                _ => {}
            }
        }
        let (op, cb, rb) = match op {
            BinOp::Sub => match self.f_neg(&cb, &rb) {
                Val::F(c, r) => (BinOp::Add, c, r),
                _ => unreachable!(),
            },
            _ => (op, cb, rb),
        };
        let o = match op {
            BinOp::Add => "+",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            _ => return self.f_fresh(),
        };
        let e = self.define("Real", format!("({o} {ra} {rb})"));
        if !is_lit(&ra) && !is_lit(&rb) {
            self.facts.push(match op {
                BinOp::Mul => format!("(and (=> (<= (fabs {rb}) 1.0) (<= (fabs {e}) (fabs {ra}))) (=> (<= (fabs {ra}) 1.0) (<= (fabs {e}) (fabs {rb}))) (=> (and (>= {ra} 0.0) (>= {rb} 0.0)) (>= {e} 0.0)) (=> (and (<= {ra} 0.0) (<= {rb} 0.0)) (>= {e} 0.0)) (=> (and (>= {ra} 0.0) (<= {rb} 0.0)) (<= {e} 0.0)) (=> (and (<= {ra} 0.0) (>= {rb} 0.0)) (<= {e} 0.0)))"),
                BinOp::Div => format!("(and (=> (> {rb} 0.0) (and (=> (<= {ra} {rb}) (<= {e} 1.0)) (=> (>= {ra} (- {rb})) (>= {e} (- 1.0))) (=> (>= {ra} 0.0) (>= {e} 0.0)) (=> (<= {ra} 0.0) (<= {e} 0.0)) (=> (>= {rb} 1.0) (<= (fabs {e}) (fabs {ra}))))) (=> (< {rb} 0.0) (and (=> (>= {ra} {rb}) (<= {e} 1.0)) (=> (<= {ra} (- {rb})) (>= {e} (- 1.0))) (=> (>= {ra} 0.0) (<= {e} 0.0)) (=> (<= {ra} 0.0) (>= {e} 0.0)) (=> (<= {rb} (- 1.0)) (<= (fabs {e}) (fabs {ra}))))))"),
                _ => "true".into(),
            });
        }
        let (c, r) = self.f_pair();
        let fin = format!("(and (= {ca} 0) (= {cb} 0))");
        let mut fs = vec![];
        match op {
            BinOp::Add => {
                fs.push(format!("(=> {fin} (fop {c} {r} {e}))"));
                fs.push(format!("(=> (not {fin}) (= {c} (fadd_c {ca} {cb})))"));
            }
            BinOp::Mul => {
                fs.push(format!("(=> {fin} (fop {c} {r} {e}))"));
                fs.push(format!("(=> (not {fin}) (= {c} (fmul_c {ca} {ra} {cb} {rb})))"));
            }
            _ => {
                let inf = format!("(or (= {c} 1) (= {c} (- 1)))");
                fs.push(format!("(=> (and {fin} (not (= {rb} 0.0))) (fop {c} {r} {e}))"));
                fs.push(format!("(=> (and {fin} (= {rb} 0.0)) (ite (= {ra} 0.0) (= {c} 2) {inf}))"));
                fs.push(format!("(=> (or (= {ca} 2) (= {cb} 2) (and (not (= {ca} 0)) (not (= {cb} 0)))) (= {c} 2))"));
                fs.push(format!("(=> (and (or (= {ca} 1) (= {ca} (- 1))) (= {cb} 0)) (ite (> {rb} 0.0) (= {c} {ca}) (ite (< {rb} 0.0) (= {c} (- {ca})) {inf})))"));
                fs.push(format!("(=> (and (= {ca} 0) (or (= {cb} 1) (= {cb} (- 1)))) (and (= {c} 0) (= {r} 0.0)))"));
            }
        }
        self.facts.extend(fs);
        Val::F(c, r)
    }

    /// `n.to_f64`: rounded, exact up to 2^53.
    pub(crate) fn f_of_int(&mut self, n: &str) -> Val {
        self.memo(format!("to_f64 {n}"), |s| s.f_of_int_new(n))
    }

    fn f_of_int_new(&mut self, n: &str) -> Val {
        let r = self.sym("Real");
        for (m, q) in self.fints.iter().rev().take(16) {
            self.facts.push(format!("(and (=> (<= {n} {m}) (<= {r} {q})) (=> (<= {m} {n}) (<= {q} {r})))"));
        }
        self.fints.push((n.to_string(), r.clone()));
        self.facts.push(format!("(fop 0 {r} (to_real {n}))"));
        self.facts.push(format!("(=> (<= (fabs (to_real {n})) 9007199254740992.0) (= {r} (to_real {n})))"));
        Val::F("0".into(), r)
    }

    /// Selects by the total order: `min`, `max` and `clamp`.
    fn f_select(&mut self, cond: &str, a: (&str, &str), b: (&str, &str)) -> Val {
        let c = self.define("Int", format!("(ite {cond} {} {})", a.0, b.0));
        let r = self.define("Real", format!("(ite {cond} {} {})", a.1, b.1));
        Val::F(c, r)
    }

    /// A builtin global on `F64`s (`min max clamp inf nan pi rand_f64`), or `None`.
    pub(crate) fn f_global(&mut self, g: &str, vals: &[Val], e: &Expr) -> Option<Val> {
        let is_f = is_ty(self.ty(e), "F64");
        Some(match (g, vals) {
            ("min" | "max", [Val::F(cx, rx), Val::F(cy, ry)]) if is_f => {
                let cond = if g == "min" { format!("(flt {cy} {ry} {cx} {rx})") } else { format!("(flt {cx} {rx} {cy} {ry})") };
                self.f_select(&cond, (cy, ry), (cx, rx))
            }
            ("clamp", [Val::F(cx, rx), Val::F(cl, rl), Val::F(ch, rh)]) if is_f => {
                let inner = self.f_select(&format!("(flt {ch} {rh} {cx} {rx})"), (ch, rh), (cx, rx));
                let Val::F(ci, ri) = inner else { unreachable!() };
                self.f_select(&format!("(flt {cx} {rx} {cl} {rl})"), (cl, rl), (&ci, &ri))
            }
            ("clamp", [Val::Int(x), Val::Int(lo), Val::Int(hi)]) if is_ty(self.ty(e), "Int") => Val::Int(self.define("Int", format!("(ite (< {x} {lo}) {lo} (ite (> {x} {hi}) {hi} {x}))"))),
            ("inf", []) if is_f => self.f_lit(f64::INFINITY),
            ("nan", []) if is_f => self.f_lit(f64::NAN),
            ("pi", []) if is_f => self.f_lit(std::f64::consts::PI),
            ("rand_f64", [_]) => {
                let r = self.sym("Real");
                self.facts.push(format!("(and (<= 0.0 {r}) (<= {r} (/ 9007199254740991.0 9007199254740992.0)))"));
                Val::Tup(vec![Val::F("0".into(), r), Val::Int(self.int_sym(MIN, MAX))])
            }
            _ => return None,
        })
    }

    /// A builtin method on an `F64` receiver, or `None` when it isn't modeled.
    pub(crate) fn f_method(&mut self, e: &Expr, name: &str, c: &str, r: &str, args: &[Val]) -> Option<Val> {
        if let Some(x) = self.lit_of(c, r) {
            let ys: Option<Vec<f64>> = args.iter().map(|a| if let Val::F(c, r) = a { self.lit_of(c, r) } else { None }).collect();
            if let Some(v) = ys.and_then(|ys| self.fold(name, x, &ys)) {
                return Some(v);
            }
        }
        if CONV.contains(&name) || !args.iter().all(|a| matches!(a, Val::F(..))) {
            return self.f_method_new(e, name, c, r, args);
        }
        let key = format!("{name} {c} {r} {args:?}");
        if let Some(v) = self.fmemo.get(&key) {
            return Some(v.clone());
        }
        let v = self.f_method_new(e, name, c, r, args)?;
        self.fmemo.insert(key, v.clone());
        Some(v)
    }

    fn f_method_new(&mut self, e: &Expr, name: &str, c: &str, r: &str, args: &[Val]) -> Option<Val> {
        let unary = |s: &mut Self, cls: String, fact: &dyn Fn(&str) -> String| {
            let c2 = s.define("Int", cls);
            let r2 = s.sym("Real");
            s.facts.push(fact(&r2));
            Val::F(c2, r2)
        };
        Some(match (name, args) {
            ("abs", []) => {
                let c2 = if c == "0" { c.to_string() } else { format!("(ite (= {c} 2) 2 (ite (= {c} 0) 0 1))") };
                Val::F(c2, format!("(fabs {r})"))
            }
            ("is_nan", []) => Val::Bool(format!("(= {c} 2)")),
            ("is_finite", []) => Val::Bool(format!("(= {c} 0)")),
            ("is_inf", []) => Val::Bool(format!("(or (= {c} 1) (= {c} (- 1)))")),
            ("sqrt", []) => unary(self, format!("(ite (= {c} 0) (ite (>= {r} 0.0) 0 2) (ite (= {c} 1) 1 2))"), &|s| {
                format!("(=> (and (= {c} 0) (>= {r} 0.0)) (and (>= {s} 0.0) (<= {s} (* (+ (/ {r} 2.0) 0.5) (+ 1.0 (/ 1.0 4503599627370496.0)))) (=> (<= {r} 1.0) (<= {s} 1.0)) (=> (>= {r} 1.0) (and (>= {s} 1.0) (<= {s} {r})))))")
            }),
            ("log10" | "log2" | "ln", []) => {
                let (lo, hi) = match name {
                    "log10" => ("(- 324.0)", "309.0"),
                    "log2" => ("(- 1075.0)", "1025.0"),
                    _ => ("(- 745.0)", "710.0"),
                };
                unary(self, format!("(ite (= {c} 0) (ite (> {r} 0.0) 0 (ite (= {r} 0.0) (- 1) 2)) (ite (= {c} 1) 1 2))"), &|s| {
                    format!("(=> (and (= {c} 0) (> {r} 0.0)) (and (<= {lo} {s}) (<= {s} {hi}) (=> (>= {r} 1.0) (>= {s} 0.0)) (=> (<= {r} 1.0) (<= {s} 0.0))))")
                })
            }
            ("exp", []) => {
                let (c2, r2) = self.f_pair();
                self.facts.push(format!("(and (=> (= {c} 0) (and (or (= {c2} 0) (= {c2} 1)) (=> (<= {r} 709.0) (= {c2} 0)))) (=> (= {c} 1) (= {c2} 1)) (=> (= {c} (- 1)) (and (= {c2} 0) (= {r2} 0.0))) (=> (= {c} 2) (= {c2} 2)))"));
                self.facts.push(format!("(=> (and (= {c} 0) (= {c2} 0)) (and (>= {r2} 0.0) (=> (>= {r} 0.0) (>= {r2} 1.0)) (=> (<= {r} 0.0) (<= {r2} 1.0)) (=> (>= {r} (- 700.0)) (> {r2} 0.0)) (=> (<= {r} 43.0) (<= {r2} 5000000000000000000.0))))"));
                Val::F(c2, r2)
            }
            ("cbrt", []) => unary(self, format!("(ite (= {c} 0) 0 {c})"), &|s| format!("(=> (= {c} 0) (and (<= (fabs {s}) (+ 1.0 (fabs {r}))) (=> (>= {r} 0.0) (>= {s} 0.0)) (=> (<= {r} 0.0) (<= {s} 0.0))))")),
            ("asin" | "acos", []) => unary(self, format!("(ite (and (= {c} 0) (<= (fabs {r}) 1.0)) 0 2)"), &|s| format!("(<= (fabs {s}) 4.0)")),
            ("sin" | "cos", []) => unary(self, format!("(ite (= {c} 0) 0 2)"), &|s| format!("(=> (= {c} 0) (<= (fabs {s}) 1.0))")),
            ("tanh" | "atan", []) => unary(self, format!("(ite (= {c} 2) 2 0)"), &|s| format!("(=> (not (= {c} 2)) (<= (fabs {s}) 2.0))")),
            ("atan2", [Val::F(cx, _)]) => unary(self, format!("(ite (or (= {c} 2) (= {cx} 2)) 2 0)"), &|s| format!("(<= (fabs {s}) 4.0)")),
            ("pow", [Val::F(cb, rb)]) => {
                let (c2, r2) = self.f_pair();
                self.facts.push(format!(
                    "(=> (and (= {c} 0) (= {cb} 0) (>= {r} 0.0) (>= {rb} 0.0) (<= {rb} 1.0)) (and (= {c2} 0) (>= {r2} 0.0) (<= {r2} (+ (* (+ 1.0 (* {rb} (- {r} 1.0))) (+ 1.0 (/ 1.0 1125899906842624.0))) (/ 1.0 18446744073709551616.0)))))"
                ));
                Val::F(c2, r2)
            }
            (m, []) if CONV.contains(&m) && is_ty(self.ty(e), "Int") => {
                let ok = format!("(and (= {c} 0) (<= (- {P63}) {r}) (< {r} {P63}))");
                self.checkpoint(e, "conv-finite", Some(format!("(= {c} 0)")));
                self.checkpoint(e, "conv", Some(ok.clone()));
                let n = self.int_sym(MIN, MAX);
                let rel = match m {
                    "floor" => format!("(and (<= (to_real {n}) {r}) (< (- {r} 1.0) (to_real {n})))"),
                    "ceil" => format!("(and (<= {r} (to_real {n})) (< (to_real {n}) (+ {r} 1.0)))"),
                    _ => format!("(and (< (- {r} 1.0) (to_real {n})) (< (to_real {n}) (+ {r} 1.0)))"),
                };
                self.assume(&format!("(and {ok} {rel})"));
                Val::Int(n)
            }
            _ => return None,
        })
    }
}

fn ast_ty(t: &Ty) -> Option<Type> {
    match t {
        Ty::Named { name, args, .. } => Some(Type::Con(name.clone(), args.iter().map(ast_ty).collect::<Option<_>>()?)),
        Ty::Tuple(ts) => Some(Type::Tuple(ts.iter().map(ast_ty).collect::<Option<_>>()?)),
        Ty::Fn { .. } => None,
    }
}

impl<'a> Enc<'a> {
    fn params_of(&mut self, f: &'a FnDef) -> Vec<(String, Val)> {
        f.params.iter().map(|p| (p.name.clone(), ast_ty(&p.ty))).collect::<Vec<_>>().into_iter().map(|(n, t)| (n, self.fresh_of(t.as_ref()))).collect()
    }

    fn entry_facts(&mut self, f: &'a FnDef, vals: &[(String, Val)]) -> Vec<String> {
        let mut out = vec![];
        for (i, c) in self.entry_conds(f) {
            let mut binds = vals.to_vec();
            if let Some(i) = i {
                binds.push(("_".into(), vals[i].1.clone()));
            }
            out.extend(self.goal_with(binds, c));
        }
        out
    }

    /// A function body on its own, with no `post` obligations: impl and trait methods.
    pub(crate) fn plain_fn(&mut self, f: &'a FnDef) {
        self.reset(&f.name, false);
        self.roots = vec![&f.body];
        let vals = self.params_of(f);
        self.env.push(vals.iter().map(|(n, v)| (n.clone(), Bind::Val(v.clone()))).collect());
        let facts = self.entry_facts(f, &vals);
        self.facts.extend(facts);
        self.ex(&f.body);
        self.finish(false, vec![]);
    }

    /// A local function, walked where it is defined under a fresh guard.
    pub(crate) fn local_fn(&mut self, f: &'a FnDef) {
        if !self.goals.is_empty() {
            return;
        }
        let vals = self.params_of(f);
        self.forget_vars();
        self.roots.push(&f.body);
        self.guard(|s| {
            s.env.push(vals.iter().map(|(n, v)| (n.clone(), Bind::Val(v.clone()))).collect());
            for t in s.entry_facts(f, &vals) {
                s.assume(&t);
            }
            s.ex(&f.body);
            s.env.pop();
        });
        self.roots.pop();
        self.forget_vars();
    }

    /// Drops what is known about every `var`: at loops and closures.
    pub(crate) fn forget_vars(&mut self) {
        for s in &mut self.env {
            for b in s.values_mut() {
                if let Bind::Var(v) = b {
                    *v = None;
                }
            }
        }
    }
}

/// Inlining limits (float mode): nesting, callee size, and nodes per function.
const INLINE_DEPTH: usize = 3;
const INLINE_SIZE: usize = 400;
pub(crate) const INLINE_BUDGET: usize = 4000;

pub(crate) fn size(e: &Expr) -> usize {
    let mut n = 0;
    visit::walk_expr(e, &mut |_| {
        n += 1;
        true
    });
    n
}

impl<'a> Enc<'a> {
    /// The value of a call to `g` with its body encoded in place, so that its sites see the
    /// caller's facts (site kinds get the call span as a suffix) and the caller sees its result.
    pub(crate) fn inline(&mut self, e: &Expr, g: &'a FnDef, binds: &[(String, Val)]) -> Option<Val> {
        if !self.floats || !self.goals.is_empty() || self.inlined.len() >= INLINE_DEPTH || g.name == self.fname || self.inlined.contains(&g.name) {
            return None;
        }
        if g.ext.is_some() || g.kernel.is_some() || matches!(g.body.kind, ExprKind::Table(_)) || count_returns(&g.body) > 0 {
            return None;
        }
        let n = size(&g.body);
        if n > INLINE_SIZE || n > self.budget {
            return None;
        }
        self.budget -= n;
        let scope = binds.iter().map(|(k, v)| (k.clone(), Bind::Val(v.clone()))).collect();
        let env = std::mem::replace(&mut self.env, vec![scope]);
        let chain = self.chain.clone();
        write!(self.chain, "@{}-{}", e.span.start, e.span.end).unwrap();
        self.inlined.push(g.name.clone());
        self.roots.push(&g.body);
        let frames = std::mem::take(&mut self.frames);
        let v = self.ex(&g.body);
        self.frames = frames;
        self.roots.pop();
        self.inlined.pop();
        self.chain = chain;
        self.env = env;
        Some(if v == Val::Never { self.fresh_e(e) } else { v })
    }
}

/// Methods whose one-parameter callback gets the receiver's elements.
const ELEM_METHODS: &[&str] = &["map", "filter", "any", "all", "each", "flat_map", "count", "find", "filter_map", "find_index", "take_while", "drop_while", "sort_by", "min_by", "max_by", "group_by", "partition", "sum_by"];

impl<'a> Enc<'a> {
    /// `(a..b).m(i => ..)` and `xs.filter(x => p).m(y => ..)`: the callback's parameter is in
    /// `a..b`, or satisfies `p`.
    pub(crate) fn elem_lambda(&mut self, e: &'a Expr, recv: &'a Expr, name: &str, args: &'a [Expr]) -> Option<Val> {
        let [arg] = args else { return None };
        let ExprKind::Lambda { params, body, .. } = &arg.kind else { return None };
        if params.len() != 1 || !self.goals.is_empty() || !ELEM_METHODS.contains(&name) {
            return None;
        }
        let elem = match self.ty(arg) {
            Some(Type::Fn(ps, ..)) => ps.first().cloned(),
            _ => None,
        };
        let (rv, cond, x) = match &recv.kind {
            ExprKind::Range(a, b) => {
                let rv = self.ex(recv);
                let (Val::Int(lo), Val::Int(hi)) = (self.ex(a), self.ex(b)) else { return None };
                let i = self.int_sym(MIN, MAX);
                (rv, format!("(and (<= {lo} {i}) (< {i} {hi}))"), Val::Int(i))
            }
            ExprKind::Method { recv: inner, name: f, args: fa, .. } if f == "filter" && !self.check.user_methods.contains(&(recv.span.start, recv.span.end)) => {
                let [pa] = fa.as_slice() else { return None };
                let ExprKind::Lambda { params: qs, body: pb, .. } = &pa.kind else { return None };
                if qs.len() != 1 {
                    return None;
                }
                let rv = self.ex(recv);
                let _ = inner;
                let x = self.fresh_of(elem.as_ref());
                self.env.push([(qs[0].clone(), Bind::Val(x.clone()))].into_iter().collect());
                let pv = self.guard(|s| s.ex(pb));
                self.env.pop();
                (rv, self.bool_of(pv), x)
            }
            _ => return None,
        };
        self.forget_vars();
        self.env.push([(params[0].clone(), Bind::Val(x))].into_iter().collect());
        self.guard(|s| s.under(cond, |s| s.ex(body)));
        self.env.pop();
        self.forget_vars();
        Some(self.builtin_method(e, recv, rv, name, vec![Val::Other]))
    }
}


/// An invariant candidate of a loop's `F64` var: the Bool that stands for it, which predicate
/// of `pred`, and the queries (entry, then each assignment) that must all be `unsat`.
pub(crate) struct Cand {
    pub fun: usize,
    pub b: String,
    k: usize,
    pub obls: Vec<Option<Query>>,
}

const PREDS: usize = 6;

fn pred(k: usize, c: &str, r: &str) -> String {
    match k {
        0 => format!("(= {c} 0)"),
        1 => format!("(fle 0 0.0 {c} {r})"),
        2 => format!("(fle {c} {r} 0 0.0)"),
        3 => format!("(and (= {c} 0) (<= (fabs {r}) 1.0))"),
        4 => format!("(and (= {c} 0) (<= (fabs {r}) 9007199254740992.0))"),
        _ => format!("(and (= {c} 0) (<= (fabs {r}) 4611686018427387904.0))"),
    }
}

/// The names assigned in `e`, outside closures, and those assigned inside closures.
fn assigns<'e>(e: &'e Expr, closure: bool, out: &mut Vec<(&'e str, &'e Expr)>, inner: &mut HashSet<&'e str>) {
    match &e.kind {
        ExprKind::Lambda { body, .. } => assigns(body, true, out, inner),
        ExprKind::Block(stmts) => {
            for st in stmts {
                match st {
                    Stmt::Assign(n, x, _) => {
                        if closure {
                            inner.insert(n);
                        } else {
                            out.push((n, x));
                        }
                        assigns(x, closure, out, inner);
                    }
                    Stmt::Fn(f) => assigns(&f.body, true, out, inner),
                    Stmt::Let(_, x) | Stmt::Var(_, x) | Stmt::Expr(x) => assigns(x, closure, out, inner),
                    Stmt::For(_, a, b) | Stmt::While(a, b) => {
                        assigns(a, closure, out, inner);
                        assigns(b, closure, out, inner);
                    }
                }
            }
        }
        _ => visit::children(e).into_iter().for_each(|c| assigns(c, closure, out, inner)),
    }
}

impl<'a> Enc<'a> {
    /// Whether a lambda or local function here assigns `n`, which then changes when it runs.
    pub(crate) fn closure_assigned(&self, n: &str) -> bool {
        let mut inner = HashSet::new();
        for r in &self.roots {
            assigns(r, false, &mut vec![], &mut inner);
        }
        inner.contains(n)
    }

    pub(crate) fn tracked(&self, n: &str) -> bool {
        self.frames.iter().any(|f| f.contains_key(n))
    }

    /// A value of the tracked var `n`, which every invariant candidate of it describes.
    pub(crate) fn f_invariant(&mut self, n: &str) -> Val {
        let (c, r) = self.f_pair();
        let cs: Vec<usize> = self.frames.iter().filter_map(|f| f.get(n)).flatten().copied().collect();
        for i in cs {
            let t = format!("(=> {} {})", self.an.cands[i].b, pred(self.an.cands[i].k, &c, &r));
            self.facts.push(t);
        }
        Val::F(c, r)
    }

    fn obligation(&mut self, i: usize, v: &Val) {
        let q = match v {
            Val::F(c, r) => Some(Query { fun: self.fidx, upto: self.facts.len(), extra: (0, 0), path: self.path.clone(), goal: pred(self.an.cands[i].k, c, r) }),
            _ => None,
        };
        self.an.cands[i].obls.push(q);
    }

    /// `n := v` in float mode: an obligation for each candidate of each loop that tracks `n`.
    pub(crate) fn assigned(&mut self, n: &str, v: Val) {
        let cs: Vec<usize> = self.frames.iter().filter_map(|f| f.get(n)).flatten().copied().collect();
        for i in &cs {
            self.obligation(*i, &v);
        }
        let tracked = !cs.is_empty();
        let nv = if tracked { Some(self.f_invariant(n)) } else { None };
        if let Some(s) = self.env.iter_mut().rev().find(|s| s.contains_key(n))
            && matches!(s[n], Bind::Var(_))
        {
            s.insert(n.to_string(), Bind::Var(nv));
        }
    }

    /// At a loop: the `F64` vars it assigns get invariant candidates, checked at entry here and
    /// at each assignment; the rest of what is known about vars is dropped.
    pub(crate) fn enter_loop(&mut self, body: &[&'a Expr]) -> Vec<String> {
        if !self.floats {
            self.forget_vars();
            return vec![];
        }
        let (mut out, mut inner) = (vec![], HashSet::new());
        for b in body {
            assigns(b, false, &mut out, &mut inner);
        }
        for r in &self.roots {
            assigns(r, false, &mut vec![], &mut inner);
        }
        let mut names: Vec<String> = vec![];
        for (n, x) in out {
            let is_var = self.env.iter().rev().find_map(|s| s.get(n)).is_some_and(|b| matches!(b, Bind::Var(_)));
            if is_var && !inner.contains(n) && is_ty(self.ty(x), "F64") && !names.iter().any(|m| m == n) {
                names.push(n.to_string());
            }
        }
        let mut frame = HashMap::new();
        for n in &names {
            let cur = match self.env.iter().rev().find_map(|s| s.get(n.as_str())) {
                Some(Bind::Var(Some(v))) => v.clone(),
                _ if self.tracked(n) => self.f_invariant(n),
                _ => self.f_fresh(),
            };
            let mut ids = vec![];
            for k in 0..PREDS {
                let b = self.sym("Bool");
                ids.push(self.an.cands.len());
                self.an.cands.push(Cand { fun: self.fidx, b, k, obls: vec![] });
            }
            for &i in &ids {
                self.obligation(i, &cur);
            }
            frame.insert(n.clone(), ids);
        }
        self.forget_vars();
        self.frames.push(frame);
        for n in &names {
            let v = self.f_invariant(n);
            if let Some(s) = self.env.iter_mut().rev().find(|s| s.contains_key(n.as_str())) {
                s.insert(n.clone(), Bind::Var(Some(v)));
            }
        }
        names
    }

    pub(crate) fn leave_loop(&mut self, names: Vec<String>) {
        if !self.floats {
            self.forget_vars();
            return;
        }
        let frame = self.frames.pop().unwrap_or_default();
        self.forget_vars();
        self.frames.push(frame);
        for n in &names {
            let v = self.f_invariant(n);
            if let Some(s) = self.env.iter_mut().rev().find(|s| s.contains_key(n.as_str())) {
                s.insert(n.clone(), Bind::Var(Some(v)));
            }
        }
        self.frames.pop();
    }
}
