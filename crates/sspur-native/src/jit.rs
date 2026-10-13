use cranelift_codegen::ir::{condcodes::IntCC, types, AbiParam, InstBuilder, MemFlagsData, Value};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module as _};
use sspur_syntax::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use crate::nval::{scalar_display, Layouts, NVal};
use crate::LOG_HOOK;
use sspur_check::Type;

const MAX_ARITY: usize = 6;
const DEFAULT_MAX_DEPTH: i64 = 10_000;

pub(crate) const T_OVERFLOW: i64 = 1;
pub(crate) const T_DIV_ZERO: i64 = 2;
pub(crate) const T_PRE: i64 = 3;
pub(crate) const T_POST: i64 = 4;
const T_PARAM: i64 = 5;
const T_NEG_EXP: i64 = 6;
pub(crate) const T_DEPTH: i64 = 7;
pub(crate) const T_INDEX: i64 = 8;
pub(crate) const T_UNWRAP: i64 = 9;
pub(crate) const T_NOMATCH: i64 = 10;
pub(crate) const T_REFINE: i64 = 11;
pub(crate) const T_MSG: i64 = 12;
pub(crate) const T_REPEAT: i64 = 13;
pub(crate) const T_RAISE: i64 = 100;
pub(crate) const T_GUESS: i64 = 14;
pub(crate) const T_OOM: i64 = 15;
pub(crate) const T_ALLOC: i64 = 18;
pub(crate) const T_BYTE: i64 = 19;
const T_DEADLOCK: i64 = 16;
const T_CLOSED: i64 = 17;

#[repr(C)]
struct Status {
    code: i64,
    func: i64,
    clause: i64,
    value: i64,
    limit: i64,
    rbuf: *mut i64,
    rlen: i64,
    err: *mut u8,
    err_type: i64,
}

impl Default for Status {
    fn default() -> Self {
        Status { code: 0, func: 0, clause: 0, value: 0, limit: 0, rbuf: std::ptr::null_mut(), rlen: 0, err: std::ptr::null_mut(), err_type: 0 }
    }
}

#[repr(C)]
pub(crate) struct HostApi {
    fmt_f64: extern "C" fn(f64, *mut u8) -> i64,
    fmt_f64_display: extern "C" fn(f64, *mut u8) -> i64,
    str_op: extern "C" fn(i64, *const u8, i64, *mut u8, i64) -> i64,
    str_class: extern "C" fn(i64, *const u8, i64) -> i64,
    str_spans: extern "C" fn(i64, *const u8, i64, *mut i64, i64) -> i64,
    log: extern "C" fn(*const u8, i64),
}

unsafe fn host_str<'a>(p: *const u8, len: i64) -> &'a str {
    if len == 0 {
        return "";
    }
    unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(p, len as usize)) }
}

unsafe fn host_write(s: &str, out: *mut u8, cap: i64) -> i64 {
    let n = s.len().min(cap.max(0) as usize);
    unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), out, n) };
    n as i64
}

extern "C" fn host_fmt_f64(x: f64, out: *mut u8) -> i64 {
    unsafe { host_write(&format!("{x:?}"), out, 64) }
}

extern "C" fn host_fmt_f64_display(x: f64, out: *mut u8) -> i64 {
    unsafe { host_write(&format!("{x}"), out, 400) }
}

extern "C" fn host_str_op(op: i64, p: *const u8, len: i64, out: *mut u8, cap: i64) -> i64 {
    let s = unsafe { host_str(p, len) };
    let r = match op {
        1 => s.to_lowercase(),
        2 => s.to_uppercase(),
        4 => s.to_uppercase().to_lowercase(),
        _ => format!("{s:?}"),
    };
    unsafe { host_write(&r, out, cap) }
}

extern "C" fn host_str_class(_op: i64, p: *const u8, len: i64) -> i64 {
    let s = unsafe { host_str(p, len) };
    i64::from(!s.is_empty() && s.chars().all(char::is_alphabetic))
}

extern "C" fn host_str_spans(op: i64, p: *const u8, len: i64, out: *mut i64, cap: i64) -> i64 {
    let s = unsafe { host_str(p, len) };
    let base = s.as_ptr() as usize;
    let mut spans: Vec<i64> = Vec::new();
    if op == 2 {
        let t = s.trim();
        spans.push((t.as_ptr() as usize - base) as i64);
        spans.push(t.len() as i64);
    } else {
        for w in s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
            spans.push((w.as_ptr() as usize - base) as i64);
            spans.push(w.len() as i64);
        }
    }
    let n = spans.len().min(cap.max(0) as usize);
    unsafe { std::ptr::copy_nonoverlapping(spans.as_ptr(), out, n) };
    n as i64
}

extern "C" fn host_log(p: *const u8, len: i64) {
    let s = unsafe { host_str(p, len) };
    match LOG_HOOK.with(|h| h.get()) {
        Some((f, ctx)) => f(ctx, s),
        None => {
            use std::io::Write;
            let _ = writeln!(std::io::stdout(), "{s}");
        }
    }
}

pub(crate) static HOST: HostApi = HostApi { fmt_f64: host_fmt_f64, fmt_f64_display: host_fmt_f64_display, str_op: host_str_op, str_class: host_str_class, str_spans: host_str_spans, log: host_log };

pub enum NativeError {
    Trap(String),
    Raise(NVal),
}

pub(crate) struct RichFn {
    pub(crate) ptr: *const u8,
    pub(crate) params: Vec<Type>,
    pub(crate) ret: Type,
}

thread_local! {
    /// Parameter values a Cranelift function recorded before a `pre` trap, shown in the message.
    static PRE_NOTES: std::cell::RefCell<Vec<i64>> = const { std::cell::RefCell::new(Vec::new()) };
}

extern "C" fn sspur_note(v: i64) {
    PRE_NOTES.with(|n| n.borrow_mut().push(v));
}

