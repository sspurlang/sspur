use crate::{iam, migrate, Service, Store};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

pub type Sink = Arc<dyn Fn(&str) + Send + Sync>;

pub struct Req {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Req {
    fn header(&self, k: &str) -> Option<&str> {
        self.headers.iter().find(|(h, _)| h.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }
}

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    fn json(status: u16, v: &Value) -> Resp {
        Resp { status, headers: vec![("content-type".into(), "application/json".into())], body: v.to_string().into_bytes() }
    }
}

fn read_req(s: &mut TcpStream) -> std::io::Result<Option<Req>> {
    let mut r = BufReader::new(s);
    let mut line = String::new();
    if r.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let len = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse::<usize>().ok()).unwrap_or(0);
    if len > 6 << 20 {
        return Err(std::io::Error::other("request too large"));
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };
    Ok(Some(Req { method, path, query, headers, body }))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "Status",
    }
}

fn write_resp(s: &mut TcpStream, r: &Resp) -> std::io::Result<()> {
    let mut out = format!("HTTP/1.1 {} {}\r\n", r.status, reason(r.status));
    for (k, v) in &r.headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!("content-length: {}\r\nconnection: close\r\n\r\n", r.body.len()));
    s.write_all(out.as_bytes())?;
    s.write_all(&r.body)?;
    s.flush()
}

type Handle = Arc<dyn Fn(Req) -> Resp + Send + Sync>;

fn serve(l: TcpListener, h: Handle) {
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let h = h.clone();
            std::thread::spawn(move || {
                if let Ok(Some(req)) = read_req(&mut s) {
                    let resp = h(req);
                    let _ = write_resp(&mut s, &resp);
                }
                // a child spawned between accept and FD_CLOEXEC holds a copy of the socket, so closing alone may never send FIN
                let _ = s.shutdown(Shutdown::Both);
            });
        }
    });
}

fn bind(port: u16) -> Result<(TcpListener, u16), String> {
    let l = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("cannot bind 127.0.0.1:{port}: {e}"))?;
    let p = l.local_addr().map_err(|e| e.to_string())?.port();
    Ok((l, p))
}

struct Grant {
    who: String,
    acts: BTreeSet<(String, String)>,
    fun: Arc<Fun>,
}

#[derive(Default)]
pub(crate) struct Known {
    pub complete: bool,
    pub keys: BTreeSet<String>,
}

type ReplayState = (HashMap<String, Known>, Vec<String>);

pub struct Dynamo {
    tables: Mutex<HashMap<String, BTreeMap<String, Value>>>,
    grants: Mutex<HashMap<String, Grant>>,
    pub calls: Mutex<Vec<(String, String, String)>>,
    pub page: usize,
    pub delay_ms: AtomicU64,
    tracing: AtomicBool,
    trace: Mutex<HashMap<String, Vec<Value>>>,
    replay: Mutex<Option<ReplayState>>,
}

fn ddb_err(kind: &str, msg: &str) -> Resp {
    Resp { status: 400, headers: vec![("content-type".into(), "application/x-amz-json-1.0".into())], body: json!({"__type": format!("com.amazonaws.dynamodb.v20120810#{kind}"), "message": msg}).to_string().into_bytes() }
}

impl Dynamo {
    fn new(page: usize) -> Dynamo {
        Dynamo {
            tables: Mutex::new(HashMap::new()),
            grants: Mutex::new(HashMap::new()),
            calls: Mutex::new(Vec::new()),
            page,
            delay_ms: AtomicU64::new(0),
            tracing: AtomicBool::new(false),
            trace: Mutex::new(HashMap::new()),
            replay: Mutex::new(None),
        }
    }

    pub fn items(&self, table: &str) -> BTreeMap<String, Value> {
        self.tables.lock().unwrap().get(table).cloned().unwrap_or_default()
    }

    fn take_trace(&self, id: &str) -> Vec<Value> {
        self.trace.lock().unwrap().remove(id).unwrap_or_default()
    }

