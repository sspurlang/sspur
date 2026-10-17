# ADR 0041: Cold native builds of big packages

Status: accepted, 2026-10-17

## Problem

A cold native build of a big package was slow. On a synthetic package of 200 modules, 2.55 MB of source and about 8,750 functions (`bench/bigpkg/gen.py`), a cold `sspur test` took 280 to 400 s and a cold `sspur publish --dry-run` about 290 s, against about 10 s and 28 s warm. A dependent's first `sspur run` paid the same again.

Profiled on Apple M1 Pro (10 cores) with `sample` and `SSPUR_SPLIT_DEBUG=1`, the cold `sspur test` spent:

| Phase | Time | What |
|---|---|---|
| Load and check | 4 s | linking the modules, the check cache |
| Optimize, lower, generate C | 17 s | most of it 9,700 Z3 questions asked one at a time, and opening a cache file per question |
| Split into units | 24 s | `split::units` walked every item of the program's C once for each of the 8,758 units |
| Compile | 242 s | 8,758 units, at most 4 clang processes |
| Link | 2 s | |

Compiling was the cost, and it was mostly not the functions' code. Each unit repeats the declarations, types and helpers its function uses (about 22 KB of C for one small function), and clang spent about 50 ms parsing that and 40 ms compiling the copied helpers again, against about 12 ms for the function itself: the 133 units of one module took 12.2 s in one clang process, and the same module as one translation unit 1.6 s. The jobs were also capped at 4 (ADR 0022 decision 9), on a 10-core machine.

## Options

1. **More clang processes.** One per core instead of 4: about 2.5 times faster here, nothing else changes. Taken.
2. **Fewer, bigger units.** Units of a whole program (`SSPUR_SPLIT=0`) give the least repeated work, but an edit then recompiles everything (ADR 0022). Grouping only the functions of big packages keeps the program's own functions in their own units. Taken, measured below.
3. **A precompiled header** for the part every unit shares. The shared part is only the system headers (about 20 ms of the 90 ms); the rest differs per unit, so a header saves little.
4. **Lower optimization for test builds.** `-O1` compiles as slowly as `-O2` on this code (1.56 s against 1.61 s for the module above); `-O0` is 3.5 times faster to compile but runs the tests slower, and its objects can't be shared with `sspur run`, which needs `-O2`. Not taken.
5. **Tiered start**: run the tests in the interpreter while native code compiles, switching per function. Not done: with the other changes the cold build of this package is under a minute, and switching a running test between tiers would need the interpreter and native code to share live values.
6. **Reusing a package's objects in its dependents.** Objects are already cached by the text of their unit (ADR 0022), so two dependents of a package share them (ADR 0026 decision 2). A package's own `sspur test` names its modules `m001__f`, while dependents and `sspur publish` name them after the package (`bigpkg_m001___f`), so those builds can't share objects; sharing them would mean building a package's modules under the published names in the package itself. Not done.
7. **Z3 in parallel.** The questions don't depend on each other's answers. Taken.

## Decision

### Units

`unit_groups` in `sspur-native/src/cgen.rs` decides which definitions share a unit:
- A function of the program itself keeps its own unit, as before, so editing it recompiles it and the callers that copy it.
- The functions of a package or module with at least 16 functions (names `lib__f`, the linking prefix of ADR 0026) are put in units by a hash of their name, in `next_power_of_two(ceil(n / 32))` units for `n` functions, 16 to 32 functions on average. A unit holds one package's functions only.
- A debug build (`--debug`) keeps one unit per definition.

A function's unit depends only on its name and its package's function count, so a dependency's units have the same text in every program that uses it and stay cache hits (tested in `crates/sspur-cli/tests/packages.rs`), and adding a function to a module moves other functions only when the count crosses a power of two. `split::units` takes the groups: an owner's definitions and the copies of small callees go in the group's unit, and the copies allowed per unit (24) are multiplied by the group's size. With a group of one, a unit's text is byte for byte what it was, so programs without big packages, including every benchmark, compile to the same objects.

Measured on the package above (cold `sspur test`, compile phase only): one unit per definition 266 s with 4 jobs; groups of about 8, 16 and 32 functions 50 s, 35 s and 26 s with 10 jobs; a unit per module 24 s. Groups of up to 32 keep most of the gain, and an edit in one of the package's 44-function modules recompiles about half of that module.

### Jobs

`flags::jobs` runs one clang process per core (`available_parallelism`), but more than 4 only when each gets at least 256 KB of C (about a second of work), and fewer when memory couldn't hold that many compiles of the biggest unit (about 64 MB plus 320 bytes per byte of C each, within half of the available memory on Linux and Windows and a quarter of the physical memory on macOS). `SSPUR_JOBS` sets the number. A build of up to 4 jobs batches its units as before (`ceil(units / (2 * jobs))` per clang process, at most 16), so a small program starts the same processes as before. A bigger one sorts the units by size, biggest first, and batches them into about a quarter of each job's share of the C, at most 16 units, so no long compile is left for the end. When `sspur publish` tests a package's modules side by side, each run gets its share of the cores.

`split::units` builds the units on every core and only visits each unit's own items, so splitting the big package takes 0.5 s instead of 24 s.

### Z3

