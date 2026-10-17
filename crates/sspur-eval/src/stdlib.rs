use crate::value::Value;
use crate::{from_nval, to_nval, trap, Interp, R};
use sspur_syntax::{Expr, Span};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

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

fn list(v: &Value) -> R<Rc<Vec<Value>>> {
    match v {
        Value::List(xs) => Ok(xs.clone()),
        v => trap(format!("expected List, got {v}")),
    }
}

fn pair(a: Value, b: Value) -> Value {
    Value::Tuple(Rc::new(vec![a, b]))
}

fn opt(o: Option<Value>) -> Value {
    Value::Opt(o.map(Rc::new))
}

fn ints(xs: impl Iterator<Item = i64>) -> Value {
    Value::list(xs.map(Value::Int).collect())
}

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn rng(s: &mut u64) -> u64 {
    *s = s.wrapping_add(GOLDEN);
    mix(*s)
}

fn rng_f64(s: &mut u64) -> f64 {
    (rng(s) >> 11) as f64 * (1.0 / 9007199254740992.0)
}

fn rng_below(s: &mut u64, n: usize) -> usize {
    ((rng(s) as u128 * n as u128) >> 64) as usize
}

fn rng_normal(s: &mut u64, mean: f64, sd: f64) -> f64 {
    let u1 = rng_f64(s);
    let u2 = rng_f64(s);
    let r = (-2.0 * (1.0 - u1).ln()).sqrt();
    let c = (std::f64::consts::TAU * u2).cos();
    let z = r * c;
    let t = sd * z;
    mean + t
}

#[cfg(target_os = "macos")]
const ENOTEMPTY: i32 = 66;
#[cfg(not(any(target_os = "macos", windows)))]
const ENOTEMPTY: i32 = 39;

#[cfg(windows)]
pub fn os_reason(e: &std::io::Error) -> String {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound => "not found".into(),
        K::PermissionDenied => "permission denied".into(),
        K::IsADirectory => "is a directory".into(),
        K::NotADirectory => "not a directory".into(),
        K::AlreadyExists => "already exists".into(),
        K::DirectoryNotEmpty => "directory not empty".into(),
        _ => match e.raw_os_error() {
            Some(n) => format!("os error {n}"),
            None => "invalid UTF-8".into(),
        },
    }
}

#[cfg(not(windows))]
pub fn os_reason(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(2) => "not found".into(),
        Some(1 | 13) => "permission denied".into(),
        Some(21) => "is a directory".into(),
        Some(20) => "not a directory".into(),
        Some(17) => "already exists".into(),
        Some(ENOTEMPTY) => "directory not empty".into(),
        Some(n) => format!("os error {n}"),
        None => "invalid UTF-8".into(),
    }
}

// Windows reports opening a directory as a file as access denied, where POSIX says EISDIR.
#[cfg(windows)]
pub fn open_err(p: &str, e: std::io::Error) -> std::io::Error {
    if e.kind() == std::io::ErrorKind::PermissionDenied && std::fs::metadata(p).is_ok_and(|m| m.is_dir()) { std::io::ErrorKind::IsADirectory.into() } else { e }
}

#[cfg(not(windows))]
pub fn open_err(_: &str, e: std::io::Error) -> std::io::Error {
    e
}

fn io_res(path: &str, r: Result<Value, std::io::Error>) -> Value {
    if path.contains('\0') {
        return Value::Res(Err(Rc::new(Value::str(&format!("{path}: invalid path")))));
    }
    match r {
        Ok(v) => Value::Res(Ok(Rc::new(v))),
        Err(e) => Value::Res(Err(Rc::new(Value::str(&format!("{path}: {}", os_reason(&e)))))),
    }
}

fn fs_msg(path: &str, m: &str) -> Value {
    Value::Res(Err(Rc::new(Value::str(&format!("{path}: {m}")))))
}

#[cfg(unix)]
fn same_file(meta: &std::fs::Metadata, to: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(to).is_ok_and(|m| m.dev() == meta.dev() && m.ino() == meta.ino())
}

#[cfg(not(unix))]
fn same_file(_: &std::fs::Metadata, from_to: (&str, &str)) -> bool {
    matches!((std::fs::canonicalize(from_to.0), std::fs::canonicalize(from_to.1)), (Ok(a), Ok(b)) if a == b)
}

#[cfg(unix)]
fn file_mode_of(m: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    m.mode() & 0o7777
}

#[cfg(not(unix))]
fn file_mode_of(m: &std::fs::Metadata) -> u32 {
    match (m.is_dir(), m.permissions().readonly()) {
        (true, true) => 0o555,
        (true, false) => 0o755,
        (false, true) => 0o444,
        (false, false) => 0o644,
    }
}

#[cfg(unix)]
fn permissions(mode: u32) -> std::fs::Permissions {
    use std::os::unix::fs::PermissionsExt;
    std::fs::Permissions::from_mode(mode)
}

#[cfg(not(unix))]
fn set_mode(p: &str, mode: u32) -> std::io::Result<()> {
    let mut perm = std::fs::metadata(p)?.permissions();
    perm.set_readonly(mode & 0o200 == 0);
    std::fs::set_permissions(p, perm)
}

#[cfg(unix)]
fn set_mode(p: &str, mode: u32) -> std::io::Result<()> {
    std::fs::set_permissions(p, permissions(mode))
}

#[cfg(unix)]
fn modified_ms(m: &std::fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    m.mtime() * 1000 + m.mtime_nsec() / 1_000_000
}

#[cfg(not(unix))]
fn modified_ms(m: &std::fs::Metadata) -> i64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64)
}

