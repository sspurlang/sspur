# ADR 0033: Freeing at last use, in front of the collector

Status: accepted, 2026-10-13

## Problem

Native code allocated everything on the conservative collector of ADR 0006 and collected only after max(256 MB, 4x live) of allocation (ADR 0008, speed first). Most benchmarks never reached a collection, so their peak memory was simply everything they had ever allocated: `simd` peaked at 273 MB against 40 MB in C++, `app` at 137 MB against 24 MB, `churn` at 268 MB for a program that keeps almost nothing. The goal for 0.8.2 was memory near C++ without giving up any speed.

## Options

1. **Lower the collection threshold.** Cheap to build. Measured on two allocation-bound programs that keep old versions of a map or a tree alive (about 100 MB live): a 64 MB minimum made them 8% and 21% slower and saved nothing, because 4x live dominates; a factor of 2 instead of 4 cut their peaks from 370 MB and 330 MB to 254 MB and 208 MB and made them 36% and 52% slower. Rejected: the collector's cost is tied to the live set, not to the garbage.
2. **Full Perceus** (Reinking, Xie, de Moura, Leijen, PLDI 2021): a reference count in every heap block, `dup` and `drop` inserted from the typed IR, reuse of dropped cells, borrowed parameters, no collector. This is what the design notes promised. It doesn't fit SSPUR's native representation as it is:
   - A `Str` is a `(len, ptr)` slice that may point into the middle of another string (`split`, `words`, `trim`, `take` all return slices), and list versions share one buffer (`push` grows in place, `take` and `drop` are views). Counting them means finding the block header from an interior pointer on every `dup`, which is a page-table lookup.
   - Every helper in the C runtime (about 3,000 lines) and every piece of code the 15 generator modules emit (maps, heaps, sets, show, compare, hash, encode, closures, views, GPU transfers, parallel workers, FFI) would need an ownership convention, and every one of them is a place to get a count wrong. A missed `dup` is a use after free.
   - Atomic counts would be needed for values shared with `par` tasks, threads and channels.
   The parts of Perceus that pay off here are its static ones: ownership, drop at last use, borrowed parameters and reuse of a dropped block by the next allocation of its size. The runtime count is the part that costs speed and correctness risk.
3. **Static ownership in front of the collector** (chosen). The compiler frees a block when it can prove the block has exactly one owner and that owner just made its last use. The count of every block it frees is known to be 1 at compile time, so no count is stored and no `dup` is ever emitted. Everything it can't prove stays on the collector, which is unchanged and still the backstop, so a missed opportunity costs memory, never safety.

## Decision

### What gets freed

Only list buffers and string blocks, the blocks that dominate peak memory, and only shallowly: freeing a list buffer leaves its elements to the collector, unless the list provably owns them (below).

