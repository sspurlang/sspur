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
| 10 | Ranking: passes everything, then fewer failing tests, then fewer broken contracts or traps, then fewer parameters left unused, then the cost with the prior (decision 15), then fewer distinct functions, then whether it uses an input, then text. When no test or input reaches the hole, candidates that use an input come before constants. Only passing candidates are shown; if none passes, the one that passes the most tests is shown alone | Agents read the first line. A failing candidate is worth one line as a hint, never three |
| 11 | The search stops once the best passing candidate costs no more than the size just finished (nothing larger can outrank it) and 3 candidates pass, at 7 nodes, or at the time budget: 400 ms a hole in `sspur check`, 1000 ms in `sspur fill`, 600 ms in editors, 400 ms in the playground, with 8 observed inputs and half the fuzzed calls | The budget is per hole and bounds `check` for any program. In the playground the checker runs as you type, in a worker, and WebAssembly is slower than native code, so it searches less |
| 12 | Holes are filled in order, and once a hole has a passing candidate it stays filled while the later holes are searched | With `?a + ?b`, every test stops at `?a` first; filling it lets the tests reach `?b` |
| 13 | Surfaces: `sspur check` (text and `--json`) puts the candidates in the `E_HOLE` diagnostic's `fix` as `fill` ops with `evidence` (`passes 3 tests, post`, and `2 others also fit` when other candidates pass the same checks, decision 18) and `ok`, and prints them one per line, `?total: EXPR (EVIDENCE)`. `sspur fill [file] [?hole\|fn]` prints the best candidate per hole (`--all` up to 3); `--apply` writes it into the file, wrapped in parentheses when the bare text would parse differently, or applies the `fill` ops to the codebase. When z3 is installed and the best candidate is in the SMT fragment, `fill` proves the function's `post` with it and says `post proved`. The MCP server's `check` shows the candidates and new `fill` and `suggest` tools call the commands; the playground's quick fix for a hole lists the candidates with their evidence | One line per candidate costs a few tokens and carries both the expression and the reason to trust it. Proving takes a z3 process start (half a second on macOS), so `check` leaves it out |

### Weak tests

Agents usually write one or two tests before filling a hole, and one test rarely pins an expression down: `fn longest(xs: List[Str]) -> Int` with `longest(["ab", "abcd", "x"]) == 4` was filled with `1 + xs.len`, and the intended `xs.map(_.len).max.or(0)` was 7 nodes, out of reach in the budget. The benchmark below measures this case, and decisions 14 to 19 came from it.

| # | Decision | Why |
|---|---|---|
| 14 | `bench/fill/holes.txt` holds 51 holes of the kinds agents leave (sums and products of fields, counts with a filter, max and min with defaults, joins, lengths, `contains`, `any`, `all`, option defaults, constructors, `with` updates, index arithmetic, reverse, sort, take, drop, averages with a guard, results under `post` or a refined type), each with the intended answer and its equivalent spellings and the one or two tests an agent writes. They were frozen before any tuning. The `fill_benchmark` test in `crates/sspur-cli/tests/cli.rs` runs them with a 5 s budget, so the result doesn't depend on the machine, and fails if a hole marked `*` loses its answer or fewer than 41 hit; `cargo test --release fill_benchmark -- --nocapture` prints the table, and `SSPUR_FILL_BENCH_MS=200` measures at the `check` budget | A number to tune against, and a guard so a later change can't quietly undo a fix |
| 15 | Ranking uses a cost in tenths of a node: 10 a node, plus a prior from a fixed table. Methods agents rarely want in a hole cost more (3 for `sort`, `take`, `trim`, `abs`, `words`...), and `unique`, `push`, `flat_map`, `replace` and `items` count as 2 nodes in the search itself, so they are reached later. A rewrite table charges for patterns that are vacuous or roundabout on the tests' inputs: the same operand twice (`xs + xs`), `xs.drop(k).len`, `1 % n`, a comparison with `[]` or `""` where `is_empty` says it, an ordering of strings, a comparison with `1` where `0` is the boundary, and an offset by a literal (`1 + xs.len`, 8; halving or doubling, 4). Three more come from running the candidate: a constant answer (20), a function of collections whose result on the all-empty input isn't the empty value (5: sums, counts and filters give `0`, `""` or `[]` there), and, where the function has a `pre`, a candidate that runs fine on the inputs the `pre` turns away (6: the `pre` was written to protect something) | With weak tests many candidates pass, and only a prior can choose. The penalties never take the cost below 10 a node, so the stopping rule (decision 11) stays sound |
| 16 | A parameter the body uses nowhere else must be used by the candidate: candidates that leave one out rank after every one that doesn't | `n` passes `longer_than(["a", "abcd", "abc"], 2) == 2`, and almost never means it |
| 17 | New components: a method returning `Opt[T]` followed by `.or(d)` with the natural default of `T` (`0`, `0.0`, `""`, `[]`, `false`) is one step, so `xs.map(_.len).max.or(0)` is 4 nodes (`index_of` is left out, since `0` is a valid index); `r with f := v` for each record field; and constants: `2`, which counts as 2 nodes, and up to three integers, three strings and two separators (`","` of `"a,b,c"`) from the tests that reach the hole, which count as 3 nodes, or 1 for a string the function is named after (`"admin"` in `is_admin`). A hole with nothing in scope gets no constants from the tests, where the expected value would be the whole answer | These are what the misses needed: the composite brings the max and min idioms within reach, and a test is the only place a literal such as `80` or `","` can come from |
| 18 | When other candidates pass the same tests and contracts, the evidence says how many: `passes 1 test, 3 others also fit`. Each of them differs from the shown one on some observed input (they are different classes), so the tests don't decide between them and the ranking did | An agent that sees it knows to add a test before trusting the fill. The count covers what the search found before it stopped, so it is a lower bound |
| 19 | A test that reached the hole once passes or fails by the value at the hole alone, so its outcome is cached by that value and other candidates that give the same value there don't run it; candidates are validated cheapest first, up to 4,000. Classes are found by a hash of the type and the values, and argument values go straight into their slots | Validation used to cap at 400 candidates and ran out on the larger levels. Now most are a lookup |