fn copy_file(from: &str, to: &str) -> Value {
    use std::io::{Read, Write};
    let err = |p: &str, e: std::io::Error| fs_msg(p, &os_reason(&open_err(p, e)));
    if from.contains('\0') {
        return fs_msg(from, "invalid path");
    }
    if to.contains('\0') {
        return fs_msg(to, "invalid path");
    }
    let mut src = match std::fs::File::open(from) {
        Ok(f) => f,
        Err(e) => return err(from, e),
    };
    let meta = match src.metadata() {
        Ok(m) => m,
        Err(e) => return err(from, e),
    };
    if meta.is_dir() {
        return fs_msg(from, "is a directory");
    }
    #[cfg(unix)]
    let same = same_file(&meta, to);
    #[cfg(not(unix))]
    let same = same_file(&meta, (from, to));
    if same {
        return fs_msg(from, "same file");
    }
    let mode = file_mode_of(&meta);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, mode);
    let mut dst = match opts.open(to) {
        Ok(f) => f,
        Err(e) => return err(to, e),
    };
    let mut buf = vec![0u8; 65536];
    loop {
        let k = match src.read(&mut buf) {
            Ok(k) => k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return err(from, e),
        };
        if k == 0 {
            break;
        }
        if let Err(e) = dst.write_all(&buf[..k]) {
            return err(to, e);
        }
    }
    #[cfg(unix)]
    if let Err(e) = dst.set_permissions(permissions(mode)) {
        return err(to, e);
    }
    #[cfg(windows)]
    drop((dst, mode));
    #[cfg(target_family = "wasm")]
    let _ = (dst, mode);
    Value::Res(Ok(Rc::new(Value::Unit)))
}

pub use crate::sys::mono_ns;

fn run_cmd(prog: &str, args: &[String], input: String) -> Result<Value, String> {
    use std::process::{Command, Stdio};
    if prog.contains('\0') {
        return Err("invalid path".into());
    }
    if args.iter().any(|a| a.contains('\0')) {
        return Err("invalid argument".into());
    }
    let mut child = Command::new(prog).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| os_reason(&e))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        let _ = stdin.write_all(input.as_bytes());
    });
    let out = child.wait_with_output().map_err(|e| os_reason(&e))?;
    let _ = writer.join();
    #[cfg(unix)]
    let code = out.status.code().map_or_else(|| 128 + i64::from(std::os::unix::process::ExitStatusExt::signal(&out.status).unwrap_or(0)), i64::from);
    #[cfg(not(unix))]
    let code = out.status.code().map_or(-1, i64::from);
    let text = |b: Vec<u8>| String::from_utf8(b).map_err(|_| "invalid UTF-8".to_string());
    let (so, se) = (text(out.stdout)?, text(out.stderr)?);
    Ok(Value::Tuple(Rc::new(vec![Value::Int(code), Value::str(&so), Value::str(&se)])))
}

fn mkdir_all(p: &str) -> Result<Value, std::io::Error> {
    if p.is_empty() {
        return Err(std::io::Error::from_raw_os_error(2));
    }
    let b = p.as_bytes();
    for i in 1..=b.len() {
        if i < b.len() && b[i] != b'/' {
            continue;
        }
        if let Err(e) = std::fs::create_dir(&p[..i])
            && !(e.kind() == std::io::ErrorKind::AlreadyExists && std::fs::metadata(&p[..i]).is_ok_and(|m| m.is_dir())) {
                return Err(e);
            }
    }
    Ok(Value::Unit)
}

fn read_line_raw() -> R<Option<String>> {
    let mut line = Vec::new();
    let mut got = false;
    while let Some(b) = crate::sys::read_byte(0) {
        got = true;
        if b == b'\n' {
            break;
        }
        line.push(b);
    }
    if !got {
        return Ok(None);
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    match String::from_utf8(line) {
        Ok(l) => Ok(Some(l)),
        Err(_) => trap("stdin is not valid UTF-8"),
    }
}

pub fn fmt_fixed(x: f64, d: usize) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "inf".into() } else { "-inf".into() }
    } else {
        format!("{x:.d$}")
    }
}

pub fn float_syntax(t: &str) -> bool {
    let b = t.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i = 1;
    }
    let d0 = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i > d0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f0 = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        digits |= i > f0;
    }
    if !digits {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let e0 = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == e0 {
            return false;
        }
    }
    i == b.len()
}

fn pad(x: &str, n: i64, fill: &str, left: bool) -> R {
    let len = x.chars().count() as i64;
    if fill.is_empty() || len >= n {
        return Ok(Value::str(x));
    }
    let fc = fill.chars().count() as i64;
    let copies = (n - len + fc - 1) / fc;
    if copies > (1i64 << 28) / fill.len() as i64 {
        return trap("out of memory");
    }
    let p: String = fill.repeat(copies as usize).chars().take((n - len) as usize).collect();
    Ok(Value::str(&if left { p + x } else { x.to_string() + &p }))
}

