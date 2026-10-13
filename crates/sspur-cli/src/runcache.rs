//! The cached path of `sspur run` (ADR 0034). A run that went through the normal path records the
//! native library it ran, keyed by everything the library depends on; the next run of the same
//! program with the same compiler and environment runs that library's `main` directly, without
//! parsing, checking or generating code. Anything that doesn't match falls back to the normal path.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Environment variables besides `SSPUR_*` that can change what the compiler builds.
const ENV: &[&str] = &["CC", "PATH", "HOME", "LOCALAPPDATA", "ProgramFiles", "SDKROOT", "MACOSX_DEPLOYMENT_TARGET", "CPATH", "C_INCLUDE_PATH", "LIBRARY_PATH"];

/// What the normal path learned about a program that the cached path needs.
pub struct Entry {
    pub lib: PathBuf,
    pub scalar: bool,
    pub profile: Option<PathBuf>,
}

thread_local! {
    static FAILED: std::cell::RefCell<Option<sspur_native::MainFailure>> = const { std::cell::RefCell::new(None) };
}

/// The cached path ran `main` and it failed: the normal path describes the failure instead of
/// running `main` again.
pub fn take_failure() -> Option<sspur_native::MainFailure> {
    FAILED.with(|f| f.borrow_mut().take())
}

fn exe_id() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let m = std::fs::metadata(&exe).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(format!("{}\0{}\0{}\0{}", env!("CARGO_PKG_VERSION"), exe.display(), m.len(), t.as_nanos()))
}

fn key(src: &[u8]) -> Option<String> {
    let mut h = blake3::Hasher::new();
    h.update(b"sspur run 1\0");
    h.update(exe_id()?.as_bytes());
    let mut vars: Vec<(String, String)> = std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .filter(|(k, _)| k.starts_with("SSPUR_") || ENV.contains(&k.as_str()))
        .collect();
    vars.sort();
    for (k, v) in vars {
        h.update(format!("\0{k}={v}").as_bytes());
    }
    h.update(b"\0\0");
    h.update(src);
    Some(h.finalize().to_hex()[..32].to_string())
}

fn entry_path(key: &str) -> Option<PathBuf> {
    Some(sspur_syntax::cache_root()?.join("run").join(key))
}

fn file_hash(p: &Path) -> Option<String> {
    Some(blake3::hash(&std::fs::read(p).ok()?).to_hex().to_string())
}

fn in_package(path: &str) -> bool {
    let dir = Path::new(path).parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    sspur_store::pkg::find_root(dir).is_some()
}

/// A plain `sspur run file.ssp [args]` outside a package: the only form the cached path serves.
pub fn applies(cmd: &str, flags: &[String], path: Option<&String>) -> bool {
    cmd == "run" && flags.is_empty() && path.is_some()
}

fn emit(_: *const (), s: &str) {
    println!("{s}");
}

/// Runs the program at `path` from the cache, or returns `None` (having done nothing) when the
/// cache has no valid entry for it.
pub fn try_run(path: &str) -> Option<ExitCode> {
    let src = std::fs::read(path).ok()?;
    let text = std::fs::read_to_string(entry_path(&key(&src)?)?).ok()?;
    let mut lines = text.lines();
    let lib = PathBuf::from(lines.next()?.strip_prefix("lib ")?);
    if file_hash(&lib)? != lines.next()?.strip_prefix("lib-hash ")? {
        return None;
    }
    let scalar = match lines.next()?.strip_prefix("entry ")? {
        "scalar" => true,
        "rich" => false,
        _ => return None,
    };
    if let Some(l) = lines.next() {
        let (hash, p) = l.strip_prefix("profile ")?.split_once(' ')?;
        let same = if hash == "-" { !Path::new(p).exists() } else { file_hash(Path::new(p)).is_some_and(|h| h == hash) };
        if !same {
            return None;
        }
    }
    if lines.next().is_some() || in_package(path) {
        return None;
    }
    sspur_native::set_log_hook(Some((emit, std::ptr::null())));
    let r = sspur_native::run_cached_main(&lib, scalar, 1_000_000);
    sspur_native::set_log_hook(None);
    match r {
        Err(_) => None,
        Ok(Ok(())) => Some(ExitCode::SUCCESS),
        Ok(Err(f)) => {
            FAILED.with(|x| *x.borrow_mut() = Some(f));
            None
        }
    }
}

/// Records `e` for the program `src` that the normal path is about to run.
pub fn store(path: &str, src: &str, e: &Entry) {
    if in_package(path) {
        return;
    }
    let (Some(key), Some(lib_hash)) = (key(src.as_bytes()), file_hash(&e.lib)) else { return };
    let Some(dest) = entry_path(&key) else { return };
    let mut text = format!("lib {}\nlib-hash {lib_hash}\nentry {}\n", e.lib.display(), if e.scalar { "scalar" } else { "rich" });
    if let Some(p) = &e.profile {
        let hash = file_hash(p).unwrap_or_else(|| "-".into());
        text.push_str(&format!("profile {hash} {}\n", p.display()));
    }
    if text.lines().count() != 3 + usize::from(e.profile.is_some()) || std::fs::read_to_string(&dest).is_ok_and(|t| t == text) {
        return;
    }
    let Some(dir) = dest.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = dir.join(format!("{key}.{}.tmp", std::process::id()));
    let written = std::fs::File::create(&tmp).and_then(|mut f| f.write_all(text.as_bytes()));
    if written.is_err() || std::fs::rename(&tmp, &dest).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}
