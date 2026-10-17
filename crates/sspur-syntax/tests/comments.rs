//! `sspur fmt` keeps every comment next to the code it was written next to (ADR 0042).

use sspur_syntax::{format_source, format_source_at, lexer::lex_comments, parse, print_module};

fn texts(src: &str) -> Vec<String> {
    let (_, cs) = lex_comments(src).unwrap();
    cs.iter().map(|c| src[c.span.start as usize..c.span.end as usize].trim_end().to_string()).collect()
}

/// `src` formats to `want`, which formats to itself, has the same comments and the same
/// canonical text.
fn check(src: &str, want: &str) {
    let got = format_source(src).unwrap();
    assert_eq!(got, want, "\n{got}");
    assert_eq!(format_source(&got).unwrap(), got, "not idempotent:\n{got}");
    assert_eq!(texts(&got), texts(src));
    assert_eq!(print_module(&parse(&got).unwrap()), print_module(&parse(src).unwrap()));
}

#[test]
fn comments_between_and_before_definitions_stay() {
    check(
        "// header\n\n\n// doc for f\nfn f() -> Int = 1\n// doc for g\n\nfn g() -> Int = 2 // g\n  // more about g\n// end\n",
        "// header\n\n// doc for f\nfn f() -> Int\n= 1\n\n// doc for g\n\nfn g() -> Int\n= 2 // g\n  // more about g\n// end\n",
    );
    check("// only\n// comments", "// only\n// comments\n");
    check("profile sys // why\n\n// x\nfn main() -> Int\n= 0 // end", "profile sys // why\n\n// x\nfn main() -> Int\n= 0 // end\n");
}

#[test]
fn comments_inside_bodies_stay_on_their_lines() {
    let src = "fn blk(xs: List[Int]) -> Int\n= do\n  // start\n  var n = 0 // counter\n  for x in xs // each\n    // add it\n    n := n + x\n  // done\n  n\n";
    check(src, src);
    check(
        "fn area(s: Shape) -> F64 // area\n= match s\n  // circles first\n  | Circle{r} => 3.14 * r * r // pi r^2\n  // then rectangles\n  | Rect{w, h} => w * h\n",
        "fn area(s: Shape) -> F64 // area\n= match s\n// circles first\n| Circle{r} => 3.14 * r * r // pi r^2\n// then rectangles\n| Rect{w, h} => w * h\n",
    );
    check(
        "fn sign(n: Int) -> Str\n= if n < 0 then \"neg\" // negative\n  // zero next\n  else if n == 0 then \"zero\"\n  else \"pos\" // positive\n",
        "fn sign(n: Int) -> Str\n= if n < 0 then \"neg\" // negative\n  // zero next\n  else if n == 0 then \"zero\"\n  else \"pos\" // positive\n",
    );
}

#[test]
fn a_comment_inside_a_short_line_breaks_it_where_the_comment_was() {
    check("fn f() -> List[Int]\n= [1, // one\n  2]\n", "fn f() -> List[Int]\n= [\n  1, // one\n  2,\n]\n");
    check("fn f() -> List[Int]\n= [\n  1,\n  // two next\n  2, // two\n]\n", "fn f() -> List[Int]\n= [\n  1,\n  // two next\n  2, // two\n]\n");
    check("type P = {\n  x: Int, // the x\n  y: Int,\n}\n", "type P = {\n  x: Int, // the x\n  y: Int,\n}\n");
    check("type C =\n  | Red // warm\n  | Green // calm\n  | Blue\n", "type C =\n  | Red // warm\n  | Green // calm\n  | Blue\n");
    check("fn up(p: P) -> P\n= p with a := 1, // one\n  b := 2\n", "fn up(p: P) -> P\n= p with\n  a := 1, // one\n  b := 2\n");
    check("fn c(xs: List[Int]) -> Int\n= xs // all\n  .filter(x => x > 0) // positive\n  .sum\n", "fn c(xs: List[Int]) -> Int\n= xs // all\n  .filter(x => x > 0) // positive\n  .sum\n");
    check("fn c(a: Bool, b: Bool) -> Bool\n= a and // first\n  b\n", "fn c(a: Bool, b: Bool) -> Bool\n= a and // first\n  b\n");
    check(
        "fn add(a: Int, // left\n  b: Int) -> Int\n= a + b\n",
        "fn add(\n  a: Int, // left\n  b: Int,\n) -> Int\n= a + b\n",
    );
}

#[test]
fn comments_survive_narrow_widths_and_foreign_spellings() {
    let src = "fn f(o: Opt[Int]) -> Int\n= do\n  let x = 1 // x\n  // y next\n  let y = x + 1\n  if x > 0 && y > 0 then x else y // done\n";
    for w in [16, 40, 100] {
        let out = format_source_at(src, w).unwrap();
        assert_eq!(texts(&out), texts(src), "{w}:\n{out}");
        assert_eq!(format_source_at(&out, w).unwrap(), out, "{w}");
        assert_eq!(print_module(&parse(&out).unwrap()), print_module(&parse(src).unwrap()));
    }
    let out = format_source(src).unwrap();
    assert!(out.contains("  x = 1 // x\n  // y next\n  y = x + 1\n"), "{out}");
}

#[test]
fn comment_like_text_in_strings_is_code() {
    let src = "fn u() -> Str\n= \"https://example.com\" // a url\n";
    check(src, src);
}
