# ADR 0032: `e is p` anywhere, guard clauses and long lines

Status: accepted, 2026-10-12

## Problem

ADR 0029 added `if e is p then a else b` and left two things out. `e is p` worked only right after `if`, so `xs.filter(_ is some(_))`, `ok = r is ok(_)` and `test t = parse("x") is err(_)` were parse errors, and two tests in one condition, `if a is Circle{r} and b is Circle{r: r2} then`, needed nested `if`s or a `match` on a tuple. And a function that takes values apart one at a time, `some(user) = find(id)` then `ok(v) = parse(user.raw)`, had to nest a `match` per step or move each step into a helper: there was no way to bind a pattern that can fail and say what happens when it doesn't.

Separately, a statement had to fit on one line unless it was inside brackets. `= a > 0 and` followed by `b > 0` on the next line was `E_PARSE_EXPR` ("expected an expression, found end of line"), and so was `= a +` then `b`. A method chain with one call per line, `= xs` then `.map(_ * 2)` and `.sum`, was `E_PARSE_DEF`, because a line starting with `.` looked like a new definition. People hit this in the playground and in editors as soon as a condition or a pipeline got long, and the only way out was to wrap it in parentheses.

## `e is p` anywhere

```
fn somes(xs: List[Opt[Int]]) -> Int
= xs.filter(_ is some(_)).len

fn big(o: Opt[Int]) -> Bool
= o is some(v) and v > 10

fn both(a: Shape, b: Shape) -> F64
= if a is Circle{r} and b is Circle{r: r2} then r + r2 else 0.0

test bad_input = parse("x") is err(_)
```

| # | Decision | Why |
|---|---|---|
| 1 | `e is p` is a `Bool` expression wherever an expression goes. It is a two-arm `match`, `\| p => true \| _ => false`, marked as written with `is`, so the checker, the interpreter, native code, the ownership checker and the SMT translation see an ordinary `match`, as in ADR 0029 decision 8 | No new semantics in any tier. In native code it is the pattern's tests and nothing else: one tag compare for `some(_)`, one length compare for `[_, ..]`, no allocation |
| 2 | `is` keeps the precedence ADR 0029 gave it after `if`, now everywhere: it takes the whole expression before it, so `a and b is p` is `E_PARSE_IS` unless `a and b` is parenthesized. After the pattern, `and` and `or` take the whole expression after them: `e is p and rest`. Anything else after a pattern (`==`, `+`, a second `is`) is `E_PARSE_IS` with the hint to parenthesize the test, `(e is p) == x` | One rule in every position, the one `if` already had, so no existing program changes meaning. Binding tighter than `and`, as C# and Kotlin do, would read `a and b is some(v)` as `a and (b is some(v))`, which is fine, but it would also read the guard in `if o is some(v) and v > 0 or v < -9` as `(o is some(v) and v > 0) or v < -9`, which ADR 0029 accepted with the opposite meaning. The price is that a test in the middle of a chain of `and`s needs parentheses or goes first, and the error says so |
| 3 | The names a test binds are in scope in the `and ...` after it, and, when the test is an `if` condition, in the `then` branch. A test that binds a name nothing can see, `ok = r is ok(v)`, `o is some(v) or g`, `r is ok(v) and flag`, is `E_IS_BINDS`, with the pattern rewritten with `_` as the hint | This is the guard rule of ADR 0029 decision 5, applied to every `and`. A name bound under `or` or `not`, or at the end of an expression, would be unbound whenever the test failed, and a warning would let a typo through as a silently unused name |
| 4 | In `if t1 and t2 and ... then a else b`, the `then` branch sees the names of every test. When only the first test binds names (or none does), it is ADR 0029's `if e is p and guard`, unchanged. When a later test binds names the branch can use, the tests up to the last such one become nested matches whose innermost arm yields `some(a)`, and the outer match is `\| some(it) => it \| _ => b`, marked as written as an `if` | Each test runs only after the ones before it matched, so `if o is some(i) and xs[i] is ok(v)` is safe, and `b` is written once and sees none of the names. Nesting the `if`s instead would copy `b` into every level: twice the code per test, exponential in a chain of `else if`s, and `b` would see names the earlier patterns shadowed. The `Opt` is a C struct on the stack that clang removes |
| 5 | A pattern that always matches warns `W_ARM_UNREACHABLE` ("this test is always true"). A pattern that can't match the value written there, a literal, a constructor call, a list or a tuple of them (`none is some(_)`, `[] is [_]`, `Dot is Circle{..}`), warns `W_NEVER_MATCHES` | The first is the usefulness check of ADR 0029 decision 9; the second needs no types, only the shape of the expression, and catches the test that was meant for a different value. Both are warnings, like an unreachable arm |
| 6 | The printer writes the test back as `e is p`, an `if` chain as `if e1 is p1 and e2 is p2 then`, and parenthesizes a test used as an operand (`g or (o is none)`, `(o is none) and g`, `not (o is none)`). The content hash tags each form, so `o is some(_)` and the equivalent `match` hash differently, as `if e is p` already did | The canonical text and the hash keep the form the author wrote |
| 7 | Hole filling can propose `x is V` for each variant of a sum with more than one, and `r is ok(_)` and `r is err(_)` when the program uses `Res`, as candidates for a `Bool` hole. It doesn't propose `o is some(_)` or `xs is []`, which `.is_some` and `.is_empty` already cover | Testing a variant had no other spelling the search could build. On `bench/fill/holes.txt` the top candidate is unchanged, 41 of 51 at 200 ms and at 5000 ms |

