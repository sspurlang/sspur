//! Debug builds (ADR 0036) under a real debugger: lldb on macOS, gdb on Linux. Each check is
//! skipped when its debugger isn't installed or can't start a process here.

use std::path::{Path, PathBuf};
use std::process::Command;

const PROG: &str = "type Shape = Circle{r: F64} | Rect{w: F64, h: F64}

fn total(xs: List[Int]) -> Int = do
  var acc = 0
  for x in xs do
    acc := acc + x
  acc

fn area(s: Shape) -> F64 =
  match s
    | Circle{r} => 3.14 * r * r
    | Rect{w, h} => w * h

fn main() -> Unit ! log = do
  let name = \"debug me\"
  let t = total([4, 5, 6])
  log(name + \" \" + t.str + \" \" + area(Rect{w: 2.0, h: 3.0}).str)
";

const LOOP_LINE: usize = 6;

fn setup(name: &str) -> (PathBuf, PathBuf) {
    let d = std::env::temp_dir().join(format!("sspur-debug-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("prog")).unwrap();
    std::fs::write(d.join("prog/prog.ssp"), PROG).unwrap();
    (d.join("prog"), d.join("cache"))
}

fn sspur(dir: &Path, cache: &Path, args: &[&str]) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_sspur")).args(args).current_dir(dir).env("SSPUR_CACHE", cache).output().unwrap();
    (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

fn on_path(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok_and(|o| o.status.success())
}

fn script(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/debugger").join(name)
}

fn cannot_launch(out: &str) -> bool {
    ["process launch failed", "attach failed", "Operation not permitted", "not allowed to attach", "ptrace: ", "Could not create inferior", "During startup program exited", "exited with status -1"].iter().any(|m| out.contains(m))
}

#[test]
fn debug_build_stops_at_an_ssp_line_with_sspur_names_and_values() {
    if !on_path("clang") {
        return;
    }
    let (dir, cache) = setup("session");
    let (ok, out) = sspur(&dir, &cache, &["build", "--debug", "prog.ssp"]);
    assert!(ok && out.contains("with debug info") && out.contains("prog.ssp"), "{out}");
    let (ok, out) = sspur(&dir, &cache, &["run", "--debug", "prog.ssp"]);
    assert!(ok && out.contains("debug me 15 6"), "{out}");
    let exe = env!("CARGO_BIN_EXE_sspur");
    let line = LOOP_LINE.to_string();
    let o = if cfg!(target_os = "macos") {
        if !on_path("lldb") {
            eprintln!("skipped: no lldb");
            return;
        }
        let import = format!("command script import \"{}\"", script("sspur_lldb.py").display());
        let bp = format!("breakpoint set -f prog.ssp -l {line}");
        Command::new("lldb").args(["-b", "-o", &import, "-o", &bp, "-o", "run", "-o", "bt", "-o", "frame variable", "-o", "up", "-o", "frame variable", "-o", "kill", "--", exe, "run", "--debug", "prog.ssp"]).current_dir(&dir).env("SSPUR_CACHE", &cache).output().unwrap()
    } else if cfg!(target_os = "linux") {
        if !on_path("gdb") {
            eprintln!("skipped: no gdb");
            return;
        }
        let source = format!("source {}", script("sspur_gdb.py").display());
        let bp = format!("break prog.ssp:{line}");
        Command::new("gdb").args(["-batch", "-nx", "-ex", &source, "-ex", "set breakpoint pending on", "-ex", &bp, "-ex", "run", "-ex", "bt", "-ex", "info args", "-ex", "info locals", "-ex", "up", "-ex", "info locals", "-ex", "kill", "--args", exe, "run", "--debug", "prog.ssp"]).current_dir(&dir).env("SSPUR_CACHE", &cache).output().unwrap()
    } else {
        return;
    };
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    if cannot_launch(&out) {
        eprintln!("skipped: the debugger cannot start processes here\n{out}");
        return;
    }
    let at = format!("prog.ssp:{line}");
    assert!(out.contains(&at), "no stop at {at}:\n{out}");
    assert!(out.contains("total (") || out.contains("`total("), "no SSPUR function name in the backtrace:\n{out}");
    assert!(out.contains("main") && out.contains("prog.ssp:16"), "no caller frame at its line:\n{out}");
    assert!(out.contains("acc = 0") && out.contains("x = 4"), "no SSPUR locals with their values:\n{out}");
    assert!(out.contains("\"debug me\""), "no readable Str in the caller:\n{out}");
    assert!(!out.contains("v_acc_$"), "C names leaked:\n{out}");
}

#[test]
fn debug_init_writes_the_formatters_and_codebases_get_a_source_file() {
    let (dir, cache) = setup("init");
    let (ok, out) = sspur(&dir, &cache, &["debug-init"]);
    assert!(ok && out.contains("sspur_lldb.py") && out.contains("sspur_gdb.py"), "{out}");
    for f in ["sspur_lldb.py", "sspur_gdb.py"] {
        let p = cache.join("debug").join(f);
        assert!(p.is_file(), "missing {}", p.display());
        if on_path("python3") {
            let o = Command::new("python3").args(["-c", "import ast, sys; ast.parse(open(sys.argv[1]).read())"]).arg(&p).output().unwrap();
            assert!(o.status.success(), "{f}: {}", String::from_utf8_lossy(&o.stderr));
        }
    }
    if !on_path("clang") {
        return;
    }
    let (ok, out) = sspur(&dir, &cache, &["init", "prog.ssp"]);
    assert!(ok, "{out}");
    std::fs::remove_file(dir.join("prog.ssp")).unwrap();
    let (ok, out) = sspur(&dir, &cache, &["build", "--debug"]);
    assert!(ok, "{out}");
    let src = out.lines().find_map(|l| l.strip_prefix("source lines refer to ")).unwrap_or_else(|| panic!("{out}"));
    assert!(src.ends_with("prog.ssp") && Path::new(src).starts_with(std::fs::canonicalize(&cache).unwrap()), "{src}");
    let (_, projected) = sspur(&dir, &cache, &["src"]);
    assert_eq!(std::fs::read_to_string(src).unwrap(), projected);
}
