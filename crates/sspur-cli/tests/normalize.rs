//! Tolerant input, canonical storage: `edit` accepts foreign spellings that have one meaning,
//! stores the SSPUR form, and says what it rewrote.

use std::io::Write;
use std::process::{Command, Stdio};

fn sspur(dir: &std::path::Path, args: &[&str], stdin: &str) -> (String, bool) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sspur")).args(args).current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    (String::from_utf8_lossy(&o.stdout).trim_end().to_string() + &String::from_utf8_lossy(&o.stderr), o.status.success())
}

const BASE: &str = "type E = Bad{n: Int}\n\ntype Row = {name: Str, rate: Int}\n\nfn pay(r: Row) -> Int\n= r.rate * 2\n\nfn check(n: Int) -> Int ! fail[E]\n= if n < 0 then raise Bad{n: n} else n\n\nfn main() -> Unit ! log\n= log(\"hi\")\n";

fn fresh(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sspur-norm-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("a.ssp"), BASE).unwrap();
    assert!(sspur(&d, &["init", "a.ssp"], "").1);
    std::fs::remove_file(d.join("a.ssp")).unwrap();
    d
}

/// (foreign input, the stored definition, the note).
const CASES: &[(&str, &str, &str)] = &[
    ("fn f(a: Bool, b: Bool) -> Bool\n= a && !b || a\n", "fn f(a: Bool, b: Bool) -> Bool\n= a and not b or a", "&& -> and"),
    ("fn f(a: Bool, b: Bool) -> Bool\n= !a == b\n", "fn f(a: Bool, b: Bool) -> Bool\n= (not a) == b", "!x -> not x"),
    ("fn f(n: Int) -> Int\n= if n > 1 then 2 elif n > 0 then 1 else 0\n", "fn f(n: Int) -> Int\n= if n > 1 then 2 else if n > 0 then 1 else 0", "elif -> else if"),
    ("fn f(n: Int) -> Int\n= do\n  let m = n + 1\n  let mut k = m\n  k += 2\n  k\n", "fn f(n: Int) -> Int\n= do\n  m = n + 1\n  var k = m\n  k := k + 2\n  k", "let x = e -> x = e"),
    ("fn f(e: E) -> Bool\n= match e\n  | Bad{_} => true\n", "fn f(e: E) -> Bool\n= match e\n| Bad => true", "Ctor{_} pattern -> Ctor"),
    ("fn f(queue: List[Int], store: Int) -> Int\n= queue.len + store\n", "fn f(queue: List[Int], store: Int) -> Int\n= queue.len + store", ""),
    ("fn f(o: Opt[Int]) -> Int\n= match o\n  | Some(x) => x\n  | None => 0\n", "fn f(o: Opt[Int]) -> Int\n= match o\n| some(x) => x\n| none => 0", "Some -> some"),
    ("fn f(n: Int) -> Opt[Int]\n= if n > 0 then Some(n) else None\n", "fn f(n: Int) -> Opt[Int]\n= if n > 0 then some(n) else none", "None -> none"),
    ("fn f() -> Bool\n= True\n", "fn f() -> Bool\n= true", "True -> true"),
    ("fn f(xs: List[Int], s: Str) -> Int\n= len(xs) + s.length + xs.size\n", "fn f(xs: List[Int], s: Str) -> Int\n= xs.len + s.len + xs.len", "len(x) -> x.len"),
    ("fn f(s: Str, i: Int) -> Str\n= s.slice(0, i) + s.slice(i + 1, s.len) + s.substring(2)\n", "fn f(s: Str, i: Int) -> Str\n= s.take(i) + s.drop(i + 1).take(s.len - (i + 1)) + s.drop(2)", "s.slice(a, b) -> s.drop(a).take(b - a)"),
    ("fn f(s: Str) -> Bool\n= s.startsWith(\"a\") and s.toLowerCase.endsWith(\"b\") and s.strip.isEmpty\n", "fn f(s: Str) -> Bool\n= s.starts_with(\"a\") and s.lower.ends_with(\"b\") and s.trim.is_empty", ".startsWith -> .starts_with"),
    ("fn f(n: Int) -> Str\n= n.to_string\n", "fn f(n: Int) -> Str\n= n.str", ".to_string -> .str"),
    ("fn f(o: Opt[Int]) -> Int\n= o.unwrap_or(0) + o.unwrap\n", "fn f(o: Opt[Int]) -> Int\n= o.or(0) + o.get", ".unwrap_or -> .or"),
    ("fn f(o: Opt[Int]) -> Int ! fail[E]\n= do\n  v = o.ok_or(Bad{n: 0})\n  v.get\n", "fn f(o: Opt[Int]) -> Int ! fail[E]\n= do\n  v = o.ok_or(Bad{n: 0})\n  v", "v.get -> v (it is already Int)"),
    ("fn f(rs: List[Row]) -> List[Row]\n= rs.sort_by((-pay(_), _.name))\n", "fn f(rs: List[Row]) -> List[Row]\n= rs.sort_by(x => (-pay(x), x.name))", "f(g(_)) -> x => f(g(x))"),
    ("fn f(rs: List[Row]) -> List[Row]\n= rs.sort_by((-len(_.name), _.name))\n", "fn f(rs: List[Row]) -> List[Row]\n= rs.sort_by((-_.name.len, _.name))", "len(x) -> x.len"),
    ("fn f(n: Int) -> Int\n= check(n) + 1\n", "fn f(n: Int) -> Int ! fail[E]\n= check(n) + 1", "f now declares fail[E]"),
    ("fn f() -> Unit\n= print(\"x\")\n", "fn f() -> Unit ! log\n= log(\"x\")", "print -> log"),
    ("fn f(n: Int) -> Int\n= do\n  var a: Int = n\n  b: Int = a + 1\n  b\n", "fn f(n: Int) -> Int\n= do\n  var a = n\n  b = a + 1\n  b", "var x: T = e -> var x = e"),
    ("fn f(xs: List[Int]) -> List[Int]\n= xs.map((x) => {\n  y = x * 2\n  return y + 1\n})\n", "fn f(xs: List[Int]) -> List[Int]\n= xs.map(x => do\n  y = x * 2\n  y + 1)", "x => { block } -> x => do block"),
    ("fn f(o: Opt[Int]) -> Int\n= if let some(x) = o then x else 0\n", "fn f(o: Opt[Int]) -> Int\n= if o is some(x) then x else 0", "if let p = e -> if e is p"),
    ("fn f(xs: List[Int]) -> Int\n= match xs\n  | [x, ...rest] => x + rest.len\n  | [] => 0\n", "fn f(xs: List[Int]) -> Int\n= match xs\n| [x, ..rest] => x + rest.len\n| [] => 0", "...rest -> ..rest"),
    ("fn f(xs: List[Int]) -> Int\n= match xs\n  | [x, *rest] => x + rest.len\n  | [] => 0\n", "fn f(xs: List[Int]) -> Int\n= match xs\n| [x, ..rest] => x + rest.len\n| [] => 0", "*rest -> ..rest"),
    ("fn f(o: Opt[Int]) -> Int\n= do\n  let some(x) = o else { return 0 }\n  x + 1\n", "fn f(o: Opt[Int]) -> Int\n= do\n  some(x) = o else return 0\n  x + 1", "let p = e else -> p = e else"),
    ("fn f(o: Opt[Int]) -> Int\n= do\n  guard let x = o else {\n    return 0\n  }\n  x\n", "fn f(o: Opt[Int]) -> Int\n= do\n  some(x) = o else return 0\n  x", "guard let x = e else -> some(x) = e else"),
    ("fn f(n: Int) -> Int ! fail[E] ! log\n= do\n  log(\"x\")\n  check(n)\n", "fn f(n: Int) -> Int ! fail[E], log\n= do\n  log(\"x\")\n  check(n)", "! a ! b -> ! a, b"),
    ("fn f(o: Opt[Int], g: Bool) -> Bool\n= o is some(v) && v > 0 || g\n", "fn f(o: Opt[Int], g: Bool) -> Bool\n= o is some(v) and (v > 0 or g)", "&& -> and"),
];

