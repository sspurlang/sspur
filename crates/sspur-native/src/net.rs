//! The network runtime (`net_rt.c`): built once into a shared library in the cache, then loaded
//! by native programs (through the path the generated C embeds) and by the interpreter (through
//! the bindings here). Programs that don't use networking never build or load it.
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;

pub const SRC: &str = include_str!("net_rt.c");

/// The shared library, building it on first use (with miniz, for the server's gzip).
pub fn library() -> Result<PathBuf, String> {
    crate::rt::library(crate::rt::Kind::Net)
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SnStr {
    p: *const u8,
    n: i64,
}

impl SnStr {
    fn of(s: &str) -> SnStr {
        SnStr { p: s.as_ptr(), n: s.len() as i64 }
    }
    fn get(&self) -> String {
        if self.p.is_null() || self.n <= 0 {
            return String::new();
        }
        let b = unsafe { std::slice::from_raw_parts(self.p, self.n as usize) };
        String::from_utf8_lossy(b).into_owned()
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SnHdr {
    name: SnStr,
    value: SnStr,
}

#[repr(C)]
struct SnCfg {
    host: SnStr,
    port: i64,
    max_body: i64,
    timeout_ms: i64,
    max_requests: i64,
    gzip: i32,
    access_log: i32,
    cors: *const SnStr,
    ncors: i64,
    files: *const SnStr,
    nfiles: i64,
    health: SnStr,
    cert: SnStr,
    key: SnStr,
    log: extern "C" fn(*mut c_void, *const u8, i64),
    log_ctx: *mut c_void,
}

#[repr(C)]
struct SnCall {
    method: SnStr,
    url: SnStr,
    body: SnStr,
    headers: *const SnHdr,
    nheaders: i64,
    timeout_ms: i64,
}

#[repr(C)]
struct SnReq {
    method: SnStr,
    url: SnStr,
    body: SnStr,
    headers: *const SnHdr,
    nheaders: i64,
    timeout_ms: i64,
    retries: i64,
    redirects: i64,
    download: SnStr,
}

#[repr(C)]
struct SnResp {
    status: i64,
    err: i64,
    body: *mut u8,
    body_n: i64,
    headers: *mut SnHdr,
    nheaders: i64,
    msg: *mut u8,
    msg_n: i64,
    store: *mut u8,
}

struct Api {
    listen: unsafe extern "C" fn(*const SnCfg, *mut u8, i64) -> *mut c_void,
    port: unsafe extern "C" fn(*mut c_void) -> i64,
    is_tls: unsafe extern "C" fn(*mut c_void) -> i32,
    next: unsafe extern "C" fn(*mut c_void, i64, *mut i32) -> *mut SnCall,
    reply: unsafe extern "C" fn(*mut SnCall, i64, *const SnHdr, i64, *const u8, i64),
    close: unsafe extern "C" fn(*mut c_void),
    fetch: unsafe extern "C" fn(*const SnReq, *mut SnResp) -> i64,
    resp_free: unsafe extern "C" fn(*mut SnResp),
    parse_head: unsafe extern "C" fn(*const u8, i64, *mut u8) -> i64,
    dechunk: unsafe extern "C" fn(*const u8, i64, i64, *mut SnBuf) -> i64,
    gzip: unsafe extern "C" fn(*const u8, i64, *mut SnBuf),
    free: unsafe extern "C" fn(*mut c_void),
}

#[repr(C)]
struct SnBuf {
    p: *mut u8,
    n: i64,
    cap: i64,
}

fn api() -> Result<&'static Api, String> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(|| {
        let path = library()?;
        let lib = crate::load_library(&path)?;
        macro_rules! sym {
            ($n:literal) => {
                *unsafe { lib.get($n) }.map_err(|e| e.to_string())?
            };
        }
        let api = Api {
            listen: sym!(b"sn_listen"),
            port: sym!(b"sn_port"),
            is_tls: sym!(b"sn_is_tls"),
            next: sym!(b"sn_next"),
            reply: sym!(b"sn_reply"),
            close: sym!(b"sn_close"),
            fetch: sym!(b"sn_fetch"),
            resp_free: sym!(b"sn_resp_free"),
            parse_head: sym!(b"sn_parse_head"),
            dechunk: sym!(b"sn_dechunk"),
            gzip: sym!(b"sn_gzip"),
            free: sym!(b"sn_free"),
        };
        std::mem::forget(lib);
        Ok(api)
    })
    .as_ref()
    .map_err(|e| e.clone())
}