## Guard clauses: `p = e else b`

```
fn first_port(args: List[Str]) -> Int
= do
  [first, ..] = args else 80
  some(p) = first.to_int else 80
  p

fn need(opts: Map[Str, Str], name: Str) -> Str ! fail[ArgErr]
= do
  some(v) = opts.get(name) else raise Missing{name}
  v.trim
```

| # | Decision | Why |
|---|---|---|
| 8 | A binding in a block can have a pattern that may fail, followed by `else b`: `some(user) = find(id) else raise NotFound{id}`. When `e` matches, the rest of the block runs with the names bound; when it doesn't, the block ends there and its value is `b`. It is the match `\| p => <the rest of the block> \| _ => b`, marked as written as a guard | No `let`, as everywhere in SSPUR, so it costs one token more than a plain binding. As a `match` it needs nothing new in any tier, and in native code it is the pattern's tests and one branch |
| 9 | The `else` value is the value of the block, not necessarily of the function. In a function body that is the function's result, so `else 0`, `else return err("bad")` and `else raise Bad` all do what they say; in a loop body `else ()` goes on to the next element; in a lambda's `do` block it is the lambda's value | SSPUR has no `break` or `continue`, and lambdas have no `return`. Rust and Swift require the `else` to diverge, which needs both, and would make guards useless in loops and lambdas. Requiring `return`, `raise` or `exit` and checking it would add a divergence analysis to the checker and still leave `else 0` in a function body, the common case, as an error to be rewritten `else return 0`. A value for the block is what the desugaring means anyway, and one sentence explains it |
| 10 | A pattern that can fail without `else` stays `E_REFUTABLE_LET`, now with the hint `some(v) = e else <value, raise or return>`. A guard whose pattern can't fail warns `W_ARM_UNREACHABLE` ("'else' never runs"), and one that can't match the value warns `W_NEVER_MATCHES`. Record destructuring, `Point{x, y} = p`, is a pattern that can't fail in every profile, not only `sys` | The checker already knew which patterns can fail; a record pattern of a record type always matches, and the interpreter and native code already bound it |
| 11 | `else` may start the next line when it is indented deeper than the binding. The printer puts the guard on one line, then the statements after it at the same indentation | A long value pushes `else` past the edge of the screen |
| 12 | Rust's `let some(x) = e else { return 0 }` and Swift's `guard let x = e else { ... }` and `guard case p = e else { ... }` are accepted and stored as `some(x) = e else return 0`, with the notes `let p = e else -> p = e else`, `guard let x = e else -> some(x) = e else` and `p = e else { b } -> p = e else b`. The braces may hold several lines. `guard let x = e` without `else` is `E_PARSE_GUARD` | The tolerant-input mechanism of ADR 0029 decision 6: the first try of an agent used to Rust or Swift lands, and the codebase has one spelling |
| 13 | The fuzzer's program generator writes `e is p` tests with and without `and`, `if` chains of two tests, and guards with every kind of `else` (a value, `()`, `return`), and compares the interpreter with native code. The editors offer `is` and `guard` snippets | The new forms get the same differential testing and the same completions as the old ones |

