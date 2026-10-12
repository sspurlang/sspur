use sspur_syntax::{parse, parse_all, parse_noted, print_module, syntax_hint};

fn hint(src: &str) -> String {
    let e = parse(src).expect_err("should not parse");
    syntax_hint(src, &e).unwrap_or_default()
}

#[test]
fn foreign_syntax_gets_a_hint() {
    assert_eq!(hint("fn f(a: Bool, b: Bool) -> Bool\n= a & b"), "no '&&' operator: write 'and'");
    assert_eq!(hint("fn f() -> Unit ! log fail[E]\n= ()"), "separate effects with commas: '! log, fail[E]'");
    assert!(hint("type E = A{x: Int}\nfn f(e: E) -> Int\n= match e\n  | A{_, y} => 1").starts_with("a bare 'A' pattern ignores the fields"));
    assert_eq!(hint("fn f(xs: List[Int]) -> List[Int]\n= xs.map(x -> x + 1)"), "a lambda is 'x => e' or '(a, b) => e'");
    assert!(hint("fn f() -> Str\n= \"\\d+\"").starts_with("a regex escape doubles the backslash"));
    assert!(hint("fn f(xs: List[Int]) -> List[Int]\n= do\n  ys = xs.map(x => do\n  y = x\n  y)\n  ys").starts_with("indent a lambda's block 2 deeper"));
    assert!(hint("fn f(xs: List[Int]) -> List[Int]\n= do\n  ys = xs.map { x =>\n    x\n  }\n  ys").starts_with("a lambda goes inside the parentheses"));
}

#[test]
fn a_tuple_one_paren_short_around_a_record_update_gets_a_hint() {
    let ty = "type R = {name: Str, items: List[Str]}\ntype G = {room: Str, rooms: List[R], inventory: List[Str]}\n";
    let want = "a ')' here closes more than it opens; a tuple holding a record update is written ((r with a := x, b := y), other)";
    let taken = "(g with rooms := g.rooms.map(r => if r.name == g.room then r with items := r.items.filter(_ != item) else r), inventory := g.inventory.push(item)), \"Taken.\")";
    let one_line = format!("{ty}fn take(g: G, item: Str) -> (G, Str)\n= if g.inventory.contains(item) then {taken} else (g, \"There is no {{item}} here.\")");
    assert_eq!(hint(&one_line), want);
    let in_do = format!("{ty}fn take(g: G, item: Str) -> (G, Str)\n= do\n  n = 1\n  if n > 0 then {taken}\n  else (g, \"no\")");
    assert_eq!(hint(&in_do), want);
    for src in [
        "fn f(x: Int) -> Int\n= (x), 2",
        "type P = {a: Int, b: Int}\nfn f(p: P) -> P\n= (p with a := 1), 2",
        "fn f(x: Int) -> Int\n= g(x), 2",
    ] {
        assert!(!hint(src).contains("closes more than it opens"), "{src}");
    }
}

#[test]
fn parse_all_reports_every_bad_definition() {
    let src = "fn a(x: Int) -> Bool\n= x > 0 ? 1 : 2\n\nfn b() -> Int\n= 1\n\nfn c(x: Bool) -> Bool\n= x -> 2\n";
    let errs = parse_all(src).expect_err("two bad definitions");
    assert_eq!(errs.len(), 2);
    assert!(errs[1].span.start as usize > src.find("fn c").unwrap());
}