/// What the client sends: `download` names a file that receives the body instead of memory.
pub struct Request<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a str,
    pub timeout_ms: i64,
    pub retries: i64,
    pub redirects: i64,
    pub download: &'a str,
}

pub struct Response {
    pub status: i64,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Error kinds: 1 DNS, 2 connect, 3 TLS, 4 timeout, 5 other.
pub fn fetch(r: &Request) -> Result<Response, (i64, String)> {
    let api = api().map_err(|e| (5, e))?;
    let hs: Vec<SnHdr> = r.headers.iter().map(|(n, v)| SnHdr { name: SnStr::of(n), value: SnStr::of(v) }).collect();
    let rq = SnReq { method: SnStr::of(r.method), url: SnStr::of(r.url), body: SnStr::of(r.body), headers: hs.as_ptr(), nheaders: hs.len() as i64, timeout_ms: r.timeout_ms, retries: r.retries, redirects: r.redirects, download: SnStr::of(r.download) };
    let mut out = SnResp { status: 0, err: 0, body: std::ptr::null_mut(), body_n: 0, headers: std::ptr::null_mut(), nheaders: 0, msg: std::ptr::null_mut(), msg_n: 0, store: std::ptr::null_mut() };
    unsafe { (api.fetch)(&rq, &mut out) };
    let res = if out.err != 0 {
        Err((out.err, SnStr { p: out.msg, n: out.msg_n }.get()))
    } else {
        let hs = unsafe { std::slice::from_raw_parts(out.headers, out.nheaders as usize) };
        let headers = hs.iter().map(|h| (h.name.get(), h.value.get())).collect();
        let bytes = if out.body.is_null() { &[][..] } else { unsafe { std::slice::from_raw_parts(out.body, out.body_n as usize) } };
        match std::str::from_utf8(bytes) {
            Ok(b) => Ok(Response { status: out.status, headers, body: b.to_string() }),
            Err(_) => Err((5, NOT_UTF8.to_string())),
        }
    };
    unsafe { (api.resp_free)(&mut out) };
    res
}

/// The client's message for a body that isn't text, the same in both tiers.
pub const NOT_UTF8: &str = "the response body is not valid UTF-8; save binary content with http.download";

pub struct ServerCfg<'a> {
    pub host: &'a str,
    pub port: i64,
    pub max_body: i64,
    pub timeout_ms: i64,
    pub max_requests: i64,
    pub gzip: bool,
    pub access_log: bool,
    pub cors: &'a [String],
    pub files: &'a [(String, String)],
    pub health: &'a str,
    pub cert: &'a str,
    pub key: &'a str,
}

extern "C" fn log_line(_: *mut c_void, p: *const u8, n: i64) {
    use std::io::Write;
    if let Some(l) = crate::format_log(&SnStr { p, n }.get()) {
        let _ = writeln!(std::io::stdout(), "{l}");
    }
}

pub struct Server {
    ptr: *mut c_void,
    api: &'static Api,
}

pub struct Call {
    ptr: *mut SnCall,
    api: &'static Api,
    pub method: String,
    pub url: String,
    pub body: String,
    pub headers: Vec<(String, String)>,
    pub timeout_ms: i64,
}

pub enum Next {
    Call(Call),
    Idle,
    Done,
}

