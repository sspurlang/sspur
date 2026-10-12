use sspur_syntax::{parse, print_module, visit::strip_spans, Def, Expr, ExprKind, Stmt};
use std::path::Path;

fn roundtrip(src: &str) {
    let mut a = parse(src).unwrap_or_else(|e| panic!("parse failed: {e:?}"));
    let printed = print_module(&a);
    let mut b = parse(&printed).unwrap_or_else(|e| panic!("reparse failed: {e:?}\n{printed}"));
    assert_eq!(printed, print_module(&b), "printer is not idempotent");
    strip_spans(&mut a);
    strip_spans(&mut b);
    assert_eq!(a, b, "AST changed through print:\n{printed}");
}

#[test]
fn roundtrip_programs() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let mut n = 0;
    for dir in ["programs", "ownership/accept"] {
        for entry in std::fs::read_dir(base.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "ssp") {
                roundtrip(&std::fs::read_to_string(&path).unwrap());
                n += 1;
            }
        }
    }
    let rejects = std::fs::read_to_string(base.join("ownership/reject.ssp")).unwrap();
    for case in rejects.split("// case ").skip(1) {
        roundtrip(case.split_once('\n').unwrap().1);
        n += 1;
    }
    assert!(n > 100);
}

#[test]
fn field_assignment_is_sugar_for_with() {
    let m = parse("fn f(p: P) -> P\n= do\n  var q = p\n  q.a.b := 1\n  q").unwrap();
    let printed = print_module(&m);
    assert!(printed.contains("  q.a.b := 1\n"), "{printed}");
    let Def::Fn(f) = &m.defs[0] else { panic!() };
    let ExprKind::Block(stmts) = &f.body.kind else { panic!() };
    assert!(matches!(&stmts[1], Stmt::Assign(n, Expr { kind: ExprKind::With(..), .. }, _) if n == "q"));
}

#[test]
fn placeholder_becomes_lambda() {
    let m = parse("fn f(xs: List[Int]) -> List[Int]\n= xs.map(_ * _ + 1)").unwrap();
    let printed = print_module(&m);
    assert!(printed.contains("xs.map(_ * _ + 1)"), "{printed}");
}

#[test]
fn semicolons_canonicalize_to_block() {
    let m = parse("fn f() -> Int ! log\n= log(\"a\"); 1").unwrap();
    assert_eq!(print_module(&m), "fn f() -> Int ! log\n= do\n  log(\"a\")\n  1\n");
}

#[test]
fn errors_have_codes() {
    let e = parse("fn f( -> Int\n= 1").unwrap_err();
    assert_eq!(e.code, "E_PARSE_EXPECTED");
    let e = parse("queue Ship = fifo[Order]").unwrap_err();
    assert_eq!(e.code, "E_UNSUPPORTED");
    let e = parse("store Orders = blob[Str, Str]").unwrap_err();
    assert_eq!(e.code, "E_UNSUPPORTED");
}

#[test]
fn store_and_svc_roundtrip() {
    let src = "store Orders = table[OrderId, Order]\n\nsvc shop\n  ep post \"/orders\" = place\n  ep get \"/orders/{id}\" = find\n";
    let m = parse(src).unwrap();
    assert_eq!(print_module(&m), src);
    let Def::Svc(s) = &m.defs[1] else { panic!() };
    assert_eq!(s.eps[1].path_params(), ["id"]);
}

#[test]
fn tolerant_input_canonicalizes() {
    let natural = "fn f(xs: List[Int], n: Int) -> Int\n= do\n  var t = 0\n  for x in xs do\n    if x < n then\n      t := t + x\n    else\n      t := t - 1\n  t\n";
    let canonical = "fn f(xs: List[Int], n: Int) -> Int\n= do\n  var t = 0\n  for x in xs\n    if x < n then t := t + x else t := t - 1\n  t\n";
    let mut a = parse(natural).unwrap();
    let mut b = parse(canonical).unwrap();
    strip_spans(&mut a);
    strip_spans(&mut b);
    assert_eq!(a, b);
    assert_eq!(print_module(&parse(natural).unwrap()), canonical);
}

