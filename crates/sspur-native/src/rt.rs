//! The C runtimes behind the data libraries (ADR 0037): SQLite, crypto, and compression with
//! CSV. Each is its glue file plus vendored parts (`vendor/`, packed by `build.rs`), built once
//! by the C compiler into a shared library in the cache the first time a program needs it, and
//! then loaded by native programs (through a path the generated C embeds) and by the
//! interpreter (through `libloading`). A program that uses none of them builds and loads nothing.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

mod vendor {
    include!(concat!(env!("OUT_DIR"), "/vendor.rs"));
}

/// A runtime: its name in the cache, its glue and the libraries it links on each system.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Db,
    Crypto,
    Zip,
    Net,
}

pub const DB_SRC: &str = include_str!("db_rt.c");
pub const CRYPTO_SRC: &str = include_str!("crypto_rt.c");
pub const ZIP_SRC: &str = include_str!("zip_rt.c");

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Db => "db",
            Kind::Crypto => "crypto",
            Kind::Zip => "zip",
            Kind::Net => "net",
        }
    }

    pub fn glue(self) -> &'static str {
        match self {
            Kind::Db => DB_SRC,
            Kind::Crypto => CRYPTO_SRC,
            Kind::Zip => ZIP_SRC,
            Kind::Net => crate::net::SRC,
        }
    }

    fn libs(self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        if cfg!(windows) {
            match self {
                Kind::Crypto => v.push("-lbcrypt".into()),
                Kind::Net => v.extend(["-lws2_32", "-lwinhttp", "-lcrypt32"].map(String::from)),
                _ => {}
            }
            v.extend(crate::cgen::flags::sys_libs());
        } else if !cfg!(target_os = "macos") {
            v.extend(["-lpthread", "-ldl", "-lm"].map(String::from));
        }
        v
    }
}

/// The vendored part `name`, decompressed.
pub fn part(name: &str) -> Result<String, String> {
    let (_, z) = vendor::PARTS.iter().find(|(n, _)| *n == name).ok_or_else(|| format!("no vendored part {name}"))?;
    let bytes = miniz_oxide::inflate::decompress_to_vec(z).map_err(|e| format!("vendored part {name} is damaged: {e:?}"))?;
    String::from_utf8(bytes).map_err(|_| format!("vendored part {name} is not UTF-8"))
}

fn part_bytes(name: &str) -> &'static [u8] {
    vendor::PARTS.iter().find(|(n, _)| *n == name).map_or(&[], |(_, z)| z)
}

/// The parts a glue file names on `//@part NAME` lines.
fn parts_of(glue: &str) -> Vec<&str> {
    glue.lines().filter_map(|l| l.strip_prefix("//@part ")).map(str::trim).collect()
}

/// The whole C source of a runtime: the glue with each `//@part` line replaced by that part.
pub fn source(kind: Kind) -> Result<String, String> {
    let mut out = String::new();
    for line in kind.glue().split_inclusive('\n') {
        match line.strip_prefix("//@part ") {
            Some(name) => {
                out.push_str(&part(name.trim())?);
                out.push('\n');
            }
            None => out.push_str(line),
        }
    }
    Ok(out)
}

fn lib_name(kind: Kind, key: &str) -> String {
    let ext = std::env::consts::DLL_EXTENSION;
    if cfg!(windows) { format!("sspur_{}_{key}.{ext}", kind.name()) } else { format!("libsspur_{}_{key}.{ext}", kind.name()) }
}

fn key(kind: Kind, cc: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(format!("{cc}\n{}\n{}\n", std::env::consts::ARCH, kind.name()).as_bytes());
    h.update(kind.glue().as_bytes());
    for p in parts_of(kind.glue()) {
        h.update(part_bytes(p));
    }
    h.finalize().to_hex()[..16].to_string()
}

/// The shared library of `kind`, building it on first use.
pub fn library(kind: Kind) -> Result<PathBuf, String> {
    static LIBS: [OnceLock<Result<PathBuf, String>>; 4] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];
    let i = kind as usize;
    LIBS[i].get_or_init(|| build(kind)).clone()
}

