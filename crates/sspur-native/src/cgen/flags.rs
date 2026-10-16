use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lto {
    #[default]
    Off,
    Thin,
    Full,
}

impl Lto {
    pub fn parse(s: &str) -> Option<Lto> {
        match s {
            "off" | "0" | "none" => Some(Lto::Off),
            "thin" | "1" => Some(Lto::Thin),
            "full" | "fat" => Some(Lto::Full),
            _ => None,
        }
    }

    pub fn flag(self) -> Option<&'static str> {
        match self {
            Lto::Off => None,
            Lto::Thin => Some("-flto=thin"),
            Lto::Full => Some("-flto=full"),
        }
    }

    pub fn linker(self) -> Option<&'static str> {
        (cfg!(not(target_os = "macos")) && self != Lto::Off && on_path("ld.lld").is_some()).then_some("-fuse-ld=lld")
    }
}

/// Extra clang flags from `SSPUR_CFLAGS` (for example `-fsanitize=address,undefined`) and those of a debug build, part of every cache key.
pub fn extra() -> Vec<String> {
    let mut f: Vec<String> = std::env::var("SSPUR_CFLAGS").map(|v| v.split_whitespace().map(str::to_string).collect()).unwrap_or_default();
    f.extend(super::debug::cflags());
    f
}

pub fn sys_libs() -> Vec<String> {
    if cfg!(windows) {
        win_builtins().into_iter().collect()
    } else if cfg!(target_os = "macos") {
        vec![]
    } else {
        vec!["-lm".into(), "-lpthread".into()]
    }
}

/// Drops unreached runtime functions and their libc imports, which cost time at every load.
pub fn strip() -> &'static [&'static str] {
    if cfg!(target_os = "macos") { &["-Wl,-dead_strip"] } else { &[] }
}

pub fn pic() -> &'static [&'static str] {
    if cfg!(windows) { &[] } else { &["-fPIC"] }
}

pub fn lto_cache_flag(dir: &Path) -> String {
    if cfg!(windows) { format!("-Wl,/lldltocache:{}", dir.display()) } else { format!("-Wl,--thinlto-cache-dir={}", dir.display()) }
}

pub fn drop_link_leftovers(out: &Path) {
    if cfg!(windows) {
        for ext in ["lib", "exp"] {
            let _ = std::fs::remove_file(out.with_extension(ext));
        }
    }
}

fn win_builtins() -> Option<String> {
    static B: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    B.get_or_init(|| {
        let ask = |args: &[&str]| Command::new(cc()).args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        if let Some(p) = ask(&["--rtlib=compiler-rt", "-print-libgcc-file-name"]).filter(|p| Path::new(p).exists()) {
            return Some(p);
        }
        let res = PathBuf::from(ask(&["-print-resource-dir"])?);
        let arch = std::env::consts::ARCH;
        [res.join("lib/windows").join(format!("clang_rt.builtins-{arch}.lib")), res.join("lib").join(format!("{arch}-pc-windows-msvc")).join("clang_rt.builtins.lib")].into_iter().find(|p| p.exists()).map(|p| p.display().to_string())
    })
    .clone()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Pgo {
    #[default]
    Off,
    Generate(PathBuf),
    Use(PathBuf),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildOpts {
    pub lto: Lto,
    pub pgo: Pgo,
}

static OPTS: Mutex<Option<BuildOpts>> = Mutex::new(None);

pub fn set(o: BuildOpts) {
    *OPTS.lock().unwrap() = Some(o);
}

pub fn current() -> BuildOpts {
    if let Some(o) = OPTS.lock().unwrap().clone() {
        return o;
    }
    let lto = std::env::var("SSPUR_LTO").ok().and_then(|v| Lto::parse(&v)).unwrap_or_default();
    let pgo = match std::env::var_os("SSPUR_PGO_GEN") {
        Some(d) => Pgo::Generate(PathBuf::from(d)),
        None => Pgo::Off,
    };
    BuildOpts { lto, pgo }
}

pub fn pgo_paths(key_text: &str, opt: &str) -> (PathBuf, PathBuf) {
    let k = blake3::hash(format!("{opt}\n{key_text}").as_bytes()).to_hex()[..32].to_string();
    let d = super::cache_dir().join("pgo");
    (d.join(format!("{k}.raw")), d.join(format!("{k}.profdata")))
}

pub fn cc() -> String {
    if let Ok(c) = std::env::var("CC") {
        return c;
    }
    if !cfg!(windows) {
        return "clang".into();
    }
    static CC: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CC.get_or_init(|| {
        if on_path("clang").is_some() {
            return "clang".into();
        }
        let llvm = std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("LLVM").join("bin"));
        on_path("clang-cl").and_then(|p| p.parent().map(Path::to_path_buf)).into_iter().chain(llvm).map(|d| d.join("clang.exe")).find(|c| c.exists()).map_or_else(|| "clang".into(), |c| c.display().to_string())
    })
    .clone()
}

pub fn on_path(name: &str) -> Option<PathBuf> {
    let exe = |p: PathBuf| -> Option<PathBuf> {
        if p.is_file() {
            return Some(p);
        }
        // Append rather than with_extension: "ld.lld" already has an extension.
        let mut e = p.into_os_string();
        e.push(std::env::consts::EXE_SUFFIX);
        let e = PathBuf::from(e);
        (cfg!(windows) && e.is_file()).then_some(e)
    };
    let p = Path::new(name);
    if p.components().count() > 1 {
        return exe(p.to_path_buf());
    }
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|d| exe(d.join(name)))
}

