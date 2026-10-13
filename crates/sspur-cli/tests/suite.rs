use sspur_check::check;
use sspur_eval::Interp;
use sspur_hash::{hash_module_with, Resolution};
use sspur_syntax::parse;
use std::path::Path;

fn big_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().stack_size(1 << 29).spawn(f).unwrap().join().unwrap();
}

#[test]
fn every_suite_program_checks_and_passes_its_tests() {
    big_stack(run_suite);
}

fn run_suite() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/programs");
    let mut programs = 0;
    let mut tests = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "ssp") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let module = parse(&src).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
        let out = check(&module);
        let errors: Vec<_> = out.diags.iter().filter(|d| d.is_error()).collect();
        assert!(errors.is_empty(), "{}: {errors:?}", path.display());

        let res = Resolution { user_methods: Some(&out.user_methods), record_types: Some(&out.record_types) };
        assert_eq!(hash_module_with(&module, &res), hash_module_with(&module, &res));
        let (module, out) = sspur_check::elab::lower(&module, &out).unwrap_or_else(|e| panic!("{}: {e}", path.display())).unwrap_or((module, out));

        let mut interp = Interp::new(&module, out.record_types.clone(), out.user_methods.clone(), out.gen_loops.clone());
        interp.set_check(&out);
        let results = interp.run_tests();
        assert!(!results.is_empty(), "{} has no tests", path.display());
        for (name, r) in &results {
            assert!(r.is_ok(), "{}::{name}: {:?}", path.display(), r);
        }
        programs += 1;
        tests += results.len();
    }
    assert!(programs >= 10, "suite needs at least 10 programs, found {programs}");
    assert!(tests >= 40);
}

#[test]
fn contract_violations_trap_with_context() {
    let src = "type Pos = Int where _ > 0\nfn half(x: Pos) -> Int\n  post r * 2 == x\n= x / 2\ntest odd = half(3) == 1\ntest neg = half(-2) == -1";
    let m = parse(src).unwrap();
    let out = check(&m);
    let results = Interp::new(&m, out.record_types, out.user_methods, out.gen_loops).run_tests();
    assert!(results[0].1.as_ref().unwrap_err().contains("post r * 2 == x"));
    assert!(results[1].1.as_ref().unwrap_err().contains("type Pos"));
}

#[test]
fn integer_overflow_traps_instead_of_wrapping() {
    let src = "fn big() -> Int\n= 9223372036854775807 + 1\ntest t = big() == 0";
    let m = parse(src).unwrap();
    let out = check(&m);
    let results = Interp::new(&m, out.record_types, out.user_methods, out.gen_loops).run_tests();
    assert_eq!(results[0].1.as_ref().unwrap_err(), "integer overflow");
}

#[test]
fn native_release_matches_interpreter_on_every_suite_program() {
    if std::process::Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    big_stack(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/programs");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "ssp") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            let module = parse(&src).unwrap();
            let out = check(&module);
            let (module, out) = sspur_check::elab::lower(&module, &out).unwrap_or_else(|e| panic!("{}: {e}", path.display())).unwrap_or((module, out));
            let mut first = Interp::new(&module, out.record_types.clone(), out.user_methods.clone(), out.gen_loops.clone());
            first.set_check(&out);
            let interp = first.run_tests();
            let mut native = Interp::new(&module, out.record_types.clone(), out.user_methods.clone(), out.gen_loops.clone());
            native.set_check(&out);
            let compiled = sspur_native::cgen::compile_release(&module, &out, "-O2").unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(compiled.fallback.is_none(), "{}: per-definition build failed {:?}", path.display(), compiled.fallback);
            let effectful = module.defs.iter().any(|d| matches!(d, sspur_syntax::Def::Effect(_))) || src.contains("yield");
            assert!(compiled.skipped.is_empty() || effectful, "{}: not native: {:?}", path.display(), compiled.skipped);
            native.set_native(compiled);
            assert_eq!(interp, native.run_tests(), "{}", path.display());
        }
    });
}