const FOREIGN: &[&str] = &["if let", "...", "*rest", "&&", "||", " !", "elif", "let ", "+=", "{_}", "None", "Some(", "True", ".length", ".slice(", ".size", ".to_string", ".unwrap", "print(", "len("];

#[test]
fn foreign_spellings_are_stored_canonical_and_round_trip() {
    let d = fresh("cases");
    for (input, stored, note) in CASES {
        let (out, ok) = sspur(&d, &["edit", "--interp"], input);
        assert!(ok, "{input}\n{out}");
        if !note.is_empty() {
            assert!(out.contains("stored as: ") && out.contains(note), "{input}\n{out}");
        } else {
            assert!(!out.contains("stored as"), "{input}\n{out}");
        }
        let (body, _) = sspur(&d, &["q", "body", "f"], "");
        assert_eq!(body, *stored, "{input}");
        let rest: String = body.lines().skip(1).collect::<Vec<_>>().join("\n");
        for f in FOREIGN {
            assert!(!rest.contains(f), "printer emitted '{f}': {body}");
        }
        let (again, ok) = sspur(&d, &["edit", "--interp"], &format!("{stored}\n"));
        assert!(ok && !again.contains("stored as"), "the canonical form is accepted as is: {again}");
        assert_eq!(sspur(&d, &["q", "body", "f"], "").0, *stored, "canonical text is a fixed point");
    }
}

#[test]
fn ambiguous_spellings_stay_errors_with_hints() {
    let d = fresh("ambiguous");
    for (input, hint) in [
        ("fn f(xs: List[Int]) -> Opt[Int]\n= xs.head\n", "'.first' (an Opt)"),
        ("fn f(s: Str) -> Opt[Str]\n= s.get\n", ""),
        ("test t = pay(_) == 2\n", "write 'x => f(g(x))'"),
    ] {
        let (out, ok) = sspur(&d, &["edit"], input);
        assert!(!ok && out.starts_with("rejected, nothing changed"), "{input}\n{out}");
        assert!(out.contains(hint), "{input}\n{out}");
    }
}