#[test]
fn foreign_operators_parse_to_the_canonical_form() {
    for (src, canon, note) in [
        ("fn f(a: Bool, b: Bool) -> Bool\n= a && b || !a\n", "fn f(a: Bool, b: Bool) -> Bool\n= a and b or not a\n", "&& -> and"),
        ("fn f(n: Int) -> Int\n= do\n  var x = 0\n  x += n\n  x\n", "fn f(n: Int) -> Int\n= do\n  var x = 0\n  x := x + n\n  x\n", "x += e -> x := x + e"),
        ("type E = A{x: Int}\n\nfn f(e: E) -> Int\n= match e\n  | A{_} => 1\n", "type E = A{x: Int}\n\nfn f(e: E) -> Int\n= match e\n| A => 1\n", "Ctor{_} pattern -> Ctor"),
        ("fn f(n: Int) -> Int\n= if n > 0 then 1\n  elif n < 0 then 2\n  else 3\n", "fn f(n: Int) -> Int\n= if n > 0 then 1 else if n < 0 then 2 else 3\n", "elif -> else if"),
        ("fn f(pre: Int, post: Int, effect: Int) -> Int\n= pre + post + effect\n", "fn f(pre: Int, post: Int, effect: Int) -> Int\n= pre + post + effect\n", ""),
    ] {
        let (m, notes) = parse_noted(src).unwrap_or_else(|e| panic!("{src}: {e:?}"));
        let printed = print_module(&m);
        assert_eq!(printed, canon, "{src}");
        assert!(note.is_empty() && notes.is_empty() || notes.iter().any(|n| n == note), "{src}: {notes:?}");
        let (m2, again) = parse_noted(&printed).unwrap();
        assert!(again.is_empty() && print_module(&m2) == printed, "the canonical form is a fixed point: {printed}");
    }
}

#[test]
fn block_lambda_variants_parse_to_the_canonical_form() {
    let canon = "fn f(xs: List[Int]) -> List[Int]\n= xs.map(x => do\n  y = x * 2\n  y + 1)\n";
    for (src, note) in [
        ("fn f(xs: List[Int]) -> List[Int]\n= xs.map(x => do\n  y = x * 2\n  y + 1)\n", ""),
        ("fn f(xs: List[Int]) -> List[Int]\n= xs.map(x =>\n  y = x * 2\n  y + 1)\n", ""),
        ("fn f(xs: List[Int]) -> List[Int]\n= xs.map(x => do\n    y = x * 2\n    y + 1\n)\n", ""),
        ("fn f(xs: List[Int]) -> List[Int]\n= xs.map(x => do\n  y = x * 2\n  y + 1\n  )\n", ""),
        ("fn f(xs: List[Int]) -> List[Int]\n= xs.map(\n  x => do\n    y = x * 2\n    y + 1\n)\n", ""),
        ("fn f(xs: List[Int]) -> List[Int]\n= xs.map(x =>\n  do\n    y = x * 2\n    y + 1)\n", ""),
        ("fn f(xs: List[Int]) -> List[Int]\n= xs.map((x) => {\n  const y = x * 2\n  return y + 1\n})\n", "x => { block } -> x => do block"),
        ("fn f(xs: List[Int]) -> List[Int]\n= xs.map(x => do\n  y = x * 2\n  return y + 1)\n", "return e at the end of a lambda -> e"),
    ] {
        let (m, notes) = parse_noted(src).unwrap_or_else(|e| panic!("{src}: {e:?}"));
        assert_eq!(print_module(&m), canon, "{src}");
        assert!(note.is_empty() || notes.iter().any(|n| n == note), "{src}: {notes:?}");
    }
    let rec = "type P = {a: Int, b: Int}\n\nfn f(xs: List[Int]) -> List[P]\n= xs.map(x => {\n  a: x,\n  b: 1})\n";
    assert!(print_module(&parse(rec).unwrap()).contains("xs.map(x => {a: x, b: 1})"));
    let m = parse("fn f(xs: List[Int]) -> List[Int]\n= xs.map(x => match x\n  | 1 => 10\n  | _ => x)\n").unwrap();
    let printed = print_module(&m);
    assert_eq!(printed, "fn f(xs: List[Int]) -> List[Int]\n= xs.map(x => do\n  match x\n  | 1 => 10\n  | _ => x)\n");
    assert_eq!(print_module(&parse(&printed).unwrap()), printed);
}
