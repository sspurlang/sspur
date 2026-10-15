use sspur_check::check;
use sspur_syntax::parse;

fn codes(src: &str) -> Vec<String> {
    let m = parse(src).unwrap_or_else(|e| panic!("{e:?}"));
    check(&m).diags.into_iter().map(|d| d.code).collect()
}

fn diags(src: &str) -> Vec<sspur_check::Diag> {
    check(&parse(src).unwrap()).diags
}

#[test]
fn clean_program_has_no_diags() {
    let src = "type E = Bad\nfn f(x: Int) -> Int ! fail[E], log\n= do\n  log(\"x\")\n  if x < 0 then raise Bad\n  x * 2";
    assert!(codes(src).is_empty(), "{:?}", diags(src));
}

#[test]
fn missing_effect_has_fix_op() {
    let d = diags("fn f() -> Unit\n= log(\"hi\")");
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].code, "E_EFFECT_MISSING");
    assert_eq!(d[0].fix[0]["contract"]["effects"][0], "+log");
}

#[test]
fn unused_effect_warns() {
    let d = diags("fn f() -> Int ! log\n= 1");
    assert_eq!(d[0].code, "W_EFFECT_UNUSED");
    assert!(!d[0].is_error());
}

#[test]
fn effects_flow_through_lambdas() {
    let base = "fn noisy(x: Int) -> Int ! log\n= do\n  log(\"{x}\")\n  x\n";
    assert_eq!(codes(&format!("{base}fn g(xs: List[Int]) -> List[Int]\n= xs.map(x => noisy(x))")), vec!["E_EFFECT_MISSING"]);
    assert!(codes(&format!("{base}fn g(xs: List[Int]) -> List[Int] ! log\n= xs.map(x => noisy(x))")).is_empty());
    assert!(codes("fn g(xs: List[Int]) -> List[Int]\n= xs.map(_ + 1)").is_empty());
}

#[test]
fn user_higher_order_fn_is_effect_polymorphic() {
    let src = "fn twice[A, e](x: A, f: A -> A ! e) -> A ! e\n= f(f(x))\nfn noisy(x: Int) -> Int ! log\n= do\n  log(\"{x}\")\n  x\nfn pure_use() -> Int\n= twice(1, _ + 1)\nfn loud_use() -> Int ! log\n= twice(1, noisy)";
    assert!(codes(src).is_empty(), "{:?}", diags(src));
}

#[test]
fn raise_requires_fail_effect_and_catch_discharges_it() {
    assert_eq!(codes("type E = Bad\nfn f() -> Int\n= raise Bad"), vec!["E_EFFECT_MISSING"]);
    let src = "type E = Bad | Worse\nfn f(x: Int) -> Int ! fail[E]\n= if x > 0 then x else raise Bad\nfn g(x: Int) -> Int\n= catch f(x)\n  | Bad => 0\n  | Worse => 1";
    assert!(codes(src).is_empty(), "{:?}", diags(src));
    let partial = "type E = Bad | Worse\nfn f(x: Int) -> Int ! fail[E]\n= if x > 0 then x else raise Bad\nfn g(x: Int) -> Int\n= catch f(x)\n  | Bad => 0";
    assert_eq!(codes(partial), vec!["E_EFFECT_MISSING"]);
}

#[test]
fn type_mismatch() {
    assert_eq!(codes("fn f() -> Int\n= \"no\""), vec!["E_TYPE_MISMATCH"]);
    assert_eq!(codes("fn f() -> Int\n= 1 + 2.0"), vec!["E_TYPE_MISMATCH"]);
}

#[test]
fn nonexhaustive_match_lists_missing() {
    let d = diags("type S = A | B{x: Int} | C\nfn f(s: S) -> Int\n= match s\n  | A => 1");
    assert_eq!(d[0].code, "E_NONEXHAUSTIVE");
    assert!(d[0].msg.contains("B{..}") && d[0].msg.contains("C"), "{}", d[0].msg);
}

#[test]
fn holes_report_expected_type_and_candidates() {
    let d = diags("fn f(a: Int, b: Str) -> Int\n= ?");
    assert_eq!(d[0].code, "E_HOLE");
    assert!(d[0].msg.contains("Int"));
    assert_eq!(d[0].hint.as_deref(), Some("in scope with type Int: a"));
}

