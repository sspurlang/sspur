//! Networking (ADR 0035): the HTTP server and client end to end in both tiers, TLS with a
//! certificate made for the run, the wire protocol through raw sockets, a load smoke test, and
//! the request parser under a mutation fuzzer with AddressSanitizer.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/net")
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sspur-net-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Server {
    child: Child,
    port: u16,
    out: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn start(tier: &str, env: &[(&str, &str)]) -> Server {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sspur"));
        cmd.arg("run");
        if !tier.is_empty() {
            cmd.arg(tier);
        }
        cmd.arg(dir().join("server.ssp")).env("NETTEST_PORT", "0").stdout(Stdio::piped()).stderr(Stdio::piped());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let err = child.stderr.take().unwrap();
        let first = lines.next().and_then(Result::ok).unwrap_or_default();
        if !first.contains("listening on") {
            let mut e = String::new();
            let _ = BufReader::new(err).read_to_string(&mut e);
            panic!("the server did not start: {first}\n{e}");
        }
        let port: u16 = first.rsplit(':').next().unwrap().trim().parse().unwrap_or_else(|_| panic!("{first}"));
        let out = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let o = out.clone();
        std::thread::spawn(move || {
            for l in lines.map_while(Result::ok) {
                o.lock().unwrap().push(l);
            }
        });
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut BufReader::new(err), &mut std::io::sink());
        });
        Server { child, port, out }
    }

    /// Stops the server as SIGINT would (TerminateProcess on Windows) and returns its exit status.
    fn stop(mut self) -> Option<i32> {
        #[cfg(unix)]
        {
            let _ = Command::new("kill").arg("-INT").arg(self.child.id().to_string()).status();
            for _ in 0..100 {
                if let Ok(Some(s)) = self.child.try_wait() {
                    return s.code();
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            None
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
            Some(0)
        }
    }
}

fn client(tier: &str, base: &str, env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_sspur"));
    cmd.arg("run");
    if !tier.is_empty() {
        cmd.arg(tier);
    }
    cmd.arg(dir().join("client.ssp")).env("BASE", base);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let r = cmd.output().unwrap();
    assert!(r.status.success(), "{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
    String::from_utf8_lossy(&r.stdout).replace("\r\n", "\n")
}

const EXPECTED: &str = r#"200 application/json {"id":3,"name":"item 3","tags":["t3"]}
404 application/json {"error":{"tag":"NotFound","id":0}}
200 application/json [{"id":1,"name":"item 1","tags":["x y"]},{"id":2,"name":"item 2","tags":["x y"]},{"id":3,"name":"item 3","tags":["x y"]}]
400 application/json {"error":"invalid request","detail":"query parameter limit: expected Int"}
201 application/json {"id":1001,"name":"new","tags":[]}
400 application/json {"error":"invalid request","detail":"body.name: expected Str, found a number"}
400 application/json {"error":"invalid request","detail":"body: invalid JSON"}
400 application/json {"error":{"tag":"Invalid","why":"empty name"}}
204 -
200 application/json {"id":1,"name":"one","tags":[]}
404 application/json {"error":"not found"}
200 application/json ["POST","/echo","a b c","yes","payload"]
200 application/json ["GET","/echo",null,null,""]
405 application/json {"error":"method not allowed"}
404 application/json {"error":"no route","detail":"GET /nowhere"}
200 20000
200 application/json {"id":1,"name":"item 1","tags":["t1"]}
302 some("/items/1")
503
200 text/plain; charset=utf-8 ok after 3
timeout
503 text/plain; charset=utf-8 the handler did not answer in time

500 application/json {"error":"internal error"}
200 text/html; charset=utf-8 <p>hi</p>
200 text/plain; charset=utf-8 ok

{"id":2,"name":"item 2","tags":["t2"]}
raised 404 {"error":{"tag":"NotFound","id":0}}
200 0 20000
connect
dns
io unsupported URL: only http:// and https:// are allowed
200 text/plain; charset=utf-8 hello static

"#;

fn static_dir(name: &str) -> PathBuf {
    let d = tmp(name);
    std::fs::write(d.join("hello.txt"), "hello static\n").unwrap();
    d
}

fn run_pair(server_tier: &str, client_tier: &str) {
    let files = static_dir(&format!("files{server_tier}{client_tier}"));
    let s = Server::start(server_tier, &[("NETTEST_FILES", files.to_str().unwrap())]);
    let base = format!("http://127.0.0.1:{}", s.port);
    let dl = files.join("download.txt");
    let got = client(client_tier, &base, &[("DOWNLOAD", dl.to_str().unwrap())]);
    let norm = |t: &str| t.lines().map(str::trim_end).collect::<Vec<_>>().join("\n");
    let (body, last) = got.trim_end().rsplit_once('\n').unwrap();
    assert_eq!(norm(body), norm(EXPECTED.trim()), "server {server_tier:?}, client {client_tier:?}");
    assert!(last == "404" || last == "400", "a path that climbs out of the static directory: {last}");
    assert_eq!(std::fs::read_to_string(&dl).unwrap().len(), 20000);
    let code = s.stop();
    if cfg!(unix) {
        assert_eq!(code, Some(0), "SIGINT shuts the server down cleanly");
    }
}

#[test]
fn server_and_client_native() {
    run_pair("", "");
}

#[test]
fn server_and_client_interpreted() {
    run_pair("--interp", "--interp");
}

#[test]
fn native_server_interpreted_client_and_back() {
    run_pair("", "--interp");
    run_pair("--interp", "");
}

#[test]
fn test_programs_compile_fully_native() {
    for f in ["server.ssp", "client.ssp"] {
        let r = Command::new(env!("CARGO_BIN_EXE_sspur")).args(["native", "--release"]).arg(dir().join(f)).output().unwrap();
        let out = String::from_utf8_lossy(&r.stdout);
        assert!(r.status.success() && !out.contains("interp "), "{f}:\n{out}{}", String::from_utf8_lossy(&r.stderr));
    }
}

/// One exchange over a raw socket: the status line, the headers named in `show`, and the body.
fn raw(port: u16, req: &[u8], show: &[&str]) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(req).unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();
    responses(&text, show)
}

fn responses(text: &str, show: &[&str]) -> String {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some((head, tail)) = rest.split_once("\r\n\r\n") {
        let mut lines = head.lines();
        let status = lines.next().unwrap_or("").to_string();
        let mut len = 0usize;
        let mut kept = Vec::new();
        for l in lines {
            let (n, v) = l.split_once(':').unwrap_or((l, ""));
            if n.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
            if show.iter().any(|s| s.eq_ignore_ascii_case(n)) {
                kept.push(format!("{}:{v}", n.to_ascii_lowercase()));
            }
        }
        let body_len = if status.contains(" 100 ") || status.contains(" 204 ") || status.contains(" 304 ") || head.is_empty() { 0 } else { len.min(tail.len()) };
        let (body, next) = tail.split_at(body_len);
        out.push(format!("{status} | {} | {body}", kept.join(" ")));
        rest = next;
    }
    out.join("\n")
}

fn protocol(tier: &str) -> String {
    let files = static_dir(&format!("proto{tier}"));
    let s = Server::start(tier, &[("NETTEST_FILES", files.to_str().unwrap()), ("NETTEST_MAX_BODY", "64")]);
    let p = s.port;
    let ka = &["connection", "content-type"];
    let cases: Vec<(&str, Vec<u8>, &[&str])> = vec![
        ("keep-alive and pipelining", "GET /items/1 HTTP/1.1\r\nHost: a\r\n\r\nGET /find/2 HTTP/1.1\r\nHost: a\r\nConnection: close\r\n\r\n".into(), ka),
        ("HTTP/1.0 closes", "GET /items/2 HTTP/1.0\r\n\r\n".into(), ka),
        ("HEAD has no body", "HEAD /items/3 HTTP/1.1\r\nHost: a\r\nConnection: close\r\n\r\n".into(), &["content-length"]),
        ("missing Host", "GET / HTTP/1.1\r\n\r\n".into(), &[]),
        ("bad request line", "GET\r\n\r\n".into(), &[]),
        ("HTTP/2 preface", "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n".into(), &[]),
        ("folded header", "GET / HTTP/1.1\r\nHost: a\r\nX: 1\r\n  2\r\n\r\n".into(), &[]),
        ("both lengths", "POST /echo HTTP/1.1\r\nHost: a\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n".into(), &[]),
        ("body too large", format!("POST /echo HTTP/1.1\r\nHost: a\r\nContent-Length: 100\r\n\r\n{}", "x".repeat(100)).into_bytes(), &[]),
        ("chunked body", "POST /echo?x=1 HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n4;e=1\r\ndefg\r\n0\r\n\r\n".into(), &[]),
        ("bad chunk", "POST /echo HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n".into(), &[]),
        ("expect 100-continue", "POST /echo HTTP/1.1\r\nHost: a\r\nExpect: 100-continue\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi".into(), &[]),
        ("invalid UTF-8", b"POST /echo HTTP/1.1\r\nHost: a\r\nContent-Length: 2\r\nConnection: close\r\n\r\n\xff\xfe".to_vec(), &[]),
        ("gzip", "GET /big HTTP/1.1\r\nHost: a\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n".into(), &["content-encoding", "vary"]),
        ("no gzip when refused", "GET /big HTTP/1.1\r\nHost: a\r\nAccept-Encoding: gzip;q=0\r\nConnection: close\r\n\r\n".into(), &["content-encoding"]),
        ("CORS preflight", "OPTIONS /items HTTP/1.1\r\nHost: a\r\nOrigin: https://ok.example\r\nAccess-Control-Request-Method: POST\r\nAccess-Control-Request-Headers: x-a\r\nConnection: close\r\n\r\n".into(), &["access-control-allow-origin", "access-control-allow-methods", "access-control-allow-headers"]),
        ("CORS allowed", "GET /items/1 HTTP/1.1\r\nHost: a\r\nOrigin: https://ok.example\r\nConnection: close\r\n\r\n".into(), &["access-control-allow-origin", "vary"]),
        ("CORS refused", "GET /items/1 HTTP/1.1\r\nHost: a\r\nOrigin: https://evil.example\r\nConnection: close\r\n\r\n".into(), &["access-control-allow-origin"]),
        ("request id kept", "GET /healthz HTTP/1.1\r\nHost: a\r\nX-Request-Id: trace-42\r\nConnection: close\r\n\r\n".into(), &["x-request-id"]),
        ("static file", "GET /static/hello.txt HTTP/1.1\r\nHost: a\r\nConnection: close\r\n\r\n".into(), &["content-type"]),
        ("static traversal", "GET /static/../server.ssp HTTP/1.1\r\nHost: a\r\nConnection: close\r\n\r\n".into(), &[]),
        ("static encoded traversal", "GET /static/%2e%2e/server.ssp HTTP/1.1\r\nHost: a\r\nConnection: close\r\n\r\n".into(), &[]),
        ("static missing", "GET /static/nope.txt HTTP/1.1\r\nHost: a\r\nConnection: close\r\n\r\n".into(), &[]),
        ("absolute-form target", "GET http://example.com/items/4 HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n".into(), &[]),
        ("percent-encoded path", "GET /items/%35 HTTP/1.1\r\nHost: a\r\nConnection: close\r\n\r\n".into(), &[]),
        ("handler trap", "GET /trap HTTP/1.1\r\nHost: a\r\nConnection: close\r\n\r\n".into(), &[]),
    ];
    let mut out = String::new();
    for (name, req, show) in &cases {
        out.push_str(&format!("{name}:\n{}\n", raw(p, req, show)));
    }
    let _ = s.stop();
    out.replace('\u{fffd}', "?")
}

#[test]
fn wire_protocol_is_the_same_in_both_tiers() {
    let native = protocol("");
    let interp = protocol("--interp");
    assert_eq!(native, interp);
    for want in [
        "keep-alive and pipelining:\nHTTP/1.1 200 OK | content-type: application/json | {\"id\":1,\"name\":\"item 1\",\"tags\":[\"t1\"]}\nHTTP/1.1 404 Not Found | content-type: application/json connection: close | {\"error\":\"not found\"}",
        "HTTP/1.0 closes:\nHTTP/1.1 200 OK | content-type: application/json connection: close |",
        "HEAD has no body:\nHTTP/1.1 200 OK | content-length: 38 | \n",
        "missing Host:\nHTTP/1.1 400 Bad Request",
        "HTTP/2 preface:\nHTTP/1.1 505 HTTP Version Not Supported",
        "folded header:\nHTTP/1.1 400 Bad Request",
        "both lengths:\nHTTP/1.1 400 Bad Request",
        "body too large:\nHTTP/1.1 413 Content Too Large",
        "chunked body:\nHTTP/1.1 200 OK |  | [\"POST\",\"/echo\",\"1\",null,\"abcdefg\"]",
        "bad chunk:\nHTTP/1.1 400 Bad Request",
        "expect 100-continue:\nHTTP/1.1 100 Continue |  | \nHTTP/1.1 200 OK |  | [\"POST\",\"/echo\",null,null,\"hi\"]",
        "invalid UTF-8:\nHTTP/1.1 400 Bad Request",
        "gzip:\nHTTP/1.1 200 OK | content-encoding: gzip vary: Accept-Encoding |",
        "no gzip when refused:\nHTTP/1.1 200 OK |  | abcdefghij",
        "CORS preflight:\nHTTP/1.1 204 No Content | access-control-allow-methods: GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS access-control-allow-headers: x-a access-control-allow-origin: https://ok.example |",
        "CORS allowed:\nHTTP/1.1 200 OK | access-control-allow-origin: https://ok.example vary: Origin |",
        "CORS refused:\nHTTP/1.1 200 OK |  |",
        "request id kept:\nHTTP/1.1 200 OK | x-request-id: trace-42 | ok",
        "static file:\nHTTP/1.1 200 OK | content-type: text/plain; charset=utf-8 | hello static",
        "static traversal:\nHTTP/1.1 400 Bad Request",
        "static encoded traversal:\nHTTP/1.1 400 Bad Request",
        "static missing:\nHTTP/1.1 404 Not Found",
        "absolute-form target:\nHTTP/1.1 200 OK |  | {\"id\":4",
        "percent-encoded path:\nHTTP/1.1 200 OK |  | {\"id\":5",
        "handler trap:\nHTTP/1.1 500 Internal Server Error |  | {\"error\":\"internal error\"}",
    ] {
        assert!(native.contains(want), "missing:\n{want}\n\nin:\n{native}");
    }
}

fn have(tool: &str, arg: &str) -> bool {
    Command::new(tool).arg(arg).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

#[test]
fn tls_with_a_certificate_made_for_the_run() {
    if !have("openssl", "version") {
        eprintln!("skipping: no openssl command to make a certificate");
        return;
    }
    let d = tmp("tls");
    let (cert, key) = (d.join("cert.pem"), d.join("key.pem"));
    let r = Command::new("openssl").args(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost,IP:127.0.0.1", "-keyout"]).arg(&key).arg("-out").arg(&cert).output().unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    for tier in ["", "--interp"] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sspur"));
        cmd.arg("run");
        if !tier.is_empty() {
            cmd.arg(tier);
        }
        let probe = cmd.arg(dir().join("server.ssp")).env("NETTEST_PORT", "0").env("NETTEST_CERT", &cert).env("NETTEST_KEY", &key).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let out = probe.wait_with_output_timeout();
        if out.contains("OpenSSL") {
            eprintln!("skipping TLS: {out}");
            return;
        }
        let s = Server::start(tier, &[("NETTEST_CERT", cert.to_str().unwrap()), ("NETTEST_KEY", key.to_str().unwrap())]);
        let base = format!("https://localhost:{}", s.port);
        let got = client(tier, &base, &[("SSL_CERT_FILE", cert.to_str().unwrap()), ("DOWNLOAD", d.join("dl.txt").to_str().unwrap())]);
        assert!(got.starts_with("200 application/json {\"id\":3"), "{tier}: {got}");
        let untrusted = client(tier, &base, &[("SSL_CERT_FILE", ""), ("DOWNLOAD", d.join("dl2.txt").to_str().unwrap())]);
        assert!(untrusted.starts_with("error Tls{"), "{tier}: an unknown certificate fails verification: {untrusted}");
        drop(s);
    }
}

