# Native performance versus C++

`sspur run` and `sspur test` compile to native code by default: generated C, optimized by clang/LLVM, and cached by content hash. `--interp` forces the interpreter. `--native` uses the Cranelift JIT dev tier.

## Results (Apple M1 Pro, 2026-10-13, best of 7, wall time of the whole process)

| Workload | File | C++ clang -O2 | SSPUR | Ratio | Peak memory C++ / SSPUR |
|---|---|---|---|---|---|
| Compute-heavy: recursion, loops, primes, gcd | `compute_big` | 1.02s | 0.91s | **0.89x** | 1 MB / 3 MB |
| Records, lists, persistent trees, float simulation | `typical` | 0.82s idiomatic, 0.26s hand-tuned (arena, never frees) | 0.11s | **0.14x** idiomatic, **0.44x** tuned | 70 MB / 11 MB (tuned: 212 MB) |
| Strings: build 2M words, lowercase, split, count, sort | `strings_big` | 0.10s | 0.06s | **0.57x** | 22 MB / 24 MB |
| App: generate CSV, parse with errors, aggregate in a map, report | `app` | 0.17s | 0.10s | **0.59x** | 24 MB / 17 MB |
| Long-running loop: 200,000 iterations, about 400 MB of short-lived lists | `churn` | | 0.03s | | 3 MB |
| Pure pipelines: Collatz, primes, hashed table, Mandelbrot (10 cores) | `parallel` | 2.11s (single-threaded) | 0.31s | **0.15x** | 17 MB / 18 MB |
| Data-parallel numeric: checked Int map/filter/sum, squares, counts, mapped lists, F64 dot and saxpy over 1M-element lists | `simd` | 0.33s | 0.16s | **0.48x** | 40 MB / 34 MB |
| GPU kernels (Metal): F32 saxpy over 4M elements, two 4M reductions (strided partial sums, shared-memory tree), naive and tiled 512 x 512 matmul, a 256-bin histogram with atomics | `gpu` | 0.84s (single-threaded CPU) | 0.32s | **0.38x** | 54 MB / 92 MB |

The machine had heavy background load during this run (endpoint agents kept two to four cores busy, load average 18 to 20), so every column was measured in the same session, alternating runs; C++ times are higher than in quieter sessions (`typical` 0.75s, `app` 0.15s), and `gpu` varies by about 10% from run to run. `compute_big` is now 11% below C++. Most of that is the Collatz loop, which went from 0.70s to 0.59s against 0.72s for C++: its `if x % 2 == 0 then x / 2 else 3 * x + 1` now compiles to a select of both arms, a shift and a single range check for the multiplication, instead of a branch on the parity that is mispredicted about half the time (ADR 0007). The same change, and the interval facts behind it, made `compute` 14% faster, `app` 14% (a map lookup followed by a `put` of the same key reuses the node it found), `parallel` 7% and `simd` 5%; nothing got slower beyond run-to-run noise.

Against C++ -O3 (same machine, 2026-10-03): compute 0.88x, typical 0.18x (hand-tuned C++ -O3 0.20s, so 0.60x), strings 0.54x, app 0.60x, parallel 0.15x. -O3 is no faster than -O2 on these workloads.

An unchanged program runs from the cache without parsing or checking it (ADR 0034): `sspur run` of hello world takes 9.6 ms, against 7.2 ms for a C hello world and 8.3 ms for `sspur --version` (10.6 ms before native libraries were linked with dead stripping, 12.7 ms before the cached path), and the normal path, taken after any change or with any flag, adds 3 to 15 ms on these workloads. Most of what is left over C is the dynamic linker binding the `sspur` binary's 200 libc imports at every start, about 40,000 instructions each. When speed and memory trade off, SSPUR picks speed (ADR 0008).

