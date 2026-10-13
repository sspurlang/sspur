use sspur_native::compile;
use sspur_syntax::parse;

fn run(src: &str, f: &str, args: &[i64]) -> Result<i64, String> {
    let m = parse(src).unwrap();
    let jit = compile(&m).unwrap();
    assert!(jit.functions.iter().any(|n| n == f), "{f} not native: {:?}", jit.skipped);
    let a = jit.call(f, args).unwrap();
    let check = sspur_check::check(&m);
    let release = sspur_native::cgen::compile_release(&m, &check, "-O2").unwrap();
    let b = release.call(f, args).unwrap();
    assert_eq!(a, b, "Cranelift and release tiers disagree on {f}{args:?}");
    a
}

#[test]
fn arithmetic_traps_match_the_interpreter() {
    assert_eq!(run("fn f(a: Int, b: Int) -> Int\n= a + b", "f", &[i64::MAX, 1]), Err("integer overflow".into()));
    assert_eq!(run("fn f(a: Int, b: Int) -> Int\n= a * b", "f", &[i64::MAX / 2, 3]), Err("integer overflow".into()));
    assert_eq!(run("fn f(a: Int) -> Int\n= a * 3", "f", &[i64::MAX / 2]), Err("integer overflow".into()));
    assert_eq!(run("fn f(a: Int, b: Int) -> Int\n= a / b", "f", &[1, 0]), Err("division by zero".into()));
    assert_eq!(run("fn f(a: Int, b: Int) -> Int\n= a % b", "f", &[i64::MIN, -1]), Err("integer overflow".into()));
    assert_eq!(run("fn f(a: Int) -> Int\n= -a", "f", &[i64::MIN]), Err("integer overflow".into()));
    assert_eq!(run("fn f(a: Int, b: Int) -> Int\n= a ** b", "f", &[2, -1]), Err("negative exponent".into()));
    assert_eq!(run("fn f(a: Int, b: Int) -> Int\n= a ** b", "f", &[3, 4]), Ok(81));
}

#[test]
fn contract_traps_match_the_interpreter() {
    let src = "fn half(x: Int where _ >= 0) -> Int\n  pre x < 100\n  post r * 2 == x\n= x / 2";
    assert_eq!(run(src, "half", &[4]), Ok(2));
    assert_eq!(run(src, "half", &[3]), Err("contract violated: post r * 2 == x in half (r = 1)".into()));
    assert_eq!(run(src, "half", &[200]), Err("contract violated: pre x < 100 in half (x = 200)".into()));
    assert_eq!(run(src, "half", &[-2]), Err("contract violated: parameter 'x' of half where _ >= 0 (value = -2)".into()));
}

#[test]
fn traps_propagate_through_calls_and_recursion_is_bounded() {
    let src = "fn inner(x: Int) -> Int\n= x / (x - 5)\nfn outer(x: Int) -> Int\n= inner(x) + 1\nfn down(n: Int) -> Int\n= down(n + 1)";
    assert_eq!(run(src, "outer", &[5]), Err("division by zero".into()));
    assert_eq!(run(src, "outer", &[6]), Ok(7));
    assert_eq!(run(src, "down", &[0]), Err("stack overflow in down".into()));
}

#[test]
fn control_flow() {
    let src = "fn f(n: Int) -> Int ! div\n= do\n  var s = 0\n  var i = 0\n  while i < n\n    if i % 3 == 0 then s := s + i\n    i := i + 1\n  for j in 0..n\n    s := s - 1\n  s\nfn g(a: Bool, b: Bool) -> Bool\n= a and not b or not a and b\nfn early(x: Int) -> Int\n= do\n  if x > 10 then return 99\n  x * 2";
    assert_eq!(run(src, "f", &[10]), Ok(3 + 6 + 9 - 10));
    assert_eq!(run(src, "g", &[1, 0]), Ok(1));
    assert_eq!(run(src, "g", &[1, 1]), Ok(0));
    assert_eq!(run(src, "early", &[11]), Ok(99));
    assert_eq!(run(src, "early", &[4]), Ok(8));
}

#[test]
fn ineligible_functions_are_reported() {
    let m = parse("fn s(x: Str) -> Int\n= x.len\nfn uses(x: Int) -> Int\n= s(\"a\") + x").unwrap();
    let c = compile(&m).unwrap();
    assert!(c.functions.is_empty());
    assert!(c.skipped["uses"].contains("not native"));
}

