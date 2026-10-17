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
