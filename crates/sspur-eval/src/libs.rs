//! The interpreter's library primitives (ADR 0037): `__crypto_*`, `__zip_*`, `__csv_*` and
//! `__db_*`, over the same C runtimes native code loads, so both tiers hash, compress, parse and
//! query with one implementation. Everything else in `crypto.*`, `csv.*` and `db.*` is the
//! library prelude, which both tiers run as SSPUR.
use crate::value::Value;
use crate::{trap, Interp, R};
use std::rc::Rc;

fn s(v: &Value) -> R<&str> {
    match v {
        Value::Str(x) => Ok(x),
        v => trap(format!("expected Str, got {v}")),
    }
}

fn int(v: &Value) -> R<i64> {
    match v {
        Value::Int(n) => Ok(*n),
        v => trap(format!("expected Int, got {v}")),
    }
}

fn b(v: &Value) -> R<&[u8]> {
    crate::stdbytes::of(v)
}

fn tuple(xs: Vec<Value>) -> Value {
    Value::Tuple(Rc::new(xs))
}

fn opt(v: Option<Value>) -> Value {
    Value::Opt(v.map(Rc::new))
}

fn rows(rs: Vec<Vec<String>>) -> Value {
    Value::list(rs.into_iter().map(|r| Value::list(r.iter().map(|c| Value::str(c)).collect())).collect())
}

fn strs(v: &Value) -> R<Vec<Vec<String>>> {
    let Value::List(xs) = v else { return trap(format!("expected a list, got {v}")) };
    xs.iter()
        .map(|r| match r {
            Value::List(cs) => cs.iter().map(|c| s(c).map(str::to_string)).collect(),
            v => trap(format!("expected a list, got {v}")),
        })
        .collect()
}

/// A runtime failure the program can't handle: the runtime didn't build or load.
fn rt<T>(r: Result<T, String>) -> R<T> {
    r.or_else(trap)
}

fn cell(v: &sspur_native::rt::SqlVal) -> Value {
    use sspur_native::rt::SqlVal;
    let (k, i, f, t, x) = match v {
        SqlVal::Null => (0, 0, 0.0, String::new(), vec![]),
        SqlVal::Int(n) => (1, *n, 0.0, String::new(), vec![]),
        SqlVal::Real(r) => (2, 0, *r, String::new(), vec![]),
        SqlVal::Text(s) => (3, 0, 0.0, s.clone(), vec![]),
        SqlVal::Blob(b) => (4, 0, 0.0, String::new(), b.clone()),
    };
    tuple(vec![Value::Int(k), Value::Int(i), Value::Float(f), Value::str(&t), Value::bytes(x)])
}

fn param(v: &Value) -> R<sspur_native::rt::SqlVal> {
    use sspur_native::rt::SqlVal;
    let Value::Tuple(t) = v else { return trap(format!("bad SQL parameter {v}")) };
    Ok(match int(&t[0])? {
        1 => SqlVal::Int(int(&t[1])?),
        2 => SqlVal::Real(match &t[2] {
            Value::Float(x) => *x,
            v => return trap(format!("expected F64, got {v}")),
        }),
        3 => SqlVal::Text(s(&t[3])?.to_string()),
        4 => SqlVal::Blob(b(&t[4])?.to_vec()),
        _ => SqlVal::Null,
    })
}

const PRIMS: &[&str] = &["__crypto_hash", "__crypto_hmac", "__crypto_random", "__crypto_same", "__crypto_aead", "__crypto_argon2", "__crypto_ed_public", "__crypto_ed_sign", "__crypto_ed_verify", "__zip_pack", "__zip_unpack", "__zip_file", "__csv_rows", "__csv_text", "__csv_open", "__csv_next", "__csv_close", "__db_connect", "__db_disconnect", "__db_exec", "__db_tx"];

impl Interp {
    pub(crate) fn lib_global(&self, n: &str, a: &[Value]) -> Option<R> {
        if !PRIMS.contains(&n) {
            return None;
        }
        Some(lib_call(n, a))
    }
}

