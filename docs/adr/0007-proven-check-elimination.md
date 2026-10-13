# ADR 0007: Removing runtime checks the compiler can prove

Status: accepted, 2026-10-03

## Problem

Native code checks every integer operation for overflow, every division for zero, every index for bounds, and every contract on entry. Most of these checks can never fail, and C++ code written for speed omits them.

## Decision

The release code generator runs an interval analysis over integers during codegen (`crates/sspur-native/src/cgen/prove.rs`). A check is emitted only when the analysis cannot prove that it never fails.

| Source of facts | Example |
|---|---|
| Literals and arithmetic | `x / 2` and `x % 2` need no zero or overflow check |
| Parameter refinements, alias refinements, `pre` | `fn fib(n: Int where _ >= 0)` gives `n >= 0` in the body |
| Branch conditions, including `and`/`or` short-circuit | In `if n < 2 then .. else fib(n - 1)`, the else branch knows `n >= 2`, so `n - 1` is unchecked |
| `for i in a..b` | `i` lies in `[a, b - 1]`; with `b = xs.len` (or `n = xs.len`), `xs[i]` has no bounds check |
| `let` bindings | The interval of the bound expression |
| `var` assignments | Least fixpoint over every assignment, widened to infinity after two rounds |
| `post` with literal bounds | The callee's result interval |
| Lengths | `len` is in `[0, 2^47]` (address-space bound) |

Facts apply only to immutable bindings. `var`s get a whole-lifetime invariant instead of path facts.

Contracts are proved at each call site. A function with entry checks compiles to a checking wrapper `f_name` and a body `f_name__np`. When the arguments' intervals prove every refinement and `pre`, the call goes straight to `__np`. Recursive calls such as `gcd(b, a % b)` therefore pay for the check once, at the outer call.

## Why intervals, not an SMT solver

The facts that matter in practice are bounds against constants and `i < xs.len`. Intervals decide them in microseconds, deterministically, with no external dependency. An SMT backend can be added behind the same `holds` interface if relational contracts (`pre a <= b`) become common.

## Result

All checks are gone from `gcd`, from `fib`'s recursion, and from division and indexing in typical loops. Benchmark runtime barely moves (compute stays at 0.95x C++): the checks that remain in the hot loops, such as `3 * x + 1` and the running sums, really can overflow, and the removed ones were already cheap, well-predicted branches.

## Verification

Native and interpreter results match on all 199 corpus programs (normally and under GC stress) and under differential fuzzing. `proven_check_removal_keeps_real_traps` in `crates/sspur-native/tests/parity.rs` covers the boundary cases: `a < MAX` versus `a <= MAX`, `MIN / -1`, negating `MIN`, call-site `pre` proofs, and shadowed lists.

## Update, 2026-10-13: facts about locals, signs and selects

### The analysis had stopped seeing locals

Since the per-definition translation units (ADR 0022), the names the code generator gives locals contain a `$` (`v_x_$6`). The analysis only accepted names made of letters, digits and `_`, so it silently ignored every local: `let` bindings, `var` invariants, `for` indices, and the temporary names it uses itself to prove a call's contract from a computed argument. Only parameters still had facts. With `$` accepted again, everything in the table above applies as it was designed; in `compute_big`, `steps + 1`, `d + 1` and `n % d` in `is_prime` lost their checks, and in `typical` a record constructor's refinement checks and the overflow check of its random-number generator. The in-place map update of ADR 0008, where `m.put(k, v)` after `m.get(k)` of the same `k` rewrites the node the lookup found, had the same filter and works again, which makes `app` 14% faster.

### Unsigned division by proven-positive operands

`a / b` and `a % b` where the analysis proves `a >= 0` and `b > 0` compile to unsigned division, which gives the same result without signed division's sign fix-ups. A power of two becomes a shift or a mask: in the Collatz loop of `compute_big`, `x` starts positive and the fixpoint over `x / 2` and `3 * x + 1` (whose overflow check traps) gives `x >= 0`, so `x % 2` is `x & 1` and `x / 2` is `x >> 1`. Operands that can be negative keep signed division, so `-7 / 2` is still `-3`.

### Selects instead of branches for small checked arms

`if c then a else b` over `Int`, where `a` and `b` are small (at most six operations on variables and literals, no calls, no indexing, division only by a nonzero literal) and at least one of them has an overflow check, compiles to straight-line code: both arms are computed with wrapping arithmetic, each with an exact overflow flag, the chosen arm's flag traps, and the result is a select. A multiplication by a literal is checked as a range check on the other operand (one comparison when its sign is known, `x > 3074457345618258602` for `3 * x`); a check the analysis or the solver proves away is not flagged. Every flagged check is an integer overflow, so the trap is the one the first failing check of the chosen arm would have raised, and the other arm can't trap, has no effects and is thrown away. A program traps exactly when and how it did before.

Before, the trap in the odd arm kept clang from converting the branch on `x % 2 == 0`, which is mispredicted about half the time. Timed alone, the Collatz part of `compute_big` went from 0.70s to 0.59s (C++ 0.72s), and the whole benchmark from 1.02s to 0.91s against 1.02s for C++.

### Verification

`proven_signs_and_selects_keep_exact_results_and_traps` in `crates/sspur-native/tests/parity.rs` checks the select and the unsigned forms at the boundaries: an odd `i64::MAX` traps and an even `i64::MAX - 1` doesn't, `i64::MAX / 3 + 1` traps and `i64::MAX / 3 - 1` doesn't, `i64::MIN` and negative odd and even values, the Collatz loop of a value whose first step overflows, and division and remainder by 3 and 4 of negative and positive values with and without a sign refinement. The corpus (199/199, also under `SSPUR_MEM=check`), the whole test suite and `sspur fuzz --gen 200` with seeds 2026 and 99 (0 findings) pass.
