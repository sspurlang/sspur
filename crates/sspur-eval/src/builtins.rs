use crate::value::Value;
use crate::{trap, Ctrl, Interp, R};
use std::collections::BTreeMap;
use std::rc::Rc;

const GLOBALS: &[&str] = &["log", "some", "ok", "err", "empty_map", "min", "max", "secret", "pii", "untrusted", "guess", "atomic", "chan", "__cmp", "__hash"];

pub fn is_global(n: &str) -> bool {
    GLOBALS.contains(&n) || sspur_check::is_std_global(n)
}

fn int(v: &Value) -> R<i64> {
    match v {
        Value::Int(n) => Ok(*n),
        v => trap(format!("expected Int, got {v}")),
    }
}

fn s(v: &Value) -> R<&str> {
    match v {
        Value::Str(x) => Ok(x),
        v => trap(format!("expected Str, got {v}")),
    }
}

fn boolean(v: Value) -> R<bool> {
    match v {
        Value::Bool(b) => Ok(b),
        v => trap(format!("expected Bool, got {v}")),
    }
}

fn index(n: i64, len: usize) -> usize {
    n.clamp(0, len as i64) as usize
}

impl Interp {
    pub(crate) fn call_global(&self, n: &str, mut a: Vec<Value>) -> R {
        Ok(match n {
            "log" => return self.perform("log", a),
            "some" => Value::some(a.remove(0)),
            "ok" => Value::Res(Ok(Rc::new(a.remove(0)))),
            "err" => Value::Res(Err(Rc::new(a.remove(0)))),
            "empty_map" => Value::Map(Rc::new(BTreeMap::new())),
            "atomic" => self.new_atomic(int(&a[0])?),
            "chan" => self.new_chan(),
            "secret" => Value::Wrap("Secret".into(), Rc::new(a.remove(0))),
            "pii" => Value::Wrap("Pii".into(), Rc::new(a.remove(0))),
            "untrusted" => Value::Wrap("Untrusted".into(), Rc::new(a.remove(0))),
            "guess" => {
                let v = a.remove(0);
                let Value::Float(c) = a[0] else { return trap("guess confidence must be F64") };
                if !(0.0..=1.0).contains(&c) {
                    return trap(format!("guess confidence {c} is outside [0, 1]"));
                }
                Value::Guess(Rc::new(v), c)
            }
            "min" => {
                let (x, y) = (a.remove(0), a.remove(0));
                if y < x { y } else { x }
            }
            "max" => {
                let (x, y) = (a.remove(0), a.remove(0));
                if y > x { y } else { x }
            }
            "__cmp" => Value::Int(a[0].cmp(&a[1]) as i64),
            "__hash" => Value::Int(s(&a[0])?.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3)) as i64),
            _ if self.ops.contains(n) => return self.perform(n, a),
            _ => return self.std_global(n, a),
        })
    }

    pub(crate) fn call_method(&self, name: &str, recv: Value, a: Vec<Value>) -> R {
        #[cfg(not(target_family = "wasm"))]
        let mut a = a;
        if name == "str" {
            return Ok(Value::str(&recv.to_string()));
        }
        match recv {
            #[cfg(not(target_family = "wasm"))]
            Value::List(mut xs) if name == "push" => {
                Rc::make_mut(&mut xs).push(a.remove(0));
                Ok(Value::List(xs))
            }
            #[cfg(not(target_family = "wasm"))]
            Value::List(mut xs) if name == "concat" => match a.remove(0) {
                Value::List(ys) => {
                    Rc::make_mut(&mut xs).extend(ys.iter().cloned());
                    Ok(Value::List(xs))
                }
                v => trap(format!("concat expects a List, got {v}")),
            },
            #[cfg(not(target_family = "wasm"))]
            Value::Map(mut m) if name == "put" => {
                let k = a.remove(0);
                Rc::make_mut(&mut m).insert(k, a.remove(0));
                Ok(Value::Map(m))
            }
            #[cfg(not(target_family = "wasm"))]
            Value::Set(mut st) if name == "add" => {
                Rc::make_mut(&mut st).insert(a.remove(0));
                Ok(Value::Set(st))
            }
            #[cfg(not(target_family = "wasm"))]
            Value::Buf(mut b) => match name {
                "add" => {
                    Rc::make_mut(&mut b).push_str(s(&a[0])?);
                    Ok(Value::Buf(b))
                }
                "byte_len" => Ok(Value::Int(b.len() as i64)),
                _ => str_method(name, &b, a),
            },
            Value::List(xs) => self.list_method(name, &xs, a),
            Value::Str(x) => str_method(name, &x, a),
            Value::Opt(o) => self.opt_method(name, o, a),
            Value::Res(r) => match name {
                "is_ok" => Ok(Value::Bool(r.is_ok())),
                "is_err" => Ok(Value::Bool(r.is_err())),
                "or" => Ok(r.map_or_else(|_| a[0].clone(), |v| (*v).clone())),
                "map" => Ok(Value::Res(match r {
                    Ok(v) => Ok(Rc::new(self.apply(&a[0], vec![(*v).clone()])?)),
                    Err(e) => Err(e),
                })),
                "and_then" => match r {
                    Ok(v) => self.apply(&a[0], vec![(*v).clone()]),
                    Err(e) => Ok(Value::Res(Err(e))),
                },
                "get" => match r {
                    Ok(v) => Ok((*v).clone()),
                    Err(e) => Err(Ctrl::Raise((*e).clone())),
                },
                _ => trap(format!("no method '{name}' on Res")),
            },
            Value::Map(m) => map_method(name, &m, a),
            Value::Set(st) => self.set_method(name, &st, a),
            Value::Heap(h) => self.heap_method(name, &h, a),
            Value::Time(t) => match crate::stdtz::time_method(name, t, &a)? {
                Some(v) => Ok(v),
                None => crate::stdtime::time_method(name, t, &a),
            },
            Value::Dur(d) => crate::stdtime::dur_method(name, d, &a),
            Value::Bits(n, w) => crate::stdx::bits_method(name, n, &w, &a),
            #[cfg(not(target_family = "wasm"))]
            Value::Bytes(b) => crate::stdbytes::method(name, &b.0, &a),
            Value::View(n) => self.view_method(name, n, a),
            Value::Record(n, fs) if &*n == "#Rng" => crate::stdrng::method(name, &fs, &a),
            Value::Record(n, fs) if &*n == "#Complex" => crate::stdcx::method(name, &fs, &a),
            Value::Record(n, fs) if &*n == "#Ratio" => crate::stdratio::method(name, &fs, &a),
            Value::Record(n, fs) if &*n == "#Locale" => crate::stdlocale::method(name, &fs, &a),
            Value::Record(ref n, _) if &**n == "#Zone" => crate::stdtz::method(name, &recv, &a),
            Value::Record(ref n, _) if &**n == "#File" => self.file_method(name, &recv, &a),
            Value::Record(n, fs) if &*n == "#FlatMap" => crate::stdflat::flat_method(name, &fs, &a),
            Value::Record(n, fs) if &*n == "#MdSpan" => crate::stdflat::md_method(name, &fs, &a),
            Value::Big(b) => crate::stdbig::big_method(name, &b, &a),
            Value::Regex(r) => crate::stdre::method(name, &r, &a),
            Value::Dec(m, s) => crate::stdbig::dec_method(name, &m, s, &a),
            Value::Dev(d) => crate::kernel::dev_method(name, &d, &a),
            Value::Atomic(c) => Ok(match name {
                "load" => Value::Int(c.v.get()),
                "store" => {
                    c.v.set(int(&a[0])?);
                    Value::Unit
                }
                "add" => match c.v.get().checked_add(int(&a[0])?) {
                    Some(v) => {
                        c.v.set(v);
                        Value::Unit
                    }
                    None => return trap("integer overflow"),
                },
                "cas" => {
                    let ok = c.v.get() == int(&a[0])?;
                    if ok {
                        c.v.set(int(&a[1])?);
                    }
                    Value::Bool(ok)
                }
                _ => return trap(format!("no method '{name}' on Atomic")),
            }),
            Value::Chan(c) => match name {
                "send" => self.chan_send(&c, a[0].clone()),
                "recv" => Ok(Value::Opt(self.chan_recv(&c)?.map(Rc::new))),
                "close" => self.chan_close(&c),
                _ => trap(format!("no method '{name}' on Chan")),
            },
            Value::Wrap(kind, inner) => match name {
                "map" => Ok(Value::Wrap(kind, Rc::new(self.apply(&a[0], vec![(*inner).clone()])?))),
                "check" => self.apply(&a[0], vec![(*inner).clone()]),
                "expose" | "trust" => Ok((*inner).clone()),
                "validate" => self.apply(&a[0], vec![(*inner).clone()]),
                _ => trap(format!("no method '{name}' on {kind}")),
            },
            Value::Guess(inner, c) => match name {
                "conf" => Ok(Value::Float(c)),
                "map" => Ok(Value::Guess(Rc::new(self.apply(&a[0], vec![(*inner).clone()])?), c)),
                "verify" => Ok(if boolean(self.apply(&a[0], vec![(*inner).clone()])?)? { Value::some((*inner).clone()) } else { Value::Opt(None) }),
                "at_least" => match a[0] {
                    Value::Float(min) => Ok(if c >= min { Value::some((*inner).clone()) } else { Value::Opt(None) }),
                    _ => trap("at_least expects F64"),
                },
                "accept" => Ok((*inner).clone()),
                _ => trap(format!("no method '{name}' on Guess")),
            },
            Value::Int(n) => match name {
                "abs" => n.checked_abs().map(Value::Int).map_or_else(|| trap("integer overflow"), Ok),
                "to_f64" => Ok(Value::Float(n as f64)),
                "band" => Ok(Value::Int(n & int(&a[0])?)),
                "bor" => Ok(Value::Int(n | int(&a[0])?)),
                "bxor" => Ok(Value::Int(n ^ int(&a[0])?)),
                "shl" => Ok(Value::Int(u32::try_from(int(&a[0])?).ok().filter(|k| *k < 64).map_or(0, |k| ((n as u64) << k) as i64))),
                "shr" => Ok(Value::Int(u32::try_from(int(&a[0])?).ok().filter(|k| *k < 64).map_or(0, |k| ((n as u64) >> k) as i64))),
                _ => crate::stdlib::std_int(name, n, a),
            },
            Value::Float(x) => match name {
                "abs" => Ok(Value::Float(x.abs())),
                "round" => crate::stdlib::f2i(x.round()),
                "floor" => crate::stdlib::f2i(x.floor()),
                "sqrt" => Ok(Value::Float(x.sqrt())),
                _ => crate::stdlib::std_float(name, x, a),
            },
            v => trap(format!("no method '{name}' on {v}")),
        }
    }

    fn list_method(&self, name: &str, xs: &Rc<Vec<Value>>, mut a: Vec<Value>) -> R {
        let f = a.first().cloned();
        let call = |x: Value| self.apply(f.as_ref().unwrap(), vec![x]);
        Ok(match name {
            "len" => Value::Int(xs.len() as i64),
            "is_empty" => Value::Bool(xs.is_empty()),
            "map" => Value::list(xs.iter().map(|x| call(x.clone())).collect::<R<_>>()?),
            "flat_map" => {
                let mut out = Vec::new();
                for x in xs.iter() {
                    match call(x.clone())? {
                        Value::List(ys) => out.extend(ys.iter().cloned()),
                        v => return trap(format!("flat_map expects a List, got {v}")),
                    }
                }
                Value::list(out)
            }
            "filter" => {
                let mut out = Vec::new();
                for x in xs.iter() {
                    if boolean(call(x.clone())?)? {
                        out.push(x.clone());
                    }
                }
                Value::list(out)
            }
            "fold" => {
                let mut acc = a.remove(0);
                let g = a.remove(0);
                for x in xs.iter() {
                    acc = self.apply(&g, vec![acc, x.clone()])?;
                }
                acc
            }
            "any" | "all" | "find" => {
                for x in xs.iter() {
                    let hit = boolean(call(x.clone())?)?;
                    match (name, hit) {
                        ("any", true) => return Ok(Value::Bool(true)),
                        ("all", false) => return Ok(Value::Bool(false)),
                        ("find", true) => return Ok(Value::some(x.clone())),
                        _ => {}
                    }
                }
                match name {
                    "any" => Value::Bool(false),
                    "all" => Value::Bool(true),
                    _ => Value::Opt(None),
                }
            }
            "sort_by" => {
                let mut keyed: Vec<(Value, Value)> = xs.iter().map(|x| Ok((call(x.clone())?, x.clone()))).collect::<R<_>>()?;
                keyed.sort_by(|a, b| a.0.cmp(&b.0));
                Value::list(keyed.into_iter().map(|(_, x)| x).collect())
            }
            "sum" => {
                if xs.iter().all(|x| matches!(x, Value::Float(_))) && !xs.is_empty() {
                    Value::Float(xs.iter().map(|x| if let Value::Float(f) = x { *f } else { 0.0 }).sum())
                } else {
                    let mut t: i64 = 0;
                    for x in xs.iter() {
                        t = t.checked_add(int(x)?).map_or_else(|| trap("integer overflow"), Ok)?;
                    }
                    Value::Int(t)
                }
            }
            "push" => {
                let mut v = (**xs).clone();
                v.push(a.remove(0));
                Value::list(v)
            }
            "concat" => match a.remove(0) {
                Value::List(ys) => Value::list(xs.iter().chain(ys.iter()).cloned().collect()),
                v => return trap(format!("concat expects a List, got {v}")),
            },
            "take" => Value::list(xs[..index(int(&a[0])?, xs.len())].to_vec()),
            "drop" => Value::list(xs[index(int(&a[0])?, xs.len())..].to_vec()),
            "reverse" => Value::list(xs.iter().rev().cloned().collect()),
            "sort" => {
                let mut v = (**xs).clone();
                v.sort();
                Value::list(v)
            }
            "unique" => {
                let mut seen = std::collections::BTreeSet::new();
                Value::list(xs.iter().filter(|x| seen.insert((*x).clone())).cloned().collect())
            }
            "contains" => Value::Bool(xs.contains(&a[0])),
            "first" => xs.first().cloned().map_or(Value::Opt(None), Value::some),
            "last" => xs.last().cloned().map_or(Value::Opt(None), Value::some),
            "get" => {
                let i = int(&a[0])?;
                if i < 0 { Value::Opt(None) } else { xs.get(i as usize).cloned().map_or(Value::Opt(None), Value::some) }
            }
            "min" => xs.iter().min().cloned().map_or(Value::Opt(None), Value::some),
            "max" => xs.iter().max().cloned().map_or(Value::Opt(None), Value::some),
            "counts" => {
                let mut order: Vec<Value> = Vec::new();
                let mut n: BTreeMap<Value, i64> = BTreeMap::new();
                for x in xs.iter() {
                    let c = n.entry(x.clone()).or_insert(0);
                    if *c == 0 {
                        order.push(x.clone());
                    }
                    *c += 1;
                }
                Value::list(order.into_iter().map(|k| Value::Tuple(Rc::new(vec![k.clone(), Value::Int(n[&k])]))).collect())
            }
            "zip" => match a.remove(0) {
                Value::List(ys) => Value::list(xs.iter().zip(ys.iter()).map(|(x, y)| Value::Tuple(Rc::new(vec![x.clone(), y.clone()]))).collect()),
                v => return trap(format!("zip expects a List, got {v}")),
            },
            "enumerate" => Value::list(xs.iter().enumerate().map(|(i, x)| Value::Tuple(Rc::new(vec![Value::Int(i as i64), x.clone()]))).collect()),
            "join" => {
                let sep = s(&a[0])?;
                let parts: Vec<String> = xs.iter().map(|x| x.to_string()).collect();
                Value::str(&parts.join(sep))
            }
            _ => return self.std_list(name, xs, a),
        })
    }

    fn opt_method(&self, name: &str, o: Option<Rc<Value>>, mut a: Vec<Value>) -> R {
        Ok(match name {
            "or" => o.map_or_else(|| a.remove(0), |v| (*v).clone()),
            "is_some" => Value::Bool(o.is_some()),
            "get" => match o {
                Some(v) => (*v).clone(),
                None => return trap("unwrapped none with .get; check with is_some, match on some/none, or use .or(default)"),
            },
            "is_none" => Value::Bool(o.is_none()),
            "map" => match o {
                Some(v) => Value::some(self.apply(&a[0], vec![(*v).clone()])?),
                None => Value::Opt(None),
            },
            "and_then" => match o {
                Some(v) => self.apply(&a[0], vec![(*v).clone()])?,
                None => Value::Opt(None),
            },
            "ok_or" => match o {
                Some(v) => (*v).clone(),
                None => return Err(Ctrl::Raise(a.remove(0))),
            },
            _ => return trap(format!("no method '{name}' on Opt")),
        })
    }
}

