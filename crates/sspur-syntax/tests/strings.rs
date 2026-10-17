//! How string literals read (ADR 0042): when `{` interpolates, escapes, `\u{...}`.

use sspur_syntax::{parse, parse_expr, print_module, Def, ExprKind, StrPart};

/// The parts of the string expression `src`: literal text, or `{...}` with the printed code.
fn parts(src: &str) -> Vec<String> {
    let e = parse_expr(src).unwrap_or_else(|e| panic!("{src}: {e:?}"));
    let ExprKind::Str(ps) = e.kind else { panic!("{src}: not a string: {:?}", e.kind) };
    ps.iter()
        .map(|p| match p {
            StrPart::Lit(s) => s.clone(),
            StrPart::Expr(x) => format!("{{{}}}", sspur_syntax::printer::expr(x, 0)),
        })
        .collect()
}

#[test]
fn a_brace_interpolates_only_an_expression() {
    assert_eq!(parts(r#""a {x} b""#), ["a ", "{x}", " b"]);
    assert_eq!(parts(r#""{ x.len }""#), ["{x.len}"]);
    assert_eq!(parts(r#""{"-".repeat(3)}""#), ["{\"-\".repeat(3)}"]);
    assert_eq!(parts(r#""{xs.join(\" -> \")}""#), ["{xs.join(\" -> \")}"]);
    // Literal text: empty, not an expression, a lone literal, or starting with `{`.
    for (src, text) in [
        (r#""{}""#, "{}"),
        (r#""a{3}""#, "a{3}"),
        (r#""^a{2,3}$""#, "^a{2,3}$"),
        (r#"".cal{display:grid;gap:2px}""#, ".cal{display:grid;gap:2px}"),
        (r#""{{x}}""#, "{{x}}"),
        (r#""{{""#, "{{"),
        (r#""}}""#, "}}"),
        (r#""{\"a\":1}""#, "{\"a\":1}"),
        (r#""{x\\y}""#, "{x\\y}"),
        (r#""\{id}""#, "{id}"),
        (r#""\}""#, "}"),
    ] {
        assert_eq!(parts(src), [text], "{src}");
    }
}

#[test]
fn a_brace_string_never_swallows_the_code_after_it() {
    // `"{"` is one string even when a `"}"` follows on the line.
    let e = parse_expr(r#"s.contains("{") or s.contains("}")"#).unwrap();
    assert!(matches!(e.kind, ExprKind::Binary(..)), "{:?}", e.kind);
    let e = parse_expr(r#""{{" + x + "}}""#).unwrap();
    assert!(matches!(e.kind, ExprKind::Binary(..)), "{:?}", e.kind);
    let e = parse_expr(r#"s.replace("{", "").replace("}", "")"#).unwrap();
    assert!(matches!(e.kind, ExprKind::Method { .. }), "{:?}", e.kind);
    let e = parse_expr(r#""{a" + x + "b}""#).unwrap();
    assert!(matches!(e.kind, ExprKind::Binary(..)), "{:?}", e.kind);
}

#[test]
fn escapes_and_unicode() {
    assert_eq!(parts(r#""\\{x}""#), ["\\", "{x}"]);
    assert_eq!(parts(r#""\u{e9}t\u{E9}\u{1F600}""#), ["\u{e9}t\u{e9}\u{1F600}"]);
    assert_eq!(parts(r#""\n\t\r\"\\""#), ["\n\t\r\"\\"]);
    for bad in [r#""\u{}""#, r#""\u{110000}""#, r#""\u{d800}""#, r#""\u{zz}""#, r#""\u{1234567}""#, r#""\q""#] {
        let e = parse_expr(bad).unwrap_err();
        assert_eq!(e.code, "E_LEX_ESCAPE", "{bad}: {e:?}");
    }
}

#[test]
fn strings_print_back_as_the_same_strings() {
    for src in [
        "fn f(x: Int) -> Str\n= \"a {x} \\{b} } \\\\{x} \\u{1b}[0m\"\n",
        "fn f(s: Str) -> Bool\n= s.contains(\"\\{\") or s.contains(\"}\")\n",
        "fn f(s: Str) -> Str\n= match s\n| \"\\{x}\" => \"{s}\"\n| _ => \"\"\n",
    ] {
        let m = parse(src).unwrap();
        let printed = print_module(&m);
        assert_eq!(printed, src);
        assert_eq!(print_module(&parse(&printed).unwrap()), printed);
    }
    // An interpolated literal (only a rewrite makes one) keeps reading as an interpolation.
    let mut m = parse("fn f() -> Str\n= \"{x}\"\n").unwrap();
    let Def::Fn(f) = &mut m.defs[0] else { panic!() };
    let ExprKind::Str(ps) = &mut f.body.kind else { panic!() };
    let StrPart::Expr(x) = &mut ps[0] else { panic!() };
    x.kind = ExprKind::Int(3);
    let printed = print_module(&m);
    assert_eq!(printed, "fn f() -> Str\n= \"{(3)}\"\n");
    assert_eq!(print_module(&parse(&printed).unwrap()), printed);
}

#[test]
fn route_and_asm_strings_keep_their_braces() {
    let m = parse("fn get_user(id: Int) -> Str\n= \"\"\n\nsvc api\n  ep get \"/users/{id}/posts/{post:int}\" = get_user\n").unwrap();
    let Def::Svc(s) = &m.defs[1] else { panic!() };
    assert_eq!(s.eps[0].path, "/users/{id}/posts/{post:int}");
}

#[test]
fn an_interpolated_string_literal_still_interpolates() {
    assert_eq!(parts(r#""{"x"} {"-".repeat(2)}""#), ["{\"x\"}", " ", "{\"-\".repeat(2)}"]);
    for src in [r#"s == "{" or s == "}""#, r#"f("{", "}")"#, r#"["{", "}"]"#, r#""{" + x + "}""#] {
        let e = parse_expr(src).unwrap();
        assert!(!matches!(e.kind, ExprKind::Str(_)), "{src}: {:?}", e.kind);
    }
}

#[test]
fn interpolated_literals_print_so_they_still_interpolate() {
    for (lit, want) in [("\"x\"", "\"{\"x\"}\""), ("\")\"", "\"{(\")\")}\""), ("7", "\"{(7)}\"")] {
        let mut m = parse("fn f() -> Str\n= \"{x}\"\n").unwrap();
        let Def::Fn(f) = &mut m.defs[0] else { panic!() };
        f.body.kind = ExprKind::Str(vec![StrPart::Expr(parse_expr(lit).unwrap())]);
        let printed = print_module(&m);
        assert_eq!(printed, format!("fn f() -> Str\n= {want}\n"));
        assert_eq!(print_module(&parse(&printed).unwrap()), printed);
    }
}