impl Interp {
    pub(crate) fn std_global(&self, n: &str, a: Vec<Value>) -> R {
        if let Some(r) = self.net_global(n, &a) {
            return r;
        }
        #[cfg(feature = "native")]
        if let Some(r) = self.lib_global(n, &a) {
            return r;
        }
        #[cfg(target_family = "wasm")]
        {
            if n == "eprint" {
                self.emit(format!("{}{}", crate::web::STDERR, s(&a[0])?));
                return Ok(Value::Unit);
            }
            if let Some(v) = crate::web::builtin(n, &a)? {
                return Ok(v);
            }
        }
        if let Some(v) = crate::stdrng::global(n, &a)? {
            return Ok(v);
        }
        if let Some(v) = crate::stdcx::global(n, &a)? {
            return Ok(v);
        }
        if let Some(v) = crate::stdratio::global(n, &a)? {
            return Ok(v);
        }
        if let Some(v) = crate::stdlocale::global(n, &a)? {
            return Ok(v);
        }
        if let Some(v) = crate::stdtz::global(n, &a)? {
            return Ok(v);
        }
        if let Some(v) = self.file_global(n, &a)? {
            return Ok(v);
        }
        if let Some(v) = crate::stdview::global(n, &a)? {
            return Ok(v);
        }
        #[cfg(not(target_family = "wasm"))]
        if let Some(v) = crate::stdbytes::global(n, &a)? {
            return Ok(v);
        }
        match n {
            "empty_flat_map" => return Ok(crate::stdflat::empty_flat()),
            "mdspan" => return crate::stdflat::mdspan(&a[0], &a[1]),
            _ => {}
        }
        Ok(match n {
            "empty_set" => Value::Set(Rc::new(BTreeSet::new())),
            "empty_heap" => Value::Heap(Rc::new(Vec::new())),
            "str_buf" => Value::Buf(Rc::new(String::new())),
            "dev_f32" | "dev_f64" | "dev_i32" | "dev_u32" | "dev_int" => crate::kernel::dev_new(n, &a[0])?,
            "gpu_sync" => {
                crate::kernel::sync()?;
                Value::Unit
            }
            "range" => {
                let (st, en, step) = (int(&a[0])? as i128, int(&a[1])? as i128, int(&a[2])? as i128);
                if step == 0 {
                    return trap("range step must not be 0");
                }
                let count = if step > 0 && en > st {
                    (en - st - 1) / step + 1
                } else if step < 0 && en < st {
                    (st - en - 1) / -step + 1
                } else {
                    0
                };
                if count > 1 << 26 {
                    return trap("out of memory");
                }
                ints((0..count).map(|k| (st + k * step) as i64))
            }
            "clamp" => {
                let mut it = a.into_iter();
                let (x, lo, hi) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
                if x < lo {
                    lo
                } else if x > hi {
                    hi
                } else {
                    x
                }
            }
            "rand" | "rand_int" | "rand_f64" => {
                let z = (int(&a[0])? as u64).wrapping_add(GOLDEN);
                let m = mix(z);
                let v = match n {
                    "rand" => Value::Int((m >> 1) as i64),
                    "rand_f64" => Value::Float((m >> 11) as f64 * (1.0 / 9007199254740992.0)),
                    _ => {
                        let (lo, hi) = (int(&a[1])?, int(&a[2])?);
                        if lo >= hi {
                            return trap("rand_int needs lo < hi");
                        }
                        let span = (hi as i128 - lo as i128) as u128;
                        Value::Int((lo as i128 + ((m as u128 * span) >> 64) as i128) as i64)
                    }
                };
                pair(v, Value::Int(z as i64))
            }
            "rand_normal" | "rand_uniform" | "rand_exp" | "rand_bool" => {
                let mut st = int(&a[0])? as u64;
                let f = |i: usize| match a.get(i) {
                    Some(Value::Float(x)) => Ok(*x),
                    _ => trap("expected F64"),
                };
                let v = match n {
                    "rand_normal" => Value::Float(rng_normal(&mut st, f(1)?, f(2)?)),
                    "rand_uniform" => {
                        let (lo, hi) = (f(1)?, f(2)?);
                        let u = rng_f64(&mut st);
                        let w = hi - lo;
                        let t = w * u;
                        Value::Float(lo + t)
                    }
                    "rand_exp" => {
                        let rate = f(1)?;
                        Value::Float(-(1.0 - rng_f64(&mut st)).ln() / rate)
                    }
                    _ => {
                        let p = f(1)?;
                        Value::Bool(rng_f64(&mut st) < p)
                    }
                };
                pair(v, Value::Int(st as i64))
            }
            "from_bytes" => {
                let xs = list(&a[0])?;
                let mut bs = Vec::with_capacity(xs.len());
                for x in xs.iter() {
                    match u8::try_from(int(x)?) {
                        Ok(b) => bs.push(b),
                        Err(_) => return Ok(opt(None)),
                    }
                }
                opt(String::from_utf8(bs).ok().map(|s| Value::str(&s)))
            }
            "from_codes" => {
                let xs = list(&a[0])?;
                let mut out = String::new();
                for x in xs.iter() {
                    match u32::try_from(int(x)?).ok().and_then(char::from_u32) {
                        Some(c) => out.push(c),
                        None => return Ok(opt(None)),
                    }
                }
                opt(Some(Value::str(&out)))
            }
            "read_file" => {
                let p = s(&a[0])?;
                let r = std::fs::read(p).map_err(|e| open_err(p, e)).and_then(|b| String::from_utf8(b).map_err(|_| std::io::Error::other("utf8"))).map(|t| Value::str(&t));
                io_res(p, r)
            }
            "write_file" | "append_file" => {
                use std::io::Write;
                let (p, text) = (s(&a[0])?, s(&a[1])?);
                let f = if n == "write_file" { std::fs::File::create(p) } else { std::fs::OpenOptions::new().append(true).create(true).open(p) };
                io_res(p, f.map_err(|e| open_err(p, e)).and_then(|mut f| f.write_all(text.as_bytes())).map(|_| Value::Unit))
            }
            "list_dir" => {
                let p = s(&a[0])?;
                let r = std::fs::read_dir(p).and_then(|rd| {
                    let mut names = Vec::new();
                    for e in rd {
                        names.push(e?.file_name().to_string_lossy().into_owned());
                    }
                    names.sort();
                    Ok(Value::list(names.iter().map(|x| Value::str(x)).collect()))
                });
                io_res(p, r)
            }
            "remove_file" => {
                let p = s(&a[0])?;
                io_res(p, std::fs::remove_file(p).map(|_| Value::Unit))
            }
            "read_bytes" => {
                let p = s(&a[0])?;
                io_res(p, std::fs::read(p).map_err(|e| open_err(p, e)).map(|b| ints(b.into_iter().map(i64::from))))
            }
            #[cfg(not(target_family = "wasm"))]
            "read_bin" => {
                let p = s(&a[0])?;
                io_res(p, std::fs::read(p).map_err(|e| open_err(p, e)).map(Value::bytes))
            }
            #[cfg(not(target_family = "wasm"))]
            "write_bin" => {
                let p = s(&a[0])?;
                let b = crate::stdbytes::of(&a[1])?;
                io_res(p, std::fs::write(p, b).map_err(|e| open_err(p, e)).map(|_| Value::Unit))
            }
            "write_bytes" => {
                let p = s(&a[0])?;
                let mut bs = Vec::new();
                for x in list(&a[1])?.iter() {
                    match u8::try_from(int(x)?) {
                        Ok(b) => bs.push(b),
                        Err(_) if !p.contains('\0') => return Ok(Value::Res(Err(Rc::new(Value::str(&format!("{p}: byte out of range")))))),
                        Err(_) => break,
                    }
                }
                io_res(p, std::fs::write(p, bs).map_err(|e| open_err(p, e)).map(|_| Value::Unit))
            }
            "mkdir" => {
                let p = s(&a[0])?;
                io_res(p, std::fs::create_dir(p).map(|_| Value::Unit))
            }
            "mkdir_all" => {
                let p = s(&a[0])?;
                io_res(p, mkdir_all(p))
            }
            "remove_dir" => {
                let p = s(&a[0])?;
                io_res(p, std::fs::remove_dir(p).map(|_| Value::Unit))
            }
            "rename" => {
                let (p, q) = (s(&a[0])?, s(&a[1])?);
                if q.contains('\0') {
                    return Ok(Value::Res(Err(Rc::new(Value::str(&format!("{p}: invalid path"))))));
                }
                io_res(p, std::fs::rename(p, q).map(|_| Value::Unit))
            }
            "copy_file" => copy_file(s(&a[0])?, s(&a[1])?),
            "symlink" => {
                let (t, l) = (s(&a[0])?, s(&a[1])?);
                if t.contains('\0') {
                    return Ok(fs_msg(l, "invalid path"));
                }
                #[cfg(unix)]
                let r = std::os::unix::fs::symlink(t, l);
                #[cfg(windows)]
                let r = if std::fs::metadata(t).is_ok_and(|m| m.is_dir()) { std::os::windows::fs::symlink_dir(t, l) } else { std::os::windows::fs::symlink_file(t, l) };
                #[cfg(not(any(unix, windows)))]
                let r: std::io::Result<()> = Err(std::io::ErrorKind::Unsupported.into());
                io_res(l, r.map(|_| Value::Unit))
            }
            "read_link" => {
                let p = s(&a[0])?;
                match std::fs::read_link(p) {
                    Err(e) if e.raw_os_error() == Some(if cfg!(windows) { 4390 } else { 22 }) && !p.contains('\0') => fs_msg(p, "not a symlink"),
                    Ok(t) => match t.into_os_string().into_string() {
                        Ok(t) => Value::Res(Ok(Rc::new(Value::str(&t)))),
                        Err(_) => fs_msg(p, "invalid UTF-8"),
                    },
                    Err(e) => io_res(p, Err(e)),
                }
            }
            "is_symlink" => {
                let p = s(&a[0])?;
                Value::Bool(!p.contains('\0') && std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
            }
            "file_mode" => {
                let p = s(&a[0])?;
                io_res(p, std::fs::metadata(p).map(|m| Value::Int(i64::from(file_mode_of(&m)))))
            }
            "set_mode" => {
                let (p, m) = (s(&a[0])?, int(&a[1])?);
                if !(0..=0o7777).contains(&m) && !p.contains('\0') {
                    return Ok(fs_msg(p, "mode out of range"));
                }
                io_res(p, set_mode(p, m as u32).map(|_| Value::Unit))
            }
            "exists" | "is_dir" => {
                let p = s(&a[0])?;
                let m = if p.contains('\0') { None } else { std::fs::metadata(p).ok() };
                Value::Bool(m.is_some_and(|m| n == "exists" || m.is_dir()))
            }
            "file_size" | "modified_ms" => {
                let p = s(&a[0])?;
                io_res(p, std::fs::metadata(p).map(|m| Value::Int(if n == "file_size" { m.len() as i64 } else { modified_ms(&m) })))
            }
            "run_cmd" => {
                let prog = s(&a[0])?;
                let args = list(&a[1])?.iter().map(|x| s(x).map(str::to_string)).collect::<R<Vec<_>>>()?;
                Value::Res(match run_cmd(prog, &args, s(&a[2])?.to_string()) {
                    Ok(v) => Ok(Rc::new(v)),
                    Err(m) => Err(Rc::new(Value::str(&format!("{prog}: {m}")))),
                })
            }
            "exit" => {
                use std::io::Write;
                let code = int(&a[0])?;
                let _ = std::io::stdout().flush();
                std::process::exit(code as i32)
            }
            "eprint" => {
                use std::io::Write;
                let _ = writeln!(std::io::stderr(), "{}", s(&a[0])?);
                Value::Unit
            }
            "read_line" => opt(read_line_raw()?.map(|l| Value::str(&l))),
            "read_lines" => {
                let mut out = Vec::new();
                while let Some(l) = read_line_raw()? {
                    out.push(Value::str(&l));
                }
                Value::list(out)
            }
            "now_ms" => Value::Int(crate::sys::wall_ms()),
            "mono_ns" => Value::Int(mono_ns()),
            "sleep_ms" => {
                crate::sys::sleep_ms(int(&a[0])?);
                Value::Unit
            }
            "env_var" => {
                let k = s(&a[0])?;
                if k.is_empty() || k.contains('=') || k.contains('\0') {
                    opt(None)
                } else {
                    opt(std::env::var(k).ok().map(|v| Value::str(&v)))
                }
            }
            "args" => Value::list(sspur_native::program_args().iter().map(|x| Value::str(x)).collect()),
            "pi" => Value::Float(std::f64::consts::PI),
            "euler" => Value::Float(std::f64::consts::E),
            "inf" => Value::Float(f64::INFINITY),
            "nan" => Value::Float(f64::NAN),
            "bits" => crate::stdx::bits_new(int(&a[0])?)?,
            "regex" => crate::stdre::global(&a)?,
            "big" | "parse_big" | "decimal" => crate::stdbig::global(n, &a)?,
            "hash_map" => Value::Map(Rc::new(BTreeMap::new())),
            "hash_set" => Value::Set(Rc::new(BTreeSet::new())),
            "time_ms" | "date" | "datetime" | "parse_time" | "now" | "millis" | "secs" | "mins" | "hours" | "days" => crate::stdtime::global(n, &a)?,
            _ => return trap(format!("unknown builtin '{n}'")),
        })
    }

    fn msort(&self, a: &mut [Value], f: &Value) -> R<()> {
        let n = a.len();
        if n < 2 {
            return Ok(());
        }
        let h = n / 2;
        self.msort(&mut a[..h], f)?;
        self.msort(&mut a[h..], f)?;
        let (mut i, mut j) = (0, h);
        let mut out = Vec::with_capacity(n);
        while i < h && j < n {
            if int(&self.apply(f, vec![a[j].clone(), a[i].clone()])?)? < 0 {
                out.push(a[j].clone());
                j += 1;
            } else {
                out.push(a[i].clone());
                i += 1;
            }
        }
        out.extend_from_slice(&a[i..h]);
        out.extend_from_slice(&a[j..n]);
        a.clone_from_slice(&out);
        Ok(())
    }

    pub(crate) fn std_list(&self, name: &str, xs: &Rc<Vec<Value>>, mut a: Vec<Value>) -> R {
        let lower = |x: &Value| {
            let (mut lo, mut hi) = (0, xs.len());
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if xs[mid] < *x {
                    lo = mid + 1
                } else {
                    hi = mid
                }
            }
            lo
        };
        let sizes = |what: &str| -> R<usize> {
            let n = int(&a[0])?;
            if n <= 0 { trap(format!("{what} size must be > 0")) } else { Ok(n as usize) }
        };
        Ok(match name {
            "binary_search" => {
                let i = lower(&a[0]);
                opt((i < xs.len() && xs[i] == a[0]).then_some(Value::Int(i as i64)))
            }
            "lower_bound" => Value::Int(lower(&a[0]) as i64),
            "upper_bound" => Value::Int(xs.partition_point(|y| *y <= a[0]) as i64),
            "take_while" | "drop_while" => {
                let mut k = 0;
                while k < xs.len() && boolean(self.apply(&a[0], vec![xs[k].clone()])?)? {
                    k += 1;
                }
                Value::list(if name == "take_while" { xs[..k].to_vec() } else { xs[k..].to_vec() })
            }
            "repeat" => {
                let n = int(&a[0])?;
                if n < 0 {
                    return trap("repeat count must be >= 0");
                }
                if !xs.is_empty() && n > (1i64 << 28) / xs.len() as i64 {
                    return trap("out of memory");
                }
                let mut v = Vec::with_capacity(xs.len() * n as usize);
                for _ in 0..n {
                    v.extend(xs.iter().cloned());
                }
                Value::list(v)
            }
            "rotate" => {
                let mut v = (**xs).clone();
                if !v.is_empty() {
                    let k = int(&a[0])?.rem_euclid(v.len() as i64) as usize;
                    v.rotate_left(k);
                }
                Value::list(v)
            }
            "merge" => {
                let ys = list(&a[0])?;
                let (mut i, mut j) = (0, 0);
                let mut out = Vec::with_capacity(xs.len() + ys.len());
                while i < xs.len() && j < ys.len() {
                    if ys[j] < xs[i] {
                        out.push(ys[j].clone());
                        j += 1;
                    } else {
                        out.push(xs[i].clone());
                        i += 1;
                    }
                }
                out.extend_from_slice(&xs[i..]);
                out.extend_from_slice(&ys[j..]);
                Value::list(out)
            }
            "next_perm" => {
                let mut v = (**xs).clone();
                let n = v.len();
                let mut i = n.wrapping_sub(2);
                while i < n && v[i] >= v[i + 1] {
                    i = i.wrapping_sub(1);
                }
                if n < 2 || i >= n {
                    opt(None)
                } else {
                    let mut j = n - 1;
                    while v[j] <= v[i] {
                        j -= 1;
                    }
                    v.swap(i, j);
                    v[i + 1..].reverse();
                    opt(Some(Value::list(v)))
                }
            }
            "sort_with" => {
                let mut v = (**xs).clone();
                self.msort(&mut v, &a[0])?;
                Value::list(v)
            }
            "chunks" => {
                let n = sizes("chunk")?;
                Value::list(xs.chunks(n).map(|c| Value::list(c.to_vec())).collect())
            }
            "windows" => {
                let n = sizes("window")?;
                Value::list(xs.windows(n).map(|c| Value::list(c.to_vec())).collect())
            }
            "group_by" => {
                let mut idx: BTreeMap<Value, usize> = BTreeMap::new();
                let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
                for x in xs.iter() {
                    let k = self.apply(&a[0], vec![x.clone()])?;
                    match idx.get(&k) {
                        Some(&g) => groups[g].1.push(x.clone()),
                        None => {
                            idx.insert(k.clone(), groups.len());
                            groups.push((k, vec![x.clone()]));
                        }
                    }
                }
                Value::list(groups.into_iter().map(|(k, g)| pair(k, Value::list(g))).collect())
            }
            "partition" => {
                let (mut yes, mut no) = (Vec::new(), Vec::new());
                for x in xs.iter() {
                    if boolean(self.apply(&a[0], vec![x.clone()])?)? {
                        yes.push(x.clone())
                    } else {
                        no.push(x.clone())
                    }
                }
                pair(Value::list(yes), Value::list(no))
            }
            "scan" => {
                let g = a.remove(1);
                let mut acc = a.remove(0);
                let mut out = Vec::with_capacity(xs.len());
                for x in xs.iter() {
                    acc = self.apply(&g, vec![acc, x.clone()])?;
                    out.push(acc.clone());
                }
                Value::list(out)
            }
            "flatten" => {
                let mut out = Vec::new();
                for x in xs.iter() {
                    out.extend(list(x)?.iter().cloned());
                }
                Value::list(out)
            }
            "index_of" => opt(xs.iter().position(|x| *x == a[0]).map(|i| Value::Int(i as i64))),
            "find_index" => {
                for (i, x) in xs.iter().enumerate() {
                    if boolean(self.apply(&a[0], vec![x.clone()])?)? {
                        return Ok(opt(Some(Value::Int(i as i64))));
                    }
                }
                opt(None)
            }
            "slice" => {
                let clampi = |v: i64| v.clamp(0, xs.len() as i64) as usize;
                let (from, to) = (clampi(int(&a[0])?), clampi(int(&a[1])?));
                Value::list(if to > from { xs[from..to].to_vec() } else { vec![] })
            }
            "push_front" => {
                let mut v = Vec::with_capacity(xs.len() + 1);
                v.push(a.remove(0));
                v.extend(xs.iter().cloned());
                Value::list(v)
            }
            "pop_front" => opt(xs.first().map(|x| pair(x.clone(), Value::list(xs[1..].to_vec())))),
            "pop_back" => opt(xs.last().map(|x| pair(x.clone(), Value::list(xs[..xs.len() - 1].to_vec())))),
            "to_set" => Value::Set(Rc::new(xs.iter().cloned().collect())),
            "to_flat_map" => crate::stdflat::to_flat(xs)?,
            "view" => crate::stdview::of_list(xs.clone()),
            "to_bits" => crate::stdx::bits_from(xs, int(&a[0])?)?,
            "to_hash_set" => Value::Set(Rc::new(xs.iter().cloned().collect())),
            "to_hash_map" => {
                let mut m = BTreeMap::new();
                for x in xs.iter() {
                    let Value::Tuple(p) = x else { return trap("expected a pair") };
                    m.insert(p[0].clone(), p[1].clone());
                }
                Value::Map(Rc::new(m))
            }
            "shuffle" => {
                let mut v = (**xs).clone();
                let mut st = int(&a[0])? as u64;
                for i in (1..v.len()).rev() {
                    let j = rng_below(&mut st, i + 1);
                    v.swap(i, j);
                }
                Value::list(v)
            }
            "choice" => {
                let mut st = int(&a[0])? as u64;
                opt((!xs.is_empty()).then(|| xs[rng_below(&mut st, xs.len())].clone()))
            }
            "to_heap" => {
                let mut v = (**xs).clone();
                v.sort();
                Value::Heap(Rc::new(v))
            }
            _ => return trap(format!("no method '{name}' on List")),
        })
    }