#[test]
fn proven_check_removal_keeps_real_traps() {
    let src = "fn lt(a: Int) -> Int\n= if a < 9223372036854775807 then a + 1 else 0\nfn le(a: Int) -> Int\n= if a <= 9223372036854775807 then a + 1 else 0\nfn dv(a: Int, b: Int) -> Int\n= if b == -1 then a / b else if b == 0 then 0 else a % b\nfn ng(a: Int) -> Int\n= if a > -5 then -a else 0\nfn m(a: Int) -> Int\n  pre a >= 0\n= a - 1\nfn c(n: Int) -> Int\n= if n > 0 then m(n - 1) else m(n)\nfn g(a: Int, b: Int) -> Int\n  pre a >= 0 and b >= 0\n= if b == 0 then a else g(b, a % b)";
    assert_eq!(run(src, "lt", &[i64::MAX]), Ok(0));
    assert_eq!(run(src, "le", &[i64::MAX]), Err("integer overflow".into()));
    assert_eq!(run(src, "dv", &[i64::MIN, -1]), Err("integer overflow".into()));
    assert_eq!(run(src, "dv", &[7, 0]), Ok(0));
    assert_eq!(run(src, "ng", &[i64::MIN]), Ok(0));
    assert_eq!(run(src, "c", &[3]), Ok(1));
    assert_eq!(run(src, "c", &[-3]), Err("contract violated: pre a >= 0 in m (a = -3)".into()));
    assert_eq!(run(src, "g", &[84, 36]), Ok(12));
}

#[test]
fn proven_signs_and_selects_keep_exact_results_and_traps() {
    let src = "fn step(x: Int) -> Int\n= if x % 2 == 0 then x / 2 else 3 * x + 1\nfn len(n: Int where _ > 0) -> Int ! div\n= do\n  var x = n\n  var s = 0\n  while x != 1\n    x := if x % 2 == 0 then x / 2 else 3 * x + 1\n    s := s + 1\n  s\nfn q(a: Int) -> Int\n= a / 4 + a % 4 * 10 + a % 3 * 100 + a / 3 * 1000\nfn pq(a: Int where _ >= 0) -> Int\n= a / 4 + a % 4 * 10 + a % 3 * 100\nfn sel(a: Int, b: Int) -> Int\n= if a > b then a - b else b * 2 - a";
    assert_eq!(run(src, "step", &[i64::MAX]), Err("integer overflow".into()));
    assert_eq!(run(src, "step", &[i64::MAX - 1]), Ok(i64::MAX / 2));
    assert_eq!(run(src, "step", &[i64::MAX / 3 + 1]), Err("integer overflow".into()));
    assert_eq!(run(src, "step", &[i64::MAX / 3 - 1]), Ok(3 * (i64::MAX / 3 - 1) + 1));
    assert_eq!(run(src, "step", &[i64::MIN]), Ok(i64::MIN / 2));
    assert_eq!(run(src, "step", &[i64::MIN + 1]), Err("integer overflow".into()));
    assert_eq!(run(src, "step", &[-7]), Ok(-20));
    assert_eq!(run(src, "step", &[-6]), Ok(-3));
    assert_eq!(run(src, "len", &[27]), Ok(111));
    assert_eq!(run(src, "len", &[1]), Ok(0));
    assert_eq!(run(src, "len", &[i64::MAX / 3 + 1]), Err("integer overflow".into()));
    for a in [-9, -8, -5, -1, 0, 1, 5, 7, i64::MIN, i64::MAX] {
        let want = (a / 3).checked_mul(1000).map(|t| a / 4 + (a % 4) * 10 + (a % 3) * 100 + t).ok_or_else(|| "integer overflow".to_string());
        assert_eq!(run(src, "q", &[a]), want, "q({a})");
        if a >= 0 {
            assert_eq!(run(src, "pq", &[a]), Ok(a / 4 + (a % 4) * 10 + (a % 3) * 100), "pq({a})");
        }
    }
    assert_eq!(run(src, "sel", &[i64::MAX, -1]), Err("integer overflow".into()));
    assert_eq!(run(src, "sel", &[-1, i64::MAX]), Err("integer overflow".into()));
    assert_eq!(run(src, "sel", &[5, 9]), Ok(13));
    assert_eq!(run(src, "sel", &[9, 5]), Ok(4));
}

fn release(src: &str, f: &str, args: &[i64]) -> Result<i64, String> {
    let m = parse(src).unwrap();
    let check = sspur_check::check(&m);
    let r = sspur_native::cgen::compile_release(&m, &check, "-O2").unwrap();
    assert!(r.functions.iter().any(|n| n == f), "{f} not native: {:?}", r.skipped);
    r.call(f, args).unwrap()
}