- Checks the compiler proves can never fail are omitted (ADR 0007). The rest stay.
- Every C++ version carries the same safety checks SSPUR always has (overflow, bounds, contracts) via `__builtin_*_overflow` and `abort()`.
- The output of each pair is identical (checked by `diff`).
- `parallel` compares against single-threaded C++ on purpose: SSPUR parallelizes a pure pipeline with no code change, and its traps and results stay identical to sequential execution for any `SSPUR_THREADS` (default: online cores; `1` disables). The machine had other load during measurement; with `SSPUR_THREADS=1`, SSPUR is 0.96x C++.
- In `simd` (single-threaded, per 40 rounds), the checked Int kernels take 18 ms (`affine`) and 21 ms (`squares`) against 35 ms each before ADR 0015 and 42 ms in C++. The F64 dot product ties C++ (sums keep sequential order). The two kernels that build new 8 MB lists used to lose to C++ on first-touch page faults in the GC heap; since ADR 0033 each list is freed after its `sum` and the next round gets the same pages back, which took the whole benchmark from 0.20s to 0.18s and its peak from 273 MB to 39 MB.
- `gpu` (ADR 0020, 2026-10-04, Apple M1 Pro, C++ built with `-ffp-contract=off` so both sides round every `F32` operation): saxpy 100 launches 83 ms C++ vs 72 ms (1.15x), reduction 100 launches 65 ms vs 47 ms (1.4x), matmul 20 launches 247 ms vs 56 ms (4.4x; the kernel alone takes 2.2 ms against 12.4 ms per multiply). Output is bit-identical to C++, to the interpreter and to `SSPUR_GPU=0` (the exact sequential C fallback, 5.1s). Launches are synchronous, about 0.25 ms each, and the CPU shares the unified memory bandwidth, so the memory-bound kernels gain little; SSPUR's 62 ms setup builds the inputs as host lists before upload.
- `gpu` after ADR 0023 (2026-10-04, same machine, best of 3, output bit-identical to C++, the interpreter and `SSPUR_GPU=0`): launches are queued on one command buffer and synchronize only when the host reads a buffer. saxpy 100 launches 63 ms C++ vs 47 ms (was 73 ms with a wait per launch; the launch loop alone drops from 62 to 40 ms), strided reduction 65 ms vs 44 ms (1.5x), naive matmul 284 ms vs 60 ms (4.7x), shared-memory tree reduction (8 elements per thread, 256-thread tree, read back after each of 100 launches) 35 ms vs 31 ms (1.1x, memory bound plus one wait per read), tiled 16 x 16 matmul with `@grid2` and two shared tiles 284 ms vs 34 ms (8.4x; the result equals the naive one bit for bit), histogram of 4M values into 256 bins with shared atomics then global atomics, 20 launches, 53 ms vs 14 ms (3.8x). Whole program 0.81s vs 0.35s; the exact sequential C fallback (`SSPUR_GPU=0`) takes 7.5s.
- A cold first build adds about 0.6 to 0.9s of clang time, once. After that it's cached under `~/.cache/sspur/native/`. Each definition is its own cached object (ADR 0022), so after an edit only the changed definition and the callers that inline it recompile; `--O3` keeps a whole-program build. Measured per-object against whole-program: every benchmark within run-to-run noise.

## Memory

Native code frees what the compiler can prove has one owner at its last use, and a garbage collector reclaims the rest (ADR 0033 and ADR 0006):
- list buffers and strings that are fresh, owned by one local or temporary and only read afterwards are freed right after the last read; a `var` frees its old value when it gets a new one, and a list of strings it built from fresh strings frees them with it
- a parameter that a function only reads is borrowed, so the caller frees a fresh argument after the call; a `Map.put` that takes an owned key frees it when the key was already there
- `var v = []` that is only pushed onto and then joined once is built as a string directly
- `xs.map(f).fold(init, g)` and `xs.filter(p).fold(init, g)`, where `g` has no effects and captures nothing, run as one loop over blocks of 64 elements and never build the mapped list; if `g` traps, the trap is held until `f` has seen every element, so a later failure of `f` still wins, as in the unfused order
- freed small blocks go to a per-size LIFO list the allocator takes from, and freed pointer-free runs over 32 KB stay mapped (up to 64 MB) for the next allocation of the same size, so a loop that frees and allocates reuses the same memory
- everything else (shared values, records, sums, maps, closures, `var` cells, anything that crosses a thread) is collected: mark-sweep over size-classed 64 KB pages in one reserved 8 GB region, conservative scanning of registers and the stack, bump allocation and lazy sweeping, a collection after max(256 MB, 4x live) of allocation