    fn reset(&self, seed: HashMap<String, BTreeMap<String, Value>>, known: HashMap<String, Known>) {
        let mut t = self.tables.lock().unwrap();
        for (name, items) in t.iter_mut() {
            *items = seed.get(name).cloned().unwrap_or_default();
        }
        *self.replay.lock().unwrap() = Some((known, Vec::new()));
    }

    fn misses(&self) -> Vec<String> {
        self.replay.lock().unwrap().as_mut().map(|r| std::mem::take(&mut r.1)).unwrap_or_default()
    }

    fn handle(&self, req: &Req) -> Resp {
        let auth = req.header("authorization").unwrap_or("");
        let Some(akid) = auth.strip_prefix("AWS4-HMAC-SHA256 Credential=").and_then(|r| r.split('/').next()).filter(|_| auth.contains("SignedHeaders=") && auth.contains("Signature=")) else {
            return ddb_err("MissingAuthenticationTokenException", "request is not SigV4 signed");
        };
        let (who, allowed, rid) = {
            let grants = self.grants.lock().unwrap();
            let Some(g) = grants.get(akid) else {
                return ddb_err("UnrecognizedClientException", "the security token included in the request is invalid");
            };
            (g.who.clone(), g.acts.clone(), g.fun.current.lock().unwrap().clone())
        };
        let Some(op) = req.header("x-amz-target").and_then(|t| t.strip_prefix("DynamoDB_20120810.")) else {
            return ddb_err("UnknownOperationException", "missing X-Amz-Target");
        };
        let Ok(body) = serde_json::from_slice::<Value>(&req.body) else {
            return ddb_err("SerializationException", "body is not JSON");
        };
        let delay = self.delay_ms.load(Ordering::Relaxed);
        if delay > 0 {
            std::thread::sleep(Duration::from_millis(delay));
        }
        let table = body["TableName"].as_str().unwrap_or("").to_string();
        self.calls.lock().unwrap().push((who.clone(), format!("dynamodb:{op}"), table.clone()));
        if !allowed.contains(&(op.to_string(), table.clone())) {
            return ddb_err("AccessDeniedException", &format!("User: arn:local:sts::000000000000:assumed-role/{who} is not authorized to perform: dynamodb:{op} on resource: table/{table}"));
        }
        let out = {
            let mut tables = self.tables.lock().unwrap();
            let Some(t) = tables.get_mut(&table) else {
                return ddb_err("ResourceNotFoundException", "Requested resource not found");
            };
            match self.apply(op, &body, &table, t) {
                Ok(v) => v,
                Err(r) => return r,
            }
        };
        if self.tracing.load(Ordering::Relaxed)
            && let Some(id) = rid
        {
            let mut req = body.clone();
            if let Some(o) = req.as_object_mut() {
                o.remove("TableName");
            }
            self.trace.lock().unwrap().entry(id).or_default().push(json!({"op": op, "table": table, "req": req, "resp": out}));
        }
        Resp { status: 200, headers: vec![("content-type".into(), "application/x-amz-json-1.0".into())], body: out.to_string().into_bytes() }
    }

    fn note_miss(&self, table: &str, what: impl FnOnce(&Known) -> Option<String>) {
        if let Some((known, misses)) = self.replay.lock().unwrap().as_mut() {
            let k = known.entry(table.to_string()).or_default();
            if let Some(m) = what(k) {
                misses.push(m);
            }
        }
    }