trait WaitTimeout {
    fn wait_with_output_timeout(self) -> String;
}

impl WaitTimeout for Child {
    /// The first line a server prints (or its error), then the process is stopped.
    fn wait_with_output_timeout(mut self) -> String {
        let mut line = String::new();
        let mut out = BufReader::new(self.stdout.take().unwrap());
        let _ = out.read_line(&mut line);
        if !line.contains("listening") {
            let mut e = String::new();
            let _ = self.stderr.take().unwrap().read_to_string(&mut e);
            line.push_str(&e);
        }
        let _ = self.kill();
        let _ = self.wait();
        line
    }
}

fn load(tier: &str, total: usize, conc: usize) {
    let s = Server::start(tier, &[]);
    let port = s.port;
    let per = total / conc;
    let handles: Vec<_> = (0..conc)
        .map(|t| {
            std::thread::spawn(move || {
                let mut sock = TcpStream::connect(("127.0.0.1", port)).unwrap();
                sock.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
                let mut r = BufReader::new(sock.try_clone().unwrap());
                let mut ok = 0;
                for i in 0..per {
                    let id = t * per + i + 1;
                    write!(sock, "GET /items/{id} HTTP/1.1\r\nHost: a\r\n\r\n").unwrap();
                    let mut status = String::new();
                    r.read_line(&mut status).unwrap();
                    let mut len = 0;
                    loop {
                        let mut h = String::new();
                        r.read_line(&mut h).unwrap();
                        if h == "\r\n" {
                            break;
                        }
                        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap();
                        }
                    }
                    let mut body = vec![0; len];
                    r.read_exact(&mut body).unwrap();
                    if status.starts_with("HTTP/1.1 200") && String::from_utf8_lossy(&body).contains(&format!("\"id\":{id},")) {
                        ok += 1;
                    }
                }
                ok
            })
        })
        .collect();
    let ok: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
    assert_eq!(ok, per * conc, "{tier}: every request answered correctly");
    let errors = s.out.lock().unwrap().iter().filter(|l| l.starts_with("error")).count();
    assert_eq!(errors, 0);
    let code = s.stop();
    if cfg!(unix) {
        assert_eq!(code, Some(0));
    }
}

