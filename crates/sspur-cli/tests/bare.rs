use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sspur(args: &[&str]) -> (String, String, bool) {
    let o = Command::new(env!("CARGO_BIN_EXE_sspur")).args(args).current_dir(root()).output().unwrap();
    (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned(), o.status.success())
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sspur-bare-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn has(bin: &str) -> bool {
    Command::new(bin).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn build(target: &str, src: &Path, out: &Path) -> Option<()> {
    let qemu = sspur_native::bare::qemu_args(target, "")[0].clone();
    let qemu = qemu.as_str();
    if !has(qemu) {
        eprintln!("skip: {qemu} not found");
        return None;
    }
    let (_, err, ok) = sspur(&["build", "--target", target, src.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    if !ok && err.contains("toolchain not found") {
        eprintln!("skip: {err}");
        return None;
    }
    assert!(ok, "build failed for {target}: {err}");
    Some(())
}

fn boot(target: &str, elf: &Path) -> (String, Option<i32>) {
    let args = sspur_native::bare::qemu_args(target, elf.to_str().unwrap());
    let mut child = Command::new(&args[0]).args(&args[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut out = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if start.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    (reader.join().unwrap().replace('\r', ""), status)
}

fn boots(target: &str, example: &str, expect: &str) {
    let dir = if example == "hello" { "examples" } else { "tests" };
    boots_with(target, &format!("{dir}/bare/{example}.ssp"), expect, 0);
}

fn boots_with(target: &str, path: &str, expect: &str, status: i32) {
    let example = Path::new(path).file_stem().unwrap().to_string_lossy().into_owned();
    let d = scratch(&format!("{target}-{example}"));
    let elf = d.join("kernel.elf");
    if build(target, &root().join(path), &elf).is_none() {
        return;
    }
    let (out, code) = boot(target, &elf);
    assert_eq!(code, Some(status), "{target} {example}: exit {code:?}, output {out:?}");
    assert_eq!(out, expect, "{target} {example}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn hello_boots_on_riscv64() {
    boots("riscv64-qemu", "hello", "hello from sspur\n");
}

#[test]
fn hello_boots_on_aarch64() {
    boots("aarch64-qemu", "hello", "hello from sspur\n");
}

#[test]
fn hello_boots_on_cortex_m4() {
    boots("thumbv7em-mps2", "hello", "hello from sspur\n");
    boots("thumbv7em-mps2", "m4_hello", "hello from sspur on cortex-m4\nleds 2 primask 0 64-bit 710430\nirq 5 handled\nafter irq\n");
}

#[test]
fn timer_interrupt_on_cortex_m4() {
    boots("thumbv7em-mps2", "timer", "arming timer\ntimer interrupt\n");
}

#[test]
fn timer_interrupt_on_riscv64() {
    boots("riscv64-qemu", "timer", "arming timer\ntimer interrupt\n");
}

#[test]
fn timer_interrupt_on_aarch64() {
    boots("aarch64-qemu", "timer", "arming timer\ntimer interrupt\n");
}

#[test]
fn records_options_tuples_generics_and_drops_on_both_targets() {
    for target in ["riscv64-qemu", "aarch64-qemu", "thumbv7em-mps2"] {
        boots_with(target, "tests/bare/values.ssp", "107581\n7", 42);
    }
}

const TRAP: &str = "profile bare

fn putc(c: Int) ! mmio
= if arch() == \"riscv64\" then mmio[U8](0x10000000).write(c) else if arch() == \"thumbv7em\" then do
    mmio[U32](0x40004008).write(1)
    mmio[U32](0x40004000).write(c)
  else mmio[U32](0x09000000).write(c)

fn on_trap(code: Int) ! mmio
= do
  putc(84)
  putc(48 + code)
  putc(10)

fn grow(x: Int, n: Int) -> Int
= if n == 0 then x else grow(x * 3, n - 1)

fn main() -> Int
= grow(1, 70)
";

#[test]
fn traps_reach_the_halt_handler() {
    for (target, exit) in [("riscv64-qemu", 65), ("aarch64-qemu", 65), ("thumbv7em-mps2", 65)] {
        let d = scratch(&format!("trap-{target}"));
        let src = d.join("trap.ssp");
        std::fs::write(&src, TRAP).unwrap();
        let elf = d.join("kernel.elf");
        if build(target, &src, &elf).is_none() {
            continue;
        }
        let (out, code) = boot(target, &elf);
        assert_eq!((out.as_str(), code), ("T1\n", Some(exit)), "{target}");
        std::fs::write(&src, TRAP.replace("grow(1, 70)", "grow(1, 3)")).unwrap();
        build(target, &src, &elf).unwrap();
        assert_eq!(boot(target, &elf), (String::new(), Some(27)), "{target} exit status from main");
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[test]
fn bare_rejects_runtime_features() {
    let cases = [
        ("fn f(xs: List[Int]) -> Int\n= xs.len", "E_PROFILE_BARE", "lists need the GC heap"),
        ("fn f(n: Int) -> Str\n= \"n={n}\"", "E_PROFILE_BARE", "interpolation"),
        ("fn f() ! log\n= log(\"x\")", "E_PROFILE_BARE", "no host log"),
        ("fn f(s: Str) -> Str\n= s.upper", "E_PROFILE_BARE", "'.upper' on Str"),
        ("fn f(x: F64) -> F64\n= x.sin", "E_PROFILE_BARE", "'.sin' on F64 needs libm"),
        ("fn f(x: F64) -> F64\n= x % 2.0", "E_PROFILE_BARE", "'%' on F64 needs libm"),
        ("fn f() -> Int\n= (x => x)(1)", "E_PROFILE_BARE", "lambdas"),
        ("type S = A | B\n\nfn f(s: S) -> Int\n= 1", "E_PROFILE_BARE", "heap-allocated"),
        ("fn f() -> Int ! unsafe\n= alloc(4, 0).load(0)", "E_PROFILE_BARE", "no heap"),
        ("fn f() ! fail[Int]\n= raise 1", "E_PROFILE_BARE", "error runtime"),
        ("fn f() -> Int\n= mmio[I32](0).read", "E_MMIO_WIDTH", "U8, U16, U32 or U64"),
        ("fn f() -> Int\n= mmio[U32](0).read", "E_EFFECT_MISSING", "mmio"),
        ("fn f()\n  interrupt soon\n= ()", "E_INTERRUPT_VEC", "unknown interrupt"),
        ("fn f(n: Int)\n  interrupt 7\n= ()", "E_INTERRUPT_SIG", "no parameters"),
        ("fn f()\n  interrupt timer\n= ()\n\nfn g()\n  interrupt timer\n= ()", "E_INTERRUPT_DUP", "already handled"),
        ("fn on_trap()\n= ()", "E_INTERRUPT_SIG", "on_trap(code: Int)"),
        ("test t = mmio[U8](0).read == 0", "E_PROFILE_BARE", "tests run on the host"),
    ];
    let d = scratch("reject");
    for (i, (body, code, msg)) in cases.iter().enumerate() {
        let f = d.join(format!("r{i}.ssp"));
        std::fs::write(&f, format!("profile bare\n\n{body}\n")).unwrap();
        let (out, err, ok) = sspur(&["check", f.to_str().unwrap()]);
        let all = format!("{out}{err}");
        assert!(!ok && all.lines().any(|l| l.contains(code) && l.contains(msg)), "case {i} ({body:?}) expected {code} '{msg}', got:\n{all}");
    }
    for (body, msg) in [("fn f() -> Int\n= mmio[U8](0).read", "mmio needs 'profile bare'"), ("fn f()\n  interrupt 7\n= ()", "interrupt handlers need 'profile bare'")] {
        let f = d.join("app.ssp");
        std::fs::write(&f, format!("profile sys\n\n{body}\n")).unwrap();
        let (out, err, ok) = sspur(&["check", f.to_str().unwrap()]);
        assert!(!ok && format!("{out}{err}").contains(msg), "{body}: {out}{err}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn bare_modules_test_on_the_host() {
    let (out, err, ok) = sspur(&["test", "examples/bare/hello.ssp"]);
    assert!(ok, "{out}{err}");
    assert!(out.contains("1 passed, 0 failed"), "{out}");
    let (_, err, ok) = sspur(&["run", "examples/bare/hello.ssp"]);
    assert!(!ok && err.contains("needs a bare target"), "{err}");
    let (out, _, ok) = sspur(&["fmt", "tests/bare/timer.ssp"]);
    assert!(ok && out.contains("mmio[U8](268435461)") && out.contains("  interrupt timer\n"), "{out}");
}

const BITS: &str = "fn bits(a: Int, b: Int) -> Int
= a.band(b) + a.bor(b) * 3 + a.bxor(b) * 7 + a.shl(b % 70) - a.shr(b % 70)

fn byte_at(s: Str, i: Int) -> Int
= s.byte(i) + s.byte_len

fn main() ! log
= do
  log(\"{bits(12345, 6)} {bits(-1, 62)} {bits(5, 64)} {bits(-77, -3)} {byte_at(\"h\u{e9}llo\", 2)}\")
  log(\"{byte_at(\"abc\", 3)}\")
";

#[test]
fn bit_and_byte_builtins_match_across_tiers() {
    let d = scratch("bits");
    let f = d.join("bits.ssp");
    std::fs::write(&f, BITS).unwrap();
    let native = sspur(&["run", f.to_str().unwrap()]);
    let interp = sspur(&["run", "--interp", f.to_str().unwrap()]);
    assert_eq!(native, interp);
    assert!(native.0.starts_with("913398 "), "{native:?}");
    assert!(native.1.contains("byte index 3 out of bounds for a string of 3 bytes"), "{native:?}");
    let (out, _, _) = sspur(&["native", "--release", f.to_str().unwrap()]);
    assert!(out.contains("native  bits") && out.contains("native  byte_at"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn statics_shared_with_interrupt_handlers_and_stack_arrays_on_every_target() {
    // a handler that re-arms an already due timer must not nest (a loaded host makes every 1 ms tick due on return)
    let d = scratch("overdue");
    let overdue = d.join("ticks_overdue.ssp");
    std::fs::write(&overdue, std::fs::read_to_string(root().join("tests/bare/ticks.ssp")).unwrap().replace("timer_start(tick_hz() / 1000)", "timer_start(1)")).unwrap();
    for target in ["riscv64-qemu", "aarch64-qemu", "thumbv7em-mps2"] {
        boots(target, "ticks", "hist 804\nticks 5 seen 14 base 1005\n");
        boots_with(target, overdue.to_str().unwrap(), "hist 804\nticks 5 seen 14 base 1005\n", 0);
    }
    let _ = std::fs::remove_dir_all(&d);
    let (out, err, ok) = sspur(&["test", "tests/bare/ticks.ssp"]);
    assert!(ok && out.contains("3 passed, 0 failed"), "{out}{err}");
}

#[test]
fn fixed_arrays_match_across_tiers() {
    let native = sspur(&["run", "tests/bare/arrays.ssp"]);
    let interp = sspur(&["run", "--interp", "tests/bare/arrays.ssp"]);
    assert_eq!(native, interp);
    assert!(native.0.starts_with("ring 80 500 grid 14 pts 16 flags 5\n7\n"), "{native:?}");
    assert!(native.1.contains("index 3 out of bounds for list of length 3"), "{native:?}");
    let (out, _, _) = sspur(&["native", "--release", "tests/bare/arrays.ssp"]);
    for f in ["fill_ring", "grid", "pts", "flags", "at"] {
        assert!(out.contains(&format!("native  {f}\n")), "{f}: {out}");
    }
    let (out, _, ok) = sspur(&["fmt", "tests/bare/ticks.ssp"]);
    assert!(ok && out.contains("static seen: Array[Int, 4] = [0; 4]\n") && out.contains("  var a = [0; 8]\n") && out.contains("    a[i % 8] := a[i % 8] + i\n"), "{out}");
}

#[test]
fn statics_and_arrays_reject_unsafe_access() {
    let d = scratch("static-reject");
    let pre = "static n: Int = 0\n\n";
    let cases = [
        ("profile bare", "fn f() -> Int ! static\n= n + 1", "E_STATIC_ACCESS"),
        ("profile bare", "fn f() -> Int\n= n.load", "E_EFFECT_MISSING"),
        ("profile bare", "fn f() ! static\n= n.push(1)", "E_STATIC_ACCESS"),
        ("profile bare", "static s: Str = \"x\"", "E_STATIC_TYPE"),
        ("profile bare", "static m: Int = 1 + 2", "E_STATIC_INIT"),
        ("profile bare", "fn tick() ! static\n  interrupt timer\n= n.add(1)\n\nfn main() ! static\n= n.store(n.load + 1)", "E_STATIC_RACE"),
        ("profile bare", "fn tick() ! static\n  interrupt timer\n= n.add(1)\n\nfn main() ! static\n= n.store(n.swap(0) * 2)", "E_STATIC_RACE"),
        ("profile bare", "fn f(i: Int) -> Int\n= [1; 4][i] + [true; 2].len", ""),
        ("profile bare", "fn f(a: Array[Int]) -> Int\n= 0", "E_TYPE_ARITY"),
        ("", "fn f() -> Int\n= 0", "E_PROFILE"),
        ("", "fn f() -> Int\n= [0; 2].len", "E_PROFILE"),
    ];
    for (i, (profile, body, code)) in cases.iter().enumerate() {
        let f = d.join(format!("s{i}.ssp"));
        std::fs::write(&f, format!("{profile}\n\n{pre}{body}\n")).unwrap();
        let (out, err, ok) = sspur(&["check", f.to_str().unwrap()]);
        let all = format!("{out}{err}");
        if code.is_empty() {
            assert!(ok, "case {i} should pass: {all}");
        } else {
            assert!(!ok && all.contains(code), "case {i}: expected {code}, got {all}");
        }
    }
    let ok_src = "profile bare\n\nstatic n: Int = 0\n\nfn tick() ! static\n  interrupt timer\n= n.store(n.load + 1)\n\nfn main() -> Int ! static\n= do\n  n.add(1)\n  n.load\n";
    let f = d.join("ok.ssp");
    std::fs::write(&f, ok_src).unwrap();
    let (out, err, ok) = sspur(&["check", f.to_str().unwrap()]);
    assert!(ok, "a handler's own read-modify-write is not preempted by main: {out}{err}");
    let _ = std::fs::remove_dir_all(&d);
}

fn host_floats() -> String {
    let src = std::fs::read_to_string(root().join("tests/bare/floats.ssp")).unwrap();
    let pure = src.split("// host:").next().unwrap().trim_start_matches("profile bare\n");
    let d = scratch("floats-host");
    let f = d.join("host.ssp");
    std::fs::write(&f, format!("{pure}\nfn main() ! log\n= do\n  for k in 0..count()\n    p = parts(value(k))\n    log(\"{{label(k)}} {{if p.0 then \"-\" else \"\"}}{{p.1}}.{{p.2.format(\"09\")}}\")\n")).unwrap();
    let interp = sspur(&["run", "--interp", f.to_str().unwrap()]);
    let native = sspur(&["run", f.to_str().unwrap()]);
    assert!(interp.2, "{interp:?}");
    assert_eq!(interp, native, "host tiers disagree");
    let _ = std::fs::remove_dir_all(&d);
    interp.0
}

#[test]
fn f64_on_fpu_targets_matches_the_host_interpreter() {
    let expect = host_floats();
    assert!(expect.starts_with("sqrt2 1.414213562\nfsqrt 0.000000000\n") && expect.contains("\nspecial 127.000000000\n"), "{expect}");
    for target in ["aarch64-qemu", "thumbv7em-mps2"] {
        boots(target, "floats", &expect);
    }
    let (out, err, ok) = sspur(&["test", "tests/bare/floats.ssp"]);
    assert!(ok && out.contains("1 passed, 0 failed"), "{out}{err}");
    let d = scratch("floats-rv");
    let (_, err, ok) = sspur(&["build", "--target", "riscv64-qemu", "tests/bare/floats.ssp", "-o", d.join("k.elf").to_str().unwrap()]);
    assert!(!ok && err.contains("F64 needs a floating-point unit"), "{err}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn secondary_core_shares_atomic_statics_on_aarch64() {
    boots("aarch64-qemu", "smp", "core 1 started\ntotal 200000 seen 3\n");
    let d = scratch("smp-reject");
    let pre = "profile bare\n\nstatic n: Int = 0\n\n";
    for (body, code) in [
        ("fn core_main() ! static\n= n.add(1)", "E_INTERRUPT_SIG"),
        ("fn core_main(id: Int) ! static\n= n.store(n.load + id)\n\nfn main() ! static\n= n.add(1)", "E_STATIC_RACE"),
    ] {
        let f = d.join("c.ssp");
        std::fs::write(&f, format!("{pre}{body}\n")).unwrap();
        let (out, err, ok) = sspur(&["check", f.to_str().unwrap()]);
        assert!(!ok && format!("{out}{err}").contains(code), "{body}: {out}{err}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn inline_asm_reads_cycle_counters_on_both_targets() {
    for target in ["riscv64-qemu", "aarch64-qemu"] {
        boots(target, "cycles", "mix 63534 counter advanced\n");
    }
}

#[test]
fn inline_asm_runs_natively_in_sys_code_on_the_host() {
    if cfg!(target_arch = "aarch64") {
        let (out, err, ok) = sspur(&["run", "tests/asm/host_aarch64.ssp"]);
        assert!(ok, "{err}");
        assert_eq!(out, "sum 399995 divmod 9 2 zero true false counter advanced true\n");
    }
    let (_, err, ok) = sspur(&["run", "--interp", "tests/asm/host_aarch64.ssp"]);
    assert!(!ok && err.contains("inline asm needs native code"), "{err}");
    let (out, _, ok) = sspur(&["fmt", "tests/asm/host_aarch64.ssp"]);
    assert!(ok && out.contains("= asm \"sdiv {q}, {x}, {y}\\n msub {m}, {q}, {y}, {x}\" in(x: a, y: b) out(q: Int, m: Int)\n"), "{out}");
    assert!(out.contains("out(z: Bool) clobber(\"cc\")"), "{out}");
    let d = scratch("asm-reject");
    let cases = [
        ("profile sys\n\nfn f() -> Int\n= asm \"mov {r}, #1\" out(r: Int)", "E_UNSAFE"),
        ("fn f() -> Int ! unsafe\n= asm \"mov {r}, #1\" out(r: Int)", "E_PROFILE"),
        ("profile sys\n\nfn f(s: Str) -> Int ! unsafe\n= asm \"mov {r}, {s}\" in(s: s) out(r: Int)", "E_ASM_OPERAND"),
        ("profile sys\n\nfn f() -> Int ! unsafe\n= asm \"mov {r}, {q}\" out(r: Int)", "E_ASM_OPERAND"),
        ("profile bare\n\nfn f() -> Str ! unsafe\n= asm \"nop\" out(r: Str)", "E_ASM_OPERAND"),
        ("profile sys\n\nfn f(a: Int) -> Int ! unsafe\n= asm \"mov {r}, {a}\" in(a: a) out(a: Int)", "E_ASM_OPERAND"),
    ];
    for (i, (src, code)) in cases.iter().enumerate() {
        let f = d.join(format!("a{i}.ssp"));
        std::fs::write(&f, format!("{src}\n")).unwrap();
        let (out, err, ok) = sspur(&["check", f.to_str().unwrap()]);
        assert!(!ok && format!("{out}{err}").contains(code), "case {i}: expected {code}, got {out}{err}");
    }
    let _ = std::fs::remove_dir_all(&d);
}
