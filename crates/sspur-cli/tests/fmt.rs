//! `sspur fmt` breaks long lines (ADR 0040) without changing what the canonical text, the
//! hashes or the program are.

use sspur_hash::hash_module;
use sspur_syntax::{format_module, format_module_at, parse, print_module};
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Every corpus program and every `.ssp` file of the tests, docs, examples and templates that
/// parses.
fn sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let corpus = std::fs::read_to_string(root().join("bench/eval/corpus.jsonl")).unwrap();
    for l in corpus.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        out.push((format!("corpus {}", v["id"].as_str().unwrap()), v["src"].as_str().unwrap().to_string()));
    }
    let mut files = Vec::new();
    for d in ["tests/programs", "tests/fuzz", "docs/snippets", "examples", "templates", "pulse"] {
        ssp_files(&root().join(d), &mut files);
    }
    files.sort();
    out.extend(files.into_iter().map(|p| (p.display().to_string(), std::fs::read_to_string(&p).unwrap())));
    out.retain(|(_, s)| parse(s).is_ok());
    out
}

fn big_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().stack_size(1 << 29).spawn(f).unwrap().join().unwrap();
}

#[test]
fn wrapping_keeps_the_canonical_text_and_the_hashes_at_any_width() {
    big_stack(wrapping_keeps_meaning);
}

fn wrapping_keeps_meaning() {
    let srcs = sources();
    assert!(srcs.len() > 250, "{}", srcs.len());
    let mut wrapped = 0;
    let mut odd = Vec::new();
    for (name, src) in &srcs {
        let m = parse(src).unwrap();
        let canon = print_module(&m);
        // The one-line canonical text, as 0.13 wrote it, is the reference: its hashes are the
        // source's except where that text already reads back differently (see below).
        let hashes = hash_module(&parse(&canon).unwrap());
        if hashes != hash_module(&m) {
            odd.push(name.clone());
        }
        for w in [16, 40, 60, 80, 100, 1000] {
            let out = format_module_at(&m, w);
            let m2 = parse(&out).unwrap_or_else(|e| panic!("{name} at {w}: {e:?}\n{out}"));
            assert_eq!(print_module(&m2), canon, "{name} at {w}:\n{out}");
            assert_eq!(hash_module(&m2), hashes, "{name} at {w}");
            assert_eq!(format_module_at(&m2, w), out, "{name} at {w}: not idempotent");
            if w == 100 && out != canon {
                wrapped += 1;
            }
        }
        // The canonical text reads back as itself, as before.
        assert_eq!(print_module(&parse(&canon).unwrap()), canon, "{name}");
    }
    assert!(wrapped >= 20, "only {wrapped} sources have a line over 100 columns");
    // A `catch` with arms as the `else` of an `if` in a match arm prints its arms where the
    // outer match's are: a canonical-text bug older than line breaking, not changed by it.
    assert_eq!(odd, ["corpus t46"]);
}

/// What 0.13.0 printed for every corpus program, by blake3: the canonical text, which the
/// store and the export hashes of packages (`sspur.lock`, the registry) are built from.
#[test]
fn the_canonical_text_is_what_0_13_printed() {
    let corpus = std::fs::read_to_string(root().join("bench/eval/corpus.jsonl")).unwrap();
    let mut h = blake3::Hasher::new();
    let mut n = 0;
    for l in corpus.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        if let Ok(m) = parse(v["src"].as_str().unwrap()) {
            h.update(print_module(&m).as_bytes());
            n += 1;
        }
    }
    assert_eq!((n, h.finalize().to_hex().to_string()), (GOLDEN_COUNT, GOLDEN.to_string()));
}

const GOLDEN_COUNT: usize = 210;
const GOLDEN: &str = "a0e6ed1102a71f3c9fa89419edabda2aa48f4c0d4ebf1421ae8edaa2633c395a";

#[test]
fn long_literals_calls_and_chains_break_one_element_per_line() {
    big_stack(breaks);
}