pub fn profdata_tool() -> Result<Vec<String>, String> {
    if let Ok(t) = std::env::var("SSPUR_PROFDATA") {
        return Ok(vec![t]);
    }
    if let Some(c) = on_path(&cc()) {
        let real = std::fs::canonicalize(&c).unwrap_or(c);
        if let Some(t) = real.parent().map(|d| d.join(format!("llvm-profdata{}", std::env::consts::EXE_SUFFIX))).filter(|t| t.exists()) {
            return Ok(vec![t.display().to_string()]);
        }
        if cfg!(target_os = "macos") && real.starts_with("/usr/bin") {
            return Ok(vec!["xcrun".into(), "llvm-profdata".into()]);
        }
    }
    on_path("llvm-profdata").map(|p| vec![p.display().to_string()]).ok_or_else(|| "llvm-profdata not found (set SSPUR_PROFDATA)".into())
}

pub fn merge(raw: &Path, out: &Path) -> Result<usize, String> {
    let files: Vec<PathBuf> = std::fs::read_dir(raw).map_err(|e| format!("no training profile in {}: {e}", raw.display()))?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "profraw")).collect();
    if files.is_empty() {
        return Err(format!("the training run wrote no profile into {}", raw.display()));
    }
    let tool = profdata_tool()?;
    let o = Command::new(&tool[0]).args(&tool[1..]).arg("merge").arg("-o").arg(out).args(&files).output().map_err(|e| format!("cannot run {}: {e}", tool.join(" ")))?;
    if !o.status.success() {
        return Err(format!("llvm-profdata merge failed: {}", String::from_utf8_lossy(&o.stderr).lines().take(4).collect::<Vec<_>>().join(" | ")));
    }
    Ok(files.len())
}

pub(super) fn build_pgo(src: &str, opt: &str, links: &[String], o: &BuildOpts) -> Result<PathBuf, String> {
    let links = super::resolve_links(links)?;
    let cc = cc();
    let (flag, tag) = match &o.pgo {
        Pgo::Generate(d) => (format!("-fprofile-generate={}", d.display()), d.display().to_string()),
        Pgo::Use(p) => {
            let data = std::fs::read(p).map_err(|e| format!("cannot read profile {}: {e}", p.display()))?;
            (format!("-fprofile-use={}", p.display()), blake3::hash(&data).to_hex().to_string())
        }
        Pgo::Off => return Err("no PGO mode".into()),
    };
    let lto = o.lto.flag();
    let key = blake3::hash(format!("{cc} {opt} {flag} {tag} {lto:?}{}\n{src}", links.iter().map(|l| format!(" {l}")).collect::<String>()).as_bytes()).to_hex()[..32].to_string();
    let base = super::cache_dir();
    let lib = base.join(format!("{key}.{}", std::env::consts::DLL_EXTENSION));
    if lib.exists() {
        return Ok(lib);
    }
    let dir = base.join("pgo").join(&key);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("prog.c"), src).map_err(|e| e.to_string())?;
    let tmp = lib.with_extension(format!("{}.tmp", super::tmp_suffix()));
    let mut cmd = Command::new(&cc);
    cmd.current_dir(&dir).args([opt, "-shared", "-w", &flag]).args(pic());
    if let Some(l) = lto {
        cmd.arg(l);
    }
    cmd.args(o.lto.linker());
    let out = cmd.arg("-o").arg(&tmp).arg("prog.c").args(&links).args(sys_libs()).output().map_err(|e| format!("cannot run {cc}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{cc} failed: {}", String::from_utf8_lossy(&out.stderr).lines().take(6).collect::<Vec<_>>().join(" | ")));
    }
    drop_link_leftovers(&tmp);
    publish(&tmp, &lib)?;
    Ok(lib)
}

