# ADR 0044: Proving F64 to Int conversions safe

Status: accepted, 2026-10-18

## Problem

Since 0.15, `round floor ceil trunc` on an `F64` trap with `integer overflow` for NaN, the infinities and values outside `[-2^63, 2^63)`, and 1.0 keeps that; `checked_round` and the others give an `Opt[Int]`. Nothing told you which conversions could trap, so the trap showed up in production on the first NaN. `Int` arithmetic already has its overflow checks proved away where they can't fail (ADR 0007, ADR 0009); conversions needed the same, as a warning in `sspur check`.

## Decision

`sspur check` reports `W_CONVERSION_MAY_TRAP` at every conversion that it can't prove gets a finite value in `[-2^63, 2^63)`, with a hint naming `checked_round` (etc.) and a guard. Code without the warning can't trap in a conversion. The proof is ADR 0009's encoding with `F64` added, in a second pass that only `check` runs (`crates/sspur-smt/src/float.rs`, `conv.rs`); the first pass, which native code uses, is unchanged, so its queries and the generated C are the same as before.

### `F64` in the encoding

| | Encoding |
|---|---|
| A value | A class (`-1` -inf, `0` finite, `1` +inf, `2` NaN) and a real, which matters when the class is 0. Comparing class, then real, is SSPUR's total order: NaN above `inf()`, `-0.0 == 0.0` |
| Literals | Exact rationals; arithmetic and `sqrt` on literals are folded exactly, other functions of literals within 2^-50 |
| `+ - * /` | IEEE for infinities, NaN and the sign of a zero divisor (unknown). A finite result is within 2^-53 of the exact one, relatively, plus 2^-64; it keeps the exact one's sign, doesn't pass 1 or 2^63 in magnitude if the exact one doesn't, and is infinite only past the largest double |
| Products and quotients of two variables | Also linear consequences, `\|b\| <= 1` gives `\|a*b\| <= \|a\|` and `0 < a <= b` gives `a/b <= 1`, so Z3 rarely needs nonlinear reasoning |
| `n.to_f64` | Rounded as above, exact up to 2^53, and monotonic in `n` |
| Methods | `abs sqrt exp ln log2 log10 sin cos tanh atan atan2 asin acos cbrt`, `pow` with an exponent in `[0, 1]`, `is_nan is_finite is_inf`, and `min max clamp inf nan pi rand_f64`; anything else is any value |
| A conversion | The site; after it, the `Int` is within 1 of the operand |

### What is known at a conversion

ADR 0009's facts (refinements of parameters, aliases and record fields, `pre`, path conditions of `if`, `and`, `or`, `let` bindings), plus:

- a `var` keeps its value until it is assigned, and keeps nothing across a loop or a closure;
- the `F64` vars a loop assigns get invariants, from a menu (finite, `>= 0`, `<= 0`, magnitude at most 1, 2^53 or 2^62): those that hold on entry and after every assignment, assuming the others (Houdini), are assumed in the loop and after it;
- the callback of `(a..b).map(i => ..)` (and `filter`, `any`, `all` and the rest) gets `a <= i < b`; that of `xs.filter(p).map(..)` gets `p`;
- lambdas, local functions, `catch` and `handle` bodies are walked under fresh guards;
- a call to a function that isn't recursive, has no `return` and is at most 400 nodes is encoded in place, three calls deep, so the caller sees its result.

A function is closed when it isn't `pub`, `main` or another entry point of the runtime (`core_main`, `on_trap`, a service handler, an interrupt handler, a kernel), is only ever called (never passed as a value), and can't be exported to C (ADR 0014: not every parameter and the result `Int`, `F64`, `Bool` or `Str`). A conversion in a closed function that isn't proved on its own is proved if it is proved at each of its calls, which is where its encoding in place puts it, up to three callers out. So `fn latency(s: Status, ms: F64)` whose callers pass `elapsed_ms(t0, t1)` is safe.

### Cost

Source that doesn't have `.round`, `.floor`, `.ceil` or `.trunc` costs `check` one search of its text; otherwise a walk of the package's own code finds the conversions. With conversions, only the functions that have them and their callers are encoded. Queries the cache doesn't have are answered on every core and cached in `smt2/` like the others; the first 64 cache lookups of a process look for the three answer files instead of listing a cache directory, which is about 60 times cheaper and also helps small native builds. Without Z3 (or with `SSPUR_SMT=off`), `check` prints `note: N F64 to Int conversions are not checked`.

The warning comes from `sspur check` (text, `--json`, and the codebase `HEAD`), the language server and the MCP `check` tool. `run` and `test` don't run it, so their startup is unchanged, and neither does the playground, since `sspur-smt` isn't in the WebAssembly build, which is unchanged.

### Native code

Native code keeps its range check. It is two comparisons on a branch that is never taken, the corpus has no conversions for `corpus_diff` to check a change against, and codegen would have to run the second pass and its queries in every cold build.

## Result

In this repository the warning found 30 conversions in 14 files that can trap or that it can't see are safe: display code rounding values from fetched JSON in the Pulse sections, `ticks_min` and `nice_unit` in Pulse's chart library (a `count` of 0, infinite bounds), the benchmarks' printed checksums, a handbook example, and test programs. All are fixed with a guard, a refinement or `checked_*`, except the three in the frozen copy of the Pulse library in `tests/compat`, whose golden gains three `check` lines. In 32 packages (`pkgs`, `backend-pkgs` and their example and test packages) the first version warned 9 times and this one twice: `quantile_sorted` and `latency` are proved at their callers, `best_fraction`'s continued-fraction loop by an invariant, and `bucket_wait_ms` by the IEEE rules for infinities. The two left aren't provable here: the conversion of the result of a `pub` function that rounds by parsing a string, and one in a private function that `export-c` would export, where a C caller can pass `-inf`.

`check` takes the same time and memory as before where there are no conversions (24 of those 32 packages). Where there are, it takes at most about 5 ms more with the cache warm (median of 15 interleaved runs), and up to 0.95 s more on a cold cache (`numeric`, 28 queries). With ADR 0043's linker order file, the peak footprint of `--version`, the cached `run`, `run --interp`, `fmt` and `test` of Pulse's library is the same within 48 KB and their times within noise; `check` of Pulse's library, which has 6 conversions to prove, takes 0.5 ms and 80 KB more. The playground's WebAssembly is byte for byte the same.

## Verification

`conversions_are_proved_from_refinements_guards_and_contracts` in `crates/sspur-smt/src/tests.rs` covers the patterns above and the boundaries (`9.223372036854775e18.floor` is safe; `9.223372036854775807e18.round`, `n.to_f64.floor`, `x.floor` under `x > 0.0` (NaN), under `x < 9.2e18` (-inf), a var a closure assigns, and a helper exportable to C aren't); `check_warns_about_conversions_it_cannot_prove` covers the output, `--json`, `run` and a missing Z3. A soundness fuzzer generated 502 programs of 30 functions with random refinements, `pre`s, guards, expressions over every modeled method, literals at the boundaries, private helpers, vars, loops and lambdas that assign vars, and ran each of the 8,287 functions `check` proved safe on up to 6,084 inputs (NaN, the infinities, -0.0, subnormals, values around 2^53 and 2^63), 401 programs interpreted and 101 natively: none trapped. The 199 corpus programs (`corpus_diff`), the whole test suite and `sspur fuzz --gen 200` with seeds 2026 and 99 pass, and the generated C of the test programs and benchmarks is unchanged.
