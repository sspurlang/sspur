//! The data and security libraries (ADR 0037): crypto known-answer vectors, compression, CSV and
//! SQLite programs in both tiers with identical results, round-trip properties under the fuzzer,
//! compression against the system's gzip and zstd, and an HTTP server writing to SQLite from
//! concurrent handlers.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sspur-data-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sspur(args: &[&str], tmpdir: &PathBuf) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_sspur")).args(args).env("DATATEST_TMP", tmpdir).output().unwrap();
    (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

fn both_tiers(name: &str) {
    let file = dir().join(format!("{name}.ssp"));
    let f = file.to_str().unwrap();
    let (ok_i, interp) = sspur(&["test", "--interp", f], &tmp(&format!("{name}-i")));
    let (ok_n, native) = sspur(&["test", "--strict-native", f], &tmp(&format!("{name}-n")));
    assert!(ok_i && interp.contains(" passed, 0 failed"), "interpreter:\n{interp}");
    assert!(ok_n && native.contains(" passed, 0 failed"), "native:\n{native}");
    assert_eq!(interp, native);
}

#[test]
fn crypto_vectors_and_api_in_both_tiers() {
    both_tiers("crypto");
}

#[test]
fn compression_in_both_tiers() {
    both_tiers("zip");
}

#[test]
fn csv_in_both_tiers() {
    both_tiers("csv");
}

#[test]
fn sqlite_in_both_tiers() {
    both_tiers("db");
}

#[test]
fn generic_sqlite_code_in_both_tiers() {
    both_tiers("generic");
}

#[test]
fn round_trips_hold_under_the_fuzzer() {
    for (file, fns) in [("zip", &["rt_gzip", "rt_deflate", "rt_zstd"][..]), ("csv", &["rt"][..])] {
        let p = dir().join(format!("{file}.ssp"));
        let (ok, out) = sspur(&["fuzz", "--cases", "400", p.to_str().unwrap()], &tmp(&format!("fuzz-{file}")));
        assert!(ok, "{out}");
        for f in fns {
            assert!(out.lines().any(|l| l.starts_with("ok") && l.split_whitespace().nth(1) == Some(f)), "{f} did not pass:\n{out}");
        }
    }
}

#[test]
fn compression_matches_the_system_tools() {
    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8 ^ (i / 997) as u8).collect();
    let d = tmp("tools");
    for (fmt, tool, ext) in [(1, "gzip", "gz"), (3, "zstd", "zst")] {
        if Command::new(tool).arg("--version").output().is_err() {
            eprintln!("skipping {tool}: not installed");
            continue;
        }
        let ours = sspur_native::rt::pack(fmt, 6, &data).unwrap();
        let f = d.join(format!("ours.{ext}"));
        std::fs::write(&f, &ours).unwrap();
        let r = Command::new(tool).arg("-dc").arg(&f).output().unwrap();
        assert!(r.status.success() && r.stdout == data, "{tool} could not read ours: {}", String::from_utf8_lossy(&r.stderr));
        let raw = d.join("raw.bin");
        std::fs::write(&raw, &data).unwrap();
        let r = Command::new(tool).arg("-c").arg(&raw).output().unwrap();
        assert!(r.status.success());
        assert_eq!(sspur_native::rt::unpack(fmt, &r.stdout).unwrap().unwrap(), data, "we could not read {tool}'s output");
    }
}

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn start(tier: &str, db: &std::path::Path, workers: usize) -> Server {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sspur"));
        cmd.arg("run");
        if !tier.is_empty() {
            cmd.arg(tier);
        }
        cmd.arg(dir().join("server.ssp")).env("DBTEST_PATH", db).env("DBTEST_WORKERS", workers.to_string()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let first = lines.next().and_then(Result::ok).unwrap_or_default();
        if !first.contains("listening on") {
            let mut e = String::new();
            let _ = BufReader::new(child.stderr.take().unwrap()).read_to_string(&mut e);
            panic!("the server did not start: {first}\n{e}");
        }
        let port: u16 = first.rsplit(':').next().unwrap().trim().parse().unwrap();
        std::thread::spawn(move || for _ in lines.map_while(Result::ok) {});
        Server { child, port }
    }
}

/// One request on a keep-alive connection: the status and the body.
fn call(sock: &mut TcpStream, r: &mut BufReader<TcpStream>, method: &str, path: &str, body: &str) -> (u16, String) {
    write!(sock, "{method} {path} HTTP/1.1\r\nHost: a\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut status = String::new();
    r.read_line(&mut status).unwrap();
    let code: u16 = status.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
    let mut len = 0;
    loop {
        let mut h = String::new();
        r.read_line(&mut h).unwrap();
        if h == "\r\n" || h.is_empty() {
            break;
        }
        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap();
        }
    }
    let mut b = vec![0; len];
    r.read_exact(&mut b).unwrap();
    (code, String::from_utf8_lossy(&b).into_owned())
}

fn connect(port: u16) -> (TcpStream, BufReader<TcpStream>) {
    let sock = TcpStream::connect(("127.0.0.1", port)).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(300))).unwrap();
    let r = BufReader::new(sock.try_clone().unwrap());
    (sock, r)
}

fn concurrent_writes(tier: &str, clients: usize, each: usize) {
    let d = tmp(&format!("server{tier}"));
    let s = Server::start(tier, &d.join("app.db"), 8);
    let port = s.port;
    let hs: Vec<_> = (0..clients)
        .map(|k| {
            std::thread::spawn(move || {
                let (mut sock, mut r) = connect(port);
                let mut bad = Vec::new();
                for i in 0..each {
                    let (code, body) = call(&mut sock, &mut r, "POST", "/items", &format!("{{\"name\":\"c{k}-{i}\"}}"));
                    if code != 201 {
                        bad.push(format!("{code} {body}"));
                    }
                }
                bad
            })
        })
        .collect();
    let bad: Vec<String> = hs.into_iter().flat_map(|h| h.join().unwrap()).collect();
    assert!(bad.is_empty(), "{} failed requests, the first: {:?}", bad.len(), bad.first());
    let (mut sock, mut r) = connect(port);
    let n = clients * each;
    let (code, body) = call(&mut sock, &mut r, "GET", "/check", "");
    assert_eq!((code, body), (200, format!("{{\"rows\":{n},\"distinct_seq\":{n},\"max_seq\":{n}}}")));
    assert_eq!(call(&mut sock, &mut r, "GET", "/missing", ""), (404, "{\"error\":\"not found\"}".to_string()));
    let (code, body) = call(&mut sock, &mut r, "POST", "/dup", "");
    assert_eq!(code, 409, "{body}");
    assert!(body.contains("UNIQUE constraint failed: items.seq"), "{body}");
}

#[test]
fn http_server_writes_to_sqlite_from_concurrent_handlers() {
    concurrent_writes("", 16, 64);
}

#[test]
fn http_server_writes_to_sqlite_interpreted() {
    concurrent_writes("--interp", 8, 32);
}