    pub(crate) fn set_method(&self, name: &str, st: &Rc<BTreeSet<Value>>, a: Vec<Value>) -> R {
        let other = |v: &Value| match v {
            Value::Set(t) => Ok(t.clone()),
            v => trap(format!("expected Set, got {v}")),
        };
        Ok(match name {
            "add" => {
                let mut n = (**st).clone();
                n.insert(a[0].clone());
                Value::Set(Rc::new(n))
            }
            "remove" => {
                let mut n = (**st).clone();
                n.remove(&a[0]);
                Value::Set(Rc::new(n))
            }
            "has" => Value::Bool(st.contains(&a[0])),
            "len" => Value::Int(st.len() as i64),
            "is_empty" => Value::Bool(st.is_empty()),
            "items" => Value::list(st.iter().cloned().collect()),
            "union" => Value::Set(Rc::new(st.union(&*other(&a[0])?).cloned().collect())),
            "inter" => Value::Set(Rc::new(st.intersection(&*other(&a[0])?).cloned().collect())),
            "diff" => Value::Set(Rc::new(st.difference(&*other(&a[0])?).cloned().collect())),
            "min" => opt(st.first().cloned()),
            "max" => opt(st.last().cloned()),
            _ => return trap(format!("no method '{name}' on Set")),
        })
    }

