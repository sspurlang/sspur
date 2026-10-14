//! Source-level debugging (ADR 0036): `--debug` builds, `sspur debug` and `sspur debug-init`.

use sspur_native::cgen::debug as nd;
use sspur_store::Loaded;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

pub const LLDB_PY: &str = include_str!("debugger/sspur_lldb.py");
pub const GDB_PY: &str = include_str!("debugger/sspur_gdb.py");

fn debug_dir() -> PathBuf {
    sspur_syntax::cache_root().unwrap_or_else(std::env::temp_dir).join("debug")
}

/// Where the debugger finds the source: the file itself, or for a codebase its projected source
/// written to the cache, so a breakpoint by line and the source view work for both.
pub fn source_path(label: &str, src: &str) -> Result<PathBuf, String> {
    if label != "HEAD" {
        return std::fs::canonicalize(label).map_err(|e| format!("cannot resolve {label}: {e}"));
    }
    let root = std::env::current_dir().ok().and_then(|d| sspur_store::Store::find(&d)).map(|s| s.root_dir()).unwrap_or_default();
    let stem = root.file_name().map_or("codebase".into(), |n| n.to_string_lossy().into_owned());
    let stem: String = stem.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let dir = debug_dir().join("src").join(&blake3::hash(format!("{}\n{src}", root.display()).as_bytes()).to_hex()[..16]);
    let file = dir.join(format!("{stem}.ssp"));
    if std::fs::read_to_string(&file).ok().as_deref() != Some(src) {
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        std::fs::write(&file, src).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    }
    Ok(std::fs::canonicalize(&file).unwrap_or(file))
}

/// Turns on debug code generation for this program; returns the source path its lines refer to.
pub fn enable(l: &Loaded, label: &str) -> Result<PathBuf, String> {
    let path = source_path(label, &l.src)?;
    let skip = l.module.defs[l.own.min(l.module.defs.len())..].iter().map(|d| d.name().to_string()).collect();
    nd::set(true, Some(nd::Source { path: path.display().to_string().replace('\\', "/"), text: l.src.clone(), skip }));
    Ok(path)
}

/// Writes the formatter scripts into the cache and returns their paths (lldb, gdb).
pub fn scripts() -> Result<(PathBuf, PathBuf), String> {
    let dir = debug_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let lldb = dir.join("sspur_lldb.py");
    let gdb = dir.join("sspur_gdb.py");
    for (p, text) in [(&lldb, LLDB_PY), (&gdb, GDB_PY)] {
        if std::fs::read_to_string(p).ok().as_deref() != Some(text) {
            std::fs::write(p, text).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
        }
    }
    Ok((lldb, gdb))
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// `sspur debug-init [--install]`: writes the formatters and prints (or with `--install` adds) the
/// lines that load them in every lldb and gdb session.
pub fn init(install: bool) -> ExitCode {
    let (lldb, gdb) = match scripts() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let lines = [(".lldbinit", format!("command script import \"{}\"", lldb.display())), (".gdbinit", format!("source {}", gdb.display()))];
    println!("wrote {} and {}", lldb.display(), gdb.display());
    for (rc, line) in &lines {
        let Some(h) = home() else { continue };
        let path = h.join(rc);
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        if have.lines().any(|l| l.trim() == line) {
            println!("~/{rc} already loads it");
        } else if install {
            let sep = if have.is_empty() || have.ends_with('\n') { "" } else { "\n" };
            match std::fs::write(&path, format!("{have}{sep}{line}\n")) {
                Ok(()) => println!("added to ~/{rc}: {line}"),
                Err(e) => {
                    eprintln!("cannot write {}: {e}", path.display());
                    return ExitCode::FAILURE;
                }
            }
        } else {
            println!("add to ~/{rc}:  {line}");
        }
    }
    if !install {
        println!("(or run 'sspur debug-init --install' to add them)");
    }
    ExitCode::SUCCESS
}

/// The line of `fn main`, where `sspur debug` stops first.
pub fn main_line(src: &str) -> Option<usize> {
    let m = sspur_syntax::parse(src).ok()?;
    m.defs.iter().find_map(|d| match d {
        sspur_syntax::Def::Fn(f) if f.name == "main" => Some(sspur_syntax::line_col(src, f.span.start).0),
        _ => None,
    })
}

fn debugger(name: Option<&str>) -> Option<(String, PathBuf)> {
    let order: Vec<&str> = match name {
        Some(n) => vec![n],
        None if cfg!(target_os = "macos") => vec!["lldb", "gdb"],
        None => vec!["gdb", "lldb"],
    };
    order.into_iter().find_map(|d| sspur_native::cgen::flags::on_path(d).map(|p| (d.to_string(), p)))
}

fn install_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "install the Xcode command line tools (xcode-select --install), which include lldb"
    } else if cfg!(windows) {
        "debug builds there carry CodeView; open them in Visual Studio or WinDbg (see the Debugging page), or install LLVM's lldb"
    } else {
        "install gdb (sudo apt install gdb, sudo dnf install gdb) or lldb"
    }
}

