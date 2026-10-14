# ADR 0020: GPU kernels

Status: accepted, 2026-10-04

## Context

Doc 05 section 13 sketches `kernel fn saxpy(a: F32, x: &[F32], y: &mut [F32]) @grid(x.len / 256, 256)`, lowered to PTX and SPIR-V, with host-device transfers as the `dev` effect and kernel contracts (bounds, races) verified. The language had no F32 values, no index assignment, and no device code generation. The development machine is Apple Silicon: Metal is available at run time, `xcrun metal` is installed without its toolchain component, and there is no CUDA. Homebrew LLVM 23 has the `spirv64` and `nvptx64` backends.

## Syntax

```
kernel fn saxpy(a: F32, x: &[F32], y: &mut [F32]) @grid(y.len, 256)
  pre x.len == y.len
= y[gid] := a * x[gid] + y[gid]

kernel fn matmul(a: &[F32], b: &[F32], c: &mut [F32], n: Int) @grid(n * n, 64)
= do
  row = gid / n
  col = gid % n
  var acc = 0.0
  for k in 0..n
    acc := acc + a[row * n + k] * b[k * n + col]
  c[gid] := acc

fn main() ! dev, log
= do
  var ys = [1.0, 2.0]
  saxpy(2.0, &[0.5, 0.25], &mut ys)
  x = dev_f32((0..1000000).map(i => i.to_f64))
  y = dev_f32((0..1000000).map(i => 1.0))
  saxpy(0.5, &x, &mut y)
  log("{ys} {y.to_list[9]}")
```

## Decisions

