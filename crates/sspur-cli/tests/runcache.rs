//! The cached path of `sspur run` (ADR 0034) behaves exactly like the normal path.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Run {
    out: String,
    err: String,
    code: Option<i32>,
}

fn run(dir: &Path, cache: &Path, args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Run {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sspur"));
    c.args(args).current_dir(dir).env("SSPUR_CACHE", cache).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        c.env(k, v);
    }
    let mut c = c.spawn().unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    Run { out: String::from_utf8_lossy(&o.stdout).into_owned(), err: String::from_utf8_lossy(&o.stderr).into_owned(), code: o.status.code() }
}

fn entries(cache: &Path) -> usize {
    std::fs::read_dir(cache.join("run")).map_or(0, |d| d.count())
}

fn setup(name: &str) -> (PathBuf, PathBuf) {
    let d = std::env::temp_dir().join(format!("sspur-runcache-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("prog")).unwrap();
    (d.join("prog"), d.join("cache"))
}

const PROG: &str = "type E = Bad{n: Int}

fn half(x: Int) -> Int
  pre x % 2 == 0
= x / 2

fn pick(n: Int) -> Int ! fail[E]
= if n > 100 then raise Bad{n} else n

fn main() -> Unit ! log, io, env, fail[E]
= do
  a = args()
  line = read_line().or(\"none\")
  log(\"args {a} line {line}\")
  mode = a.first.or(\"ok\")
  if mode == \"trap\" then log(\"{half(3)}\")
  if mode == \"raise\" then log(\"{pick(500)}\")
  if mode == \"div\" then log(\"{10 / (a.len - a.len)}\")
  eprint(\"done {mode}\")
";

#[test]
fn cached_runs_match_the_normal_path() {
    if Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let (dir, cache) = setup("match");
    std::fs::write(dir.join("p.ssp"), PROG).unwrap();
    for args in [vec![], vec!["x", "y"], vec!["trap"], vec!["raise"], vec!["div", "z"]] {
        let mut cmd = vec!["run", "p.ssp"];
        cmd.extend(args.iter().copied());
        let normal = run(&dir, &cache, &["run", "--no-cache", "p.ssp"].iter().copied().chain(args.iter().copied()).collect::<Vec<_>>(), "hello\n", &[]);
        let first = run(&dir, &cache, &cmd, "hello\n", &[]);
        let cached = run(&dir, &cache, &cmd, "hello\n", &[]);
        assert!(entries(&cache) >= 1, "no cache entry after {args:?}");
        for r in [&first, &cached] {
            assert_eq!((&r.out, &r.err, r.code), (&normal.out, &normal.err, normal.code), "{args:?}");
        }
    }
    let r = run(&dir, &cache, &["run", "p.ssp", "trap"], "", &[]);
    assert_eq!(r.out, "args [\"trap\"] line none\n");
    assert_eq!(r.err, "runtime error: contract violated: pre x % 2 == 0 in half (x = 3)\n");
    assert_eq!(r.code, Some(1));
    let r = run(&dir, &cache, &["run", "p.ssp", "raise"], "", &[]);
    assert_eq!(r.err, "runtime error: unhandled error: Bad{n: 500}\n");
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn anything_the_build_depends_on_invalidates_the_cache() {
    if Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let (dir, cache) = setup("invalidate");
    let p = dir.join("p.ssp");
    std::fs::write(&p, "fn main() ! log\n= log(\"one\")\n").unwrap();
    assert_eq!(run(&dir, &cache, &["run", "p.ssp"], "", &[]).out, "one\n");
    assert_eq!(run(&dir, &cache, &["run", "p.ssp"], "", &[]).out, "one\n");
    let n = entries(&cache);
    std::fs::write(&p, "fn main() ! log\n= log(\"two\")\n").unwrap();
    assert_eq!(run(&dir, &cache, &["run", "p.ssp"], "", &[]).out, "two\n");
    assert_eq!(entries(&cache), n + 1);
    for (k, v) in [("SSPUR_MEM", "gc"), ("SSPUR_MEM", "check"), ("SSPUR_OPT", "0")] {
        assert_eq!(run(&dir, &cache, &["run", "p.ssp"], "", &[(k, v)]).out, "two\n");
        assert_eq!(run(&dir, &cache, &["run", "p.ssp"], "", &[(k, v)]).out, "two\n");
    }
    assert_eq!(entries(&cache), n + 4);
    for e in std::fs::read_dir(cache.join("native")).unwrap() {
        let e = e.unwrap().path();
        if e.extension().is_some_and(|x| x == std::env::consts::DLL_EXTENSION) {
            std::fs::remove_file(e).unwrap();
        }
    }
    assert_eq!(run(&dir, &cache, &["run", "p.ssp"], "", &[]).out, "two\n");
    for e in std::fs::read_dir(cache.join("run")).unwrap() {
        std::fs::write(e.unwrap().path(), "lib /nonexistent\nlib-hash 0\nentry scalar\n").unwrap();
    }
    assert_eq!(run(&dir, &cache, &["run", "p.ssp"], "", &[]).out, "two\n");
    let warn = "fn say(s: Str) -> Unit ! log, io\n= log(s)\n\nfn main() ! log, io\n= say(\"three\")\n";
    std::fs::write(&p, warn).unwrap();
    let a = run(&dir, &cache, &["run", "p.ssp"], "", &[]);
    let b = run(&dir, &cache, &["run", "p.ssp"], "", &[]);
    assert_eq!((&a.out, &a.err), (&b.out, &b.err));
    assert!(a.err.contains("W_EFFECT_UNUSED"), "expected a warning: {}", a.err);
    assert_eq!(a.out, "three\n");
    std::fs::write(&p, "fn main() ! log\n= log(\"four\")\n").unwrap();
    assert_eq!(run(&dir, &cache, &["run", "p.ssp"], "", &[]).out, "four\n");
    std::fs::write(dir.join("sspur.toml"), "[package]\nname = \"demo\"\n").unwrap();
    let r = run(&dir, &cache, &["run", "p.ssp"], "", &[]);
    let n = run(&dir, &cache, &["run", "--no-cache", "p.ssp"], "", &[]);
    assert_eq!((&r.out, &r.err, r.code), (&n.out, &n.err, n.code));
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}
