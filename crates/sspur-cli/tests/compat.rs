//! The 1.x compatibility suite (docs/reference/stability.md). Every program and package under
//! tests/compat is frozen, and tests/compat/golden holds what 1.x promises about each: the
//! canonical text's hash, every definition's content hash, a package's export hashes and lock,
//! the codes `sspur check --json` gives, the output of `run` and `test` in the interpreter and
//! natively, and what `sspur fmt` writes. `SSPUR_COMPAT_BLESS=1` rewrites the goldens; the policy
//! allows that only for a new major version or a documented bug fix.

use sspur_syntax::{parse, print_module};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const BLESS: &str = "SSPUR_COMPAT_BLESS";

fn compat() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/compat")
}

struct Fixture {
    /// The name in messages and of the golden file.
    name: String,
    /// The directory, relative to tests/compat, the commands run in.
    dir: PathBuf,
    file: String,
    package: bool,
}

fn fixtures() -> Vec<Fixture> {
    let root = compat();
    let mut out = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(root.join("programs")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "ssp")).collect();
    files.sort();
    for p in files {
        let file = p.file_name().unwrap().to_string_lossy().into_owned();
        out.push(Fixture { name: file.trim_end_matches(".ssp").to_string(), dir: "programs".into(), file, package: false });
    }
    for dir in ["pulse", "ws/money", "ws/shop"] {
        let toml = std::fs::read_to_string(root.join(dir).join("sspur.toml")).unwrap();
        let src = toml.lines().find_map(|l| l.strip_prefix("src = ")).unwrap().trim_matches('"').to_string();
        out.push(Fixture { name: dir.replace('/', "-"), dir: dir.into(), file: src, package: true });
    }
    out
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let p = e.unwrap().path();
        let t = to.join(p.file_name().unwrap());
        if p.is_dir() {
            if p.file_name().unwrap() != "golden" {
                copy_dir(&p, &t);
            }
        } else {
            std::fs::copy(&p, &t).unwrap();
        }
    }
}

/// Stdout, and the exit code when it isn't 0.
fn sspur(dir: &Path, args: &[&str], native: bool) -> String {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sspur"));
    c.args(args).current_dir(dir).stdin(Stdio::null()).stderr(Stdio::piped()).env_remove("SSPUR_DEBUG");
    if native {
        c.env("SSPUR_STRICT_NATIVE", "1");
    }
    let o = c.output().unwrap();
    let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
    if !o.status.success() {
        s += &format!("-- exit {}\n", o.status.code().unwrap_or(-1));
    }
    s
}

/// The facts about one fixture, in the order the golden lists them.
fn facts(f: &Fixture, work: &Path) -> Vec<(String, String)> {
    let dir = work.join(&f.dir);
    let src = std::fs::read_to_string(dir.join(&f.file)).unwrap();
    let file = f.file.as_str();
    let mut out = Vec::new();
    let canon = parse(&src).map_or_else(|e| format!("does not parse: {e:?}"), |m| print_module(&m));
    out.push(("canonical".into(), format!("blake3 {}\n", blake3::hash(canon.as_bytes()).to_hex())));
    out.push(("hashes".into(), sspur(&dir, &["hash", "--full", file], false)));
    if f.package {
        let api: serde_json::Value = serde_json::from_str(&sspur(&dir, &["api", "--json"], false)).unwrap();
        let mut ex = api["hash"].as_str().map_or(String::new(), |h| format!("package {h}\n"));
        for d in api["defs"].as_array().unwrap().iter().filter(|d| d["public"] == true && d["kind"] != "test") {
            ex += &format!("#{}  {}\n", d["hash"].as_str().unwrap(), d["name"].as_str().unwrap());
        }
        out.push(("exports".into(), ex));
    }
    let check = sspur(&dir, &["check", "--json", "--no-fill", file], false);
    let codes: String = check
        .lines()
        .map(|l| match serde_json::from_str::<serde_json::Value>(l) {
            Ok(d) => format!("{} {}\n", d["code"].as_str().unwrap_or("?"), d["def"].as_str().unwrap_or("-")),
            Err(_) => format!("{l}\n"),
        })
        .collect();
    out.push(("check".into(), codes));
    if !check.contains("-- exit") {
        for (cmd, has) in [("run", src.lines().any(|l| l.starts_with("fn main("))), ("test", src.lines().any(|l| l.starts_with("test ")))] {
            if !has {
                continue;
            }
            let mut args = vec![cmd, file];
            if cmd == "test" {
                args.push("--full");
            }
            let interp = sspur(&dir, &[args.as_slice(), &["--interp"]].concat(), false);
            let native = sspur(&dir, &args, true);
            let same = native == interp;
            out.push((format!("{cmd} interp"), interp));
            out.push((format!("{cmd} native"), if same { format!("(same as {cmd} interp)\n") } else { native }));
        }
    }
    let fmt = sspur(&dir, &["fmt", file], false);
    out.push(("fmt".into(), if fmt == src { "(the source, unchanged)\n".into() } else { fmt }));
    if dir.join("sspur.lock").exists() {
        out.push(("lock".into(), std::fs::read_to_string(dir.join("sspur.lock")).unwrap()));
    }
    out
}