    fn apply(&self, op: &str, body: &Value, table: &str, t: &mut BTreeMap<String, Value>) -> Result<Value, Resp> {
        let key = |v: &Value| v["pk"].to_string();
        Ok(match op {
            "GetItem" => {
                let k = key(&body["Key"]);
                self.note_miss(table, |kn| (!kn.complete && !kn.keys.contains(&k)).then(|| format!("GetItem {table} {k}")));
                t.get(&k).map_or(json!({}), |it| json!({"Item": it}))
            }
            "PutItem" => {
                if body["Item"]["pk"].is_null() {
                    return Err(ddb_err("ValidationException", "One of the required keys was not given a value"));
                }
                let k = key(&body["Item"]);
                if let Some(cond) = body["ConditionExpression"].as_str() {
                    let Some((n, v)) = cond.split_once(" = ") else {
                        return Err(ddb_err("ValidationException", "the emulator supports only `#name = :value` conditions"));
                    };
                    let attr = body["ExpressionAttributeNames"][n.trim()].as_str().unwrap_or("");
                    let want = &body["ExpressionAttributeValues"][v.trim()];
                    if t.get(&k).map(|it| &it[attr]) != Some(want) {
                        return Err(ddb_err("ConditionalCheckFailedException", "The conditional request failed"));
                    }
                }
                t.insert(k, body["Item"].clone());
                json!({})
            }
            "DeleteItem" => {
                let k = key(&body["Key"]);
                self.note_miss(table, |kn| (!kn.complete && !kn.keys.contains(&k)).then(|| format!("DeleteItem {table} {k}")));
                match t.remove(&k) {
                    Some(old) if body["ReturnValues"] == "ALL_OLD" => json!({"Attributes": old}),
                    _ => json!({}),
                }
            }
            "Scan" => {
                self.note_miss(table, |kn| (!kn.complete).then(|| format!("Scan {table}")));
                let start = body.get("ExclusiveStartKey").map(key);
                let page = body["Limit"].as_u64().map_or(self.page, |l| (l as usize).clamp(1, self.page.max(1) * 50));
                let items: Vec<(&String, &Value)> = t.iter().filter(|(k, _)| start.as_ref().is_none_or(|s| *k > s)).take(page + 1).collect();
                let more = items.len() > page;
                let page: Vec<Value> = items.iter().take(page).map(|(_, v)| (*v).clone()).collect();
                let mut r = json!({"Items": page, "Count": page.len(), "ScannedCount": page.len()});
                if more {
                    r["LastEvaluatedKey"] = json!({"pk": page.last().map(|v| v["pk"].clone())});
                }
                r
            }
            _ => return Err(ddb_err("UnknownOperationException", op)),
        })
    }
}

type Reply = Sender<Result<String, String>>;

struct Fun {
    name: String,
    queue: Mutex<VecDeque<(String, String, Reply)>>,
    ready: Condvar,
    inflight: Mutex<HashMap<String, Reply>>,
    current: Mutex<Option<String>>,
    init_error: Mutex<Option<String>>,
    polled: AtomicBool,
}

impl Fun {
    fn new(name: &str) -> Fun {
        Fun { name: name.to_string(), queue: Mutex::new(VecDeque::new()), ready: Condvar::new(), inflight: Mutex::new(HashMap::new()), current: Mutex::new(None), init_error: Mutex::new(None), polled: AtomicBool::new(false) }
    }

    fn handle(&self, req: Req) -> Resp {
        let p = req.path.trim_start_matches("/2018-06-01/runtime/");
        if req.method == "GET" && p == "invocation/next" {
            self.polled.store(true, Ordering::SeqCst);
            let mut q = self.queue.lock().unwrap();
            let (id, ev, tx) = loop {
                if let Some(x) = q.pop_front() {
                    break x;
                }
                q = self.ready.wait(q).unwrap();
            };
            drop(q);
            self.inflight.lock().unwrap().insert(id.clone(), tx);
            *self.current.lock().unwrap() = Some(id.clone());
            let deadline = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64 + 10_000);
            return Resp {
                status: 200,
                headers: vec![
                    ("content-type".into(), "application/json".into()),
                    ("Lambda-Runtime-Aws-Request-Id".into(), id),
                    ("Lambda-Runtime-Deadline-Ms".into(), deadline.to_string()),
                    ("Lambda-Runtime-Invoked-Function-Arn".into(), format!("arn:local:lambda:local:000000000000:function:{}", self.name)),
                ],
                body: ev.into_bytes(),
            };
        }
        if req.method == "POST" && p == "init/error" {
            *self.init_error.lock().unwrap() = Some(String::from_utf8_lossy(&req.body).into_owned());
            return Resp::json(202, &json!({"status": "OK"}));
        }
        if let Some((id, kind)) = p.strip_prefix("invocation/").and_then(|r| r.split_once('/'))
            && let Some(tx) = self.inflight.lock().unwrap().remove(id)
        {
            *self.current.lock().unwrap() = None;
            let body = String::from_utf8_lossy(&req.body).into_owned();
            let _ = tx.send(if kind == "response" { Ok(body) } else { Err(body) });
            return Resp::json(202, &json!({"status": "OK"}));
        }
        Resp::json(404, &json!({"errorType": "InvalidRequestID"}))
    }

    fn invoke(&self, id: String, ev: String, timeout: Duration) -> Result<String, String> {
        let (tx, rx) = channel();
        self.queue.lock().unwrap().push_back((id, ev, tx));
        self.ready.notify_one();
        rx.recv_timeout(timeout).map_err(|_| self.init_error.lock().unwrap().clone().unwrap_or_else(|| "function timed out".into()))?
    }
}

