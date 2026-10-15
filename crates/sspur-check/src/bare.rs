use crate::{expr_key, Diag, ExprKey, RecordTable, SumTable, Type};
use sspur_syntax::*;
use std::collections::{HashMap, HashSet};

pub struct Tables<'a> {
    pub exprs: &'a HashMap<ExprKey, Type>,
    pub fns: &'a HashMap<String, (Vec<Type>, Type)>,
    pub records: &'a RecordTable,
    pub sums: &'a SumTable,
    pub newtypes: &'a HashMap<String, Type>,
}

const STR_METHODS: &[&str] = &["len", "byte_len", "byte", "is_empty"];
const F64_METHODS: &[&str] = &["abs", "sqrt", "floor", "ceil", "round", "trunc", "is_nan", "is_finite", "is_inf", "copysign"];
const F64_HINT: &str = "bare F64 has + - * /, comparisons, abs sqrt floor ceil round trunc is_nan is_finite is_inf copysign, Int.to_f64, and pi() inf() nan()";

struct Cx<'a> {
    t: &'a Tables<'a>,
    diags: Vec<Diag>,
    def: Option<String>,
    seen: HashSet<(Option<String>, String)>,
}

pub fn check(m: &Module, t: &Tables) -> Vec<Diag> {
    let mut cx = Cx { t, diags: vec![], def: None, seen: HashSet::new() };
    let mut vectors: HashMap<String, String> = HashMap::new();
    for d in &m.defs {
        cx.def = Some(d.name().to_string());
        match d {
            Def::Fn(f) => cx.fn_def(f, &mut vectors),
            Def::Store(s) => cx.err("E_PROFILE_BARE", s.span, "stores need a deploy host, which bare code doesn't have".into(), None),
            Def::Svc(s) => cx.err("E_PROFILE_BARE", s.span, "services need a deploy host, which bare code doesn't have".into(), None),
            Def::Effect(e) => cx.err("E_PROFILE_BARE", e.span, "effect handlers need the effect runtime, which bare code doesn't have".into(), None),
            Def::Type(_) | Def::Test(_) | Def::Static(_) | Def::Use(_) | Def::Trait(_) | Def::Impl(_) => {}
        }
    }
    cx.diags
}

fn effect_why(name: &str) -> &'static str {
    match name {
        "log" => "there is no host log in bare code; write to a UART with mmio",
        "fail" => "raise and fail[E] need the error runtime; return a status value instead",
        "conc" => "tasks, atomics and channels need the thread runtime",
        "ffi" => "extern C calls need a C library, which bare code doesn't have",
        n if n.starts_with("db.") => "db effects need a deploy host",
        _ => "effect handlers and generators need the effect runtime",
    }
}

