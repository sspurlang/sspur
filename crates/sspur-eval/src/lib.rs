#![allow(clippy::mutable_key_type)]
mod builtins;
#[cfg(feature = "native")]
mod ffi;
#[cfg(not(target_family = "wasm"))]
mod lastuse;
mod net;
#[cfg(feature = "native")]
mod libs;
pub mod fuzz;
pub mod progen;
pub mod suggest;
pub mod synth;
pub mod kernel;
mod sched;
mod stdlib;
mod stdbig;
#[cfg(not(target_family = "wasm"))]
mod stdbytes;
mod stdcx;
mod stdratio;
mod stdlocale;
mod stdfile;
mod stdflat;
mod stdre;
mod stdrng;
mod stdtime;
mod stdtz;
mod stdview;
mod stdx;
mod sys;
#[cfg(target_family = "wasm")]
pub mod hop;
#[cfg(target_family = "wasm")]
pub mod web;
pub mod value;

use sspur_syntax::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
pub use value::Value;
use value::{Closure, Env};

pub enum Ctrl {
    Raise(Value),
    Return(Value),
    Trap(String),
    Abort(u64, Value),
    Escape(u64, Box<Ctrl>),
}

enum Clause {
    Resume(Value, Option<Post>),
    Done(Value),
}

struct Post {
    pat: Pat,
    rest: Vec<Stmt>,
    env: Rc<Env>,
}

#[derive(Clone)]
enum HKind {
    Arms(*const [Arm], Rc<Env>),
    Loop(*const Pat, *const Expr, Rc<Env>),
}

struct HFrame {
    id: u64,
    kind: HKind,
    posts: Vec<Post>,
}

impl HFrame {
    fn handles(&self, op: &str) -> bool {
        match &self.kind {
            HKind::Arms(arms, _) => unsafe { &**arms }.iter().any(|a| matches!(&a.pat, Pat::Ctor { name, .. } if name == op && name != "return")),
            HKind::Loop(..) => op == "yield",
        }
    }
}

fn is_resume(e: &Expr) -> Option<&[Expr]> {
    match &e.kind {
        ExprKind::Call(f, args) if matches!(&f.kind, ExprKind::Name(n) if n == "resume") => Some(args),
        _ => None,
    }
}

pub type R<T = Value> = Result<T, Ctrl>;

pub fn trap<T>(msg: impl Into<String>) -> R<T> {
    Err(Ctrl::Trap(msg.into()))
}

struct Refine {
    field: Option<String>,
    expr: Expr,
}

pub struct Interp {
    fns: HashMap<String, Rc<FnDef>>,
    tests: Vec<TestDef>,
    variant_fields: HashMap<String, Option<Vec<String>>>,
    record_fields: HashMap<String, Vec<String>>,
    newtypes: HashSet<String>,
    alias_refines: HashMap<String, Expr>,
    type_refines: HashMap<String, Vec<Refine>>,
    field_types: HashMap<(String, String), String>,
    record_types: HashMap<(u32, u32), String>,
    user_methods: HashSet<(u32, u32)>,
    globals: Rc<Env>,
    types: HashMap<String, TypeDef>,
    pub output: RefCell<Option<Vec<String>>>,
    depth: Cell<u32>,
    pub fuel: Cell<u64>,
    #[cfg(feature = "native")]
    native: Option<sspur_native::Compiled>,
    pub bypass_native: Cell<bool>,
    pub native_hits: Cell<u64>,
    pub calls: RefCell<Option<std::collections::BTreeMap<String, u64>>>,
    pub float_sums: HashSet<(u32, u32)>,
    ops: HashSet<String>,
    gen_loops: HashSet<(u32, u32)>,
    handlers: RefCell<Vec<HFrame>>,
    next_handler: Cell<u64>,
    sys: bool,
    bare: bool,
    drops: HashMap<String, String>,
    moves: HashSet<(u32, u32)>,
    inplace: HashSet<(u32, u32)>,
    #[cfg(not(target_family = "wasm"))]
    last_uses: RefCell<Option<HashSet<(u32, u32)>>>,
    heap: RefCell<Vec<Option<Vec<Value>>>>,
    sched: sched::Sched,
    pub json_types: HashMap<(u32, u32), sspur_check::Type>,
    pub layouts: sspur_native::nval::Layouts,
    kernels: RefCell<HashMap<String, Rc<sspur_check::kernel::Kernel>>>,
    statics: HashMap<String, Expr>,
    static_vals: RefCell<HashMap<String, Value>>,
    /// Functions from dependencies: their examples are the dependency's tests, not ours.
    pub foreign: HashSet<String>,
    /// What reaching a typed hole does: trap, record the values in scope, or evaluate a candidate.
    hole: RefCell<synth::HoleMode>,
    /// Synthesized `sum` calls over `F64` lists, which give `0.0` when empty.
    synth_float: RefCell<HashSet<(u32, u32)>>,
    /// Arguments of every call to the named function, for `sspur suggest`.
    recording: Cell<bool>,
    record: RefCell<(String, Vec<Vec<Value>>)>,
}

/// Calls deep on a thread whose stack the interpreter doesn't know (see `stack_from_here`).
const MAX_DEPTH: u32 = 20_000;
/// Calls deep on a thread that registered its stack; the stack itself is the real limit.
#[cfg(not(target_family = "wasm"))]
const MAX_DEPTH_KNOWN: u32 = 50_000_000;

#[cfg(not(target_family = "wasm"))]
thread_local! {
    static STACK_FLOOR: Cell<usize> = const { Cell::new(0) };
}

#[cfg(not(target_family = "wasm"))]
#[inline(always)]
fn stack_pointer() -> usize {
    let here = 0u8;
    std::hint::black_box(&here) as *const u8 as usize
}

/// Lets interpreted calls on this thread nest until they use `budget` bytes of stack below the
/// caller's frame, instead of stopping at 20,000 calls. The thread must have more stack than
/// that, for the builtins and native code that run at the deepest call.
#[cfg(not(target_family = "wasm"))]
pub fn stack_from_here(budget: usize) {
    let floor = stack_pointer().saturating_sub(budget);
    STACK_FLOOR.with(|f| f.set(floor));
}

/// The interpreter's stack budget from `SSPUR_STACK` (`512M`, `2G`, `65536`); without it,
/// interpreted calls stop at 20,000 deep.
#[cfg(not(target_family = "wasm"))]
pub fn stack_budget() -> Result<Option<usize>, String> {
    let Ok(v) = std::env::var("SSPUR_STACK") else { return Ok(None) };
    let t = v.trim();
    let (num, shift) = match t.chars().last().map(|c| c.to_ascii_uppercase()) {
        Some('K') => (&t[..t.len() - 1], 10),
        Some('M') => (&t[..t.len() - 1], 20),
        Some('G') => (&t[..t.len() - 1], 30),
        _ => (t, 0),
    };
    match num.parse::<usize>().ok().and_then(|n| n.checked_shl(shift).filter(|b| *b >> shift == n)) {
        Some(b) if b >= 1 << 20 => Ok(Some(b)),
        _ => Err(format!("SSPUR_STACK={v} is not a stack size of at least 1M, like 512M or 2G; calls stop at 20,000 deep")),
    }
}

#[cfg(not(target_family = "wasm"))]
fn too_deep(d: u32) -> bool {
    match STACK_FLOOR.with(Cell::get) {
        0 => d > MAX_DEPTH,
        floor => d > MAX_DEPTH_KNOWN || stack_pointer() < floor,
    }
}
const BARE_NAMES: &[&str] = &["halt", "wait_irq", "irq_enable", "timer_start", "ticks", "tick_hz", "arch", "start_core", "core_id"];
pub const OUT_OF_FUEL: &str = "evaluation step budget exhausted";

fn pat_rebinds(p: &Pat, env: &Env) -> bool {
    match p {
        Pat::Bind(n) => env.cell(n).is_some(),
        Pat::Tuple(ps) => ps.iter().any(|p| pat_rebinds(p, env)),
        Pat::Ctor { args: CtorArgs::Positional(ps), .. } => ps.iter().any(|p| pat_rebinds(p, env)),
        Pat::Ctor { args: CtorArgs::Record(fs), .. } => fs.iter().any(|(_, p)| pat_rebinds(p, env)),
        Pat::Or(ps) => ps.iter().any(|p| pat_rebinds(p, env)),
        Pat::List { head, rest, tail } => head.iter().chain(tail).any(|p| pat_rebinds(p, env)) || matches!(rest, Some(Some(r)) if env.cell(r).is_some()),
        _ => false,
    }
}

#[inline(never)]
fn define_local_fns(stmts: &[Stmt], env: &Rc<Env>) {
    for s in stmts {
        if let Stmt::Fn(f) = s {
            env.define(&f.name, Value::LocalFn(Rc::new((**f).clone()), env.clone()));
        }
    }
}

#[inline(never)]
fn frame_for(s: &Stmt, frames: &mut Vec<Rc<Env>>) -> Rc<Env> {
    let cur = frames.last().unwrap().clone();
    let shadows = match s {
        Stmt::Var(n, _) => cur.cell(n).is_some(),
        Stmt::Let(p, _) => pat_rebinds(p, &cur),
        _ => false,
    };
    if shadows {
        frames.push(Env::child(&cur));
        return frames.last().unwrap().clone();
    }
    if let Stmt::Fn(f) = s
        && frames.len() > 1
        && let Some(c) = cur.cell(&f.name)
    {
        *c.borrow_mut() = Value::LocalFn(Rc::new((**f).clone()), cur.clone());
    }
    cur
}

#[inline(never)]
fn no_match(p: &Pat, v: &Value) -> R {
    trap(format!("pattern {} did not match {v}", printer::pat(p)))
}

#[inline(never)]
fn has_span(set: &HashSet<(u32, u32)>, span: Span) -> bool {
    set.contains(&(span.start, span.end))
}

/// Empties `n`'s cell when it still holds `v`, so `v` is the value's only reference.
#[cfg(not(target_family = "wasm"))]
fn release(n: &str, v: &Value, env: &Rc<Env>) {
    let Some(c) = env.cell(n) else { return };
    let same = match (&*c.borrow(), v) {
        (Value::List(a), Value::List(b)) | (Value::Heap(a), Value::Heap(b)) => Rc::ptr_eq(a, b),
        (Value::Map(a), Value::Map(b)) => Rc::ptr_eq(a, b),
        (Value::Set(a), Value::Set(b)) => Rc::ptr_eq(a, b),
        (Value::Buf(a), Value::Buf(b)) => Rc::ptr_eq(a, b),
        _ => false,
    };
    if same {
        *c.borrow_mut() = Value::Unit;
    }
}

#[inline(never)]
fn closure(params: &[String], body: &Expr, env: &Rc<Env>) -> Value {
    Value::Closure(Rc::new(Closure { params: params.to_vec(), body: body.clone(), env: env.clone() }))
}

