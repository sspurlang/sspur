//! `sspur fix`: code written for 0.14 rewritten so that 1.0 reads it the same, and the places
//! that can't be rewritten listed. The programs in `tests/fix` are 0.14 code, and each `.out` is
//! what `sspur run --interp` printed for it under 0.14.0.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sspur-fix-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sspur(d: &Path, args: &[&str]) -> (String, String, bool) {
    let o = Command::new(env!("CARGO_BIN_EXE_sspur")).args(args).current_dir(d).env("SSPUR_CACHE", d.join("cache")).output().unwrap();
    (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.success())
}

/// `sspur fix` on `src`: the file it writes and what it printed. Running it again changes nothing.
fn fix(name: &str, src: &str) -> (String, String) {
    let d = dir(name);
    std::fs::write(d.join("p.ssp"), src).unwrap();
    let (out, err, ok) = sspur(&d, &["fix", "p.ssp"]);
    assert!(ok, "{out}{err}");
    let fixed = std::fs::read_to_string(d.join("p.ssp")).unwrap();
    let (again, _, ok) = sspur(&d, &["fix", "--check", "p.ssp"]);
    assert!(ok && !again.contains("fix"), "not idempotent:\n{again}\n{fixed}");
    assert_eq!(std::fs::read_to_string(d.join("p.ssp")).unwrap(), fixed);
    (fixed, out)
}

const MAIN: &str = "fn main() -> Unit ! log\n= do\n  x = 5\n";

#[test]
fn strings_keep_their_0_14_meaning() {
    for (before, after) in [
        ("  log(\"a{3}\")\n", "  log(\"a{(3)}\")\n"),
        ("  log(\"p{-3}q {1.5}\")\n", "  log(\"p{(-3)}q {(1.5)}\")\n"),
        ("  log(\"\\\\{x}\")\n", "  log(\"\\{x}\")\n"),
        ("  log(\"{\" + \"z\" + \"}\")\n", "  log(\"{\\\" + \\\"z\\\" + \\\"}\")\n"),
        ("  t = \"a{b\"\n  log((t.contains(\"{\") or t.contains(\"}\")).str)\n", "  t = \"a{b\"\n  log((t.contains(\"{(\") or t.contains(\")}\")).str)\n"),
    ] {
        let (got, out) = fix("str", &format!("{MAIN}{before}"));
        assert_eq!(got, format!("{MAIN}{after}"), "{out}");
        assert!(out.contains("fixed: string kept as 0.14 read it"), "{out}");
    }
    // Strings both versions read alike are left alone.
    let same = format!("{MAIN}  log(\"^a{{2,4}}$ {{}} v{{x}}w .a{{display:grid}} {{\\\"k\\\": 1}}\")\n");
    assert_eq!(fix("same", &same).0, same);
    // A pattern 0.14 read as text, and one whose 0.14 meaning 1.0 can't spell.
    let (got, _) = fix("pat", "fn k(s: Str) -> Int\n= match s\n  | \"a{x}\" => 1 // braces\n  | _ => 0\n");
    assert_eq!(got, "fn k(s: Str) -> Int\n= match s\n  | \"a\\{x}\" => 1 // braces\n  | _ => 0\n");
}

#[test]
fn source_0_14_could_not_lex_is_not_0_14_code() {
    let src = format!("{MAIN}  log(\"a{{3}} \\u{{e9}}\")\n");
    assert_eq!(fix("u", &src).0, src);
}

#[test]
fn nested_arms_go_back_to_the_inner_match() {
    // A catch needn't be exhaustive, so 1.0 would run `| B` for the outer catch.
    let src = "type E = | A | B\n\nfn g(n: Int) -> Int ! fail[E]\n= if n == 0 then raise A else if n == 1 then raise B else n\n\nfn h(n: Int) -> Int ! fail[E]\n= catch g(n)\n  | A => catch g(n + 1)\n      | A => 1 // inner\n  | B => 2\n";
    let (got, out) = fix("catch", src);
    assert_eq!(got, src.replace("\n  | B => 2", "\n      | B => 2"), "{out}");
    assert!(out.contains("p.ssp:10:3 fixed: in 0.14 this arm and the ones after it belonged to the match on line 8"), "{out}");
    // A 0.14 program that 1.0 rejects (the inner match isn't exhaustive alone).
    let src = "fn f(a: Int, b: Int) -> Str\n= match a\n  | 0 => \"zero\"\n  | n => match b\n      | 0 => \"b0\"\n  | _ => \"rest\"\n";
    assert_eq!(fix("broken", src).0, src.replace("\n  | _", "\n      | _"));
    // Arms that never run in either reading, and code written for 1.0, stay.
    for src in [
        "fn f(a: Int, b: Int) -> Str\n= match a\n  | 0 => \"zero\"\n  | _ => match b\n      | 0 => \"b0\"\n      | _ => \"bx\"\n  | 5 => \"five\"\n",
        "fn f(a: Opt[Int], b: Opt[Int]) -> Int\n= match a\n  | some(x) => match b\n      | some(y) => x + y\n      | none => x\n  | none => 0\n",
    ] {
        assert_eq!(fix("keep", src).0, src);
    }
}

#[test]
fn annotations_0_14_ignored_are_removed_when_they_dont_hold() {
    let src = "fn main() -> Unit ! log\n= do\n  x: I32 = 7 // seven\n  var t: F64 = 3\n  ok: List[Int] = [x]\n  log(\"{x} {t} {ok}\")\n";
    let (got, out) = fix("ann", src);
    assert_eq!(got, "fn main() -> Unit ! log\n= do\n  x = 7 // seven\n  var t = 3\n  ok: List[Int] = [x]\n  log(\"{x} {t} {ok}\")\n", "{out}");
    assert!(out.contains("p.ssp:3:4 fixed: 0.14 ignored the annotation 'I32'"), "{out}");
}