    pub(crate) fn heap_method(&self, name: &str, h: &Rc<Vec<Value>>, a: Vec<Value>) -> R {
        Ok(match name {
            "push" => {
                let mut v = (**h).clone();
                let i = v.partition_point(|x| *x <= a[0]);
                v.insert(i, a[0].clone());
                Value::Heap(Rc::new(v))
            }
            "pop" => opt(h.first().map(|x| pair(x.clone(), Value::Heap(Rc::new(h[1..].to_vec()))))),
            "peek" => opt(h.first().cloned()),
            "len" => Value::Int(h.len() as i64),
            "is_empty" => Value::Bool(h.is_empty()),
            "items" => Value::list((**h).clone()),
            _ => return trap(format!("no method '{name}' on Heap")),
        })
    }

    #[inline(never)]
    pub(crate) fn json_op(&self, span: Span, name: &str, args: &[Expr], env: &Rc<crate::value::Env>) -> R {
        let Some(t) = self.json_types.get(&(span.start, span.end)) else { return trap("json needs the checker's type information") };
        let v = self.eval(&args[0], env)?;
        match name {
            "encode" => {
                let mut out = String::new();
                match to_nval(&v).and_then(|nv| sspur_native::json::encode(&self.layouts, &nv, t, &mut out)) {
                    Some(()) => Ok(Value::str(&out)),
                    None => trap(format!("cannot encode {v} as JSON")),
                }
            }
            _ => Ok(Value::Res(match sspur_native::json::decode(&self.layouts, s(&v)?, t) {
                Ok(nv) => Ok(Rc::new(from_nval(nv))),
                Err(m) => Err(Rc::new(Value::str(&m))),
            })),
        }
    }
}