## Long lines

```
fn ready(u: User) -> Bool
= u.active and
  u.email != ""

fn total(items: List[Item]) -> Int
= items
  .filter(_.qty > 0)
  .map(_.price * _.qty)
  .sum
```

| # | Decision | Why |
|---|---|---|
| 14 | A line that ends in a binary operator continues on the next line: `and`, `or`, `+`, `-`, `*`, `/`, `%`, `**`, the comparisons, `&&`, `\|\|`, `,`, `:=`, `->`, `with`, and `is` after a value | An expression can't end in an operator, so the reading is never in doubt, whatever the next line's indentation. It is where Go, Kotlin and Swift break long lines |
| 15 | A line that starts with `.name` or a binary operator (`and`, `or`, `+`, `-`, `*`, `/`, `%`, a comparison, `&&`, `is`) continues the line before it when it is indented deeper than the line its statement starts on and the line before ends in a value: a name, a literal, `_` or a closing bracket | A leading-dot chain is how long pipelines are written in Kotlin, Swift, Rust and JavaScript, and a leading `and` is the usual way to break a long condition. The two conditions keep everything that parsed before as it was: the first line of a block comes after `do`, `=`, `=>`, `then` or `else`, not after a value, and a statement at its block's indentation is not deeper, so `-x` alone on a line in a block is still a statement |
| 16 | The lexer does it, by not emitting the line break, so the parser and every tier see one line | No new grammar and no new syntax tree. The layout rules for blocks, `do`, arms and rule tables only look at the line breaks the lexer still emits, so they are untouched |
| 17 | `\|` is never a continuation: at the start of a line it begins a match arm, a variant or a rule row, and a line ending in a single `\|` doesn't continue | Or-patterns put the alternatives on one arm's line already, and reading `\| B` as the rest of `\| A` would merge two arms |
| 18 | `sspur fmt` prints a continued statement on one line, as before, and doesn't wrap long lines (superseded by ADR 0040: the canonical text still does, and `sspur fmt` breaks lines over 100 columns) | The printer works from the syntax tree, which keeps no line breaks, and the canonical text must not depend on where the input was broken: two agents that write the same code get the same text and the same hash. Wrapping at a fixed width would make an edit that changes one line's length move the breaks of the lines around it in every diff. Editors wrap long lines on screen |
| 19 | Editors indent the line after one that ends in an operator, an open bracket, `=>`, `=` or `do` by one level, keep a `.name` chain aligned, and go back to the statement's indent after its last continued line. The playground (CodeMirror's indent service), VS Code (`onEnterRules` in the language configuration) and the language server (`textDocument/onTypeFormatting` on Enter, which LSP4IJ uses in JetBrains IDEs) follow the same rules, kept in `sspur-ide` for the server, and the playground test checks the other two against the same cases | Enter should put the cursor where the language reads the line, in every editor alike |
| 20 | The fuzzer's program generator breaks some of its lines after an operator or before a `.method` | Continued lines go through the interpreter and native code like any other |

## Not done

- `while e is p` loops, for the reason ADR 0029 gives: without `break` it needs a hidden flag. A guard in the loop body covers the common case.
- `is` binding tighter than `and` (decision 2). If chains of plain conditions and tests turn out to be common, `a and b is p` can become `a and (b is p)` later without breaking a program, since it is an error today.
- An `is` test as a cell of a `rule` row needs parentheses, `\| (x is some(_)), _ => ...`; a cell is already a pattern or a condition.
- Wrapping long lines in `sspur fmt` (decision 18). Done in ADR 0040.

