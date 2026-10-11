# ADR 0029: Or-patterns, list patterns and `if e is p`

Status: accepted, 2026-10-10

## Problem

Patterns stopped at constructors, tuples and literals. Two variants that share an arm needed the arm written twice, a list could only be taken apart with `.first`, `.len` and indexing (each a separate check the reader has to connect), and testing one case of an `Opt` or a sum took a four-line `match` with a `| _ =>` arm. Agents reach for all three constantly, because Rust, Swift, Python, Haskell, Kotlin and C# have them. The exhaustiveness check also looked one level deep: `match o | some(some(x)) => .. | some(none) => .. | none => ..` was reported as missing `some(_)`, and nothing reported an arm that could never match.

## Syntax

```
fn radius(s: Shape) -> F64
= match s
  | Circle{r} | Ring{r} => r
  | Dot => 0.0

fn sum(xs: List[Int]) -> Int
= match xs
  | [] => 0
  | [x, ..rest] => x + sum(rest)

fn ends(xs: List[Int]) -> Int
= match xs
  | [first, .., last] => first + last
  | [x] => x
  | [] => 0

fn name_or(u: Opt[User], d: Str) -> Str
= if u is some(v) and v.name != "" then v.name else d
```

## Decisions

| # | Decision | Why |
|---|---|---|
| 1 | An or-pattern is `p1 \| p2`, at the top of an arm (`\| A \| B => e`) or nested anywhere (`some(0 \| 1)`, `(Red \| Blue, _)`). The first alternative that matches binds | The arm already starts with `\|`, so a second `\|` on the same line reads as "or this". It is the spelling of Rust, Python, OCaml and Swift |
| 2 | Every alternative binds the same names with the same types, or it is `E_PATTERN_OR_BINDS`, with a hint naming the missing name or the two types | A name bound in only one alternative would be unbound in the body whenever another one matched. Requiring equal types keeps one C type per binding |
| 3 | List patterns are `[]`, `[x]`, `[x, y]`, `[x, ..rest]`, `[..init, last]`, `[first, .., last]` and `[_, ..]`: elements, then at most one `..` that matches any number of elements and optionally names them. Elements are full patterns (`[ok(x), ..]`, `[(a, b), ..rest]`, `["GET", path]`, `[0, ..]`) | Python and Rust spell it this way (Rust with `@`), and `..` is SSPUR's range token already. A second `..` would be ambiguous, so it is a parse error with a hint |
| 4 | `rest` is a `List[T]`. In native code it is a view of the matched list, `{len - k, data + h, hdr}`, the representation `drop` already returns, so binding it allocates nothing and `[x, ..rest] => f(rest)` recursion is linear; the interpreter copies the slice | Speed in the tier that matters, with no new runtime type and no change to how lists are freed or reused in place |
| 5 | `if e is p then a else b` matches one pattern, with an optional guard after `and` (`if o is some(v) and v > 0 then`), the names bound in `then` only, and `else` optional when `then` is `Unit`. It chains as `else if y is q then` | One line instead of a four-line `match` with `\| _ =>`, with the value first as in `match e`. `is` is the spelling of C#, Kotlin and Dart. `and` for the guard keeps it a reading of the condition; a match arm's `if` guard would read as a nested `if` here |
| 6 | `is` is canonical; `if let p = e then` (Rust, Swift) is accepted and stored as `if e is p` with the note `if let p = e -> if e is p`, the existing tolerant-input mechanism | `if let` costs two more tokens, puts the pattern before the value it tests, and uses `=`, which in SSPUR is a binding that cannot fail. One spelling in the codebase, while the first try of an agent used to Rust still lands |
| 7 | `is` takes the whole expression before it, so `if a and b is p` is `E_PARSE_IS`, with a hint to test the pattern first or parenthesize the value. The printer parenthesizes such a value, `if (a and b) is true then`. `is` is a contextual word, so existing names `is` keep working | The alternative, `is` binding tighter than `and`, would make `a and b is p` silently test only `b` while the names it binds leak into a condition that may be false |
| 8 | `if e is p` is a two-arm `match` (`p` then `_`) marked as written with `is`. The checker, interpreter, native code, ownership checker and SMT translation see an ordinary `match`; only the printer and the content hash look at the mark | No new semantics in any tier, so no new parity risk, while the canonical text and the hash keep the form the author wrote |
| 9 | Exhaustiveness and redundancy use the usefulness algorithm (Maranget) over constructors: sums, `Bool`, `Opt`, `Res`, tuples and records, with or-patterns expanded, literals as an open set, and lists split into lengths `0..n-1` and "at least `n`", where `n` comes from the longest fixed pattern and the widest `..` pattern. `[]` plus `[x, ..rest]` is exhaustive. Missing cases are listed as patterns (`some(none)`, `[_, _, ..]`, `(false, false)`, `B{y: none}`) | One algorithm for every shape, nested to any depth, instead of the one-level check it replaces; it accepts every program the old check accepted |
| 10 | An arm that earlier unguarded arms cover, or one alternative of an or-pattern that earlier patterns cover, is the warning `W_ARM_UNREACHABLE`; for `if e is p` it says the pattern always matches. It is a warning, not an error | A dead arm is harmless at run time and often a leftover while editing; the rule tables keep their `E_RULE_SHADOWED` error because a row there is a business rule that silently never applies |
| 11 | Native matching stays a chain of tests the C compiler turns into a decision tree: a sum test is one tag compare, a list test one length compare, element tests index the array directly, and an or-pattern is `(a \|\| b)` whose alternatives assign the bindings into locals declared before the test. The Cranelift JIT does not take `match` and falls back as before; strict native (`SSPUR_STRICT_NATIVE=1`) compiles all of it | No allocation and no runtime helper per match, and clang's switch lowering applies |
| 12 | The fuzzer generates list patterns with rests and literals, or-patterns of variants and of integers (nested in tuples), and `if e is p` with and without a guard, and compares the interpreter with native code | The new forms get the same differential testing as the old ones |
| 13 | Also accepted and stored canonically: `[x, ...rest]` (JavaScript) and `[x, *rest]` (Python) as `[x, ..rest]` | Two more spellings agents produce, with one meaning |

## Not done

- `while e is p` loops. SSPUR has no `break`, so it would need a hidden flag variable, and `while` already carries the `div` effect; a `match` inside the loop body does the job.
- `e is p` as a Bool expression outside `if` (`xs.count(_ is some(_))`). Its bindings would have no scope; write `match` or a helper. Done in ADR 0032, where the bindings are in scope in the `and ...` after the test, and `if` conditions take several tests.
- `let ... else` (done in ADR 0032 as `p = e else b`) and rest sub-patterns (`[x, ..[a, b]]`).
