use super::*;

fn an(src: &str) -> Analysis {
    let m = parse(src).unwrap();
    let c = sspur_check::check(&m);
    assert!(!c.has_errors(), "{:?}", c.diags);
    analyze(&m, &c)
}

fn clause<'a>(a: &'a Analysis, f: &str, text: &str) -> &'a Clause {
    a.clauses.iter().find(|c| c.func == f && c.text == text).unwrap_or_else(|| panic!("no clause {f}: {text}"))
}

fn verdicts(a: &Analysis, c: &Clause) -> Vec<Option<Answer>> {
    let mut s = Solver::new().uncached();
    c.obligations.iter().map(|o| o.query.as_ref().map(|q| s.check(&a.script(q), &a.model_vars(q)))).collect()
}

fn z3() -> bool {
    Solver::new().available()
}

#[test]
fn literals_and_ranges_encode_as_smt_lib() {
    assert_eq!(lit(-5), "(- 5)");
    assert_eq!(lit(7), "7");
    assert_eq!(in_range("x"), format!("(and (<= {MIN} x) (<= x {MAX}))"));
    assert_eq!(and(&[]), "true");
    assert_eq!(and(&["a".into(), "b".into()]), "(and a b)");
}

#[test]
fn post_query_asserts_negated_goal_after_entry_facts() {
    let a = an("fn inc(x: Int) -> Int\n  pre x < 100\n  post r > x\n= x + 1");
    let c = clause(&a, "inc", "r > x");
    let q = c.obligations[0].query.as_ref().unwrap();
    let s = a.script(q);
    assert!(s.contains("(declare-const"));
    assert!(s.contains("(assert (< k1 100))"), "{s}");
    assert!(s.trim_end().ends_with(&format!("(assert (not {}))", q.goal)));
    assert!(a.sites.keys().any(|k| k.2 == "arith"));
}

#[test]
fn truncated_division_matches_runtime_semantics() {
    if !z3() {
        return;
    }
    let a = an("fn d(a: Int, b: Int) -> Int\n  pre b != 0 and a > -100 and a < 100 and b > -100 and b < 100\n  post r * b + a % b == a\n= a / b\nfn m(a: Int) -> Int\n  post r <= 0\n= -7 % 2 + a * 0");
    assert_eq!(verdicts(&a, clause(&a, "d", "r * b + a % b == a")), vec![Some(Answer::Unsat)]);
    assert_eq!(verdicts(&a, clause(&a, "m", "r <= 0")), vec![Some(Answer::Unsat)]);
}

#[test]
fn relational_contracts_prove_and_refute() {
    if !z3() {
        return;
    }
    let src = "fn clamp(x: Int, lo: Int, hi: Int) -> Int\n  pre lo <= hi\n  post r >= lo and r <= hi\n= if x < lo then lo else if x > hi then hi else x\nfn bad(a: Int, b: Int) -> Int\n  pre a <= b\n  post r >= b\n= a\nfn use(x: Int) -> Int\n= clamp(x, 0, 10) + 1\nfn wrong(x: Int) -> Int\n= clamp(x, 5, x)";
    let a = an(src);
    assert_eq!(verdicts(&a, clause(&a, "clamp", "r >= lo and r <= hi")), vec![Some(Answer::Unsat)]);
    let v = verdicts(&a, clause(&a, "bad", "r >= b"));
    assert!(matches!(&v[0], Some(Answer::Sat(m)) if m.len() == 2), "{v:?}");
    let pre = verdicts(&a, clause(&a, "clamp", "lo <= hi"));
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[0], Some(Answer::Unsat));
    assert!(matches!(pre[1], Some(Answer::Sat(_))));
    let o = Oracle::new(&parse(src).unwrap(), &sspur_check::check(&parse(src).unwrap()));
    assert!(o.post_proved("clamp", 0));
    assert!(!o.post_proved("bad", 0));
}

#[test]
fn overflow_sites_use_path_conditions_and_callee_posts() {
    if !z3() {
        return;
    }
    let a = an("fn small(x: Int) -> Int\n  post r >= 0 and r < 1000\n= if x < 0 then 0 else x % 1000\nfn f(a: Int, b: Int) -> Int\n= if a < b and a >= 0 then b - a else small(a) * 1000\nfn g(a: Int) -> Int\n= a + 1");
    let mut s = Solver::new().uncached();
    let mut by_fn: HashMap<String, Vec<bool>> = HashMap::new();
    for site in a.sites.iter().filter(|(k, _)| k.2 == "arith").map(|(_, v)| v) {
        let ok = site.queries.iter().all(|q| q.as_ref().is_some_and(|q| s.check(&a.script(q), &[]) == Answer::Unsat));
        by_fn.entry(site.func.clone()).or_default().push(ok);
    }
    assert!(by_fn["f"].iter().all(|b| *b), "{by_fn:?}");
    assert!(by_fn["g"].iter().all(|b| !*b));
}

