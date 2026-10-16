# ADR 0022: Per-definition native objects, proven rewrites and cost-driven choices

Status: accepted, 2026-10-04

## 1. Per-definition native compilation

### Delivered

| Piece | Where |
|---|---|
| C top-level splitter: items, `#define`/`#undef` groups, declared names and references | `sspur-native/src/cgen/split.rs` |
| One translation unit per top-level definition plus one runtime unit, objects cached by content in `~/.cache/sspur/native/obj` and linked into the program's shared library | `build_split` in `sspur-native/src/cgen.rs` |
| Stable trap ids: function, refinement and error-type ids are content hashes instead of positions | `stable_slot`, `Compiled::message` |
| Order-independent fresh names (`v_x$17` renumbered per unit) | `Cx::fresh`, `split::canon` |

### Decisions

| # | Decision | Reason |
|---|---|---|
| 1 | Codegen is unchanged; the generated whole-program C is split afterwards into units | One code generator, so whole-program and per-object builds produce the same code. The splitter only moves text |
| 2 | A unit holds one definition (`f_name`, its specializations, lambdas, `__own` variants and entry points), plus the transitive closure of the declarations it references, in source order | The unit text is the cache key, so it covers exactly what affects the definition's code: its body, the signatures of its callees, the types and helpers it uses, and the bodies of callees it inlines |
| 3 | Small callees (under 1200 bytes of C, without static state, at most 24 per unit) are copied into the caller's unit as `static` functions; larger ones are called through a hidden-visibility prototype | Keeps cross-function inlining at `-O2` without LTO. An edit to a small function recompiles it and its inlining callers, like a C++ header |
| 4 | Mutable globals, functions with static locals or constructors, exported functions and large non-inline runtime functions live once in the runtime unit; other units see `extern` hidden declarations | One heap, one thread pool, one trap buffer; less duplicated code per unit |
| 5 | Specializations, lambdas and helpers are `static` and duplicated in each unit that uses them | Like C++ templates; no cross-unit naming |
| 6 | Fresh names carry a `$` marker and each unit renumbers them by first use | The global counter otherwise renamed every later function's locals after one edit |
| 7 | Function ids in `st->func`, refinement ids and error-type ids are 30-bit content hashes with probing, mapped back by `Compiled` | Positional ids shifted when a definition or contract was added above |
| 8 | Default `-O2` builds are per-object; `--O3` keeps the whole-program build; `SSPUR_SPLIT=0|1` overrides, `SSPUR_LTO=1` adds ThinLTO with the linker's LTO cache | Measured below: per-object with copied small callees is within noise of whole-program on every benchmark, and ThinLTO is no faster |
| 9 | Units compile in batches of up to 16 per clang process, at most 4 processes | clang startup (about 50 ms here) dominated cold builds. Update: one process per core within memory, and the functions of a package with 16 or more share units ([ADR 0041](0041-cold-native-builds.md)) |
| 10 | A memo file maps the whole-program C hash to the linked library | Warm runs skip splitting |
| 11 | If the per-object build fails, the whole-program build runs | The fallback is the old path |

### Measurements (Apple M1 Pro, macOS, background load, `bench/incremental`)

Generated program, 200 functions plus 40 helpers and 200 tests (`gen.py 200`), `sspur test`, check cache warm, native cache cold at first:

| | Whole program | Per definition |
|---|---|---|
| Cold build | 5.3 s | 5.95 s (242 units) |
| Edit one function, re-test (3 edits) | 5.1 to 5.3 s | 0.93 to 0.96 s (1 unit compiled) |
| Unchanged re-test | 0.07 s | 0.07 s |

Of the 0.93 s, compiling the unit takes about 0.2 s, linking 242 objects 0.18 s, and loading a new library about 0.5 s (first load of a new binary on this machine).

Benchmarks (`bench/incremental/bench.sh`, best of 5, interleaved; seconds):

| Benchmark | Whole | Per definition | Whole (second run) | ThinLTO per definition |
|---|---|---|---|---|
| compute_big | 1.070 | 1.068 | 1.062 | 1.060 |
| typical | 0.152 | 0.156 | 0.170 | 0.172 |
| strings_big | 0.092 | 0.096 | 0.095 | 0.092 |
| app | 0.164 | 0.152 | 0.162 | 0.164 |
| churn | 0.105 | 0.097 | 0.107 | 0.109 |
| parallel | 0.326 | 0.322 | 0.333 | 0.323 |
| simd | 0.258 | 0.255 | 0.261 | 0.261 |

The spread between runs of the same build is about 5 to 10%; no benchmark moves outside it.

### Not done

- `profile bare` kernels and `export-c` libraries still build as one unit with positional ids.
- An edit that changes a type or a widely used helper's text recompiles every unit that includes it.

## 2. Proven rewrites

### Delivered

An AST optimizer (`sspur-native/src/cgen/opt.rs`) runs before lowering and C generation in `compile_release`. It rewrites function bodies, prints the module, re-parses and re-typechecks it, and hands the result to codegen. `sspur explain-opt file.ssp [--all]` prints each applied rewrite with its proof and cost note (and, with `--all`, every rejected candidate with the reason). `SSPUR_OPT=0` turns it off.