#[test]
fn else_on_next_line_and_single_quotes() {
    let a = parse("fn g(n: Int) -> Str\n= if n < 0 then 'neg'\n  else if n == 0 then 'zero'\n  else 'pos'").unwrap();
    assert_eq!(print_module(&a), "fn g(n: Int) -> Str\n= if n < 0 then \"neg\" else if n == 0 then \"zero\" else \"pos\"\n");
}

#[test]
fn blocks_inside_parentheses_use_layout() {
    let src = "fn h(xs: List[Int]) -> Int\n= xs.fold(0, (acc, x) =>\n    do\n    y = x * 2\n    acc + y\n  )";
    let m = parse(src).unwrap_or_else(|e| panic!("{e:?}"));
    let printed = print_module(&m);
    let mut a = m;
    let mut b = parse(&printed).unwrap_or_else(|e| panic!("{e:?}\n{printed}"));
    strip_spans(&mut a);
    strip_spans(&mut b);
    assert_eq!(a, b);
}

#[test]
fn nested_strings_in_interpolation() {
    let m = parse("fn f(x: Str) -> Str\n= \"a {x.replace(\"b\", 'c')} d\"").unwrap();
    assert!(print_module(&m).contains("x.replace(\"b\", \"c\")"));
}

#[test]
fn nested_if_keeps_its_else_through_print() {
    let src = "type E = Bad\ntype S = A | B\nfn f(a: Bool, b: Bool) -> Int ! log\n= do\n  var v = 0\n  if a then do\n    if b then v := 1\n  else do\n    log(\"x\")\n  if a then do\n    if b then v := 2 else if not b then v := 3\n  else v := 4\n  v\n\nfn g(a: Bool, s: S) -> Int\n= if a then match s\n    | A => 1\n    | B => 2\n  else 3\n\nfn h(a: Bool, b: Bool) -> Int\n= if a then (if b then 1 else 2) else 3\n\nfn k(a: Bool) -> Int\n= if a then do\n    catch r()\n    | Bad => 1\n  else 2\n\nfn r() -> Int ! fail[E]\n= raise Bad\n";
    let m = parse(src).unwrap_or_else(|e| panic!("{e:?}"));
    let Def::Fn(f) = &m.defs[2] else { panic!() };
    let ExprKind::Block(stmts) = &f.body.kind else { panic!() };
    assert!(matches!(&stmts[1], Stmt::Expr(Expr { kind: ExprKind::If(_, t, Some(_)), .. }) if matches!(t.kind, ExprKind::If(_, _, None))));
    roundtrip(src);
}

#[test]
fn nested_if_inside_interpolation_stays_on_one_line() {
    let src = "fn f(a: Bool, b: Bool) -> Str\n= \"{if a then (if b then 1 else 2) else 3} {g(if a then (if b then 4) else 5)}\"\n\nfn g(x: Unit) -> Int\n= 1\n";
    let printed = print_module(&parse(src).unwrap());
    assert!(printed.contains("\"{if a then (if b then 1 else 2) else 3}"), "{printed}");
    roundtrip(src);
}

#[test]
fn negative_zero_receiver_keeps_its_parentheses() {
    let src = "fn f() -> Int\n= (-0.0).fmt(1).len + (-0.0).round\n";
    assert!(print_module(&parse(src).unwrap()).contains("(-0.0).fmt(1).len + (-0.0).round"));
    roundtrip(src);
}

