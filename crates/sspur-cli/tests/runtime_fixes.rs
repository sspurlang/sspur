mod common;

use std::path::PathBuf;
use std::process::Command;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sspur-rtfix-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn has_cc() -> bool {
    Command::new("clang").arg("--version").output().is_ok()
}

fn sspur(d: &std::path::Path, args: &[&str]) -> (String, String, bool) {
    let o = Command::new(env!("CARGO_BIN_EXE_sspur")).args(args).current_dir(d).env("SSPUR_CACHE", d.join("cache")).env_remove("SSPUR_STRICT_NATIVE").output().unwrap();
    (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.success())
}

/// Runs `cmd` (`test` or `run`) on `src` natively (strict) and interpreted; both must agree and succeed.
fn both(name: &str, cmd: &str, src: &str) -> String {
    let d = dir(name);
    std::fs::write(d.join("p.ssp"), src).unwrap();
    let (iout, ierr, iok) = sspur(&d, &[cmd, "--interp", "p.ssp"]);
    assert!(iok, "interp {name}: {iout}{ierr}");
    if has_cc() {
        let (nout, nerr, nok) = sspur(&d, &[cmd, "--strict-native", "p.ssp"]);
        assert!(nok, "native {name}: {nout}{nerr}");
        assert_eq!(iout, nout, "{name}: tiers differ");
    }
    iout
}

#[test]
fn to_bits_alone_builds_natively() {
    let out = both("bits", "test", "fn g(n: Int) -> Bool\n= [4].to_bits(n).has(2)\n\ntest t = not g(8)\n\ntest u = not g(5)\n");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

#[test]
fn recursive_function_passed_by_name_and_inside_a_lambda_compiles() {
    let src = "type C = Leaf | All{cs: List[C]}\n\nfn depth(c: C) -> Int\n= match c\n  | Leaf => 0\n  | All{cs} => 1 + cs.map(depth).sum\n\nfn total(gs: List[List[C]]) -> Int\n= gs.map(g => g.map(depth).sum).sum\n\ntest t = total([[Leaf, All{cs: [Leaf, All{cs: [Leaf]}]}], [All{cs: []}]]) == 3\n";
    let out = both("depth", "test", src);
    assert!(out.contains("1 passed, 0 failed"), "{out}");
}

#[test]
fn interpreter_appends_are_linear() {
    let d = dir("appends");
    let src = "fn pushes(n: Int) -> Int\n= do\n  var xs = []\n  for i in 0..n\n    xs := xs.push(i)\n  xs.len\n\nfn folds(n: Int) -> Int\n= (0..n).fold([], (acc, i) => acc.push(i)).len\n\nfn bufs(n: Int) -> Int\n= do\n  var b = str_buf()\n  for i in 0..n\n    b := b.add(\"ab\")\n  b.byte_len\n\nfn puts(n: Int) -> Int\n= do\n  var m = empty_map()\n  for i in 0..n\n    m := m.put(i, i)\n  m.len\n\nfn main() -> Unit ! log\n= log(\"{pushes(200000)} {folds(200000)} {bufs(200000)} {puts(200000)}\")\n";
    std::fs::write(d.join("p.ssp"), src).unwrap();
    let (o, cpu) = common::run_timed(Command::new(env!("CARGO_BIN_EXE_sspur")).args(["run", "--interp", "p.ssp"]).current_dir(&d).env("SSPUR_CACHE", d.join("cache")).env_remove("SSPUR_STRICT_NATIVE"));
    let (out, err) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(o.status.success(), "{out}{err}");
    assert_eq!(out.trim(), "200000 200000 400000 200000");
    assert!(cpu.as_secs() < 30, "quadratic appends: {cpu:?} of CPU");
}

#[test]
fn interpreter_stack_is_sspur_stack_and_overflow_traps() {
    let d = dir("stackenv");
    std::fs::write(d.join("p.ssp"), "fn down(n: Int) -> Int\n= if n == 0 then 0 else 1 + down(n - 1)\n\nfn forever(n: Int) -> Int\n= 1 + forever(n + 1)\n\nfn main() -> Unit ! log, env\n= log(if args().len > 0 then \"{forever(0)}\" else \"{down(60000)}\")\n").unwrap();
    let go = |stack: Option<&str>, args: &[&str]| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_sspur"));
        c.args(["run", "--interp", "p.ssp"]).args(args).current_dir(&d).env("SSPUR_CACHE", d.join("cache"));
        match stack {
            Some(s) => c.env("SSPUR_STACK", s),
            None => c.env_remove("SSPUR_STACK"),
        };
        let o = c.output().unwrap();
        (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.success())
    };
    // Without SSPUR_STACK, calls stop at 20,000 deep, as before.
    let (out, err, ok) = go(None, &[]);
    assert!(!ok && out.is_empty() && err.contains("stack overflow in down"), "{out}{err}");
    let (_, err, ok) = go(None, &["x"]);
    assert!(!ok && err.contains("stack overflow in forever"), "{err}");
    // With it, the stack is the limit: 60,000 calls fit in 256 MB on every platform, not in 16 MB.
    assert_eq!(go(Some("256M"), &[]), ("60000\n".into(), String::new(), true));
    let (out, err, ok) = go(Some("16M"), &[]);
    assert!(!ok && out.is_empty() && err.contains("stack overflow in down"), "{out}{err}");
    let (_, err, ok) = go(Some("256M"), &["x"]);
    assert!(!ok && err.contains("stack overflow in forever"), "{err}");
    let (out, err, ok) = go(Some("lots"), &[]);
    assert!(!ok && out.is_empty() && err.contains("warning: SSPUR_STACK=lots is not a stack size") && err.contains("stack overflow in down"), "{out}{err}");
}