extern "C" fn sspur_pow(base: i64, exp: i64, st: *mut Status) -> i64 {
    let st = unsafe { &mut *st };
    if exp < 0 {
        st.code = T_NEG_EXP;
        return 0;
    }
    match u32::try_from(exp).ok().and_then(|e| base.checked_pow(e)) {
        Some(v) => v,
        None => {
            st.code = T_OVERFLOW;
            0
        }
    }
}

pub struct Compiled {
    max_depth: std::cell::Cell<i64>,
    pub functions: Vec<String>,
    pub skipped: BTreeMap<String, String>,
    pub fallback: Option<String>,
    /// The shared library the functions were loaded from, for a release build.
    pub library: Option<std::path::PathBuf>,
    inner: Rc<Inner>,
}

/// How `run_main` enters a native `main` with no parameters: `Some(true)` through the scalar
/// entry, `Some(false)` through the encoded entry (only when it returns `Unit`).
pub type MainEntry = Option<bool>;

/// A run of a cached `main` that trapped or raised; `Compiled::main_failure` describes it the way
/// the interpreter's call would have.
pub struct MainFailure {
    st: Status,
    code: i64,
    scalar: bool,
}

/// Runs the `main` of a library built by an earlier `compile_release` without loading the
/// program: installs the host functions and arguments, then calls `main` through the entry that
/// `Compiled::main_entry` chose. `Err` means nothing ran (the library or a symbol is missing).
pub fn run_cached_main(lib: &std::path::Path, scalar: bool, max_depth: i64) -> Result<Result<(), MainFailure>, String> {
    let library = unsafe { libloading::Library::new(lib) }.map_err(|e| e.to_string())?;
    let sym: &[u8] = if scalar { b"sspur_entry_main" } else { b"sspur_wentry_main" };
    let entry = *unsafe { library.get::<unsafe extern "C" fn()>(sym) }.map_err(|e| e.to_string())? as *const u8;
    let free = *unsafe { library.get::<unsafe extern "C" fn(*mut i64)>(b"sspur_buf_free") }.map_err(|e| e.to_string())?;
    let set_host = unsafe { library.get::<unsafe extern "C" fn(*const HostApi)>(b"sspur_set_host") }.map_err(|e| e.to_string())?;
    unsafe { set_host(&HOST) };
    if let Ok(set_args) = unsafe { library.get::<unsafe extern "C" fn(*const [usize; 2], i64)>(b"sspur_set_args") } {
        let args = crate::program_args();
        let views: Vec<[usize; 2]> = args.iter().map(|a| [a.len(), a.as_ptr() as usize]).collect();
        unsafe { set_args(views.as_ptr(), views.len() as i64) };
    }
    std::mem::forget(library);
    let mut st = Status { limit: max_depth.max(1), ..Status::default() };
    if scalar {
        PRE_NOTES.with(|n| n.borrow_mut().clear());
        unsafe { std::mem::transmute::<*const u8, extern "C" fn(*mut Status) -> i64>(entry)(&mut st) };
        let code = st.code;
        return Ok(if code == 0 { Ok(()) } else { Err(MainFailure { st, code, scalar }) });
    }
    let words = [0i64];
    let mut out: *mut i64 = std::ptr::null_mut();
    let mut out_len: i64 = 0;
    let code = unsafe { std::mem::transmute::<*const u8, extern "C" fn(*const i64, *mut Status, *mut *mut i64, *mut i64) -> i64>(entry)(words.as_ptr(), &mut st, &mut out, &mut out_len) };
    if code != 0 {
        return Ok(Err(MainFailure { st, code, scalar }));
    }
    unsafe { free(out) };
    Ok(Ok(()))
}

struct Inner {
    _keep: Box<dyn std::any::Any>,
    ptrs: HashMap<String, (*const u8, usize)>,
    defs: Vec<FnDef>,
    index: HashMap<String, usize>,
    rich: HashMap<String, RichFn>,
    layouts: Layouts,
    refines: HashMap<i64, (String, String, Type)>,
    free: Option<unsafe extern "C" fn(*mut i64)>,
    err_types: HashMap<i64, Type>,
    fids: HashMap<i64, usize>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Int,
    Bool,
    Unit,
}

fn kind_of(t: Option<&Ty>) -> Option<Kind> {
    match t {
        None => Some(Kind::Unit),
        Some(Ty::Named { name, args, .. }) if args.is_empty() => match name.as_str() {
            "Int" => Some(Kind::Int),
            "Bool" => Some(Kind::Bool),
            "Unit" => Some(Kind::Unit),
            _ => None,
        },
        _ => None,
    }
}

impl Compiled {
    pub fn set_max_depth(&self, depth: i64) {
        self.max_depth.set(depth.max(1));
    }

    pub fn call(&self, name: &str, args: &[i64]) -> Option<Result<i64, String>> {
        let (ptr, arity) = *self.inner.ptrs.get(name)?;
        if arity != args.len() {
            return None;
        }
        let mut st = Status { limit: self.max_depth.get(), ..Status::default() };
        let s = &mut st as *mut Status;
        PRE_NOTES.with(|n| n.borrow_mut().clear());
        let r = unsafe {
            match arity {
                0 => std::mem::transmute::<*const u8, extern "C" fn(*mut Status) -> i64>(ptr)(s),
                1 => std::mem::transmute::<*const u8, extern "C" fn(i64, *mut Status) -> i64>(ptr)(args[0], s),
                2 => std::mem::transmute::<*const u8, extern "C" fn(i64, i64, *mut Status) -> i64>(ptr)(args[0], args[1], s),
                3 => std::mem::transmute::<*const u8, extern "C" fn(i64, i64, i64, *mut Status) -> i64>(ptr)(args[0], args[1], args[2], s),
                4 => std::mem::transmute::<*const u8, extern "C" fn(i64, i64, i64, i64, *mut Status) -> i64>(ptr)(args[0], args[1], args[2], args[3], s),
                5 => std::mem::transmute::<*const u8, extern "C" fn(i64, i64, i64, i64, i64, *mut Status) -> i64>(ptr)(args[0], args[1], args[2], args[3], args[4], s),
                6 => std::mem::transmute::<*const u8, extern "C" fn(i64, i64, i64, i64, i64, i64, *mut Status) -> i64>(ptr)(args[0], args[1], args[2], args[3], args[4], args[5], s),
                _ => return None,
            }
        };
        Some(if st.code == 0 { Ok(r) } else { Err(self.message(&st)) })
    }

