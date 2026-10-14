use crate::value::{Fields, Value};
use crate::{Ctrl, Interp};
use sspur_syntax::*;
use std::collections::BTreeMap;
use std::rc::Rc;

pub struct Options {
    pub cases: usize,
    pub seed: u64,
    pub edge: bool,
}

pub struct Report {
    pub name: String,
    pub cases: usize,
    pub discarded: usize,
    pub skipped: Option<String>,
    pub failure: Option<(Vec<String>, String)>,
}

pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub(crate) fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }

    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

const ALLOWED_EFFECTS: &[&str] = &["fail", "log", "div"];
const MAX_DEPTH: u32 = 4;
pub(crate) const FUEL: u64 = 200_000;

pub(crate) fn is_input_error(msg: &str) -> bool {
    msg.starts_with("contract violated: pre ") || msg.starts_with("contract violated: parameter ")
}

impl Interp {
    pub fn fuzz(&self, opts: &Options) -> Vec<Report> {
        let mut names: Vec<&String> = self.fns.keys().collect();
        names.sort();
        let mut rng = Rng(opts.seed.max(1));
        names.into_iter().map(|n| self.fuzz_fn(&self.fns[n].clone(), opts, &mut rng)).collect()
    }

    pub fn differential(&self, opts: &Options) -> Vec<DiffReport> {
        let mut names: Vec<String> = self.fns.keys().filter(|n| self.native_has(n)).cloned().collect();
        names.sort();
        let mut rng = Rng(opts.seed.max(1));
        let saved = self.output.replace(Some(vec![]));
        let mut out = Vec::new();
        for n in names {
            let f = self.fns[&n].clone();
            let mut rep = DiffReport { name: n.clone(), cases: 0, native_cases: 0, mismatch: None };
            if f.params.iter().any(|p| has_fn_type(&p.ty)) || f.effects.iter().any(|e| matches!(e.name.as_str(), "conc" | "fs" | "io" | "time" | "proc" | "net")) {
                out.push(rep);
                continue;
            }
            let mut attempts = 0;
            let want = if f.params.is_empty() { 1 } else { opts.cases };
            while rep.cases < want && attempts < want * 10 {
                attempts += 1;
                let Some(args) = f.params.iter().map(|p| self.generate(&p.ty, &mut rng, opts, 0)).collect::<Option<Vec<_>>>() else { continue };
                self.bypass_native.set(true);
                self.depth.set(0);
                self.fuel.set(FUEL);
                *self.output.borrow_mut() = Some(vec![]);
                let interp = self.call_fn(&f, args.clone());
                let interp_log = self.output.replace(Some(vec![])).unwrap_or_default();
                self.fuel.set(u64::MAX);
                self.bypass_native.set(false);
                let interp = match interp {
                    Err(Ctrl::Trap(m)) if m == crate::OUT_OF_FUEL || m == "out of memory" || m.starts_with("stack overflow") => continue,
                    other => describe_result(other),
                };
                self.depth.set(0);
                let hits = self.native_hits.get();
                if std::env::var_os("SSPUR_FUZZ_TRACE").is_some() {
                    eprintln!("native {}({})", n, args.iter().map(|a| crate::value::Quoted(a).to_string()).collect::<Vec<_>>().join(", "));
                }
                let native = describe_result(self.call_fn(&f, args.clone()));
                let native_log = self.output.replace(Some(vec![])).unwrap_or_default();
                rep.cases += 1;
                if self.native_hits.get() > hits {
                    rep.native_cases += 1;
                }
                let (interp, native) = if interp == native && interp_log != native_log {
                    (format!("{interp} log {interp_log:?}"), format!("{native} log {native_log:?}"))
                } else {
                    (interp, native)
                };
                if interp != native {
                    rep.mismatch = Some((args.iter().map(|a| crate::value::Quoted(a).to_string()).collect(), interp, native));
                    break;
                }
            }
            out.push(rep);
        }
        *self.output.borrow_mut() = saved;
        out
    }

