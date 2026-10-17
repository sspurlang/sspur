use sspur_syntax::{Expr, FnDef};
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::rc::Rc;

pub type Fields = Rc<Vec<(Rc<str>, Value)>>;

#[derive(Clone)]
pub enum Value {
    Unit,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    /// A `StrBuf`: compares, shows and converts as the `Str` it holds.
    Buf(Rc<String>),
    List(Rc<Vec<Value>>),
    Tuple(Rc<Vec<Value>>),
    Record(Rc<str>, Fields),
    Variant(Rc<str>, Option<Fields>),
    Opt(Option<Rc<Value>>),
    Res(Result<Rc<Value>, Rc<Value>>),
    Map(Rc<BTreeMap<Value, Value>>),
    Set(Rc<BTreeSet<Value>>),
    Heap(Rc<Vec<Value>>),
    New(Rc<str>, Rc<Value>),
    Closure(Rc<Closure>),
    Func(Rc<FnDef>),
    LocalFn(Rc<FnDef>, Rc<Env>),
    Wrap(Rc<str>, Rc<Value>),
    Guess(Rc<Value>, f64),
    Builtin(Rc<str>),
    Ptr(u32, i64),
    Ref(Rc<RefCell<Value>>),
    Atomic(Rc<AtomCell>),
    Chan(Rc<ChanCell>),
    Time(i64),
    Dur(i64),
    Bits(i64, Rc<Vec<u64>>),
    #[cfg(not(target_family = "wasm"))]
    Bytes(Rc<ByteBuf>),
    Big(Rc<sspur_native::bigint::Big>),
    Dec(Rc<sspur_native::bigint::Big>, i64),
    Regex(Rc<crate::stdre::Re>),
    Dev(Rc<crate::kernel::DevCell>),
    View(Rc<crate::stdview::Node>),
}

#[cfg(not(target_family = "wasm"))]
/// The contents of a `Bytes` value. They are wiped when the last reference goes, so key material
/// (a `Secret[Bytes]`) doesn't linger in freed memory.
#[derive(Default)]
pub struct ByteBuf(pub Vec<u8>);

#[cfg(not(target_family = "wasm"))]
impl Drop for ByteBuf {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

impl Value {
    #[cfg(not(target_family = "wasm"))]
    pub fn bytes(b: Vec<u8>) -> Value {
        Value::Bytes(Rc::new(ByteBuf(b)))
    }
}

pub struct AtomCell {
    pub id: u64,
    pub v: Cell<i64>,
}

pub struct ChanCell {
    pub id: u64,
    pub q: RefCell<VecDeque<Value>>,
    pub closed: Cell<bool>,
    pub waiters: RefCell<VecDeque<usize>>,
}

pub struct Closure {
    pub params: Vec<String>,
    pub body: Expr,
    pub env: Rc<Env>,
}

#[derive(Default)]
pub struct Env {
    pub vars: RefCell<HashMap<Rc<str>, Rc<RefCell<Value>>>>,
    pub parent: Option<Rc<Env>>,
    pub owned: RefCell<Vec<Rc<str>>>,
}

impl Env {
    pub fn child(parent: &Rc<Env>) -> Rc<Env> {
        Rc::new(Env { vars: RefCell::new(HashMap::new()), parent: Some(parent.clone()), owned: RefCell::new(Vec::new()) })
    }

    pub fn define(&self, name: &str, v: Value) {
        self.vars.borrow_mut().insert(name.into(), Rc::new(RefCell::new(v)));
    }

    pub fn cell(&self, name: &str) -> Option<Rc<RefCell<Value>>> {
        if let Some(c) = self.vars.borrow().get(name) {
            return Some(c.clone());
        }
        self.parent.as_ref().and_then(|p| p.cell(name))
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        self.cell(name).map(|c| c.borrow().clone())
    }
}

impl Value {
    fn rank(&self) -> u8 {
        match self {
            Value::Unit => 0,
            Value::Bool(_) => 1,
            Value::Int(_) => 2,
            Value::Float(_) => 3,
            Value::Str(_) | Value::Buf(_) => 4,
            Value::List(_) => 5,
            Value::Tuple(_) => 6,
            Value::Record(..) => 7,
            Value::Variant(..) => 8,
            Value::Opt(_) => 9,
            Value::Res(_) => 10,
            Value::Map(_) => 11,
            Value::New(..) => 12,
            Value::Wrap(..) => 14,
            Value::Guess(..) => 15,
            Value::Closure(_) | Value::Func(_) | Value::LocalFn(..) | Value::Builtin(_) | Value::Ref(_) => 13,
            Value::Ptr(..) => 18,
            Value::Atomic(_) => 16,
            Value::Chan(_) => 17,
            Value::Set(_) => 19,
            Value::Heap(_) => 20,
            Value::Time(_) => 21,
            Value::Dur(_) => 22,
            Value::Bits(..) => 23,
            #[cfg(not(target_family = "wasm"))]
            Value::Bytes(_) => 29,
            Value::Big(_) => 24,
            Value::Dec(..) => 25,
            Value::Regex(_) => 26,
            Value::Dev(_) => 27,
            Value::View(_) => 28,
        }
    }