fn ty_name(t: &Ty) -> Option<&str> {
    match t {
        Ty::Named { name, .. } => Some(name),
        _ => None,
    }
}

impl Interp {
    pub fn new(m: &Module, record_types: HashMap<(u32, u32), String>, user_methods: HashSet<(u32, u32)>, gen_loops: HashSet<(u32, u32)>) -> Self {
        let mut it = Interp {
            fns: HashMap::new(),
            tests: vec![],
            variant_fields: HashMap::new(),
            record_fields: HashMap::new(),
            newtypes: HashSet::new(),
            alias_refines: HashMap::new(),
            type_refines: HashMap::new(),
            field_types: HashMap::new(),
            record_types,
            user_methods,
            globals: Rc::new(Env::default()),
            types: HashMap::new(),
            output: RefCell::new(None),
            depth: Cell::new(0),
            fuel: Cell::new(u64::MAX),
            #[cfg(feature = "native")]
            native: None,
            bypass_native: Cell::new(false),
            native_hits: Cell::new(0),
            calls: RefCell::new(None),
            float_sums: HashSet::new(),
            ops: ["yield", "log"].iter().map(|s| s.to_string()).collect(),
            gen_loops,
            handlers: RefCell::new(vec![]),
            next_handler: Cell::new(0),
            sys: matches!(m.profile.as_deref(), Some("sys" | "bare")),
            bare: m.profile.as_deref() == Some("bare"),
            drops: HashMap::new(),
            moves: HashSet::new(),
            inplace: HashSet::new(),
            #[cfg(not(target_family = "wasm"))]
            last_uses: RefCell::new(None),
            heap: RefCell::new(vec![None]),
            sched: Default::default(),
            json_types: HashMap::new(),
            layouts: Default::default(),
            kernels: RefCell::new(HashMap::new()),
            statics: HashMap::new(),
            static_vals: RefCell::new(HashMap::new()),
            foreign: HashSet::new(),
            hole: RefCell::new(synth::HoleMode::default()),
            synth_float: RefCell::new(HashSet::new()),
            recording: Cell::new(false),
            record: RefCell::new((String::new(), vec![])),
        };
        for (internal, src) in sspur_check::STD_RECORDS.iter().filter(|(n, _)| sspur_check::OPEN_STD_RECORDS.contains(n)).chain(sspur_check::std_sums()) {
            if let Ok(sm) = sspur_syntax::parse(src)
                && let Some(Def::Type(t)) = sm.defs.first()
            {
                let mut t = t.clone();
                t.name = internal.to_string();
                it.add_type(&t);
                #[cfg(not(target_family = "wasm"))]
                if matches!(t.body, TypeBody::Sum(_)) {
                    it.types.insert(t.name.clone(), t);
                }
            }
        }
        for d in &m.defs {
            match d {
                Def::Fn(f) => {
                    it.fns.insert(f.name.clone(), Rc::new(f.clone()));
                }
                Def::Test(t) => it.tests.push(t.clone()),
                Def::Type(t) => {
                    if let Some(d) = t.drop.as_ref().filter(|_| t.res) {
                        it.drops.insert(t.name.clone(), d.clone());
                    }
                    it.types.insert(t.name.clone(), t.clone());
                    it.add_type(t)
                }
                Def::Effect(e) => it.ops.extend(e.ops.iter().map(|o| o.name.clone())),
                Def::Static(s) => {
                    it.statics.insert(s.name.clone(), s.init.clone());
                }
                Def::Store(_) | Def::Svc(_) | Def::Use(_) | Def::Trait(_) | Def::Impl(_) => {}
            }
        }
        it
    }

    fn add_type(&mut self, t: &TypeDef) {
        let add_fields = |owner: &str, fs: &[Field], refines: &mut HashMap<String, Vec<Refine>>, ftypes: &mut HashMap<(String, String), String>| {
            for f in fs {
                if let Some(r) = &f.refine {
                    refines.entry(owner.to_string()).or_default().push(Refine { field: Some(f.name.clone()), expr: r.clone() });
                }
                if let Some(n) = ty_name(&f.ty) {
                    ftypes.insert((owner.to_string(), f.name.clone()), n.to_string());
                }
            }
        };
        match &t.body {
            TypeBody::Record(fs) => {
                self.record_fields.insert(t.name.clone(), fs.iter().map(|f| f.name.clone()).collect());
                add_fields(&t.name, fs, &mut self.type_refines, &mut self.field_types);
            }
            TypeBody::Sum(vs) => {
                for v in vs {
                    self.variant_fields.insert(v.name.clone(), v.fields.as_ref().map(|fs| fs.iter().map(|f| f.name.clone()).collect()));
                    if let Some(fs) = &v.fields {
                        add_fields(&v.name, fs, &mut self.type_refines, &mut self.field_types);
                    }
                }
            }
            TypeBody::Alias(_, Some(r)) => {
                self.alias_refines.insert(t.name.clone(), r.clone());
            }
            TypeBody::Alias(_, None) => {}
            TypeBody::New(_) => {
                self.newtypes.insert(t.name.clone());
            }
        }
    }

    fn emit(&self, line: String) {
        match &mut *self.output.borrow_mut() {
            Some(buf) => buf.push(line),
            None => sspur_native::emit_log(&line),
        }
    }

    pub fn run_main(&self) -> Result<(), String> {
        let Some(main) = self.fns.get("main").cloned() else {
            return Err("no 'main' function".into());
        };
        if !main.params.is_empty() {
            return Err("'main' must take no parameters".into());
        }
        self.static_vals.borrow_mut().clear();
        match self.call_fn(&main, vec![]).and_then(|_| kernel::sync()) {
            Ok(_) => Ok(()),
            Err(c) => Err(describe(c)),
        }
    }

    /// `run_main` for a program whose native `main` already ran outside the interpreter (the
    /// cached path of `sspur run`) and failed: the same error `run_main` would have reported.
    #[cfg(feature = "native")]
    pub fn run_main_failed(&self, failure: sspur_native::MainFailure) -> Result<(), String> {
        let Some(main) = self.fns.get("main").cloned() else {
            return Err("no 'main' function".into());
        };
        self.static_vals.borrow_mut().clear();
        let r = match self.native.as_ref().and_then(|n| n.main_failure(failure)) {
            Some(sspur_native::NativeError::Trap(msg)) => trap(msg),
            Some(sspur_native::NativeError::Raise(v)) => Err(Ctrl::Raise(from_nval(v))),
            None => {
                let globals = self.globals.clone();
                self.call_fn_in(&main, vec![], &globals)
            }
        };
        match r.and_then(|_| kernel::sync()) {
            Ok(_) => Ok(()),
            Err(c) => Err(describe(c)),
        }
    }

    pub fn run_tests(&self) -> Vec<(String, Result<(), String>)> {
        self.test_cases().iter().map(|(name, body)| (name.clone(), self.run_case(body))).collect()
    }

    /// The examples of the program's own functions, then its tests, with their names.
    pub fn test_cases(&self) -> Vec<(String, Expr)> {
        let mut cases: Vec<(String, Expr)> = Vec::new();
        let mut names: Vec<&String> = self.fns.keys().collect();
        names.sort();
        for n in names.into_iter().filter(|n| !self.foreign.contains(*n)) {
            for (i, ex) in self.fns[n].examples.iter().enumerate() {
                cases.push((format!("{n}.ex{}", i + 1), ex.clone()));
            }
        }
        cases.extend(self.tests.iter().map(|t| (t.name.clone(), t.body.clone())));
        cases
    }

