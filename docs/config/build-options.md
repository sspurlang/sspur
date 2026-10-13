# Build options

`sspur run` and `sspur test` compile to native code by default: SSPUR emits C, compiles it with `clang`, and caches the result by content hash under `~/.cache/sspur/native/`. The first run of a program pays for the build; later runs load the cached library. Each definition is its own cached object, so after an edit only the changed definitions and their callers are rebuilt. A plain `sspur run file.ssp` of a program that hasn't changed since its last run (same source, same `sspur` binary, same `SSPUR_*`, `CC` and `PATH`) skips parsing, checking and code generation and runs the cached library's `main` directly (ADR 0034); anything else, any flag included, takes the normal path.

```sspur
{{#include ../snippets/config/build.ssp}}
```

```console
{{#include ../snippets/config/build.out:tiers}}
```

## Execution tiers

| Flag | Effect |
|---|---|
| (none) | Native code at `-O2` through clang, cached |
| `--O3` | Native code at `-O3`, compiled as one unit |
| `--interp` | The reference interpreter. Every tier gives the same output and traps at the same point |
| `--native` | The Cranelift JIT tier |
| `--strict-native` | Fail if any function can't be compiled natively, instead of warning and falling back |
| `--quiet` | Hide the fallback warning |

If a native build fails, SSPUR prints one warning naming the function and the C error, then falls back to the whole-program build or the interpreter, so a program always runs. `--strict-native` (or `SSPUR_STRICT_NATIVE=1`) makes that an error. CI runs with it, and so should anything that measures speed.

## Profile-guided and link-time optimization

```console
{{#include ../snippets/config/build.out:pgo}}
```

| Flag | Effect |
|---|---|
| `--pgo` | Run the program once instrumented, merge the profile with `llvm-profdata`, and build with it. The profile is cached per source |
| `--retrain` | With `--pgo`, repeat the training run |
| `--lto off\|thin\|full` | Link-time optimization across the per-definition objects. Default `off`. On Linux it needs `lld` |

Both work with `run`, `test` and `build`. `sspur run --profile FILE` runs the interpreter and records call counts that guide inlining (ADR 0022), and `sspur explain-opt FILE [--all]` prints the proven rewrites applied before code generation.

## Targets

| Command | Builds |
|---|---|
| `sspur build --target riscv64-qemu FILE -o kernel.elf` | A `profile bare` kernel for QEMU `virt`, RISC-V machine mode |
| `sspur build --target aarch64-qemu FILE -o kernel.elf` | A bare kernel for QEMU `virt` on Cortex-A53, with two cores |
| `sspur build --target thumbv7em-mps2 FILE -o fw.elf` | Cortex-M4 firmware for QEMU `mps2-an386` |
| `sspur gpu FILE --emit metal\|opencl\|spirv\|ptx [-o out]` | The `kernel fn`s as Metal or OpenCL C source, or SPIR-V or PTX |
| `sspur export-c FILE -o libfoo [--shared]` | A C static or shared library and its header |
| `sspur deploy plan FILE` | A Lambda `bootstrap.c`, CloudFormation and IAM policies |
| `sspur build --backend llvm FILE -o prog` | The direct LLVM IR prototype, for scalars, control flow and records |

The [bare-metal](../tutorials/bare-metal.md), [GPU](../tutorials/gpu-kernels.md), [C interop](../tutorials/ffi.md) and [CRUD service](../tutorials/crud-service.md) tutorials use these. Tools each target needs are listed under [Installation](../get-started/installation.md#optional-tools).

## Check and test options

| Option | Commands | Effect |
|---|---|---|
| `--json` | `check`, `edit`, `apply`, `q` | Machine-readable output, with `fix` ops on diagnostics |
| `--no-cache` | `check`, `run` | `check`: recheck every definition instead of using the shared check cache. `run`: take the normal path even when the program is unchanged |
| `--cases N`, `--seed N`, `--edge` | `fuzz` | Cases per function, the random seed (default 7), and a mix of extreme values such as the limits of `Int` and non-finite floats |
| `--differential` | `fuzz` | Compare native code with the interpreter on every generated input |
| `--write` | `fmt` | Rewrite the file in canonical form instead of printing it |