    /// See `MainEntry`.
    pub fn main_entry(&self) -> MainEntry {
        if self.inner.ptrs.get("main").is_some_and(|p| p.1 == 0) {
            return Some(true);
        }
        let f = self.inner.rich.get("main")?;
        (f.params.is_empty() && matches!(&f.ret, Type::Con(n, a) if n == "Unit" && a.is_empty())).then_some(false)
    }

    /// The result `call` or `call_rich` would have given for the failed run `f` of this program's
    /// `main`; `None` where `call_rich` gives `None`.
    pub fn main_failure(&self, f: MainFailure) -> Option<NativeError> {
        let st = &f.st;
        if f.scalar {
            return Some(NativeError::Trap(self.message(st)));
        }
        if f.code == T_RAISE {
            let t = self.inner.err_types.get(&st.err_type)?;
            let slice = unsafe { std::slice::from_raw_parts(st.rbuf, st.rlen as usize) };
            let mut pos = 0;
            let v = self.inner.layouts.decode(slice, &mut pos, t);
            if let Some(free) = self.inner.free {
                unsafe { free(st.rbuf) };
            }
            return Some(NativeError::Raise(v?));
        }
        Some(NativeError::Trap(self.message(st)))
    }

    pub fn has(&self, name: &str) -> bool {
        self.inner.ptrs.contains_key(name) || self.inner.rich.contains_key(name)
    }

    pub fn call_rich(&self, name: &str, args: &[NVal]) -> Option<Result<NVal, NativeError>> {
        let f = self.inner.rich.get(name)?;
        if f.params.len() != args.len() {
            return None;
        }
        let mut words = Vec::new();
        for (a, t) in args.iter().zip(&f.params) {
            self.inner.layouts.encode(a, t, &mut words)?;
        }
        words.push(0);
        let mut st = Status { limit: self.max_depth.get(), ..Status::default() };
        let mut out: *mut i64 = std::ptr::null_mut();
        let mut out_len: i64 = 0;
        let entry = unsafe { std::mem::transmute::<*const u8, extern "C" fn(*const i64, *mut Status, *mut *mut i64, *mut i64) -> i64>(f.ptr) };
        let code = entry(words.as_ptr(), &mut st, &mut out, &mut out_len);
        if code == T_RAISE {
            let t = self.inner.err_types.get(&st.err_type)?;
            let slice = unsafe { std::slice::from_raw_parts(st.rbuf, st.rlen as usize) };
            let mut pos = 0;
            let v = self.inner.layouts.decode(slice, &mut pos, t);
            if let Some(free) = self.inner.free {
                unsafe { free(st.rbuf) };
            }
            return Some(Err(NativeError::Raise(v?)));
        }
        if code != 0 {
            return Some(Err(NativeError::Trap(self.message(&st))));
        }
        let slice = unsafe { std::slice::from_raw_parts(out, out_len as usize) };
        let mut pos = 0;
        let v = self.inner.layouts.decode(slice, &mut pos, &f.ret);
        if let Some(free) = self.inner.free {
            unsafe { free(out) };
        }
        Some(v.ok_or_else(|| NativeError::Trap("native result could not be decoded".to_string())))
    }

    fn decode_rbuf(&self, st: &Status, t: &Type) -> Option<String> {
        if st.rbuf.is_null() {
            return None;
        }
        let slice = unsafe { std::slice::from_raw_parts(st.rbuf, st.rlen as usize) };
        let mut pos = 0;
        let v = self.inner.layouts.decode(slice, &mut pos, t).map(|v| v.to_string());
        if let Some(free) = self.inner.free {
            unsafe { free(st.rbuf) };
        }
        v
    }

    pub fn returns_bool(&self, name: &str) -> bool {
        self.inner.index.get(name).is_some_and(|i| kind_of(self.inner.defs[*i].ret.as_ref()) == Some(Kind::Bool))
    }

    pub fn returns_unit(&self, name: &str) -> bool {
        self.inner.index.get(name).is_some_and(|i| kind_of(self.inner.defs[*i].ret.as_ref()) == Some(Kind::Unit))
    }

    fn show(f: &FnDef, kind: Option<Kind>, v: i64) -> String {
        let _ = f;
        match kind {
            Some(Kind::Bool) => (v != 0).to_string(),
            Some(Kind::Unit) => "()".into(),
            _ => v.to_string(),
        }
    }