#[test]
fn index_after_method_with_constructor_expression() {
    let src = "type S = A{x: Int} | C\nfn g(m: Map[Int, Int]) -> Int = m.keys[A{x: 3}.x]\nfn h(j: Json) -> Int = j.as[Int]\n";
    let m = parse(src).unwrap_or_else(|e| panic!("{e:?}"));
    let Def::Fn(f) = &m.defs[1] else { panic!() };
    assert!(matches!(&f.body.kind, ExprKind::Index(..)), "{:?}", f.body.kind);
    let Def::Fn(h) = &m.defs[2] else { panic!() };
    assert!(matches!(&h.body.kind, ExprKind::Method { targs, .. } if targs.len() == 1));
    roundtrip(src);
}

#[test]
fn richer_patterns_print_canonically() {
    let cases = [
        ("fn f(o: Opt[Int]) -> Int\n= if let some(v) = o then v else 0", "= if o is some(v) then v else 0"),
        ("fn f(o: Opt[Int]) -> Int\n= if o is some(v) and v > 0 or v < -9 then v else 0", "= if o is some(v) and (v > 0 or v < -9) then v else 0"),
        ("fn f(a: Bool, b: Bool) -> Int\n= if (a and b) is true then 1 else 0", "= if (a and b) is true then 1 else 0"),
        ("fn f(o: Opt[Int]) -> Unit ! log\n= if o is some(v) then log(\"{v}\")", "= if o is some(v) then log(\"{v}\")"),
        ("fn f(o: Opt[Int], p: Opt[Int]) -> Int\n= if o is some(v) then v else if p is some(w) then w else 0", "= if o is some(v) then v else if p is some(w) then w else 0"),
        ("fn f(o: Opt[Int], p: Opt[Int]) -> Int\n= if o is some(v) then (if p is some(w) then w else v) else 0", "then (if p is some(w) then w else v) else 0"),
        ("fn f(xs: List[Int]) -> Int\n= match xs\n  | [x, ...rest] => x\n  | [*rest] => 0", "| [x, ..rest] => x\n| [..rest] => 0"),
        ("fn f(xs: List[Int]) -> Int\n= match xs\n  | [] | [_] => 0\n  | [a, .., z] | [a, z] => a + z", "| [] | [_] => 0\n| [a, .., z] | [a, z] => a + z"),
        ("fn f(o: Opt[Int]) -> Int\n= match o\n  | some(0 | 1) | none => 0\n  | some(_) => 1", "| some(0 | 1) | none => 0"),
    ];
    for (src, want) in cases {
        roundtrip(src);
        let printed = print_module(&parse(src).unwrap());
        assert!(printed.contains(want), "{src}\n---\n{printed}");
    }
    let (_, notes) = sspur_syntax::parse_noted("fn f(o: Opt[Int], xs: List[Int]) -> Int\n= if let some(v) = o then v else match xs\n  | [x, ...r] => x\n  | _ => 0").unwrap();
    assert_eq!(notes, ["if let p = e -> if e is p", "...rest -> ..rest"]);
    for (src, code) in [("fn f(a: Bool, b: Opt[Int]) -> Int\n= if a and b is some(v) then v else 0", "E_PARSE_IS"), ("fn f(xs: List[Int]) -> Int\n= match xs\n  | [a, .., ..] => a\n  | _ => 0", "E_PARSE_PATTERN")] {
        assert_eq!(parse(src).unwrap_err().code, code, "{src}");
    }
}