    /// Runs one test or example body with fresh statics and captured output.
    pub fn run_case(&self, body: &Expr) -> Result<(), String> {
        self.static_vals.borrow_mut().clear();
        *self.output.borrow_mut() = Some(vec![]);
        let r = match self.eval(body, &Env::child(&self.globals)).and_then(|v| kernel::sync().map(|_| v)) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(Value::Bool(false)) => Err(self.explain(body, &Env::child(&self.globals)).unwrap_or_else(|| "evaluated to false".to_string())),
            Ok(v) => Err(format!("expected Bool, got {v}")),
            Err(c) => Err(describe(c)),
        };
        *self.output.borrow_mut() = None;
        r
    }

    /// Why a test body that evaluated to false is false: the values of the comparison that failed.
    /// Re-evaluates the parts it shows; `None` if nothing more specific than "false" can be said.
    fn explain(&self, e: &Expr, env: &Rc<Env>) -> Option<String> {
        use BinOp::*;
        let show = |v: &Value| clip(value::Quoted(v).to_string());
        let is_false = |v: &R| matches!(v, Ok(Value::Bool(false)));
        match &e.kind {
            ExprKind::Binary(And, l, r) => match self.truthy(l, env).ok()? {
                false => self.explain(l, env),
                true => self.explain(r, env),
            },
            ExprKind::Binary(Or, l, r) => match (self.explain(l, env), self.explain(r, env)) {
                (Some(a), Some(b)) => Some(format!("{a}; {b}")),
                (a, b) => a.or(b),
            },
            ExprKind::Binary(op @ (Eq | Ne | Lt | Le | Gt | Ge), l, r) => {
                let a = self.eval(l, env).ok()?;
                let b = self.eval(r, env).ok()?;
                let ok = match op {
                    Eq => a == b,
                    Ne => a != b,
                    Lt => a < b,
                    Le => a <= b,
                    Gt => a > b,
                    _ => a >= b,
                };
                (!ok).then(|| format!("left {}, right {}", show(&a), show(&b)))
            }
            ExprKind::Unary(UnOp::Not, x) => match &x.kind {
                ExprKind::Binary(Eq | Ne | Lt | Le | Gt | Ge, l, r) => {
                    let a = self.eval(l, env).ok()?;
                    let b = self.eval(r, env).ok()?;
                    Some(format!("left {}, right {}", show(&a), show(&b)))
                }
                _ => None,
            },
            ExprKind::Method { recv, name, args, .. } if !args.iter().any(|a| matches!(a.kind, ExprKind::Lambda { .. })) => {
                let rv = self.eval(recv, env).ok()?;
                let avs: Vec<String> = args.iter().map(|a| self.eval(a, env).ok().map(|v| show(&v))).collect::<Option<_>>()?;
                is_false(&self.eval(e, env)).then(|| format!("{}.{name}({}) is false", show(&rv), avs.join(", ")))
            }
            ExprKind::Method { recv, name, .. } => {
                let rv = self.eval(recv, env).ok()?;
                is_false(&self.eval(e, env)).then(|| format!("{name} is false on {}", show(&rv)))
            }
            ExprKind::Catch(body, arms) => match self.eval(body, env) {
                Ok(Value::Bool(false)) => self.explain(body, env),
                Err(Ctrl::Raise(v)) => {
                    for a in arms {
                        let inner = Env::child(env);
                        if !self.bind_pat(&a.pat, &v, &inner) || a.guard.as_ref().is_some_and(|g| !self.truthy(g, &inner).unwrap_or(false)) {
                            continue;
                        }
                        let why = match a.body.kind {
                            ExprKind::Bool(false) => None,
                            _ => self.explain(&a.body, &inner),
                        };
                        return Some(match why {
                            Some(w) => format!("raised {}: {w}", show(&v)),
                            None => format!("raised {}", show(&v)),
                        });
                    }
                    None
                }
                _ => None,
            },
            ExprKind::Match(s, arms, _) => {
                let v = self.eval(s, env).ok()?;
                for a in arms {
                    let inner = Env::child(env);
                    if !self.bind_pat(&a.pat, &v, &inner) || a.guard.as_ref().is_some_and(|g| !self.truthy(g, &inner).unwrap_or(false)) {
                        continue;
                    }
                    return match a.body.kind {
                        ExprKind::Bool(false) => Some(format!("matched {}", show(&v))),
                        _ => self.explain(&a.body, &inner),
                    };
                }
                None
            }
            ExprKind::If(c, a, b) => match self.truthy(c, env).ok()? {
                true => self.explain(a, env),
                false => self.explain(b.as_deref()?, env),
            },
            ExprKind::Block(stmts) if !stmts.iter().any(|s| matches!(s, Stmt::Fn(_))) => {
                let (Stmt::Expr(last), init) = stmts.split_last()? else { return None };
                let mut frames = vec![Env::child(env)];
                for s in init {
                    let cur = frame_for(s, &mut frames);
                    self.exec(s, &cur).ok()?;
                }
                self.explain(last, frames.last()?)
            }
            _ => None,
        }
    }

    pub fn set_ownership(&mut self, moves: HashSet<(u32, u32)>, inplace: HashSet<(u32, u32)>) {
        self.moves = moves;
        self.inplace = inplace;
    }

    fn droppable(&self, v: &Value) -> bool {
        matches!(v, Value::Record(t, _) if self.drops.contains_key(&**t))
    }

    fn own_define(&self, env: &Rc<Env>, name: &str, v: Value) {
        let owned = self.droppable(&v);
        env.define(name, v);
        if owned {
            env.owned.borrow_mut().push(name.into());
        }
    }

    fn run_drop(&self, v: Value) -> R<()> {
        let Value::Record(t, _) = &v else { return Ok(()) };
        let Some(f) = self.drops.get(&**t).and_then(|d| self.fns.get(d)).cloned() else { return Ok(()) };
        self.call_fn(&f, vec![v]).map(|_| ())
    }

    fn release<T>(&self, env: &Rc<Env>, r: R<T>) -> R<T> {
        if env.owned.borrow().is_empty() || matches!(r, Err(Ctrl::Trap(_))) {
            return r;
        }
        let names: Vec<Rc<str>> = std::mem::take(&mut *env.owned.borrow_mut());
        for n in names.iter().rev() {
            let Some(cell) = env.vars.borrow().get(n).cloned() else { continue };
            let v = std::mem::replace(&mut *cell.borrow_mut(), Value::Unit);
            if self.droppable(&v) {
                self.run_drop(v)?;
            }
        }
        r
    }

    #[inline(never)]
    fn arg_values(&self, f: &FnDef, args: &[&Expr], env: &Rc<Env>) -> R<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for (i, a) in args.iter().enumerate() {
            let borrow = f.params.get(i).is_some_and(|p| matches!(&p.ty, Ty::Named { name, .. } if name == "&" || name == "&mut"));
            if !borrow {
                out.push(self.eval(a, env)?);
                continue;
            }
            let place = match &a.kind {
                ExprKind::Unary(UnOp::Ref | UnOp::RefMut, x) => x,
                _ => *a,
            };
            let cell = match &place.kind {
                ExprKind::Name(n) => env.cell(n),
                _ => None,
            };
            let cell = match cell {
                Some(c) => c,
                None => Rc::new(RefCell::new(self.eval(place, env)?)),
            };
            out.push(Value::Ref(cell));
        }
        Ok(out)
    }

    fn sys_call(&self, n: &str, a: Vec<Value>) -> R {
        let mut a = a.into_iter();
        let mut next = || a.next().unwrap_or(Value::Unit);
        match n {
            "drop" => self.run_drop(next()).map(|_| Value::Unit),
            "leak" => Ok(Value::Unit),
            "null" => Ok(Value::Ptr(0, 0)),
            "alloc" => {
                let Value::Int(len) = next() else { return trap("alloc expects an Int size") };
                if !(0..=1 << 28).contains(&len) {
                    return trap(format!("invalid allocation size {len}"));
                }
                let init = next();
                let mut heap = self.heap.borrow_mut();
                heap.push(Some(vec![init; len as usize]));
                Ok(Value::Ptr(heap.len() as u32 - 1, 0))
            }
            "free" => {
                let Value::Ptr(b, off) = next() else { return trap("free expects a Ptr") };
                let mut heap = self.heap.borrow_mut();
                match heap.get_mut(b as usize) {
                    _ if b == 0 => trap("undefined behavior: free of null"),
                    Some(slot @ Some(_)) if off == 0 => {
                        *slot = None;
                        Ok(Value::Unit)
                    }
                    Some(Some(_)) => trap("undefined behavior: free of an interior pointer"),
                    _ => trap("undefined behavior: double free"),
                }
            }
            _ => trap(format!("unknown sys builtin {n}")),
        }
    }

    fn ptr_method(&self, name: &str, b: u32, off: i64, a: Vec<Value>) -> R {
        if name == "is_null" {
            return Ok(Value::Bool(b == 0));
        }
        let mut a = a.into_iter();
        let Some(Value::Int(i)) = a.next() else { return trap(format!("Ptr.{name} expects an Int")) };
        if name == "offset" {
            return Ok(Value::Ptr(b, off.checked_add(i).ok_or_else(|| Ctrl::Trap("undefined behavior: pointer overflow".into()))?));
        }
        let mut heap = self.heap.borrow_mut();
        let block = match heap.get_mut(b as usize) {
            _ if b == 0 => return trap("undefined behavior: null dereference"),
            Some(Some(block)) => block,
            _ => return trap("undefined behavior: use after free"),
        };
        let k = off.checked_add(i).filter(|k| *k >= 0 && (*k as usize) < block.len());
        let Some(k) = k else { return trap("undefined behavior: out-of-bounds access") };
        match name {
            "load" => Ok(block[k as usize].clone()),
            "store" => {
                block[k as usize] = a.next().unwrap_or(Value::Unit);
                Ok(Value::Unit)
            }
            _ => trap(format!("no method {name} on Ptr")),
        }
    }

    pub fn set_check(&mut self, c: &sspur_check::CheckOutput) {
        self.json_types = c.json_types.clone();
        self.layouts = sspur_native::nval::Layouts::from_check(c);
    }

    #[cfg(feature = "native")]
    pub fn set_native(&mut self, c: sspur_native::Compiled) {
        self.native = Some(c);
    }

    #[cfg(feature = "native")]
    pub fn native_has(&self, name: &str) -> bool {
        self.native.as_ref().is_some_and(|n| n.has(name))
    }

    #[cfg(not(feature = "native"))]
    pub fn native_has(&self, _name: &str) -> bool {
        false
    }

    pub fn call_fn(&self, f: &FnDef, args: Vec<Value>) -> R {
        #[cfg(feature = "native")]
        if let Some(n) = self.native.as_ref().filter(|_| !self.bypass_native.get() && !self.log_handled()) {
            fn emit_hook(ctx: *const (), s: &str) {
                let it = unsafe { &*(ctx as *const Interp) };
                it.emit(s.to_string());
            }
            let raw: Option<Vec<i64>> = args
                .iter()
                .map(|a| match a {
                    Value::Int(i) => Some(*i),
                    Value::Bool(b) => Some(i64::from(*b)),
                    _ => None,
                })
                .collect();
            let scalar = raw.and_then(|raw| {
                sspur_native::set_log_hook(Some((emit_hook, self as *const Interp as *const ())));
                let r = n.call(&f.name, &raw);
                sspur_native::set_log_hook(None);
                r
            });
            if let Some(r) = scalar {
                    self.native_hits.set(self.native_hits.get() + 1);
                    return match r {
                        Ok(v) if n.returns_bool(&f.name) => Ok(Value::Bool(v != 0)),
                        Ok(_) if n.returns_unit(&f.name) => Ok(Value::Unit),
                        Ok(v) => Ok(Value::Int(v)),
                        Err(msg) => trap(msg),
                    };
                }
            if n.has(&f.name)
                && let Some(nargs) = args.iter().map(to_nval).collect::<Option<Vec<_>>>()
            {
                sspur_native::set_log_hook(Some((emit_hook, self as *const Interp as *const ())));
                let r = n.call_rich(&f.name, &nargs);
                sspur_native::set_log_hook(None);
                if let Some(r) = r {
                    self.native_hits.set(self.native_hits.get() + 1);
                    return match r {
                        Ok(v) => Ok(from_nval(v)),
                        Err(sspur_native::NativeError::Trap(msg)) => trap(msg),
                        Err(sspur_native::NativeError::Raise(v)) => Err(Ctrl::Raise(from_nval(v))),
                    };
                }
            }
        }
        let globals = self.globals.clone();
        self.call_fn_in(f, args, &globals)
    }

    #[inline(never)]
    fn launch(&self, f: &FnDef, args: &[Expr], env: &Rc<Env>) -> R {
        let k = match self.kernels.borrow().get(&f.name).cloned() {
            Some(k) => k,
            None => match sspur_check::kernel::lower_with(f, &|n| self.fns.get(n).map(|d| (**d).clone())) {
                Ok(k) => Rc::new(k),
                Err(_) => return trap(format!("kernel {} did not check", f.name)),
            },
        };
        self.kernels.borrow_mut().insert(f.name.clone(), k.clone());
        let mut vals = Vec::with_capacity(args.len());
        let mut cells = Vec::new();
        for (i, (a, p)) in args.iter().zip(&k.params).enumerate() {
            let place = match &a.kind {
                ExprKind::Unary(UnOp::Ref | UnOp::RefMut, x) => &**x,
                _ => a,
            };
            let v = self.eval(place, env)?;
            if p.slice == Some(true)
                && !matches!(v, Value::Dev(_))
                && let ExprKind::Name(n) = &place.kind
                && let Some(c) = env.cell(n)
            {
                cells.push((i, c));
            }
            vals.push(v);
        }
        let all_dev = vals.iter().zip(&k.params).all(|(v, p)| p.slice.is_none() || matches!(v, Value::Dev(_)));
        if !all_dev {
            kernel::sync()?;
        }
        let (mut kargs, devs, n) = match self.launch_args(f, &k, &vals) {
            Ok(x) => x,
            Err(e) => {
                kernel::sync()?;
                return Err(e);
            }
        };
        if all_dev && kernel::pending() {
            return Ok(Value::Unit);
        }
        if let Err(e) = kernel::run(&k, &mut kargs, n) {
            if all_dev {
                kernel::defer(e);
                return Ok(Value::Unit);
            }
            return Err(e);
        }
        for (i, d) in devs {
            if let kernel::Arg::Slice(xs) = &mut kargs[i] {
                *d.data.borrow_mut() = std::mem::take(xs);
            }
        }
        for (i, c) in cells {
            if let kernel::Arg::Slice(xs) = &kargs[i] {
                *c.borrow_mut() = Value::list(xs.iter().map(|x| kernel::from_kv(*x)).collect());
            }
        }
        Ok(Value::Unit)
    }

    #[allow(clippy::type_complexity)]
    fn launch_args(&self, f: &FnDef, k: &sspur_check::kernel::Kernel, vals: &[Value]) -> R<(Vec<kernel::Arg>, Vec<(usize, Rc<kernel::DevCell>)>, (i64, i64))> {
        for (i, (v, p)) in vals.iter().zip(&k.params).enumerate() {
            for (w, q) in vals.iter().zip(&k.params).take(i) {
                if let (Value::Dev(a), Value::Dev(b)) = (v, w)
                    && Rc::ptr_eq(a, b)
                    && (p.slice == Some(true) || q.slice == Some(true))
                {
                    return trap(format!("dev: arguments {} and {} of {} are the same buffer", q.name, p.name, f.name));
                }
            }
        }
        let mut devs = Vec::new();
        let mut kargs = Vec::with_capacity(vals.len());
        for (i, (v, p)) in vals.iter().zip(&k.params).enumerate() {
            kargs.push(match (p.slice, v) {
                (Some(m), Value::Dev(d)) => {
                    if m {
                        devs.push((i, d.clone()));
                    }
                    kernel::Arg::Slice(d.data.borrow().clone())
                }
                (None, v) => kernel::Arg::Scalar(kernel::to_kv(p.ty, v, &|| format!("argument {} of {}", p.name, f.name))?),
                (Some(_), Value::List(xs)) => {
                    let mut out = Vec::with_capacity(xs.len());
                    for (j, x) in xs.iter().enumerate() {
                        out.push(kernel::to_kv(p.ty, x, &|| format!("element {j} of {}", p.name))?);
                    }
                    kernel::Arg::Slice(out)
                }
                (Some(_), v) => return trap(format!("dev: {} expects a list, got {v}", p.name)),
            });
        }
        let n = kernel::prepare(f, k, &mut kargs)?;
        Ok((kargs, devs, n))
    }

    fn call_fn_in(&self, f: &FnDef, args: Vec<Value>, parent: &Rc<Env>) -> R {
        let d = self.depth.get() + 1;
        #[cfg(target_family = "wasm")]
        let too_deep = d > MAX_DEPTH || hop::stack_low();
        #[cfg(not(target_family = "wasm"))]
        let too_deep = too_deep(d);
        if too_deep {
            return trap(format!("stack overflow in {}", f.name));
        }
        if let Some(c) = self.calls.borrow_mut().as_mut() {
            *c.entry(f.name.clone()).or_insert(0) += 1;
        }
        if self.recording.get() {
            let mut r = self.record.borrow_mut();
            if r.0 == f.name && r.1.len() < 400 {
                r.1.push(args.clone());
            }
        }
        self.depth.set(d);
        #[cfg(target_family = "wasm")]
        if hop::due() {
            let (mut args, mut r) = (Some(args), None);
            hop::hop(|| r = args.take().map(|a| self.call_fn_inner(f, a, parent)));
            self.depth.set(d - 1);
            return r.unwrap();
        }
        let r = self.call_fn_inner(f, args, parent);
        self.depth.set(d - 1);
        r
    }

    fn call_fn_inner(&self, f: &FnDef, args: Vec<Value>, parent: &Rc<Env>) -> R {
        if f.ext.is_some() {
            #[cfg(feature = "native")]
            return ffi::call(f, &args);
            #[cfg(not(feature = "native"))]
            return trap(format!("extern fn {}: C interop is not available in the playground", f.name));
        }
        let env = Env::child(parent);
        let r = self.call_body(f, args, &env);
        self.release(&env, r)
    }

    fn call_body(&self, f: &FnDef, args: Vec<Value>, env: &Rc<Env>) -> R {
        let env = env.clone();
        let destructor = self.drops.values().any(|d| *d == f.name);
        for (p, v) in f.params.iter().zip(args) {
            if let Value::Ref(cell) = v {
                env.vars.borrow_mut().insert(p.name.as_str().into(), cell);
                continue;
            }
            if p.refine.is_some() || self.has_alias_refine(&p.ty) {
                self.check_param(f, p, &v)?;
            }
            if destructor {
                env.define(&p.name, v);
            } else {
                self.own_define(&env, &p.name, v);
            }
        }
        if !f.pres.is_empty() {
            self.check_pres(f, &env)?;
        }
        let body = match &f.body.kind {
            ExprKind::Table(rows) => self.eval_table(rows, f, &env),
            _ => self.eval(&f.body, &env),
        };
        let result = match body {
            Ok(v) | Err(Ctrl::Return(v)) => v,
            Err(c) => return Err(c),
        };
        if let Some(rt) = &f.ret
            && self.has_alias_refine(rt)
        {
            self.check_value_type(rt, &result, &format!("result of {}", f.name))?;
        }
        if !f.posts.is_empty() {
            self.check_posts(f, &env, &result)?;
        }
        Ok(result)
    }

    #[inline(never)]
    fn check_pres(&self, f: &FnDef, env: &Rc<Env>) -> R<()> {
        for pre in &f.pres {
            if !self.truthy(pre, env)? {
                let shown: Vec<String> = visit::mentioned_params(&f.params, pre)
                    .into_iter()
                    .filter_map(|i| env.get(&f.params[i].name).map(|v| format!("{} = {}", f.params[i].name, value::Quoted(&v))))
                    .collect();
                let vals = if shown.is_empty() { String::new() } else { format!(" ({})", shown.join(", ")) };
                return trap(format!("contract violated: pre {} in {}{vals}", printer::expr(pre, 0), f.name));
            }
        }
        Ok(())
    }

    #[inline(never)]
    fn check_posts(&self, f: &FnDef, env: &Rc<Env>, result: &Value) -> R<()> {
        let penv = Env::child(env);
        penv.define("r", result.clone());
        for post in &f.posts {
            if !self.truthy(post, &penv)? {
                return trap(format!("contract violated: post {} in {} (r = {})", printer::expr(post, 0), f.name, value::Quoted(result)));
            }
        }
        Ok(())
    }

    fn has_alias_refine(&self, t: &Ty) -> bool {
        !self.alias_refines.is_empty() && ty_name(t).is_some_and(|n| self.alias_refines.contains_key(n))
    }

    #[inline(never)]
    fn check_param(&self, f: &FnDef, p: &Param, v: &Value) -> R<()> {
        self.check_value_type(&p.ty, v, &format!("parameter '{}' of {}", p.name, f.name))?;
        if let Some(r) = &p.refine {
            self.check_refine(r, v, &format!("parameter '{}' of {}", p.name, f.name))?;
        }
        Ok(())
    }

    fn check_value_type(&self, t: &Ty, v: &Value, ctx: &str) -> R<()> {
        if let Some(name) = ty_name(t)
            && let Some(r) = self.alias_refines.get(name) {
                self.check_refine(r, v, &format!("type {name} in {ctx}"))?;
            }
        Ok(())
    }

    fn check_refine(&self, r: &Expr, v: &Value, ctx: &str) -> R<()> {
        let env = Env::child(&self.globals);
        env.define("_", v.clone());
        if self.truthy(r, &env)? {
            Ok(())
        } else {
            trap(format!("contract violated: {} where {} (value = {})", ctx, printer::expr(r, 0), value::Quoted(v)))
        }
    }

    fn truthy(&self, e: &Expr, env: &Rc<Env>) -> R<bool> {
        match self.eval(e, env)? {
            Value::Bool(b) => Ok(b),
            v => trap(format!("expected Bool, got {v}")),
        }
    }

    fn build_record(&self, owner: &str, fields: Vec<(Rc<str>, Value)>) -> R<value::Fields> {
        if let Some(rs) = self.type_refines.get(owner) {
            for r in rs {
                let fname = r.field.as_deref().unwrap();
                let v = &fields.iter().find(|(n, _)| &**n == fname).unwrap().1;
                self.check_refine(&r.expr, v, &format!("field '{fname}' of {owner}"))?;
            }
        }
        for (fname, v) in &fields {
            if let Some(tn) = self.field_types.get(&(owner.to_string(), fname.to_string()))
                && let Some(r) = self.alias_refines.get(tn) {
                    self.check_refine(r, v, &format!("field '{fname}' of {owner}"))?;
                }
        }
        Ok(Rc::new(fields))
    }

    pub fn apply(&self, f: &Value, args: Vec<Value>) -> R {
        match f {
            Value::Closure(c) => {
                let env = Env::child(&c.env);
                for (p, v) in c.params.iter().zip(args) {
                    env.define(p, v);
                }
                self.eval(&c.body, &env)
            }
            Value::Func(d) => self.call_fn(d, args),
            Value::LocalFn(d, env) => self.call_fn_in(d, args, env),
            Value::Builtin(n) => self.call_global(n, args),
            v => trap(format!("cannot call {v}")),
        }
    }

    #[cfg(feature = "native")]
    fn log_handled(&self) -> bool {
        self.handlers.borrow().iter().any(|f| f.handles("log"))
    }

    pub(crate) fn perform(&self, op: &str, args: Vec<Value>) -> R {
        let found = self.handlers.borrow().iter().rposition(|f| f.handles(op));
        let Some(i) = found else {
            if op == "log" {
                self.emit(args[0].to_string());
                return Ok(Value::Unit);
            }
            return trap(format!("unhandled effect operation '{op}'"));
        };
        let tail = self.handlers.borrow_mut().split_off(i);
        let (id, kind) = (tail[0].id, tail[0].kind.clone());
        let r = self.run_clause(&kind, op, args);
        self.handlers.borrow_mut().extend(tail);
        match r {
            Ok(Clause::Resume(v, post)) => {
                if let Some(p) = post {
                    self.handlers.borrow_mut()[i].posts.push(p);
                }
                Ok(v)
            }
            Ok(Clause::Done(v)) => Err(Ctrl::Abort(id, v)),
            Err(c @ (Ctrl::Raise(_) | Ctrl::Return(_))) => Err(Ctrl::Escape(id, Box::new(c))),
            Err(c) => Err(c),
        }
    }

    fn run_clause(&self, kind: &HKind, op: &str, args: Vec<Value>) -> R<Clause> {
        match kind {
            HKind::Loop(pat, body, env) => {
                let (pat, body) = unsafe { (&**pat, &**body) };
                let inner = Env::child(env);
                if self.bind_pat(pat, &args[0], &inner) {
                    self.eval(body, &inner)?;
                }
                Ok(Clause::Resume(Value::Unit, None))
            }
            HKind::Arms(arms, env) => {
                let arms = unsafe { &**arms };
                let Some(arm) = arms.iter().find(|a| matches!(&a.pat, Pat::Ctor { name, .. } if name == op)) else { return trap(format!("no arm for '{op}'")) };
                let inner = Env::child(env);
                if let Pat::Ctor { args: CtorArgs::Positional(ps), .. } = &arm.pat {
                    for (p, v) in ps.iter().zip(&args) {
                        self.bind_pat(p, v, &inner);
                    }
                }
                self.eval_tail(&arm.body, &inner)
            }
        }
    }

    fn resume_value(&self, args: &[Expr], env: &Rc<Env>) -> R {
        match args.first() {
            Some(a) => self.eval(a, env),
            None => Ok(Value::Unit),
        }
    }

    fn eval_tail(&self, e: &Expr, env: &Rc<Env>) -> R<Clause> {
        if let Some(args) = is_resume(e) {
            return Ok(Clause::Resume(self.resume_value(args, env)?, None));
        }
        match &e.kind {
            ExprKind::Block(stmts) => {
                let env = Env::child(env);
                for s in stmts {
                    if let Stmt::Fn(f) = s {
                        env.define(&f.name, Value::LocalFn(Rc::new((**f).clone()), env.clone()));
                    }
                }
                let mut last = Value::Unit;
                let mut frames = vec![env];
                for (i, s) in stmts.iter().enumerate() {
                    let env = frame_for(s, &mut frames);
                    let resumed = match s {
                        Stmt::Expr(x) if i + 1 == stmts.len() => return self.eval_tail(x, &env),
                        Stmt::Expr(x) => is_resume(x).map(|a| (Pat::Wild, a)),
                        Stmt::Let(p, x) => is_resume(x).map(|a| (p.clone(), a)),
                        _ => None,
                    };
                    if let Some((pat, args)) = resumed {
                        let v = self.resume_value(args, &env)?;
                        return Ok(Clause::Resume(v, Some(Post { pat, rest: stmts[i + 1..].to_vec(), env })));
                    }
                    last = self.exec(s, &env)?;
                }
                Ok(Clause::Done(last))
            }
            ExprKind::If(c, t, f) => {
                if self.truthy(c, env)? {
                    self.eval_tail(t, env)
                } else if let Some(f) = f {
                    self.eval_tail(f, env)
                } else {
                    Ok(Clause::Done(Value::Unit))
                }
            }
            ExprKind::Match(s, arms, _) => {
                let v = self.eval(s, env)?;
                for a in arms {
                    let inner = Env::child(env);
                    if !self.bind_pat(&a.pat, &v, &inner) {
                        continue;
                    }
                    if let Some(g) = &a.guard
                        && !self.truthy(g, &inner)? {
                            continue;
                        }
                    return self.eval_tail(&a.body, &inner);
                }
                trap(format!("no match arm for {v}"))
            }
            _ => Ok(Clause::Done(self.eval(e, env)?)),
        }
    }

    fn push_handler(&self, kind: HKind) -> (u64, usize) {
        let id = self.next_handler.get();
        self.next_handler.set(id + 1);
        let mut hs = self.handlers.borrow_mut();
        hs.push(HFrame { id, kind, posts: vec![] });
        (id, hs.len() - 1)
    }

    fn pop_handler(&self, base: usize) -> Vec<Post> {
        let mut hs = self.handlers.borrow_mut();
        hs.truncate(base + 1);
        hs.pop().map(|f| f.posts).unwrap_or_default()
    }

    #[inline(never)]
    fn eval_handle(&self, body: &Expr, arms: &[Arm], env: &Rc<Env>) -> R {
        let (id, base) = self.push_handler(HKind::Arms(arms as *const [Arm], env.clone()));
        let r = self.eval(body, env);
        let posts = self.pop_handler(base);
        let mut x = match r {
            Ok(v) => match arms.iter().find(|a| matches!(&a.pat, Pat::Ctor { name, .. } if name == "return")) {
                Some(Arm { pat: Pat::Ctor { args: CtorArgs::Positional(ps), .. }, body, .. }) => {
                    let inner = Env::child(env);
                    if let Some(p) = ps.first() {
                        self.bind_pat(p, &v, &inner);
                    }
                    self.eval(body, &inner)?
                }
                _ => v,
            },
            Err(Ctrl::Abort(i, v)) if i == id => v,
            Err(Ctrl::Escape(i, c)) if i == id => return Err(*c),
            Err(c) => return Err(c),
        };
        for p in posts.into_iter().rev() {
            let inner = Env::child(&p.env);
            if !self.bind_pat(&p.pat, &x, &inner) {
                return trap(format!("pattern {} did not match {x}", printer::pat(&p.pat)));
            }
            x = Value::Unit;
            let mut frames = vec![inner];
            for s in &p.rest {
                let cur = frame_for(s, &mut frames);
                x = self.exec(s, &cur)?;
            }
        }
        Ok(x)
    }

    fn eval_args(&self, args: &[Expr], env: &Rc<Env>) -> R<Vec<Value>> {
        args.iter().map(|a| self.eval(a, env)).collect()
    }

    fn method(&self, span: Span, name: &str, recv: Value, args: Vec<Value>) -> R {
        if name == "sum" && (self.float_sums.contains(&(span.start, span.end)) || span.start >= synth::SPAN_BASE && self.synth_float.borrow().contains(&(span.start, span.end))) && matches!(&recv, Value::List(xs) if xs.is_empty()) {
            return Ok(Value::Float(0.0));
        }
        if self.user_methods.contains(&(span.start, span.end))
            && let Some(f) = self.fns.get(name).cloned() {
                let mut all = vec![recv];
                all.extend(args);
                return self.call_fn(&f, all);
            }
        if let Value::Ptr(b, off) = recv {
            return self.ptr_method(name, b, off, args);
        }
        self.call_method(name, recv, args)
    }

    #[inline(never)]
    fn is_static(&self, x: &Expr, env: &Rc<Env>) -> bool {
        matches!(&x.kind, ExprKind::Name(n) if self.statics.contains_key(n) && env.cell(n).is_none())
    }

    #[inline(never)]
    fn static_op(&self, x: &Expr, m: &str, args: &[Expr], env: &Rc<Env>) -> R {
        let ExprKind::Name(n) = &x.kind else { unreachable!() };
        let vals = self.eval_args(args, env)?;
        if !self.static_vals.borrow().contains_key(n) {
            let v = self.eval(&self.statics[n], &Env::child(&self.globals))?;
            self.static_vals.borrow_mut().insert(n.clone(), v);
        }
        let mut map = self.static_vals.borrow_mut();
        let cell = map.get_mut(n).unwrap();
        let (slot, rest) = match cell {
            Value::List(xs) => {
                if m == "len" {
                    return Ok(Value::Int(xs.len() as i64));
                }
                let Some(Value::Int(i)) = vals.first() else { return trap("static array access needs an index") };
                let len = xs.len();
                if *i < 0 || *i as usize >= len {
                    return trap(format!("index {i} out of bounds for list of length {len}"));
                }
                (&mut Rc::make_mut(xs)[*i as usize], &vals[1..])
            }
            v => (v, &vals[..]),
        };
        Ok(match (m, rest) {
            ("load", []) => slot.clone(),
            ("store", [v]) => {
                *slot = v.clone();
                Value::Unit
            }
            ("swap", [v]) => std::mem::replace(slot, v.clone()),
            ("add", [Value::Int(d)]) => match slot {
                Value::Int(cur) => {
                    *cur = cur.checked_add(*d).map_or_else(|| trap("integer overflow"), Ok)?;
                    Value::Unit
                }
                _ => return trap("static add needs an Int"),
            },
            ("cas", [old, new]) => {
                let hit = slot == old;
                if hit {
                    *slot = new.clone();
                }
                Value::Bool(hit)
            }
            _ => return trap(format!("unknown static access {n}.{m}")),
        })
    }

    /// Kept small, with the bigger cases in their own functions, because every level of
    /// recursion in the program being run puts a frame of it on the stack. A branch of an `if`
    /// is evaluated in the same frame.
    pub fn eval(&self, e: &Expr, env: &Rc<Env>) -> R {
        let mut e = e;
        loop {
            let fuel = self.fuel.get();
            if fuel == 0 {
                return trap(OUT_OF_FUEL);
            }
            self.fuel.set(fuel - 1);
            return match &e.kind {
                ExprKind::Field(x, f) if self.is_static(x, env) => self.static_op(x, f, &[], env),
                ExprKind::Method { recv, name, args, .. } if self.is_static(recv, env) => self.static_op(recv, name, args, env),
                ExprKind::Method { recv, name, targs, .. } if name == "#array" && targs.len() == 1 => self.eval_array(recv, targs, env),
                ExprKind::Method { recv, name, .. } if name == sspur_syntax::ASCRIBE => {
                    e = recv;
                    continue;
                }
                ExprKind::Int(n) => Ok(Value::Int(*n)),
                ExprKind::Float(x) => Ok(Value::Float(*x)),
                ExprKind::Bool(b) => Ok(Value::Bool(*b)),
                ExprKind::Unit => Ok(Value::Unit),
                ExprKind::Str(parts) => self.eval_str(parts, env),
                ExprKind::Placeholder => env.get("_").map_or_else(|| trap("unbound '_'"), Ok),
                ExprKind::Hole(_) => self.on_hole(e, env),
                ExprKind::Name(n) if has_span(&self.moves, e.span) => match env.cell(n) {
                    Some(c) => Ok(std::mem::replace(&mut *c.borrow_mut(), Value::Unit)),
                    None => self.eval_name(n, env),
                },
                ExprKind::Name(n) => self.eval_name(n, env),
                ExprKind::Field(x, f) if self.sys && has_span(&self.user_methods, e.span) && self.fns.contains_key(f) => self.eval_sys_method(f, &[&**x], env),
                ExprKind::Method { recv, name, args, .. } if self.sys && has_span(&self.user_methods, e.span) && self.fns.contains_key(name) => {
                    let all: Vec<&Expr> = std::iter::once(&**recv).chain(args.iter()).collect();
                    self.eval_sys_method(name, &all, env)
                }
                ExprKind::Unary(UnOp::Ref | UnOp::RefMut, x) => {
                    e = x;
                    continue;
                }
                ExprKind::Field(x, f) => self.eval_field(e.span, x, f, env),
                ExprKind::Method { recv, name, args, .. } if matches!(&recv.kind, ExprKind::Name(n) if n == "json") && env.cell("json").is_none() && !self.fns.contains_key("json") => self.json_op(e.span, name, args, env),
                ExprKind::Method { name, targs, .. } if name == "mmio" && !targs.is_empty() => trap("mmio needs a bare target: build with 'sspur build --target riscv64-qemu|aarch64-qemu'"),
                ExprKind::Method { recv, name, args, .. } if name == "asm" && args.len() == 3 && matches!(recv.kind, ExprKind::Str(_)) => trap("inline asm needs native code: it runs in compiled sys functions and on bare targets, not in the interpreter"),
                ExprKind::Method { recv, name, args, .. } => self.eval_method(e.span, recv, name, args, env),
                ExprKind::Call(f, args) => self.eval_call(f, args, env),
                ExprKind::Index(a, i) => self.eval_index(a, i, env),
                ExprKind::Lambda { params, body, .. } => Ok(closure(params, body, env)),
                ExprKind::Binary(op, l, r) => self.eval_binary(*op, l, r, env),
                ExprKind::Unary(UnOp::Neg, x) => self.eval_neg(x, env),
                ExprKind::Unary(UnOp::Not, x) => Ok(Value::Bool(!self.truthy(x, env)?)),
                ExprKind::Range(a, b) => self.eval_range(a, b, env),
                ExprKind::If(c, t, f) => {
                    if self.truthy(c, env)? {
                        e = t;
                        continue;
                    } else if let Some(f) = f {
                        e = f;
                        continue;
                    } else {
                        Ok(Value::Unit)
                    }
                }
                ExprKind::Match(s, arms, _) => self.eval_match(s, arms, env),
                ExprKind::Catch(body, arms) => self.eval_catch(body, arms, env),
                ExprKind::Handle(body, arms) => self.eval_handle(body, arms, env),
                ExprKind::Block(stmts) => self.eval_block(stmts, env),
                ExprKind::Record { ctor, fields } => self.eval_record(e.span, ctor, fields, env),
                ExprKind::List(xs) => Ok(Value::list(self.eval_args(xs, env)?)),
                ExprKind::Tuple(xs) => Ok(Value::Tuple(Rc::new(self.eval_args(xs, env)?))),
                ExprKind::Par(xs) => self.eval_par(xs, env),
                ExprKind::Raise(x) => Err(Ctrl::Raise(self.eval(x, env)?)),
                ExprKind::Return(x) => Err(Ctrl::Return(self.eval(x, env)?)),
                ExprKind::Table(_) => trap("a rule table can only be the body of a rule"),
                ExprKind::With(base, ups) => self.eval_with(base, ups, env),
            };
        }
    }

    #[inline(never)]
    fn eval_array(&self, recv: &Expr, targs: &[Ty], env: &Rc<Env>) -> R {
        let v = self.eval(recv, env)?;
        let n = match &targs[0] {
            Ty::Named { name, .. } => name.parse::<usize>().unwrap_or(0),
            _ => 0,
        };
        Ok(Value::List(Rc::new(vec![v; n])))
    }

    #[inline(never)]
    fn eval_str(&self, parts: &[StrPart], env: &Rc<Env>) -> R {
        let mut s = String::new();
        for p in parts {
            match p {
                StrPart::Lit(l) => s.push_str(l),
                StrPart::Expr(x) => s.push_str(&self.eval(x, env)?.to_string()),
            }
        }
        Ok(Value::str(&s))
    }

    #[inline(never)]
    fn eval_sys_method(&self, name: &str, args: &[&Expr], env: &Rc<Env>) -> R {
        let fd = self.fns[name].clone();
        let a = self.arg_values(&fd, args, env)?;
        self.call_fn(&fd, a)
    }

    #[inline(never)]
    fn eval_field(&self, span: Span, x: &Expr, f: &str, env: &Rc<Env>) -> R {
        let v = self.eval(x, env)?;
        if let Some(fv) = v.field(f) {
            return Ok(fv);
        }
        if let (Value::Tuple(items), Ok(i)) = (&v, f.parse::<usize>()) {
            return items.get(i).cloned().map_or_else(|| trap("tuple index out of range"), Ok);
        }
        if let (Value::New(_, inner), "raw") = (&v, f) {
            return Ok((**inner).clone());
        }
        self.method(span, f, v, vec![])
    }

    #[inline(never)]
    fn eval_method(&self, span: Span, recv: &Expr, name: &str, args: &[Expr], env: &Rc<Env>) -> R {
        let r = self.eval(recv, env)?;
        let a = self.eval_args(args, env)?;
        #[cfg(not(target_family = "wasm"))]
        if let ExprKind::Name(n) = &recv.kind
            && lastuse::MOVE_METHODS.contains(&name)
            && self.is_last_use(recv.span)
        {
            release(n, &r, env);
        }
        self.method(span, name, r, a)
    }

    /// Whether `span` is a variable's last use as the receiver of an appending method.
    #[cfg(not(target_family = "wasm"))]
    #[inline(never)]
    fn is_last_use(&self, span: Span) -> bool {
        if self.last_uses.borrow().is_none() {
            let mut lu = lastuse::LastUse::default();
            for f in self.fns.values() {
                lu.def(f);
            }
            for t in &self.tests {
                lu.func(&[], &t.body, true);
            }
            for e in self.statics.values() {
                lu.func(&[], e, true);
            }
            *self.last_uses.borrow_mut() = Some(lu.finish());
        }
        self.last_uses.borrow().as_ref().is_some_and(|s| s.contains(&(span.start, span.end)))
    }

    #[inline(never)]
    fn eval_call(&self, f: &Expr, args: &[Expr], env: &Rc<Env>) -> R {
        if let ExprKind::Name(n) = &f.kind
            && env.cell(n).is_none()
        {
            if self.newtypes.contains(n) {
                let v = self.eval(&args[0], env)?;
                return Ok(Value::New(n.as_str().into(), Rc::new(v)));
            }
            if let Some(fd) = self.fns.get(n).cloned() {
                if fd.kernel.is_some() {
                    return self.launch(&fd, args, env);
                }
                let a = if self.sys { self.arg_values(&fd, &args.iter().collect::<Vec<_>>(), env)? } else { self.eval_args(args, env)? };
                return self.call_fn(&fd, a);
            }
            return self.eval_call_named(n, f, args, env);
        }
        let fv = self.eval(f, env)?;
        let a = self.eval_args(args, env)?;
        self.apply(&fv, a)
    }

    #[inline(never)]
    fn eval_call_named(&self, n: &str, f: &Expr, args: &[Expr], env: &Rc<Env>) -> R {
        if self.bare && BARE_NAMES.contains(&n) {
            return if n == "arch" { Ok(Value::str("host")) } else { trap(format!("'{n}' needs a bare target: build with 'sspur build --target riscv64-qemu|aarch64-qemu'")) };
        }
        if self.sys && matches!(n, "drop" | "leak" | "alloc" | "free" | "null") {
            let a = self.eval_args(args, env)?;
            return self.sys_call(n, a);
        }
        if self.ops.contains(n) {
            let a = self.eval_args(args, env)?;
            return self.perform(n, a);
        }
        if builtins::is_global(n) {
            let a = self.eval_args(args, env)?;
            return self.call_global(n, a);
        }
        let fv = self.eval(f, env)?;
        let a = self.eval_args(args, env)?;
        self.apply(&fv, a)
    }

    #[inline(never)]
    fn eval_index(&self, a: &Expr, i: &Expr, env: &Rc<Env>) -> R {
        let av = self.eval(a, env)?;
        let iv = self.eval(i, env)?;
        match (av, iv) {
            (Value::List(xs), Value::Int(i)) => {
                if i < 0 || i as usize >= xs.len() {
                    trap(format!("index {i} out of bounds for list of length {}", xs.len()))
                } else {
                    Ok(xs[i as usize].clone())
                }
            }
            (a, _) => trap(format!("cannot index {a}")),
        }
    }

    #[inline(never)]
    fn eval_neg(&self, x: &Expr, env: &Rc<Env>) -> R {
        match self.eval(x, env)? {
            Value::Int(n) => n.checked_neg().map(Value::Int).map_or_else(|| trap("integer overflow"), Ok),
            Value::Float(f) => Ok(Value::Float(-f)),
            v => trap(format!("cannot negate {v}")),
        }
    }

    #[inline(never)]
    fn eval_range(&self, a: &Expr, b: &Expr, env: &Rc<Env>) -> R {
        match (self.eval(a, env)?, self.eval(b, env)?) {
            (Value::Int(a), Value::Int(b)) if b as i128 - a as i128 > 1 << 26 => trap("out of memory"),
            (Value::Int(a), Value::Int(b)) => Ok(Value::list((a..b).map(Value::Int).collect())),
            _ => trap("range bounds must be Int"),
        }
    }

    #[inline(never)]
    fn eval_match(&self, s: &Expr, arms: &[Arm], env: &Rc<Env>) -> R {
        let v = self.eval(s, env)?;
        match self.match_arms(&v, arms, env)? {
            Some(r) => Ok(r),
            None => trap(format!("no match arm for {v}")),
        }
    }

    /// Whether a `catch` with these arms handles `v`: one whose arms name constructors handles
    /// only that sum type, as in native code, so a `_` arm there doesn't catch other errors.
    fn catches(&self, arms: &[Arm], v: &Value) -> bool {
        fn ctor(p: &Pat) -> Option<&str> {
            match p {
                Pat::Ctor { name, .. } => Some(name),
                Pat::Or(ps) => ps.iter().find_map(ctor),
                _ => None,
            }
        }
        let Some(want) = arms.iter().find_map(|a| ctor(&a.pat)).and_then(|c| self.type_of_ctor(c)) else { return true };
        matches!(v, Value::Variant(n, _) if self.type_of_ctor(n) == Some(want))
    }

    #[inline(never)]
    fn eval_catch(&self, body: &Expr, arms: &[Arm], env: &Rc<Env>) -> R {
        match self.eval(body, env) {
            Err(Ctrl::Raise(v)) if !self.catches(arms, &v) => Err(Ctrl::Raise(v)),
            Err(Ctrl::Raise(v)) => match self.match_arms(&v, arms, env)? {
                Some(r) => Ok(r),
                None => Err(Ctrl::Raise(v)),
            },
            other => other,
        }
    }

    #[inline(never)]
    fn eval_block(&self, stmts: &[Stmt], env: &Rc<Env>) -> R {
        let env = Env::child(env);
        define_local_fns(stmts, &env);
        let mut last = Value::Unit;
        let mut frames = vec![env];
        let mut r = Ok(());
        for s in stmts {
            let cur = frame_for(s, &mut frames);
            match self.exec(s, &cur) {
                Ok(v) => last = v,
                Err(c) => {
                    r = Err(c);
                    break;
                }
            }
        }
        for f in frames.iter().rev() {
            r = self.release(f, r);
        }
        r.map(|_| last)
    }

    #[inline(never)]
    fn eval_record(&self, span: Span, ctor: &Option<String>, fields: &[(String, Expr)], env: &Rc<Env>) -> R {
        let owner = match ctor {
            Some(c) => self.record_types.get(&(span.start, span.end)).filter(|r| r.starts_with('#')).cloned().unwrap_or_else(|| c.clone()),
            None => match self.record_types.get(&(span.start, span.end)) {
                Some(n) => n.clone(),
                None => return trap("untyped record literal"),
            },
        };
        let order: Vec<String> = match self.variant_fields.get(&owner) {
            Some(Some(fs)) => fs.clone(),
            _ => self.record_fields.get(&owner).cloned().unwrap_or_else(|| fields.iter().map(|(n, _)| n.clone()).collect()),
        };
        let mut vals = Vec::with_capacity(order.len());
        let mut given = HashMap::new();
        for (n, x) in fields {
            given.insert(n.as_str(), self.eval(x, env)?);
        }
        for n in &order {
            let v = match given.remove(n.as_str()) {
                Some(v) => v,
                #[cfg(not(target_family = "wasm"))]
                None if sspur_check::std_field_default(&owner, n) && n == "data" => Value::bytes(vec![]),
                None if sspur_check::std_field_default(&owner, n) => Value::str(""),
                None => return trap(format!("missing field {n}")),
            };
            vals.push((Rc::<str>::from(n.as_str()), v));
        }
        let fs = self.build_record(&owner, vals)?;
        if self.variant_fields.contains_key(&owner) {
            Ok(Value::Variant(owner.as_str().into(), Some(fs)))
        } else {
            Ok(Value::Record(owner.as_str().into(), fs))
        }
    }

    #[inline(never)]
    fn eval_par(&self, xs: &[Expr], env: &Rc<Env>) -> R {
        Ok(Value::Tuple(Rc::new(self.run_tasks(xs.len(), &|k| self.eval(&xs[k], &Env::child(env)))?)))
    }

    #[inline(never)]
    fn eval_with(&self, base: &Expr, ups: &[(Vec<PathSeg>, Expr)], env: &Rc<Env>) -> R {
        let mut v = self.eval(base, env)?;
        for (path, value) in ups {
            let mut keys = Vec::with_capacity(path.len());
            for seg in path {
                keys.push(match seg {
                    PathSeg::Field(f) => Ok(f.as_str()),
                    PathSeg::Index(i) => match self.eval(i, env)? {
                        Value::Int(n) => Err(n),
                        other => return trap(format!("index must be Int, got {other}")),
                    },
                });
            }
            let nv = self.eval(value, env)?;
            v = self.set_path(&v, &keys, nv)?;
        }
        Ok(v)
    }

    fn eval_table(&self, rows: &[TableRow], f: &FnDef, env: &Rc<Env>) -> R {
        'rows: for r in rows {
            let inner = Env::child(env);
            for (c, p) in r.cells.iter().zip(&f.params) {
                let ok = match c {
                    sspur_syntax::Cell::Any => true,
                    sspur_syntax::Cell::Pat(pat) => self.bind_pat(pat, &env.get(&p.name).unwrap_or(Value::Unit), &inner),
                    sspur_syntax::Cell::Cond(e) => self.truthy(e, &inner)?,
                };
                if !ok {
                    continue 'rows;
                }
            }
            return self.eval(&r.out, &inner);
        }
        let args: Vec<String> = f.params.iter().map(|p| format!("{} = {}", p.name, env.get(&p.name).map(|v| value::Quoted(&v).to_string()).unwrap_or_default())).collect();
        trap(format!("no row of rule {} matched ({})", f.name, args.join(", ")))
    }

    fn set_path(&self, v: &Value, keys: &[Result<&str, i64>], nv: Value) -> R {
        let Some((k, rest)) = keys.split_first() else { return Ok(nv) };
        match (k, v) {
            (Ok(f), Value::Record(n, fs)) => {
                let mut out = Vec::with_capacity(fs.len());
                for (name, fv) in fs.iter() {
                    let nfv = if &**name == *f { self.set_path(fv, rest, nv.clone())? } else { fv.clone() };
                    out.push((name.clone(), nfv));
                }
                Ok(Value::Record(n.clone(), self.build_record(n, out)?))
            }
            (Err(i), Value::List(xs)) => {
                if *i < 0 || *i as usize >= xs.len() {
                    return trap(format!("index {i} out of bounds for list of length {}", xs.len()));
                }
                let mut ys = (**xs).clone();
                ys[*i as usize] = self.set_path(&xs[*i as usize], rest, nv)?;
                Ok(Value::list(ys))
            }
            (_, v) => trap(format!("cannot update a path through {v}")),
        }
    }

    #[inline(never)]
    fn eval_name(&self, n: &str, env: &Rc<Env>) -> R {
        if let Some(v) = env.get(n) {
            return Ok(v);
        }
        if n == "none" {
            return Ok(Value::Opt(None));
        }
        if let Some(None) = self.variant_fields.get(n) {
            return Ok(Value::Variant(n.into(), None));
        }
        if let Some(f) = self.fns.get(n) {
            return Ok(Value::Func(f.clone()));
        }
        if builtins::is_global(n) || self.ops.contains(n) {
            return Ok(Value::Builtin(n.into()));
        }
        trap(format!("unbound name '{n}'"))
    }

    fn exec(&self, s: &Stmt, env: &Rc<Env>) -> R {
        match s {
            Stmt::Expr(e) => self.eval(e, env),
            Stmt::Let(p, e) => {
                let v = self.eval(e, env)?;
                if !self.bind_pat(p, &v, env) {
                    return no_match(p, &v);
                }
                Ok(Value::Unit)
            }
            Stmt::Var(n, e) => {
                let v = self.eval(e, env)?;
                self.own_define(env, n, v);
                Ok(Value::Unit)
            }
            Stmt::Assign(n, e, span) => self.exec_assign(n, e, *span, env),
            Stmt::Fn(_) => Ok(Value::Unit),
            _ => self.exec_loop(s, env),
        }
    }

    #[inline(never)]
    fn exec_assign(&self, n: &str, e: &Expr, span: Span, env: &Rc<Env>) -> R {
        let v = self.eval(e, env)?;
        let Some(c) = env.cell(n) else { return trap(format!("unbound name '{n}'")) };
        let old = std::mem::replace(&mut *c.borrow_mut(), v);
        if self.droppable(&old) && !self.inplace.contains(&(span.start, span.end)) {
            self.run_drop(old)?;
        }
        Ok(Value::Unit)
    }

    #[inline(never)]
    fn exec_loop(&self, s: &Stmt, env: &Rc<Env>) -> R {
        match s {
            Stmt::While(c, body) => {
                while self.truthy(c, env)? {
                    self.eval(body, &Env::child(env))?;
                }
                Ok(Value::Unit)
            }
            Stmt::For(p, it, body) if self.gen_loops.contains(&(it.span.start, it.span.end)) => {
                let (id, base) = self.push_handler(HKind::Loop(p as *const Pat, body as *const Expr, env.clone()));
                let r = self.eval(it, env);
                self.pop_handler(base);
                match r {
                    Ok(_) => Ok(Value::Unit),
                    Err(Ctrl::Abort(i, _)) if i == id => Ok(Value::Unit),
                    Err(Ctrl::Escape(i, c)) if i == id => Err(*c),
                    Err(c) => Err(c),
                }
            }
            Stmt::For(p, it, body) if matches!(&it.kind, ExprKind::Par(xs) if xs.len() == 1) => {
                let ExprKind::Par(src) = &it.kind else { unreachable!() };
                let Value::List(xs) = self.eval(&src[0], env)? else { return trap("for expects a List") };
                self.run_tasks(xs.len(), &|k| {
                    let inner = Env::child(env);
                    if self.bind_pat(p, &xs[k], &inner) {
                        self.eval(body, &inner)?;
                    }
                    Ok(Value::Unit)
                })?;
                Ok(Value::Unit)
            }
            Stmt::For(p, it, body) => {
                if let Some((s, e, k)) = self.lazy_range(it, env)? {
                    let n = if k > 0 && e > s {
                        (e as i128 - s as i128 - 1) / k as i128 + 1
                    } else if k < 0 && e < s {
                        (s as i128 - e as i128 - 1) / -(k as i128) + 1
                    } else {
                        0
                    };
                    for c in 0..n {
                        let inner = Env::child(env);
                        if self.bind_pat(p, &Value::Int((s as i128 + c * k as i128) as i64), &inner) {
                            self.eval(body, &inner)?;
                        }
                    }
                    return Ok(Value::Unit);
                }
                let xs = match self.eval(it, env)? {
                    Value::List(xs) => xs,
                    Value::View(n) => {
                        self.for_view(&n, |x| {
                            let inner = Env::child(env);
                            if self.bind_pat(p, &x, &inner) {
                                self.eval(body, &inner)?;
                            }
                            Ok(())
                        })?;
                        return Ok(Value::Unit);
                    }
                    Value::Chan(c) => {
                        while let Some(x) = self.chan_recv(&c)? {
                            let inner = Env::child(env);
                            if self.bind_pat(p, &x, &inner) {
                                let r = self.eval(body, &inner);
                                self.release(&inner, r)?;
                            }
                        }
                        return Ok(Value::Unit);
                    }
                    _ => return trap("for expects a List"),
                };
                for x in xs.iter() {
                    let inner = Env::child(env);
                    if self.bind_pat(p, x, &inner) {
                        self.eval(body, &inner)?;
                    }
                }
                Ok(Value::Unit)
            }
            _ => unreachable!(),
        }
    }

    fn lazy_range(&self, it: &Expr, env: &Rc<Env>) -> R<Option<(i64, i64, i64)>> {
        let int = |v: Value| match v {
            Value::Int(n) => Ok(n),
            _ => trap("range bounds must be Int"),
        };
        match &it.kind {
            ExprKind::Range(a, b) => Ok(Some((int(self.eval(a, env)?)?, int(self.eval(b, env)?)?, 1))),
            ExprKind::Call(f, args) if args.len() == 3 && matches!(&f.kind, ExprKind::Name(n) if n == "range" && !self.fns.contains_key(n) && env.get(n).is_none()) => {
                let (s, e, k) = (int(self.eval(&args[0], env)?)?, int(self.eval(&args[1], env)?)?, int(self.eval(&args[2], env)?)?);
                if k == 0 {
                    return trap("range step must not be 0");
                }
                Ok(Some((s, e, k)))
            }
            _ => Ok(None),
        }
    }

    fn match_arms(&self, v: &Value, arms: &[Arm], env: &Rc<Env>) -> R<Option<Value>> {
        for a in arms {
            let inner = Env::child(env);
            if !self.bind_pat(&a.pat, v, &inner) {
                continue;
            }
            if let Some(g) = &a.guard
                && !self.truthy(g, &inner)? {
                    continue;
                }
            return self.eval(&a.body, &inner).map(Some);
        }
        Ok(None)
    }

    fn bind_pat(&self, p: &Pat, v: &Value, env: &Rc<Env>) -> bool {
        match (p, v) {
            (Pat::Wild, _) => true,
            (Pat::Bind(n), _) => {
                self.own_define(env, n, v.clone());
                true
            }
            (Pat::Int(a), Value::Int(b)) => a == b,
            (Pat::Str(a), Value::Str(b)) => a.as_str() == &**b,
            (Pat::Bool(a), Value::Bool(b)) => a == b,
            (Pat::Tuple(ps), Value::Tuple(vs)) => ps.len() == vs.len() && ps.iter().zip(vs.iter()).all(|(p, v)| self.bind_pat(p, v, env)),
            (Pat::Or(alts), _) => alts.iter().any(|a| self.bind_pat(a, v, env)),
            (Pat::List { head, rest, tail }, Value::List(xs)) => {
                let (h, t, n) = (head.len(), tail.len(), xs.len());
                let fits = if rest.is_some() { n >= h + t } else { n == h };
                if !(fits && head.iter().zip(xs.iter()).all(|(p, x)| self.bind_pat(p, x, env)) && tail.iter().zip(&xs[n - t..]).all(|(p, x)| self.bind_pat(p, x, env))) {
                    return false;
                }
                if let Some(Some(r)) = rest {
                    let mid = if h == 0 && t == 0 { v.clone() } else { Value::List(Rc::new(xs[h..n - t].to_vec())) };
                    self.own_define(env, r, mid);
                }
                true
            }
            (Pat::Ctor { name, args }, _) => match (name.as_str(), args, v) {
                ("some", CtorArgs::Positional(ps), Value::Opt(Some(x))) => self.bind_pat(&ps[0], x, env),
                ("none", _, Value::Opt(None)) => true,
                ("ok", CtorArgs::Positional(ps), Value::Res(Ok(x))) => self.bind_pat(&ps[0], x, env),
                ("err", CtorArgs::Positional(ps), Value::Res(Err(x))) => self.bind_pat(&ps[0], x, env),
                (_, _, Value::Variant(n, _) | Value::Record(n, _)) if &**n != name.as_str() => false,
                (_, CtorArgs::None, Value::Variant(..) | Value::Record(..)) => true,
                (_, CtorArgs::Record(pfs), Value::Variant(_, Some(_)) | Value::Record(..)) => pfs.iter().all(|(f, sp)| v.field(f).is_some_and(|fv| self.bind_pat(sp, &fv, env))),
                _ => false,
            },
            _ => false,
        }
    }

    #[inline(never)]
    fn eval_binary(&self, op: BinOp, l: &Expr, r: &Expr, env: &Rc<Env>) -> R {
        use BinOp::*;
        if op == And {
            return Ok(Value::Bool(self.truthy(l, env)? && self.truthy(r, env)?));
        }
        if op == Or {
            return Ok(Value::Bool(self.truthy(l, env)? || self.truthy(r, env)?));
        }
        let a = self.eval(l, env)?;
        let b = self.eval(r, env)?;
        Ok(match op {
            Eq => Value::Bool(a == b),
            Ne => Value::Bool(a != b),
            Lt => Value::Bool(a < b),
            Le => Value::Bool(a <= b),
            Gt => Value::Bool(a > b),
            Ge => Value::Bool(a >= b),
            _ => return arith(op, a, b),
        })
    }
}