    fn message(&self, st: &Status) -> String {
        let f = if self.inner.fids.is_empty() { self.inner.defs.get(st.func as usize) } else { self.inner.fids.get(&st.func).and_then(|i| self.inner.defs.get(*i)) };
        match (st.code, f) {
            (T_OVERFLOW, _) => "integer overflow".into(),
            (T_DIV_ZERO, _) => "division by zero".into(),
            (T_NEG_EXP, _) => "negative exponent".into(),
            (T_DEPTH, Some(f)) => format!("stack overflow in {}", f.name),
            (T_INDEX, _) => format!("index {} out of bounds for list of length {}", st.value, st.clause),
            (T_UNWRAP, _) => "unwrapped none with .get; check with is_some, match on some/none, or use .or(default)".into(),
            (T_NOMATCH, _) => "no match arm".into(),
            (T_REPEAT, _) => "repeat count must be >= 0".into(),
            (T_OOM, _) => "out of memory".into(),
            (T_ALLOC, _) => format!("invalid allocation size {}", st.clause),
            (T_BYTE, _) => format!("byte index {} out of bounds for a string of {} bytes", st.value, st.clause),
            (T_DEADLOCK, _) => "deadlock: every task is blocked on recv".into(),
            (T_CLOSED, _) => "send on a closed channel".into(),
            (T_GUESS, _) => format!("guess confidence {} is outside [0, 1]", f64::from_bits(st.value as u64)),
            (T_MSG, _) if !st.rbuf.is_null() => {
                let bytes = unsafe { std::slice::from_raw_parts(st.rbuf as *const u8, st.rlen as usize) };
                let m = String::from_utf8_lossy(bytes).into_owned();
                if let Some(free) = self.inner.free {
                    unsafe { free(st.rbuf) };
                }
                m
            }
            (T_REFINE, _) => {
                let Some((ctx, expr, t)) = self.inner.refines.get(&st.clause) else { return "contract violated".into() };
                let v = self.decode_rbuf(st, t).unwrap_or_else(|| scalar_display(t, st.value));
                format!("contract violated: {ctx} where {expr} (value = {v})")
            }
            (T_POST, Some(f)) if self.inner.rich.contains_key(&f.name) => {
                let t = &self.inner.rich[&f.name].ret;
                let v = self.decode_rbuf(st, t).unwrap_or_else(|| scalar_display(t, st.value));
                format!("contract violated: post {} in {} (r = {v})", printer::expr(&f.posts[st.clause as usize], 0), f.name)
            }
            (T_PRE, _) if !st.rbuf.is_null() => {
                let bytes = unsafe { std::slice::from_raw_parts(st.rbuf as *const u8, st.rlen as usize) };
                let m = String::from_utf8_lossy(bytes).into_owned();
                if let Some(free) = self.inner.free {
                    unsafe { free(st.rbuf) };
                }
                m
            }
            (T_PRE, Some(f)) => {
                let pre = &f.pres[st.clause as usize];
                let notes = PRE_NOTES.with(|n| std::mem::take(&mut *n.borrow_mut()));
                let idx = sspur_syntax::visit::mentioned_params(&f.params, pre);
                let vals = if notes.len() == idx.len() && !idx.is_empty() {
                    let shown: Vec<String> = idx.iter().zip(&notes).map(|(j, v)| format!("{} = {}", f.params[*j].name, Self::show(f, kind_of(Some(&f.params[*j].ty)), *v))).collect();
                    format!(" ({})", shown.join(", "))
                } else {
                    String::new()
                };
                format!("contract violated: pre {} in {}{vals}", printer::expr(pre, 0), f.name)
            }
            (T_POST, Some(f)) => format!(
                "contract violated: post {} in {} (r = {})",
                printer::expr(&f.posts[st.clause as usize], 0),
                f.name,
                Self::show(f, kind_of(f.ret.as_ref()), st.value)
            ),
            (T_PARAM, Some(f)) => {
                let p = &f.params[st.clause as usize];
                format!(
                    "contract violated: parameter '{}' of {} where {} (value = {})",
                    p.name,
                    f.name,
                    printer::expr(p.refine.as_ref().unwrap(), 0),
                    Self::show(f, kind_of(Some(&p.ty)), st.value)
                )
            }
            (c, _) => format!("native trap {c}"),
        }
    }
}

fn eligible_expr(e: &Expr, fns: &HashSet<String>, locals: &mut Vec<String>) -> Result<(), String> {
    let mut ok = Ok(());
    let mut check = |r: Result<(), String>| {
        if ok.is_ok() {
            ok = r;
        }
    };
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Unit | ExprKind::Placeholder => {}
        ExprKind::Name(n) => {
            if !locals.contains(n) {
                return Err(format!("uses '{n}' as a value"));
            }
        }
        ExprKind::Unary(_, x) | ExprKind::Return(x) => check(eligible_expr(x, fns, locals)),
        ExprKind::Binary(_, a, b) => {
            check(eligible_expr(a, fns, locals));
            check(eligible_expr(b, fns, locals));
        }
        ExprKind::If(c, t, f) => {
            check(eligible_expr(c, fns, locals));
            check(eligible_expr(t, fns, locals));
            if let Some(f) = f {
                check(eligible_expr(f, fns, locals));
            }
        }
        ExprKind::Call(f, args) => match &f.kind {
            ExprKind::Name(n) if fns.contains(n) && !locals.contains(n) => {
                for a in args {
                    check(eligible_expr(a, fns, locals));
                }
            }
            ExprKind::Name(n) => return Err(format!("calls '{n}', which is not native")),
            _ => return Err("calls a function value".into()),
        },
        ExprKind::Block(stmts) => {
            let n = locals.len();
            for s in stmts {
                match s {
                    Stmt::Expr(x) => check(eligible_expr(x, fns, locals)),
                    Stmt::Let(Pat::Bind(name), x) | Stmt::Var(name, x) => {
                        check(eligible_expr(x, fns, locals));
                        locals.push(name.clone());
                    }
                    Stmt::Let(Pat::Wild, x) => check(eligible_expr(x, fns, locals)),
                    Stmt::Assign(name, x, _) => {
                        if !locals.contains(name) {
                            return Err(format!("assigns '{name}'"));
                        }
                        check(eligible_expr(x, fns, locals));
                    }
                    Stmt::While(c, body) => {
                        check(eligible_expr(c, fns, locals));
                        check(eligible_expr(body, fns, locals));
                    }
                    Stmt::For(Pat::Bind(i), it, body) => match &it.kind {
                        ExprKind::Range(a, b) => {
                            check(eligible_expr(a, fns, locals));
                            check(eligible_expr(b, fns, locals));
                            locals.push(i.clone());
                            check(eligible_expr(body, fns, locals));
                            locals.pop();
                        }
                        _ => return Err("for loop over a non-range".into()),
                    },
                    _ => return Err("uses a statement form the native backend does not support".into()),
                }
            }
            locals.truncate(n);
        }
        other => return Err(format!("uses {} expressions", kind_name(other))),
    }
    ok
}

fn kind_name(k: &ExprKind) -> &'static str {
    match k {
        ExprKind::Float(_) => "float",
        ExprKind::Str(_) => "string",
        ExprKind::Match(..) => "match",
        ExprKind::Catch(..) => "catch",
        ExprKind::Handle(..) => "handle",
        ExprKind::Lambda { .. } => "lambda",
        ExprKind::List(_) => "list",
        ExprKind::Record { .. } => "record",
        ExprKind::Method { .. } | ExprKind::Field(..) => "method or field",
        ExprKind::Raise(_) => "raise",
        _ => "unsupported",
    }
}