`compile_release` generates the C inside `sspur_smt::collecting(64, ..)`: the first 64 questions that miss the cache are answered as usual, and later ones get `Unknown` and are kept. When some were kept, `sspur_smt::solve_all` answers them into the cache with one Z3 process per core, and the C is generated again with the answers. A warm build or a small edit asks fewer than 64 new questions and generates once, as before. The C of the second pass is the C one pass would give, since every answer it uses comes from the cache or from Z3 (checked: the library of the big package has the same hash both ways).

The SMT cache is now `SSPUR_CACHE/smt2/KEY[..2]/KEY.ANSWER`, empty files whose names are the answers. The first question in a subdirectory lists it, so a warm build reads one directory per 256th of the keys instead of opening a file per question (4.5 s of a 12 s warm `sspur test` here, much of it the endpoint security software checking each open). A file is complete as soon as it exists, so no temporary file and rename are needed, which also suits Windows. Answers in the old `smt/` directory are not read; Z3 answers them again once.

## What doesn't change

- The C of a program without packages of 16 or more functions, its objects, the cached-run path of ADR 0034 and its startup.
- What native code computes: grouping moves definitions between units, and the per-unit cache key is still the unit's text. The corpus check (199/199), the fuzzer and the suite run with `SSPUR_STRICT_NATIVE=1`.
- Windows: objects and libraries are still published with `flags::publish` (a hard link that loses cleanly to a concurrent build), the link still retries `LNK1104`, the memo still only counts a name that is a file.

## Verification

- `split::tests::big_packages_are_grouped_by_name` (one package per unit, stable groups, a new function moves no other) and `grouped_units_hold_each_definition_once`; `packages::a_big_dependency_compiles_in_grouped_units_that_dependents_share`.
- `SSPUR_FILL_MS=5000 SSPUR_STRICT_NATIVE=1 cargo test --release --no-fail-fast`, `tools/corpus_diff.py` (199/199), `sspur fuzz --gen 200` with seeds 2026 and 99 (0 findings), `pulse/test.sh`, clippy for the host, `x86_64-pc-windows-gnu` and `wasm32-unknown-unknown`, and a release build for `x86_64-pc-windows-gnu`.

## Result

Apple M1 Pro, 10 cores, 32 GB, macOS with endpoint security software running, `bench/bigpkg/run.sh` with 0.14.0 and this build (both `cargo build --release`) taking turns on every step; each cold step starts from an empty `SSPUR_CACHE`. Wall time and the peak of the summed RSS of the process tree (`sspur` and its clang and Z3 processes), sampled every 0.1 s:

| Step | 0.14.0 cold | now cold | 0.14.0 warm | now warm |
|---|---|---|---|---|
| `sspur check` | 5.0 s, 218 MB | 4.7 s, 221 MB | 1.0 s, 223 MB | 1.0 s, 227 MB |
| `sspur test` (native) | 282 s, 542 MB | 40 s, 1.2 GB | 9.8 s, 459 MB | 4.9 s, 407 MB |
| `sspur test --interp` | 4.5 s, 228 MB | 4.5 s, 224 MB | 1.0 s, 225 MB | 1.0 s, 222 MB |
| `sspur publish --dry-run --no-gh` | 291 s, 833 MB | 59 s, 1.3 GB | 27.8 s, 561 MB | 21.3 s, 683 MB |
| A dependent's first `sspur run` | 251 s, 715 MB | 41 s, 1.2 GB | 8.8 s, 391 MB | 4.9 s, 412 MB |
| `sspur test` after editing one function of a module | | | 36.0 s, 836 MB | 6.8 s, 610 MB |
| `sspur test` of `examples/packages/app` (median of 20) | 1.33 s, about 200 MB | 1.33 s, about 200 MB | 0.02 s | 0.02 s |
| `sspur run` hello world (median of 15) | 0.92 s, 93 MB | 0.91 s, 92 MB | 0.01 s | 0.01 s |

The cold builds' extra memory is the 10 clang processes at once; clang peaks at about 135 MB on a unit of 400 KB of C, the size of the biggest here. The warm builds gain from the SMT cache; `publish` warm is mostly the registry checks themselves.

Programs without big packages compile to the same objects and libraries as in 0.14.0: every program of `bench/native` builds with 0 units compiled from a cache that 0.14.0 filled, and links to the same library names. Best of 5, interleaved (0.14.0, now): `compute_big` 0.912 s, 0.916 s; `typical` 0.134 s, 0.149 s (best of 7 again: 0.114 s, 0.113 s); `strings_big` 0.061 s, 0.059 s; `app` 0.108 s, 0.106 s; `churn` 0.029 s, 0.031 s; `parallel` 0.309 s, 0.345 s (best of 7 again: 0.280 s, 0.285 s); `simd` 0.168 s, 0.171 s; `gpu` 0.270 s, 0.275 s.

`sspur run` of hello world from the cache (ADR 0034), 320 interleaved runs: 10.35 ms median for 0.14.0, 10.47 ms for a copy of it signed again and 10.34 ms for this build, with a peak RSS of 2.4 to 2.5 MB. The normal path (`--no-cache`, 150 runs): 13.57 ms, 13.78 ms and 13.59 ms, 6.3 MB. (The endpoint security software takes up to about 0.6 ms longer to start a binary whose code signature it has seen less often, which is why a re-signed copy of the old binary is in the comparison.)
