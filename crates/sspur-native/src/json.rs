use crate::nval::{Layouts, NVal};
use sspur_check::Type;
use std::fmt::Write as _;

pub enum Jv {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Jv>),
    Obj(Vec<(String, Jv)>),
}

fn strip(l: &Layouts, t: &Type) -> Type {
    match t {
        Type::Con(n, a) if n == "Pii" && a.len() == 1 => strip(l, &a[0]),
        Type::Con(n, a) if a.is_empty() && l.newtypes.contains_key(n) => strip(l, &l.newtypes[n]),
        _ => t.clone(),
    }
}

fn unwrap(v: &NVal) -> &NVal {
    match v {
        NVal::New(_, x) | NVal::Wrap(_, x) => unwrap(x),
        _ => v,
    }
}

pub fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn encode(l: &Layouts, v: &NVal, t: &Type, out: &mut String) -> Option<()> {
    let t = strip(l, t);
    let v = unwrap(v);
    match (v, &t) {
        (NVal::Unit, _) => out.push_str("null"),
        (NVal::Int(n) | NVal::Dur(n), _) => write!(out, "{n}").unwrap(),
        (NVal::Time(t), _) => quote(&crate::chrono::iso(*t), out),
        (NVal::Big(b), _) => write!(out, "{b}").unwrap(),
        (NVal::Dec(m, s), _) => quote(&crate::bigint::dec_str(m, *s), out),
        (NVal::Bytes(b), _) => quote(&crate::bytes::base64(b, false), out),
        (NVal::Bits(n, w), _) => {
            out.push('"');
            for i in (0..*n).rev() {
                out.push(if w[(i / 64) as usize] >> (i % 64) & 1 == 1 { '1' } else { '0' });
            }
            out.push('"');
        }
        (NVal::Bool(b), _) => out.push_str(if *b { "true" } else { "false" }),
        (NVal::Float(x), _) => {
            if x.is_finite() {
                write!(out, "{x:?}").unwrap()
            } else {
                out.push_str("null")
            }
        }
        (NVal::Str(s), _) => quote(s, out),
        (NVal::List(xs) | NVal::Set(xs) | NVal::Heap(xs), Type::Con(_, a)) => {
            out.push('[');
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode(l, x, &a[0], out)?;
            }
            out.push(']');
        }
        (NVal::Opt(None), _) => out.push_str("null"),
        (NVal::Opt(Some(x)), Type::Con(_, a)) => encode(l, x, &a[0], out)?,
        (NVal::Map(kv), Type::Con(_, a)) => {
            let obj = strip(l, &a[0]) == Type::str();
            out.push(if obj { '{' } else { '[' });
            for (i, (k, x)) in kv.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if obj {
                    encode(l, k, &a[0], out)?;
                    out.push(':');
                    encode(l, x, &a[1], out)?;
                } else {
                    out.push('[');
                    encode(l, k, &a[0], out)?;
                    out.push(',');
                    encode(l, x, &a[1], out)?;
                    out.push(']');
                }
            }
            out.push(if obj { '}' } else { ']' });
        }
        (NVal::Tuple(xs), Type::Tuple(ts)) => {
            out.push('[');
            for (i, (x, xt)) in xs.iter().zip(ts).enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode(l, x, xt, out)?;
            }
            out.push(']');
        }
        (NVal::Rec(_, fs), Type::Con(n, a)) => {
            let decl = l.record_fields(n, a)?;
            out.push('{');
            for (i, ((f, x), (_, ft))) in fs.iter().zip(&decl).enumerate() {
                if i > 0 {
                    out.push(',');
                }
                quote(f, out);
                out.push(':');
                encode(l, x, ft, out)?;
            }
            out.push('}');
        }
        (NVal::Variant(c, fs), Type::Con(n, a)) => {
            let vs = l.sum_variants(n, a)?;
            let decl = vs.iter().find(|(v, _)| v == c)?.1.clone();
            match (fs, decl) {
                (Some(fs), Some(decl)) => {
                    out.push_str("{\"tag\":");
                    quote(sspur_syntax::display_name(c), out);
                    for ((f, x), (_, ft)) in fs.iter().zip(&decl) {
                        out.push(',');
                        quote(f, out);
                        out.push(':');
                        encode(l, x, ft, out)?;
                    }
                    out.push('}');
                }
                _ => quote(sspur_syntax::display_name(c), out),
            }
        }
        _ => return None,
    }
    Some(())
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
    depth: u32,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        if self.s.len() - self.i < 4 {
            return None;
        }
        let mut v = 0;
        for k in 0..4 {
            v = v * 16 + (self.s[self.i + k] as char).to_digit(16)?;
        }
        self.i += 4;
        Some(v)
    }

    fn string(&mut self) -> Option<String> {
        self.i += 1;
        let mut o: Vec<u8> = Vec::new();
        loop {
            let c = *self.s.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(o).ok(),
                c if c < 0x20 => return None,
                b'\\' => {
                    let e = *self.s.get(self.i)?;
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) {
                                if self.s.len() - self.i < 6 || self.s[self.i] != b'\\' || self.s[self.i + 1] != b'u' {
                                    return None;
                                }
                                self.i += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..=0xDFFF).contains(&lo) {
                                    return None;
                                }
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                            } else if (0xDC00..0xE000).contains(&cp) {
                                return None;
                            }
                            char::from_u32(cp)?
                        }
                        _ => return None,
                    };
                    let mut b = [0u8; 4];
                    o.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                }
                c => o.push(c),
            }
        }
    }

    fn digits(&mut self) -> usize {
        let st = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
        }
        self.i - st
    }

    fn value(&mut self) -> Option<Jv> {
        self.ws();
        self.depth += 1;
        if self.i >= self.s.len() || self.depth > 256 {
            return None;
        }
        let c = self.s[self.i];
        let r = match c {
            b'{' | b'[' => {
                let obj = c == b'{';
                let close = if obj { b'}' } else { b']' };
                self.i += 1;
                self.ws();
                let mut items = Vec::new();
                let mut keys = Vec::new();
                if self.s.get(self.i) == Some(&close) {
                    self.i += 1;
                } else {
                    loop {
                        if obj {
                            self.ws();
                            if self.s.get(self.i) != Some(&b'"') {
                                return None;
                            }
                            keys.push(self.string()?);
                            self.ws();
                            if self.s.get(self.i) != Some(&b':') {
                                return None;
                            }
                            self.i += 1;
                        }
                        items.push(self.value()?);
                        self.ws();
                        match self.s.get(self.i) {
                            Some(b',') => self.i += 1,
                            Some(x) if *x == close => {
                                self.i += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                if obj { Jv::Obj(keys.into_iter().zip(items).collect()) } else { Jv::Arr(items) }
            }
            b'"' => Jv::Str(self.string()?),
            b'-' | b'0'..=b'9' => {
                let st = self.i;
                if c == b'-' {
                    self.i += 1;
                }
                match self.s.get(self.i) {
                    Some(b'0') => self.i += 1,
                    Some(d) if d.is_ascii_digit() => {
                        self.digits();
                    }
                    _ => return None,
                }
                if self.s.get(self.i) == Some(&b'.') {
                    self.i += 1;
                    if self.digits() == 0 {
                        return None;
                    }
                }
                if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
                    self.i += 1;
                    if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                        self.i += 1;
                    }
                    if self.digits() == 0 {
                        return None;
                    }
                }
                Jv::Num(String::from_utf8_lossy(&self.s[st..self.i]).into_owned())
            }
            _ => {
                let rest = &self.s[self.i..];
                let (w, v) = if rest.starts_with(b"null") {
                    (4, Jv::Null)
                } else if rest.starts_with(b"true") {
                    (4, Jv::Bool(true))
                } else if rest.starts_with(b"false") {
                    (5, Jv::Bool(false))
                } else {
                    return None;
                };
                self.i += w;
                v
            }
        };
        self.depth -= 1;
        Some(r)
    }
}

pub fn parse(s: &str) -> Option<Jv> {
    let mut p = P { s: s.as_bytes(), i: 0, depth: 0 };
    let v = p.value()?;
    p.ws();
    (p.i == p.s.len()).then_some(v)
}

fn get<'a>(v: &'a Jv, k: &str) -> Option<&'a Jv> {
    match v {
        Jv::Obj(kv) => kv.iter().find(|(n, _)| n == k).map(|(_, x)| x),
        _ => None,
    }
}