#[test]
fn changes_that_cant_be_rewritten_are_listed() {
    let src = "type Key = {id: Int, note: Str}\n\nimpl Eq for Key\n  fn eq(a: Self, b: Self) -> Bool = a.id == b.id\n\ntype Pair = {k: Key, n: Int} derive Eq\n\nfn same(a: Key, b: Key) -> Bool\n= a == b and some(a) == some(b) and Pair{k: a, n: 1} != Pair{k: b, n: 1}\n\nfn cents(x: F64) -> Int\n= (x * 100.0).round + x.floor + 2.5.ceil\n\nfn neg(x: F64) -> Bool\n= x < 0.0 or x == -0.0\n\nfn ok(d: Dec) -> Dec\n= d.round(2)\n";
    let (got, out) = fix("notes", src);
    assert_eq!(got, src);
    let lines: Vec<&str> = out.lines().collect();
    let want = [
        "p.ssp:9:22 check: == on Opt[Key] compares the Key inside with its impl Eq",
        "p.ssp:9:54 check: != on Pair compares the Key inside with its impl Eq",
        "p.ssp:12:14 check: .round traps on NaN",
        "p.ssp:12:24 check: .floor traps on NaN",
        "p.ssp:12:36 check: .ceil traps on NaN",
        "p.ssp:15:5 check: '<' with a zero",
        "p.ssp:15:16 check: '==' with a zero",
    ];
    assert_eq!(lines.len(), want.len(), "{out}");
    for (l, w) in lines.iter().zip(want) {
        assert!(l.starts_with(w), "{l}\nwant {w}\n{out}");
    }
}

#[test]
fn check_lists_without_writing_and_fails() {
    let d = dir("check");
    let src = format!("{MAIN}  log(\"a{{3}}\")\n");
    std::fs::write(d.join("p.ssp"), &src).unwrap();
    std::fs::write(d.join("q.ssp"), "fn f(x: F64) -> Int = x.round\n").unwrap();
    let (out, err, ok) = sspur(&d, &["fix", "--check"]);
    assert!(!ok, "{out}{err}");
    assert!(out.contains("p.ssp:4:7 would fix: string kept as 0.14 read it: \"a{3}\" -> \"a{(3)}\""), "{out}");
    assert!(out.contains("q.ssp:1:24 check: .round traps"), "{out}");
    assert!(err.contains("would rewrite 1 places in 1 of 2 files; 1 places to check"), "{err}");
    assert_eq!(std::fs::read_to_string(d.join("p.ssp")).unwrap(), src);
    // Places to check alone don't fail it.
    let (_, _, ok) = sspur(&d, &["fix", "--check", "q.ssp"]);
    assert!(ok);
}

/// Each 0.14 program, rewritten, prints under 1.0 what it printed under 0.14.
#[test]
fn rewritten_programs_print_what_0_14_printed() {
    let d = dir("golden");
    let src = root().join("crates/sspur-cli/tests/fix");
    let mut names: Vec<String> = std::fs::read_dir(&src).unwrap().flatten().filter_map(|e| e.file_name().to_str()?.strip_suffix(".ssp").map(String::from)).collect();
    names.sort();
    assert_eq!(names, ["annotations", "catch", "nested", "strings"]);
    for n in &names {
        std::fs::copy(src.join(format!("{n}.ssp")), d.join(format!("{n}.ssp"))).unwrap();
        let (before, _, _) = sspur(&d, &["run", "--interp", &format!("{n}.ssp")]);
        assert_ne!(before, std::fs::read_to_string(src.join(format!("{n}.out"))).unwrap(), "{n} reads the same in 1.0 without fixing");
    }
    let (out, err, ok) = sspur(&d, &["fix"]);
    assert!(ok, "{out}{err}");
    for n in &names {
        let want = std::fs::read_to_string(src.join(format!("{n}.out"))).unwrap();
        let (got, err, ok) = sspur(&d, &["run", "--interp", &format!("{n}.ssp")]);
        assert!(ok, "{n}: {err}");
        assert_eq!(got, want, "{n}:\n{}", std::fs::read_to_string(d.join(format!("{n}.ssp"))).unwrap());
    }
}

/// Every `.ssp` file of the repository and every corpus program reads the same in 0.14 and 1.0,
/// so `sspur fix` rewrites none of them (only the 0.14 programs above and those of the
/// migration guide).
#[test]
fn fix_is_a_no_op_on_the_repository_and_the_corpus() {
    let d = dir("repo");
    let corpus = std::fs::read_to_string(root().join("bench/eval/corpus.jsonl")).unwrap();
    for (i, l) in corpus.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        std::fs::write(d.join(format!("{i:03}.ssp")), v["src"].as_str().unwrap()).unwrap();
    }
    let r = root();
    let (out, err, ok) = sspur(&r, &["fix", "--check", ".", d.to_str().unwrap()]);
    let rewrites: Vec<&str> = out.lines().filter(|l| l.contains("would fix")).filter(|l| !l.contains("tests/fix/") && !l.contains("get-started/migrating/")).collect();
    assert!(rewrites.is_empty(), "{rewrites:#?}");
    assert!(!ok && err.contains(" in 7 of "), "{err}");
}