    pub(crate) fn skip_reason(&self, f: &FnDef) -> Option<String> {
        if f.params.is_empty() {
            return Some("no parameters".into());
        }
        if sspur_check::kernel::is_device_fn(f) {
            return Some("device fn, inlined into kernels".into());
        }
        if let Some(e) = f.effects.iter().find(|e| !ALLOWED_EFFECTS.contains(&e.name.as_str()) && !f.tparams.iter().any(|p| p.name == e.name)) {
            return Some(format!("performs '{}'", printer::effect(e)));
        }
        if f.params.iter().any(|p| has_fn_type(&p.ty)) {
            return Some("takes a function argument".into());
        }
        None
    }

    fn fuzz_fn(&self, f: &FnDef, opts: &Options, rng: &mut Rng) -> Report {
        let mut rep = Report { name: f.name.clone(), cases: 0, discarded: 0, skipped: self.skip_reason(f), failure: None };
        if rep.skipped.is_some() {
            return rep;
        }
        let saved = self.output.replace(Some(vec![]));
        let mut attempts = 0;
        while rep.cases < opts.cases && attempts < opts.cases * 20 {
            attempts += 1;
            let Some(args) = f.params.iter().map(|p| self.generate(&p.ty, rng, opts, 0)).collect::<Option<Vec<_>>>() else {
                rep.discarded += 1;
                continue;
            };
            match self.outcome(f, args.clone()) {
                Outcome::Discard => rep.discarded += 1,
                Outcome::Pass => rep.cases += 1,
                Outcome::Fail(msg) => {
                    let (args, msg) = self.shrink(f, args, msg);
                    rep.cases += 1;
                    rep.failure = Some((args.iter().map(|a| crate::value::Quoted(a).to_string()).collect(), msg));
                    break;
                }
            }
        }
        *self.output.borrow_mut() = saved;
        rep
    }

    fn outcome(&self, f: &FnDef, args: Vec<Value>) -> Outcome {
        self.depth.set(0);
        self.fuel.set(FUEL);
        let r = self.call_fn(f, args);
        self.fuel.set(u64::MAX);
        match r {
            Err(Ctrl::Trap(m)) if m == crate::OUT_OF_FUEL => Outcome::Discard,
            Ok(_) | Err(Ctrl::Raise(_)) => Outcome::Pass,
            Err(Ctrl::Trap(m)) if is_input_error(&m) => Outcome::Discard,
            Err(Ctrl::Trap(m)) => Outcome::Fail(m),
            Err(Ctrl::Return(_)) => Outcome::Pass,
            Err(c) => Outcome::Fail(crate::describe(c)),
        }
    }

    fn shrink(&self, f: &FnDef, mut args: Vec<Value>, mut msg: String) -> (Vec<Value>, String) {
        let mut budget = 3000;
        let mut improved = true;
        while improved && budget > 0 {
            improved = false;
            for i in 0..args.len() {
                let mut cands = self.subterms(&args[i]);
                cands.extend(smaller(&args[i]));
                for cand in cands {
                    budget -= 1;
                    let mut trial = args.clone();
                    trial[i] = cand;
                    if let Outcome::Fail(m) = self.outcome(f, trial.clone()) {
                        args = trial;
                        msg = m;
                        improved = true;
                        break;
                    }
                    if budget == 0 {
                        break;
                    }
                }
            }
        }
        (args, msg)
    }

    fn type_of_ctor(&self, ctor: &str) -> Option<&str> {
        self.types.values().find(|t| matches!(&t.body, TypeBody::Sum(vs) if vs.iter().any(|v| v.name == ctor))).map(|t| t.name.as_str())
    }

