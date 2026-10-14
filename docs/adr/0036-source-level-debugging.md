# ADR 0036: Source-level debugging with lldb and gdb

Status: accepted, 2026-10-14

## Problem

Native code is C that clang compiles (ADR 0005, ADR 0022). Before this change a debugger attached to `sspur run` saw that C: no line of the `.ssp` file, functions called `f_total`, locals called `v_acc_$6` next to dozens of temporaries, a `Str` as a pair of a length and a pointer, a sum value as a pointer to a tagged union with members `v0` and `v1`. A trap stopped inside `longjmp` with nothing on the stack that pointed at the source. The goal for 0.10 is lldb and gdb at the source level: breakpoints by `file.ssp:line` and by function name, a backtrace of SSPUR functions and `.ssp` lines, locals under their SSPUR names with readable values, and a place to stop when a program traps.

## Options

1. **Emit DWARF ourselves.** The code generator knows every span, so it could write the line table and the variables directly. It would have to describe C's frame layout, which only clang knows, for every target and for CodeView too. Rejected: a second debug-info writer for a backend we don't own.
2. **Debug through the LLVM IR prototype** (`--backend llvm`, ADR 0024). It covers scalars and records only, so most programs couldn't be debugged.
3. **`#line` directives and C names chosen for the debugger** (chosen). clang already turns `#line N "file"` into DWARF and CodeView line tables. The generated C keeps its shape; a debug build adds directives at the statements that matter, names functions and variables after the source where C allows it, and compiles at `-O0`.

## Decision

### Building

`sspur build --debug`, `run --debug`, `test --debug` and `native --debug`, or `SSPUR_DEBUG=1` for any of them, build the program's native library with debug info. `build --debug file.ssp` prints where the library is and which source file its lines refer to. The program still runs inside `sspur` (the runtime's host functions, such as `log`, are Rust), so a debugger runs `sspur run --debug file.ssp`; breakpoints are pending until the library loads and then resolve. `sspur debug file.ssp [args]` does that: it builds, writes the formatters (below) and starts lldb on macOS or gdb elsewhere, with the formatters loaded, a breakpoint on `fn main`'s line and one on `sspur_trap`, and runs. Without a debugger it says how to install one.

| # | Decision | Reason |
|---|---|---|
| 1 | clang gets `-g -O0 -fno-omit-frame-pointer -gno-column-info`, and `-gcodeview` when the target is Windows | `-O0` keeps every local; `-Og` was measured (below) and loses locals and moves breakpoints. Columns would point into C, not into the `.ssp` line. Frame pointers keep backtraces right through the runtime |
| 2 | The flags are appended to `SSPUR_CFLAGS` in every cache key, and the C itself differs, so debug and release objects never mix | One cache, no stale objects |
| 3 | The proven rewrites of ADR 0022 are off, the per-definition build is always used and small callees are not copied into their callers | Rewrites would move code away from its source line; at `-O0` a copied callee is never inlined, so a copy only adds breakpoint locations |
| 4 | Release builds take none of these paths | The generated C of every program in `bench/native`, `tests/programs` and `examples` is byte-identical with and without this change, and the native benchmarks are unchanged (below) |

### Lines