#[test]
fn check_holes_gives_the_filler_its_context() {
    let src = "type E = Bad\nfn helper(x: Int) -> Int ! log\n= x\nfn f(a: Int, b: Str) -> Int ! fail[E]\n= do\n  c = a + 1\n  ?x\ntest t = ?";
    let (out, ctx) = sspur_check::check_holes(&parse(src).unwrap());
    assert_eq!(out.diags.iter().filter(|d| d.code == "E_HOLE").count(), 2);
    let h = &ctx.holes[0];
    assert_eq!((h.label().as_str(), h.def.as_str(), h.ty.to_string().as_str(), h.in_test), ("?x", "f", "Int", false));
    let locals: Vec<String> = h.locals.iter().map(|(n, t)| format!("{n}: {t}")).collect();
    assert_eq!(locals, ["a: Int", "b: Str", "c: Int"]);
    assert_eq!(h.effects.iter().cloned().collect::<Vec<_>>(), ["fail[E]"]);
    assert!(ctx.holes[1].in_test && ctx.holes[1].effects.is_empty());
    let helper = ctx.sigs.iter().find(|s| s.name == "helper").unwrap();
    assert_eq!((helper.kind.clone(), helper.effects.iter().cloned().collect::<Vec<_>>()), (sspur_check::SigKind::Fn, vec!["log".to_string()]));
    assert!(ctx.sigs.iter().any(|s| s.name == "len" && s.kind == sspur_check::SigKind::Method("List".into())));
    assert!(sspur_check::check_holes(&parse("fn g() -> Int\n= 1").unwrap()).1.holes.is_empty());
}

#[test]
fn unknown_name_suggests() {
    let d = diags("fn total(x: Int) -> Int\n= x\nfn g() -> Int\n= totl(1)");
    assert_eq!(d[0].code, "E_UNKNOWN_NAME");
    assert_eq!(d[0].hint.as_deref(), Some("did you mean 'total'?"));
}

#[test]
fn foreign_spellings_get_exact_hints() {
    let hint = |src: &str| diags(src).into_iter().find(|d| d.severity == "error").and_then(|d| d.hint).unwrap_or_default();
    assert_eq!(hint("fn f(xs: List[Int]) -> Int\n= len(xs, 1)"), "'len' is a method: write 'x.len' for 'len(x)', and 'x.len(a)' for 'len(x, a)'");
    for src in ["fn f(xs: List[Int]) -> Int\n= len(xs)", "fn f() -> Opt[Int]\n= None", "fn f() -> Opt[Int]\n= Some(1)", "fn f(s: Str) -> Str\n= s.slice(1, 2)", "fn f(n: Int) -> Int\n= n.get"] {
        let d = diags(src);
        assert_eq!(codes(src), vec!["N_FOREIGN"], "{src}");
        assert_eq!(d[0].fix[0]["op"], "norm", "{src}");
    }
    assert_eq!(hint("fn f(xs: List[Int]) -> Opt[Int]\n= xs.head"), "write '.first' (an Opt)");
    assert_eq!(hint("type P = {qty: Int, name: Str}\nfn f(p: P) -> Int\n= p.quantity"), "P has fields qty, name");
    assert_eq!(hint("fn f(n: Int) -> Str\n= \"n=\" + n"), "convert with '.str', or interpolate: \"n={n}\"");
    assert_eq!(hint("fn f(n: Int)\n= n + 1"), "declare the return type: 'fn f(..) -> Int'");
    assert_eq!(hint("type E = A{x: Int} | B{y: Int}\nfn f(e: E) -> Int\n= match e\n  | A{x} => x"), "add | B => ?");
    assert!(hint("fn f(e: Opt[Int], b: Bool) -> Int\n= match e\n  | some(x) => match b\n    | true => x\n    | false => 0\n  | none => 1").contains("helper fn"));
}

#[test]
fn records_and_fields() {
    let ok = "type P = {x: Int, y: Int}\nfn f() -> Int\n= do\n  p = {x: 1, y: 2}\n  p.x + p.y";
    assert!(codes(ok).is_empty(), "{:?}", diags(ok));
    assert_eq!(codes("type P = {x: Int, y: Int}\nfn f() -> P\n= P{x: 1}"), vec!["E_FIELD_MISSING"]);
    assert_eq!(codes("type P = {x: Int}\nfn f(p: P) -> Int\n= p.z"), vec!["E_UNKNOWN_METHOD"]);
}

#[test]
fn contracts_must_be_pure_and_boolean() {
    assert_eq!(codes("fn f(x: Int) -> Int\n  pre x\n= x"), vec!["E_TYPE_MISMATCH"]);
    assert!(codes("fn f(x: Int where _ > 0) -> Int\n  post r > x\n= x + 1").is_empty());
}

#[test]
fn immutable_assignment() {
    assert_eq!(codes("fn f() -> Int\n= do\n  x = 1\n  x := 2\n  x"), vec!["E_ASSIGN_IMMUTABLE"]);
}