fn arith(op: BinOp, a: Value, b: Value) -> R {
    use BinOp::*;
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            let r = match op {
                Add => x.checked_add(y),
                Sub => x.checked_sub(y),
                Mul => x.checked_mul(y),
                Div if y == 0 => return trap("division by zero"),
                Div => x.checked_div(y),
                Rem if y == 0 => return trap("division by zero"),
                Rem => x.checked_rem(y),
                Pow if y < 0 => return trap("negative exponent"),
                Pow => u32::try_from(y).ok().and_then(|e| x.checked_pow(e)),
                _ => unreachable!(),
            };
            r.map(Value::Int).map_or_else(|| trap("integer overflow"), Ok)
        }
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(match op {
            Add => x + y,
            Sub => x - y,
            Mul => x * y,
            Div => x / y,
            Rem => x % y,
            Pow => x.powf(y),
            _ => unreachable!(),
        })),
        (Value::Str(x), Value::Str(y)) if op == Add => Ok(Value::str(&format!("{x}{y}"))),
        (Value::List(x), Value::List(y)) if op == Add => Ok(Value::list(x.iter().chain(y.iter()).cloned().collect())),
        (a, b) => trap(format!("operator '{}' cannot apply to {a} and {b}", op.symbol())),
    }
}

fn nfields(fs: &value::Fields) -> Option<Vec<(String, sspur_native::nval::NVal)>> {
    fs.iter().map(|(n, v)| Some((n.to_string(), to_nval(v)?))).collect()
}