fn local_check(f: &FnDef, fns: &HashSet<String>) -> Result<(), String> {
    if f.ext.is_some() {
        return Err("is extern".into());
    }
    if !f.tparams.is_empty() {
        return Err("is generic".into());
    }
    if f.params.len() > MAX_ARITY {
        return Err(format!("has more than {MAX_ARITY} parameters"));
    }
    if let Some(e) = f.effects.iter().find(|e| e.name != "div") {
        return Err(format!("performs '{}'", printer::effect(e)));
    }
    if kind_of(f.ret.as_ref()).is_none() {
        return Err("returns a non-Int/Bool type".into());
    }
    for p in &f.params {
        if !matches!(kind_of(Some(&p.ty)), Some(Kind::Int | Kind::Bool)) {
            return Err(format!("parameter '{}' is not Int or Bool", p.name));
        }
    }
    if matches!(f.body.kind, ExprKind::Table(_)) {
        return Err("is a rule table".into());
    }
    let mut locals: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
    for p in &f.params {
        if let Some(r) = &p.refine {
            locals.push("_".into());
            eligible_expr(r, fns, &mut locals)?;
            locals.pop();
        }
    }
    for x in &f.pres {
        eligible_expr(x, fns, &mut locals)?;
    }
    locals.push("r".into());
    for x in &f.posts {
        eligible_expr(x, fns, &mut locals)?;
    }
    locals.pop();
    eligible_expr(&f.body, fns, &mut locals)
}

pub(crate) fn eligible(m: &Module) -> (Vec<FnDef>, HashSet<String>, BTreeMap<String, String>) {
    let defs: Vec<FnDef> = m.defs.iter().filter_map(|d| if let Def::Fn(f) = d { Some(f.clone()) } else { None }).collect();
    let mut skipped = BTreeMap::new();
    let mut ok: HashSet<String> = defs.iter().map(|f| f.name.clone()).collect();
    loop {
        let mut changed = false;
        for f in &defs {
            if ok.contains(&f.name)
                && let Err(why) = local_check(f, &ok) {
                    ok.remove(&f.name);
                    skipped.insert(f.name.clone(), why);
                    changed = true;
                }
        }
        if !changed {
            break;
        }
    }
    (defs, ok, skipped)
}

