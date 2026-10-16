//! The registry's `build.fmt` accepts exactly what `sspur fmt` writes, and the one-line form
//! that 0.13 wrote, for every program the repository has.

use sspur_registry::facts::fmt_check;
use sspur_syntax::{format_module, format_module_at, parse, print_module};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ssp_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            ssp_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "ssp") {
            out.push(p);
        }
    }
}

fn check_all() {
    let mut srcs: Vec<(String, String)> = Vec::new();
    for l in std::fs::read_to_string(root().join("bench/eval/corpus.jsonl")).unwrap().lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        srcs.push((format!("corpus {}", v["id"].as_str().unwrap()), v["src"].as_str().unwrap().to_string()));
    }
    let mut files = Vec::new();
    for d in ["tests/programs", "tests/fuzz", "docs/snippets", "examples", "templates", "pulse"] {
        ssp_files(&root().join(d), &mut files);
    }
    files.sort();
    srcs.extend(files.into_iter().map(|p| (p.display().to_string(), std::fs::read_to_string(&p).unwrap())));
    // Layouts that used to disagree: a `match` as a call argument inside a lambda, and
    // `if .. then do` / `else do` blocks in a `for` body.
    srcs.push(("match in a lambda".into(), "fn f(n: Int) -> Int\n= n + 1\n\nfn g(xs: List[Opt[Int]]) -> List[Int]\n= xs.map(o => f(match o\n  | some(v) => v\n  | none => 0))\n\nfn h(xs: List[Opt[Int]]) -> List[Int]\n= xs.map(o => (match o\n  | some(v) => v\n  | none => 0) + 1)\n".into()));
    srcs.push(("if blocks in a for".into(), "fn f(xs: List[Int]) -> Int\n= do\n  var n = 0\n  for x in xs\n    if x > 0 then do\n      n := n + x\n      n := n * 2\n    else do\n      n := n - 1\n      n := n * 3\n  n\n".into()));
    let mut bad = Vec::new();
    for (name, src) in &srcs {
        let Ok(m) = parse(src) else { continue };
        if std::env::var("SHOW").is_ok_and(|v| name.contains(&v)) {
            println!("{}\n{}", format_module(&m), print_module(&m));
        }
        for (form, text) in [("fmt", format_module(&m)), ("fmt at 40", format_module_at(&m, 40)), ("0.13", print_module(&m))] {
            let r = fmt_check(&text, &parse(&text).unwrap());
            if r["clean"] != true {
                bad.push(format!("{name} ({form}): {r}"));
            }
        }
    }
    assert!(bad.is_empty(), "{} failures:\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn build_fmt_accepts_what_sspur_fmt_writes() {
    std::thread::Builder::new().stack_size(1 << 29).spawn(check_all).unwrap().join().unwrap();
}