Measured on an M1 Pro, `SSPUR_FILL_BENCH_MS=200`, each hole on its own (the `total_chars` hole counts `ws.join("").len` as its answer, an exact equivalent, in both rows):

| | Top-1 | Top-3 | Median per hole | Max per hole |
|---|---|---|---|---|
| Before | 26 of 51 | 32 of 51 | 28 ms | 184 ms |
| After | 41 of 51 | 45 of 51 | 31 ms | 188 ms |

With a 5 s budget the hits are the same, so what is left is ranking, not time. The ten misses: `count_longer_than` (`min(n, ws.len)` fits), `count_adults` and `evens` (their lambdas `_.age >= 18` and `_ % 2 == 0` are over the 3-node lambda bound), `any_negative` (`xs != []` fits one positive test), `average_guarded` (`xs.sum / max(xs.len, 1)` is 7 nodes and `(xs + xs).len` fits both tests), `in_range` (7 nodes, and `hi != x - lo` fits), `count_equal`, `field_update`, `clamp_post` and `cheapest_price` (the answer is second or third: `xs.len - w.len`, `it with qty := n + 1`, `x.abs` and `items.len` fit as well). Every one of them but `evens`, which finds nothing that passes, says `others also fit`. A lambda bound of 5 would reach two of them but makes the lambda pools so large that the search finds half as many answers in the budget.

`sspur fill` with the default budget on the examples of decision 13:

| Hole | Found | Time | Terms evaluated | Classes kept |
|---|---|---|---|---|
| `List[Item] -> Int`, a sum of a product of fields | `items.map(_.qty * _.price).sum` | 224 ms | 17,460 | 4,525 |
| `List[Str] -> Int` | `xs.len` | 23 ms | 2,774 | 552 |
| `(List[Int], Int) -> Int`, first or a default | `xs.first.or(d)` | 19 ms | 2,962 | 636 |
| `Int -> Shape` | `Circle{r: r}` | 7 ms | 151 | 55 |
| `Int -> Int` in a pure function, with a logging helper | `x + 1` | 9 ms | 465 | 121 |

The first one now searches on after its answer to find the others that fit (the first measurement, before this section, was 113 ms). `sspur check` of the file with the first four holes takes 0.27 s in all (0.23 s before), and of any file without holes the same time as before, since nothing new runs. The corpus check is unchanged.

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
- No `if`, `match`, `fold`, `let` or recursion in candidates, and no lambdas of two parameters. Lambda bodies are at most 3 nodes, so `_.age >= 18` (a constant from the tests counts as 3) and `_ % 2 == 0` are out of reach.
- The prior of decision 15 is a hand-written table checked against one benchmark; with one weak test it still picks the wrong candidate for about one hole in five, and then the evidence says `others also fit`.
- Contract suggestions don't cover monotonicity across calls, relations between two results, or quantified clauses (`r.all(xs.contains)`), and a `pre` is at most two clauses from the grammar.
- Z3 confirms `post` suggestions and the best hole candidate in `sspur fill`, not every candidate in `sspur check`.