| Rule | Rewrite | Proof obligation | Discharged by |
|---|---|---|---|
| `inline` | `f(a, b)` to `f`'s body with `a`, `b` substituted, or bound by fresh `let`s in argument order | `f` has no effects, contracts, refined parameter or result types, `return`, `raise`, `catch`, handlers or local functions, is not in a call-graph cycle, and no free name of its body is bound at the call site | construction |
| `fold` | `2 + 3` to `5`, comparisons and `not` of literals, `true and x` to `x`, `x + 0`, `x * 1` and `1 * x` on Int, `if true/false` | the exact 128-bit result lies in Int range and the divisor is non-zero, so no trap is removed; identities hold for every Int | construction |
| `dead-branch` | `if c then a else b` to the live branch | `c` is pure, cannot trap, and is constant under the facts in scope: parameter refinements, alias refinements, `pre`, enclosing conditions, `let`s and `for` ranges | intervals (the prover's `raw_op`) or Z3 (`sspur-smt`'s solver, with 64-bit bounds, the facts and their overflow-freedom asserted) |
| `map-map` | `xs.map(f).map(g)` to `xs.map(x => g(f(x)))`, also across function boundaries after inlining | both stages pure; trap order unchanged: at most one stage can trap, or every possible trap is the same element-independent kind (`integer overflow` or `division by zero`) | construction or intervals |
| `loop-fusion` | `src.map(..).filter(..)...sum` or `.len` to one `for` loop with an accumulator | stages pure; the same trap-order condition over the stages and the consumer (an Int `sum` can overflow) | construction or intervals |

### Decisions

| # | Decision | Reason |
|---|---|---|
| 1 | Preserved semantics: the value, the raised error, and the trap and its message. Out-of-fuel, out-of-memory and stack-depth traps are resource limits and are not preserved | The same equivalence the differential fuzzer already uses; inlining removes frames, fusion removes allocations |
| 2 | Trap sets are computed per stage over abstract values (Int intervals, records with per-field intervals) flowing from the source through the stages; a record literal's `where` checks are discharged when the field's interval is inside the refinement | `(0..n).map(i => Item{qty: i % 7 + 1, ..})` is trap-free, so the pipeline after it may be reordered |
| 3 | An unknown construct (calls to recursive or effectful functions, partial methods, `match`) makes a stage "may trap with anything", which blocks reordering unless every other stage is trap-free | Unknown is never assumed safe |
| 4 | Rewrites happen on the AST, then the module is printed, parsed and checked again. A function whose rewritten text does not parse or typecheck keeps its original body | Codegen and its type information stay unchanged, and every optimized program is a valid program |
| 5 | Inlined callees with shadowing or capture risk are skipped; let-bound arguments are only introduced in statement positions | Hygiene without a renaming pass |
| 6 | Generated names are fresh (`x7_`, `acc3_`) against every identifier in the module | No capture |
| 7 | `sys` and `bare` modules are not optimized | Ownership and destructor semantics are checked on the original text |

### Verification

- `tests/programs/pipelines.ssp` exercises every rule, including the rejected cases: `shaky` (a division trap and an index trap, not fused) and `first_trap` (overflow plus division by zero in one stage, not fused). `sspur fuzz --differential` is clean on it, also with `--edge`, and on all 48 `tests/programs` files.
- The 199-program corpus is identical (258/258 functions native); 16 corpus programs get rewrites (inlining and loop fusion).
- `explain_opt_lists_proven_rewrites` checks the command output, including a Z3 proof of `lo > hi` being false under `pre lo <= hi`.

## 3. Cost-driven choices

The optimizer estimates instructions per evaluation for each node: 1 per literal or name, 2 per checked arithmetic operation, 24 per allocation, 8 per call, and per element of a list operation, with ranges of literal bounds counted exactly and other lists assumed to hold 1000 elements. Allocation of a materialized list element costs 6.

| Choice | Rule |
|---|---|
| Inline or call | Inline when the callee is at most 40 instructions, or when inlining exposes a pipeline (the callee builds a list that the call site consumes with `map`/`filter`/`sum`/`len`, or consumes its parameter with one and the argument is a pipeline) |
| Fused loop or method chain | Keep the chain when native codegen fuses it in place (its lambdas are simple and can raise at most one trap kind, ADR 0008), since that path vectorizes and parallelizes; otherwise build the loop when its estimate (`n * (stages + 2)`) beats the materialized one (`n * (stages + 6 per list)`) |
| `map.map` | Compose only when native codegen would not fuse the pair itself |
| Profile | `sspur run --profile file.ssp` runs the interpreter with call counting and writes `~/.cache/sspur/profile/<module hash>.json`. With a profile, never-called callees are not inlined and callees with at least 10 000 calls are inlined up to 160 instructions |

Parallel versus sequential stays a runtime decision (the timed warm-up of ADR 0011), which already uses measured costs.

### Result (best of 5, interleaved, seconds; optimizer off / on)

| Benchmark | Off | On | Rewrites |
|---|---|---|---|
| simd | 0.275 | 0.235 | `shifted(xs, r).sum` inlined, so native code fuses `xs.map(x => x * 5 - r).sum` instead of building a 1M-element list 40 times |
| typical | 0.153 (85 MB peak) | 0.140 (15 MB peak) | `order_total(make_items(n))` inlined twice and fused into one loop; no 3M-record list |
| compute_big, strings_big, app, churn, parallel | | | no rewrites, same code; differences are run-to-run noise |

The simd gain is 15% and repeats across runs (0.268 to 0.223, 0.274 to 0.234). On typical the time difference is within noise, but peak memory drops 5.7x. The orders part alone (`orders_bench(3000000)`) goes from 62 ms to 55 ms and 76 MB to 6 MB.

### Not done

- Rewrites inside `match` arms use no facts from the pattern; `match` stages are not analyzed for traps.
- No float rewrites (signed zero and NaN make most identities unsound).
- The profile records call counts only, not list lengths per pipeline.