pub fn to_nval(v: &Value) -> Option<sspur_native::nval::NVal> {
    use sspur_native::nval::NVal;
    Some(match v {
        Value::Unit => NVal::Unit,
        Value::Int(n) => NVal::Int(*n),
        Value::Bool(b) => NVal::Bool(*b),
        Value::Float(x) => NVal::Float(*x),
        Value::Str(s) => NVal::Str(s.to_string()),
        #[cfg(not(target_family = "wasm"))]
        Value::Buf(s) => NVal::Str(s.to_string()),
        Value::New(n, x) => NVal::New(n.to_string(), Box::new(to_nval(x)?)),
        Value::Wrap(k, x) => NVal::Wrap(k.to_string(), Box::new(to_nval(x)?)),
        Value::Guess(x, c) => NVal::Guess(Box::new(to_nval(x)?), *c),
        Value::Record(n, fs) => NVal::Rec(n.to_string(), nfields(fs)?),
        Value::Variant(n, fs) => NVal::Variant(
            n.to_string(),
            match fs {
                Some(fs) => Some(nfields(fs)?),
                None => None,
            },
        ),
        Value::List(xs) => NVal::List(xs.iter().map(to_nval).collect::<Option<_>>()?),
        Value::Map(m) => NVal::Map(m.iter().map(|(k, v)| Some((to_nval(k)?, to_nval(v)?))).collect::<Option<_>>()?),
        Value::Tuple(xs) => NVal::Tuple(xs.iter().map(to_nval).collect::<Option<_>>()?),
        Value::Set(s) => NVal::Set(s.iter().map(to_nval).collect::<Option<_>>()?),
        Value::Heap(xs) => NVal::Heap(xs.iter().map(to_nval).collect::<Option<_>>()?),
        Value::Res(r) => NVal::Res(match r {
            Ok(x) => Ok(Box::new(to_nval(x)?)),
            Err(e) => Err(Box::new(to_nval(e)?)),
        }),
        Value::Opt(o) => NVal::Opt(match o {
            Some(x) => Some(Box::new(to_nval(x)?)),
            None => None,
        }),
        Value::Time(t) => NVal::Time(*t),
        Value::Dur(d) => NVal::Dur(*d),
        Value::Bits(n, w) => NVal::Bits(*n, (**w).clone()),
        #[cfg(not(target_family = "wasm"))]
        Value::Bytes(b) => NVal::Bytes(b.0.clone()),
        Value::Big(b) => NVal::Big((**b).clone()),
        Value::Regex(r) => NVal::Regex(r.src.clone()),
        Value::Dec(m, s) => NVal::Dec((**m).clone(), *s),
        _ => return None,
    })
}

