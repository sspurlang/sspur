//! The interpreter's two network primitives (ADR 0035): `__net_fetch` and `__net_serve`, over
//! the same runtime library native code loads, so both tiers share its parser and its client.
//! Everything else in `http.*` is the network prelude, which both tiers run as SSPUR.
#![cfg_attr(not(feature = "native"), allow(dead_code))]
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

fn pairs(v: &Value) -> R<Vec<(String, String)>> {
    let Value::List(xs) = v else { return trap(format!("expected a list of pairs, got {v}")) };
    xs.iter()
        .map(|p| match p {
            Value::Tuple(t) if t.len() == 2 => Ok((s(&t[0])?.to_string(), s(&t[1])?.to_string())),
            v => trap(format!("expected a pair, got {v}")),
        })
        .collect()
}

fn strs(v: &Value) -> R<Vec<String>> {
    let Value::List(xs) = v else { return trap(format!("expected a list, got {v}")) };
    xs.iter().map(|x| s(x).map(str::to_string)).collect()
}

fn pair_list(hs: &[(String, String)]) -> Value {
    Value::List(Rc::new(hs.iter().map(|(n, v)| Value::Tuple(Rc::new(vec![Value::str(n), Value::str(v)]))).collect()))
}

fn fetch_result(kind: i64, status: i64, headers: &[(String, String)], body: &str) -> Value {
    Value::Tuple(Rc::new(vec![Value::Int(kind), Value::Int(status), pair_list(headers), Value::str(body)]))
}

/// The line logged when a handler traps or raises, and the response it gets.
pub const FAILED: &str = "{\"error\":\"internal error\"}";

impl Interp {
    pub(crate) fn net_global(&self, n: &str, a: &[Value]) -> Option<R> {
        match n {
            "__net_fetch" => Some(self.net_fetch(a)),
            "__net_serve" => Some(self.net_serve(&a[0], &a[1])),
            _ => None,
        }
    }

    #[cfg(feature = "native")]
    fn net_fetch(&self, a: &[Value]) -> R {
        let headers = pairs(&a[2])?;
        let req = sspur_native::net::Request { method: s(&a[0])?, url: s(&a[1])?, headers: &headers, body: s(&a[3])?, timeout_ms: int(&a[4])?, retries: int(&a[5])?, redirects: int(&a[6])?, download: s(&a[7])? };
        Ok(match sspur_native::net::fetch(&req) {
            Ok(r) => fetch_result(0, r.status, &r.headers, &r.body),
            Err((k, m)) => fetch_result(k, 0, &[], &m),
        })
    }

    #[cfg(not(feature = "native"))]
    fn net_fetch(&self, _: &[Value]) -> R {
        Ok(fetch_result(5, 0, &[], &format!("networking is {}", crate::web::UNAVAILABLE)))
    }

    #[cfg(not(feature = "native"))]
    fn net_serve(&self, _: &Value, _: &Value) -> R {
        trap(format!("serving HTTP is {}", crate::web::UNAVAILABLE))
    }

    #[cfg(feature = "native")]
    fn net_serve(&self, cfg: &Value, h: &Value) -> R {
        use sspur_native::net::{Next, Server, ServerCfg};
        let Value::Tuple(c) = cfg else { return trap("bad server config") };
        let flag = |v: &Value| -> R<bool> {
            match v {
                Value::Bool(b) => Ok(*b),
                v => trap(format!("expected Bool, got {v}")),
            }
        };
        let host = s(&c[1])?.to_string();
        let cors = strs(&c[6])?;
        let files = pairs(&c[8])?;
        let sc = ServerCfg {
            host: &host,
            port: int(&c[0])?,
            max_body: int(&c[3])?,
            timeout_ms: int(&c[4])?,
            max_requests: int(&c[5])?,
            gzip: flag(&c[7])?,
            access_log: flag(&c[12])?,
            cors: &cors,
            files: &files,
            health: s(&c[9])?,
            cert: s(&c[10])?,
            key: s(&c[11])?,
        };
        let server = match Server::listen(&sc) {
            Ok(sv) => sv,
            Err(e) => return trap(format!("http.serve: {e}")),
        };
        let scheme = if server.is_tls() { "https" } else { "http" };
        let shown = if host.contains(':') { format!("[{host}]") } else { host.clone() };
        sspur_native::emit_log(&format!("info: listening on {scheme}://{shown}:{}", server.port()));
        loop {
            match server.next(-1) {
                Next::Done => break,
                Next::Idle => continue,
                Next::Call(call) => {
                    let args = vec![Value::str(&call.method), Value::str(&call.url), pair_list(&call.headers), Value::str(&call.body), Value::Int(call.timeout_ms)];
                    let line = format!("{} {}", call.method, call.url);
                    let out = self.apply(h, args).and_then(|r| match r {
                        Value::Tuple(t) if t.len() == 3 => Ok((int(&t[0])?, pairs(&t[1])?, s(&t[2])?.to_string())),
                        v => trap(format!("bad handler result {v}")),
                    });
                    sspur_native::rt::db_release_thread();
                    match out {
                        Ok((status, headers, body)) => call.reply(status, &headers, &body),
                        Err(e) => {
                            sspur_native::emit_log(&format!("error: {line} failed: {}", crate::describe(e)));
                            call.reply(500, &[("content-type".into(), "application/json".into())], FAILED);
                        }
                    }
                }
            }
        }
        drop(server);
        Ok(Value::Unit)
    }
}