#[test]
fn newtypes_do_not_mix() {
    let base = "type UserId = new Str\ntype OrderId = new Str\nfn u(id: UserId) -> Str\n= id.raw\n";
    assert!(codes(&format!("{base}fn g() -> Str\n= u(UserId(\"a\"))")).is_empty());
    assert_eq!(codes(&format!("{base}fn g() -> Str\n= u(OrderId(\"a\"))")), vec!["E_TYPE_MISMATCH"]);
}

#[test]
fn secrets_cannot_leak() {
    let base = "type U = {pw: Secret[Str]}\n";
    assert_eq!(codes(&format!("{base}fn f(u: U) -> Str\n= \"pw={{u.pw}}\"")), vec!["E_SECRET_LEAK"]);
    assert_eq!(codes(&format!("{base}fn f(u: U) -> Str\n= \"{{u}}\"")), vec!["E_SECRET_LEAK"], "secrets nested in records are caught");
    assert_eq!(codes(&format!("{base}fn f(u: U) -> Str\n= u.pw.str")), vec!["E_SECRET_LEAK"]);
    assert_eq!(codes(&format!("{base}fn f(u: U) -> Bool\n= u.pw == secret(\"x\")")), vec!["E_SECRET_COMPARE"]);
    assert_eq!(codes(&format!("{base}fn f(u: U) -> Str\n= u.pw.expose(\"\")")), vec!["E_REASON_REQUIRED"]);
    assert_eq!(codes(&format!("{base}fn f(u: U) -> Str ! log\n= u.pw.map(p => do_log(p))\nfn do_log(p: Str) -> Str ! log\n= do\n  log(p)\n  p"))[0], "E_EFFECT_NOT_ALLOWED");
    let ok = diags(&format!("{base}fn f(u: U) -> Str\n= u.pw.expose(\"hashing for storage\")"));
    assert!(ok.iter().all(|d| !d.is_error()));
    assert_eq!(ok[0].code, "A_DECLASSIFY");
}

#[test]
fn untrusted_and_guess_must_be_handled() {
    assert_eq!(codes("fn q(name: Untrusted[Str]) -> Str\n= \"SELECT * WHERE n = '{name}'\""), vec!["E_UNTRUSTED_INTERP"]);
    assert_eq!(codes("fn q(name: Untrusted[Str]) -> Str\n= name + \"x\"")[0], "E_TYPE_MISMATCH");
    assert_eq!(codes("fn f(g: Guess[Int]) -> Int\n= g + 1")[0], "E_TYPE_MISMATCH");
    assert!(codes("fn f(g: Guess[Int]) -> Int\n= g.at_least(0.8).or(0)").is_empty());
}

const ASK: &str = "effect ask() -> Int\neffect state\n  get() -> Int\n  put(v: Int)\nfn twice() -> Int ! ask\n= ask() + ask()\n";

#[test]
fn performing_an_operation_needs_its_effect() {
    let d = diags(&format!("{ASK}fn f() -> Int\n= ask()"));
    assert_eq!(d[0].code, "E_EFFECT_MISSING");
    assert_eq!(d[0].fix[0]["contract"]["effects"][0], "+ask");
}

#[test]
fn handle_removes_the_effect_and_adds_arm_effects() {
    assert!(codes(&format!("{ASK}fn f() -> Int\n= handle twice()\n  | ask() => resume(1)")).is_empty());
    assert_eq!(codes(&format!("{ASK}fn f() -> Int\n= handle twice()\n  | ask() => do\n    log(\"asked\")\n    resume(1)")), vec!["E_EFFECT_MISSING"]);
    assert!(codes(&format!("{ASK}fn f() -> Int ! log\n= handle twice()\n  | ask() => do\n    log(\"asked\")\n    resume(1)")).is_empty());
    assert!(codes("fn f() -> Int\n= handle do\n    log(\"x\")\n    1\n  | log(m) => resume()").is_empty(), "log is an ordinary effect");
}

#[test]
fn handle_must_cover_every_operation() {
    let d = diags(&format!("{ASK}fn c() -> Int ! state\n= get()\nfn f() -> Int\n= handle c()\n  | get() => resume(1)"));
    assert_eq!(d[0].code, "E_HANDLE_PARTIAL");
    assert!(d[0].hint.as_ref().unwrap().contains("put"));
    assert_eq!(codes(&format!("{ASK}fn f() -> Int\n= handle twice()\n  | akk() => resume(1)"))[0], "E_UNKNOWN_OP");
    assert_eq!(codes(&format!("{ASK}fn f() -> Int\n= handle 1\n  | ask() => resume(2)")), vec!["W_HANDLE_UNUSED"]);
}