pub(crate) fn std_str(name: &str, x: &str, a: Vec<Value>) -> R {
    Ok(match name {
        "add" => Value::str(&format!("{x}{}", s(&a[0])?)),
        "split_once" => opt(x.split_once(s(&a[0])?).map(|(l, r)| pair(Value::str(l), Value::str(r)))),
        "index_of" => opt(x.find(s(&a[0])?).map(|b| Value::Int(x[..b].chars().count() as i64))),
        "pad_left" | "pad_right" => return pad(x, int(&a[0])?, s(&a[1])?, name == "pad_left"),
        "to_f64" => {
            let t = x.trim();
            opt(if float_syntax(t) { t.parse::<f64>().ok().map(Value::Float) } else { None })
        }
        "format" => Value::str(&crate::stdx::format_str(x, s(&a[0])?)?),
        "bytes" => ints(x.bytes().map(i64::from)),
        #[cfg(not(target_family = "wasm"))]
        "to_bytes" => Value::bytes(x.as_bytes().to_vec()),
        "codes" => ints(x.chars().map(|c| c as i64)),
        _ => return trap(format!("no method '{name}' on Str")),
    })
}

/// An integral `d` as an Int; NaN, the infinities and values outside Int's range trap like Int overflow.
pub(crate) fn f2i(d: f64) -> R {
    if d >= -9223372036854775808.0 && d < 9223372036854775808.0 { Ok(Value::Int(d as i64)) } else { trap("integer overflow") }
}