Peak memory and time, C++ against 0.8.1 (collector only) and 0.8.2, best of 5 (the 0.8.1 column was measured earlier the same day):

| Workload | C++ | 0.8.1 | 0.8.2 | 0.8.2 with `SSPUR_MEM=gc` | Blocks left for the collector at exit (0.8.2) |
|---|---|---|---|---|---|
| `compute_big` | 1.01s, 1 MB | 1.03s, 7 MB | 1.02s, 3 MB | 1.04s, 3 MB | 0 |
| `typical` | 0.75s, 70 MB | 0.11s, 16 MB | 0.11s, 11 MB | 0.10s, 11 MB | 329,508 (8 MB): tree nodes and records, which are never freed early |
| `strings_big` | 0.09s, 23 MB | 0.06s, 92 MB | 0.06s, 25 MB | 0.05s, 55 MB | 2 (11 MB): the lowered text and the sorted list, which the printed result points into |
| `app` | 0.15s, 24 MB | 0.13s, 137 MB | 0.12s, 17 MB | 0.12s, 114 MB | 134 (10 MB): the generated CSV, which the split lines point into, and the map |
| `churn` | | 0.08s, 268 MB | 0.03s, 3 MB | 0.08s, 264 MB | 0 |
| `parallel` | 2.09s, 17 MB | 0.35s, 23 MB | 0.31s, 18 MB | 0.32s, 18 MB | 0 |
| `simd` | 0.31s, 40 MB | 0.20s, 273 MB | 0.16s, 34 MB | 0.18s, 268 MB | 2 (16 MB): the two F64 inputs, which `dot` and `saxpy` read inside a lambda, so they aren't borrowed |
| `gpu` | 0.82s, 54 MB | 0.31s, 344 MB | 0.28s, 92 MB | 0.26s, 92 MB | 0 |

Every benchmark is as fast as in 0.8.1 or faster, and each one's peak is 4 to 5 MB lower since `sspur run` stopped holding the checked program while `main` runs. `gpu` is still above C++ in memory, for what a GPU program keeps that the CPU-only C++ version doesn't: 52 MB of device buffers (C++ keeps the same data in about 50 MB of vectors), 18 MB of host copies of the buffers kernels write (kept so a launch that flags a thread can be rerun exactly on the CPU), and 14 MB for Metal itself. It no longer builds a host list before each upload: `dev_f32((0..n).map(f))` writes each value straight into the device buffer and `d.to_list.sum` sums the buffer in place, and the per-launch masks of flagged threads are no longer cleared up front (a new Metal buffer is already zero), which kept 512 KB resident for each of up to 256 queued launches. `app` used to hold its 400,000 parsed sales (19 MB) next to the 10 MB CSV they point into; the parse and the aggregation now run as one fused fold, so the sales are never stored, and it peaks below the C++ version, which keeps a copy of every line. `strings_big` and `app` are also helped by two fixes outside ADR 0033 that `SSPUR_MEM=gc` keeps: `text.lower.words.counts` now counts while scanning instead of building the 2M-word list, and `split` no longer allocates a buffer it throws away.

The whole heap is released when native code returns to the interpreter. `SSPUR_MEM=gc` turns the frees off for A/B runs; `SSPUR_MEM=check` poisons freed blocks instead of reusing them. Stress mode is `SSPUR_GC_STRESS=<bytes>`, which collects every N bytes. The suite (4 KB and 64 KB), the benchmarks (3 MB), and all 199 corpus programs produce identical results under it. `SSPUR_GC_STATS=1` prints per-collection statistics and, at exit, the blocks freed at last use and the blocks left for the collector.

## Coverage