fn build(kind: Kind) -> Result<PathBuf, String> {
    let cc = crate::cgen::flags::cc();
    let key = key(kind, &cc);
    let base = sspur_syntax::cache_root().unwrap_or_else(std::env::temp_dir).join("native");
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let lib = base.join(lib_name(kind, &key));
    if lib.exists() {
        return Ok(lib);
    }
    let _lock = Lock::take(&base.join(format!("sspur_{}_{key}.lock", kind.name())), &lib);
    if lib.exists() {
        return Ok(lib);
    }
    let what = match kind {
        Kind::Db => "the SQLite runtime",
        Kind::Crypto => "the crypto runtime",
        Kind::Zip => "the compression runtime",
        Kind::Net => "the network runtime",
    };
    let src_text = source(kind)?;
    let uniq = format!("{}-{}", std::process::id(), key);
    let src = base.join(format!("sspur_{}_{uniq}.c", kind.name()));
    std::fs::write(&src, src_text).map_err(|e| e.to_string())?;
    let tmp = base.join(format!("sspur_{}_{uniq}.tmp.{}", kind.name(), std::env::consts::DLL_EXTENSION));
    let mut cmd = Command::new(&cc);
    cmd.args(["-O2", "-shared", "-w"]).args(crate::cgen::flags::pic());
    if cfg!(target_os = "macos") {
        cmd.arg(format!("-Wl,-install_name,{}", lib.display()));
    }
    cmd.arg("-o").arg(&tmp).arg(&src).args(kind.libs());
    let out = cmd.output().map_err(|e| format!("cannot run {cc}: {e}"))?;
    let _ = std::fs::remove_file(&src);
    if !out.status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{cc} failed to build {what}: {}", String::from_utf8_lossy(&out.stderr).lines().filter(|l| !l.trim().is_empty()).take(6).collect::<Vec<_>>().join(" | ")));
    }
    crate::cgen::flags::drop_link_leftovers(&tmp);
    crate::cgen::flags::publish(&tmp, &lib)?;
    Ok(lib)
}

/// A build lock shared by processes: the first to create the file builds, the others wait for
/// the library (or for the lock to go stale, after ten minutes without the builder finishing).
struct Lock(Option<PathBuf>);

