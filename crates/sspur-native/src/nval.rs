use sspur_check::{CheckOutput, Type};
use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum NVal {
    Unit,
    Int(i64),
    Bool(bool),
    Float(f64),
    Str(String),
    Rec(String, Vec<(String, NVal)>),
    Variant(String, Option<Vec<(String, NVal)>>),
    List(Vec<NVal>),
    Map(Vec<(NVal, NVal)>),
    Opt(Option<Box<NVal>>),
    Tuple(Vec<NVal>),
    New(String, Box<NVal>),
    Wrap(String, Box<NVal>),
    Guess(Box<NVal>, f64),
    Set(Vec<NVal>),
    Heap(Vec<NVal>),
    Res(Result<Box<NVal>, Box<NVal>>),
    Time(i64),
    Dur(i64),
    Bits(i64, Vec<u64>),
    Bytes(Vec<u8>),
    Big(crate::bigint::Big),
    Dec(crate::bigint::Big, i64),
    Regex(String),
}

#[derive(Clone, Default)]
pub struct Layouts {
    pub records: sspur_check::RecordTable,
    pub sums: sspur_check::SumTable,
    pub newtypes: HashMap<String, Type>,
}

impl Layouts {
    pub fn from_check(c: &CheckOutput) -> Self {
        Layouts { records: c.records.clone(), sums: c.sums.clone(), newtypes: c.newtypes.clone() }
    }
}

pub type Variants = Vec<(String, Option<Vec<(String, Type)>>)>;

pub fn subst(t: &Type, params: &[String], args: &[Type]) -> Type {
    match t {
        Type::Param(p) => params.iter().position(|x| x == p).map_or_else(|| t.clone(), |i| args[i].clone()),
        Type::Con(n, a) => Type::Con(n.clone(), a.iter().map(|x| subst(x, params, args)).collect()),
        Type::Tuple(xs) => Type::Tuple(xs.iter().map(|x| subst(x, params, args)).collect()),
        other => other.clone(),
    }
}

impl Layouts {
    pub fn record_fields(&self, name: &str, args: &[Type]) -> Option<Vec<(String, Type)>> {
        let (ps, fs) = self.records.get(name)?;
        Some(fs.iter().map(|(n, t)| (n.clone(), subst(t, ps, args))).collect())
    }

    pub fn sum_variants(&self, name: &str, args: &[Type]) -> Option<Variants> {
        let (ps, vs) = self.sums.get(name)?;
        Some(vs.iter().map(|(v, fs)| (v.clone(), fs.as_ref().map(|fs| fs.iter().map(|(n, t)| (n.clone(), subst(t, ps, args))).collect()))).collect())
    }