pub(crate) fn std_float(name: &str, x: f64, a: Vec<Value>) -> R {
    let y = || match a.first() {
        Some(Value::Float(y)) => Ok(*y),
        v => trap(format!("expected F64, got {v:?}", v = v.map(|v| v.to_string()))),
    };
    Ok(match name {
        "fmt" => {
            let d = int(&a[0])?;
            if !(0..=20).contains(&d) {
                return trap("fmt digits must be in 0..=20");
            }
            Value::str(&fmt_fixed(x, d as usize))
        }
        "pow" => Value::Float(x.powf(y()?)),
        "exp" => Value::Float(x.exp()),
        "ln" => Value::Float(x.ln()),
        "log2" => Value::Float(x.log2()),
        "log10" => Value::Float(x.log10()),
        "sin" => Value::Float(x.sin()),
        "cos" => Value::Float(x.cos()),
        "tan" => Value::Float(x.tan()),
        "asin" => Value::Float(x.asin()),
        "acos" => Value::Float(x.acos()),
        "atan" => Value::Float(x.atan()),
        "atan2" => Value::Float(x.atan2(y()?)),
        "hypot" => Value::Float(x.hypot(y()?)),
        "ceil" => f2i(x.ceil())?,
        "trunc" => f2i(x.trunc())?,
        "checked_round" | "checked_floor" | "checked_ceil" | "checked_trunc" => {
            let d = match name {
                "checked_round" => x.round(),
                "checked_floor" => x.floor(),
                "checked_ceil" => x.ceil(),
                _ => x.trunc(),
            };
            f2i(d).map_or(Value::Opt(None), Value::some)
        }
        "is_nan" => Value::Bool(x.is_nan()),
        "is_finite" => Value::Bool(x.is_finite()),
        "is_inf" => Value::Bool(x.is_infinite()),
        "format" => Value::str(&crate::stdx::format_f64(x, s(&a[0])?)?),
        "fma" => match a.get(1) {
            Some(Value::Float(z)) => Value::Float(unsafe { cm::fma(x, y()?, *z) }),
            _ => return trap("expected F64"),
        },
        _ => match (cm::unary(name), cm::binary(name)) {
            (Some(f), _) => Value::Float(unsafe { f(x) }),
            (_, Some(f)) => Value::Float(unsafe { f(x, y()?) }),
            _ => return trap(format!("no method '{name}' on F64")),
        },
    })
}