pub fn from_nval(v: sspur_native::nval::NVal) -> Value {
    use sspur_native::nval::NVal;
    let fields = |fs: Vec<(String, NVal)>| Rc::new(fs.into_iter().map(|(n, v)| (Rc::<str>::from(n.as_str()), from_nval(v))).collect::<Vec<_>>());
    match v {
        NVal::Unit => Value::Unit,
        NVal::Int(n) => Value::Int(n),
        NVal::Bool(b) => Value::Bool(b),
        NVal::Float(x) => Value::Float(x),
        NVal::Str(s) => Value::str(&s),
        NVal::New(n, x) => Value::New(n.as_str().into(), Rc::new(from_nval(*x))),
        NVal::Wrap(k, x) => Value::Wrap(k.as_str().into(), Rc::new(from_nval(*x))),
        NVal::Guess(x, c) => Value::Guess(Rc::new(from_nval(*x)), c),
        NVal::Rec(n, fs) => Value::Record(n.as_str().into(), fields(fs)),
        NVal::Variant(n, fs) => Value::Variant(n.as_str().into(), fs.map(fields)),
        NVal::List(xs) => Value::list(xs.into_iter().map(from_nval).collect()),
        NVal::Map(kv) => Value::Map(Rc::new(kv.into_iter().map(|(k, v)| (from_nval(k), from_nval(v))).collect())),
        NVal::Tuple(xs) => Value::Tuple(Rc::new(xs.into_iter().map(from_nval).collect())),
        NVal::Opt(o) => Value::Opt(o.map(|x| Rc::new(from_nval(*x)))),
        NVal::Set(xs) => Value::Set(Rc::new(xs.into_iter().map(from_nval).collect())),
        NVal::Heap(xs) => {
            let mut v: Vec<Value> = xs.into_iter().map(from_nval).collect();
            v.sort();
            Value::Heap(Rc::new(v))
        }
        NVal::Res(r) => Value::Res(match r {
            Ok(x) => Ok(Rc::new(from_nval(*x))),
            Err(e) => Err(Rc::new(from_nval(*e))),
        }),
        NVal::Time(t) => Value::Time(t),
        NVal::Dur(d) => Value::Dur(d),
        NVal::Bits(n, w) => Value::Bits(n, Rc::new(w)),
        #[cfg(not(target_family = "wasm"))]
        NVal::Bytes(b) => Value::bytes(b),
        #[cfg(target_family = "wasm")]
        NVal::Bytes(_) => Value::Unit,
        NVal::Big(b) => Value::Big(Rc::new(b)),
        NVal::Regex(s) => match stdre::compile(&s) {
            Ok(r) => Value::Regex(Rc::new(r)),
            Err(_) => Value::Unit,
        },
        NVal::Dec(m, s) => Value::Dec(Rc::new(m), s),
    }
}

pub fn describe(c: Ctrl) -> String {
    match c {
        Ctrl::Trap(m) => m,
        Ctrl::Raise(v) => format!("unhandled error: {v}"),
        Ctrl::Return(_) => "return outside of a function".into(),
        Ctrl::Abort(..) => "effect handler result escaped its handle".into(),
        Ctrl::Escape(_, c) => describe(*c),
    }
}

/// A value shown in a failing test's explanation, cut to a size an agent can read.
fn clip(s: String) -> String {
    const MAX: usize = 300;
    if s.len() <= MAX {
        return s;
    }
    let mut end = MAX;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}