impl Lock {
    fn take(path: &Path, lib: &Path) -> Lock {
        let start = std::time::Instant::now();
        loop {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(_) => return Lock(Some(path.to_path_buf())),
                Err(_) => {
                    if lib.exists() {
                        return Lock(None);
                    }
                    let stale = std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_none_or(|age| age.as_secs() > 600);
                    if stale || start.elapsed().as_secs() > 900 {
                        let _ = std::fs::remove_file(path);
                        if start.elapsed().as_secs() > 900 {
                            return Lock(None);
                        }
                        continue;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        if let Some(p) = &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// A loaded runtime, for the interpreter. The library stays loaded for the life of the process.
pub fn load(kind: Kind) -> Result<&'static libloading::Library, String> {
    static LOADED: [OnceLock<Result<&'static libloading::Library, String>>; 4] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];
    LOADED[kind as usize]
        .get_or_init(|| {
            let path = library(kind)?;
            let lib = unsafe { libloading::Library::new(&path) }.map_err(|e| format!("cannot load {}: {e}", path.display()))?;
            Ok(&*Box::leak(Box::new(lib)))
        })
        .clone()
}


macro_rules! api {
    ($kind:expr, $ty:ident { $($f:ident : $t:ty = $sym:literal),* $(,)? }) => {
        struct $ty { $($f: $t),* }
        impl $ty {
            fn get() -> Result<&'static $ty, String> {
                static API: OnceLock<Result<$ty, String>> = OnceLock::new();
                API.get_or_init(|| {
                    let lib = load($kind)?;
                    Ok($ty { $($f: *unsafe { lib.get::<$t>($sym) }.map_err(|e| e.to_string())?),* })
                })
                .as_ref()
                .map_err(|e| e.clone())
            }
        }
    };
}

api!(Kind::Crypto, Crypto {
    hash: unsafe extern "C" fn(i64, *const u8, i64, *mut u8) = b"sc_hash",
    hmac: unsafe extern "C" fn(i64, *const u8, i64, *const u8, i64, *mut u8) = b"sc_hmac",
    random: unsafe extern "C" fn(*mut u8, i64) -> i64 = b"sc_random",
    equal: unsafe extern "C" fn(*const u8, i64, *const u8, i64) -> i64 = b"sc_equal",
    seal: unsafe extern "C" fn(*const u8, *const u8, *const u8, i64, *const u8, i64, *mut u8) = b"sc_seal",
    open: unsafe extern "C" fn(*const u8, *const u8, *const u8, i64, *const u8, i64, *mut u8) -> i64 = b"sc_open",
    argon2: unsafe extern "C" fn(i64, *const u8, i64, *const u8, i64, *const u8, i64, *const u8, i64, i64, i64, i64, *mut u8, i64) -> i64 = b"sc_argon2",
    ed_public: unsafe extern "C" fn(*const u8, *mut u8) = b"sc_ed25519_public",
    ed_sign: unsafe extern "C" fn(*const u8, *const u8, i64, *mut u8) = b"sc_ed25519_sign",
    ed_verify: unsafe extern "C" fn(*const u8, *const u8, i64, *const u8) -> i64 = b"sc_ed25519_verify",
});

/// The messages for misuse that traps, the same in both tiers.
pub fn key_len_msg(what: &str, want: usize, got: usize) -> String {
    format!("{what}: the key must be {want} bytes, got {got}")
}

/// `alg` 1 SHA-256, 2 SHA-512, 3 BLAKE2b-512.
pub fn hash(alg: i64, data: &[u8]) -> Result<Vec<u8>, String> {
    let a = Crypto::get()?;
    let mut out = vec![0u8; if alg == 1 { 32 } else { 64 }];
    unsafe { (a.hash)(alg, data.as_ptr(), data.len() as i64, out.as_mut_ptr()) };
    Ok(out)
}

/// `alg` 1 HMAC-SHA-256, 2 HMAC-SHA-512.
pub fn hmac(alg: i64, key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    let a = Crypto::get()?;
    let mut out = vec![0u8; if alg == 1 { 32 } else { 64 }];
    unsafe { (a.hmac)(alg, key.as_ptr(), key.len() as i64, data.as_ptr(), data.len() as i64, out.as_mut_ptr()) };
    Ok(out)
}

pub fn random(n: usize) -> Result<Vec<u8>, String> {
    let a = Crypto::get()?;
    let mut out = vec![0u8; n];
    if unsafe { (a.random)(out.as_mut_ptr(), n as i64) } != 0 {
        return Err("the system's random number generator failed".into());
    }
    Ok(out)
}

pub fn equal(x: &[u8], y: &[u8]) -> Result<bool, String> {
    let a = Crypto::get()?;
    Ok(unsafe { (a.equal)(x.as_ptr(), x.len() as i64, y.as_ptr(), y.len() as i64) } != 0)
}

/// XChaCha20-Poly1305 with a 32-byte key and a 24-byte nonce: the cipher text and tag, or for
/// `seal == false` the plain text (`None` when the tag doesn't match). Wrong key or nonce lengths
/// are `Err` with the message both tiers trap with.
pub fn aead(seal: bool, key: &[u8], nonce: &[u8], ad: &[u8], data: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let what = if seal { "crypto.seal" } else { "crypto.open" };
    if key.len() != 32 {
        return Err(key_len_msg(what, 32, key.len()));
    }
    if nonce.len() != 24 {
        return Err(format!("{what}: the nonce must be 24 bytes, got {}", nonce.len()));
    }
    let a = Crypto::get()?;
    if seal {
        let mut out = vec![0u8; data.len() + 16];
        unsafe { (a.seal)(key.as_ptr(), nonce.as_ptr(), ad.as_ptr(), ad.len() as i64, data.as_ptr(), data.len() as i64, out.as_mut_ptr()) };
        return Ok(Some(out));
    }
    if data.len() < 16 {
        return Ok(None);
    }
    let mut out = vec![0u8; data.len() - 16];
    let r = unsafe { (a.open)(key.as_ptr(), nonce.as_ptr(), ad.as_ptr(), ad.len() as i64, data.as_ptr(), data.len() as i64, out.as_mut_ptr()) };
    Ok((r == 0).then_some(out))
}

/// Argon2 (`alg` 0 d, 1 i, 2 id): `None` for parameters out of range or memory that can't be had.
#[allow(clippy::too_many_arguments)]
pub fn argon2(alg: i64, pw: &[u8], salt: &[u8], key: &[u8], ad: &[u8], m: i64, t: i64, p: i64, n: i64) -> Result<Option<Vec<u8>>, String> {
    let a = Crypto::get()?;
    if !(4..=1024).contains(&n) {
        return Ok(None);
    }
    let mut out = vec![0u8; n as usize];
    let r = unsafe { (a.argon2)(alg, pw.as_ptr(), pw.len() as i64, salt.as_ptr(), salt.len() as i64, key.as_ptr(), key.len() as i64, ad.as_ptr(), ad.len() as i64, m, t, p, out.as_mut_ptr(), n) };
    Ok((r == 0).then_some(out))
}

pub fn ed_public(seed: &[u8]) -> Result<Vec<u8>, String> {
    if seed.len() != 32 {
        return Err(key_len_msg("crypto.public_key", 32, seed.len()));
    }
    let a = Crypto::get()?;
    let mut pk = vec![0u8; 32];
    unsafe { (a.ed_public)(seed.as_ptr(), pk.as_mut_ptr()) };
    Ok(pk)
}

pub fn ed_sign(seed: &[u8], msg: &[u8]) -> Result<Vec<u8>, String> {
    if seed.len() != 32 {
        return Err(key_len_msg("crypto.sign", 32, seed.len()));
    }
    let a = Crypto::get()?;
    let mut sig = vec![0u8; 64];
    unsafe { (a.ed_sign)(seed.as_ptr(), msg.as_ptr(), msg.len() as i64, sig.as_mut_ptr()) };
    Ok(sig)
}

pub fn ed_verify(pk: &[u8], msg: &[u8], sig: &[u8]) -> Result<bool, String> {
    if pk.len() != 32 || sig.len() != 64 {
        return Ok(false);
    }
    let a = Crypto::get()?;
    Ok(unsafe { (a.ed_verify)(pk.as_ptr(), msg.as_ptr(), msg.len() as i64, sig.as_ptr()) } != 0)
}

#[repr(C)]
struct SzBuf {
    p: *mut u8,
    n: i64,
    cap: i64,
}

#[repr(C)]
struct SzStr {
    p: *const u8,
    n: i64,
}

#[repr(C)]
struct SvRes {
    code: i64,
    msg: *mut u8,
    msg_n: i64,
    text: *const u8,
    ends: *const i64,
    rows: *const i64,
    nrows: i64,
}

api!(Kind::Zip, Zip {
    pack: unsafe extern "C" fn(i64, i64, *const u8, i64, *mut SzBuf) -> i64 = b"sz_pack",
    unpack: unsafe extern "C" fn(i64, *const u8, i64, *mut SzBuf, *mut u8, i64) -> i64 = b"sz_unpack",
    file: unsafe extern "C" fn(i64, *const std::ffi::c_char, *const std::ffi::c_char, *mut u8, i64) -> i64 = b"sz_file",
    free: unsafe extern "C" fn(*mut u8) = b"sz_free",
    parse: unsafe extern "C" fn(*const u8, i64, *mut SvRes) = b"sv_parse",
    res_free: unsafe extern "C" fn(*mut SvRes) = b"sv_res_free",
    open: unsafe extern "C" fn(*const std::ffi::c_char, *mut u8, i64) -> *mut std::ffi::c_void = b"sv_open",
    next: unsafe extern "C" fn(*mut std::ffi::c_void, i64, *mut SvRes) = b"sv_next",
    close: unsafe extern "C" fn(*mut std::ffi::c_void) = b"sv_close",
    write: unsafe extern "C" fn(*const SzStr, *const i64, i64, *mut SzBuf) = b"sv_write",
});

fn take(z: &Zip, b: SzBuf) -> Vec<u8> {
    let v = if b.p.is_null() { vec![] } else { unsafe { std::slice::from_raw_parts(b.p, b.n as usize) }.to_vec() };
    unsafe { (z.free)(b.p) };
    v
}

fn cstr_err(buf: &[u8]) -> String {
    let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

/// `fmt` 1 gzip, 2 raw DEFLATE, 3 zstd.
pub fn pack(fmt: i64, level: i64, data: &[u8]) -> Result<Vec<u8>, String> {
    let z = Zip::get()?;
    let mut out = SzBuf { p: std::ptr::null_mut(), n: 0, cap: 0 };
    let r = unsafe { (z.pack)(fmt, level, data.as_ptr(), data.len() as i64, &mut out) };
    let v = take(z, out);
    if r != 0 {
        return Err("out of memory".into());
    }
    Ok(v)
}

pub fn unpack(fmt: i64, data: &[u8]) -> Result<Result<Vec<u8>, String>, String> {
    let z = Zip::get()?;
    let mut out = SzBuf { p: std::ptr::null_mut(), n: 0, cap: 0 };
    let mut err = vec![0u8; 512];
    let r = unsafe { (z.unpack)(fmt, data.as_ptr(), data.len() as i64, &mut out, err.as_mut_ptr(), err.len() as i64) };
    let v = take(z, out);
    Ok(if r == 0 { Ok(v) } else { Err(cstr_err(&err)) })
}

/// `op` 1 gzip, 2 gunzip, 3 zstd, 4 unzstd, file to file: `""` or the error.
pub fn zip_file(op: i64, src: &str, dst: &str) -> Result<String, String> {
    let z = Zip::get()?;
    let (Ok(s), Ok(d)) = (std::ffi::CString::new(src), std::ffi::CString::new(dst)) else { return Ok(format!("{src}: invalid path")) };
    let mut err = vec![0u8; 1024];
    let r = unsafe { (z.file)(op, s.as_ptr(), d.as_ptr(), err.as_mut_ptr(), err.len() as i64) };
    Ok(if r == 0 { String::new() } else { cstr_err(&err) })
}

fn records(z: &Zip, mut r: SvRes) -> Result<Vec<Vec<String>>, String> {
    let out = if r.code != 0 {
        Err(if r.msg.is_null() { "out of memory".to_string() } else { String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(r.msg, r.msg_n as usize) }).into_owned() })
    } else {
        let mut rows = Vec::with_capacity(r.nrows as usize);
        let mut f = 0usize;
        let mut start = 0i64;
        for k in 0..r.nrows as usize {
            let end = unsafe { *r.rows.add(k) } as usize;
            let mut row = Vec::with_capacity(end - f);
            while f < end {
                let e = unsafe { *r.ends.add(f) };
                let b = if e > start { unsafe { std::slice::from_raw_parts(r.text.add(start as usize), (e - start) as usize) } } else { &[][..] };
                row.push(String::from_utf8_lossy(b).into_owned());
                start = e;
                f += 1;
            }
            rows.push(row);
        }
        Ok(rows)
    };
    unsafe { (z.res_free)(&mut r) };
    out
}

/// An RFC 4180 parse: the records, or the error.
pub fn csv_parse(text: &str) -> Result<Result<Vec<Vec<String>>, String>, String> {
    let z = Zip::get()?;
    let mut r = SvRes { code: 0, msg: std::ptr::null_mut(), msg_n: 0, text: std::ptr::null(), ends: std::ptr::null(), rows: std::ptr::null(), nrows: 0 };
    unsafe { (z.parse)(text.as_ptr(), text.len() as i64, &mut r) };
    Ok(records(z, r))
}

/// Opens a CSV file for `csv_next`: a handle (never 0) or the error.
pub fn csv_open(path: &str) -> Result<Result<i64, String>, String> {
    let z = Zip::get()?;
    let Ok(p) = std::ffi::CString::new(path) else { return Ok(Err(format!("{path}: invalid path"))) };
    let mut err = vec![0u8; 1024];
    let h = unsafe { (z.open)(p.as_ptr(), err.as_mut_ptr(), err.len() as i64) };
    Ok(if h.is_null() { Err(cstr_err(&err)) } else { Ok(h as i64) })
}

pub fn csv_next(h: i64, max: i64) -> Result<Result<Vec<Vec<String>>, String>, String> {
    let z = Zip::get()?;
    let mut r = SvRes { code: 0, msg: std::ptr::null_mut(), msg_n: 0, text: std::ptr::null(), ends: std::ptr::null(), rows: std::ptr::null(), nrows: 0 };
    unsafe { (z.next)(h as *mut std::ffi::c_void, max, &mut r) };
    Ok(records(z, r))
}

pub fn csv_close(h: i64) -> Result<(), String> {
    let z = Zip::get()?;
    unsafe { (z.close)(h as *mut std::ffi::c_void) };
    Ok(())
}

pub fn csv_write(rows: &[Vec<String>]) -> Result<String, String> {
    let z = Zip::get()?;
    let cells: Vec<SzStr> = rows.iter().flatten().map(|c| SzStr { p: c.as_ptr(), n: c.len() as i64 }).collect();
    let counts: Vec<i64> = rows.iter().map(|r| r.len() as i64).collect();
    let mut out = SzBuf { p: std::ptr::null_mut(), n: 0, cap: 0 };
    unsafe { (z.write)(cells.as_ptr(), counts.as_ptr(), counts.len() as i64, &mut out) };
    Ok(String::from_utf8_lossy(&take(z, out)).into_owned())
}

/// A SQL value: 0 NULL, 1 integer, 2 real, 3 text, 4 blob.
#[derive(Clone, Debug, PartialEq)]
pub enum SqlVal {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

#[repr(C)]
struct SdVal {
    kind: i64,
    i: i64,
    f: f64,
    s: *const u8,
    n: i64,
}

#[repr(C)]
struct SdRes {
    code: i64,
    msg: *mut u8,
    msg_n: i64,
    ncols: i64,
    cols: *const SzStr,
    nrows: i64,
    cells: *const SdVal,
    changes: i64,
    arena: *mut u8,
}

api!(Kind::Db, Db {
    open: unsafe extern "C" fn(*const u8, i64, *mut u8, i64) -> i64 = b"sd_open",
    close: unsafe extern "C" fn(i64) = b"sd_close",
    exec: unsafe extern "C" fn(i64, *const u8, i64, *const SdVal, i64, i64, *mut SdRes) = b"sd_exec",
    res_free: unsafe extern "C" fn(*mut SdRes) = b"sd_res_free",
    tx: unsafe extern "C" fn(i64, i64, *mut u8, i64) -> i64 = b"sd_tx",
    release: unsafe extern "C" fn() = b"sd_release_thread",
});

/// Opens a database: its id, or the error kind (1 SQL) and message.
pub fn db_open(path: &str) -> Result<Result<i64, (i64, String)>, String> {
    let d = Db::get()?;
    db_used();
    let mut err = vec![0u8; 1024];
    let id = unsafe { (d.open)(path.as_ptr(), path.len() as i64, err.as_mut_ptr(), err.len() as i64) };
    Ok(if id > 0 { Ok(id) } else { Err((1, cstr_err(&err))) })
}

pub fn db_close(id: i64) -> Result<(), String> {
    let d = Db::get()?;
    unsafe { (d.close)(id) };
    Ok(())
}

/// What a statement returned.
pub struct DbOut {
    pub cols: Vec<String>,
    pub rows: Vec<Vec<SqlVal>>,
    pub changes: i64,
}

/// Runs SQL: the result, or the error kind (1 SQL, 2 constraint, 3 busy) and message.
pub fn db_exec(id: i64, sql: &str, params: &[SqlVal], want: i64) -> Result<Result<DbOut, (i64, String)>, String> {
    let d = Db::get()?;
    let ps: Vec<SdVal> = params
        .iter()
        .map(|v| match v {
            SqlVal::Null => SdVal { kind: 0, i: 0, f: 0.0, s: std::ptr::null(), n: 0 },
            SqlVal::Int(i) => SdVal { kind: 1, i: *i, f: 0.0, s: std::ptr::null(), n: 0 },
            SqlVal::Real(f) => SdVal { kind: 2, i: 0, f: *f, s: std::ptr::null(), n: 0 },
            SqlVal::Text(s) => SdVal { kind: 3, i: 0, f: 0.0, s: s.as_ptr(), n: s.len() as i64 },
            SqlVal::Blob(b) => SdVal { kind: 4, i: 0, f: 0.0, s: b.as_ptr(), n: b.len() as i64 },
        })
        .collect();
    let mut r = SdRes { code: 0, msg: std::ptr::null_mut(), msg_n: 0, ncols: 0, cols: std::ptr::null(), nrows: 0, cells: std::ptr::null(), changes: 0, arena: std::ptr::null_mut() };
    unsafe { (d.exec)(id, sql.as_ptr(), sql.len() as i64, ps.as_ptr(), ps.len() as i64, want, &mut r) };
    let bytes = |p: *const u8, n: i64| if p.is_null() || n <= 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(p, n as usize) } };
    let out = if r.code != 0 {
        Err((r.code, String::from_utf8_lossy(bytes(r.msg, r.msg_n)).into_owned()))
    } else {
        let cols: Vec<String> = (0..r.ncols as usize).map(|i| unsafe { &*r.cols.add(i) }).map(|c| String::from_utf8_lossy(bytes(c.p, c.n)).into_owned()).collect();
        let nc = r.ncols as usize;
        let mut rows = Vec::with_capacity(r.nrows as usize);
        for k in 0..r.nrows as usize {
            let mut row = Vec::with_capacity(nc);
            for j in 0..nc {
                let v = unsafe { &*r.cells.add(k * nc + j) };
                row.push(match v.kind {
                    1 => SqlVal::Int(v.i),
                    2 => SqlVal::Real(v.f),
                    3 => SqlVal::Text(String::from_utf8_lossy(bytes(v.s, v.n)).into_owned()),
                    4 => SqlVal::Blob(bytes(v.s, v.n).to_vec()),
                    _ => SqlVal::Null,
                });
            }
            rows.push(row);
        }
        Ok(DbOut { cols, rows, changes: r.changes })
    };
    unsafe { (d.res_free)(&mut r) };
    Ok(out)
}

/// `op` 0 begin, 1 commit, 2 roll back: `Ok(())` or the error kind and message.
pub fn db_tx(id: i64, op: i64) -> Result<Result<(), (i64, String)>, String> {
    let d = Db::get()?;
    let mut err = vec![0u8; 1024];
    let k = unsafe { (d.tx)(id, op, err.as_mut_ptr(), err.len() as i64) };
    Ok(if k == 0 { Ok(()) } else { Err((k, cstr_err(&err))) })
}

/// Rolls back what this thread holds, if a database was ever opened.
pub fn db_release_thread() {
    if DB_USED.load(std::sync::atomic::Ordering::Acquire)
        && let Ok(d) = Db::get()
    {
        unsafe { (d.release)() };
    }
}

static DB_USED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn db_used() {
    DB_USED.store(true, std::sync::atomic::Ordering::Release);
}