#[test]
fn verify_reports_proofs_counterexamples_and_unknowns() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/programs/contracts.ssp");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_sspur")).arg("verify").arg(&path).arg("--json").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let status = |f: &str, prefix: &str| v["clauses"].as_array().unwrap().iter().find(|c| c["fn"] == f && c["clause"].as_str().unwrap().starts_with(prefix)).map(|c| c["status"].as_str().unwrap().to_string()).unwrap();
    if v["z3"] == false {
        assert_eq!(status("clamp", "post"), "unknown");
        assert!(out.status.success());
        return;
    }
    assert_eq!((v["proved"].as_u64(), v["counterexample"].as_u64(), v["unknown"].as_u64()), (Some(19), Some(1), Some(1)), "{v}");
    assert_eq!(status("search", "pre"), "proved");
    assert_eq!(status("midpoint", "post"), "proved");
    assert_eq!(status("mean2", "post"), "counterexample");
    assert_eq!(status("total_len", "post"), "unknown");
    assert!(!out.status.success());
}

#[test]
fn parallel_pipelines_report_the_sequential_trap_at_any_thread_count() {
    if std::process::Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let base = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/programs/concurrency.ssp")).unwrap();
    let extra = "
fn combo(i: Int, a: Int, b: Int) -> Int
= if i == a or i == a + 1 then 9000000000000000000 else if i == b then 1 / (i - i) else 1

fn combo_sum(n: Int, a: Int, b: Int) -> Int
= (0..n).map(i => combo(i, a, b)).sum

fn refine_at(n: Int, at: Int) -> Int ! div
= (0..n).map(i => collatz_len(at - i)).sum

test excursion = spiky_sum(2000000, 10, 1500000) == 0
test div_first = first_fault(2000000, 1200000, 1800000) == 0
test ovf_first = first_fault(2000000, 1800000, 1200000) == 0
test deep = deep_at(200000, 150000) == 0
test refine = refine_at(30000, 20000) == 0
test deferred = combo_sum(2000000, 1500000, 1800000) == 0
test deferred_ovf = combo_sum(2000000, 1500000, -1) == 0
test big_ok = prime_count(2000000) == 148933 and first_fault(2000000, -1, -1) == 1999999000000
";
    let path = std::env::temp_dir().join(format!("sspur_parallel_traps_{}.ssp", std::process::id()));
    std::fs::write(&path, format!("{base}\n{extra}")).unwrap();
    let expected = [
        "FAIL  excursion: integer overflow",
        "FAIL  div_first: division by zero",
        "FAIL  ovf_first: integer overflow",
        "FAIL  deep: stack overflow in down",
        "FAIL  refine: contract violated: parameter 'n' of collatz_len where _ > 0 (value = 0)",
        "FAIL  deferred: division by zero",
        "FAIL  deferred_ovf: integer overflow",
    ];
    let mut outputs = Vec::new();
    for threads in [None, Some("1"), Some("3")] {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_sspur"));
        cmd.arg("test").arg(&path);
        if let Some(t) = threads {
            cmd.env("SSPUR_THREADS", t);
        }
        let out = String::from_utf8(cmd.output().unwrap().stdout).unwrap();
        for line in expected {
            assert!(out.lines().any(|l| l == line), "threads {threads:?}: missing {line:?} in\n{out}");
        }
        assert!(!out.contains("big_ok"), "threads {threads:?}: big_ok failed in\n{out}");
        outputs.push(out);
    }
    std::fs::remove_file(&path).ok();
    assert!(outputs.iter().all(|o| *o == outputs[0]));
}