    pub fn str(s: &str) -> Value {
        Value::Str(s.into())
    }

    pub fn list(v: Vec<Value>) -> Value {
        Value::List(Rc::new(v))
    }

    pub fn some(v: Value) -> Value {
        Value::Opt(Some(Rc::new(v)))
    }

    pub fn field(&self, name: &str) -> Option<Value> {
        match self {
            Value::Record(_, fs) | Value::Variant(_, Some(fs)) => fs.iter().find(|(n, _)| &**n == name).map(|(_, v)| v.clone()),
            _ => None,
        }
    }
}

fn cmp_fields(a: &Fields, b: &Fields) -> Ordering {
    a.iter().map(|(_, v)| v).cmp(b.iter().map(|(_, v)| v))
}

/// F64 order: numeric, except that NaN equals itself and sorts above +inf; -0.0 equals 0.0.
fn canon(x: f64) -> f64 {
    if x.is_nan() {
        f64::NAN
    } else if x == 0.0 {
        0.0
    } else {
        x
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        use Value::*;
        match (self, other) {
            (Unit, Unit) => Ordering::Equal,
            (Bool(a), Bool(b)) => a.cmp(b),
            (Int(a), Int(b)) => a.cmp(b),
            (Float(a), Float(b)) => canon(*a).total_cmp(&canon(*b)),
            (Str(a), Str(b)) => a.cmp(b),
            (Buf(a), Buf(b)) => a.cmp(b),
            (Buf(a), Str(b)) => (**a).as_str().cmp(&**b),
            (Str(a), Buf(b)) => (**a).cmp(b.as_str()),
            (List(a), List(b)) | (Tuple(a), Tuple(b)) => a.iter().cmp(b.iter()),
            (Record(n1, a), Record(n2, b)) if &**n1 == "#Ratio" && &**n2 == "#Ratio" => match (&a[0].1, &a[1].1, &b[0].1, &b[1].1) {
                (Big(an), Big(ad), Big(bn), Big(bd)) => sspur_native::bigint::ratio_cmp(an, ad, bn, bd),
                _ => cmp_fields(a, b),
            },
            (Record(n1, a), Record(n2, b)) => n1.cmp(n2).then_with(|| cmp_fields(a, b)),
            (Variant(n1, a), Variant(n2, b)) => n1.cmp(n2).then_with(|| match (a, b) {
                (Some(a), Some(b)) => cmp_fields(a, b),
                (a, b) => a.is_some().cmp(&b.is_some()),
            }),
            (Opt(a), Opt(b)) => a.cmp(b),
            (Res(a), Res(b)) => match (a, b) {
                (Ok(x), Ok(y)) | (Err(x), Err(y)) => x.cmp(y),
                (Ok(_), Err(_)) => Ordering::Less,
                (Err(_), Ok(_)) => Ordering::Greater,
            },
            (Map(a), Map(b)) => a.iter().cmp(b.iter()),
            (Set(a), Set(b)) => a.iter().cmp(b.iter()),
            (Heap(a), Heap(b)) => a.iter().cmp(b.iter()),
            (New(n1, a), New(n2, b)) | (Wrap(n1, a), Wrap(n2, b)) => n1.cmp(n2).then_with(|| a.cmp(b)),
            (Guess(a, c1), Guess(b, c2)) => a.cmp(b).then_with(|| c1.total_cmp(c2)),
            (Ptr(a, x), Ptr(b, y)) => a.cmp(b).then_with(|| x.cmp(y)),
            (Atomic(a), Atomic(b)) => a.id.cmp(&b.id),
            (Chan(a), Chan(b)) => a.id.cmp(&b.id),
            (Time(a), Time(b)) | (Dur(a), Dur(b)) => a.cmp(b),
            (Bits(n, a), Bits(m, b)) => n.cmp(m).then_with(|| a.cmp(b)),
            #[cfg(not(target_family = "wasm"))]
            (Bytes(a), Bytes(b)) => a.0.cmp(&b.0),
            (Big(a), Big(b)) => a.compare(b),
            (Regex(a), Regex(b)) => a.src.cmp(&b.src),
            (Dev(a), Dev(b)) => a.id.cmp(&b.id),
            (Dec(a, sa), Dec(b, sb)) => crate::stdbig::dec_order(a, *sa, b, *sb),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Value {}

fn write_fields(f: &mut fmt::Formatter<'_>, fs: &Fields) -> fmt::Result {
    write!(f, "{{")?;
    for (i, (n, v)) in fs.iter().enumerate() {
        if i > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{n}: {}", Quoted(v))?;
    }
    write!(f, "}}")
}

fn write_seq(f: &mut fmt::Formatter<'_>, open: &str, close: &str, xs: &[Value]) -> fmt::Result {
    write!(f, "{open}")?;
    for (i, v) in xs.iter().enumerate() {
        if i > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{}", Quoted(v))?;
    }
    write!(f, "{close}")
}

pub struct Quoted<'a>(pub &'a Value);

impl fmt::Display for Quoted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Value::Str(s) => write!(f, "{s:?}"),
            Value::Buf(s) => write!(f, "{s:?}"),
            v => write!(f, "{v}"),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Unit => write!(f, "()"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(x) => {
                let s = format!("{x:?}");
                write!(f, "{s}")
            }
            Value::Str(s) => write!(f, "{s}"),
            Value::Buf(s) => write!(f, "{s}"),
            Value::List(xs) => write_seq(f, "[", "]", xs),
            Value::Tuple(xs) => write_seq(f, "(", ")", xs),
            Value::Record(n, fs) if &**n == "#File" => write!(f, "File({})", Quoted(&fs[1].1)),
            Value::Record(n, fs) if &**n == "#Ratio" => match (&fs[0].1, &fs[1].1) {
                (Value::Big(a), Value::Big(b)) => write!(f, "{}", sspur_native::bigint::ratio_str(a, b)),
                _ => write_fields(f, fs),
            },
            Value::Record(n, fs) => {
                write!(f, "{}", sspur_syntax::display_name(n.trim_start_matches('#')))?;
                write_fields(f, fs)
            }
            Value::Variant(n, None) => write!(f, "{}", sspur_syntax::display_name(n)),
            Value::Variant(n, Some(fs)) => {
                write!(f, "{}", sspur_syntax::display_name(n))?;
                write_fields(f, fs)
            }
            Value::Opt(None) => write!(f, "none"),
            Value::Opt(Some(v)) => write!(f, "some({})", Quoted(v)),
            Value::Res(Ok(v)) => write!(f, "ok({})", Quoted(v)),
            Value::Res(Err(v)) => write!(f, "err({})", Quoted(v)),
            Value::Map(m) => {
                write!(f, "{{")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", Quoted(k), Quoted(v))?;
                }
                write!(f, "}}")
            }
            Value::Set(s) => {
                write!(f, "{{")?;
                for (i, v) in s.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", Quoted(v))?;
                }
                write!(f, "}}")
            }
            Value::Heap(xs) => write_seq(f, "heap[", "]", xs),
            Value::New(n, v) => write!(f, "{}({})", sspur_syntax::display_name(n), Quoted(v)),
            Value::Wrap(k, _) if &**k == "Secret" => write!(f, "<secret>"),
            Value::Wrap(k, _) if &**k == "Pii" => write!(f, "<redacted>"),
            Value::Wrap(_, v) => write!(f, "untrusted({})", Quoted(v)),
            Value::Guess(v, c) => write!(f, "guess({}, {c})", Quoted(v)),
            Value::Closure(_) => write!(f, "<fn>"),
            Value::Func(d) | Value::LocalFn(d, _) => write!(f, "<fn {}>", d.name),
            Value::Builtin(n) => write!(f, "<builtin {n}>"),
            Value::Ptr(..) => write!(f, "<ptr>"),
            Value::Ref(c) => write!(f, "{}", c.borrow()),
            Value::Atomic(_) => write!(f, "<atomic>"),
            Value::Chan(_) => write!(f, "<chan>"),
            Value::Time(t) => write!(f, "{}", sspur_native::chrono::iso(*t)),
            Value::Dur(d) => write!(f, "{}", sspur_native::chrono::dur_str(*d)),
            Value::Big(b) => write!(f, "{b}"),
            Value::Regex(r) => write!(f, "/{}/", r.src),
            Value::Dev(d) => write!(f, "<devbuf {} x{}>", d.ty.name(), d.data.borrow().len()),
            Value::View(_) => write!(f, "<view>"),
            Value::Dec(m, s) => write!(f, "{}", sspur_native::bigint::dec_str(m, *s)),
            #[cfg(not(target_family = "wasm"))]
            Value::Bytes(b) => write!(f, "{}", sspur_native::bytes::show(&b.0)),
            Value::Bits(n, w) => {
                let items: Vec<String> = crate::stdx::bits_items(*n, w).iter().map(|i| i.to_string()).collect();
                write!(f, "bits({n}){{{}}}", items.join(", "))
            }
        }
    }
}