#[test]
fn long_lines_continue() {
    let cases = [
        ("fn f(a: Int, b: Int) -> Bool\n= a > 0 and\n  b > 0", "= a > 0 and b > 0"),
        ("fn f(a: Int, b: Int) -> Int\n= a +\n  b", "= a + b"),
        ("fn f(xs: List[Int]) -> Int\n= xs\n  .map(_ * 2)\n  .sum", "= xs.map(_ * 2).sum"),
        ("fn f(a: Int, b: Int) -> Bool\n= a > 0\n  and b > 0\n  or a == b", "= a > 0 and b > 0 or a == b"),
        ("fn f(a: Int) -> Int\n= a\n  - 1", "= a - 1"),
        ("fn f(xs: List[Int]) -> Int\n= do\n  y = xs.map(x => do\n      z = x\n      z + 1)\n    .sum\n  y\n    * 2", "  y * 2"),
        ("fn f(a: Int) -> Int\n= do\n  var x = a\n  x :=\n    x + 1\n  x", "  x := x + 1"),
        ("fn f(o: Opt[Int]) -> Int\n= match o\n  | some(v) => v +\n    1\n  | none => 0", "| some(v) => v + 1\n| none => 0"),
        ("fn f(a: Int) -> Int\n= do\n  b = a\n  -b", "  b = a\n  -b"),
        ("fn f(a: Int) -> Int\n= do\n  -a", "= -a"),
        ("fn f(r: R) -> R\n= r with a := 1,\n  b := 2", "= r with a := 1, b := 2"),
        ("fn f() -> Int\n= do\n  (a, b) =\n    (1, 2)\n  a + b", "  (a, b) = (1, 2)"),
        ("fn f() -> Int\n= do\n  total =\n    1 + 2\n  total", "  total = 1 + 2"),
        ("type S =\n  | A\n  | B{x: Int}", "type S = A | B{x: Int}"),
        ("type S = | A | B", "type S = A | B"),
        ("fn f(p: List[Str]) -> Str\n= \"c {p.join(\\\" -> \\\")}\"", "= \"c {p.join(\" -> \")}\""),
        ("type E = Bad\ntest t = catch do\n  a = 1\n  a == 1\n  | _ => false", "| _ => false"),
    ];
    for (src, want) in cases {
        roundtrip(src);
        let printed = print_module(&parse(src).unwrap_or_else(|e| panic!("{src}\n{e:?}")));
        assert!(printed.contains(want), "{src}\n---\n{printed}");
    }
}

