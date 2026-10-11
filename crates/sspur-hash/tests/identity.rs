use sspur_hash::hash_module;
use sspur_syntax::parse;
use std::collections::HashMap;

fn hashes(src: &str) -> HashMap<String, String> {
    hash_module(&parse(src).unwrap()).into_iter().collect()
}

#[test]
fn renaming_a_definition_keeps_its_hash() {
    let a = hashes("fn double(x: Int) -> Int\n= x * 2");
    let b = hashes("fn twice(x: Int) -> Int\n= x * 2");
    assert_eq!(a["double"], b["twice"]);
}

#[test]
fn renaming_locals_and_params_keeps_hash() {
    let a = hashes("fn f(x: Int) -> Int\n= do\n  y = x + 1\n  y * y");
    let b = hashes("fn f(n: Int) -> Int\n= do\n  m = n + 1\n  m * m");
    assert_eq!(a["f"], b["f"]);
}

#[test]
fn formatting_does_not_matter() {
    let a = hashes("fn f() -> Int ! log\n= log(\"a\"); 1");
    let b = hashes("fn f() -> Int ! log\n= do\n  log(\"a\")\n  1");
    assert_eq!(a["f"], b["f"]);
}

#[test]
fn changing_meaning_changes_hash() {
    let a = hashes("fn f(x: Int) -> Int\n= x * 2");
    let b = hashes("fn f(x: Int) -> Int\n= x * 3");
    assert_ne!(a["f"], b["f"]);
    let c = hashes("fn f(x: Int) -> Int ! log\n= x * 2");
    assert_ne!(a["f"], c["f"]);
}

#[test]
fn dependents_change_when_dependencies_change() {
    let a = hashes("fn g(x: Int) -> Int\n= x + 1\nfn f(x: Int) -> Int\n= g(x) * 2");
    let b = hashes("fn g(x: Int) -> Int\n= x + 2\nfn f(x: Int) -> Int\n= g(x) * 2");
    assert_ne!(a["f"], b["f"]);
    let c = hashes("fn h(x: Int) -> Int\n= x + 1\nfn f(x: Int) -> Int\n= h(x) * 2");
    assert_eq!(a["f"], c["f"], "renaming the dependency must not change the dependent");
}

#[test]
fn mutual_recursion_is_stable_under_renaming() {
    let a = hashes("fn even(n: Int) -> Bool\n= if n == 0 then true else odd(n - 1)\nfn odd(n: Int) -> Bool\n= if n == 0 then false else even(n - 1)");
    let b = hashes("fn is_e(n: Int) -> Bool\n= if n == 0 then true else is_o(n - 1)\nfn is_o(n: Int) -> Bool\n= if n == 0 then false else is_e(n - 1)");
    assert_eq!(a["even"], b["is_e"]);
    assert_eq!(a["odd"], b["is_o"]);
    assert_ne!(a["even"], a["odd"]);
}

#[test]
fn constructor_renames_do_not_change_hashes() {
    let a = hashes("type S = A | B\nfn f(s: S) -> Int\n= match s\n  | A => 1\n  | B => 2");
    let b = hashes("type T = X | Y\nfn f(s: T) -> Int\n= match s\n  | X => 1\n  | Y => 2");
    assert_eq!(a["S"], b["T"]);
    assert_eq!(a["f"], b["f"]);
}

#[test]
fn hashes_are_52_char_base32() {
    let a = hashes("fn f() -> Int\n= 1");
    assert_eq!(a["f"].len(), 52);
    assert!(a["f"].chars().all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c)));
}

#[test]
fn examples_are_part_of_identity() {
    let a = hashes("fn f(x: Int) -> Int\n  ex f(1) == 2\n= x * 2");
    let b = hashes("fn f(x: Int) -> Int\n  ex f(2) == 4\n= x * 2");
    let c = hashes("fn g(y: Int) -> Int\n  ex g(1) == 2\n= y * 2");
    assert_ne!(a["f"], b["f"]);
    assert_eq!(a["f"], c["g"], "self-references in examples are rename-stable");
}

#[test]
fn richer_patterns_hash_by_meaning() {
    let h = |src: &str| hashes(src)["f"].clone();
    let swap = h("fn f(p: (Int, Int)) -> Int\n= match p\n  | (a, 0) | (0, a) => a\n  | _ => 1");
    assert_eq!(swap, h("fn f(q: (Int, Int)) -> Int\n= match q\n  | (b, 0) | (0, b) => b\n  | _ => 1"));
    let order = |pat: &str| h(&format!("fn f(p: (Int, Int)) -> Int\n= match p\n  | {pat} => a - b\n  | _ => 1"));
    assert_ne!(order("(a, b) | (b, a)"), order("(a, b) | (a, b)"));
    let lists = ["[x, ..r]", "[..r, x]", "[x, .., y]", "[x, y]"].map(|p| h(&format!("fn f(xs: List[Int]) -> Int\n= match xs\n  | {p} => 1\n  | _ => 0")));
    assert!(lists.iter().enumerate().all(|(i, a)| lists[i + 1..].iter().all(|b| a != b)));
    let is = h("fn f(o: Opt[Int]) -> Int\n= if o is some(v) then v else 0");
    assert_eq!(is, h("fn f(o: Opt[Int]) -> Int\n= if let some(w) = o then w else 0"));
    assert_ne!(is, h("fn f(o: Opt[Int]) -> Int\n= match o\n  | some(v) => v\n  | _ => 0"));
}

#[test]
fn is_tests_and_guards_hash_as_written() {
    let h = |s: &str| hashes(s)["f"].clone();
    let test = h("fn f(o: Opt[Int]) -> Bool\n= o is some(_)");
    assert_ne!(test, h("fn f(o: Opt[Int]) -> Bool\n= match o\n  | some(_) => true\n  | _ => false"), "the hash keeps how a test was written");
    assert_eq!(h("fn f(o: Opt[Int]) -> Bool\n= o is some(v) and v > 0"), h("fn f(p: Opt[Int]) -> Bool\n= p is some(w) &&\n  w > 0"));
    let chain = h("fn f(a: Opt[Int], b: Opt[Int]) -> Int\n= if a is some(x) and b is some(y) then x + y else 0");
    assert_eq!(chain, h("fn f(p: Opt[Int], q: Opt[Int]) -> Int\n= if p is some(m) and q is some(n) then m + n else 0"));
    assert_ne!(chain, h("fn f(a: Opt[Int], b: Opt[Int]) -> Int\n= if a is some(x) and b is some(y) then y + x else 0"));
    let guard = h("fn f(o: Opt[Int]) -> Int\n= do\n  some(v) = o else 0\n  v + 1");
    assert_eq!(guard, h("fn f(p: Opt[Int]) -> Int\n= do\n  let some(w) = p else { 0 }\n  w + 1"));
    assert_ne!(guard, h("fn f(o: Opt[Int]) -> Int\n= match o\n  | some(v) => v + 1\n  | _ => 0"));
}