| # | Decision | Reason |
|---|---|---|
| 1 | `kernel` is contextual before `fn`. `@grid(threads, group)` follows the signature; `threads` is the total thread count (an `Int` expression over parameters and `.len`), `group` an `Int` literal from 1 to 1024 | A total count needs no tail handling when lengths aren't multiples of the group, unlike the sketch's group count. The group size only affects `lid`, `group_id` and scheduling |
| 2 | Parameters are scalars (`Int I32 U32 F32 F64 Bool`) or slice borrows `&[T]` / `&mut [T]` with `T` in `Int I32 U32 F32 F64`. No result, no generics, no `post`; `pre` clauses are checked on the host before launch. The only effect a kernel may declare is `dev`, and calling a kernel performs `dev` | Device code can't log, raise or allocate. Contracts that only see parameters and lengths are cheap host checks with the usual `contract violated: pre ...` trap |
| 3 | The body is a restricted subset, lowered by `crates/sspur-check/src/kernel.rs` to a typed kernel IR shared by every backend: `let`, `var`, `:=`, `y[i] := v`, `for i in a..b`, `if`, arithmetic, comparisons, `and`/`or`/`not`, `.len`, conversions `.to_int .to_i32 .to_u32 .to_f32 .to_f64` (no float to int), `.sqrt .abs .floor .ceil .round .min .max`, and the intrinsics `gid lid group_id group_size grid_size`. Calls, `while`, lambdas, lists, records, strings and tuples are rejected with `E_KERNEL`, `E_KERNEL_TYPE`, `E_KERNEL_SIG`, `E_KERNEL_EFFECT`, `E_KERNEL_WRITE` or `E_KERNEL_GRID` | No calls means no recursion and no allocation by construction, and only ranged loops means every thread terminates. One IR keeps the interpreter, C, Metal and OpenCL in lockstep |
| 4 | Inside kernels, numbers have their declared widths. Float literals default to `F32` unless the kernel's floats are all `F64`; integer literals take the other operand's type. Mixing types needs an explicit conversion. Subnormal `F32` literals are rejected | Kernels are written in `F32`; requiring `.to_f32` on every literal would be noise |
| 5 | Race rejection (`E_KERNEL_RACE`): for every `&mut` slice that is written, all reads and writes use one per-thread index shape: `gid` plus a thread-uniform offset (`gid`, `gid + c`, `gid - c`), or a block `gid * s + j` with `for j in 0..s` (or `j` a literal below a literal `s`). Uniform means built from literals, scalar parameters, lengths, `group_size` and `grid_size`; immutable locals are substituted. Anything else, including a `var` index, is rejected | Distinct threads then touch disjoint elements and read only their own elements of written buffers, so any execution order gives the sequential result. `y[gid] := y[gid + 1]` and `y[0] := ..` are races; `out[gid] := acc` after a loop over a read-only slice is not |
| 6 | `y[i] := v` is new statement sugar for `y := y with [i] := v` everywhere (and `x[i].f := v`). Outside kernels it is an ordinary value update | The design's `y[gid] = ..` collides with `let`, and `:=` is the assignment operator |
| 7 | Host side: call a kernel to launch it. Scalars convert (`F64` to `F32` rounds to nearest; `Int` to `I32`/`U32` traps with `dev: argument k of times is out of range for I32 (value = ...)`). Slices take `&xs` for `List[F64]`/`List[Int]` and `&mut ys` for a `var`, copied in and out with element range checks and NaN canonicalized on the way back; or `DevBuf[T]` from `dev_f32 dev_f64 dev_i32 dev_u32 dev_int`, which stays in device memory and is updated in place. `b.to_list` copies back and `b.len` is free. `DevBuf` is a handle like `Atomic`: copies alias, and passing one buffer twice when either is `&mut` traps with `dev: arguments x and y of k are the same buffer` | Lists keep value semantics and need no new concepts; device buffers avoid a host round trip per launch, which otherwise dominates memory-bound kernels |
| 8 | The interpreter runs `gid = 0, 1, .., n-1` in order with Rust `f32`/`f64` arithmetic and the language's trap messages (`integer overflow`, `division by zero`, `index i out of bounds for list of length n`). `I32` and `U32` arithmetic traps when the result leaves the type's range | This is the reference semantics every backend must reproduce bit for bit, including subnormals and the first trapping thread |
| 9 | Native code compiles each kernel to three things: a sequential C function with exact checks (`TRAPV`, contraction off) that is the reference fallback, a Metal Shading Language entry compiled at run time through a small Objective-C runtime (`gpu_rt.m`, built once into the cache and linked into the program), and the host wrapper that converts arguments, checks `pre`, evaluates the grid and launches | `xcrun metal` lacks its toolchain here, while `newLibraryWithSource` is always available. A C fallback keeps every program runnable without a GPU (`SSPUR_GPU=0`, other platforms) |
| 10 | Metal code is exact `F32`: safe math mode, `fp contract(off)` and precise functions; measured bit-identical to the CPU for `+ - * /`, `sqrt`, `floor`, `ceil`, `round`, `mulhi` and integer-to-float conversion, except that Apple GPUs flush subnormals. So the device computes a per-thread `bad` flag, branch-free: any check that could trap on the CPU (index, overflow, division, range) and any `F32` subnormal hazard (a subnormal load; an add, subtract, multiply or divide whose result is below 2 x `FLT_MIN` when it shouldn't be zero). Loads use a clamped index while `bad`, loops stop, stores are skipped. A flagged launch reruns on the CPU, which reproduces the exact result or trap | Flags are supersets of the CPU's traps and of every case where FTZ could change a result, so a clean GPU run equals the sequential one. Branch-free flags cost nothing on memory-bound kernels (saxpy runs at the plain kernel's speed) |
| 11 | Reruns: list arguments are copies, so the CPU reruns everything from the originals. A kernel whose writes are all trailing top-level stores is commit-safe: each thread computes, checks and only then writes, and a flagged thread records itself in a bit mask and writes nothing. For in-place `DevBuf`s the CPU then reruns just the flagged threads, in order. Other kernels on in-place buffers back the written buffers up before launch and restore them before a full rerun. A GPU failure after a commit-safe in-place launch has started traps with `dev: the GPU failed while running k` | Threads are independent (decision 5), so rerunning the flagged subset on unchanged inputs equals sequential execution. The backup is only paid by kernels that store inside loops |
| 12 | Every kernel also gets a 32-bit variant. Before launch the host evaluates the interval of every integer node of the kernel with the actual parameter values, lengths and thread count (in 128-bit arithmetic, loops widened to their bounds, mutable integer `var`s disqualify); if all fit in `I32` and the declared type, the launch uses the variant, which computes indices in `int` without overflow checks | Apple GPUs emulate 64-bit integer multiply; matmul drops from 11.2 ms to 2.2 ms per multiply. Launch-time intervals are exact where compile-time intervals can't be (`n * n` with `n` a parameter) |
| 13 | `F64` kernels run on the CPU on Metal (no FP64 on Apple GPUs); `SSPUR_GPU_TRACE=1` reports where each launch ran | Correctness first; `F64` still compiles to OpenCL, SPIR-V and PTX |
| 14 | `sspur gpu file.ssp --emit metal|opencl|spirv|ptx [-o out]` prints or writes device code. OpenCL C comes from the same emitter (flags as a `uint`, an atomic-or bit mask, `cl_khr_fp64`); SPIR-V is `clang -target spirv64 -cl-std=CL2.0`, PTX is `clang --target=nvptx64-nvidia-cuda -march=sm_70` with builtin macros so the PTX has no unresolved calls. The compiler is `SSPUR_GPU_CC`, `clang`, or Homebrew's LLVM | The portable path the design promised, without a runtime to test it on here; the suite compile-checks every kernel |
| 15 | Device buffers are freed when the outermost native call returns, like the GC heap; `DevBuf` values never cross into the interpreter (no entry points, like `Atomic` and `Chan`). GPU runtime calls are serialized, so kernels can launch from `par` tasks | No finalizers are needed, and a buffer's lifetime is bounded by the call that created it |

## Verification

`crates/sspur-cli/tests/gpu.rs` and `tests/gpu/`:
- 20 rejected programs, each with its expected code (`tests/gpu/reject.ssp`, one `// case NAME CODE` section each): races (constant index, `gid / 2`, neighbours, a block wider than its stride, a `var` index), writes to `&[T]`, `while`, calls, effects, allocation, group size, mixed types, element types, `&mut` of a non-`var`, `&xs` with `&mut xs`, missing `dev`, missing `&`, kernels as values, `gid` in the grid.
- `basic.ssp` (saxpy, a strided partial sum, matmul, an `I32` block kernel), `exact.ssp` (30 000 `F32` results of every operation over random data with and without subnormals, plus integer kernels), `devbuf.ssp` (device buffers, aliasing, in-place partial reruns, launches from `par` tasks) `sys.ssp` (a kernel in `profile sys`) and `traps.ssp` (out-of-bounds reads, `I32` and `U32` overflow, division by zero, `pre`, element and argument range, subnormal inputs, narrowing) give identical output in the interpreter, on the GPU and with `SSPUR_GPU=0`; the GPU runs are required to have used Metal, and the subnormal datasets to have rerun on the CPU. Without Metal the tests check the CPU fallback and say so.
- Every suite kernel emits Metal and OpenCL, and compiles to SPIR-V (magic number checked) and PTX (every entry present, no `.extern`) when a capable clang exists.
- The F32 results also match C++ (`bench/native/gpu.cpp`, `-ffp-contract=off`) bit for bit. GC stress, `fuzz --differential` and the 199-program corpus (199/199 identical, 258/258 native) are unchanged.

## Result

`bench/native/gpu.ssp` against single-threaded C++ -O2 with the same checks (Apple M1 Pro, best of 3, identical output):

| Section | C++ -O2 | SSPUR (Metal) | Speedup |
|---|---|---|---|
| saxpy, 4M `F32`, 100 launches | 83 ms | 72 ms | 1.15x |
| reduction, 4M to 65 536 partial sums, 100 launches | 65 ms | 47 ms | 1.4x |
| matmul 512 x 512, 20 launches | 247 ms | 56 ms | 4.4x (kernel alone 5.8x) |
| whole program, wall | 0.41 s | 0.27 s | 1.5x |

Launches are synchronous, about 0.25 ms of submit-and-wait each, and the CPU shares the same unified memory bandwidth, so memory-bound kernels gain little; compute-bound ones gain the most. SSPUR's 62 ms setup builds the inputs as 4M-element host lists before uploading them.

## Not yet

ADR 0023 adds asynchronous launches, threadgroup memory and barriers, atomics, 2D grids and helper calls. Still open from this list: 3D grids; float-to-int conversions; a resident subnormal-free bit on `DevBuf` to drop load checks; running SPIR-V and PTX (no Vulkan or CUDA runtime here) and an ahead-of-time `metallib`; `export-c` with kernels.

Update (0.8.2): `dev_f32((a..b).map(f))` and the other `dev_*` constructors of a mapped range write each value straight into the device buffer, so the setup no longer builds host lists before the upload, and `d.to_list.sum` sums the buffer without building a list. `bench/native/gpu` now peaks at 92 MB instead of 344 MB; the current numbers are in `bench/native/README.md`.