/// Windows caps a command line at 32767 characters, so a long list of inputs goes to clang in a response file.
pub fn inputs(cmd: &mut Command, files: &[PathBuf], rsp: &Path) -> Result<(), String> {
    if cfg!(windows) && files.iter().map(|f| f.as_os_str().len() + 3).sum::<usize>() > 16000 {
        let text: String = files.iter().map(|f| format!("\"{}\"\n", f.display().to_string().replace('\\', "/"))).collect();
        std::fs::write(rsp, text).map_err(|e| e.to_string())?;
        cmd.arg(format!("@{}", rsp.display()));
    } else {
        cmd.args(files);
    }
    Ok(())
}

/// Moves a finished build into the cache. A concurrent build of the same key may have won, and Windows cannot replace a loaded library or running program, so an existing destination counts as done.
pub fn publish(tmp: &Path, dst: &Path) -> Result<(), String> {
    // Windows can't open a file while a rename replaces it, so a concurrent build reading `dst`
    // could fail. A hard link never replaces, and loses cleanly to whoever published first.
    if cfg!(windows) {
        match std::fs::hard_link(tmp, dst) {
            Ok(()) => {
                let _ = std::fs::remove_file(tmp);
                return Ok(());
            }
            Err(_) if dst.exists() => {
                let _ = std::fs::remove_file(tmp);
                return Ok(());
            }
            Err(_) => {}
        }
    }
    match std::fs::rename(tmp, dst) {
        Ok(()) => Ok(()),
        Err(_) if dst.exists() => {
            let _ = std::fs::remove_file(tmp);
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// How many clang processes a split build runs at once: one per core, fewer when the machine's
/// memory couldn't hold that many compiles of units of up to `unit_bytes` bytes of C.
/// `SSPUR_JOBS` sets the number.
pub fn jobs(unit_bytes: usize) -> usize {
    if let Some(j) = std::env::var("SSPUR_JOBS").ok().and_then(|j| j.trim().parse::<usize>().ok()) {
        return j.clamp(1, 256);
    }
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    // clang's peak at -O2: about 60 MB to start, and some 300 bytes per byte of C in a unit.
    let per = (64 << 20) + 320 * unit_bytes as u64;
    let by_mem = memory().map_or(cores as u64, |m| (m / 2 / per).max(1));
    cores.min(by_mem as usize).max(1)
}

/// Physical memory free for new processes, in bytes, if the system says.
fn memory() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let info = std::fs::read_to_string("/proc/meminfo").ok()?;
        let line = info.lines().find(|l| l.starts_with("MemAvailable:"))?;
        line.split_whitespace().nth(1)?.parse::<u64>().ok().map(|kb| kb * 1024)
    }
    #[cfg(target_os = "macos")]
    {
        // The total: macOS keeps most memory as file cache, so what is free says little.
        unsafe extern "C" {
            fn sysctlbyname(name: *const std::ffi::c_char, old: *mut std::ffi::c_void, oldlen: *mut usize, new: *mut std::ffi::c_void, newlen: usize) -> i32;
        }
        let mut v: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        let ok = unsafe { sysctlbyname(c"hw.memsize".as_ptr(), (&raw mut v).cast(), &mut len, std::ptr::null_mut(), 0) } == 0;
        (ok && v > 0).then_some(v / 2)
    }
    #[cfg(windows)]
    {
        #[repr(C)]
        struct MemoryStatusEx {
            length: u32,
            load: u32,
            total_phys: u64,
            avail_phys: u64,
            total_page: u64,
            avail_page: u64,
            total_virtual: u64,
            avail_virtual: u64,
            avail_ext: u64,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GlobalMemoryStatusEx(buf: *mut MemoryStatusEx) -> i32;
        }
        let mut m = MemoryStatusEx { length: std::mem::size_of::<MemoryStatusEx>() as u32, load: 0, total_phys: 0, avail_phys: 0, total_page: 0, avail_page: 0, total_virtual: 0, avail_virtual: 0, avail_ext: 0 };
        let ok = unsafe { GlobalMemoryStatusEx(&mut m) } != 0;
        (ok && m.avail_phys > 0).then_some(m.avail_phys)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        None
    }
}