/// The command that runs `prog` under `dbg` with the formatters loaded, stopped first at `line` of `src_path`.
pub fn command(dbg: &str, dbg_path: &Path, script: &Path, src_path: &Path, line: Option<usize>, prog: &[String]) -> Command {
    let mut c = Command::new(dbg_path);
    let file = src_path.display().to_string();
    if dbg == "gdb" {
        c.arg("-q").args(["-ex", &format!("source {}", script.display()), "-ex", "set breakpoint pending on"]);
        if let Some(n) = line {
            c.args(["-ex", &format!("break {file}:{n}")]);
        }
        c.args(["-ex", "break sspur_trap", "-ex", "run", "--args"]).args(prog);
    } else {
        c.args(["-o", &format!("command script import \"{}\"", script.display())]);
        if let Some(n) = line {
            c.args(["-o", &format!("breakpoint set -f \"{file}\" -l {n}")]);
        }
        c.args(["-o", "breakpoint set -n sspur_trap", "-o", "run", "--"]).args(prog);
    }
    c
}

/// `sspur debug [file.ssp] [args]`: builds the program with debug info and starts lldb or gdb on it.
pub fn run(file: Option<&String>, prog_args: &[String], which: Option<&str>) -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("cannot find the sspur binary: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (label, src) = match file {
        Some(f) => match std::fs::read_to_string(f) {
            Ok(s) => (f.clone(), s),
            Err(e) => {
                eprintln!("cannot read {f}: {e}");
                return ExitCode::from(2);
            }
        },
        None => match std::env::current_dir().ok().and_then(|d| sspur_store::Store::find(&d)) {
            Some(s) => ("HEAD".to_string(), s.head_root().map(|r| s.root_src(&r)).unwrap_or_default()),
            None => {
                eprintln!("usage: sspur debug file.ssp [args]  (or run it inside a codebase)");
                return ExitCode::from(2);
            }
        },
    };
    let Some((dbg, dbg_path)) = debugger(which) else {
        eprintln!("no debugger found: {}", install_hint());
        eprintln!("the program can still be built for one with: sspur build --debug {}", file.map_or("", String::as_str));
        return ExitCode::FAILURE;
    };
    let mut build = Command::new(&exe);
    build.arg("build").arg("--debug").args(file);
    match build.stdout(std::process::Stdio::null()).status() {
        Ok(s) if s.success() => {}
        Ok(_) => return ExitCode::FAILURE,
        Err(e) => {
            eprintln!("cannot run {}: {e}", exe.display());
            return ExitCode::FAILURE;
        }
    }
    let path = match source_path(&label, &src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let (lldb, gdb) = match scripts() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let mut prog = vec![exe.display().to_string(), "run".into(), "--debug".into()];
    prog.extend(file.cloned());
    prog.extend(prog_args.iter().cloned());
    let mut cmd = command(&dbg, &dbg_path, if dbg == "gdb" { &gdb } else { &lldb }, &path, main_line(&src), &prog);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = cmd.exec();
        eprintln!("cannot start {dbg}: {e}");
        ExitCode::FAILURE
    }
    #[cfg(not(unix))]
    match cmd.status() {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(e) => {
            eprintln!("cannot start {dbg}: {e}");
            ExitCode::FAILURE
        }
    }
}