| | Rule |
|---|---|
| **Fresh** | List and range literals, string literals (static, so freeing one does nothing) and interpolations, `map`, `filter`, `sort`, `sort_by`, `reverse`, `unique`, `enumerate`, `zip`, `flat_map`, `counts`, `join`, `lower`, `upper`, `replace`, `repeat`, the list `split`, `words` and `chars` return (not the strings in it), `DevBuf.to_list`, calls of functions whose result is fresh, and `if`, `match` and blocks whose every result is fresh |
| **Reads** | List `len`, indexing, `get`, `first`, `last`, `sum`, `contains`, `map`, `filter`, `fold`, `any`, `all`, `find`, `min`, `max`, `sort`, `sort_by`, `unique`, `enumerate`, `zip`, `counts`, `reverse`, `join`; string `len`, `byte`, comparisons, `contains`, `starts_with`, `ends_with`, `lower`, `upper`, `replace`, `repeat`, `to_int`; interpolation and `log`; a list or string as the argument of `zip`, `concat`, `join`, `split`, `contains`, `Map.get` and `Map.has`; passing it to a parameter the callee only reads; iterating it with `for` |
| **Escapes** | Anything else: aliasing (`ys = xs`), storing it in a record, list, tuple, map or closure, slices and views (`take`, `drop`, `trim`, `split`), matching on it, returning it, raising it, sending it on a channel, passing it to a parameter that keeps it, any use inside a lambda or local function |
| **Owned local** | `x = <fresh>` or `var x = <fresh>` whose later uses are all reads. It is freed right after the statement with its last use, or not at all if the block returns it (then the block's value is fresh) |
| **`var` updates** | `x := <fresh>` (which may read the old `x`) frees the old value once the new one exists; `x := x.push(..)` and `x := x.concat(..)` free the old buffer when the push had to move it. A `for` loop over `x` can't also assign `x` |
| **Temporaries** | A fresh value consumed by a read and never named (`shifted(xs, r).sum`, `log("{..}")`, `dev_f32((0..n).map(f))`, a fused pipeline's source) is freed right after the read |
| **Summaries** | A function's result is fresh if its body is; a parameter is borrowed if the body only reads it. Both come from a greatest fixpoint over the whole program, so recursion is covered |
| **Deep ownership** | A list of strings that is only ever built from fresh strings (`var lines = []` then `lines := lines.push("...")`, or `xs.map(x => "{x}")`) and only read by `len`, `is_empty`, `join`, `contains` and interpolation owns its strings: freeing it frees them too |
| **Consumed keys** | An owned string whose last use is as the key of a `Map.put` is handed to the put, which frees it if the key was already there and keeps it otherwise. This is the Perceus owned-argument convention for one call |
| **Push then join** | `var v = []` that is only pushed onto and then joined once, outside any loop, with a literal separator, is never built: the pushes append to a string builder and the join returns it |

A trap, a `raise`, an early `return` or a self tail call skips the drops of that path, which leaves the blocks to the collector. Nothing is freed inside `sys` and `bare` code, `__own` reuse functions, parallel workers or vectorized loops.

### Reuse

A freed small block goes onto a per-size-class LIFO list, and the allocator takes from it when its bump cursor runs out, so a block freed in one iteration is the one the next iteration gets, still in cache. The check sits after the bump test, so code that only allocates pays nothing. A freed large block (over 32 KB) that holds no pointers stays mapped in a cache of up to 64 MB and is handed back to the next allocation of the same page count, which saves the page faults of re-touching fresh memory (40 rounds of two 8 MB lists in `simd`). A request the cache can't serve releases it. Pointer blocks are zeroed when freed, so stale words in a reused block can't keep garbage alive through the conservative scan.

The runtime's own temporaries are freed too: sort buffers, `sort_by` key pairs, `counts` hash tables, the old buffer when a string builder grows, and the scratch list `split` used to throw away.

### The collector

Unchanged: same heap, same conservative scan, same pacing. Each collection first moves the free lists into the page free bits, so a freed block is never marked or handed out twice. `SSPUR_GC_MIN_MB` and `SSPUR_GC_FACTOR` change the pacing for anyone who wants to trade speed for memory; the defaults stay at 256 MB and 4x.

### Threads

Frees do nothing while any `par` task, thread or parallel pipeline is running (`ss_live` or `par_busy`), so no count is ever shared between threads and nothing needs to be atomic. Values that cross threads escape anyway.

### Cycles

A freed block can't be part of a cycle: list buffers are freed shallowly, and the only deep case is a list of strings, which contain no pointers. Closures that capture themselves, `var` cells, effect handlers and generators (lowered to closures), actors, channels and in-place maps never get freed early; they all escape and stay on the collector, which handles cycles as before.

### Modes

| `SSPUR_MEM` | Effect |
|---|---|
| unset | Drops at last use, as above |
| `gc` | No drops compiled and no frees at run time: the 0.8.1 behavior, for A/B runs and as a fallback |
| `check` | Every free fills the block with `0xA5` (and poisons it under AddressSanitizer) instead of reusing it, and aborts on a second free of the same block |

`SSPUR_GC_STATS=1` now also prints how many blocks were freed at last use and how many were left for the collector when the program finished.

`SSPUR_CFLAGS` adds flags to every native build and to its cache key, so `SSPUR_CFLAGS="-fsanitize=address,undefined"` with `DYLD_INSERT_LIBRARIES` set to clang's ASan runtime builds and runs native code under the sanitizers.

## Verification

- `cargo test --release` with `SSPUR_FILL_MS=5000 SSPUR_STRICT_NATIVE=1`: 274 passed, 0 failed. `tests/programs/drops.ssp` is new: 27 tests aimed at the analysis (aliases, views, slices, closures, records, shadowing, early return, catch, list patterns, nested lists, `var` updates and push chains, deep lists with an escaping element, consumed keys in persistent and in-place maps, temporaries in interpolation, the Unicode fallback of the one-pass word count), which pass in the interpreter, natively, under `SSPUR_MEM=check`, and under `SSPUR_GC_STRESS` at 4 KB and 64 KB.
- The 199-program corpus: 199/199 identical, 270/270 functions native, also under `SSPUR_MEM=check`.
- `sspur fuzz --gen 200` with seeds 2026 and 99: 0 findings each, also under `SSPUR_MEM=check`.
- AddressSanitizer and UndefinedBehaviorSanitizer (`-fsanitize=address,undefined -fno-sanitize-recover=undefined`): the suite programs, the benchmarks and the corpus give the same output as without sanitizers, with `SSPUR_MEM=check` (every freed block poisoned) and with reuse on. They found one old bug: the allocator's fast path did arithmetic on a null cursor.
- Blocks left for the collector at exit: 0 for `compute_big`, `churn`, `parallel` and `gpu`. The others keep what is really shared or what the result points into: `typical` its tree nodes and records (sums are never freed early), `strings_big` the lowered text and the sorted list the printed result points into, `app` the generated CSV (the parsed sales hold slices of it) and the map, `simd` its two F64 inputs (read inside a lambda by `dot` and `saxpy`, so not borrowed). The collector releases them with the rest of the heap when native code returns.

## Result

Apple M1 Pro, 2026-10-13, best of 5 alternating runs, wall time of the whole process, peak RSS (`wait4`). 0.8.1 is a build of the previous release; `SSPUR_MEM=gc` is this build with the frees turned off.

| Workload | C++ clang -O2 | 0.8.1 | 0.8.2 | 0.8.2, `SSPUR_MEM=gc` |
|---|---|---|---|---|
| `compute_big` | 1.02s, 1 MB | 1.03s, 7 MB | 1.03s, 7 MB | 1.04s, 7 MB |
| `typical` | 0.80s, 70 MB (tuned 0.26s, 212 MB) | 0.11s, 16 MB | 0.11s, 16 MB | 0.12s, 16 MB |
| `strings_big` | 0.09s, 23 MB | 0.06s, 92 MB | 0.06s, 29 MB | 0.06s, 60 MB |
| `app` | 0.15s, 24 MB | 0.13s, 137 MB | 0.13s, 41 MB | 0.13s, 137 MB |
| `churn` | | 0.08s, 268 MB | 0.03s, 7 MB | 0.08s, 268 MB |
| `parallel` | 2.12s, 17 MB | 0.35s, 23 MB | 0.35s, 23 MB | 0.34s, 23 MB |
| `simd` | 0.33s, 40 MB | 0.20s, 273 MB | 0.18s, 39 MB | 0.21s, 273 MB |
| `gpu` | 0.82s, 54 MB | 0.31s, 344 MB | 0.30s, 178 MB | 0.31s, 345 MB |

No benchmark got slower; `simd` and `churn` got faster because freed memory is reused instead of faulted in fresh. Peak memory is at C++'s level on `simd` and below it on `typical`, close on `strings_big` (29 against 23 MB, of which about 6 MB is SSPUR's process), and still 1.7x on `app` and 3.3x on `gpu`:
- `app` holds the whole 10 MB CSV while 400,000 parsed sales (19 MB of records) hold slices of it; C++ copies the fields.
- `gpu` keeps about 52 MB of device buffers and Metal's 14 MB, and builds each 4M-element input as a 32 MB host list before uploading it; the C++ version runs on the CPU.
- `compute_big` is at parity with C++ (1.01x), as 0.8.1 is when measured the same day; this change doesn't touch scalar code, and in-process its four functions take 1.01s against C++'s 1.05s.

Two fixes outside the ownership analysis also show in `SSPUR_MEM=gc`: `text.lower.words.counts` (method calls without parentheses) now takes ADR 0008's one-pass path instead of building the 2M-word list, and a string builder's growth moved to a cold function, which made interpolation-heavy code about 7% faster.

On the two GC-bound programs used in option 1 (persistent maps and trees with old versions kept, no frees possible), 0.8.2 is 1% to 2% slower than 0.8.1; the hot code is identical, and the conservative scan finds slightly different stale words.

## Not done

- Runtime reference counts (option 2). The workloads that still peak above C++ do so because of values that really are shared, or because of what the program keeps alive, not because of garbage the analysis missed.
- Freeing through records, tuples, sum values and maps. A record holding a list hands the list to the collector.
- Reusing a dropped cell directly for the next constructor of the same shape (Perceus reuse tokens). ADR 0008's `__own` functions already do this for trees; for lists and strings the size-class free list gives the same effect one allocation later.
- Streaming `dev_f32((0..n).map(f))` straight into the device buffer, which would remove `gpu`'s remaining transient host lists.