A mark `#line N "file.ssp"` goes before every statement of a block, before each match arm (so stepping through a match visits the arm being tested, and back to the match's line before the no-match trap), into each branch of a multi-line `if`, into loop bodies that are a single expression, at function entry (the depth check, so `break total` stops on the `fn` line), and before each `pre` and `post` check. A mark ends at the end of the function. Because the C of one statement spans several C lines, the debug build repeats the mark before every C line it covers. The function's bookkeeping (storing the result, the jump to the epilogue, the return) is line 0, which debuggers step over, so `next` on the last line returns to the caller.

Each translation unit starts with `#line 2 "<cache>/native/obj/KEY.c"` and ends every mapped region with a directive back to it, and that copy of the unit's C is kept next to its object, so stepping into the runtime shows the runtime's C.

### One breakpoint location per line

lldb sets a breakpoint on a line at the first address of that line in every lexical block that has code for it. The generated C nests statement expressions, and each one that declares a temporary is its own block, so a breakpoint on `acc := acc + x` first had eight locations, and `continue` stopped four times per iteration. In a debug build every declaration of a generated temporary (a name with `$` or a trailing `_`) is `__attribute__((nodebug))`: blocks that hold only temporaries get no scope of their own, and the temporaries leave the list of locals. The closing braces of a loop body map to the loop's line. With both, every line resolves to one location per function.

### Names

| C in a release build | Debug build | Rule |
|---|---|---|
| `f_total` | `total` | Unless the name is a C keyword, a name the C headers declare or define (found once per compiler by preprocessing the program's `#include` lines, cached under `native/debug/`), or any identifier of the generated program. `main`, `round` or `free` keep the `f_` prefix: `break f_main` |
| `a0`, `a1` | the parameter names | Same check, within the function |
| `v_acc_$6` | `acc` | A second `acc` in the same function is `acc__2` |
| `lam$12` | `total__lambda$k0` | Lambdas carry the name of the function they are in |
| `int64_t` for a `Bool` | `ss_bool` | A typedef of `int64_t`, so the formatters can show `true` and `false`; `Int` bindings are declared `int64_t` |
| a sum's tag | `enum SST_S_Shape { SST_S_Shape__Circle = 0, ... }` | A `used` variable of the enum keeps it in the debug info, where the formatters find variant names |

### Formatters

`sspur debug-init` writes `sspur_lldb.py` and `sspur_gdb.py` to `~/.cache/sspur/debug/` and prints the line that loads each from `~/.lldbinit` or `~/.gdbinit` (`--install` adds them). They show `Str` as text, `Bool` as `true` or `false`, a `List` with its elements, `Opt` as `some(3)` or `none`, and a sum value as `Circle{r: 2}` with its fields as children (null-pointer variants of two-variant sums included). `sspur-locals` prints only the frame's SSPUR parameters and locals.

### Traps

Every trap and contract failure already calls `sspur_trap(st, code, fn, clause, value)` before unwinding, so `breakpoint set -n sspur_trap` (gdb: `break sspur_trap`) stops with the failing SSPUR frame one `up` away, at the line of the failing `pre`, `post` or expression.

### Codebases

When the program is a codebase in `.sspur/` rather than a file, its projected source is written to `~/.cache/sspur/debug/src/<hash>/<dir>.ssp` and the lines refer to that file.

## Measurements (Apple M1 Pro, macOS, lldb 2100)

`-O0` against `-Og`, same program, breakpoints on two lines: at `-O0` both resolve to one location each and every local is shown; at `-Og` a breakpoint on `let name = ...` moved two lines down, a list local and the result of a call showed `<variable not available>`, the breakpoint inside the loop had three locations, and `next` jumped back to the loop header in the middle of the body.

Debug builds of `bench/native` programs, cold cache: `app` 3.0s (including the one-time scan of the C headers), `typical` 1.0s, `compute_big` 0.9s. Debug code runs about 10 times slower than release (`compute_big` 8.9s against 0.92s); the debug build is for stepping, not timing.

`sspur run --debug` of the whole corpus (`tools/corpus_diff.py` with `SSPUR_DEBUG=1`): 199/199 programs give the interpreter's output, 270/270 functions native, no build failures.

Release code: the generated C of 33 programs is identical before and after (compared by hash), and the native benchmarks, best of 5 against the previous release's binary, are within run-to-run noise:

| Benchmark | 0.9.0 | This change |
|---|---|---|
| `compute_big` | 0.971s | 0.965s |
| `typical` | 0.191s | 0.189s |
| `strings_big` | 0.117s | 0.120s |
| `app` | 0.160s | 0.158s |
| `churn` | 0.080s | 0.080s |
| `parallel` | 0.428s | 0.377s |
| `simd` | 0.216s | 0.225s |
| `gpu` | 0.353s | 0.359s |

Both binaries build the same C, so they load the same cached libraries; the differences are noise (the machine had background load).

## Platforms

| | Status |
|---|---|
| macOS, lldb | `crates/sspur-cli/tests/debug.rs` runs lldb in batch mode (breakpoint by `prog.ssp:line`, backtrace with SSPUR names and lines, locals with values, a `Str` in the caller), here and on the macOS CI runner unless it can't start processes. Checked by hand: breakpoints by name, `next`, `finish`, `up`, `sspur_trap`, the formatters, `sspur debug` |
| Linux, gdb | The same test runs gdb in batch mode on the Linux CI runner, which now installs gdb. gdb wasn't available on the development machine (Apple Silicon), so the pretty-printers were also run against a small model of gdb's Python API |
| Windows | Objects carry CodeView (`-gcodeview`); `crates/sspur-native` checks that a Windows debug build cross-compiled with zig has a `.debug$S` section that names the `.ssp` file. Linking passes `-g`, so lld-link writes a PDB next to the library. Not exercised under Visual Studio or WinDbg in CI |

## Not done

- Variables captured by a lambda are fields `c0`, `c1` of its environment `env_`, not names.
- A function whose SSPUR name is taken in C keeps its `f_` prefix, and locals that collide get a `__2` suffix rather than the source name.
- Functions that come from a package's dependencies are not mapped to lines (their spans are in other sources); their frames show C.
- No Debug Adapter of our own: VS Code debugs through CodeLLDB (see Debugging).
- Maps, sets and the other standard containers have no formatter yet; they show as C structs.
