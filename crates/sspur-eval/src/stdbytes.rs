//! `Bytes`: an immutable byte string, for crypto, compression and binary files.
use crate::value::Value;
use crate::{trap, R};
use sspur_native::bytes as b64;
use std::rc::Rc;

fn int(v: &Value) -> R<i64> {
    match v {
        Value::Int(n) => Ok(*n),
        v => trap(format!("expected Int, got {v}")),
    }
}

fn opt(v: Option<Value>) -> Value {
    Value::Opt(v.map(Rc::new))
}

pub(crate) fn of(v: &Value) -> R<&[u8]> {
    match v {
        Value::Bytes(b) => Ok(&b.0),
        v => trap(format!("expected Bytes, got {v}")),
    }
}

pub(crate) fn method(name: &str, b: &[u8], a: &[Value]) -> R {
    Ok(match name {
        "len" => Value::Int(b.len() as i64),
        "is_empty" => Value::Bool(b.is_empty()),
        "get" => {
            let i = int(&a[0])?;
            opt(usize::try_from(i).ok().and_then(|i| b.get(i)).map(|&x| Value::Int(i64::from(x))))
        }
        "slice" => {
            let clampi = |v: i64| v.clamp(0, b.len() as i64) as usize;
            let (from, to) = (clampi(int(&a[0])?), clampi(int(&a[1])?));
            Value::bytes(if to > from { b[from..to].to_vec() } else { vec![] })
        }
        "concat" => {
            let c = of(&a[0])?;
            if b.len() + c.len() > 1 << 31 {
                return trap("out of memory");
            }
            let mut v = Vec::with_capacity(b.len() + c.len());
            v.extend_from_slice(b);
            v.extend_from_slice(c);
            Value::bytes(v)
        }
        "to_list" => Value::list(b.iter().map(|&x| Value::Int(i64::from(x))).collect()),
        "to_str" => opt(std::str::from_utf8(b).ok().map(Value::str)),
        "hex" => Value::str(&b64::hex(b)),
        "base64" => Value::str(&b64::base64(b, false)),
        "base64url" => Value::str(&b64::base64(b, true)),
        _ => return trap(format!("no method '{name}' on Bytes")),
    })
}

/// `bytes(xs)`: each element must be in `0..=255`.
pub(crate) fn from_list(xs: &[Value]) -> R {
    let mut v = Vec::with_capacity(xs.len());
    for x in xs {
        let n = int(x)?;
        match u8::try_from(n) {
            Ok(b) => v.push(b),
            Err(_) => return trap(format!("bytes: {n} is not a byte (0 to 255)")),
        }
    }
    Ok(Value::bytes(v))
}

pub(crate) fn global(n: &str, a: &[Value]) -> R<Option<Value>> {
    let text = |v: &Value| -> R<String> {
        match v {
            Value::Str(s) => Ok(s.to_string()),
            v => trap(format!("expected Str, got {v}")),
        }
    };
    Ok(Some(match n {
        "bytes" => match &a[0] {
            Value::List(xs) => from_list(xs)?,
            v => return trap(format!("expected a list, got {v}")),
        },
        "from_hex" => opt(b64::from_hex(&text(&a[0])?).map(Value::bytes)),
        "from_base64" => opt(b64::from_base64(&text(&a[0])?, false).map(Value::bytes)),
        "from_base64url" => opt(b64::from_base64(&text(&a[0])?, true).map(Value::bytes)),
        _ => return Ok(None),
    }))
}