    pub fn encode(&self, v: &NVal, t: &Type, out: &mut Vec<i64>) -> Option<()> {
        match (v, t) {
            (NVal::Unit, _) => out.push(0),
            (NVal::Int(n) | NVal::Time(n) | NVal::Dur(n), _) => out.push(*n),
            (NVal::Big(b), _) => {
                out.push(if b.neg { -(b.mag.len() as i64) } else { b.mag.len() as i64 });
                out.extend(b.mag.iter().map(|x| i64::from(*x)));
            }
            (NVal::Dec(b, s), _) => {
                out.push(if b.neg { -(b.mag.len() as i64) } else { b.mag.len() as i64 });
                out.extend(b.mag.iter().map(|x| i64::from(*x)));
                out.push(*s);
            }
            (NVal::Bits(n, w), _) => {
                out.push(*n);
                out.extend(w.iter().map(|x| *x as i64));
            }
            (NVal::Bool(b), _) => out.push(i64::from(*b)),
            (NVal::Float(x), _) => out.push(x.to_bits() as i64),
            (NVal::Bytes(b), _) => {
                out.push(b.len() as i64);
                for chunk in b.chunks(8) {
                    let mut w = [0u8; 8];
                    w[..chunk.len()].copy_from_slice(chunk);
                    out.push(i64::from_le_bytes(w));
                }
            }
            (NVal::Str(s) | NVal::Regex(s), _) => {
                let b = s.as_bytes();
                out.push(b.len() as i64);
                for chunk in b.chunks(8) {
                    let mut w = [0u8; 8];
                    w[..chunk.len()].copy_from_slice(chunk);
                    out.push(i64::from_le_bytes(w));
                }
            }
            (NVal::New(_, x), Type::Con(n, _)) => self.encode(x, self.newtypes.get(n)?, out)?,
            (NVal::Wrap(_, x), Type::Con(_, a)) => self.encode(x, a.first()?, out)?,
            (NVal::Guess(x, c), Type::Con(_, a)) => {
                self.encode(x, a.first()?, out)?;
                out.push(c.to_bits() as i64);
            }
            (NVal::Tuple(xs), Type::Tuple(ts)) => {
                for (x, t) in xs.iter().zip(ts) {
                    self.encode(x, t, out)?;
                }
            }
            (NVal::List(xs), Type::Con(n, a)) if n == "List" => {
                out.push(xs.len() as i64);
                for x in xs {
                    self.encode(x, &a[0], out)?;
                }
            }
            (NVal::Set(xs) | NVal::Heap(xs), Type::Con(n, a)) if n == "#Set" || n == "#Heap" || n == "#HashSet" => {
                out.push(xs.len() as i64);
                for x in xs {
                    self.encode(x, &a[0], out)?;
                }
            }
            (NVal::Res(r), Type::Con(n, a)) if n == "Res" => match r {
                Ok(x) => {
                    out.push(1);
                    self.encode(x, &a[0], out)?;
                }
                Err(e) => {
                    out.push(0);
                    self.encode(e, &a[1], out)?;
                }
            },
            (NVal::Map(kv), Type::Con(n, a)) if n == "Map" || n == "#HashMap" => {
                out.push(kv.len() as i64);
                for (k, v) in kv {
                    self.encode(k, &a[0], out)?;
                    self.encode(v, &a[1], out)?;
                }
            }
            (NVal::Opt(o), Type::Con(n, a)) if n == "Opt" => match o {
                None => out.push(0),
                Some(x) => {
                    out.push(1);
                    self.encode(x, &a[0], out)?;
                }
            },
            (NVal::Rec(_, fs), Type::Con(n, a)) => {
                let decl = self.record_fields(n, a)?;
                for ((_, fv), (_, ft)) in fs.iter().zip(&decl) {
                    self.encode(fv, ft, out)?;
                }
            }
            (NVal::Variant(c, fs), Type::Con(n, a)) => {
                let vs = self.sum_variants(n, a)?;
                let tag = vs.iter().position(|(v, _)| v == c)?;
                out.push(tag as i64);
                if let (Some(fs), Some(Some(decl))) = (fs, vs.get(tag).map(|v| &v.1)) {
                    for ((_, fv), (_, ft)) in fs.iter().zip(decl) {
                        self.encode(fv, ft, out)?;
                    }
                }
            }
            _ => return None,
        }
        Some(())
    }