fn breaks() {
    let src = "type Order = {customer: Str, item: Str, cents: Int}\n\nfn main() -> Unit ! log\n= do\n  orders = [Order{customer: \"ana\", item: \"pen\", cents: 300}, Order{customer: \"bo\", item: \"ink\", cents: 900}, Order{customer: \"ana\", item: \"pad\", cents: 700}, Order{customer: \"cy\", item: \"pen\", cents: 300}]\n  log(\"{orders.len}\")\n";
    let want = "type Order = {customer: Str, item: Str, cents: Int}\n\nfn main() -> Unit ! log\n= do\n  orders = [\n    Order{customer: \"ana\", item: \"pen\", cents: 300},\n    Order{customer: \"bo\", item: \"ink\", cents: 900},\n    Order{customer: \"ana\", item: \"pad\", cents: 700},\n    Order{customer: \"cy\", item: \"pen\", cents: 300},\n  ]\n  log(\"{orders.len}\")\n";
    assert_eq!(format_module(&parse(src).unwrap()), want);
    let cases = [
        // Short things stay on one line.
        ("fn f(x: Int) -> Int\n= x + 1\n", "fn f(x: Int) -> Int\n= x + 1\n"),
        (
            "fn ok(a: Int, b: Int, c: Int) -> Bool\n= a > 1000000000 and b > 1000000000 and c > 1000000000 and a + b + c < 9000000000000 and a != b and b != c\n",
            "fn ok(a: Int, b: Int, c: Int) -> Bool\n= a > 1000000000 and\n  b > 1000000000 and\n  c > 1000000000 and\n  a + b + c < 9000000000000 and\n  a != b and\n  b != c\n",
        ),
        (
            "fn total(xs: List[Int]) -> Int\n= xs.filter(x => x > 1000000).map(x => x * 1000 + 17).filter(x => x % 3 == 0).map(x => x / 7).filter(x => x > 0).sum\n",
            "fn total(xs: List[Int]) -> Int\n= xs\n  .filter(x => x > 1000000)\n  .map(x => x * 1000 + 17)\n  .filter(x => x % 3 == 0)\n  .map(x => x / 7)\n  .filter(x => x > 0)\n  .sum\n",
        ),
        (
            "type P = {name: Str, age: Int, email: Str}\n\nfn older(p: P) -> P\n= p with age := p.age + 1, email := \"someone-with-a-rather-long-address@mail.example.com\", name := \"x\"\n",
            "type P = {name: Str, age: Int, email: Str}\n\nfn older(p: P) -> P\n= p with\n  age := p.age + 1,\n  email := \"someone-with-a-rather-long-address@mail.example.com\",\n  name := \"x\"\n",
        ),
        (
            "fn grade(n: Int) -> Str\n= if n > 90 then \"excellent work\" else if n > 75 then \"good work\" else if n > 50 then \"pass\" else \"fail\"\n",
            "fn grade(n: Int) -> Str\n= if n > 90 then \"excellent work\"\n  else if n > 75 then \"good work\"\n  else if n > 50 then \"pass\"\n  else \"fail\"\n",
        ),
        (
            "fn connect(host: Str, port: Int, user: Str, password: Str, timeout_ms: Int, retries: Int, verbose: Bool) -> Str\n= host\n",
            "fn connect(\n  host: Str,\n  port: Int,\n  user: Str,\n  password: Str,\n  timeout_ms: Int,\n  retries: Int,\n  verbose: Bool,\n) -> Str\n= host\n",
        ),
    ];
    for (src, want) in cases {
        let got = format_module(&parse(src).unwrap());
        assert_eq!(got, want, "\n{got}");
    }
}

fn sspur(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_sspur")).args(args).output().unwrap();
    (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

/// The suite programs, formatted narrow so that most of their lines break, pass the same tests
/// in the interpreter and in native code as the originals.
#[test]
fn formatted_programs_run_the_same_in_both_tiers() {
    big_stack(run_both_tiers);
}

fn run_both_tiers() {
    let dir = std::env::temp_dir().join(format!("sspur-fmt-tiers-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut files = Vec::new();
    ssp_files(&root().join("tests/programs"), &mut files);
    files.sort();
    for p in files {
        let src = std::fs::read_to_string(&p).unwrap();
        let m = parse(&src).unwrap();
        let narrow = dir.join(p.file_name().unwrap());
        std::fs::write(&narrow, format_module_at(&m, 40)).unwrap();
        let (o, n) = (p.to_str().unwrap(), narrow.to_str().unwrap());
        for mode in [&["test", "--interp"][..], &["test"]] {
            let (a, b) = (sspur(&[mode, &[o]].concat()), sspur(&[mode, &[n]].concat()));
            assert_eq!(a.0, b.0, "{o} {mode:?}: {}\n{}", a.1, b.1);
            // Diagnostics name other lines in the narrow text.
            let lines = |s: String, f: &str| s.replace(f, "F").lines().map(|l| if l.starts_with("F:") { l.split_once(' ').map_or(l, |x| x.1).to_string() } else { l.to_string() }).collect::<Vec<_>>();
            assert_eq!(lines(a.1, o), lines(b.1, n), "{o} {mode:?}");
        }
        let (ok, out) = sspur(&["fmt", n]);
        assert!(ok);
        assert_eq!(out, format_module(&m), "{o}: sspur fmt of the narrow text");
    }
    std::fs::remove_dir_all(&dir).ok();
}