- Every function in `tests/programs/` (18 programs) and in the 199-program evaluation corpus written by other models (247 of 247 functions) compiles natively.
- Native results are identical to the interpreter on all of them, plus 300 random inputs per function (`sspur fuzz --differential`).

## Why it matches or beats C++

| Technique | Effect |
|---|---|
| Lists are immutable vectors that grow in place at the buffer's end | O(1) amortized `push` with value semantics |
| Bump allocation, frees at last use proven at compile time (ADR 0033), a collector for the rest | No reference counts at run time (`shared_ptr` costs 3.6x on `typical`), no per-object `malloc` |
| Payloadless constructors are static singletons; one-payload sums are nullable pointers | Smaller, fewer allocations (Rust-style niche optimization) |
| List methods compile their lambdas inline | `xs.filter(..).map(..).sum` becomes plain loops |
| **In-place reuse** (ADR 0008): a tree updated through `t := insert(t, x)` from a fresh value is uniquely owned, so `insert` rewrites the matched node; `ps := ps.map(f)` maps its buffer in place | One allocation per insert instead of a path copy; no per-step buffers |
| **Pipeline fusion**: `filter`/`map` chains ending in `sum`, `len`, `map`, or `filter`, `(a..b).map(f)`, and a `map` or `filter` folded by an effect-free function | One loop, no intermediate lists |
| **Linearity analysis**: a map built from `empty_map()` and consumed exactly once per step (`fold`, or `var m` with `m := m.put(..)`) is mutated in place | Persistent semantics, with in-place speed when the compiler proves no one else can observe the old version |
| `Map` is a persistent treap with key-ordered iteration | O(log n) `put`, versus the interpreter's O(n) copy |
| `counts` uses a hash table consistent with SSPUR equality, sized to the distinct keys; `s.words.counts` counts while scanning | O(n), first-seen order preserved, no intermediate word list |
| ASCII fast paths for case mapping, `words`, and `trim` | Unicode-exact host fallback only for non-ASCII text |
| Single-allocation `join` with inline short copies; `for` loops that push once per iteration reserve capacity up front; hand-written integer formatting | No `snprintf`, no reallocation chains |
| Traps `longjmp` from a cold function to the entry; calls to functions that can't `raise` carry no result check; self tail calls are loops | Safety checks stay off the hot path; recursion costs what it does in C |
| Generic functions monomorphized; closures are a function pointer plus an arena environment | Template-like specialization; no boxing |
| **Automatic parallelism** (ADR 0009): fused `map`/`filter` pipelines ending in an Int `sum`, `len`, or a scalar `map`, whose lambdas and callees are effect-free (only `div` allowed) and scalar-only, run on a pthread pool once a timed sequential warm-up predicts more than 100 microseconds of work | 6x on 10 cores with bit-identical results and traps; float sums stay sequential |
| **Vectorizable checks** (ADR 0015): fused Int and F64 pipelines compute 64-element blocks branch-free, OR-ing overflow, range, and bounds conditions into one flag; a flagged block re-runs sequentially to report the exact trap. Under a checked small-value assumption the interval prover removes most per-element checks | clang emits NEON/SSE code for checked arithmetic; checked Int kernels about 2x faster than before and than C++ with the same checks |
| **Proven rewrites** (ADR 0022): inlining that exposes a pipeline, loop fusion and `map`/`map` composition when the trap order provably can't change, overflow-aware folding, dead branches removed by intervals or Z3 | `simd` 15% faster (no 1M-element list per round), `typical` peak memory 85 MB to 15 MB; `sspur explain-opt` lists each rewrite and its proof |

## Tiers

| Tier | Flag | Coverage | Speed |
|---|---|---|---|
| Interpreter | `--interp` | everything | reference semantics; also powers `fuzz` with step budgets |
| Cranelift JIT | `--native` | Int/Bool functions | about 1.66x C++, compiles in about 10ms |
| Native (default) | none | the whole language | 0.14x to 0.89x C++ (faster on every benchmark; see Results) |
