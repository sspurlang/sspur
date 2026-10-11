# ADR 0030: Hole filling and suggested contracts

Status: accepted, 2026-10-11

## Problem

A typed hole (`?` or `?name`) told the agent its type and the names in scope that had that type, and nothing more. The program usually knows much more than that: its tests say what the function returns on some inputs, its `post` and refinement types say what it must return on all of them, and its signature says which effects it may use. An agent filling the hole still had to guess an expression, edit, run the tests and try again, and each round costs a full turn of tokens.

The reverse problem is contracts. `pre`, `post` and `where` pay off four ways (runtime checks, fuzzing, proofs, faster native code), but agents rarely write them, and when they do they write the obvious ones. The interpreter can watch a function run and see most of the simple ones hold.

Both are search problems over small expressions, judged by running the program, so they share one design.

## Hole filling

```
fn total(items: List[Item]) -> Int
  post r >= 0
= ?total

test t1 = total([]) == 0
test t2 = total([Item{sku: "a", qty: 2, price: 3}]) == 6
```

```
a.ssp:3:3 hole E_HOLE hole ?total expects type Int
  ?total: items.map(_.qty * _.price).sum (passes 2 tests, post)
```

| # | Decision | Why |
|---|---|---|
| 1 | The checker gains `check_holes`, which returns each hole's resolved type, the locals in scope with their types, the effects the enclosing function declares (`log`, `fail[E]`, ...), and every signature the search may call: the program's functions, trait methods with the types that implement them, and the builtin globals and methods from the checker's own tables. Ordinary `check` does not collect any of it | The search must agree with the checker on types and effects, and reading the checker's tables keeps it from drifting. Collecting only on request keeps `sspur check` of a file without holes exactly as fast as before |
| 2 | Candidates are enumerated bottom-up by size: locals, a few literals (`0`, `1`, `""`, `[]`, `none`, `true`, `false`, `0.0` when `F64` appears), nullary variants and zero-argument functions at size 1, then every component applied to smaller terms: field access, record and variant constructors (up to 4 fields), user functions and trait methods (up to 4 parameters), arithmetic, comparisons, `and`/`or`/`not`, `+` on strings and lists, and an allowlist of builtin methods (`len sum map filter find any all first last reverse sort sort_by unique contains take drop push min max index_of join` on lists, `len trim lower upper split contains starts_with replace take drop to_int words chars` on strings, `or is_some is_none map` on options, `abs to_f64` on Int, `abs round floor sqrt` on F64, a few on maps and sets, and the globals `min max clamp some ok err`) | The whole builtin table is several hundred methods, and almost none of them help fill a hole; with all of them the search found the same answers several times slower. The allowlist is the set agents use, which is also the set the agent spec teaches |
| 3 | Size counts AST nodes; `_` inside a lambda is free, so `items.map(_.qty * _.price).sum` is 6. The bound is 7 nodes, and lambda bodies up to 3 nodes are enumerated separately per element type (`_.f`, `_.f * _.g`, `_ > 0`, `_.name.len`) and offered to `map`, `filter`, `find`, `any`, `all`, `sort_by`, `flat_map` and `Opt.map`, along with the program's one-parameter functions as values (`xs.map(area)`) | 7 nodes covers the holes agents leave in practice (a sum of a field, a length, a constructor from the parameters, an option default, a call with the parameters). The next size up multiplies the count by about four |
| 4 | Each candidate is evaluated on a set of observed inputs, and two candidates of the same type that give the same value (or the same trap or raise) on every one of them are one class; only the first, smallest one is kept, and of two the same size the one with fewer functions, then the shorter text (observational equivalence) | This is what makes the search fast. Without it, `x + 0`, `0 + x`, `x * 1` and `x.abs` on positive inputs all survive and grow into larger copies of each other; with it a class is built on once. On the example above it keeps 2,500 classes out of 10,000 terms it evaluated |
| 5 | The observed inputs are the values in scope each time a test or example reaches the hole, plus the values when the enclosing function runs on small inputs (`0`, `1`, `-1`, `[]`, `[0]`, `""`, `"a"`, `none`, ...) alternating with fuzzed ones from its `pre` and `where` (the fuzzer's generator), at most 16. A probe mode in the interpreter records them at the hole and traps there, so nothing else in the program changes | The tests are the best evidence of what the hole should be, and the small and random inputs separate expressions the tests happen to agree on, such as `xs.first` and `xs.min` on `[5, 6]` |
| 6 | A term is built over the values its arguments already have: the new node is evaluated with each argument bound to its cached value, so one step costs one node per input, not a re-evaluation of the whole expression | Most terms are rejected as equal to a smaller one, and the cost of deciding that is now independent of their size |
| 7 | Terms that can't become the hole's type within the remaining size are not built (a reachability check over the components), nor are terms that are certainly equal to smaller ones: a method on a literal (`"".len`), an operator on two constants, `x.abs.abs`, `xs.reverse.sum`, `xs.map(f).len` | Cheap rules that remove most of the work before any evaluation |
| 8 | A candidate may only use effects the enclosing function declares, and inside a test none; the effect of each component is checked with its type parameters filled in (`fail[E]` of `ok_or`) | A hole in a pure function must not be filled with a call that logs. The final type check below would reject it anyway, but filtering early keeps such terms out of the search |
| 9 | Candidates of the hole's type are validated as each size completes, smallest first: the tests and examples that reach the hole run with the candidate in the hole (the interpreter evaluates it there), and if they all pass, the function runs on the observed inputs, where a `post` or result refinement that breaks, or any other trap, counts against it. The survivors to be shown are typechecked in place by substituting them into the module and checking the enclosing definition | Running the program is the only check that knows what the tests mean. Typechecking only the few shown keeps the cost small, and makes sure every `fill` fix applies cleanly |
| 10 | Ranking: passes everything, then fewer failing tests, then fewer broken contracts or traps, then size, then fewer distinct functions, then whether it uses an input, then text. When no test or input reaches the hole, candidates that use an input come before constants. Only passing candidates are shown; if none passes, the one that passes the most tests is shown alone | Agents read the first line. A failing candidate is worth one line as a hint, never three |
| 11 | The search stops when 3 candidates pass (larger ones can't outrank them), at 7 nodes, or at the time budget: 200 ms a hole in `sspur check`, 1000 ms in `sspur fill`, 150 ms in the playground, with 8 observed inputs and half the fuzzed calls | The budget is per hole and bounds `check` for any program. In the playground the checker runs as you type, in a worker, and WebAssembly is slower than native code, so it searches less |
| 12 | Holes are filled in order, and once a hole has a passing candidate it stays filled while the later holes are searched | With `?a + ?b`, every test stops at `?a` first; filling it lets the tests reach `?b` |
| 13 | Surfaces: `sspur check` (text and `--json`) puts the candidates in the `E_HOLE` diagnostic's `fix` as `fill` ops with `evidence` (`passes 3 tests, post`) and `ok`, and prints them one per line, `?total: EXPR (EVIDENCE)`. `sspur fill [file] [?hole\|fn]` prints the best candidate per hole (`--all` up to 3); `--apply` writes it into the file, wrapped in parentheses when the bare text would parse differently, or applies the `fill` ops to the codebase. When z3 is installed and the best candidate is in the SMT fragment, `fill` proves the function's `post` with it and says `post proved`. The MCP server's `check` shows the candidates and new `fill` and `suggest` tools call the commands; the playground's quick fix for a hole lists the candidates with their evidence | One line per candidate costs a few tokens and carries both the expression and the reason to trust it. Proving takes a z3 process start (half a second on macOS), so `check` leaves it out |

### Measured

On an M1 Pro, `sspur fill` with the default budget, each hole on its own:

| Hole | Found | Time | Terms evaluated | Classes kept |
|---|---|---|---|---|
| `List[Item] -> Int`, a sum of a product of fields | `items.map(_.qty * _.price).sum` | 113 ms | 9,847 | 2,549 |
| `List[Str] -> Int` | `xs.len` | 21 ms | 2,088 | 390 |
| `(List[Int], Int) -> Int`, first or a default | `xs.first.or(d)` | 42 ms | 6,551 | 1,884 |
| `Int -> Shape` | `Circle{r: r}` | 9 ms | 395 | 146 |
| `Int -> Int` in a pure function, with a logging helper | `x + 1` | 8 ms | 524 | 166 |

`sspur check` of the file with the first four holes takes 0.23 s in all, and of any file without holes the same time as before, since nothing new runs. The corpus check is unchanged. In the playground (Node, WebAssembly) the two holes of the quick-fix test take about 130 ms.

## Suggested contracts

```
$ sspur suggest c.ssp
at
  pre i >= 0  // traps on 207 of 227 inputs without it
  pre i < xs.len  // traps on 207 of 227 inputs without it
avg
  pre xs.len > 0  // traps on 1 of 176 inputs without it
clip
  post r.len <= xs.len  // proved
```

| # | Decision | Why |
|---|---|---|
| 1 | Inputs are the arguments of every call the tests and examples make to the function (recorded by the interpreter), every combination of small values of the parameter types up to 200, and 200 fuzzed ones, without duplicates. Each runs with the fuzzer's step budget; an input that breaks an existing `pre` or `where` is dropped | Test arguments are the realistic ones, small values find the edge cases (`[]`, `0`, `[0]`) that random ones rarely hit, and random ones cover the rest |
| 2 | `post` candidates come from a fixed grammar over numeric terms of the result and the parameters (`r`, `r.len`, `r.f` for `Int` and `F64` fields, the same for parameters): `t > 0`, `t >= 0` for a result term, `t OP p` for each parameter term `p` and `OP` in `== < > <= >=`, `r == x` for a parameter of the result's type, and `r == r.sort` for lists of `Int`, `Str` or `F64`. A clause is kept when it held on every input that returned, with at least 5 of them, and a relation to a parameter term only when that term took more than one value | This is Daikon's approach, kept small: a grammar an agent would write itself, and only clauses that the runs justify |
| 3 | Implied and trivial clauses are dropped: `>=` when `==` or `>` holds, `t >= 0` for a length, `t >= 0` when `t` is at least a length, `r.len == xs.len` when `r == xs`. The rest are ranked equalities first, then strict, then non-strict relations, then signs, and at most 6 are printed | The list is meant to be read; `r >= 0`, `r > 0` and `r >= xs.len` together say less than the one that implies the others |
| 4 | A `pre` is suggested only when the function trapped on some inputs and ran on others: the first clause (`x >= 0`, `x != 0`, `x > 0`, `xs.len > 0`, `a <= b`, `a < b`, a `Bool` parameter, `o.is_some`) that is true on every input that ran and false on every one that trapped, weakest first, or else the first pair that covers the traps together | A trap on `[]` is the common case of a missing precondition, and the clause that separates exactly those inputs is the one to write |
| 5 | With z3 installed, each `post` is proved by adding it to the function (with any suggested `pre`) and asking the SMT encoding of `sspur verify`; a proved one says `proved`, the others `held on N runs`, and proved ones are listed first | The difference between the two is what the agent needs to know before keeping a clause |
| 6 | Output is one function name, then `  pre CLAUSE  // NOTE` and `  post CLAUSE  // NOTE` lines that paste into the signature. `--json` gives each as a `refine` fix. `--apply` adds them, to the file by reprinting the changed functions, or to the codebase as `refine` ops, and only if the result still typechecks. Nothing is applied without it | A suggestion can be wrong in a way the runs didn't show, so the agent or the author decides |

## Not done, and limits

- Candidates larger than 7 nodes are not found: `Point{x: p.x + d, y: p.y + d}` is 9. Leave a smaller hole, `Point{x: ?x, y: ?y}`.
- Observational equivalence is only as good as the inputs. Two expressions that agree on every observed input but differ elsewhere count once, and the shorter one is shown; the tests and the contracts then decide, but a program with no tests and no `post` can get a plausible constant such as `0`.
- A hole reached only after another trap in the same test, or only on a path the tests don't take, has fewer observed inputs. Holes in functions with function parameters or effects the fuzzer can't run (`io`, `fs`, ...) get no fuzzed inputs.
- No `if`, `match`, `fold`, `let` or recursion in candidates, and no lambdas of two parameters.
- Contract suggestions don't cover monotonicity across calls, relations between two results, or quantified clauses (`r.all(xs.contains)`), and a `pre` is at most two clauses from the grammar.
- Z3 confirms `post` suggestions and the best hole candidate in `sspur fill`, not every candidate in `sspur check`.