fn str_method(name: &str, x: &str, a: Vec<Value>) -> R {
    let strs = |it: Vec<&str>| Value::list(it.into_iter().map(Value::str).collect());
    Ok(match name {
        "len" => Value::Int(x.chars().count() as i64),
        "is_empty" => Value::Bool(x.is_empty()),
        "byte_len" => Value::Int(x.len() as i64),
        "byte" => {
            let i = int(&a[0])?;
            match usize::try_from(i).ok().and_then(|k| x.as_bytes().get(k)) {
                Some(b) => Value::Int(i64::from(*b)),
                None => return trap(format!("byte index {i} out of bounds for a string of {} bytes", x.len())),
            }
        }
        "take" => Value::str(&x.chars().take(int(&a[0])?.max(0) as usize).collect::<String>()),
        "drop" => Value::str(&x.chars().skip(int(&a[0])?.max(0) as usize).collect::<String>()),
        "reverse" => Value::str(&x.chars().rev().collect::<String>()),
        "get" => {
            let i = int(&a[0])?;
            if i < 0 { Value::Opt(None) } else { x.chars().nth(i as usize).map_or(Value::Opt(None), |c| Value::some(Value::str(&c.to_string()))) }
        }
        "first" => x.chars().next().map_or(Value::Opt(None), |c| Value::some(Value::str(&c.to_string()))),
        "last" => x.chars().last().map_or(Value::Opt(None), |c| Value::some(Value::str(&c.to_string()))),
        "lower" => Value::str(&x.to_lowercase()),
        "upper" => Value::str(&x.to_uppercase()),
        "fold_case" => Value::str(&x.to_uppercase().to_lowercase()),
        "compare_ci" => match &a[0] {
            Value::Str(y) => Value::Int(match x.to_uppercase().to_lowercase().cmp(&y.to_uppercase().to_lowercase()) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }),
            v => return trap(format!("expected Str, got {v}")),
        },
        "trim" => Value::str(x.trim()),
        "split" => strs(x.split(s(&a[0])?).collect()),
        "words" => strs(x.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect()),
        "chars" => Value::list(x.chars().map(|c| Value::str(&c.to_string())).collect()),
        "contains" => Value::Bool(x.contains(s(&a[0])?)),
        "starts_with" => Value::Bool(x.starts_with(s(&a[0])?)),
        "ends_with" => Value::Bool(x.ends_with(s(&a[0])?)),
        "replace" => Value::str(&x.replace(s(&a[0])?, s(&a[1])?)),
        "repeat" => {
            let n = int(&a[0])?;
            if n < 0 {
                return trap("repeat count must be >= 0");
            }
            if !x.is_empty() && n > (1i64 << 28) / x.len() as i64 {
                return trap("out of memory");
            }
            Value::str(&x.repeat(n as usize))
        }
        "to_int" => x.trim().parse::<i64>().map_or(Value::Opt(None), |n| Value::some(Value::Int(n))),
        "is_alpha" => Value::Bool(!x.is_empty() && x.chars().all(char::is_alphabetic)),
        _ => return crate::stdlib::std_str(name, x, a),
    })
}

fn map_method(name: &str, m: &Rc<BTreeMap<Value, Value>>, mut a: Vec<Value>) -> R {
    let pair = |k: &Value, v: &Value| Value::Tuple(Rc::new(vec![k.clone(), v.clone()]));
    Ok(match name {
        "get" => m.get(&a[0]).cloned().map_or(Value::Opt(None), Value::some),
        "put" => {
            let mut n = (**m).clone();
            let k = a.remove(0);
            n.insert(k, a.remove(0));
            Value::Map(Rc::new(n))
        }
        "remove" => {
            let mut n = (**m).clone();
            n.remove(&a[0]);
            Value::Map(Rc::new(n))
        }
        "has" => Value::Bool(m.contains_key(&a[0])),
        "keys" => Value::list(m.keys().cloned().collect()),
        "values" => Value::list(m.values().cloned().collect()),
        "items" => Value::list(m.iter().map(|(k, v)| pair(k, v)).collect()),
        "len" => Value::Int(m.len() as i64),
        "is_empty" => Value::Bool(m.is_empty()),
        _ => return trap(format!("no method '{name}' on Map")),
    })
}
