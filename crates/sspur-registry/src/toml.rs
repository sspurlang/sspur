//! The small TOML subset `sspur.toml` and `sspur.lock` use: sections, `[[arrays]]`, strings,
//! string arrays and inline tables.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum TVal {
    Str(String),
    Arr(Vec<String>),
    Table(BTreeMap<String, TVal>),
}

impl TVal {
    pub fn str(&self) -> Option<&str> {
        if let TVal::Str(s) = self { Some(s) } else { None }
    }
}

pub struct TSection {
    pub name: String,
    pub array: bool,
    pub kv: BTreeMap<String, TVal>,
}

struct VP<'a> {
    b: &'a [u8],
    i: usize,
}

impl VP<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && (self.b[self.i] == b' ' || self.b[self.i] == b'\t') {
            self.i += 1;
        }
    }

    fn key(&mut self) -> Result<String, String> {
        self.ws();
        if self.b.get(self.i) == Some(&b'"') {
            return self.string();
        }
        let s = self.i;
        while self.i < self.b.len() && (self.b[self.i].is_ascii_alphanumeric() || self.b[self.i] == b'_' || self.b[self.i] == b'-') {
            self.i += 1;
        }
        if s == self.i {
            return Err("expected a key".into());
        }
        Ok(String::from_utf8_lossy(&self.b[s..self.i]).into_owned())
    }

    fn string(&mut self) -> Result<String, String> {
        let q = self.b[self.i];
        self.i += 1;
        let mut out = Vec::new();
        while self.i < self.b.len() && self.b[self.i] != q {
            if q == b'"' && self.b[self.i] == b'\\' && self.i + 1 < self.b.len() {
                self.i += 1;
                out.push(match self.b[self.i] {
                    b'n' => b'\n',
                    b't' => b'\t',
                    c => c,
                });
            } else {
                out.push(self.b[self.i]);
            }
            self.i += 1;
        }
        if self.i >= self.b.len() {
            return Err("unterminated string".into());
        }
        self.i += 1;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    fn value(&mut self) -> Result<TVal, String> {
        self.ws();
        match self.b.get(self.i) {
            Some(b'"' | b'\'') => self.string().map(TVal::Str),
            Some(b'[') => {
                self.i += 1;
                let mut xs = Vec::new();
                loop {
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b']') => {
                            self.i += 1;
                            break;
                        }
                        Some(b',') => self.i += 1,
                        Some(b'"' | b'\'') => xs.push(self.string()?),
                        _ => return Err("expected a string in the array".into()),
                    }
                }
                Ok(TVal::Arr(xs))
            }
            Some(b'{') => {
                self.i += 1;
                let mut kv = BTreeMap::new();
                loop {
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b'}') => {
                            self.i += 1;
                            break;
                        }
                        Some(b',') => self.i += 1,
                        Some(_) => {
                            let k = self.key()?;
                            self.ws();
                            if self.b.get(self.i) != Some(&b'=') {
                                return Err(format!("expected '=' after {k}"));
                            }
                            self.i += 1;
                            kv.insert(k, self.value()?);
                        }
                        None => return Err("unterminated inline table".into()),
                    }
                }
                Ok(TVal::Table(kv))
            }
            _ => {
                let s = self.i;
                while self.i < self.b.len() && !matches!(self.b[self.i], b',' | b'}' | b']' | b' ') {
                    self.i += 1;
                }
                let w = String::from_utf8_lossy(&self.b[s..self.i]).into_owned();
                if w.is_empty() { Err("expected a value".into()) } else { Ok(TVal::Str(w)) }
            }
        }
    }
}

pub fn strip_comment(line: &str) -> &str {
    let mut q: Option<char> = None;
    for (i, c) in line.char_indices() {
        match (q, c) {
            (None, '#') => return &line[..i],
            (None, '"' | '\'') => q = Some(c),
            (Some(x), c) if c == x => q = None,
            _ => {}
        }
    }
    line
}

pub fn parse_toml(text: &str) -> Result<Vec<TSection>, String> {
    let mut out = vec![TSection { name: String::new(), array: false, kv: BTreeMap::new() }];
    for (n, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let at = |e: String| format!("line {}: {e}", n + 1);
        if let Some(name) = line.strip_prefix("[[").and_then(|l| l.strip_suffix("]]")) {
            out.push(TSection { name: name.trim().into(), array: true, kv: BTreeMap::new() });
        } else if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push(TSection { name: name.trim().into(), array: false, kv: BTreeMap::new() });
        } else {
            let mut p = VP { b: line.as_bytes(), i: 0 };
            let k = p.key().map_err(at)?;
            p.ws();
            if p.b.get(p.i) != Some(&b'=') {
                return Err(at(format!("expected '=' after {k}")));
            }
            p.i += 1;
            let v = p.value().map_err(at)?;
            out.last_mut().unwrap().kv.insert(k, v);
        }
    }
    Ok(out)
}