#[test]
fn discarding_a_pure_result_warns() {
    let d = dir("unused");
    let src = "fn helper(x: Int) -> Int\n= x + 1\n\nfn main() -> Unit ! log, fs\n= do\n  b = str_buf()\n  b.add(\"a\")\n  var c = str_buf()\n  c := c.add(\"b\")\n  _ = helper(1)\n  helper(2)\n  write_file(\"/nonexistent/x\", \"y\")\n  [1].map(x => do\n    log(\"{x}\")\n    x)\n  log(\"{b.str}{c.str}\")\n";
    std::fs::write(d.join("p.ssp"), src).unwrap();
    let (out, err, ok) = sspur(&d, &["run", "--interp", "p.ssp"]);
    assert!(ok, "{out}{err}");
    let warns: Vec<&str> = err.lines().filter(|l| l.contains("W_UNUSED_RESULT")).collect();
    assert_eq!(warns, ["p.ssp:7:3 warning W_UNUSED_RESULT the result of '.add' (StrBuf) is discarded", "p.ssp:11:3 warning W_UNUSED_RESULT the result of 'helper' (Int) is discarded"], "{err}");
    assert!(err.contains("keep the new StrBuf with 'b := b.add(\"a\")' (and 'var b')"), "{err}");
    assert_eq!(out, "1\nb\n");
}