#[test]
fn hidden_returns_and_catch_block_proofs() {
    let a = an("type E = Bad\nfn h(x: Int) -> Int\n  post r > 0\n= do\n  y = catch (if x < 0 then return 0 - 1 else if x == 0 then raise Bad else x)\n    | Bad => 0\n  5 + y * 0");
    assert!(clause(&a, "h", "r > 0").obligations.iter().all(|o| o.query.is_none()));
}

#[test]
fn record_field_invariants_feed_proofs() {
    if !z3() {
        return;
    }
    let a = an("type Acct = {bal: Int where _ >= 0}\nfn dep(a: Acct, n: Int where _ > 0) -> Acct\n  post r.bal == a.bal + n\n= {bal: a.bal + n}");
    assert_eq!(verdicts(&a, clause(&a, "dep", "r.bal == a.bal + n")), vec![Some(Answer::Unsat)]);
    let mut s = Solver::new().uncached();
    let site = a.sites.iter().find(|(k, _)| k.2 == "field:bal").unwrap().1;
    assert!(site.queries.iter().all(|q| s.check(&a.script(q.as_ref().unwrap()), &[]) == Answer::Unsat));
}

#[test]
fn models_parse_negative_values() {
    assert_eq!(parse_model("((k1 5) (k2 (- 3)) (k3 true))"), vec![("k1".into(), "5".into()), ("k2".into(), "-3".into()), ("k3".into(), "true".into())]);
}

fn conv_warnings(src: &str) -> Vec<String> {
    let m = parse(src).unwrap();
    let c = sspur_check::check(&m);
    assert!(!c.has_errors(), "{:?}", c.diags);
    let mut ds: Vec<String> = conv::warnings(&m, &c, m.defs.len()).diags.into_iter().map(|d| d.def.unwrap()).collect();
    ds.sort();
    ds
}

#[test]
fn doubles_encode_exactly() {
    use float::real_lit;
    assert_eq!(real_lit(0.5), "(/ 1.0 2.0)");
    assert_eq!(real_lit(-3.0), "(- 3.0)");
    assert_eq!(real_lit(-0.0), "(- 0.0)");
    assert_eq!(real_lit(9_223_372_036_854_775_808.0), "9223372036854775808.0");
    assert_eq!(real_lit(0.95), "(/ 4278419646001971.0 4503599627370496.0)");
    assert_eq!(real_lit(5e-324), format!("(/ 1.0 {}.0)", float::times_pow2(1, 1074)));
}

#[test]
fn conversions_are_proved_from_refinements_guards_and_contracts() {
    if !z3() {
        return;
    }
    let safe = "type Prob = F64 where _ >= 0.0 and _ <= 1.0
type Pt = {x: F64 where _.abs < 1000.0, y: F64}
fn pct(p: Prob) -> Int
= (p * 100.0).round
fn guarded(x: F64) -> Int
= if x.is_finite and x.abs < 9.2e18 then x.floor else 0
fn total_order(x: F64, cap: Int) -> Int
= if x.is_nan or x >= cap.to_f64 then cap else if x <= 0.0 then 0 else x.round
fn pre_ok(x: F64) -> Int
  pre x.abs < 1000000.0
= x.trunc
fn share(xs: List[Int], p: Prob) -> Int
= (xs.len.to_f64 * p).floor
fn clamped(x: F64) -> Int
= min(max(x, 0.0), 100.0).round + clamp(x, -5.0, 5.0).ceil
fn field(p: Pt) -> Int
= p.x.floor
fn edges() -> Int
= 9.223372036854775e18.floor + (-9223372036854775808.0).floor + (2.5 * 4.0).round
fn lam(xs: List[F64]) -> List[Int]
= xs.map(x => if x.abs < 100.0 then x.round else 0)
fn helper(t: F64, k: List[Int]) -> Int
= t.floor + k.len
fn caller(p: Prob) -> Int
= helper(p * 10.0, [])
fn guarded_var(x: F64) -> Int
= do
  var v = x
  if v.abs < 10.0 then v.round else 0
";
    assert_eq!(conv_warnings(safe), Vec::<String>::new());
    let unsafe_ = "fn plain(x: F64) -> Int
= x.floor
fn nan_above(x: F64) -> Int
= if x > 0.0 then x.floor else 0
fn neg_inf(x: F64) -> Int
= if x < 9.2e18 then x.floor else 0
fn rounds_up(n: Int) -> Int
= n.to_f64.floor
fn at_two_63() -> Int
= 9.223372036854775807e18.round
fn sqrt_neg(x: F64) -> Int
= if x < 100.0 then x.sqrt.floor else 0
fn open_helper(t: F64) -> Int
= t.floor
fn caller(p: F64 where _ >= 0.0 and _ <= 1.0) -> Int
= open_helper(p)
fn looped(x: F64) -> Int
= do
  var v = 0.5
  for i in 0..3
    v := v * x
  v.round
";
    assert_eq!(conv_warnings(unsafe_), ["at_two_63", "looped", "nan_above", "neg_inf", "open_helper", "plain", "rounds_up", "sqrt_neg"]);
}