fn render(f: &Fixture, facts: &[(String, String)]) -> String {
    let mut s = format!("# {}/{}: frozen by the 1.x compatibility promise (docs/reference/stability.md)\n", f.dir.display(), f.file);
    for (k, v) in facts {
        s += &format!("== {k}\n{v}");
    }
    s
}

fn read_golden(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for l in text.split_inclusive('\n') {
        if let Some(k) = l.strip_prefix("== ") {
            out.push((k.trim_end().to_string(), String::new()));
        } else if let Some((_, v)) = out.last_mut() {
            v.push_str(l);
        }
    }
    out
}

/// What changed in one fact, as the first line that differs.
fn first_diff(was: &str, now: &str) -> String {
    let (a, b): (Vec<_>, Vec<_>) = (was.lines().collect(), now.lines().collect());
    let i = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let show = |v: &[&str]| v.get(i).map_or("(nothing)".to_string(), |l| format!("`{l}`"));
    format!("line {}: was {}, now {}", i + 1, show(&a), show(&b))
}

#[test]
fn the_1x_compatibility_suite_holds() {
    let bless = std::env::var_os(BLESS).is_some_and(|v| v == "1");
    let work = std::env::temp_dir().join(format!("sspur-compat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    copy_dir(&compat(), &work);
    let all = fixtures();
    assert!(all.len() >= 20, "{}", all.len());
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 6);
    std::thread::scope(|s| {
        for _ in 0..workers {
            std::thread::Builder::new()
                .stack_size(256 << 20)
                .spawn_scoped(s, || {
                    while let Some(f) = all.get(next.fetch_add(1, Ordering::SeqCst)) {
                        let r = facts(f, &work);
                        results.lock().unwrap().push((f.name.clone(), r));
                    }
                })
                .unwrap();
        }
    });
    let _ = std::fs::remove_dir_all(&work);
    let mut results = results.into_inner().unwrap();
    results.sort();
    let golden = compat().join("golden");
    let mut changed = Vec::new();
    for (name, now) in &results {
        let f = all.iter().find(|f| &f.name == name).unwrap();
        let path = golden.join(format!("{name}.txt"));
        if bless {
            std::fs::create_dir_all(&golden).unwrap();
            std::fs::write(&path, render(f, now)).unwrap();
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            changed.push(format!("{name}: no golden at tests/compat/golden/{name}.txt"));
            continue;
        };
        let was = read_golden(&text);
        for (k, v) in now {
            match was.iter().find(|(k2, _)| k2 == k) {
                None => changed.push(format!("{name}: `{k}` is new")),
                Some((_, old)) if old != v => changed.push(format!("{name}: `{k}` changed, {}", first_diff(old, v))),
                _ => {}
            }
        }
        for (k, _) in was.iter().filter(|(k, _)| !now.iter().any(|(k2, _)| k2 == k)) {
            changed.push(format!("{name}: `{k}` is gone"));
        }
    }
    if bless {
        eprintln!("compat: wrote {} goldens; list the change under Changed in CHANGELOG.md", results.len());
        return;
    }
    assert!(
        changed.is_empty(),
        "the 1.x compatibility suite found {} changed facts:\n  {}\n\nStable behaviour may not change in 1.x (docs/reference/stability.md). Fix the change, or, if it is a documented bug fix or a new major version, rerun with {BLESS}=1 (cargo test --release -p sspur-cli --test compat) and list it under Changed in CHANGELOG.md.",
        changed.len(),
        changed.join("\n  ")
    );
}