#[test]
fn load_smoke_10k_requests_at_concurrency_64() {
    load("", 10_048, 64);
}

#[cfg(unix)]
fn rss_kb(pid: u32) -> u64 {
    let o = Command::new("ps").args(["-o", "rss=", "-p", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0)
}

#[cfg(unix)]
#[test]
fn memory_stays_flat_under_load() {
    let s = Server::start("", &[("SSPUR_GC_MIN_MB", "8"), ("NETTEST_WORKERS", "4")]);
    let port = s.port;
    let burst = |n: usize| {
        let hs: Vec<_> = (0..16)
            .map(|_| {
                std::thread::spawn(move || {
                    let mut sock = TcpStream::connect(("127.0.0.1", port)).unwrap();
                    let mut r = BufReader::new(sock.try_clone().unwrap());
                    for i in 0..n / 16 {
                        if i > 0 && i % 500 == 0 {
                            sock = TcpStream::connect(("127.0.0.1", port)).unwrap();
                            r = BufReader::new(sock.try_clone().unwrap());
                        }
                        sock.write_all(b"GET /items?limit=20 HTTP/1.1\r\nHost: a\r\n\r\n").unwrap();
                        let mut len = 0;
                        loop {
                            let mut h = String::new();
                            r.read_line(&mut h).unwrap();
                            if h == "\r\n" {
                                break;
                            }
                            if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                                len = v.trim().parse().unwrap();
                            }
                        }
                        let mut body = vec![0; len];
                        r.read_exact(&mut body).unwrap();
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
    };
    burst(8_000);
    let before = rss_kb(s.child.id());
    burst(40_000);
    let after = rss_kb(s.child.id());
    assert!(after < before + 30_000, "RSS grew from {before} KB to {after} KB over 40,000 requests");
}

#[test]
fn load_smoke_interpreted() {
    load("--interp", 2_048, 64);
}

#[test]
fn proxies_come_from_the_environment() {
    let s = Server::start("", &[]);
    let proxy = format!("http://127.0.0.1:{}", s.port);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_sspur"));
    let src = tmp("proxy").join("p.ssp");
    std::fs::write(&src, "fn main() -> Unit ! net, log\n= log(http.get(\"http://example.invalid/items/9\").map(r => r.body).or(\"failed\"))\n").unwrap();
    let r = cmd.arg("run").arg(&src).env("http_proxy", &proxy).env("HTTP_PROXY", &proxy).env_remove("no_proxy").env_remove("NO_PROXY").output().unwrap();
    let out = String::from_utf8_lossy(&r.stdout);
    assert!(out.contains("\"id\":9"), "{out}{}", String::from_utf8_lossy(&r.stderr));
}

#[test]
fn gzip_round_trips_through_the_system_decoder() {
    let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8 ^ (i / 1000) as u8).collect();
    let gz = sspur_native::net::gzip(&data).unwrap();
    assert!(gz.len() < data.len());
    let d = tmp("gzip");
    std::fs::write(d.join("x.gz"), &gz).unwrap();
    let Ok(r) = Command::new("gzip").arg("-dc").arg(d.join("x.gz")).output() else {
        eprintln!("skipping: no gzip");
        return;
    };
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(r.stdout, data);
}

#[test]
fn request_parser_survives_the_fuzzer() {
    let cc = sspur_native::cgen::flags::cc();
    let d = tmp("fuzz");
    let exe = d.join(format!("fuzz{}", std::env::consts::EXE_SUFFIX));
    let rt = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sspur-native/src/net_rt.c");
    let def = format!("-DNET_RT=\"{}\"", rt.display().to_string().replace('\\', "/"));
    let build = |san: &[&str]| {
        let mut cmd = Command::new(&cc);
        cmd.args(["-O1", "-g", "-w"]).args(san).arg(&def).arg("-o").arg(&exe).arg(dir().join("fuzz_parser.c"));
        if cfg!(windows) {
            cmd.args(["-lws2_32", "-lwinhttp", "-lcrypt32"]);
        } else if cfg!(target_os = "linux") {
            cmd.args(["-lpthread", "-ldl"]);
        }
        cmd.output().unwrap()
    };
    let mut b = if cfg!(windows) { build(&[]) } else { build(&["-fsanitize=address,undefined", "-fno-sanitize-recover=undefined"]) };
    if !b.status.success() {
        eprintln!("the sanitizers are not available here; fuzzing with the parser's own checks only");
        b = build(&[]);
    }
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    for seed in ["2026", "99"] {
        let r = Command::new(&exe).args(["200000", seed]).output().unwrap();
        let out = String::from_utf8_lossy(&r.stdout);
        assert!(r.status.success() && out.contains("fuzz ok"), "{out}{}", String::from_utf8_lossy(&r.stderr));
    }
    let parse = |s: &str| sspur_native::net::parse_head(s.as_bytes()).unwrap();
    assert_eq!(parse("GET / HTTP/1.1\r\nHost: a\r\n\r\n"), 27);
    assert_eq!(parse("GET / HTTP/1.1\r\nHost: a\r\n"), 0);
    assert_eq!(parse("GET / HTTP/1.1\r\nHost: a\nX: b\r\n\r\n"), -400);
    assert_eq!(parse("GET / HTTP/1.1\r\nHost: a\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n"), -400);
    assert_eq!(parse("GET / HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: gzip\r\n\r\n"), -501);
    assert_eq!(parse(&format!("GET /{} HTTP/1.1\r\nHost: a\r\n\r\n", "a".repeat(9000))), -414);
    assert_eq!(sspur_native::net::dechunk(b"5\r\nhello\r\n0\r\n\r\n", 100).unwrap(), (15, b"hello".to_vec()));
    assert_eq!(sspur_native::net::dechunk(b"5\r\nhello\r\n0\r\n\r\n", 3).unwrap().0, -413);
}
