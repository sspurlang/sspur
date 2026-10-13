# ADR 0006: Garbage collection for native code

Status: accepted, 2026-10-03

## Problem

The native tier allocated from a bump arena that was freed only when control returned to the interpreter. A long-running native loop, such as a server or a simulation, grew memory without bound.

## Decision

Native code uses a conservative mark-sweep collector built into the generated C runtime (`PRELUDE` in `crates/sspur-native/src/cgen.rs`).

| Part | Choice |
|---|---|
| Heap | One reserved 16 GB virtual region (`MAP_NORESERVE`), split into 64 KB pages. Small objects use 35 size classes up to 32 KB; larger objects get dedicated page runs |
| Allocation | Bump-pointer cursor per size class (as cheap as the old arena). Free lists come from lazily swept pages |
| Roots | Registers (via `setjmp`), the native stack from the outermost entry frame, and the `Status` block (which holds in-flight raised errors) |
| Precision | Conservative and interior-pointer aware. Pointers exactly at an object boundary also count for the preceding object, but only for roots (optimized loops keep end pointers in registers) |
| No-scan objects | String bytes, plus list buffers whose element type has no pointers (decided at codegen) |
| Trigger | After max(64 MB, 2x live bytes) allocated (raised to max(256 MB, 4x live) by ADR 0008), checked when a page's cursor is exhausted and before every large allocation |
| Page reuse | A free-page bitmap. Large objects take the first free run that fits, and freed runs are unmapped so RSS drops |
| Lifetime | The whole heap is reset when native code returns to the interpreter |

## Why not reference counting now

Precise RC with reuse (Perceus) remains the long-term goal: deterministic memory, in-place updates at refcount 1, no stack scanning. It needs explicit dup/drop insertion in a core IR that the current direct-to-C generator doesn't have. SSPUR's immutable data can't form cycles, so RC will need no cycle collector when it lands. The conservative collector gives bounded memory today without touching the code generator.

ADR 0033 (0.8.2) took the static half of that plan: blocks the compiler proves have one owner are freed at their last use, and this collector stays behind them for everything else.

## Bugs found while building it

1. A page left behind by an advancing cursor didn't record its high-water mark, so its live objects looked unallocated and the page was released. Stress mode caught this as a corrupted tree (a "stack overflow" in `insert`).
2. Applying the boundary rule to heap contents retained the previously allocated neighbor of every object, which kept every garbage tree version alive (65 MB live instead of 3 MB). Root-only scanning fixed it.
3. Large allocations (list buffers over 32 KB) skipped the threshold check, so a loop rebuilding a big list grew the heap until a small allocation happened to trigger a collection. Freed multi-page runs were also never reused. Fixing both cut peak memory on `typical` from 472 MB to 264 MB and on `strings_big` from 246 MB to 164 MB.

## Verification

`SSPUR_GC_STRESS` collects every N bytes. With it, the suite, the benchmarks, and all 199 corpus programs produce identical results; differential fuzzing and all Rust tests pass. `bench/native/churn.ssp` scaled 10x (about 7 GB of garbage) peaks at 73 MB.