#[test]
fn resume_is_typed_and_positioned() {
    assert_eq!(codes(&format!("{ASK}fn f() -> Int\n= handle twice()\n  | ask() => resume(\"x\")")), vec!["E_TYPE_MISMATCH"]);
    assert_eq!(codes(&format!("{ASK}fn f() -> Int\n= handle twice()\n  | ask() => resume(1) + 1")), vec!["E_RESUME_POSITION"]);
    assert_eq!(codes(&format!("{ASK}fn f() -> Int\n= handle twice()\n  | ask() => [1].map(x => resume(x)).sum")), vec!["E_RESUME_POSITION"]);
    assert_eq!(codes(&format!("{ASK}fn f() -> Int\n= handle twice()\n  | ask() => do\n    r = resume(1)\n    resume(r)")), vec!["E_RESUME_POSITION"]);
    assert!(codes(&format!("{ASK}fn f() -> Int\n= handle twice()\n  | ask() => do\n    r = resume(1)\n    r * 2")).is_empty());
    assert!(codes(&format!("{ASK}fn f(b: Bool) -> Int\n= handle twice()\n  | ask() => if b then resume(1) else 0")).is_empty());
    assert_eq!(codes(&format!("{ASK}fn f() -> Int\n= handle twice()\n  | ask() => return 1")), vec!["E_RETURN_IN_HANDLER"]);
}

#[test]
fn generators_are_typed_through_for() {
    let base = "fn nums() -> Unit ! yield[Int]\n= do\n  yield(1)\n  yield(2)\n";
    assert!(codes(&format!("{base}fn f() -> Int\n= do\n  var s = 0\n  for x in nums()\n    s := s + x\n  s")).is_empty());
    assert_eq!(codes(&format!("{base}fn f() -> Int\n= do\n  var s = 0\n  for x in nums()\n    s := s + x.len\n  s")), vec!["E_UNKNOWN_METHOD"]);
    assert_eq!(codes("fn g() -> Unit ! yield[Int]\n= yield(\"a\")"), vec!["E_EFFECT_MISSING", "W_EFFECT_UNUSED"]);
    assert_eq!(codes("fn g() -> Unit ! yield\n= ()"), vec!["E_EFFECT_ARGS"]);
    let take = "fn take[T, e](n: Int, gen: () -> Unit ! yield[T], e) -> List[T] ! e\n= do\n  var out = []\n  handle gen()\n    | yield(x) => do\n      out := out.push(x)\n      if out.len >= n then () else resume()\n  out\n";
    assert!(codes(&format!("{base}{take}fn f() -> List[Int]\n= take(1, nums)")).is_empty());
    assert_eq!(codes(&format!("{base}{take}fn f() -> List[Str]\n= take(1, nums)")), vec!["E_TYPE_MISMATCH"]);
}

#[test]
fn deep_constructor_nesting_checks_fast() {
    let ty = "type Expr = Num{v: Int} | Var{name: Str} | Add{a: Expr, b: Expr} | Mul{a: Expr, b: Expr} | Sub{a: Expr, b: Expr} | Div{a: Expr, b: Expr}\nfn id[T](x: T) -> T\n= x\n";
    let mut e = "Num{v: 0}".to_string();
    for i in 0..12 {
        e = format!("{}{{a: {e}, b: Var{{name: \"x\"}}}}", ["Add", "Sub", "Mul", "Div"][i % 4]);
    }
    let start = std::time::Instant::now();
    assert!(codes(&format!("{ty}test t = id({e}) == Var{{name: \"x\"}}")).is_empty());
    assert!(start.elapsed() < std::time::Duration::from_secs(1), "{:?}", start.elapsed());
    let deep = "type L9 = {s: Secret[Str]}\ntype L8 = A8{x: L9} | B8{x: Expr}\ntype L7 = {x: L8}\ntype W = W{x: L7, e: Expr}\n";
    assert_eq!(codes(&format!("{ty}{deep}fn f(a: W, b: W) -> Bool\n= a == b")), vec!["E_SECRET_COMPARE"]);
}