#[test]
fn is_tests_and_guards_print_canonically() {
    let cases = [
        ("fn f(xs: List[Opt[Int]]) -> Int\n= xs.filter(_ is some(_)).len", "= xs.filter(_ is some(_)).len"),
        ("fn f(r: Res[Int, Str]) -> Bool\n= do\n  ok = r is ok(_)\n  ok", "  ok = r is ok(_)"),
        ("fn f(o: Opt[Int]) -> Bool\n= o is some(v) && v > 0", "= o is some(v) and v > 0"),
        ("fn f(o: Opt[Int], g: Bool) -> Bool\n= o is none or g", "= o is none or g"),
        ("fn f(o: Opt[Int], g: Bool) -> Bool\n= g or (o is none)", "= g or (o is none)"),
        ("fn f(o: Opt[Int], g: Bool) -> Bool\n= (o is none) and g", "= (o is none) and g"),
        ("fn f(o: Opt[Int]) -> Bool\n= not (o is none)", "= not (o is none)"),
        ("fn f(o: Opt[Int]) -> Bool\n= (o is none) == true", "= (o is none) == true"),
        ("fn f(a: Int, b: Int) -> Bool\n= a + b is 3 | 4", "= a + b is 3 | 4"),
        ("fn f(a: Opt[Int], b: Opt[Int]) -> Int\n= if a is some(x) and b is some(y) then x + y else 0", "= if a is some(x) and b is some(y) then x + y else 0"),
        ("fn f(a: Opt[Int], b: Opt[Int], c: Bool) -> Int\n= if a is some(x) and b is some(y) and c and x > y then x else 0", "= if a is some(x) and b is some(y) and c and x > y then x else 0"),
        ("fn f(a: Opt[Int], b: Opt[Int]) -> Int\n= if a is none and b is some(y) then y else 0", "= if a is none and b is some(y) then y else 0"),
        ("fn f(a: Opt[Int], b: Opt[Int]) -> Int\n= if a is some(x) and b is none then x else 0", "= if a is some(x) and b is none then x else 0"),
        ("fn f(a: Opt[Int], b: Opt[Int]) -> Unit ! log\n= if a is some(x) and b is some(y) then log(\"{x}{y}\")", "then log(\"{x}{y}\")"),
        ("fn f(a: Opt[Int], b: Opt[Int]) -> Int\n= if a is some(x) and b is some(y) then x else if b is some(z) and a is none then z else 0", "else if b is some(z) and a is none then z else 0"),
        ("fn f(o: Opt[Int]) -> Int\n= do\n  some(v) = o else 0\n  v + 1", "  some(v) = o else 0\n  v + 1"),
        ("fn f(xs: List[Int]) -> Int\n= do\n  [a, ..rest] = xs else 0\n  [b, ..] = rest else a\n  a + b", "  [a, ..rest] = xs else 0\n  [b, ..] = rest else a\n  a + b"),
        ("fn f(o: Opt[Int]) -> Int\n= do\n  some(v) = o\n    else 0\n  v", "  some(v) = o else 0\n  v"),
        ("fn f(o: Opt[Int]) -> Unit ! log\n= do\n  log(\"a\")\n  some(v) = o else ()", "  log(\"a\")\n  some(v) = o else ()"),
        ("fn f(o: Opt[Int]) -> Int\n= do\n  some(v) = o else do\n    w = 1\n    w + 1\n  v", "  some(v) = o else do\n    w = 1\n    w + 1\n  v"),
        ("fn f(xs: List[Opt[Int]]) -> List[Int]\n= xs.map(x => do\n  some(v) = x else 0\n  v * 2)", "xs.map(x => do\n  some(v) = x else 0\n  v * 2)"),
        ("fn f(xs: List[Opt[Int]]) -> Unit ! log\n= do\n  for x in xs\n    some(v) = x else ()\n    log(\"{v}\")\n  ()", "    some(v) = x else ()\n    log("),
        ("fn f(o: Opt[Int]) -> Int\n= do\n  x = 1; some(v) = o else x; v", "  some(v) = o else x\n  v"),
        ("fn f(xs: List[Int]) -> Int ! div\n= do\n  var q = xs\n  var t = 0\n  while q.pop_front is some((x, r)) and x > 0 do\n    t := t + x\n    q := r\n  t", "  while q.pop_front is some((x, r)) and x > 0\n    t := t + x\n    q := r\n  t"),
        ("fn f(a: Opt[Int], b: Opt[Int]) -> Unit ! log, div\n= do\n  while a is some(x) and b is some(y) and x < y\n    log(\"{x}{y}\")\n  ()", "  while a is some(x) and b is some(y) and x < y\n    log("),
        ("fn f(a: Opt[Int]) -> Unit ! log, div\n= do\n  while a is some(_)\n    log(\"x\")\n  ()", "  while a is some(_)\n    log("),
    ];
    for (src, want) in cases {
        roundtrip(src);
        let printed = print_module(&parse(src).unwrap_or_else(|e| panic!("{src}\n{e:?}")));
        assert!(printed.contains(want), "{src}\n---\n{printed}");
    }
    let (_, notes) = sspur_syntax::parse_noted("fn f(o: Opt[Int]) -> Int\n= do\n  let some(x) = o else { return 0 }\n  guard let y = o else { return 1 }\n  x + y").unwrap();
    assert_eq!(notes, ["let p = e else -> p = e else", "p = e else { b } -> p = e else b", "guard let x = e else -> some(x) = e else"]);
    for (src, code) in [
        ("fn f(a: Bool, b: Opt[Int]) -> Bool\n= a and b is some(_)", "E_PARSE_IS"),
        ("fn f(b: Opt[Int]) -> Bool\n= b is some(_) == true", "E_PARSE_IS"),
        ("fn f(b: Opt[Int]) -> Bool\n= b is some(_) is true", "E_PARSE_IS"),
        ("fn f(b: Opt[Int]) -> Int\n= do\n  guard let x = b\n  x", "E_PARSE_GUARD"),
    ] {
        assert_eq!(parse(src).unwrap_err().code, code, "{src}");
    }
}
