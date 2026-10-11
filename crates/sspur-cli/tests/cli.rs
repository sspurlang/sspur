use std::io::Write;
use std::process::{Command, Stdio};

fn sspur(dir: &std::path::Path, args: &[&str], stdin: &str) -> (String, bool) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sspur")).args(args).current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    (String::from_utf8_lossy(&o.stdout).trim_end().to_string() + &String::from_utf8_lossy(&o.stderr), o.status.success())
}

fn fresh(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sspur-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("a.ssp"), "fn one() -> Int\n= 1\n\nfn two() -> Int\n= one() + one()\n\ntest two_t = two() == 2\n").unwrap();
    assert_eq!(sspur(&d, &["init", "a.ssp"], ""), ("ok 3 definitions".into(), true));
    d
}

#[test]
fn failing_tests_show_the_values_in_both_tiers() {
    let d = fresh("values");
    let src = "type E = Bad{n: Int}\n\nfn g(x: Int) -> Int ! fail[E]\n= if x > 3 then raise Bad{n: x} else x\n\nfn c(lo: Int, hi: Int, s: Str) -> Int\n  pre lo <= hi\n= hi - lo\n\ntest t_eq = two() + 5 == 8\ntest t_and = two() == 2 and [two()].contains(3)\ntest t_catch = catch g(5) == 5\n  | Bad{n} => n == 4\ntest t_raised = catch g(9) == 9\n  | _ => false\ntest t_pre = c(3, 0, \"a\") == 0\ntest t_do = do\n  x = \"a{two()}\"\n  x == \"a3\"\n";
    let want = "FAIL  t_and: [2].contains(3) is false\nFAIL  t_catch: raised Bad{n: 5}: left 5, right 4\nFAIL  t_do: left \"a2\", right \"a3\"\nFAIL  t_eq: left 7, right 8\nFAIL  t_pre: contract violated: pre lo <= hi in c (lo = 3, hi = 0)\nFAIL  t_raised: raised Bad{n: 9}\n1 passed, 6 failed";
    for tier in ["--interp", "--strict-native"] {
        let (out, ok) = sspur(&d, &["edit", "--test", tier], src);
        assert!(!ok);
        assert_eq!(out.split_once('\n').unwrap().1, want, "{tier}");
    }
}

#[test]
fn edit_replaces_by_name_and_runs_tests() {
    let d = fresh("edit");
    let (out, ok) = sspur(&d, &["edit", "--test", "--interp"], "fn one() -> Int\n= 2\n\nfn three() -> Int\n= one() + 1\n");
    assert!(!ok);
    assert_eq!(out, "ok ~one +three\nFAIL  two_t: left 4, right 2\n0 passed, 1 failed");
    let (out, ok) = sspur(&d, &["edit", "-e", "rename three tri\nremove two_t"], "");
    assert!(ok, "{out}");
    assert_eq!(out, "ok three->tri -two_t");
    assert_eq!(sspur(&d, &["q", "body", "tri"], "").0, "fn tri() -> Int\n= one() + 1");
}

#[test]
fn rejected_edit_reports_def_relative_errors() {
    let d = fresh("reject");
    let (out, ok) = sspur(&d, &["edit"], "fn two() -> Int\n= do\n  x = one()\n  x + \"a\"\n");
    assert!(!ok);
    assert!(out.starts_with("rejected, nothing changed\ntwo:4:"), "{out}");
    let (out, _) = sspur(&d, &["edit", "-e", "fn x( -> Int\n= 1"], "");
    assert!(out.contains("input:1:"), "{out}");
}