impl Cx<'_> {
    fn err(&mut self, code: &str, span: Span, msg: String, hint: Option<&str>) {
        if self.seen.insert((self.def.clone(), msg.clone())) {
            self.diags.push(Diag { code: code.into(), severity: "error", def: self.def.clone(), span: [span.start, span.end], msg, hint: hint.map(str::to_string), fix: vec![] });
        }
    }

    fn type_why(&self, t: &Type, depth: u32) -> Option<String> {
        if depth > 8 {
            return None;
        }
        match t {
            Type::Con(n, a) => match n.as_str() {
                "Int" | "Bool" | "Unit" | "Str" | "I8" | "I16" | "I32" | "U8" | "U16" | "U32" | "U64" | "Mmio" | "Ptr" => None,
                "F32" => Some("F32 is a GPU kernel type; bare code uses F64".into()),
                "List" => Some("lists need the GC heap; bare code has none".into()),
                "Map" => Some("maps need the GC heap; bare code has none".into()),
                "Res" | "Guess" => Some(format!("{n} values need the app runtime")),
                "Atomic" | "Chan" => Some(format!("{n} needs the thread runtime")),
                "#DevBuf" => Some("device buffers need the GPU runtime".into()),
                "#Time" | "#Duration" => Some(format!("{} values need the app runtime", &n[1..])),
                "#Set" | "#Heap" | "#StrBuf" | "#Bits" | "#Bytes" | "#HashMap" | "#HashSet" | "#BigInt" | "#Dec" | "#Regex" | "#View" => Some(format!("{} values need the GC heap; bare code has none", &n[1..])),
                "Array" => self.type_why(&a[0], depth + 1),
                "Opt" | "Secret" | "Pii" | "Untrusted" => a.iter().find_map(|x| self.type_why(x, depth + 1)),
                _ if self.t.newtypes.contains_key(n) => self.type_why(&self.t.newtypes[n], depth + 1),
                _ if self.t.records.contains_key(n) => self.t.records[n].1.iter().map(|(_, ft)| ft).chain(a).find_map(|x| self.type_why(x, depth + 1)),
                _ if self.t.sums.contains_key(n) => Some(format!("sum type {n} is heap-allocated; use a record with an Int tag")),
                _ => None,
            },
            Type::Tuple(xs) => xs.iter().find_map(|x| self.type_why(x, depth + 1)),
            Type::Fn(..) => Some("function values need closures on the GC heap".into()),
            Type::Var(_) | Type::Param(_) => None,
        }
    }

    fn check_type(&mut self, t: &Type, span: Span) {
        if let Some(why) = self.type_why(t, 0) {
            self.err("E_PROFILE_BARE", span, format!("uses {t}: {why}"), None);
        }
    }

    fn fn_def(&mut self, f: &FnDef, vectors: &mut HashMap<String, String>) {
        if f.ext.is_some() {
            self.err("E_PROFILE_BARE", f.sig_span, "extern C functions need a C library, which bare code doesn't have".into(), None);
            return;
        }
        for e in &f.effects {
            if !matches!(e.name.as_str(), "div" | "unsafe" | "mmio" | "static") && !f.tparams.iter().any(|p| p.name == e.name) {
                self.err("E_PROFILE_BARE", e.span, format!("effect '{}' is not available in bare code: {}", printer::effect(e), effect_why(&e.name)), Some("bare code may perform div, unsafe, mmio and static"));
            }
        }
        let sig = self.t.fns.get(&f.name).cloned();
        if let Some((ps, r)) = &sig {
            for p in ps.iter().chain([r]) {
                self.check_type(p, f.sig_span);
            }
        }
        if let Some(v) = &f.interrupt {
            let ok = v == "timer" || v.parse::<u32>().is_ok_and(|n| n < 1024);
            if !ok {
                self.err("E_INTERRUPT_VEC", f.sig_span, format!("unknown interrupt '{v}'"), Some("use 'interrupt timer' or the target's interrupt number (riscv64 mcause code, aarch64 GIC INTID, Cortex-M IRQ number)"));
            }
            if sig.as_ref().is_some_and(|(ps, r)| !ps.is_empty() || *r != Type::unit()) {
                self.err("E_INTERRUPT_SIG", f.sig_span, format!("interrupt handler {} must take no parameters and return Unit", f.name), None);
            }
            if let Some(prev) = vectors.insert(v.clone(), f.name.clone()) {
                self.err("E_INTERRUPT_DUP", f.sig_span, format!("interrupt '{v}' is already handled by {prev}"), None);
            }
        }
        if f.name == "core_main" && sig.as_ref().is_some_and(|(ps, r)| ps.as_slice() != [Type::int()] || *r != Type::unit()) {
            self.err("E_INTERRUPT_SIG", f.sig_span, "the secondary core entry must be 'fn core_main(id: Int)'".into(), None);
        }
        if f.name == "on_trap" && sig.as_ref().is_some_and(|(ps, r)| ps.as_slice() != [Type::int()] || *r != Type::unit()) {
            self.err("E_INTERRUPT_SIG", f.sig_span, "the trap handler must be 'fn on_trap(code: Int)'".into(), None);
        }
        for e in f.pres.iter().chain(&f.posts).chain([&f.body]) {
            self.expr(e);
        }
    }

    fn expr(&mut self, e: &Expr) {
        let ty = self.t.exprs.get(&expr_key(e)).cloned();
        let msg = |s: &str| s.to_string();
        match &e.kind {
            ExprKind::Lambda { .. } | ExprKind::Placeholder => self.err("E_PROFILE_BARE", e.span, msg("lambdas need closures on the GC heap; use a named function"), None),
            ExprKind::List(_) => self.err("E_PROFILE_BARE", e.span, msg("lists need the GC heap; bare code has none"), None),
            ExprKind::Par(_) => self.err("E_PROFILE_BARE", e.span, msg("par needs the thread runtime"), None),
            ExprKind::Handle(..) => self.err("E_PROFILE_BARE", e.span, msg("handle needs the effect runtime"), None),
            ExprKind::Catch(..) | ExprKind::Raise(_) => self.err("E_PROFILE_BARE", e.span, msg("raise and catch need the error runtime; return a status value instead"), None),
            ExprKind::Str(parts) if parts.iter().any(|p| matches!(p, StrPart::Expr(_))) => {
                self.err("E_PROFILE_BARE", e.span, msg("string interpolation allocates; write the pieces separately"), None)
            }
            ExprKind::Binary(BinOp::Add, a, _) if self.t.exprs.get(&expr_key(a)).is_some_and(|t| matches!(t, Type::Con(n, _) if n == "Str")) => {
                self.err("E_PROFILE_BARE", e.span, msg("string concatenation allocates"), None)
            }
            ExprKind::Method { recv, name, .. } | ExprKind::Field(recv, name) if self.t.exprs.get(&expr_key(recv)).is_some_and(|t| matches!(t, Type::Con(n, _) if n == "Str")) && !STR_METHODS.contains(&name.as_str()) && !self.t.fns.contains_key(name) => {
                self.err("E_PROFILE_BARE", e.span, format!("'.{name}' on Str is not available in bare code"), Some("bare strings are static: use len, byte_len, byte(i) and is_empty"));
            }
            ExprKind::Method { name, .. } | ExprKind::Field(_, name) if name == "str" && !self.t.fns.contains_key("str") => self.err("E_PROFILE_BARE", e.span, msg("'.str' allocates a string"), None),
            ExprKind::Method { recv, name, .. } | ExprKind::Field(recv, name) if self.t.exprs.get(&expr_key(recv)).is_some_and(|t| *t == Type::con("F64")) && !F64_METHODS.contains(&name.as_str()) && !self.t.fns.contains_key(name) => {
                self.err("E_PROFILE_BARE", e.span, format!("'.{name}' on F64 needs libm, which bare code doesn't have"), Some(F64_HINT));
            }
            ExprKind::Binary(op @ (BinOp::Rem | BinOp::Pow), a, _) if self.t.exprs.get(&expr_key(a)).is_some_and(|t| *t == Type::con("F64")) => {
                self.err("E_PROFILE_BARE", e.span, format!("'{}' on F64 needs libm, which bare code doesn't have", op.symbol()), Some(F64_HINT));
            }
            ExprKind::Call(f, _) => {
                if let ExprKind::Name(n) = &f.kind
                    && !self.t.fns.contains_key(n) {
                        let why = match n.as_str() {
                            "alloc" | "free" => Some("there is no heap in bare code; use static or caller-provided memory"),
                            "log" => Some("there is no host log in bare code; write to a UART with mmio"),
                            "atomic" | "chan" => Some("atomics and channels need the thread runtime"),
                            "empty_map" => Some("maps need the GC heap"),
                            _ => None,
                        };
                        if let Some(w) = why {
                            self.err("E_PROFILE_BARE", e.span, format!("'{n}' is not available: {w}"), None);
                        }
                    }
            }
            ExprKind::Block(stmts) => {
                for s in stmts {
                    if let Stmt::Fn(f) = s {
                        self.err("E_PROFILE_BARE", f.sig_span, format!("local function {} needs a closure; define it at the top level", f.name), None);
                    }
                }
            }
            _ => {}
        }
        if !matches!(e.kind, ExprKind::Range(..))
            && let Some(t) = ty {
                self.check_type(&t, e.span);
            }
        let skip_callee = matches!(&e.kind, ExprKind::Call(f, _) if matches!(f.kind, ExprKind::Name(_)));
        for (i, c) in visit::children(e).into_iter().enumerate() {
            if skip_callee && i == 0 {
                continue;
            }
            self.expr(c);
        }
    }
}