    pub fn decode(&self, words: &[i64], pos: &mut usize, t: &Type) -> Option<NVal> {
        let mut next = || {
            let w = *words.get(*pos)?;
            *pos += 1;
            Some(w)
        };
        Some(match t {
            Type::Con(n, a) => match n.as_str() {
                "Int" => NVal::Int(next()?),
                "#Time" => NVal::Time(next()?),
                "#Duration" => NVal::Dur(next()?),
                "#BigInt" | "#Dec" => {
                    let cnt = next()?;
                    let mut mag = Vec::new();
                    for _ in 0..cnt.unsigned_abs().min(1 << 24) {
                        mag.push(next()? as u32);
                    }
                    let b = crate::bigint::Big { neg: cnt < 0, mag };
                    if n == "#Dec" { NVal::Dec(b, next()?) } else { NVal::Big(b) }
                }
                "#Bits" => {
                    let n = next()?;
                    let mut w = Vec::new();
                    for _ in 0..(n.clamp(0, 1 << 32) + 63) / 64 {
                        w.push(next()? as u64);
                    }
                    NVal::Bits(n, w)
                }
                "Bool" => NVal::Bool(next()? != 0),
                "F64" => NVal::Float(f64::from_bits(next()? as u64)),
                "#Set" | "#Heap" | "#HashSet" => {
                    let len = next()?;
                    let mut xs = Vec::with_capacity(len.clamp(0, 1 << 16) as usize);
                    for _ in 0..len {
                        xs.push(self.decode(words, pos, &a[0])?);
                    }
                    if n == "#Heap" { NVal::Heap(xs) } else { NVal::Set(xs) }
                }
                "Res" => {
                    if next()? == 1 {
                        NVal::Res(Ok(Box::new(self.decode(words, pos, &a[0])?)))
                    } else {
                        NVal::Res(Err(Box::new(self.decode(words, pos, &a[1])?)))
                    }
                }
                "#Regex" => match self.decode(words, pos, &Type::str())? {
                    NVal::Str(s) => NVal::Regex(s),
                    _ => return None,
                },
                "Str" | "#StrBuf" | "#Bytes" => {
                    let len = next()? as usize;
                    let mut bytes = Vec::with_capacity(len.min(1 << 24));
                    for _ in 0..len.div_ceil(8) {
                        bytes.extend_from_slice(&next()?.to_le_bytes());
                    }
                    bytes.truncate(len);
                    if n == "#Bytes" { NVal::Bytes(bytes) } else { NVal::Str(String::from_utf8(bytes).ok()?) }
                }
                "Unit" => {
                    next()?;
                    NVal::Unit
                }
                "List" => {
                    let len = next()?;
                    let mut xs = Vec::with_capacity(len.max(0) as usize);
                    for _ in 0..len {
                        xs.push(self.decode(words, pos, &a[0])?);
                    }
                    NVal::List(xs)
                }
                "Map" | "#HashMap" => {
                    let len = next()?;
                    let mut kv = Vec::new();
                    for _ in 0..len {
                        let k = self.decode(words, pos, &a[0])?;
                        let v = self.decode(words, pos, &a[1])?;
                        kv.push((k, v));
                    }
                    NVal::Map(kv)
                }
                "Secret" | "Pii" | "Untrusted" => NVal::Wrap(n.clone(), Box::new(self.decode(words, pos, &a[0])?)),
                "Guess" => {
                    let v = self.decode(words, pos, &a[0])?;
                    let c = f64::from_bits(*words.get(*pos)? as u64);
                    *pos += 1;
                    NVal::Guess(Box::new(v), c)
                }
                _ if self.newtypes.contains_key(n) => NVal::New(n.clone(), Box::new(self.decode(words, pos, &self.newtypes[n].clone())?)),
                "Opt" => {
                    if next()? == 0 {
                        NVal::Opt(None)
                    } else {
                        NVal::Opt(Some(Box::new(self.decode(words, pos, &a[0])?)))
                    }
                }
                _ => {
                    if let Some(fs) = self.record_fields(n, a) {
                        let mut out = Vec::new();
                        for (fname, ft) in fs {
                            out.push((fname, self.decode(words, pos, &ft)?));
                        }
                        NVal::Rec(n.clone(), out)
                    } else {
                        let vs = self.sum_variants(n, a)?;
                        let tag = next()? as usize;
                        let (vname, fields) = vs.get(tag)?.clone();
                        match fields {
                            None => NVal::Variant(vname, None),
                            Some(fs) => {
                                let mut out = Vec::new();
                                for (fname, ft) in fs {
                                    out.push((fname, self.decode(words, pos, &ft)?));
                                }
                                NVal::Variant(vname, Some(out))
                            }
                        }
                    }
                }
            },
            Type::Tuple(ts) => {
                let mut out = Vec::new();
                for t in ts {
                    out.push(self.decode(words, pos, t)?);
                }
                NVal::Tuple(out)
            }
            _ => return None,
        })
    }
}