#[test]
fn apply_accepts_inline_ops_and_keeps_json() {
    let d = fresh("apply");
    let (out, ok) = sspur(&d, &["apply", "-e", r#"[{"op": "attach", "target": "one", "kind": "test", "value": "one() == 1"}]"#, "--test", "--interp"], "");
    assert!(ok, "{out}");
    assert_eq!(out, "ok +one_t1\n2 passed, 0 failed");
    let (out, ok) = sspur(&d, &["apply", "--json"], r#"{"ops": [{"op": "remove", "path": "one_t1"}]}"#);
    assert!(ok && out.starts_with(r#"{"ok":true,"root":""#), "{out}");
    assert_eq!(sspur(&d, &["q", "list"], "").0, "# fns 2\nfn one() -> Int\nfn two() -> Int\n# tests 1: two_t");
    assert_eq!(sspur(&d, &["q", "list", "--tests"], "").0, "# tests 1\ntwo_t");
    assert_eq!(sspur(&d, &["q", "find", "TWO"], "").0, "fn two() -> Int\ntests: two_t");
    assert_eq!(sspur(&d, &["q", "grep", "one()"], "").0, "fn one() -> Int\nfn two() -> Int\n  = one() + one()");
    assert_eq!(sspur(&d, &["q", "body", "one", "two"], "").0, "fn one() -> Int\n= 1\n\nfn two() -> Int\n= one() + one()");
}

#[test]
fn find_and_grep_cap_their_text_output() {
    let d = fresh("hits_cap");
    let mut src = String::new();
    for i in 0..30 {
        src.push_str(&format!("fn f{i:02}(x: Int) -> Int\n= one() + {i}\n\n"));
    }
    src.push_str("fn f(x: Int) -> Int\n= one()\n");
    let (out, ok) = sspur(&d, &["edit", "-e", &src], "");
    assert!(ok, "{out}");
    let (out, _) = sspur(&d, &["q", "grep", "one()"], "");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.iter().filter(|l| l.starts_with("fn ")).count(), 12, "12 definitions in full: {out}");
    let last = lines.last().unwrap();
    assert!(last.starts_with("-- 21 more: f") && last.contains(" f29"), "{last}");
    let (out, _) = sspur(&d, &["q", "find", "f"], "");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "fn f(x: Int) -> Int", "the exact name first");
    assert_eq!(lines.iter().filter(|l| l.starts_with("fn ")).count(), 25, "25 signatures: {out}");
    assert!(lines.last().unwrap().starts_with("-- 6 more: f"), "{out}");
    let (full, _) = sspur(&d, &["q", "grep", "one()", "--json"], "");
    assert!(full.contains("\"f29\""), "--json is uncapped past the text cap: {full}");
}

#[test]
fn start_prints_the_spec_and_a_small_codebase_whole() {
    let spec = sspur(&std::env::temp_dir(), &["spec"], "").0;
    let d = fresh("start_small");
    let (out, ok) = sspur(&d, &["start", "one"], "");
    assert!(ok);
    let src = sspur(&d, &["src"], "").0;
    assert_eq!(out, format!("{spec}\n## This codebase: 2 fns, 1 test, all of it\n{src}"));
    let empty = std::env::temp_dir().join(format!("sspur-cli-start-none-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    let (out, ok) = sspur(&empty, &["start"], "");
    assert!(ok && out.starts_with(&spec) && out.ends_with("or the first edit creates it."), "{out}");
}

#[test]
fn start_packs_named_definitions_of_a_large_codebase() {
    let d = fresh("start_large");
    let mut src = String::new();
    for i in 0..300 {
        src.push_str(&format!("fn padding_function_number_{i:03}(x: Int) -> Int\n= one() + x * {i}\n\n"));
    }
    let (out, ok) = sspur(&d, &["edit", "-e", &src], "");
    assert!(ok, "{out}");
    let (out, ok) = sspur(&d, &["start", "two,one", "number_00*"], "");
    assert!(ok, "{out}");
    let rest = &out[out.find("## This codebase").expect("a codebase section")..];
    let (head, rest) = rest.split_once("\n\n").unwrap();
    assert!(head.starts_with("## This codebase: 302 fns, 1 test, about ") && head.ends_with("too much to print. Search it with `q find|grep|body|callers|pack`."), "{head}");
    let pack = sspur(&d, &["q", "pack", "two,one"], "").0;
    let find = sspur(&d, &["q", "find", "number_00*"], "").0;
    assert_eq!(rest, format!("## q pack two,one\n{pack}\n\n## q find 'number_00*'\n{find}"));
    let (out, _) = sspur(&d, &["start"], "");
    assert!(out.ends_with("also packs the named definitions (and finds other words) in this output."), "{out}");
}

#[test]
fn spec_runs_queries_after_the_spec() {
    let spec = sspur(&std::env::temp_dir(), &["spec"], "").0;
    let d = fresh("spec_q");
    let (out, ok) = sspur(&d, &["spec", "src", "find", "tw*", "callers", "one"], "");
    assert!(ok, "{out}");
    let src = sspur(&d, &["src"], "").0;
    assert_eq!(out, format!("{spec}\n\n## src\n{src}\n\n## q find 'tw*'\nfn two() -> Int\ntests: two_t\n\n## q callers one\ntwo"));
    let (out, ok) = sspur(&d, &["spec", "pack"], "");
    assert!(!ok && out.trim_end().ends_with("query 'pack' needs a target: spec pack TARGET"), "{out}");
}

#[test]
fn explain_opt_lists_proven_rewrites() {
    let d = fresh("explain");
    let prog = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/programs/pipelines.ssp");
    let (out, ok) = sspur(&d, &["explain-opt", prog.to_str().unwrap()], "");
    assert!(ok, "{out}");
    for want in ["proof: intervals: condition 'n < 0' is always false", "spread  map-map", "total  loop-fusion", "sum_sq  inline", "scaled  fold"] {
        assert!(out.contains(want), "missing {want}:\n{out}");
    }
    if out.contains("width  dead-branch") {
        assert!(out.contains("proof: z3: condition 'lo > hi' is always false"), "{out}");
    }
    assert!(!out.contains("shaky  map-map"), "{out}");
    let (all, _) = sspur(&d, &["explain-opt", prog.to_str().unwrap(), "--all"], "");
    assert!(all.contains("note  shaky: not fusing maps: trap order could change"), "{all}");
}

#[test]
fn profile_counts_guide_inlining() {
    let d = fresh("profile");
    std::fs::write(d.join("p.ssp"), "fn hot(x: Int) -> Int\n= x * 3 + 1\n\nfn cold(x: Int) -> Int\n= x - 1\n\nfn run(n: Int) -> Int\n= do\n  var s = 0\n  for i in 0..n\n    s := s + hot(i)\n  if s < 0 then cold(s) else s\n\nfn main() -> Unit ! log\n= log(\"{run(100)}\")\n").unwrap();
    let go = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_sspur")).args(args).current_dir(&d).env("SSPUR_CACHE", d.join("cache")).output().unwrap();
        String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)
    };
    let before = go(&["explain-opt", "p.ssp", "--all"]);
    assert!(before.contains("run  inline\n    cold(s)"), "{before}");
    let out = go(&["run", "--profile", "p.ssp"]);
    assert!(out.starts_with("14950\nprofile: 3 functions, 102 calls"), "{out}");
    let after = go(&["explain-opt", "p.ssp", "--all"]);
    assert!(after.contains("run  inline\n    hot(i)") && after.contains("note  run: not inlining cold: cost: profile: never called"), "{after}");
}

#[cfg(unix)]
#[test]
fn failed_native_builds_warn_fall_back_and_fail_under_strict() {
    use std::os::unix::fs::PermissionsExt;
    if Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let d = std::env::temp_dir().join(format!("sspur-cli-fallback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("p.ssp"), "fn victim(n: Int) -> Int\n= n * 3 + 1\n\nfn other(n: Int) -> Int\n= victim(n) - 1\n\ntest t = other(2) == 6\n").unwrap();
    let cc = d.join("cc.sh");
    std::fs::write(&cc, "#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in *.c) n=$(grep -n 'f_victim(.*) {$' \"$a\" | head -n1 | cut -d: -f1); if [ -n \"$n\" ] && { [ \"$MODE\" = all ] || echo \" $* \" | grep -q ' -c '; }; then echo \"$a:$n:1: error: injected failure\" >&2; exit 1; fi;; esac; done\nexec clang \"$@\"\n").unwrap();
    std::fs::set_permissions(&cc, std::fs::Permissions::from_mode(0o755)).unwrap();
    let run = |mode: &str, extra: &[&str], strict: bool| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_sspur"));
        c.arg("test").arg(d.join("p.ssp")).args(extra).env("CC", &cc).env("MODE", mode).env("SSPUR_CACHE", d.join(format!("cache-{mode}"))).env_remove("SSPUR_STRICT_NATIVE").env_remove("SSPUR_SPLIT");
        if strict {
            c.env("SSPUR_STRICT_NATIVE", "1");
        }
        let o = c.output().unwrap();
        (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.success())
    };
    let (out, err, ok) = run("split", &[], false);
    assert!(ok && out.contains("1 passed"), "{out}{err}");
    assert_eq!(err.trim(), "warning: per-definition native build failed in fn victim: error: injected failure; using the whole-program build");
    let (_, err, ok) = run("split", &["--quiet"], false);
    assert!(ok && err.is_empty(), "{err}");
    let (out, err, ok) = run("split", &["--strict-native"], false);
    assert!(!ok && !out.contains("passed"), "{out}");
    assert_eq!(err.trim(), "error: per-definition native build failed in fn victim: error: injected failure");
    let (out, err, ok) = run("all", &[], false);
    assert!(ok && out.contains("1 passed"), "{out}{err}");
    assert_eq!(err.trim(), "warning: native build failed in fn victim: error: injected failure; interpreting");
    let (_, err, ok) = run("all", &[], true);
    assert!(!ok);
    assert_eq!(err.trim(), "error: native build failed in fn victim: error: injected failure");
    let _ = std::fs::remove_dir_all(&d);
}

const SHOP: &str = "type Item = {sku: Str, qty: Int, price: Int}
type Shape = Circle{r: Int} | Square{side: Int}

fn total(items: List[Item]) -> Int
= ?total

fn count(xs: List[Str]) -> Int
= ?count

fn first_or(xs: List[Int], d: Int) -> Int
= ?first

fn circle(r: Int) -> Shape
= ?shape

test t1 = total([]) == 0
test t2 = total([Item{sku: \"a\", qty: 2, price: 3}, Item{sku: \"b\", qty: 1, price: 4}]) == 10
test c1 = count([\"a\", \"b\", \"a\"]) == 3
test f1 = first_or([5, 6], 0) == 5
test f2 = first_or([], 7) == 7
test s1 = circle(2) == Circle{r: 2}
";

fn hole_dir(name: &str, src: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sspur-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("h.ssp"), src).unwrap();
    d
}

#[test]
fn fill_synthesizes_classic_holes() {
    let d = hole_dir("fill", SHOP);
    let (out, ok) = sspur(&d, &["fill", "h.ssp", "--fill-ms", "5000"], "");
    assert!(ok, "{out}");
    assert_eq!(
        out,
        "?total: items.map(_.qty * _.price).sum (passes 2 tests, 3 others also fit)\n?count: xs.len (passes 1 test, 21 others also fit)\n?first: xs.first.or(d) (passes 2 tests, 24 others also fit)\n?shape: Circle{r: r} (passes 1 test, 8 others also fit)"
    );
    let (out, ok) = sspur(&d, &["check", "h.ssp", "--fill-ms", "5000"], "");
    assert!(ok, "{out}");
    assert!(out.contains("h.ssp:8:3 hole E_HOLE hole ?count expects type Int\n  ?count: xs.len (passes 1 test, 21 others also fit)\n"), "{out}");
    let (json, _) = sspur(&d, &["check", "h.ssp", "--json", "--fill-ms", "5000"], "");
    let first: serde_json::Value = serde_json::from_str(json.lines().next().unwrap()).unwrap();
    assert_eq!(first["fix"][0], serde_json::json!({"op": "fill", "hole": "?total", "target": "total", "expr": "items.map(_.qty * _.price).sum", "evidence": "passes 2 tests, 3 others also fit", "ok": true}));
    let (out, ok) = sspur(&d, &["fill", "h.ssp", "?first", "--apply", "--fill-ms", "5000"], "");
    assert!(ok && out.ends_with("filled 1 hole in h.ssp"), "{out}");
    assert!(std::fs::read_to_string(d.join("h.ssp")).unwrap().contains("fn first_or(xs: List[Int], d: Int) -> Int\n= xs.first.or(d)\n"));
    let (out, ok) = sspur(&d, &["fill", "h.ssp", "?nope"], "");
    assert!(!ok && out == "no hole ?nope", "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn fill_respects_effects_and_contracts() {
    let src = "fn noisy(x: Int) -> Int ! log
= do
  log(\"x\")
  x + 1

fn pure_bump(x: Int) -> Int
= ?a

fn logged_bump(x: Int) -> Int ! log
= ?b

fn grow(x: Int) -> Int
  post r > x
= ?c

fn twice(x: Int) -> Int
= 2 * ?d

test a1 = pure_bump(1) == 2
test b1 = logged_bump(1) == 2
test d1 = twice(3) == 8
";
    let d = hole_dir("fill_eff", src);
    let (out, ok) = sspur(&d, &["fill", "h.ssp", "--all", "--json", "--fill-ms", "5000"], "");
    assert!(ok, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let exprs = |i: usize| -> Vec<String> { v["holes"][i]["candidates"].as_array().unwrap().iter().map(|c| c["expr"].as_str().unwrap().to_string()).collect() };
    assert!(!exprs(0).iter().any(|e| e.contains("noisy")), "a pure function can't call noisy: {:?}", exprs(0));
    assert_eq!(exprs(0)[0], "x + 1");
    assert_eq!(exprs(1)[0], "noisy(x)", "a function that declares log may");
    assert!(!exprs(2).contains(&"x".to_string()) && exprs(2)[0] == "x + 1", "x breaks the post: {:?}", exprs(2));
    assert!(v["holes"][2]["candidates"][0]["evidence"].as_str().unwrap().starts_with("no tests, post"));
    let (out, ok) = sspur(&d, &["fill", "h.ssp", "?d", "--apply", "--fill-ms", "5000"], "");
    assert!(ok, "{out}");
    assert!(std::fs::read_to_string(d.join("h.ssp")).unwrap().contains("= 2 * (x + 1)\n"), "parenthesized where the bare text would bind differently");
    assert!(sspur(&d, &["test", "--interp", "h.ssp"], "").0.contains("1 passed, 2 failed"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn fill_honors_its_budget() {
    let d = hole_dir("fill_budget", SHOP);
    let t0 = std::time::Instant::now();
    let (out, _) = sspur(&d, &["fill", "h.ssp", "?total", "--json", "--fill-ms", "20"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let ms = v["holes"][0]["ms"].as_f64().unwrap();
    assert!(ms < 60.0, "a 20 ms budget took {ms} ms");
    assert!(t0.elapsed().as_secs_f64() < 5.0);
    let (out, ok) = sspur(&d, &["check", "h.ssp", "--no-fill"], "");
    assert!(ok && !out.contains("  ?total:"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}

/// The hole-filling benchmark, bench/fill/holes.txt: the share of holes whose top candidate is the
/// intended one. `cargo test --release fill_benchmark -- --nocapture` prints the table.
#[test]
fn fill_benchmark() {
    const MIN_TOP1: usize = 25;
    let table = include_str!("../../../bench/fill/holes.txt");
    let (head, cases) = table.split_once("\n## ").unwrap();
    let prelude = head.split_once("prelude:\n").unwrap().1;
    // A generous budget, so the result depends on the search and not on how fast the machine is:
    // CI runners, Windows ones in particular, are several times slower than a laptop.
    let budget = "5000";
    let d = hole_dir("fill_bench", "");
    let (mut top1, mut top3, mut ms, mut lost) = (0, 0, vec![], vec![]);
    let mut rows = vec![];
    let n = cases.split("\n## ").count();
    for case in cases.split("\n## ") {
        let (title, rest) = case.split_once('\n').unwrap();
        let (name, guard) = (title.trim_end_matches(" *"), title.ends_with(" *"));
        let wants: Vec<&str> = rest.lines().filter_map(|l| l.strip_prefix("want: ")).collect();
        let prog: String = rest.lines().filter(|l| !l.starts_with("want: ")).map(|l| format!("{l}\n")).collect();
        std::fs::write(d.join("h.ssp"), format!("{prelude}\n{prog}")).unwrap();
        let (out, _) = sspur(&d, &["fill", "h.ssp", "--all", "--json", "--fill-ms", budget], "");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|_| panic!("{name}: {out}"));
        let hole = &v["holes"][0];
        let got: Vec<String> = hole["candidates"].as_array().map(|a| a.iter().map(|c| c["expr"].as_str().unwrap().to_string()).collect()).unwrap_or_default();
        let hit = |c: &String| wants.contains(&c.as_str());
        let (h1, h3) = (got.first().is_some_and(hit), got.iter().take(3).any(hit));
        top1 += h1 as usize;
        top3 += h3 as usize;
        if guard && !h1 {
            lost.push(name.to_string());
        }
        ms.push(hole["ms"].as_f64().unwrap_or(0.0));
        let ev = hole["candidates"][0]["evidence"].as_str().unwrap_or("").to_string();
        rows.push(format!("{:<24} {:<4} {:>6.0} ms  {} ({ev})", name, if h1 { "top1" } else if h3 { "top3" } else { "miss" }, ms.last().unwrap(), got.join(" | ")));
    }
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("{}", rows.join("\n"));
    println!("top-1 {top1}/{n}, top-3 {top3}/{n}, median {:.0} ms, max {:.0} ms", ms[ms.len() / 2], ms[ms.len() - 1]);
    let _ = std::fs::remove_dir_all(&d);
    assert!(lost.is_empty(), "no longer the top candidate: {lost:?}");
    assert!(top1 >= MIN_TOP1, "top-1 {top1}/{n}, expected at least {MIN_TOP1}");
}

#[test]
fn fill_applies_to_a_codebase() {
    let d = fresh("fill_store");
    let (out, ok) = sspur(&d, &["edit", "-e", "fn three() -> Int\n= ?\n\ntest three_t = three() == 3"], "");
    assert!(ok, "{out}");
    let (out, ok) = sspur(&d, &["fill", "--apply", "--fill-ms", "5000"], "");
    assert!(ok, "{out}");
    assert_eq!(out, "?: 1 + two() (passes 1 test)\nok ~three");
    assert_eq!(sspur(&d, &["q", "body", "three"], "").0, "fn three() -> Int\n= 1 + two()");
}

#[test]
fn suggest_proposes_contracts_from_runs() {
    let src = "fn avg(xs: List[Int]) -> Int
= xs.sum / xs.len

fn clip(xs: List[Int], n: Int) -> List[Int]
= xs.take(n)

fn at(xs: List[Int], i: Int) -> Int
= xs[i]

test a1 = avg([2, 4]) == 3
";
    let d = hole_dir("suggest", src);
    let (out, ok) = sspur(&d, &["suggest", "h.ssp"], "");
    assert!(ok, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "at");
    assert!(lines[1].starts_with("  pre i >= 0  // traps on ") && lines[2].starts_with("  pre i < xs.len  // traps on "), "{out}");
    assert_eq!(lines[3], "avg");
    assert!(lines[4].starts_with("  pre xs.len > 0  // traps on "), "a trap on [] gives the precondition: {out}");
    assert_eq!(lines[5], "clip");
    assert!(lines[6] == "  post r.len <= xs.len  // proved" || lines[6].starts_with("  post r.len <= xs.len  // held on "), "{out}");
    let (json, ok) = sspur(&d, &["suggest", "h.ssp", "avg", "--json"], "");
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["fns"][0]["fix"][0]["contract"], serde_json::json!({"pre": "xs.len > 0"}));
    let (out, ok) = sspur(&d, &["suggest", "h.ssp", "avg", "--apply"], "");
    assert!(ok && out.ends_with("added 1 clause to h.ssp"), "{out}");
    assert!(std::fs::read_to_string(d.join("h.ssp")).unwrap().starts_with("fn avg(xs: List[Int]) -> Int\n  pre xs.len > 0\n= xs.sum / xs.len\n"));
    let (out, ok) = sspur(&d, &["suggest", "h.ssp", "nope"], "");
    assert!(!ok && out == "no function nope", "{out}");
    let _ = std::fs::remove_dir_all(&d);
}
