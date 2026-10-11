# Environment variables

None of these are needed for normal use. They exist for CI, for pinning toolchains, and for measuring.

## Builds and caches

| Variable | Effect |
|---|---|
| `SSPUR_STRICT_NATIVE=1` | Same as `--strict-native`: a function that can't be compiled natively is an error, not a warning and a fallback |
| `SSPUR_CACHE` | Root of every cache, instead of `~/.cache/sspur`: native objects (`native/`), check results (`check`), packages (`pkgs/`) and deploy builds (`deploy/`) |
| `SSPUR_NO_CHECK_CACHE` | Set to any value to skip the shared check cache, like `check --no-cache` |
| `SSPUR_JOBS` | Parallel C compiler jobs for per-definition objects, 1 to 4 (default 4) |
| `SSPUR_OPT=0` | Turn off the proven-rewrite optimizer |
| `SSPUR_SMT=off` | Don't use Z3 to remove runtime checks during native builds |
| `SSPUR_SMT_TIMEOUT` | Z3 time limit per query in milliseconds (default 500) |
| `SSPUR_FILL_MS` | Time hole filling may spend per hole in milliseconds, for `check`, `fill` and the MCP tools; `--fill-ms` still wins (defaults 400 for `check`, 600 in editors, 1000 for `fill`) |
| `SSPUR_LTO` | Default for `--lto`: `off`, `thin` or `full` |
| `CC` | The C compiler for native code and `export-c` (default `clang`) |
| `SSPUR_PROFDATA` | The `llvm-profdata` to use for `--pgo` |
| `SSPUR_LLVM_CC` | The clang for `--backend llvm` |

## Running programs

| Variable | Effect |
|---|---|
| `SSPUR_THREADS` | Threads for automatic data parallelism in pure pipelines (default: the number of cores). `1` makes them sequential |
| `SSPUR_GPU=0` | Run GPU kernels on the CPU even when Metal is available |
| `SSPUR_GPU_TRACE=1` | Print a line per GPU kernel launch |
| `SSPUR_GC_STATS=1` | Print garbage collector statistics to stderr, per collection and when a native program exits |

## Targets and tools

| Variable | Effect |
|---|---|
| `SSPUR_BARE_CC`, `SSPUR_LLD` | The clang and `ld.lld` for `sspur build --target` (Apple's clang has no riscv64 backend) |
| `SSPUR_GPU_CC` | The clang for `sspur gpu --emit spirv` and `--emit ptx` |
| `SSPUR_DIR` | The codebase directory for `sspur mcp`, like `--dir` |

## Installing

| Variable | Effect |
|---|---|
| `SSPUR_VERSION` | The release `install.sh` installs, such as `0.3.2` (default: the latest) |
| `SSPUR_INSTALL_DIR` | Where `install.sh` puts the binary (default `~/.local/bin`) |