fn fields(f: &mut fmt::Formatter<'_>, fs: &[(String, NVal)]) -> fmt::Result {
    write!(f, "{{")?;
    for (i, (n, v)) in fs.iter().enumerate() {
        if i > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{n}: {v}")?;
    }
    write!(f, "}}")
}

impl fmt::Display for NVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NVal::Unit => write!(f, "()"),
            NVal::Int(n) => write!(f, "{n}"),
            NVal::Bool(b) => write!(f, "{b}"),
            NVal::Float(x) => write!(f, "{x:?}"),
            NVal::Str(s) => write!(f, "{s:?}"),
            NVal::Rec(n, fs) if n == "#File" => write!(f, "File({})", fs[1].1),
            NVal::Rec(n, fs) => {
                write!(f, "{}", sspur_syntax::display_name(n.trim_start_matches('#')))?;
                fields(f, fs)
            }
            NVal::Variant(n, None) => write!(f, "{}", sspur_syntax::display_name(n)),
            NVal::Variant(n, Some(fs)) => {
                write!(f, "{}", sspur_syntax::display_name(n))?;
                fields(f, fs)
            }
            NVal::List(xs) => {
                write!(f, "[")?;
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{x}")?;
                }
                write!(f, "]")
            }
            NVal::Map(kv) => {
                write!(f, "{{")?;
                for (i, (k, v)) in kv.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            NVal::Set(xs) => {
                write!(f, "{{")?;
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{x}")?;
                }
                write!(f, "}}")
            }
            NVal::Heap(xs) => {
                write!(f, "heap[")?;
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{x}")?;
                }
                write!(f, "]")
            }
            NVal::Res(Ok(x)) => write!(f, "ok({x})"),
            NVal::Res(Err(x)) => write!(f, "err({x})"),
            NVal::New(n, v) => write!(f, "{n}({v})"),
            NVal::Wrap(k, _) if k == "Secret" => write!(f, "<secret>"),
            NVal::Wrap(k, _) if k == "Pii" => write!(f, "<redacted>"),
            NVal::Wrap(_, v) => write!(f, "untrusted({v})"),
            NVal::Guess(v, c) => write!(f, "guess({v}, {c})"),
            NVal::Time(t) => write!(f, "{}", crate::chrono::iso(*t)),
            NVal::Dur(d) => write!(f, "{}", crate::chrono::dur_str(*d)),
            NVal::Big(b) => write!(f, "{b}"),
            NVal::Regex(s) => write!(f, "/{s}/"),
            NVal::Dec(m, s) => write!(f, "{}", crate::bigint::dec_str(m, *s)),
            NVal::Bytes(b) => write!(f, "{}", crate::bytes::show(b)),
            NVal::Bits(n, w) => {
                let items: Vec<String> = (0..*n).filter(|&i| w[(i / 64) as usize] >> (i % 64) & 1 == 1).map(|i| i.to_string()).collect();
                write!(f, "bits({n}){{{}}}", items.join(", "))
            }
            NVal::Opt(None) => write!(f, "none"),
            NVal::Opt(Some(x)) => write!(f, "some({x})"),
            NVal::Tuple(xs) => {
                write!(f, "(")?;
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{x}")?;
                }
                write!(f, ")")
            }
        }
    }
}

pub fn scalar_display(t: &Type, bits: i64) -> String {
    match t {
        Type::Con(n, _) if n == "Bool" => (bits != 0).to_string(),
        Type::Con(n, _) if n == "F64" => format!("{:?}", f64::from_bits(bits as u64)),
        Type::Con(n, _) if n == "Unit" => "()".into(),
        _ => bits.to_string(),
    }
}