#[test]
fn a_user_definition_wins_over_a_normalization() {
    let d = fresh("user");
    let (out, ok) = sspur(&d, &["edit"], "type T = None | Some{x: Int}\n\nfn len(r: Row) -> Int\n= 7\n\nfn f(t: T, r: Row) -> Int\n= match t\n  | None => len(r)\n  | Some{x} => x\n");
    assert!(ok && !out.contains("stored as"), "{out}");
}

#[test]
fn catch_of_a_non_bool_in_a_test_is_stored_canonical() {
    let d = fresh("catch");
    let (out, ok) = sspur(&d, &["edit", "--test", "--interp"], "test t = catch check(-1)\n  | Bad{n} => n == -1\n");
    assert!(ok && out.contains("stored as: catch e | .. with a non-Bool e in a test -> catch do (_ = e) false | .."), "{out}");
    let (body, _) = sspur(&d, &["q", "body", "t"], "");
    assert_eq!(body, "test t = catch do\n  _ = check(-1)\n  false\n| Bad{n} => n == -1");
    let (again, ok) = sspur(&d, &["edit", "--interp"], &format!("{body}\n"));
    assert!(ok && !again.contains("stored as"), "{again}");
}

#[test]
fn catch_with_only_false_arms_in_a_test_expects_success() {
    let d = fresh("catchok");
    let (out, ok) = sspur(&d, &["edit", "--test", "--interp"], "test ok = catch check(1)\n  | Bad{n} => false\n\ntest bad = catch check(-1)\n  | Bad{n} => false\n");
    assert!(!ok && out.contains("FAIL  bad: raised Bad{n: -1}") && out.contains("1 passed, 1 failed"), "{out}");
    assert!(out.contains("stored as: catch e | .. with a non-Bool e and only false arms in a test -> catch do (_ = e) true | .."), "{out}");
    let (body, _) = sspur(&d, &["q", "body", "ok"], "");
    assert_eq!(body, "test ok = catch do\n  _ = check(1)\n  true\n| Bad{n} => false");
}

const ORDER: &str = "type Order = {qty: Int, price: Int}\n\nfn subtotal(o: Order) -> Int\n= o.qty * o.price\n\nfn total(o: Order) -> Int\n= subtotal(o) + 1\n\ntest subtotal_sample = subtotal(Order{qty: 2, price: 3}) == 6\n";

fn orders(name: &str) -> std::path::PathBuf {
    let d = fresh(name);
    assert!(sspur(&d, &["edit"], ORDER).1);
    d
}

#[test]
fn remove_plus_the_same_definition_under_a_new_name_is_a_rename() {
    let d = orders("rename");
    let (out, ok) = sspur(&d, &["edit", "--test", "--interp"], "remove subtotal\n\nfn gross(o: Order) -> Int\n= o.qty  *  o.price\n");
    assert!(ok && out.starts_with("ok subtotal->gross\n") && out.contains("1 passed"), "{out}");
    assert_eq!(sspur(&d, &["q", "body", "subtotal_sample"], "").0, "test subtotal_sample = gross(Order{qty: 2, price: 3}) == 6");
    assert_eq!(sspur(&d, &["q", "body", "total"], "").0, "fn total(o: Order) -> Int\n= gross(o) + 1");

    let d = orders("rename2");
    let (out, ok) = sspur(&d, &["edit"], "remove subtotal\nremove subtotal_sample\n\nfn gross(o: Order) -> Int\n= o.qty * o.price\n\ntest gross_sample = gross(Order{qty: 2, price: 3}) == 6\n\nfn total(o: Order) -> Int\n= gross(o) + 2\n");
    assert!(ok && out.starts_with("ok subtotal->gross subtotal_sample->gross_sample ~total"), "{out}");
    assert_eq!(sspur(&d, &["q", "body", "total"], "").0, "fn total(o: Order) -> Int\n= gross(o) + 2");
}

#[test]
fn an_ambiguous_or_changed_removal_is_not_a_rename() {
    let d = orders("norename");
    for edit in [
        "remove subtotal\n\nfn gross(o: Order) -> Int\n= o.qty * o.price\n\nfn net(o: Order) -> Int\n= o.qty * o.price\n",
        "remove subtotal\n\nfn gross(o: Order) -> Int\n= o.qty * o.price + 1\n",
        "remove subtotal\n",
    ] {
        let (out, ok) = sspur(&d, &["edit"], edit);
        assert!(!ok && out.contains("subtotal_sample:1:24 E_UNKNOWN_NAME unknown name 'subtotal'"), "{edit}\n{out}");
        assert!(out.contains("hint: 'subtotal_sample' still uses the removed 'subtotal': write 'rename subtotal NEW' instead of 'remove subtotal'"), "{out}");
    }
}