#[test]
fn concurrency_failures_are_identical_in_both_tiers() {
    if std::process::Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let src = "type E = Boom{n: Int}
fn waiter(c: Chan[Int]) -> Int ! conc
= c.recv.or(-1)
fn dead() -> Int ! conc
= do
  c = chan()
  (a, b) = par(waiter(c), waiter(c))
  a + b
fn failing(n: Int) -> Int ! fail[E]
= if n > 2 then raise Boom{n} else n
fn fails() -> Int ! fail[E]
= do
  (a, b, c) = par(failing(1), failing(5), failing(3))
  a + b + c
fn trapper(c: Chan[Int]) -> Int ! conc
= do
  c.send(1)
  1 / 0
fn mixed() -> Int ! conc
= do
  c = chan()
  (a, b) = par(waiter(c) + waiter(c), trapper(c))
  a + b
fn closed() -> Unit ! conc
= do
  c = chan()
  c.close
  c.send(1)
test t_dead = dead() == 0
test t_fail = catch fails() == 0
  | Boom{n} => n == 5
test t_mixed = mixed() == 0
test t_closed = closed() == ()
";
    let path = std::env::temp_dir().join(format!("sspur_conc_fail_{}.ssp", std::process::id()));
    std::fs::write(&path, src).unwrap();
    let expected = ["FAIL  t_dead: deadlock: every task is blocked on recv", "FAIL  t_mixed: division by zero", "FAIL  t_closed: send on a closed channel", "1 passed, 3 failed"];
    let mut outputs = Vec::new();
    for mode in ["--interp", "--release"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_sspur")).arg("test").arg(mode).arg(&path).output().unwrap();
        let out = String::from_utf8(out.stdout).unwrap();
        for line in expected {
            assert!(out.lines().any(|l| l == line), "{mode}: missing {line:?} in\n{out}");
        }
        outputs.push(out);
    }
    std::fs::remove_file(&path).ok();
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn fused_folds_fail_like_the_unfused_order() {
    if std::process::Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let src = "type E = Neg{at: Int}
type S = {n: Int, sum: Int}
fn half(x: Int) -> Int
  pre x != 7
= x / 2
fn step(s: S, x: Int) -> S
= {n: s.n + 1, sum: s.sum + 4611686018427387904 / (x + 1) + half(x)}
fn row(x: Int) -> Int ! fail[E]
= if x < 0 then raise Neg{at: x} else if x == 1000 then 1 / (x - x) else x
fn run(xs: List[Int]) -> Int ! fail[E]
= xs.map(row).fold({n: 0, sum: 0}, step).n
fn noisy(x: Int) -> Int ! log
= do
  log(\"row {x}\")
  x
fn loud(xs: List[Int]) -> Int ! log
= xs.map(noisy).fold(0, (a, x) => a + 4611686018427387904 / (x + 1))
fn picky(xs: List[Int]) -> Int
= xs.filter(x => 100 / x > 0).fold(0, (a, x) => a + 4611686018427387904 / (x + 1))
test g_traps = run([0, 0, 0, 5]) == 0
test f_traps_after_g = run([0, 0, 0, 5, 1000]) == 0
test f_raises_after_g = catch run([0, 0, 0, 5, -3]) == 0
  | Neg{at} => at == -3
test pre_in_g = run((0..200).map(i => if i == 150 then 7 else 2000)) == 0
test logs_then_trap = loud([0, 1, 0, 2]) == 0
test filter_traps_after_g = picky([1, 1, 1, 0]) == 0
test filter_g_traps = picky((0..300).map(i => if i < 200 then 50 else 1)) == 0
test ok = run((0..500).map(i => i + 1001)) == 500
";
    let path = std::env::temp_dir().join(format!("sspur_fused_fold_{}.ssp", std::process::id()));
    std::fs::write(&path, src).unwrap();
    let expected = [
        "FAIL  g_traps: integer overflow",
        "FAIL  f_traps_after_g: division by zero",
        "FAIL  logs_then_trap: integer overflow",
        "FAIL  filter_traps_after_g: division by zero",
        "FAIL  filter_g_traps: integer overflow",
        "2 passed, 6 failed",
    ];
    let mut outputs = Vec::new();
    for mode in ["--interp", "--release"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_sspur")).arg("test").arg(mode).arg(&path).output().unwrap();
        let out = String::from_utf8(out.stdout).unwrap();
        for line in expected {
            assert!(out.lines().any(|l| l == line), "{mode}: missing {line:?} in\n{out}");
        }
        assert!(out.contains("pre x != 7"), "{mode}: {out}");
        outputs.push(out);
    }
    std::fs::remove_file(&path).ok();
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn vectorized_pipelines_trap_exactly_like_the_interpreter() {
    if std::process::Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let base = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/programs/pipelines.ssp")).unwrap();
    let extra = "
fn mixed_fault(xs: List[Int], n: Int, a: Int) -> Int
= (0..n).map(i => xs[i] * 0 + (if i == a then 9223372036854775807 else 1)).sum

test add_63 = bump(200, 63, 9223372036854775807) == 0
test add_64 = bump(200, 64, 9223372036854775807) == 0
test sum_127 = ramp_sum(200, 126, 4611686018427387904) == 0
test sum_128 = spike_sum(200, 128, 9223372036854775807) == 0
test neg_64 = negs(200, 64) == 0
test mul_65 = tripled(200, 65, 3074457345618258603) == 0
test mul_neg = tripled(200, 65, -3074457345618258603) == 0
test sq_big = sq_sum([5, 3037000500]) == 0
test idx_far = idx_sum(ints(100), 130) == 0
test ovf_before_idx = mixed_fault(ints(100), 200, 70) == 0
test idx_before_ovf = mixed_fault(ints(100), 200, 150) == 0
test big_add = bump(2000000, 1500001, 9223372036854775807) == 0
test big_sum = spike_sum(2000000, 1999999, 9223372036854775807) == 0
test big_masked = masked(2000000, 1234567) == 2000001000000 - 1234568
test big_affine = affine_pos(ints(2000000)) == ints(2000000).map(x => 3 * x + 1).filter(y => y > 0).sum
";
    let path = std::env::temp_dir().join(format!("sspur_simd_traps_{}.ssp", std::process::id()));
    std::fs::write(&path, format!("{base}\n{extra}")).unwrap();
    let run = |interp: bool, threads: Option<&str>| {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_sspur"));
        cmd.arg("test").arg(&path);
        if interp {
            cmd.arg("--interp");
        }
        if let Some(t) = threads {
            cmd.env("SSPUR_THREADS", t);
        }
        String::from_utf8(cmd.output().unwrap().stdout).unwrap()
    };
    let expected = run(true, None);
    assert_eq!(expected.lines().filter(|l| l.starts_with("FAIL")).count(), 13, "{expected}");
    for threads in [None, Some("1"), Some("3")] {
        assert_eq!(run(false, threads), expected, "threads {threads:?}");
    }
    std::fs::remove_file(&path).ok();
}

#[test]
fn std_library_traps_are_identical_in_both_tiers() {
    if std::process::Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let src = "type E = Bad{why: Str}
fn chunk(n: Int) -> Int
= [1, 2, 3].chunks(n).len
fn window(n: Int) -> Int
= [1, 2, 3].windows(n).len
fn stepped(k: Int) -> Int
= range(0, 10, k).len
fn huge() -> Int
= range(0, 9223372036854775807, 1).len
fn dice(lo: Int, hi: Int) -> Int
= rand_int(1, lo, hi).0
fn fixed(d: Int) -> Str
= 1.5.fmt(d)
fn g(a: Int, b: Int) -> Int
= a.gcd(b)
fn l(a: Int, b: Int) -> Int
= a.lcm(b)
fn unwrap(s: Str) -> Int ! fail[Str]
= json.decode[Int](s).get
fn first_bad(xs: List[Int]) -> List[Int]
= xs.sort_with((a, b) => a / b)
test t_chunk = chunk(0) == 0
test t_window = window(-1) == 0
test t_step = stepped(0) == 0
test t_huge = huge() == 0
test t_dice = dice(3, 3) == 0
test t_fmt = fixed(21) == \"\"
test t_gcd = g(-9223372036854775807 - 1, 0) == 0
test t_lcm = l(9223372036854775807, 2) == 0
test t_raise = catch unwrap(\"x\") == 0
  | e => e == \"invalid JSON\"
test t_cmp = first_bad([3, 0, 1]) == []
test t_ok = chunk(2) == 2 and g(12, 18) == 6 and unwrap(\"5\") == 5
";
    let path = std::env::temp_dir().join(format!("sspur_std_traps_{}.ssp", std::process::id()));
    std::fs::write(&path, src).unwrap();
    let expected = [
        "FAIL  t_chunk: chunk size must be > 0",
        "FAIL  t_window: window size must be > 0",
        "FAIL  t_step: range step must not be 0",
        "FAIL  t_huge: out of memory",
        "FAIL  t_dice: rand_int needs lo < hi",
        "FAIL  t_fmt: fmt digits must be in 0..=20",
        "FAIL  t_gcd: integer overflow",
        "FAIL  t_lcm: integer overflow",
        "FAIL  t_cmp: division by zero",
        "2 passed, 9 failed",
    ];
    let mut outputs = Vec::new();
    for mode in ["--interp", "--release"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_sspur")).arg("test").arg(mode).arg(&path).output().unwrap();
        let out = String::from_utf8(out.stdout).unwrap();
        for line in expected {
            assert!(out.lines().any(|l| l == line), "{mode}: missing {line:?} in\n{out}");
        }
        outputs.push(out);
    }
    let native = std::process::Command::new(env!("CARGO_BIN_EXE_sspur")).arg("native").arg("--release").arg(&path).output().unwrap();
    let native = String::from_utf8(native.stdout).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(!native.contains("interp "), "{native}");
    assert_eq!(outputs[0], outputs[1]);
}

fn assert_trap_parity(tag: &str, src: &str, expected: &[&str]) {
    let path = std::env::temp_dir().join(format!("sspur_{tag}_{}.ssp", std::process::id()));
    // @SELF@ names a file that opens on every platform (Windows cannot open a directory as a file).
    std::fs::write(&path, src.replace("@SELF@", &path.display().to_string().replace('\\', "/"))).unwrap();
    let mut outputs = Vec::new();
    for mode in ["--interp", "--release"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_sspur")).arg("test").arg(mode).arg(&path).output().unwrap();
        let out = String::from_utf8(out.stdout).unwrap();
        for line in expected {
            assert!(out.lines().any(|l| l == *line), "{mode}: missing {line:?} in\n{out}");
        }
        outputs.push(out);
    }
    let native = std::process::Command::new(env!("CARGO_BIN_EXE_sspur")).arg("native").arg("--release").arg(&path).output().unwrap();
    let native = String::from_utf8(native.stdout).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(!native.contains("interp "), "{native}");
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn std_extras_traps_are_identical_in_both_tiers() {
    if std::process::Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let src = "fn spec(s: Str) -> Str
= 5.format(s)
fn bit(i: Int) -> Bool
= bits(4).has(i)
fn sizes() -> Int
= bits(4).union(bits(5)).count
fn tfmt(p: Str) -> Str
= time_ms(0).format(p)
fn scale(k: Int) -> Str
= decimal(\"1\").get.round(k).str
fn bdiv(n: Int) -> Str
= big(1).div(big(n)).str
fn bpow(e: Int) -> Str
= big(2).pow(e).str
fn dur(n: Int) -> Int
= days(n).ms
fn neg_bits(n: Int) -> Int
= bits(n).len
fn step(k: Int) -> Int
= do
  var s = 0
  for i in range(0, 5, k)
    s := s + i
  s
fn fused(k: Int) -> Int
= range(0, 5, k).map(x => x * 2).sum
test t_spec = spec(\"q\") == \"\"
test t_bit = bit(4)
test t_sizes = sizes() == 0
test t_tfmt = tfmt(\"%Q\") == \"\"
test t_scale = scale(-1) == \"\"
test t_bdiv = bdiv(0) == \"\"
test t_bpow = bpow(-1) == \"\"
test t_dur = dur(9223372036854775807) == 0
test t_neg = neg_bits(-1) == 0
test t_step = step(0) == 0
test t_fused = fused(0) == 0
test t_ok = spec(\"03\") == \"005\" and not bit(3) and step(2) == 6 and fused(2) == 12
";
    let expected = [
        "FAIL  t_spec: bad format spec 'q'",
        "FAIL  t_bit: bit index 4 out of range for 4 bits",
        "FAIL  t_sizes: bit sets differ in size (4 and 5)",
        "FAIL  t_tfmt: bad time format '%Q'",
        "FAIL  t_scale: decimal scale must be in 0..=10000",
        "FAIL  t_bdiv: division by zero",
        "FAIL  t_bpow: negative exponent",
        "FAIL  t_dur: integer overflow",
        "FAIL  t_neg: bits size must be >= 0",
        "FAIL  t_step: range step must not be 0",
        "FAIL  t_fused: range step must not be 0",
        "1 passed, 11 failed",
    ];
    assert_trap_parity("std_extras", src, &expected);
}

#[test]
fn exit_sets_the_status_in_both_tiers() {
    if std::process::Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let path = std::env::temp_dir().join(format!("sspur_exit_{}.ssp", std::process::id()));
    std::fs::write(&path, "fn main() -> Unit ! log, proc\n= do\n  log(\"bye\")\n  exit(3)\n  log(\"never\")\n").unwrap();
    for mode in ["--interp", "--native"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_sspur")).arg("run").arg(mode).arg(&path).output().unwrap();
        assert_eq!(out.status.code(), Some(3), "{mode}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "bye\n", "{mode}");
    }
    std::fs::remove_file(&path).ok();
}

#[test]
fn std_round3_traps_are_identical_in_both_tiers() {
    if std::process::Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let src = "fn gam(k: F64) -> F64
= rand_gamma(1, k, 1.0).0
fn bin(n: Int) -> Int
= rng(1).binomial(n, 0.5).0
fn poi(m: F64) -> Int
= rand_poisson(1, m).0
fn geo(p: F64) -> Int
= rand_geometric(1, p).0
fn wpick(ws: List[F64]) -> Int
= rng(2).weighted(ws).0
test t_gam = gam(0.0) == 0.0
test t_bin = bin(-1) == 0
test t_poi = poi(-1.0) == 0
test t_geo = geo(0.0) == 0
test t_wpick = wpick([0.0, 0.0]) == 0
fn md(n: Int) -> Int
= mdspan([1, 2, 3, 4], [2, n]).size
fn mdi(i: Int) -> Int
= mdspan([1, 2, 3, 4], [2, 2]).get([1, i])
fn mdc() -> Int
= mdspan([1, 2, 3, 4], [2, 2]).get([1])
fn mds(d: Int) -> Int
= mdspan([1, 2, 3, 4], [2, 2]).slice(d, 0, 1).size
fn mdr(h: Int) -> Int
= mdspan([1, 2, 3, 4], [2, 2]).slice(1, 1, h).size
fn mdbad(o: Int) -> Int
= json.decode[MdSpan[Int]](\"\\{\\\"data\\\": [1], \\\"offset\\\": {o}, \\\"shape\\\": [1], \\\"strides\\\": [1]}\").or(mdspan([0], [1])).get([0])
fn mdneg() -> Int
= mdspan([1].take(0), [0, -1]).size
test t_md = md(3) == 0
test t_mdi = mdi(2) == 0
test t_mdc = mdc() == 0
test t_mds = mds(2) == 0
test t_mdr = mdr(3) == 0
test t_mdbad = mdbad(5) == 0
test t_mdneg = mdneg() == 0
fn fz(m: Int) -> Str
= fixed_zone(m).name
fn zfmt(p: Str) -> Str
= time_ms(0).format_in(fixed_zone(60), p)
fn zlocal(t: Int) -> Int
= fixed_zone(60).local(time_ms(t)).unix_ms
fn fmode(m: Str) -> Bool ! fs
= open_file(\"/\", m).is_ok
fn fneg(n: Int) -> Bool ! fs
= with_file(\"@SELF@\", \"r\", f => f.read(n).is_ok).or(false)
test t_fmode = fmode(\"rw\")
test t_fneg = fneg(-1)
fn vsum(n: Int) -> Int
= iota(n, n + 3).map(x => x * 2).sum
fn vrun(n: Int) -> Int
= do
  v = [n, 0, 1].view.map(x => 10 / x)
  v.len
test t_vsum = vsum(4611686018427387904) == 0
test t_vrun = vrun(5) == 0
test t_fz = fz(1440) == \"\"
test t_zfmt = zfmt(\"%Z %Q\") == \"\"
test t_zlocal = zlocal(9223372036854775807) == 0
test t_ok = gam(2.0) > 0.0 and bin(10) <= 10 and wpick([0.0, 1.0]) == 1 and mdbad(0) == 1 and mdi(1) == 4 and fz(90) == \"+01:30\"
";
    let expected = [
        "FAIL  t_gam: rand_gamma needs a finite shape > 0 and scale > 0",
        "FAIL  t_bin: rand_binomial needs n >= 0 and 0 <= p <= 1",
        "FAIL  t_poi: rand_poisson needs 0 <= mean <= 4e15",
        "FAIL  t_geo: rand_geometric needs 0 < p <= 1",
        "FAIL  t_wpick: rand_weighted needs finite weights >= 0 with a positive sum",
        "FAIL  t_md: mdspan shape needs 6 elements, got 4",
        "FAIL  t_mdi: mdspan index 2 out of range for extent 2",
        "FAIL  t_mdc: mdspan index needs 2 coordinates, got 1",
        "FAIL  t_mds: mdspan dimension 2 out of range for rank 2",
        "FAIL  t_mdr: mdspan slice 1..3 out of range for extent 2",
        "FAIL  t_mdbad: malformed mdspan",
        "FAIL  t_mdneg: mdspan extents must be >= 0",
        "FAIL  t_fmode: bad file mode 'rw'",
        "FAIL  t_fneg: read size must be >= 0",
        "FAIL  t_vsum: integer overflow",
        "FAIL  t_vrun: division by zero",
        "FAIL  t_fz: fixed_zone needs -1440 < minutes < 1440",
        "FAIL  t_zfmt: bad time format '%Z %Q'",
        "FAIL  t_zlocal: integer overflow",
        "1 passed, 19 failed",
    ];
    assert_trap_parity("std_round3", src, &expected);
}

#[test]
fn std_round4_traps_are_identical_in_both_tiers() {
    if std::process::Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let src = "fn rz(d: Int) -> Ratio
= ratio(1, d)
fn rdiv(n: Int) -> Ratio
= ratio(1, 2).div(ratio(n, 3))
fn rinv(n: Int) -> Ratio
= ratio(n, 5).inv
fn rpow(e: Int) -> Ratio
= ratio(0, 1).pow(e)
fn rbig(e: Int) -> Ratio
= ratio(3, 2).pow(e)
fn rdec(k: Int) -> Str
= ratio(1, 3).to_dec(k).str
fn lf(d: Int) -> Str
= locale(\"de-DE\").get.format_f64(1.5, d)
test t_rz = rz(0) == ratio(1, 1)
test t_rdiv = rdiv(0) == ratio(1, 1)
test t_rinv = rinv(0) == ratio(1, 1)
test t_rpow = rpow(-2) == ratio(1, 1)
test t_rbig = rbig(9223372036854775807) == ratio(1, 1)
test t_rdec = rdec(10001) == \"\"
test t_lf = lf(21) == \"\"
test t_ok = rz(-4) == ratio(-1, 4) and rpow(0) == ratio(1, 1) and lf(1) == \"1,5\"
";
    let expected = [
        "FAIL  t_rz: division by zero",
        "FAIL  t_rdiv: division by zero",
        "FAIL  t_rinv: division by zero",
        "FAIL  t_rpow: division by zero",
        "FAIL  t_rbig: out of memory",
        "FAIL  t_rdec: decimal scale must be in 0..=10000",
        "FAIL  t_lf: fmt digits must be in 0..=20",
        "1 passed, 7 failed",
    ];
    assert_trap_parity("std_round4", src, &expected);
}