fn match_route(tmpl: &str, path: &str) -> Option<(usize, Map<String, Value>)> {
    let a: Vec<&str> = tmpl.trim_matches('/').split('/').collect();
    let b: Vec<&str> = path.trim_matches('/').split('/').collect();
    if a.len() != b.len() {
        return None;
    }
    let mut params = Map::new();
    let mut literal = 0;
    for (x, y) in a.iter().zip(&b) {
        match x.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
            Some(n) if !y.is_empty() => {
                params.insert(n.to_string(), json!(pct_decode(y)));
            }
            Some(_) => return None,
            None if x == y => literal += 1,
            None => return None,
        }
    }
    Some((literal, params))
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct Version {
    pub hash: String,
    pub stores: Vec<Store>,
    routes: Vec<(String, String, String)>,
    funs: HashMap<String, Arc<Fun>>,
    backfills: BTreeMap<String, Arc<Fun>>,
    children: Mutex<Vec<Child>>,
    pub inflight: AtomicUsize,
}

impl Version {
    fn kill(&self) {
        for c in self.children.lock().unwrap().iter_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for Version {
    fn drop(&mut self) {
        self.kill();
    }
}

#[derive(Default)]
struct Router {
    stable: Option<Arc<Version>>,
    canary: Option<(Arc<Version>, u32)>,
    previous: Option<Arc<Version>>,
}

struct Inner {
    name: String,
    dynamo: Arc<Dynamo>,
    dport: u16,
    sink: Sink,
    seq: AtomicU64,
    router: RwLock<Router>,
    record: Option<Mutex<std::fs::File>>,
    swaps: Mutex<()>,
}

pub struct Local {
    pub port: u16,
    pub dynamo: Arc<Dynamo>,
    inner: Arc<Inner>,
}

#[derive(Default)]
pub struct Options {
    pub record: Option<PathBuf>,
}

pub struct Answer {
    pub id: String,
    pub version: String,
    pub handler: String,
    pub resp: Resp,
}

fn table_name(svc: &str, store: &str) -> String {
    format!("{svc}-{store}")
}

impl Inner {
    fn launch(&self, svc: &Service, bootstrap: &Path) -> Result<Arc<Version>, String> {
        if svc.name != self.name {
            return Err(format!("a hot swap keeps the service: this one is svc {}, the running one is svc {}", svc.name, self.name));
        }
        {
            let mut t = self.dynamo.tables.lock().unwrap();
            for s in &svc.stores {
                t.entry(table_name(&svc.name, &s.name)).or_default();
            }
        }
        let tag = &svc.hash[..8];
        let mut funs = HashMap::new();
        let mut backfills = BTreeMap::new();
        let mut children = Vec::new();
        let mut started = Vec::new();
        let backfill_names: BTreeSet<&str> = svc.backfills.iter().map(|b| b.name.as_str()).collect();
        for h in svc.handlers.iter().chain(&svc.backfills) {
            let f = Arc::new(Fun::new(&h.name));
            let akid = format!("LOCAL{}{}", h.name.to_uppercase(), tag.to_uppercase());
            let mut g = BTreeSet::new();
            for (store, acts) in iam::actions(h) {
                for a in acts {
                    g.insert((a.trim_start_matches("dynamodb:").to_string(), table_name(&svc.name, &store)));
                }
            }
            self.dynamo.grants.lock().unwrap().insert(akid.clone(), Grant { who: h.name.clone(), acts: g, fun: f.clone() });
            let (l, p) = bind(0)?;
            let fc = f.clone();
            serve(l, Arc::new(move |r| fc.handle(r)));
            let mut cmd = Command::new(bootstrap);
            cmd.env_clear()
                .env("AWS_LAMBDA_RUNTIME_API", format!("127.0.0.1:{p}"))
                .env("AWS_REGION", "local")
                .env("AWS_ENDPOINT_URL_DYNAMODB", format!("http://127.0.0.1:{}", self.dport))
                .env("AWS_ACCESS_KEY_ID", &akid)
                .env("AWS_SECRET_ACCESS_KEY", "local")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let backfill = backfill_names.contains(h.name.as_str());
            match h.name.strip_prefix("backfill_").filter(|_| backfill) {
                Some(store) => cmd.env("SSPUR_BACKFILL", store),
                None => cmd.env("SSPUR_HANDLER", &h.name),
            };
            for k in ["LD_LIBRARY_PATH", "SystemRoot", "windir"] {
                if let Some(v) = std::env::var_os(k) {
                    cmd.env(k, v);
                }
            }
            for s in h.stores() {
                cmd.env(format!("SSPUR_TABLE_{s}"), table_name(&svc.name, &s));
            }
            let mut child = cmd.spawn().map_err(|e| format!("cannot start {}: {e}", bootstrap.display()))?;
            for out in [child.stdout.take().map(|o| Box::new(o) as Box<dyn Read + Send>), child.stderr.take().map(|o| Box::new(o) as Box<dyn Read + Send>)].into_iter().flatten() {
                let sink = self.sink.clone();
                let name = h.name.clone();
                std::thread::spawn(move || {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        sink(&format!("[{name}] {line}"));
                    }
                });
            }
            children.push(child);
            started.push(f.clone());
            if backfill {
                backfills.insert(h.name.trim_start_matches("backfill_").to_string(), f);
            } else {
                funs.insert(h.name.clone(), f);
            }
        }
        // route traffic only once every bootstrap is up (or has failed), as Lambda does after a cold start
        let deadline = Instant::now() + Duration::from_secs(120);
        while Instant::now() < deadline
            && started.iter().zip(&mut children).any(|(f, c)| !f.polled.load(Ordering::SeqCst) && f.init_error.lock().unwrap().is_none() && matches!(c.try_wait(), Ok(None)))
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        let routes = svc.routes.iter().map(|r| (r.method.to_uppercase(), r.path.clone(), r.handler.clone())).collect();
        Ok(Arc::new(Version { hash: svc.hash.clone(), stores: svc.stores.clone(), routes, funs, backfills, children: Mutex::new(children), inflight: AtomicUsize::new(0) }))
    }

    fn pick(&self) -> Option<Arc<Version>> {
        let r = self.router.read().unwrap();
        let n = self.seq.load(Ordering::Relaxed);
        match &r.canary {
            Some((v, w)) if (n.wrapping_mul(37) % 100) < u64::from(*w) => Some(v.clone()),
            _ => r.stable.clone(),
        }
    }

    fn dispatch(&self, v: &Version, req: &Req) -> Answer {
        let id = format!("local-{:08}", self.seq.fetch_add(1, Ordering::Relaxed));
        let best = v.routes.iter().filter(|(m, _, _)| *m == req.method).filter_map(|(m, t, h)| match_route(t, &req.path).map(|(lit, ps)| (lit, ps, m, t, h))).max_by_key(|x| x.0);
        let Some((_, params, m, t, h)) = best else {
            return Answer { id, version: v.hash.clone(), handler: String::new(), resp: Resp::json(404, &json!({"message": "Not Found"})) };
        };
        let (body, b64) = match String::from_utf8(req.body.clone()) {
            Ok(s) => (s, false),
            Err(_) => (b64(&req.body), true),
        };
        let headers: Map<String, Value> = req.headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), json!(v))).collect();
        let route_key = format!("{m} {t}");
        let mut ev = json!({
            "version": "2.0",
            "routeKey": route_key,
            "rawPath": req.path,
            "rawQueryString": req.query,
            "headers": headers,
            "requestContext": {"http": {"method": m, "path": req.path, "protocol": "HTTP/1.1", "sourceIp": "127.0.0.1"}, "requestId": id, "routeKey": route_key, "stage": "$default"},
            "isBase64Encoded": b64
        });
        if !params.is_empty() {
            ev["pathParameters"] = Value::Object(params);
        }
        if !body.is_empty() {
            ev["body"] = json!(body);
        }
        let out = v.funs[h].invoke(id.clone(), ev.to_string(), Duration::from_secs(15));
        let resp = match out.ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
            Some(r) => {
                let status = r["statusCode"].as_u64().unwrap_or(200) as u16;
                let headers = r["headers"].as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect()).unwrap_or_default();
                Resp { status, headers, body: r["body"].as_str().unwrap_or("").as_bytes().to_vec() }
            }
            None => Resp::json(502, &json!({"message": "Internal Server Error"})),
        };
        Answer { id, version: v.hash.clone(), handler: h.clone(), resp }
    }

    fn front(&self, req: Req) -> Resp {
        if let Some(cmd) = req.path.strip_prefix("/_sspur/") {
            return self.control(cmd, &req);
        }
        let Some(v) = self.pick() else {
            return Resp::json(503, &json!({"message": "no version is live"}));
        };
        v.inflight.fetch_add(1, Ordering::SeqCst);
        let mut a = self.dispatch(&v, &req);
        v.inflight.fetch_sub(1, Ordering::SeqCst);
        drop(v);
        let db = self.dynamo.take_trace(&a.id);
        if let Some(f) = &self.record {
            let line = json!({
                "id": a.id, "svc": self.name, "version": a.version, "handler": a.handler,
                "method": req.method, "path": req.path, "query": req.query, "body": String::from_utf8_lossy(&req.body),
                "status": a.resp.status, "response": String::from_utf8_lossy(&a.resp.body), "db": db
            });
            let mut f = f.lock().unwrap();
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
        (self.sink)(&format!("{} {} -> {} #{} {}", req.method, req.path, a.handler, &a.version[..8], a.resp.status));
        a.resp.headers.push(("x-sspur-version".into(), a.version.clone()));
        a.resp
    }

    fn control(&self, cmd: &str, req: &Req) -> Resp {
        let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
        let r = match (req.method.as_str(), cmd) {
            ("GET", "status") => Ok(self.status()),
            ("POST", "swap") => self.swap_file(body["file"].as_str().unwrap_or(""), body["weight"].as_u64().unwrap_or(100) as u32),
            ("POST", "promote") => self.promote(),
            ("POST", "rollback") => self.rollback(),
            ("POST", "backfill") => self.backfill(body["store"].as_str()),
            _ => return Resp::json(404, &json!({"error": format!("unknown control {} /_sspur/{cmd}", req.method)})),
        };
        match r {
            Ok(v) => Resp::json(200, &v),
            Err(e) => Resp::json(409, &json!({"error": e})),
        }
    }

    fn status(&self) -> Value {
        let r = self.router.read().unwrap();
        let v = |x: &Option<Arc<Version>>| x.as_ref().map(|v| json!({"version": v.hash, "inflight": v.inflight.load(Ordering::SeqCst)}));
        json!({"svc": self.name, "stable": v(&r.stable), "canary": r.canary.as_ref().map(|(c, w)| json!({"version": c.hash, "weight": w, "inflight": c.inflight.load(Ordering::SeqCst)})), "previous": v(&r.previous)})
    }

    fn swap_file(&self, file: &str, weight: u32) -> Result<Value, String> {
        let src = std::fs::read_to_string(file).map_err(|e| format!("cannot read {file}: {e}"))?;
        let svc = crate::analyze(&src).map_err(|e| format!("{file}: {e}"))?;
        let bin = crate::build_local(&svc)?;
        self.swap(&svc, &bin, weight)
    }

    fn swap(&self, svc: &Service, bin: &Path, weight: u32) -> Result<Value, String> {
        let _g = self.swaps.lock().unwrap();
        let stable = self.router.read().unwrap().stable.clone();
        let report = match &stable {
            Some(s) => {
                if s.hash == svc.hash {
                    return Err(format!("#{} is already live", svc.hash));
                }
                let rep = migrate::compare(&s.stores, &svc.stores);
                if !rep.ok() {
                    return Err(format!("swap refused: {}", rep.text().trim_end()));
                }
                if weight < 100 && !rep.side_by_side() {
                    return Err(format!("a canary needs old and new to run side by side: {}", rep.text().trim_end()));
                }
                rep.json()
            }
            None => json!(null),
        };
        let v = self.launch(svc, bin)?;
        let mut r = self.router.write().unwrap();
        if weight >= 100 {
            r.previous = r.stable.replace(v);
            r.canary = None;
        } else {
            r.canary = Some((v, weight));
        }
        drop(r);
        (self.sink)(&format!("swap -> #{} ({}%)", &svc.hash[..8], weight.min(100)));
        Ok(json!({"version": svc.hash, "weight": weight.min(100), "migrate": report, "status": self.status()}))
    }

    fn promote(&self) -> Result<Value, String> {
        let mut r = self.router.write().unwrap();
        let Some((c, _)) = r.canary.take() else { return Err("no canary to promote".into()) };
        r.previous = r.stable.replace(c);
        drop(r);
        Ok(self.status())
    }

    fn rollback(&self) -> Result<Value, String> {
        let mut r = self.router.write().unwrap();
        if let Some((c, _)) = r.canary.take() {
            drop(r);
            (self.sink)(&format!("rollback: canary #{} removed", &c.hash[..8]));
            return Ok(self.status());
        }
        let Some(p) = r.previous.take() else { return Err("nothing to roll back to".into()) };
        r.previous = r.stable.replace(p);
        drop(r);
        (self.sink)("rollback: previous version is live again");
        Ok(self.status())
    }

    fn backfill(&self, store: Option<&str>) -> Result<Value, String> {
        let Some(v) = self.router.read().unwrap().stable.clone() else { return Err("no version is live".into()) };
        let mut out = Vec::new();
        for (s, f) in &v.backfills {
            if store.is_some_and(|x| x != s) {
                continue;
            }
            let mut start = Value::Null;
            let mut total = json!({"store": s, "pages": 0, "scanned": 0, "migrated": 0, "current": 0, "skipped": 0, "failed": 0, "errors": []});
            loop {
                let id = format!("local-{:08}", self.seq.fetch_add(1, Ordering::Relaxed));
                let ev = json!({"sspur": "backfill", "limit": 100, "start": start});
                let r: Value = serde_json::from_str(&f.invoke(id, ev.to_string(), Duration::from_secs(300))?).map_err(|e| e.to_string())?;
                if let Some(e) = r.get("error") {
                    return Err(format!("backfill {s}: {e}"));
                }
                total["pages"] = json!(total["pages"].as_u64().unwrap_or(0) + 1);
                total["schema"] = r["schema"].clone();
                for k in ["scanned", "migrated", "current", "skipped", "failed"] {
                    total[k] = json!(total[k].as_u64().unwrap_or(0) + r[k].as_u64().unwrap_or(0));
                }
                if let (Some(a), Some(b)) = (total["errors"].as_array_mut(), r["errors"].as_array()) {
                    a.extend(b.iter().cloned());
                }
                start = r["next"].clone();
                if start.is_null() {
                    break;
                }
            }
            out.push(total);
        }
        if out.is_empty() {
            return Err(format!("#{} has no migrated store{}", &v.hash[..8], store.map(|s| format!(" named {s}")).unwrap_or_default()));
        }
        Ok(json!(out))
    }
}