#[test]
fn dependency_audits_are_listed_by_check_at_their_own_line() {
    let d = dir("audits");
    std::fs::create_dir_all(d.join("frlib")).unwrap();
    std::fs::create_dir_all(d.join("app")).unwrap();
    std::fs::write(d.join("frlib/sspur.toml"), "[package]\nname = \"frlib\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(d.join("frlib/lib.ssp"), "// A hash of the key.\npub fn tag(k: Secret[Bytes]) -> Bytes\n= k.map(b => crypto.sha256(b)).expose(\"a digest is public\")\n\n// Unused effect.\npub fn w(x: Int) -> Int ! log\n= x\n").unwrap();
    std::fs::write(d.join("app/sspur.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\nsrc = \"main.ssp\"\n\n[deps]\nfrlib = { path = \"../frlib\" }\n").unwrap();
    std::fs::write(d.join("app/main.ssp"), "fn main() -> Unit ! log\n= log(\"{frlib.tag(secret(bytes([1]))).len}\")\n").unwrap();
    let app = d.join("app");
    let cache = d.join("cache");
    let go = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_sspur")).args(args).current_dir(&app).env("SSPUR_CACHE", &cache).env_remove("SSPUR_STRICT_NATIVE").output().unwrap();
        (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string())
    };
    for args in [&["run", "--interp", "main.ssp"][..], &["run", "main.ssp"]] {
        let (out, err) = go(args);
        assert_eq!((out.as_str(), err.as_str()), ("32\n", ""), "{args:?}");
    }
    let (out, err) = go(&["check", "main.ssp"]);
    assert_eq!((out.as_str(), err.as_str()), ("ok 1 definitions\n", "frlib/lib.ssp:3:3 audit A_DECLASSIFY Secret value declassified with .expose: a digest is public\n"));
}

#[test]
fn a_stale_lock_is_named_with_the_fix() {
    let d = dir("stale");
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::create_dir_all(d.join("app")).unwrap();
    std::fs::write(d.join("lib/sspur.toml"), "[package]\nname = \"lib\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(d.join("lib/lib.ssp"), "// One.\npub fn one() -> Int\n= 1\n").unwrap();
    std::fs::write(d.join("app/sspur.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\nsrc = \"main.ssp\"\n\n[deps]\nlib = { path = \"../lib\" }\n").unwrap();
    std::fs::write(d.join("app/main.ssp"), "fn main() -> Unit ! log\n= log(\"{lib.one()}\")\n").unwrap();
    let app = d.join("app");
    let cache = d.join("cache");
    let go = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_sspur")).args(args).current_dir(&app).env("SSPUR_CACHE", &cache).env_remove("SSPUR_STRICT_NATIVE").output().unwrap();
        (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string())
    };
    assert_eq!(go(&["run", "--interp", "main.ssp"]), ("1\n".to_string(), String::new()));
    std::fs::write(d.join("lib/lib.ssp"), "// One.\npub fn one() -> Int\n= 1\n// a comment\n").unwrap();
    assert_eq!(go(&["run", "--interp", "main.ssp"]), ("1\n".to_string(), String::new()));
    std::fs::write(d.join("lib/lib.ssp"), "// One.\npub fn uno() -> Int\n= 1\n").unwrap();
    let why = "lib at ../lib changed since sspur.lock pinned it, and this build uses the locked version; run 'sspur deps update lib' to review and use the new one";
    let (out, err) = go(&["run", "--interp", "main.ssp"]);
    assert_eq!((out.as_str(), err.as_str()), ("1\n", format!("note: {why}\n").as_str()));
    std::fs::write(d.join("app/main.ssp"), "fn main() -> Unit ! log\n= log(\"{lib.uno()}\")\n").unwrap();
    let (_, err) = go(&["run", "main.ssp"]);
    assert_eq!(err, format!("main.ssp:2:9 E_PKG_NAME lib has no 'uno'; it exports one\n  hint: {why}\n"));
    go(&["deps", "update", "lib"]);
    assert_eq!(go(&["run", "main.ssp"]), ("1\n".to_string(), String::new()));
}

#[test]
fn float_to_int_conversions_trap_like_int_overflow() {
    let d = dir("f2i");
    let src = "fn r(x: F64) -> Int\n= x.round\n\nfn f(x: F64) -> Int\n= x.floor\n\nfn c(x: F64) -> Int\n= x.ceil\n\nfn t(x: F64) -> Int\n= x.trunc\n\ntest t_nan = r(nan()) == 0\ntest t_big = f(1.0e19) == 0\ntest t_inf = c(inf()) == 0\ntest t_low = t(-1.0e19) == 0\ntest t_edge = t(-9223372036854775808.0) == -9223372036854775807 - 1\n";
    std::fs::write(d.join("p.ssp"), src).unwrap();
    let want = "FAIL  t_nan: integer overflow\nFAIL  t_big: integer overflow\nFAIL  t_inf: integer overflow\nFAIL  t_low: integer overflow\n1 passed, 4 failed\n";
    let (iout, _, _) = sspur(&d, &["test", "--interp", "p.ssp"]);
    assert_eq!(iout, want);
    if has_cc() {
        let (nout, nerr, _) = sspur(&d, &["test", "--strict-native", "p.ssp"]);
        assert_eq!(nout, want, "{nerr}");
    }
}

#[test]
fn namespaced_calls_convert_arguments_and_nothing_else_does() {
    let src = "fn mine(b: Bytes) -> Int\n= b.len\n\nfn r() -> Response\n= http.bytes(200, \"hi\")\n\ntest lib_takes_str = crypto.sha256(\"abc\") == crypto.sha256(\"abc\".to_bytes)\ntest http_takes_str = r().data == \"hi\".to_bytes and r().body == \"\"\ntest own_is_explicit = mine(\"abc\".to_bytes) == 3\n";
    let out = both("coerce", "test", src);
    assert!(out.contains("3 passed, 0 failed"), "{out}");
    let d = dir("coerce-own");
    std::fs::write(d.join("p.ssp"), "fn mine(b: Bytes) -> Int\n= b.len\n\ntest t = mine(\"abc\") == 3\n").unwrap();
    let (_, err, ok) = sspur(&d, &["check", "p.ssp"]);
    assert!(!ok && err.contains("E_TYPE_MISMATCH"), "{err}");
}

#[test]
fn check_warns_about_conversions_it_cannot_prove() {
    let d = dir("conv");
    let src = "fn pct(p: F64 where _ >= 0.0 and _ <= 1.0) -> Int\n= (p * 100.0).round\n\nfn cut(x: F64) -> Int\n= if x > 0.0 then x.floor else 0\n\nfn main() -> Unit ! log\n= log(\"{pct(0.5)} {cut(2.5)}\")\n";
    std::fs::write(d.join("p.ssp"), src).unwrap();
    let z3 = Command::new("z3").arg("--version").output().is_ok() || std::path::Path::new("/opt/homebrew/bin/z3").exists();
    let (_, err, ok) = sspur(&d, &["check", "p.ssp"]);
    assert!(ok, "{err}");
    if z3 {
        let warns: Vec<&str> = err.lines().filter(|l| l.contains("W_CONVERSION_MAY_TRAP")).collect();
        assert_eq!(warns, ["p.ssp:5:19 warning W_CONVERSION_MAY_TRAP 'x.floor' traps when 'x' is NaN, infinite or outside Int's range, and that isn't ruled out here"], "{err}");
        assert!(err.contains("hint: use 'x.checked_floor' (an Opt[Int]), or guard it: 'if x.is_finite and x.abs < 9.2e18 then x.floor else ...'"), "{err}");
        let (out, _, _) = sspur(&d, &["check", "--json", "p.ssp"]);
        assert!(out.contains("\"code\":\"W_CONVERSION_MAY_TRAP\"") && out.contains("\"def\":\"cut\""), "{out}");
    }
    let (out, err, ok) = sspur(&d, &["run", "--interp", "p.ssp"]);
    assert!(ok && out == "50 2\n" && !err.contains("W_CONVERSION"), "run does not prove conversions: {out}{err}");
    let o = Command::new(env!("CARGO_BIN_EXE_sspur")).args(["check", "p.ssp"]).current_dir(&d).env("SSPUR_CACHE", d.join("cold")).env("SSPUR_Z3", "off").output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!err.contains("warning W_CONVERSION") && err.contains("note: 2 F64 to Int conversions are not checked for W_CONVERSION_MAY_TRAP: z3 not found"), "{err}");
}

#[test]
fn conversion_proofs_are_cached_and_follow_their_inputs() {
    if !(Command::new("z3").arg("--version").output().is_ok() || std::path::Path::new("/opt/homebrew/bin/z3").exists()) {
        return;
    }
    let d = dir("convcache");
    let v1 = "fn helper(t: F64, k: List[Int]) -> Int\n= t.floor\n\nfn caller(x: F64) -> Int\n= if x.abs < 100.0 then helper(x, []) else 0\n\nfn g(x: F64) -> F64\n= if x.abs < 10.0 then x else 0.0\n\nfn guarded(x: F64) -> Int\n= g(x).floor\n";
    let check = |src: &str, z3: bool| {
        std::fs::write(d.join("p.ssp"), src).unwrap();
        let mut c = Command::new(env!("CARGO_BIN_EXE_sspur"));
        c.args(["check", "p.ssp"]).current_dir(&d).env("SSPUR_CACHE", d.join("cache"));
        if !z3 {
            c.env("SSPUR_Z3", "off");
        }
        let o = c.output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr).to_string();
        assert!(!err.contains("note:"), "a cached result needs no Z3: {err}");
        let mut defs: Vec<String> = err.lines().filter(|l| l.contains("W_CONVERSION_MAY_TRAP")).map(|l| l.split(':').nth(1).unwrap().to_string()).collect();
        defs.sort();
        defs
    };
    assert!(check(v1, true).is_empty());
    assert!(check(v1, false).is_empty(), "the second check answers from the cache");
    let callee = v1.replace("if x.abs < 10.0 then x else 0.0", "x");
    assert_eq!(check(&callee, true), ["11"], "a callee's guard is part of its caller's key");
    let caller = v1.replace("x.abs < 100.0", "x.abs < 1.0e300 or x.is_nan");
    assert_eq!(check(&caller, true), ["2"], "a caller's guard is part of a private helper's key");
    assert!(check(v1, false).is_empty());
    assert_eq!(check(&callee, false), ["11"]);
    assert_eq!(check(&caller, false), ["2"]);
}