#[test]
fn branching_alias_cycle_is_reported() {
    let start = std::time::Instant::now();
    assert!(codes("type A = (A, A)\nfn f(x: A) -> Int\n= 1").contains(&"E_TYPE_CYCLE".to_string()));
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn par_tasks_cannot_race_on_shared_state() {
    let race = "fn f() -> Int\n= do\n  var s = 0\n  for i in par(0..3)\n    s := s + i\n  s";
    assert_eq!(codes(race), vec!["E_PAR_RACE"]);
    let own = "fn f() -> Int\n= do\n  for i in par(0..3)\n    var s = i\n    s := s + 1\n  0";
    assert!(codes(own).is_empty(), "{:?}", diags(own));
    let closure = "fn f() -> Int\n= do\n  var s = 0\n  fn bump() -> Unit\n  = do\n    s := s + 1\n  for i in par(0..3)\n    bump()\n  s";
    assert_eq!(codes(closure), vec!["E_PAR_SHARE"]);
    let param = "fn both(f: () -> Int) -> (Int, Int)\n= par(f(), f())";
    assert_eq!(codes(param), vec!["E_PAR_SHARE", "E_PAR_SHARE"]);
    let chan_fn = "fn f() -> Unit ! conc\n= do\n  c = chan()\n  c.send(x => x + 1)";
    assert_eq!(codes(chan_fn), vec!["E_PAR_SHARE"]);
    let atomic = "fn f() -> Int ! conc\n= do\n  a = atomic(0)\n  for i in par(0..3)\n    a.add(i)\n  a.load";
    assert!(codes(atomic).is_empty(), "{:?}", diags(atomic));
}

#[test]
fn par_tasks_restrict_effects_and_control() {
    assert_eq!(codes("fn f() -> Unit ! log\n= do\n  for i in par(0..2)\n    log(\"x\")\n  ()"), vec!["E_PAR_EFFECT"]);
    assert_eq!(codes("effect ask() -> Int\nfn f() -> (Int, Int) ! ask\n= par(ask(), 1)"), vec!["E_PAR_EFFECT"]);
    assert_eq!(codes("fn f() -> Int\n= do\n  for i in par(0..2)\n    return 1\n  0"), vec!["E_RETURN_IN_PAR"]);
    assert_eq!(codes("fn f() -> (Int, Int)\n= par(1)"), vec!["E_PAR_ARITY", "E_TYPE_MISMATCH"]);
    assert_eq!(codes("fn f() -> Int\n= do\n  a = atomic(1)\n  a.load"), vec!["E_EFFECT_MISSING"]);
    assert_eq!(codes("fn f(c: Chan[Int]) -> Int\n= do\n  var s = 0\n  for x in c\n    s := s + x\n  s"), vec!["E_EFFECT_MISSING"]);
    let fails = "type E = Bad\nfn g(x: Int) -> Int ! fail[E]\n= if x > 1 then raise Bad else x\nfn f() -> (Int, Int) ! fail[E]\n= par(g(1), g(2))";
    assert!(codes(fails).is_empty(), "{:?}", diags(fails));
}

#[test]
fn std_system_calls_need_their_effects() {
    for (call, eff) in [("read_file(\"a\").is_ok", "fs"), ("read_line().is_some", "io"), ("now_ms() > 0", "time"), ("args().is_empty", "env")] {
        let d = diags(&format!("fn f() -> Bool\n= {call}"));
        assert_eq!(d.len(), 1, "{call}: {d:?}");
        assert_eq!(d[0].code, "E_EFFECT_MISSING");
        assert_eq!(d[0].fix[0]["contract"]["effects"][0], format!("+{eff}"));
        assert!(codes(&format!("fn f() -> Bool ! {eff}\n= {call}")).is_empty());
    }
}

#[test]
fn json_rejects_types_without_a_wire_form() {
    assert!(codes("type P = {x: Int, tags: Set[Str]}\nfn f(p: P) -> Res[P, Str]\n= json.decode[P](json.encode(p))").is_empty());
    assert_eq!(codes("fn f(g: Int -> Int) -> Str\n= json.encode(g)"), vec!["E_JSON"]);
    assert_eq!(codes("type Q = {n: Int where _ > 0}\nfn f(s: Str) -> Bool\n= json.decode[Q](s).is_ok"), vec!["E_JSON"]);
    assert_eq!(codes("fn f[T](x: T) -> Str\n= json.encode(x)"), vec!["E_JSON"]);
    assert_eq!(codes("fn f() -> Str\n= json.nope(1)"), vec!["E_UNKNOWN_METHOD"]);
}

#[test]
fn user_definitions_shadow_std_names() {
    assert!(codes("type Set = {n: Int}\nfn chunks(s: Set, k: Int) -> Int\n= s.n * k\nfn f() -> Int\n= Set{n: 2}.chunks(3)").is_empty());
}

#[test]
fn early_return_in_a_block_lambda_is_rejected_with_a_hint() {
    let d = diags("fn f(xs: List[Int]) -> List[Int]\n= xs.map(x => do\n  if x > 3 then return 0\n  x)");
    assert_eq!(d[0].code, "E_RETURN_IN_LAMBDA");
    assert!(d[0].hint.as_deref().unwrap_or("").contains("last line"), "{d:?}");
    assert!(codes("fn f(xs: List[Int]) -> List[Int]\n= xs.map(x => do\n  y = x + 1\n  return y)").is_empty());
    let src = "type E = Neg\nfn f(xs: List[Int]) -> Int\n= xs.map(x => do\n  log(\"{x}\")\n  if x < 0 then raise Neg\n  x).len";
    let mut c = codes(src);
    c.sort();
    assert_eq!(c, ["E_EFFECT_MISSING", "E_EFFECT_MISSING"]);
}

#[test]
fn or_patterns_bind_the_same_names_at_the_same_types() {
    let base = "type S = A{x: Int} | B{x: Int, s: Str} | C\n";
    assert!(codes(&format!("{base}fn f(v: S) -> Int\n= match v\n  | A{{x}} | B{{x}} => x\n  | C => 0")).is_empty());
    let d = diags(&format!("{base}fn f(v: S) -> Int\n= match v\n  | A{{x}} | C => x\n  | B => 0"));
    assert_eq!(d[0].code, "E_PATTERN_OR_BINDS");
    assert!(d[0].msg.contains("'A{x}' binds 'x' but the alternative 'C' does not"), "{}", d[0].msg);
    assert!(d[0].hint.as_deref().unwrap().contains("bind 'x' in every alternative"));
    let d = diags(&format!("{base}fn f(v: S) -> Int\n= match v\n  | A{{x}} | B{{s: x}} => 1\n  | C => 0"));
    assert_eq!(d[0].code, "E_PATTERN_OR_BINDS");
    assert!(d[0].msg.contains("'x' is Int in 'A{x}' but Str in 'B{s: x}'"), "{}", d[0].msg);
}

#[test]
fn exhaustiveness_sees_nested_patterns_lists_and_alternatives() {
    let missing = |src: &str| diags(src).into_iter().find(|d| d.code == "E_NONEXHAUSTIVE").map(|d| d.msg).unwrap_or_default();
    assert!(codes("fn f(o: Opt[Opt[Int]]) -> Int\n= match o\n  | some(some(x)) => x\n  | some(none) | none => 0").is_empty());
    assert_eq!(missing("fn f(o: Opt[Opt[Int]]) -> Int\n= match o\n  | some(some(x)) => x\n  | none => 0"), "match does not cover: some(none)");
    assert!(codes("fn f(xs: List[Int]) -> Int\n= match xs\n  | [] => 0\n  | [x, ..rest] => x + f(rest)").is_empty());
    assert!(codes("fn f(xs: List[Int]) -> Int\n= match xs\n  | [first, .., last] => first + last\n  | [x] => x\n  | [] => 0").is_empty());
    assert_eq!(missing("fn f(xs: List[Int]) -> Int\n= match xs\n  | [] => 0\n  | [x] => x"), "match does not cover: [_, _, ..]");
    assert_eq!(missing("fn f(xs: List[Opt[Int]]) -> Int\n= match xs\n  | [some(x), ..] => x\n  | [] => 0"), "match does not cover: [none, ..]");
    assert_eq!(missing("fn f(p: (Bool, Bool)) -> Int\n= match p\n  | (true, _) | (_, true) => 1"), "match does not cover: (false, false)");
    assert_eq!(missing("type S = A{x: Opt[Int]} | B\nfn f(v: S) -> Int\n= match v\n  | A{x: some(n)} => n\n  | B => 0"), "match does not cover: A{x: none}");
    assert!(codes("fn f(o: Opt[Int]) -> Int\n= if o is some(v) and v > 0 then v else 0").is_empty());
}

#[test]
fn arms_the_earlier_arms_cover_are_unreachable() {
    let warn = |src: &str| diags(src).into_iter().filter(|d| d.code == "W_ARM_UNREACHABLE").map(|d| d.msg).collect::<Vec<_>>();
    assert_eq!(warn("fn f(xs: List[Int]) -> Int\n= match xs\n  | [x, ..] => x\n  | [x, y] => y\n  | [] => 0"), ["arm 2 '| [x, y]' can never match: the arms above cover it"]);
    assert_eq!(warn("fn f(o: Opt[Int]) -> Int\n= match o\n  | some(x) => x\n  | none => 0\n  | _ => 1"), ["arm 3 '| _' can never match: the arms above cover it"]);
    assert_eq!(warn("fn f(n: Int) -> Int\n= match n\n  | 1 | 2 => 0\n  | 3 | 2 => 1\n  | _ => 2"), ["alternative '2' of arm 2 can never match: earlier patterns cover it"]);
    assert_eq!(warn("fn f(p: (Int, Int)) -> Int\n= if p is (a, b) then a + b else 0"), ["the pattern always matches, so the else branch never runs"]);
    assert!(warn("fn f(o: Opt[Int]) -> Int\n= match o\n  | some(x) if x > 0 => x\n  | some(x) => 0\n  | none => 1").is_empty());
    assert_eq!(codes("fn f(xs: List[Int]) -> Int\n= match xs\n  | [a, ..r] | [..r, a] => a + r.len\n  | [] => 0"), ["W_ARM_UNREACHABLE"]);
}

#[test]
fn list_patterns_type_their_parts() {
    assert_eq!(codes("fn f(n: Int) -> Int\n= match n\n  | [x] => x\n  | _ => 0"), vec!["E_PATTERN_TYPE"]);
    assert!(codes("fn f(xs: List[Str]) -> Int\n= match xs\n  | [x, ..rest] => x + rest.len\n  | [] => 0").iter().all(|c| c == "E_TYPE_MISMATCH"));
    assert!(codes("fn f(xs: List[Str]) -> Int\n= match xs\n  | [x, ..rest] => x.len + rest.len\n  | [] => 0").is_empty());
}

#[test]
fn is_tests_bind_only_for_what_follows_them() {
    assert!(codes("fn f(xs: List[Opt[Int]]) -> Int\n= xs.filter(_ is some(_)).len").is_empty());
    assert!(codes("fn f(o: Opt[Int]) -> Bool\n= o is some(v) and v > 0").is_empty());
    assert!(codes("type S = C{r: Int} | D\nfn f(a: S, b: S) -> Int\n= if a is C{r} and b is C{r: r2} then r + r2 else 0").is_empty());
    assert!(codes("fn f(a: Opt[Int], b: Opt[Int]) -> Int\n= if a is some(x) and b is some(y) and x > y then x - y else 0").is_empty());
    let d = diags("fn f(r: Res[Int, Str]) -> Bool\n= r is ok(v)");
    assert_eq!(d[0].code, "E_IS_BINDS");
    assert_eq!(d[0].msg, "'v' is bound by an 'is' test, but nothing after it uses it");
    assert!(d[0].hint.as_deref().unwrap().starts_with("write 'ok(_)'"), "{:?}", d[0].hint);
    assert_eq!(codes("fn f(o: Opt[Int], g: Bool) -> Int\n= if o is some(v) or g then 1 else 0"), ["E_IS_BINDS"]);
    assert_eq!(codes("fn f(o: Opt[Int], g: Bool) -> Bool\n= o is some(v) and g"), ["E_IS_BINDS"]);
    assert_eq!(codes("fn f(o: Opt[Int]) -> Int\n= if o is some(v) and v > 0 or v < -9 then v else 0"), Vec::<String>::new());
}

#[test]
fn is_tests_that_always_or_never_match_warn() {
    let warn = |src: &str| diags(src).into_iter().filter(|d| !d.is_error()).map(|d| (d.code, d.msg)).collect::<Vec<_>>();
    assert_eq!(warn("fn f(p: (Int, Int)) -> Bool\n= p is (_, _)"), [("W_ARM_UNREACHABLE".to_string(), "the pattern always matches, so this test is always true".to_string())]);
    assert_eq!(warn("fn f() -> Bool\n= none is some(_)"), [("W_NEVER_MATCHES".to_string(), "'some(_)' can never match 'none', so this test is always false".to_string())]);
    assert_eq!(warn("fn f() -> Int\n= if [1, 2] is [x] then x else 0")[0].0, "W_NEVER_MATCHES");
    assert_eq!(warn("type S = C{r: Int} | D\nfn f() -> Int\n= do\n  C{r} = D else 0\n  r")[0].1, "'C{r}' can never match 'D', so 'else' always runs");
    assert!(warn("fn f(o: Opt[Int]) -> Bool\n= o is some(1 | 2)").is_empty());
    assert!(warn("fn f(x: Int) -> Bool\n= some(x) is some(1)").is_empty());
}

#[test]
fn guards_need_else_exactly_when_the_pattern_can_fail() {
    assert!(codes("fn f(o: Opt[Int]) -> Int\n= do\n  some(v) = o else 0\n  v + 1").is_empty());
    assert!(codes("type E = Bad\nfn f(o: Opt[Int]) -> Int ! fail[E]\n= do\n  some(v) = o else raise Bad\n  v").is_empty());
    assert_eq!(codes("type E = Bad\nfn f(o: Opt[Int]) -> Int\n= do\n  some(v) = o else raise Bad\n  v"), ["E_EFFECT_MISSING"]);
    assert!(codes("fn f(xs: List[Opt[Int]]) -> Int\n= do\n  var t = 0\n  for x in xs\n    some(v) = x else ()\n    t := t + v\n  t").is_empty());
    assert_eq!(codes("fn f(o: Opt[Int]) -> Int\n= do\n  some(v) = o else \"none\"\n  v"), ["E_TYPE_MISMATCH"]);
    let d = diags("fn f(o: Opt[Int]) -> Int\n= do\n  some(v) = o\n  v");
    assert_eq!(d[0].code, "E_REFUTABLE_LET");
    assert!(d[0].hint.as_deref().unwrap().contains("'some(v) = o else <value, raise or return>'"), "{:?}", d[0].hint);
    let d = diags("fn f(p: (Int, Int)) -> Int\n= do\n  (a, b) = p else 0\n  a + b");
    assert_eq!((d[0].code.as_str(), d[0].msg.as_str()), ("W_ARM_UNREACHABLE", "the pattern always matches, so 'else' never runs"));
    assert!(codes("type P = {x: Int, y: Int}\nfn f(p: P) -> Int\n= do\n  P{x, y} = p\n  x + y").is_empty(), "a record pattern can't fail");
}

#[test]
fn upper_case_fields_are_field_accesses() {
    let ty = "type Omm = {OBJECT_NAME: Str, MEAN_MOTION: F64}\n";
    let ok = format!("{ty}fn f(g: Omm) -> Str\n= g.OBJECT_NAME\nfn h(o: Opt[Omm]) -> F64\n= match o\n  | some(g) => g.MEAN_MOTION * 2.0\n  | none => 0.0\nfn k(s: Str) -> Res[List[Str], Str]\n= json.decode[List[Omm]](s).map(gs => gs.map(g => g.OBJECT_NAME.trim))\nfn m() -> F64\n= do\n  o = Omm{{OBJECT_NAME: \"ISS\", MEAN_MOTION: 15.5}}\n  o.MEAN_MOTION\n");
    assert!(codes(&ok).is_empty(), "{:?}", diags(&ok));
    let d = diags(&format!("{ty}fn f(g: Omm) -> Int\n= g.OBJECT_NAMEX"));
    assert_eq!(d[0].code, "E_UNKNOWN_METHOD");
    assert_eq!(d[0].hint.as_deref(), Some("did you mean 'OBJECT_NAME'?"));
    assert_eq!(codes(&format!("{ty}fn f(g: Omm) -> Int\n= g.OBJECT_NAME")), vec!["E_TYPE_MISMATCH"]);
}

#[test]
fn data_libraries_are_typed() {
    let ok = "type U = {id: Int, email: Opt[Str]}\nfn f() -> List[U] ! db, fail[DbErr]\n= do\n  c = db.open(\":memory:\")\n  db.query[U](c, \"select * from u where id = ? and name = ?\", [1, \"a\"])\nfn g(s: Str) -> Bytes\n= crypto.sha256(s)\nfn h() -> Str ! rand\n= crypto.token(16)\nfn k(t: Str) -> Res[List[U], Str]\n= csv.decode[U](t)";
    assert!(codes(ok).iter().all(|c| c.starts_with('W')), "{:?}", diags(ok));
    assert_eq!(codes("fn f() -> Int ! db, fail[DbErr]\n= db.exec(db.open(\"x\"), \"x\", 5)"), vec!["E_SQL_PARAM"]);
    assert_eq!(codes("fn f() -> Int ! db, fail[DbErr]\n= db.exec(db.open(\"x\"), \"x\", [[1]])"), vec!["E_SQL_PARAM"]);
    assert_eq!(codes("type U = {f: List[Int]}\nfn f() -> List[U] ! db, fail[DbErr]\n= db.query[U](db.open(\"x\"), \"x\")"), vec!["E_SQL_ROW"]);
    assert_eq!(codes("type U = {f: List[Int]}\nfn f(t: Str) -> Res[List[U], Str]\n= csv.decode[U](t)"), vec!["E_CSV_TYPE"]);
    assert_eq!(codes("fn f() -> Bytes\n= crypto.sha256(5)"), vec!["E_TYPE_MISMATCH"]);
    assert_eq!(codes("fn f() -> Bytes\n= crypto.random(5)"), vec!["E_EFFECT_MISSING"]);
    assert_eq!(codes("type E = Sql{x: Int}\nfn f() -> Db ! db, fail[DbErr]\n= do\n  a = db.open(\"x\")\n  db.open(\"y\")"), vec!["E_DB_SHADOW"]);
    assert_eq!(codes("fn f() -> Int\n= db.frob(1)"), vec!["E_UNKNOWN_METHOD"]);
    assert_eq!(codes("fn f() -> Int ! db, fail[DbErr]\n= db.tx(db.open(\"x\"), () => 1)"), Vec::<String>::new());
}