impl Local {
    pub fn stop(&self) {
        let mut r = self.inner.router.write().unwrap();
        for v in [r.stable.take(), r.previous.take(), r.canary.take().map(|c| c.0)].into_iter().flatten() {
            v.kill();
        }
    }

    pub fn swap(&self, svc: &Service, bootstrap: &Path, weight: u32) -> Result<Value, String> {
        self.inner.swap(svc, bootstrap, weight)
    }

    pub fn promote(&self) -> Result<Value, String> {
        self.inner.promote()
    }

    pub fn rollback(&self) -> Result<Value, String> {
        self.inner.rollback()
    }

    pub fn backfill(&self, store: Option<&str>) -> Result<Value, String> {
        self.inner.backfill(store)
    }

    pub fn status(&self) -> Value {
        self.inner.status()
    }

    pub fn table(&self, store: &str) -> String {
        table_name(&self.inner.name, store)
    }

    pub(crate) fn replay_one(&self, req: &Req, seed: HashMap<String, BTreeMap<String, Value>>, known: HashMap<String, Known>) -> (Answer, Vec<Value>, Vec<String>) {
        self.dynamo.reset(seed, known);
        let v = self.inner.pick().expect("a live version");
        let a = self.inner.dispatch(&v, req);
        let db = self.dynamo.take_trace(&a.id);
        (a, db, self.dynamo.misses())
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn start(svc: &Service, bootstrap: &Path, port: u16, sink: Sink) -> Result<Local, String> {
    start_with(svc, bootstrap, port, sink, Options::default())
}

pub fn start_with(svc: &Service, bootstrap: &Path, port: u16, sink: Sink, opts: Options) -> Result<Local, String> {
    let dynamo = Arc::new(Dynamo::new(2));
    let (dl, dport) = bind(0)?;
    let d = dynamo.clone();
    serve(dl, Arc::new(move |r| d.handle(&r)));
    let record = match &opts.record {
        Some(p) => {
            dynamo.tracing.store(true, Ordering::Relaxed);
            Some(Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(p).map_err(|e| format!("{}: {e}", p.display()))?))
        }
        None => None,
    };
    let inner = Arc::new(Inner { name: svc.name.clone(), dynamo: dynamo.clone(), dport, sink, seq: AtomicU64::new(1), router: RwLock::new(Router::default()), record, swaps: Mutex::new(()) });
    let v = inner.launch(svc, bootstrap)?;
    inner.router.write().unwrap().stable = Some(v);
    let (fl, fport) = bind(port)?;
    let fi = inner.clone();
    serve(fl, Arc::new(move |req: Req| fi.front(req)));
    Ok(Local { port: fport, dynamo, inner })
}

pub(crate) fn start_replay(svc: &Service, bootstrap: &Path) -> Result<Local, String> {
    let l = start(svc, bootstrap, 0, Arc::new(|_: &str| {}))?;
    l.dynamo.tracing.store(true, Ordering::Relaxed);
    Ok(l)
}

fn b64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn request(port: u16, method: &str, path: &str, body: Option<&str>) -> Result<(u16, String), String> {
    request_full(port, method, path, body).map(|(s, _, b)| (s, b))
}

pub type Headers = Vec<(String, String)>;

pub fn request_full(port: u16, method: &str, path: &str, body: Option<&str>) -> Result<(u16, Headers, String), String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(300))).ok();
    let b = body.unwrap_or("");
    let req = format!("{method} {path} HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{b}", b.len());
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut out = String::new();
    s.read_to_string(&mut out).map_err(|e| e.to_string())?;
    let status = out.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or_else(|| format!("bad response: {out}"))?;
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let headers = head.lines().skip(1).filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    Ok((status, headers, body.to_string()))
}