pub(crate) fn assemble(defs: Vec<FnDef>, ptrs: HashMap<String, (*const u8, usize)>, skipped: BTreeMap<String, String>, keep: Box<dyn std::any::Any>) -> Compiled {
    let index: HashMap<String, usize> = defs.iter().enumerate().map(|(i, f)| (f.name.clone(), i)).collect();
    let mut functions: Vec<String> = ptrs.keys().cloned().collect();
    functions.sort();
    Compiled {
        max_depth: std::cell::Cell::new(DEFAULT_MAX_DEPTH),
        functions,
        skipped,
        fallback: None,
        library: None,
        inner: Rc::new(Inner { _keep: keep, ptrs, defs, index, rich: HashMap::new(), layouts: Layouts::default(), refines: HashMap::new(), free: None, err_types: HashMap::new(), fids: HashMap::new() }),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn assemble_rich(
    defs: Vec<FnDef>,
    ptrs: HashMap<String, (*const u8, usize)>,
    rich: HashMap<String, RichFn>,
    skipped: BTreeMap<String, String>,
    keep: Box<dyn std::any::Any>,
    layouts: Layouts,
    refines: HashMap<i64, (String, String, Type)>,
    free: unsafe extern "C" fn(*mut i64),
    err_types: HashMap<i64, Type>,
    fids: HashMap<i64, usize>,
) -> Compiled {
    let index: HashMap<String, usize> = defs.iter().enumerate().map(|(i, f)| (f.name.clone(), i)).collect();
    let mut functions: Vec<String> = rich.keys().cloned().collect();
    functions.sort();
    Compiled {
        max_depth: std::cell::Cell::new(DEFAULT_MAX_DEPTH),
        functions,
        skipped,
        fallback: None,
        library: None,
        inner: Rc::new(Inner { _keep: keep, ptrs, defs, index, rich, layouts, refines, free: Some(free), err_types, fids }),
    }
}

pub fn compile(m: &Module) -> Result<Compiled, String> {
    let (defs, ok, skipped) = eligible(m);

    let mut flags = settings::builder();
    flags.set("opt_level", "speed").map_err(|e| e.to_string())?;
    flags.set("is_pic", "false").map_err(|e| e.to_string())?;
    flags.set("use_colocated_libcalls", "false").map_err(|e| e.to_string())?;
    let isa = cranelift_native::builder().map_err(|e| e.to_string())?.finish(settings::Flags::new(flags)).map_err(|e| e.to_string())?;
    let mut jb = JITBuilder::with_isa(isa, default_libcall_names());
    jb.symbol("sspur_pow", sspur_pow as *const u8);
    jb.symbol("sspur_note", sspur_note as *const u8);
    let mut module = JITModule::new(jb);
    let ptr_ty = module.target_config().pointer_type();

    let index: HashMap<String, usize> = defs.iter().enumerate().map(|(i, f)| (f.name.clone(), i)).collect();
    let mut ids: HashMap<String, FuncId> = HashMap::new();
    let mut entries: HashMap<String, FuncId> = HashMap::new();
    for f in defs.iter().filter(|f| ok.contains(&f.name)) {
        let mut sig = module.make_signature();
        for _ in &f.params {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.params.push(AbiParam::new(ptr_ty));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        let id = module.declare_function(&format!("sspur_{}", f.name), Linkage::Local, &sig).map_err(|e| e.to_string())?;
        ids.insert(f.name.clone(), id);
        let mut esig = module.make_signature();
        for _ in &f.params {
            esig.params.push(AbiParam::new(types::I64));
        }
        esig.params.push(AbiParam::new(ptr_ty));
        esig.returns.push(AbiParam::new(types::I64));
        let eid = module.declare_function(&format!("sspur_entry_{}", f.name), Linkage::Local, &esig).map_err(|e| e.to_string())?;
        entries.insert(f.name.clone(), eid);
    }
    let mut pow_sig = module.make_signature();
    pow_sig.params.extend([AbiParam::new(types::I64), AbiParam::new(types::I64), AbiParam::new(ptr_ty)]);
    pow_sig.returns.push(AbiParam::new(types::I64));
    let pow_id = module.declare_function("sspur_pow", Linkage::Import, &pow_sig).map_err(|e| e.to_string())?;
    let mut note_sig = module.make_signature();
    note_sig.params.push(AbiParam::new(types::I64));
    let note_id = module.declare_function("sspur_note", Linkage::Import, &note_sig).map_err(|e| e.to_string())?;

    let frontend_cfg = module.target_config();
    let mut ctx = module.make_context();
    let mut fctx = FunctionBuilderContext::new();
    for f in defs.iter().filter(|f| ok.contains(&f.name)) {
        let id = ids[&f.name];
        ctx.func.signature = module.declarations().get_function_decl(id).signature.clone();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            let params: Vec<Value> = b.block_params(entry).to_vec();
            let st = params[f.params.len()];
            let depth = params[f.params.len() + 1];
            let mut fun_refs = HashMap::new();
            for (n, fid) in &ids {
                fun_refs.insert(n.clone(), module.declare_func_in_func(*fid, b.func));
            }
            let pow_ref = module.declare_func_in_func(pow_id, b.func);
            let note_ref = module.declare_func_in_func(note_id, b.func);
            let mf = MemFlagsData::new();
            let mut g = Gen { b, mf, st, depth, scopes: vec![HashMap::new()], fun_refs, pow_ref, note_ref, fidx: index[&f.name] as i64, exit: None, result: None };
            g.function(f, &params);
            g.b.seal_all_blocks();
            g.b.finalize(frontend_cfg);
        }
        module.define_function(id, &mut ctx).map_err(|e| format!("{}: {e:?}", f.name))?;
        module.clear_context(&mut ctx);

        let eid = entries[&f.name];
        ctx.func.signature = module.declarations().get_function_decl(eid).signature.clone();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            let mut args: Vec<Value> = b.block_params(entry).to_vec();
            let stp = *args.last().unwrap();
            let limit = b.ins().load(types::I64, MemFlagsData::new(), stp, 32);
            let start = b.ins().ineg(limit);
            args.push(start);
            let callee = module.declare_func_in_func(id, b.func);
            let call = b.ins().call(callee, &args);
            let r = b.inst_results(call)[0];
            b.ins().return_(&[r]);
            b.seal_all_blocks();
            b.finalize(frontend_cfg);
        }
        module.define_function(eid, &mut ctx).map_err(|e| format!("{}: {e:?}", f.name))?;
        module.clear_context(&mut ctx);
    }
    module.finalize_definitions().map_err(|e| e.to_string())?;
    let mut ptrs = HashMap::new();
    for f in defs.iter().filter(|f| ok.contains(&f.name)) {
        ptrs.insert(f.name.clone(), (module.get_finalized_function(entries[&f.name]), f.params.len()));
    }
    Ok(assemble(defs, ptrs, skipped, Box::new(module)))
}

struct Gen<'a> {
    b: FunctionBuilder<'a>,
    st: Value,
    depth: Value,
    scopes: Vec<HashMap<String, Variable>>,
    fun_refs: HashMap<String, cranelift_codegen::ir::FuncRef>,
    pow_ref: cranelift_codegen::ir::FuncRef,
    note_ref: cranelift_codegen::ir::FuncRef,
    fidx: i64,
    exit: Option<cranelift_codegen::ir::Block>,
    result: Option<Variable>,
    mf: MemFlagsData,
}