    fn subterms(&self, v: &Value) -> Vec<Value> {
        let Value::Variant(n, Some(fs)) = v else { return vec![] };
        let Some(ty) = self.type_of_ctor(n) else { return vec![] };
        fs.iter()
            .filter_map(|(_, fv)| match fv {
                Value::Variant(m, _) if self.type_of_ctor(m) == Some(ty) => Some(fv.clone()),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn generate(&self, t: &Ty, rng: &mut Rng, opts: &Options, depth: u32) -> Option<Value> {
        let Ty::Named { name, args, .. } = t else {
            return match t {
                Ty::Tuple(xs) => Some(Value::Tuple(Rc::new(xs.iter().map(|x| self.generate(x, rng, opts, depth + 1)).collect::<Option<_>>()?))),
                _ => None,
            };
        };
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Ty::Named { name: "Int".into(), args: vec![], span: Span::default() });
        let deep = depth >= MAX_DEPTH;
        Some(match name.as_str() {
            "Int" | "I8" | "I16" | "I32" | "U8" | "U16" | "U32" | "U64" => {
                let unsigned = name.starts_with('U');
                let v = if opts.edge && rng.chance(10) {
                    [0, 1, -1, i64::MAX, i64::MIN, i64::MAX - 1][rng.below(6) as usize]
                } else if rng.chance(50) {
                    rng.range(-10, 10)
                } else if rng.chance(70) {
                    rng.range(-1000, 1000)
                } else {
                    rng.range(-1_000_000, 1_000_000)
                };
                Value::Int(if unsigned { v.checked_abs().unwrap_or(0) } else { v })
            }
            "F64" | "F32" => Value::Float(if opts.edge && rng.chance(12) {
                [-0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, f64::MIN_POSITIVE, 5e-324, 0.1, 9007199254740993.0, -1e300][rng.below(10) as usize]
            } else if rng.chance(15) {
                0.0
            } else {
                rng.range(-100_000, 100_000) as f64 / 100.0
            }),
            "Bool" => Value::Bool(rng.chance(50)),
            "Unit" => Value::Unit,
            "Str" => {
                const CHARS: &[char] = &['a', 'b', 'c', 'x', 'y', 'z', ' ', 'A', 'Z', '0', '9', '-', '_', '.', ',', 'é', 'ß', '"', '\\', '\n', '日'];
                let len = if rng.chance(15) { 0 } else { rng.below(9) };
                Value::str(&(0..len).map(|_| CHARS[rng.below(CHARS.len() as u64) as usize]).collect::<String>())
            }
            "List" => {
                let len = if deep { 0 } else { rng.below(7) };
                Value::list((0..len).map(|_| self.generate(&arg(0), rng, opts, depth + 1)).collect::<Option<_>>()?)
            }
            "Opt" => {
                if deep || rng.chance(30) {
                    Value::Opt(None)
                } else {
                    Value::some(self.generate(&arg(0), rng, opts, depth + 1)?)
                }
            }
            "Res" => {
                if rng.chance(50) {
                    Value::Res(Ok(Rc::new(self.generate(&arg(0), rng, opts, depth + 1)?)))
                } else {
                    Value::Res(Err(Rc::new(self.generate(&arg(1), rng, opts, depth + 1)?)))
                }
            }
            "Secret" | "Pii" | "Untrusted" => Value::Wrap(name.as_str().into(), Rc::new(self.generate(&arg(0), rng, opts, depth + 1)?)),
            "Guess" => Value::Guess(Rc::new(self.generate(&arg(0), rng, opts, depth + 1)?), rng.below(101) as f64 / 100.0),
            "Map" | "HashMap" if name == "Map" || !self.types.contains_key(name) => {
                let mut m = BTreeMap::new();
                for _ in 0..if deep { 0 } else { rng.below(5) } {
                    m.insert(self.generate(&arg(0), rng, opts, depth + 1)?, self.generate(&arg(1), rng, opts, depth + 1)?);
                }
                Value::Map(Rc::new(m))
            }
            "Set" | "Heap" | "HashSet" if !self.types.contains_key(name) => {
                let mut xs = Vec::new();
                for _ in 0..if deep { 0 } else { rng.below(6) } {
                    xs.push(self.generate(&arg(0), rng, opts, depth + 1)?);
                }
                xs.sort();
                if name != "Heap" {
                    Value::Set(Rc::new(xs.into_iter().collect()))
                } else {
                    Value::Heap(Rc::new(xs))
                }
            }
            "Time" | "Duration" if !self.types.contains_key(name) => match self.generate(&Ty::Named { name: "Int".into(), args: vec![], span: Span::default() }, rng, opts, depth)? {
                Value::Int(n) if name == "Time" => Value::Time(n),
                Value::Int(n) => Value::Dur(n),
                _ => return None,
            },
            "BigInt" | "Dec" if !self.types.contains_key(name) => {
                let int_ty = Ty::Named { name: "Int".into(), args: vec![], span: Span::default() };
                let mut x = sspur_native::bigint::Big::from_i64(0);
                for _ in 0..1 + rng.below(3) {
                    let Value::Int(k) = self.generate(&int_ty, rng, opts, depth)? else { return None };
                    x = x.mul(&sspur_native::bigint::Big::from_i64(1 << 32)).ok()?.add(&sspur_native::bigint::Big::from_i64(k));
                }
                if name == "Dec" {
                    Value::Dec(Rc::new(x), rng.below(6) as i64)
                } else {
                    Value::Big(Rc::new(x))
                }
            }
            "Bits" if !self.types.contains_key(name) => {
                let n = if rng.chance(10) { 0 } else { rng.below(140) as i64 };
                let xs: Vec<Value> = (0..n).filter(|_| rng.chance(40)).map(Value::Int).collect();
                crate::stdx::bits_from(&xs, n).ok()?
            }
            "FlatMap" if !self.types.contains_key(name) => {
                let mut m = BTreeMap::new();
                for _ in 0..if deep { 0 } else { rng.below(6) } {
                    m.insert(self.generate(&arg(0), rng, opts, depth + 1)?, self.generate(&arg(1), rng, opts, depth + 1)?);
                }
                let pairs: Vec<Value> = m.into_iter().map(|(k, v)| Value::Tuple(Rc::new(vec![k, v]))).collect();
                crate::stdflat::to_flat(&pairs).ok()?
            }
            "MdSpan" if !self.types.contains_key(name) => {
                let shape: Vec<i64> = (0..rng.below(4)).map(|_| rng.below(4) as i64).collect();
                let n: i64 = shape.iter().product();
                let data = Value::list((0..n).map(|_| self.generate(&arg(0), rng, opts, depth + 1)).collect::<Option<_>>()?);
                let v = crate::stdflat::mdspan(&data, &Value::list(shape.iter().map(|e| Value::Int(*e)).collect())).ok()?;
                if rng.chance(30) { crate::stdflat::md_method("transpose", match &v { Value::Record(_, fs) => fs, _ => return None }, &[]).ok()? } else { v }
            }
            "View" if !self.types.contains_key(name) => {
                let len = if deep { 0 } else { rng.below(7) };
                crate::stdview::of_list(Rc::new((0..len).map(|_| self.generate(&arg(0), rng, opts, depth + 1)).collect::<Option<_>>()?))
            }
            "Zone" if !self.types.contains_key(name) => {
                const NAMES: &[&str] = &["America/New_York", "Europe/London", "Europe/Dublin", "Australia/Sydney", "Australia/Lord_Howe", "Asia/Kolkata", "America/St_Johns", "Pacific/Chatham", "Africa/Casablanca", "America/Sao_Paulo", "Antarctica/Troll", "Asia/Tehran", "America/Godthab"];
                let k = rng.below(NAMES.len() as u64 + 3) as usize;
                let z = match NAMES.get(k) {
                    Some(n) => sspur_native::tz::load(n).ok()?,
                    None => {
                        let m = rng.range(-1439, 1439);
                        sspur_native::tz::fixed(&sspur_native::tz::offset_name(m * 60), m * 60)
                    }
                };
                crate::stdtz::zone_value(&z)
            }
            "Complex" if !self.types.contains_key(name) => {
                let f_ty = Ty::Named { name: "F64".into(), args: vec![], span: Span::default() };
                let (re, im) = (self.generate(&f_ty, rng, opts, depth)?, self.generate(&f_ty, rng, opts, depth)?);
                Value::Record("#Complex".into(), Rc::new(vec![("re".into(), re), ("im".into(), im)]))
            }
            "Locale" if !self.types.contains_key(name) => {
                let tag = sspur_native::locale::LOCALES[rng.below(6) as usize].tag;
                Value::Record("#Locale".into(), Rc::new(vec![("tag".into(), Value::str(tag))]))
            }
            "Ratio" if !self.types.contains_key(name) => {
                let int_ty = Ty::Named { name: "Int".into(), args: vec![], span: Span::default() };
                let (Value::Int(n), Value::Int(d)) = (self.generate(&int_ty, rng, opts, depth)?, self.generate(&int_ty, rng, opts, depth)?) else { return None };
                let d = if d == 0 { 1 } else { d };
                let (n, d) = sspur_native::bigint::ratio_norm(&sspur_native::bigint::Big::from_i64(n), &sspur_native::bigint::Big::from_i64(d)).ok()?;
                crate::stdratio::value(n, d)
            }
            "Rng" if !self.types.contains_key(name) => {
                let int_ty = Ty::Named { name: "Int".into(), args: vec![], span: Span::default() };
                let (Value::Int(a), Value::Int(b)) = (self.generate(&int_ty, rng, opts, depth)?, self.generate(&int_ty, rng, opts, depth)?) else { return None };
                crate::stdrng::rng_value(sspur_native::rnd::Rng { s: a as u64, g: b as u64 | 1 })
            }
            "StrBuf" if !self.types.contains_key(name) => return self.generate(&Ty::Named { name: "Str".into(), args: vec![], span: Span::default() }, rng, opts, depth),
            _ => return self.gen_user(name, args, rng, opts, depth),
        })
    }

    fn gen_user(&self, name: &str, targs: &[Ty], rng: &mut Rng, opts: &Options, depth: u32) -> Option<Value> {
        let td = self.types.get(name)?;
        let subst = |t: &Ty| subst_ty(t, &td.params.iter().map(|p| p.name.clone()).zip(targs.iter().cloned()).collect::<Vec<_>>());
        match &td.body {
            TypeBody::Alias(base, refine) => {
                for _ in 0..30 {
                    let v = self.generate(&subst(base), rng, opts, depth)?;
                    if refine.as_ref().is_none_or(|r| self.check_refine(r, &v, "").is_ok()) {
                        return Some(v);
                    }
                }
                None
            }
            TypeBody::New(inner) => Some(Value::New(name.into(), Rc::new(self.generate(&subst(inner), rng, opts, depth)?))),
            TypeBody::Record(fs) => {
                for _ in 0..30 {
                    let vals = self.gen_fields(fs, &subst, rng, opts, depth)?;
                    if let Ok(f) = self.build_record(name, vals) {
                        return Some(Value::Record(name.into(), f));
                    }
                }
                None
            }
            TypeBody::Sum(vs) => {
                if depth > MAX_DEPTH + 2 {
                    return None;
                }
                let pool: Vec<&Variant> = if depth >= MAX_DEPTH {
                    vs.iter().filter(|v| v.fields.as_ref().is_none_or(|fs| !fs.iter().any(|f| mentions(&f.ty, name)))).collect()
                } else {
                    vs.iter().collect()
                };
                if pool.is_empty() {
                    return None;
                }
                for _ in 0..30 {
                    let v = pool[rng.below(pool.len() as u64) as usize];
                    let Some(fs) = &v.fields else { return Some(Value::Variant(v.name.as_str().into(), None)) };
                    let vals = self.gen_fields(fs, &subst, rng, opts, depth)?;
                    if let Ok(f) = self.build_record(&v.name, vals) {
                        return Some(Value::Variant(v.name.as_str().into(), Some(f)));
                    }
                }
                None
            }
        }
    }

    fn gen_fields(&self, fs: &[Field], subst: &dyn Fn(&Ty) -> Ty, rng: &mut Rng, opts: &Options, depth: u32) -> Option<Vec<(Rc<str>, Value)>> {
        fs.iter().map(|f| Some((Rc::<str>::from(f.name.as_str()), self.generate(&subst(&f.ty), rng, opts, depth + 1)?))).collect()
    }
}

pub struct DiffReport {
    pub name: String,
    pub cases: usize,
    pub native_cases: usize,
    pub mismatch: Option<(Vec<String>, String, String)>,
}

fn describe_result(r: crate::R) -> String {
    match r {
        Ok(v) => format!("ok {}", crate::value::Quoted(&v)),
        Err(Ctrl::Raise(v)) => format!("raise {}", crate::value::Quoted(&v)),
        Err(Ctrl::Trap(m)) => format!("trap {m}"),
        Err(Ctrl::Return(v)) => format!("ok {}", crate::value::Quoted(&v)),
        Err(c) => format!("trap {}", crate::describe(c)),
    }
}

enum Outcome {
    Pass,
    Discard,
    Fail(String),
}

fn mentions(t: &Ty, name: &str) -> bool {
    match t {
        Ty::Named { name: n, args, .. } => n == name || args.iter().any(|a| mentions(a, name)),
        Ty::Tuple(xs) => xs.iter().any(|x| mentions(x, name)),
        Ty::Fn { .. } => false,
    }
}

fn has_fn_type(t: &Ty) -> bool {
    match t {
        Ty::Fn { .. } => true,
        Ty::Named { args, .. } => args.iter().any(has_fn_type),
        Ty::Tuple(xs) => xs.iter().any(has_fn_type),
    }
}

fn subst_ty(t: &Ty, map: &[(String, Ty)]) -> Ty {
    match t {
        Ty::Named { name, args, span } => match map.iter().find(|(p, _)| p == name) {
            Some((_, r)) if args.is_empty() => r.clone(),
            _ => Ty::Named { name: name.clone(), args: args.iter().map(|a| subst_ty(a, map)).collect(), span: *span },
        },
        Ty::Tuple(xs) => Ty::Tuple(xs.iter().map(|x| subst_ty(x, map)).collect()),
        Ty::Fn { params, ret, effects } => Ty::Fn { params: params.iter().map(|x| subst_ty(x, map)).collect(), ret: Box::new(subst_ty(ret, map)), effects: effects.clone() },
    }
}

fn smaller(v: &Value) -> Vec<Value> {
    match v {
        Value::Int(n) if *n != 0 => {
            let mut out = vec![Value::Int(0), Value::Int(n / 2)];
            out.push(Value::Int(n - n.signum()));
            out
        }
        Value::Float(x) if *x != 0.0 => vec![Value::Float(0.0), Value::Float((x / 2.0).trunc())],
        Value::Str(s) if !s.is_empty() => {
            let half: String = s.chars().take(s.chars().count() / 2).collect();
            vec![Value::str(""), Value::str(&half)]
        }
        Value::List(xs) if !xs.is_empty() => {
            let mut out = vec![Value::list(vec![]), Value::list(xs[..xs.len() / 2].to_vec())];
            for i in 0..xs.len().min(6) {
                let mut ys = (**xs).clone();
                ys.remove(i);
                out.push(Value::list(ys));
            }
            for i in 0..xs.len().min(6) {
                for s in smaller(&xs[i]) {
                    let mut ys = (**xs).clone();
                    ys[i] = s;
                    out.push(Value::list(ys));
                }
            }
            out
        }
        Value::Opt(Some(x)) => {
            let mut out = vec![Value::Opt(None)];
            out.extend(smaller(x).into_iter().map(Value::some));
            out
        }
        Value::Tuple(xs) => {
            let mut out = Vec::new();
            for i in 0..xs.len() {
                for s in smaller(&xs[i]) {
                    let mut ys = (**xs).clone();
                    ys[i] = s;
                    out.push(Value::Tuple(Rc::new(ys)));
                }
            }
            out
        }
        Value::Record(n, fs) => shrink_fields(fs).into_iter().map(|f| Value::Record(n.clone(), f)).collect(),
        Value::Variant(n, Some(fs)) => shrink_fields(fs).into_iter().map(|f| Value::Variant(n.clone(), Some(f))).collect(),
        _ => vec![],
    }
}

fn shrink_fields(fs: &Fields) -> Vec<Fields> {
    let mut out = Vec::new();
    for i in 0..fs.len() {
        for s in smaller(&fs[i].1) {
            let mut ys = (**fs).clone();
            ys[i].1 = s;
            out.push(Rc::new(ys));
        }
    }
    out
}