fn lib_call(n: &str, a: &[Value]) -> R {
    use sspur_native::rt;
    Ok(match n {
        "__crypto_hash" => Value::bytes(rt(rt::hash(int(&a[0])?, b(&a[1])?))?),
        "__crypto_hmac" => Value::bytes(rt(rt::hmac(int(&a[0])?, b(&a[1])?, b(&a[2])?))?),
        "__crypto_random" => {
            let k = int(&a[0])?;
            if !(0..=1 << 26).contains(&k) {
                return trap(format!("crypto.random: {k} bytes is out of range"));
            }
            Value::bytes(rt(rt::random(k as usize))?)
        }
        "__crypto_same" => Value::Bool(rt(rt::equal(b(&a[0])?, b(&a[1])?))?),
        "__crypto_aead" => {
            let seal = matches!(a[0], Value::Bool(true));
            opt(rt(rt::aead(seal, b(&a[1])?, b(&a[2])?, b(&a[3])?, b(&a[4])?))?.map(Value::bytes))
        }
        "__crypto_argon2" => opt(rt(rt::argon2(int(&a[0])?, b(&a[1])?, b(&a[2])?, b(&a[3])?, b(&a[4])?, int(&a[5])?, int(&a[6])?, int(&a[7])?, int(&a[8])?))?.map(Value::bytes)),
        "__crypto_ed_public" => Value::bytes(rt(rt::ed_public(b(&a[0])?))?),
        "__crypto_ed_sign" => Value::bytes(rt(rt::ed_sign(b(&a[0])?, b(&a[1])?))?),
        "__crypto_ed_verify" => Value::Bool(rt(rt::ed_verify(b(&a[0])?, b(&a[1])?, b(&a[2])?))?),
        "__zip_pack" => Value::bytes(rt(rt::pack(int(&a[0])?, int(&a[1])?, b(&a[2])?))?),
        "__zip_unpack" => match rt(rt::unpack(int(&a[0])?, b(&a[1])?))? {
            Ok(v) => tuple(vec![Value::Int(0), Value::bytes(v), Value::str("")]),
            Err(e) => tuple(vec![Value::Int(1), Value::bytes(vec![]), Value::str(&e)]),
        },
        "__zip_file" => Value::str(&rt(rt::zip_file(int(&a[0])?, s(&a[1])?, s(&a[2])?))?),
        "__csv_rows" => match rt(rt::csv_parse(s(&a[0])?))? {
            Ok(r) => tuple(vec![Value::Int(0), Value::str(""), rows(r)]),
            Err(e) => tuple(vec![Value::Int(1), Value::str(&e), Value::list(vec![])]),
        },
        "__csv_text" => Value::str(&rt(rt::csv_write(&strs(&a[0])?))?),
        "__csv_open" => match rt(rt::csv_open(s(&a[0])?))? {
            Ok(h) => tuple(vec![Value::Int(h), Value::str("")]),
            Err(e) => tuple(vec![Value::Int(0), Value::str(&e)]),
        },
        "__csv_next" => match rt(rt::csv_next(int(&a[0])?, int(&a[1])?))? {
            Ok(r) => tuple(vec![Value::Int(0), Value::str(""), rows(r)]),
            Err(e) => tuple(vec![Value::Int(1), Value::str(&e), Value::list(vec![])]),
        },
        "__csv_close" => {
            rt(rt::csv_close(int(&a[0])?))?;
            Value::Unit
        }
        "__db_connect" => match rt(rt::db_open(s(&a[0])?))? {
            Ok(id) => tuple(vec![Value::Int(0), Value::Int(id), Value::str("")]),
            Err((k, m)) => tuple(vec![Value::Int(k), Value::Int(0), Value::str(&m)]),
        },
        "__db_disconnect" => {
            rt(rt::db_close(int(&a[0])?))?;
            Value::Unit
        }
        "__db_exec" => {
            let Value::List(ps) = &a[2] else { return trap("bad SQL parameters") };
            let ps: Vec<rt::SqlVal> = ps.iter().map(param).collect::<R<_>>()?;
            match rt(rt::db_exec(int(&a[0])?, s(&a[1])?, &ps, int(&a[3])?))? {
                Ok(o) => tuple(vec![Value::Int(0), Value::str(""), Value::list(o.cols.iter().map(|c| Value::str(c)).collect()), Value::list(o.rows.iter().map(|r| Value::list(r.iter().map(cell).collect())).collect()), Value::Int(o.changes)]),
                Err((k, m)) => tuple(vec![Value::Int(k), Value::str(&m), Value::list(vec![]), Value::list(vec![]), Value::Int(0)]),
            }
        }
        "__db_tx" => match rt(rt::db_tx(int(&a[0])?, int(&a[1])?))? {
            Ok(()) => tuple(vec![Value::Int(0), Value::str("")]),
            Err((k, m)) => tuple(vec![Value::Int(k), Value::str(&m)]),
        },
        _ => return trap(format!("unknown builtin '{n}'")),
    })
}