mod cm {
    #[cfg(target_family = "wasm")]
    pub use wasm::*;

    /// The C math library, from the `libm` crate where there is no libc.
    #[cfg(target_family = "wasm")]
    mod wasm {
        macro_rules! libm_fns {
            ($($n:ident = $f:ident($($a:ident),*);)*) => {$(
                pub unsafe extern "C" fn $n($($a: f64),*) -> f64 {
                    libm::$f($($a),*)
                }
            )*};
        }
        libm_fns! {
            sinh = sinh(x); cosh = cosh(x); tanh = tanh(x); asinh = asinh(x); acosh = acosh(x); atanh = atanh(x);
            cbrt = cbrt(x); exp2 = exp2(x); expm1 = expm1(x); log1p = log1p(x); erf = erf(x); erfc = erfc(x);
            tgamma = tgamma(x); lgamma = lgamma(x); fmod = fmod(x, y); remainder = remainder(x, y);
            copysign = copysign(x, y); nextafter = nextafter(x, y); fdim = fdim(x, y); fma = fma(x, y, z);
        }
    }

    #[cfg(not(target_family = "wasm"))]
    unsafe extern "C" {
        fn sinh(x: f64) -> f64;
        fn cosh(x: f64) -> f64;
        fn tanh(x: f64) -> f64;
        fn asinh(x: f64) -> f64;
        fn acosh(x: f64) -> f64;
        fn atanh(x: f64) -> f64;
        fn cbrt(x: f64) -> f64;
        fn exp2(x: f64) -> f64;
        fn expm1(x: f64) -> f64;
        fn log1p(x: f64) -> f64;
        fn erf(x: f64) -> f64;
        fn erfc(x: f64) -> f64;
        fn tgamma(x: f64) -> f64;
        fn lgamma(x: f64) -> f64;
        fn fmod(x: f64, y: f64) -> f64;
        fn remainder(x: f64, y: f64) -> f64;
        fn copysign(x: f64, y: f64) -> f64;
        fn nextafter(x: f64, y: f64) -> f64;
        fn fdim(x: f64, y: f64) -> f64;
        pub fn fma(x: f64, y: f64, z: f64) -> f64;
    }

    type U = unsafe extern "C" fn(f64) -> f64;
    type B = unsafe extern "C" fn(f64, f64) -> f64;

    pub fn unary(n: &str) -> Option<U> {
        Some(match n {
            "sinh" => sinh,
            "cosh" => cosh,
            "tanh" => tanh,
            "asinh" => asinh,
            "acosh" => acosh,
            "atanh" => atanh,
            "cbrt" => cbrt,
            "exp2" => exp2,
            "expm1" => expm1,
            "log1p" => log1p,
            "erf" => erf,
            "erfc" => erfc,
            "gamma" => tgamma,
            "lgamma" => lgamma,
            _ => return None,
        })
    }

    pub fn binary(n: &str) -> Option<B> {
        Some(match n {
            "fmod" => fmod,
            "remainder" => remainder,
            "copysign" => copysign,
            "nextafter" => nextafter,
            "fdim" => fdim,
            _ => return None,
        })
    }
}

fn gcd_u(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

pub(crate) fn std_int(name: &str, n: i64, a: Vec<Value>) -> R {
    let b = || int(&a[0]);
    let ovf = || trap("integer overflow");
    Ok(match name {
        "gcd" => {
            let g = gcd_u(n.unsigned_abs(), b()?.unsigned_abs());
            match i64::try_from(g) {
                Ok(g) => Value::Int(g),
                Err(_) => return ovf(),
            }
        }
        "lcm" => {
            let m = b()?;
            if n == 0 || m == 0 {
                return Ok(Value::Int(0));
            }
            let g = gcd_u(n.unsigned_abs(), m.unsigned_abs()) as u128;
            match i64::try_from(n.unsigned_abs() as u128 / g * m.unsigned_abs() as u128) {
                Ok(l) => Value::Int(l),
                Err(_) => return ovf(),
            }
        }
        "wrapping_add" => Value::Int(n.wrapping_add(b()?)),
        "wrapping_sub" => Value::Int(n.wrapping_sub(b()?)),
        "wrapping_mul" => Value::Int(n.wrapping_mul(b()?)),
        "checked_add" => opt(n.checked_add(b()?).map(Value::Int)),
        "checked_sub" => opt(n.checked_sub(b()?).map(Value::Int)),
        "checked_mul" => opt(n.checked_mul(b()?).map(Value::Int)),
        "checked_div" => opt(n.checked_div(b()?).map(Value::Int)),
        "bnot" => Value::Int(!n),
        "popcount" => Value::Int(i64::from(n.count_ones())),
        "clz" => Value::Int(i64::from(n.leading_zeros())),
        "ctz" => Value::Int(i64::from(n.trailing_zeros())),
        "rotl" => Value::Int((n as u64).rotate_left((b()? & 63) as u32) as i64),
        "rotr" => Value::Int((n as u64).rotate_right((b()? & 63) as u32) as i64),
        "byteswap" => Value::Int(n.swap_bytes()),
        "format" => Value::str(&crate::stdx::format_int(n, s(&a[0])?)?),
        "saturating_add" => Value::Int(n.saturating_add(b()?)),
        "saturating_sub" => Value::Int(n.saturating_sub(b()?)),
        "saturating_mul" => Value::Int(n.saturating_mul(b()?)),
        _ => return trap(format!("no method '{name}' on Int")),
    })
}