impl Gen<'_> {
    fn var(&mut self, name: &str, v: Value) -> Variable {
        let var = self.b.declare_var(types::I64);
        self.b.def_var(var, v);
        self.scopes.last_mut().unwrap().insert(name.to_string(), var);
        var
    }

    fn lookup(&self, name: &str) -> Variable {
        *self.scopes.iter().rev().find_map(|s| s.get(name)).expect("eligibility guarantees bound names")
    }

    fn iconst(&mut self, n: i64) -> Value {
        self.b.ins().iconst(types::I64, n)
    }

    fn trap_if(&mut self, cond: Value, code: i64, clause: i64, value: Option<Value>) {
        self.trap_noting_value(cond, code, clause, value, &[]);
    }

    /// A trap that first records `notes` (parameter values) for the message.
    fn trap_noting(&mut self, cond: Value, code: i64, clause: i64, notes: &[Value]) {
        self.trap_noting_value(cond, code, clause, None, notes);
    }

    fn trap_noting_value(&mut self, cond: Value, code: i64, clause: i64, value: Option<Value>, notes: &[Value]) {
        let trap = self.b.create_block();
        let cont = self.b.create_block();
        self.b.ins().brif(cond, trap, &[], cont, &[]);
        self.b.switch_to_block(trap);
        for v in notes {
            self.b.ins().call(self.note_ref, &[*v]);
        }
        let c = self.iconst(code);
        let fi = self.iconst(self.fidx);
        let cl = self.iconst(clause);
        let flags = self.mf;
        self.b.ins().store(flags, c, self.st, 0);
        self.b.ins().store(flags, fi, self.st, 8);
        self.b.ins().store(flags, cl, self.st, 16);
        if let Some(v) = value {
            self.b.ins().store(flags, v, self.st, 24);
        }
        let z = self.iconst(0);
        self.b.ins().return_(&[z, c]);
        self.b.switch_to_block(cont);
    }

    fn propagate(&mut self, code: Value) {
        let out = self.b.create_block();
        let cont = self.b.create_block();
        self.b.ins().brif(code, out, &[], cont, &[]);
        self.b.switch_to_block(out);
        let z = self.iconst(0);
        self.b.ins().return_(&[z, code]);
        self.b.switch_to_block(cont);
    }

    fn function(&mut self, f: &FnDef, params: &[Value]) {
        let d1 = self.b.ins().iadd_imm_s(self.depth, 1);
        self.depth = d1;
        let too_deep = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, d1, 0);
        self.trap_if(too_deep, T_DEPTH, 0, None);
        for (p, v) in f.params.iter().zip(params) {
            self.var(&p.name, *v);
        }
        for (i, p) in f.params.iter().enumerate() {
            if let Some(r) = &p.refine {
                self.scopes.push(HashMap::new());
                self.var("_", params[i]);
                let ok = self.expr(r);
                self.scopes.pop();
                let bad = self.b.ins().icmp_imm_s(IntCC::Equal, ok, 0);
                self.trap_if(bad, T_PARAM, i as i64, Some(params[i]));
            }
        }
        for (i, pre) in f.pres.iter().enumerate() {
            let ok = self.expr(pre);
            let bad = self.b.ins().icmp_imm_s(IntCC::Equal, ok, 0);
            let notes: Vec<Value> = sspur_syntax::visit::mentioned_params(&f.params, pre).into_iter().map(|j| params[j]).collect();
            self.trap_noting(bad, T_PRE, i as i64, &notes);
        }
        let exit = self.b.create_block();
        let result = self.b.declare_var(types::I64);
        self.exit = Some(exit);
        self.result = Some(result);
        let v = self.expr(&f.body);
        self.b.def_var(result, v);
        self.b.ins().jump(exit, &[]);
        self.b.switch_to_block(exit);
        let r = self.b.use_var(result);
        if !f.posts.is_empty() {
            self.scopes.push(HashMap::new());
            self.var("r", r);
            for (i, post) in f.posts.iter().enumerate() {
                let ok = self.expr(post);
                let bad = self.b.ins().icmp_imm_s(IntCC::Equal, ok, 0);
                self.trap_if(bad, T_POST, i as i64, Some(r));
            }
            self.scopes.pop();
        }
        let ok = self.iconst(0);
        self.b.ins().return_(&[r, ok]);
    }

    fn bool_of(&mut self, cmp: Value) -> Value {
        self.b.ins().uextend(types::I64, cmp)
    }

    fn expr(&mut self, e: &Expr) -> Value {
        match &e.kind {
            ExprKind::Int(n) => self.iconst(*n),
            ExprKind::Bool(b) => self.iconst(i64::from(*b)),
            ExprKind::Unit => self.iconst(0),
            ExprKind::Name(n) => {
                let v = self.lookup(n);
                self.b.use_var(v)
            }
            ExprKind::Placeholder => {
                let v = self.lookup("_");
                self.b.use_var(v)
            }
            ExprKind::Unary(UnOp::Neg, x) => {
                let v = self.expr(x);
                let is_min = self.b.ins().icmp_imm_s(IntCC::Equal, v, i64::MIN);
                self.trap_if(is_min, T_OVERFLOW, 0, None);
                self.b.ins().ineg(v)
            }
            ExprKind::Unary(UnOp::Not, x) => {
                let v = self.expr(x);
                let c = self.b.ins().icmp_imm_s(IntCC::Equal, v, 0);
                self.bool_of(c)
            }
            ExprKind::Binary(BinOp::And | BinOp::Or, a, b) => {
                let is_and = matches!(e.kind, ExprKind::Binary(BinOp::And, ..));
                let res = self.b.declare_var(types::I64);
                let av = self.expr(a);
                self.b.def_var(res, av);
                let rhs = self.b.create_block();
                let done = self.b.create_block();
                if is_and {
                    self.b.ins().brif(av, rhs, &[], done, &[]);
                } else {
                    self.b.ins().brif(av, done, &[], rhs, &[]);
                }
                self.b.switch_to_block(rhs);
                let bv = self.expr(b);
                self.b.def_var(res, bv);
                self.b.ins().jump(done, &[]);
                self.b.switch_to_block(done);
                self.b.use_var(res)
            }
            ExprKind::Binary(op, a, b) => {
                let lit = |e: &Expr| if let ExprKind::Int(n) = e.kind { Some(n) } else { None };
                let (la, lb) = (lit(a), lit(b));
                let x = self.expr(a);
                let y = self.expr(b);
                match (op, la, lb) {
                    (BinOp::Div | BinOp::Rem, _, Some(c)) if c != 0 && c != -1 => {
                        if *op == BinOp::Div { self.b.ins().sdiv(x, y) } else { self.b.ins().srem(x, y) }
                    }
                    (BinOp::Mul, Some(c), _) if c > 0 => self.mul_const(y, c),
                    (BinOp::Mul, _, Some(c)) if c > 0 => self.mul_const(x, c),
                    _ => self.binop(*op, x, y),
                }
            }
            ExprKind::If(c, t, f) => {
                let cv = self.expr(c);
                let res = self.b.declare_var(types::I64);
                let tb = self.b.create_block();
                let fb = self.b.create_block();
                let done = self.b.create_block();
                self.b.ins().brif(cv, tb, &[], fb, &[]);
                self.b.switch_to_block(tb);
                let tv = self.expr(t);
                self.b.def_var(res, tv);
                self.b.ins().jump(done, &[]);
                self.b.switch_to_block(fb);
                let fv = match f {
                    Some(f) => self.expr(f),
                    None => self.iconst(0),
                };
                self.b.def_var(res, fv);
                self.b.ins().jump(done, &[]);
                self.b.switch_to_block(done);
                self.b.use_var(res)
            }
            ExprKind::Call(f, args) => {
                let ExprKind::Name(n) = &f.kind else { unreachable!() };
                let mut vals: Vec<Value> = args.iter().map(|a| self.expr(a)).collect();
                vals.push(self.st);
                vals.push(self.depth);
                let fref = self.fun_refs[n];
                let call = self.b.ins().call(fref, &vals);
                let (r, code) = (self.b.inst_results(call)[0], self.b.inst_results(call)[1]);
                self.propagate(code);
                r
            }
            ExprKind::Return(x) => {
                let v = self.expr(x);
                self.b.def_var(self.result.unwrap(), v);
                self.b.ins().jump(self.exit.unwrap(), &[]);
                let dead = self.b.create_block();
                self.b.switch_to_block(dead);
                self.iconst(0)
            }
            ExprKind::Block(stmts) => {
                self.scopes.push(HashMap::new());
                let mut last = self.iconst(0);
                for s in stmts {
                    last = self.stmt(s);
                }
                self.scopes.pop();
                last
            }
            _ => unreachable!("eligibility check rejects this expression"),
        }
    }

    fn mul_const(&mut self, x: Value, c: i64) -> Value {
        if c == 1 {
            return x;
        }
        let hi = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, x, i64::MAX / c);
        let lo = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, x, i64::MIN / c);
        let ov = self.b.ins().bor(hi, lo);
        self.trap_if(ov, T_OVERFLOW, 0, None);
        self.b.ins().imul_imm_s(x, c)
    }

    fn binop(&mut self, op: BinOp, x: Value, y: Value) -> Value {
        match op {
            BinOp::Add => {
                let (r, ov) = self.b.ins().sadd_overflow(x, y);
                self.trap_if(ov, T_OVERFLOW, 0, None);
                r
            }
            BinOp::Sub => {
                let (r, ov) = self.b.ins().ssub_overflow(x, y);
                self.trap_if(ov, T_OVERFLOW, 0, None);
                r
            }
            BinOp::Mul => {
                let (r, ov) = self.b.ins().smul_overflow(x, y);
                self.trap_if(ov, T_OVERFLOW, 0, None);
                r
            }
            BinOp::Div | BinOp::Rem => {
                let zero = self.b.ins().icmp_imm_s(IntCC::Equal, y, 0);
                self.trap_if(zero, T_DIV_ZERO, 0, None);
                let a = self.b.ins().icmp_imm_s(IntCC::Equal, x, i64::MIN);
                let b = self.b.ins().icmp_imm_s(IntCC::Equal, y, -1);
                let ov = self.b.ins().band(a, b);
                self.trap_if(ov, T_OVERFLOW, 0, None);
                if op == BinOp::Div { self.b.ins().sdiv(x, y) } else { self.b.ins().srem(x, y) }
            }
            BinOp::Pow => {
                let call = self.b.ins().call(self.pow_ref, &[x, y, self.st]);
                let r = self.b.inst_results(call)[0];
                let code = self.b.ins().load(types::I64, self.mf, self.st, 0);
                let bad = self.b.ins().icmp_imm_s(IntCC::NotEqual, code, 0);
                let out = self.b.create_block();
                let cont = self.b.create_block();
                self.b.ins().brif(bad, out, &[], cont, &[]);
                self.b.switch_to_block(out);
                let fi = self.iconst(self.fidx);
                self.b.ins().store(self.mf, fi, self.st, 8);
                let z = self.iconst(0);
                self.b.ins().return_(&[z, code]);
                self.b.switch_to_block(cont);
                r
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let cc = match op {
                    BinOp::Eq => IntCC::Equal,
                    BinOp::Ne => IntCC::NotEqual,
                    BinOp::Lt => IntCC::SignedLessThan,
                    BinOp::Le => IntCC::SignedLessThanOrEqual,
                    BinOp::Gt => IntCC::SignedGreaterThan,
                    _ => IntCC::SignedGreaterThanOrEqual,
                };
                let c = self.b.ins().icmp(cc, x, y);
                self.bool_of(c)
            }
            BinOp::And | BinOp::Or => unreachable!(),
        }
    }

    fn stmt(&mut self, s: &Stmt) -> Value {
        match s {
            Stmt::Expr(e) => self.expr(e),
            Stmt::Let(Pat::Bind(n), e) | Stmt::Var(n, e) => {
                let v = self.expr(e);
                self.var(n, v);
                self.iconst(0)
            }
            Stmt::Let(_, e) => {
                self.expr(e);
                self.iconst(0)
            }
            Stmt::Assign(n, e, _) => {
                let v = self.expr(e);
                let var = self.lookup(n);
                self.b.def_var(var, v);
                self.iconst(0)
            }
            Stmt::While(c, body) => {
                let head = self.b.create_block();
                let bodyb = self.b.create_block();
                let done = self.b.create_block();
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(head);
                let cv = self.expr(c);
                self.b.ins().brif(cv, bodyb, &[], done, &[]);
                self.b.switch_to_block(bodyb);
                self.scopes.push(HashMap::new());
                self.expr(body);
                self.scopes.pop();
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(done);
                self.iconst(0)
            }
            Stmt::For(Pat::Bind(i), it, body) => {
                let ExprKind::Range(a, b) = &it.kind else { unreachable!() };
                let start = self.expr(a);
                let end = self.expr(b);
                let end_var = self.b.declare_var(types::I64);
                self.b.def_var(end_var, end);
                let counter = self.b.declare_var(types::I64);
                self.b.def_var(counter, start);
                let head = self.b.create_block();
                let bodyb = self.b.create_block();
                let done = self.b.create_block();
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(head);
                let cur = self.b.use_var(counter);
                let endv = self.b.use_var(end_var);
                let more = self.b.ins().icmp(IntCC::SignedLessThan, cur, endv);
                self.b.ins().brif(more, bodyb, &[], done, &[]);
                self.b.switch_to_block(bodyb);
                self.scopes.push(HashMap::new());
                let cur = self.b.use_var(counter);
                self.var(i, cur);
                self.expr(body);
                self.scopes.pop();
                let cur = self.b.use_var(counter);
                let next = self.b.ins().iadd_imm_s(cur, 1);
                self.b.def_var(counter, next);
                self.b.ins().jump(head, &[]);
                self.b.switch_to_block(done);
                self.iconst(0)
            }
            _ => unreachable!("eligibility check rejects this statement"),
        }
    }
}