struct D<'a> {
    l: &'a Layouts,
    path: Vec<String>,
}

impl D<'_> {
    fn err(&self, want: &str, v: Option<&Jv>) -> String {
        let mut path = String::new();
        for (i, s) in self.path.iter().enumerate() {
            if i > 0 && !s.starts_with('[') {
                path.push('.');
            }
            path.push_str(s);
        }
        let got = match v {
            None => "nothing",
            Some(Jv::Null) => "null",
            Some(Jv::Num(_)) => "a number",
            Some(Jv::Str(_)) => "a string",
            Some(Jv::Arr(_)) => "an array",
            Some(Jv::Obj(_)) => "an object",
            Some(Jv::Bool(_)) => "a boolean",
        };
        format!("{}: expected {want}, found {got}", if path.is_empty() { "value" } else { &path })
    }

    fn items(&mut self, v: Option<&Jv>, want: &str, et: &Type) -> Result<Vec<NVal>, String> {
        let Some(Jv::Arr(xs)) = v else { return Err(self.err(want, v)) };
        let mut out = Vec::new();
        for (i, x) in xs.iter().enumerate() {
            self.path.push(format!("[{i}]"));
            out.push(self.dec(Some(x), et)?);
            self.path.pop();
        }
        Ok(out)
    }

    fn fields(&mut self, v: Option<&Jv>, decl: &[(String, Type)]) -> Result<Vec<(String, NVal)>, String> {
        let mut out = Vec::new();
        for (f, ft) in decl {
            self.path.push(f.clone());
            let x = self.dec(v.and_then(|v| get(v, f)), ft)?;
            self.path.pop();
            out.push((f.clone(), x));
        }
        Ok(out)
    }

    fn dec(&mut self, v: Option<&Jv>, t: &Type) -> Result<NVal, String> {
        let wrap = |t: &Type, x: NVal| match t {
            Type::Con(n, a) if a.is_empty() && self.l.newtypes.contains_key(n) => NVal::New(n.clone(), Box::new(x)),
            Type::Con(n, _) if n == "Pii" => NVal::Wrap(n.clone(), Box::new(x)),
            _ => x,
        };
        if let Type::Con(n, a) = t
            && (n == "Pii" || (a.is_empty() && self.l.newtypes.contains_key(n))) {
                let inner = if n == "Pii" { a[0].clone() } else { self.l.newtypes[n].clone() };
                let x = self.dec(v, &inner)?;
                return Ok(wrap(t, x));
            }
        let want = t.to_string();
        Ok(match t {
            Type::Con(n, a) => match n.as_str() {
                "Int" => match v {
                    Some(Jv::Num(s)) if s.len() <= 24 && s.bytes().enumerate().all(|(i, c)| c.is_ascii_digit() || (i == 0 && c == b'-')) => NVal::Int(s.parse().map_err(|_| self.err(&want, v))?),
                    _ => return Err(self.err(&want, v)),
                },
                "#Duration" => match self.dec(v, &Type::int()).map_err(|_| self.err(&want, v))? {
                    NVal::Int(n) => NVal::Dur(n),
                    _ => return Err(self.err(&want, v)),
                },
                "#BigInt" => match v {
                    Some(Jv::Num(s)) if s.bytes().enumerate().all(|(i, c)| c.is_ascii_digit() || (i == 0 && c == b'-')) => NVal::Big(crate::bigint::Big::parse(s).ok_or_else(|| self.err(&want, v))?),
                    _ => return Err(self.err(&want, v)),
                },
                "#Dec" => match v {
                    Some(Jv::Num(s) | Jv::Str(s)) => match crate::bigint::dec_parse(s) {
                        Some((m, sc)) => NVal::Dec(m, sc),
                        None => return Err(self.err(&want, v)),
                    },
                    _ => return Err(self.err(&want, v)),
                },
                "#Bits" => match v {
                    Some(Jv::Str(s)) if s.len() <= 1 << 20 && s.bytes().all(|c| c == b'0' || c == b'1') => {
                        let mut w = vec![0u64; s.len().div_ceil(64)];
                        for (k, c) in s.bytes().rev().enumerate() {
                            if c == b'1' {
                                w[k / 64] |= 1 << (k % 64);
                            }
                        }
                        NVal::Bits(s.len() as i64, w)
                    }
                    _ => return Err(self.err(&want, v)),
                },
                "#Bytes" => match v {
                    Some(Jv::Str(s)) => NVal::Bytes(crate::bytes::from_base64(s, false).ok_or_else(|| self.err(&want, v))?),
                    _ => return Err(self.err(&want, v)),
                },
                "#Time" => match v {
                    Some(Jv::Str(s)) => NVal::Time(crate::chrono::parse(s).ok_or_else(|| self.err(&want, v))?),
                    _ => return Err(self.err(&want, v)),
                },
                "F64" => match v {
                    Some(Jv::Num(s)) if s.len() <= 400 => NVal::Float(s.parse().map_err(|_| self.err(&want, v))?),
                    _ => return Err(self.err(&want, v)),
                },
                "Bool" => match v {
                    Some(Jv::Bool(b)) => NVal::Bool(*b),
                    _ => return Err(self.err(&want, v)),
                },
                "Unit" => NVal::Unit,
                "Str" => match v {
                    Some(Jv::Str(s)) => NVal::Str(s.clone()),
                    _ => return Err(self.err(&want, v)),
                },
                "List" => NVal::List(self.items(v, &want, &a[0])?),
                "#Set" | "#HashSet" => NVal::Set(self.items(v, &want, &a[0])?),
                "#Heap" => NVal::Heap(self.items(v, &want, &a[0])?),
                "Opt" => match v {
                    None | Some(Jv::Null) => NVal::Opt(None),
                    Some(x) => NVal::Opt(Some(Box::new(self.dec(Some(x), &a[0])?))),
                },
                "Map" | "#HashMap" => {
                    let mut kv = Vec::new();
                    if strip(self.l, &a[0]) == Type::str() {
                        let Some(Jv::Obj(items)) = v else { return Err(self.err(&want, v)) };
                        for (k, x) in items {
                            let key = self.dec(Some(&Jv::Str(k.clone())), &a[0])?;
                            self.path.push(k.clone());
                            let val = self.dec(Some(x), &a[1])?;
                            self.path.pop();
                            kv.push((key, val));
                        }
                    } else {
                        let Some(Jv::Arr(items)) = v else { return Err(self.err(&want, v)) };
                        for (i, e) in items.iter().enumerate() {
                            self.path.push(format!("[{i}]"));
                            let Jv::Arr(pair) = e else { return Err(self.err("[key, value]", Some(e))) };
                            if pair.len() != 2 {
                                return Err(self.err("[key, value]", Some(e)));
                            }
                            let key = self.dec(Some(&pair[0]), &a[0])?;
                            let val = self.dec(Some(&pair[1]), &a[1])?;
                            self.path.pop();
                            kv.push((key, val));
                        }
                    }
                    NVal::Map(kv)
                }
                _ => {
                    if let Some(decl) = self.l.record_fields(n, a) {
                        if !matches!(v, Some(Jv::Obj(_))) {
                            return Err(self.err(&want, v));
                        }
                        NVal::Rec(n.clone(), self.fields(v, &decl)?)
                    } else if let Some(vs) = self.l.sum_variants(n, a) {
                        let tag = match v {
                            Some(o @ Jv::Obj(_)) => get(o, "tag"),
                            other => other,
                        };
                        let Some(Jv::Str(tn)) = tag else { return Err(self.err(&want, v)) };
                        match vs.iter().find(|(vn, _)| sspur_syntax::display_name(vn) == tn) {
                            Some((vn, None)) => NVal::Variant(vn.clone(), None),
                            Some((vn, Some(fs))) => NVal::Variant(vn.clone(), Some(self.fields(v, fs)?)),
                            None => return Err(self.err(&format!("one of {}", vs.iter().map(|v| sspur_syntax::display_name(&v.0)).collect::<Vec<_>>().join(", ")), tag)),
                        }
                    } else {
                        return Err(format!("{want} has no JSON form"));
                    }
                }
            },
            Type::Tuple(ts) => {
                let Some(Jv::Arr(xs)) = v else { return Err(self.err(&want, v)) };
                if xs.len() != ts.len() {
                    return Err(self.err(&want, v));
                }
                let mut out = Vec::new();
                for (i, (x, xt)) in xs.iter().zip(ts).enumerate() {
                    self.path.push(format!("[{i}]"));
                    out.push(self.dec(Some(x), xt)?);
                    self.path.pop();
                }
                NVal::Tuple(out)
            }
            _ => return Err(format!("{want} has no JSON form")),
        })
    }
}

pub fn decode(l: &Layouts, s: &str, t: &Type) -> Result<NVal, String> {
    let Some(v) = parse(s) else { return Err("invalid JSON".into()) };
    D { l, path: vec![] }.dec(Some(&v), t)
}