impl Server {
    pub fn listen(c: &ServerCfg) -> Result<Server, String> {
        let api = api()?;
        let cors: Vec<SnStr> = c.cors.iter().map(|s| SnStr::of(s)).collect();
        let files: Vec<SnStr> = c.files.iter().flat_map(|(a, b)| [SnStr::of(a), SnStr::of(b)]).collect();
        let cfg = SnCfg {
            host: SnStr::of(c.host),
            port: c.port,
            max_body: c.max_body,
            timeout_ms: c.timeout_ms,
            max_requests: c.max_requests,
            gzip: i32::from(c.gzip),
            access_log: i32::from(c.access_log),
            cors: cors.as_ptr(),
            ncors: cors.len() as i64,
            files: files.as_ptr(),
            nfiles: c.files.len() as i64,
            health: SnStr::of(c.health),
            cert: SnStr::of(c.cert),
            key: SnStr::of(c.key),
            log: log_line,
            log_ctx: std::ptr::null_mut(),
        };
        let mut err = vec![0u8; 512];
        let ptr = unsafe { (api.listen)(&cfg, err.as_mut_ptr(), err.len() as i64) };
        if ptr.is_null() {
            let n = err.iter().position(|&b| b == 0).unwrap_or(err.len());
            return Err(String::from_utf8_lossy(&err[..n]).into_owned());
        }
        Ok(Server { ptr, api })
    }

    pub fn port(&self) -> i64 {
        unsafe { (self.api.port)(self.ptr) }
    }

    pub fn is_tls(&self) -> bool {
        unsafe { (self.api.is_tls)(self.ptr) != 0 }
    }

    /// The next request; `wait_ms` < 0 waits until there is one or the server has stopped.
    pub fn next(&self, wait_ms: i64) -> Next {
        let mut idle = 0i32;
        let c = unsafe { (self.api.next)(self.ptr, wait_ms, &mut idle) };
        if c.is_null() {
            return if idle != 0 { Next::Idle } else { Next::Done };
        }
        let r = unsafe { &*c };
        let hs = unsafe { std::slice::from_raw_parts(r.headers, r.nheaders as usize) };
        Next::Call(Call { ptr: c, api: self.api, method: r.method.get(), url: r.url.get(), body: r.body.get(), headers: hs.iter().map(|h| (h.name.get(), h.value.get())).collect(), timeout_ms: r.timeout_ms })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        unsafe { (self.api.close)(self.ptr) }
    }
}

impl Call {
    pub fn reply(self, status: i64, headers: &[(String, String)], body: &str) {
        let hs: Vec<SnHdr> = headers.iter().map(|(n, v)| SnHdr { name: SnStr::of(n), value: SnStr::of(v) }).collect();
        unsafe { (self.api.reply)(self.ptr, status, hs.as_ptr(), hs.len() as i64, body.as_ptr(), body.len() as i64) };
        std::mem::forget(self);
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        unsafe { (self.api.reply)(self.ptr, 500, std::ptr::null(), 0, std::ptr::null(), 0) };
    }
}

/// The result of parsing a request head: the bytes used, 0 for more input, or `-status`.
pub fn parse_head(buf: &[u8]) -> Result<i64, String> {
    let api = api()?;
    let mut h = vec![0u8; 8192];
    Ok(unsafe { (api.parse_head)(buf.as_ptr(), buf.len() as i64, h.as_mut_ptr()) })
}

/// Decodes a chunked body: the bytes used (0 for more input, or `-status`) and the content.
pub fn dechunk(buf: &[u8], max: i64) -> Result<(i64, Vec<u8>), String> {
    let api = api()?;
    let mut out = SnBuf { p: std::ptr::null_mut(), n: 0, cap: 0 };
    let r = unsafe { (api.dechunk)(buf.as_ptr(), buf.len() as i64, max, &mut out) };
    let v = if out.p.is_null() { vec![] } else { unsafe { std::slice::from_raw_parts(out.p, out.n as usize) }.to_vec() };
    unsafe { (api.free)(out.p as *mut c_void) };
    Ok((r, v))
}

/// The server's gzip encoder, for tests.
pub fn gzip(data: &[u8]) -> Result<Vec<u8>, String> {
    let api = api()?;
    let mut out = SnBuf { p: std::ptr::null_mut(), n: 0, cap: 0 };
    unsafe { (api.gzip)(data.as_ptr(), data.len() as i64, &mut out) };
    let v = unsafe { std::slice::from_raw_parts(out.p, out.n as usize) }.to_vec();
    unsafe { (api.free)(out.p as *mut c_void) };
    Ok(v)
}