#[test]
fn smt_proofs_remove_checks_but_keep_real_traps() {
    let src = "fn clamp(x: Int, lo: Int, hi: Int) -> Int\n  pre lo <= hi\n  post r >= lo and r <= hi\n= if x < lo then lo else if x > hi then hi else x\nfn pct(x: Int) -> Int\n= clamp(x, 0, 100) * 3 + 1\nfn w(x: Int) -> Int\n= clamp(x, 5, x)\nfn sub(a: Int, b: Int) -> Int\n  pre a <= b\n= b - a\nfn bad(a: Int, b: Int) -> Int\n  post r >= a\n= a - b\nfn gap(a: Int, b: Int) -> Int\n  pre a >= 0 and a <= b\n  post r >= 0\n= b - a\nfn twice(a: Int, b: Int) -> Int\n= if a >= 0 and a <= b then gap(a, b) + gap(a, b) else 0";
    assert_eq!(release(src, "pct", &[i64::MAX]), Ok(301));
    assert_eq!(release(src, "pct", &[i64::MIN]), Ok(1));
    assert_eq!(release(src, "w", &[3]), Err("contract violated: pre lo <= hi in clamp (lo = 5, hi = 3)".into()));
    assert_eq!(release(src, "w", &[9]), Ok(9));
    assert_eq!(release(src, "sub", &[i64::MIN, 0]), Err("integer overflow".into()));
    assert_eq!(release(src, "bad", &[1, 1]), Err("contract violated: post r >= a in bad (r = 0)".into()));
    assert_eq!(release(src, "twice", &[0, i64::MAX]), Err("integer overflow".into()));
    assert_eq!(release(src, "twice", &[2, 9]), Ok(14));
    let h = "type E = Bad\nfn h(x: Int) -> Int\n  post r > 0\n= do\n  y = catch (if x < 0 then return 0 - 1 else if x == 0 then raise Bad else x)\n    | Bad => 0\n  5 + y * 0";
    assert_eq!(release(h, "h", &[-1]), Err("contract violated: post r > 0 in h (r = -1)".into()));
    assert_eq!(release(h, "h", &[3]), Ok(5));
    if !sspur_smt::Solver::new().available() {
        return;
    }
    let m = parse(src).unwrap();
    let c = sspur_native::cgen::c_source(&m, &sspur_check::check(&m));
    let body = |name: &str| {
        let head = format!("static RR_I f_{name}(int64_t a0");
        let start = c.lines().position(|l| l.starts_with(&head) && l.ends_with('{')).unwrap();
        c.lines().skip(start).take_while(|l| *l != "}").collect::<Vec<_>>().join("\n")
    };
    assert!(body("pct").contains("f_clamp__np("), "{}", body("pct"));
    assert!(!body("pct").contains("overflow"));
    assert!(!body("clamp__np").contains("TRAPV(4"));
    assert!(body("bad").contains("TRAPV(4"));
    assert!(body("w").contains("f_clamp(") && !body("w").contains("f_clamp__np("));
    assert!(body("sub__np").contains("overflow"));
    assert!(body("twice").contains("f_gap__np(") && body("twice").contains("overflow"));
}

#[test]
fn block_lambda_pipelines_fuse_like_one_line_lambdas() {
    let src = "fn blk(n: Int) -> Int\n= (0..n).map(x => do\n  y = x * 3\n  z = y - 1\n  if z > 4 then z else 0).filter(_ > 5).sum\nfn one(n: Int) -> Int\n= (0..n).map(x => if x * 3 - 1 > 4 then x * 3 - 1 else 0).filter(_ > 5).sum\nfn big(k: Int) -> Int\n= [1, 2, k].map(x => do\n  y = x * x\n  y + 1).sum";
    for n in [0, 7, 5000] {
        assert_eq!(release(src, "blk", &[n]), release(src, "one", &[n]));
    }
    assert_eq!(release(src, "big", &[4_000_000_000]), Err("integer overflow".into()));
    let m = parse(src).unwrap();
    let c = sspur_native::cgen::c_source(&m, &sspur_check::check(&m));
    let body = |name: &str| {
        let head = format!("static RR_I f_{name}(int64_t a0");
        let start = c.lines().position(|l| l.starts_with(&head) && l.ends_with('{')).unwrap();
        c.lines().skip(start).take_while(|l| *l != "}").collect::<Vec<_>>().join("\n")
    };
    for f in ["blk", "one"] {
        assert!(body(f).contains("vb_") && !body(f).contains("sspur_alloc"), "{}", body(f));
    }
}
