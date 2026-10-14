# ADR 0005: Native compilation by default

Status: accepted, 2026-10-03

## Decision

`sspur run` and `sspur test` compile every program to native code through the C/clang tier. The interpreter remains as the reference semantics, the `--interp` opt-out, the fallback when no C compiler is available, and the engine behind `fuzz` (which needs step budgets).

## What the native tier covers

All language features used by the suite and the evaluation corpus:
- types: scalars, `F64`, `Str` (UTF-8, byte-ordered), records, sums (including recursive and generic), tuples, `List`, `Opt`, `Map`, newtypes, `Secret`, `Pii`, `Untrusted`, `Guess`
- control flow: `match` (tuples, guards), `with`, `while` and `for`, `return`
- errors and effects: `raise`/`catch` (statically resolved handlers, plus propagation across calls and the native/interpreter boundary), `log`
- functions: rule tables, contracts and refinements, closures, local function groups (mutually recursive), and generic functions (monomorphized)

## Semantics guarantees

1. Every trap message is byte-identical to the interpreter's: overflow, division by zero, bounds, contracts, refinements, unwrap, rule gaps, repeat, guess.
2. Float equality and ordering use total order, as the interpreter does, so `-0.0 != 0.0` and NaN is ordered.
3. Display formatting matches, including float `{:?}` formatting and string debug-escaping, both delegated to the Rust host.
4. Unicode-sensitive string operations use ASCII fast paths and an exact host fallback.
5. In-place mutation happens only when the linearity analysis proves no other reference can observe the previous version. Otherwise values stay persistent (see `keep` in the tests).

## Verification

- `crates/sspur-cli/tests/suite.rs::native_release_matches_interpreter_on_every_suite_program` requires zero skipped functions and identical test results.
- `sspur fuzz --differential` compares native and interpreted results on random inputs (300 per function).
- 199 corpus programs written by other models produce identical results.
- The differential fuzzer found one interpreter bug, now fixed: summing an empty `List[F64]` returned `0` instead of `0.0`.

## Consequences

- Programs need `clang` (or `$CC`) for full speed. Builds are cached by a BLAKE3 hash of the generated C.
- The next performance step is erasing proven contracts and overflow checks with SMT (Phase 4), which hand-written C++ can't do safely.

Update (0.8.2, ADR 0034): a plain `sspur run` of a program that hasn't changed since its last run skips parsing, checking and code generation and calls the cached library's `main` directly, so native by default no longer costs a check on every run.
