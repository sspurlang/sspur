# Environment variables

None of these are needed for normal use. They exist for CI, for pinning toolchains, and for measuring.

## Builds and caches

| Variable | Effect |
|---|---|
| `SSPUR_STRICT_NATIVE=1` | Same as `--strict-native`: a function that can't be compiled natively is an error, not a warning and a fallback |
| `SSPUR_CACHE` | Root of every cache, instead of `~/.cache/sspur`: native objects (`native/`), check results (`check`), unchanged programs for `sspur run` (`run/`), packages (`pkgs/`) and deploy builds (`deploy/`) |
| `SSPUR_LOG=json` | Print each log line as a JSON object with the time, the level and the message |
| `SSPUR_LOG_LEVEL` | `debug`, `info`, `warn` or `error`: drop log lines below that level (a line without a level prefix is `info`) |
| `SSL_CERT_FILE` | A PEM file of certificates the HTTP client trusts instead of the system's roots |
| `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY` | Proxies for the HTTP client (lowercase forms too) |
| `SSPUR_INDEX` | The package index to read instead of sspur-packages on raw.githubusercontent.com: an HTTPS base URL, a git URL, or a directory with `packages/` |
| `SSPUR_OFFLINE=1` | Never download: use the cached index and packages only |
| `SSPUR_REGISTRY_REPO` | The GitHub repository `sspur publish` and `sspur yank` open requests in (default `utkarshavardhana/sspur-packages`) |
| `SSPUR_GH`, `SSPUR_NO_GH=1` | The GitHub CLI `sspur publish` uses, or don't use it and print the request link |
| `SSPUR_REGISTRY_CHECKS` | A `registry.ssp` to run instead of the copy built into `sspur publish` |
| `SSPUR_LIBCURL` | Path of libcurl, if the HTTP client doesn't find it (macOS and Linux) |
| `SSPUR_OPENSSL` | Directory of OpenSSL 3's libraries, if `http.serve` with TLS doesn't find them |
| `SSPUR_NO_CHECK_CACHE` | Set to any value to skip the shared check cache, like `check --no-cache` |
| `SSPUR_JOBS` | Parallel C compiler jobs for per-definition objects, 1 to 4 (default 4) |
| `SSPUR_OPT=0` | Turn off the proven-rewrite optimizer |
| `SSPUR_SMT=off` | Don't use Z3 to remove runtime checks during native builds |
| `SSPUR_SMT_TIMEOUT` | Z3 time limit per query in milliseconds (default 500) |
| `SSPUR_FILL_MS` | Time hole filling may spend per hole in milliseconds, for `check`, `fill` and the MCP tools; `--fill-ms` still wins (defaults 400 for `check`, 600 in editors, 1000 for `fill`) |
| `SSPUR_LTO` | Default for `--lto`: `off`, `thin` or `full` |
| `CC` | The C compiler for native code and `export-c` (default `clang`) |
| `SSPUR_DEBUG=1` | Same as `--debug`: native code with debug info mapped to the `.ssp` lines, at `-O0` ([Debugging](../reference/debugging.md)). Part of the cache key |
| `SSPUR_CFLAGS` | Extra C compiler flags for native builds, part of the cache key. `-fsanitize=address,undefined` builds native code under the sanitizers; on macOS run with `DYLD_INSERT_LIBRARIES` set to clang's `libclang_rt.asan_osx_dynamic.dylib` |
| `SSPUR_PROFDATA` | The `llvm-profdata` to use for `--pgo` |
| `SSPUR_LLVM_CC` | The clang for `--backend llvm` |

## Running programs

| Variable | Effect |
|---|---|
| `SSPUR` | Set by `sspur run`, `sspur test` and `sspur debug` to the path of the running `sspur` for the program and the processes it starts, unless it is already set, so a program can run the same compiler with `env_var("SSPUR").or("sspur")` instead of whichever is first on `PATH`. Set it yourself to choose another one |
| `SSPUR_THREADS` | Threads for automatic data parallelism in pure pipelines (default: the number of cores). `1` makes them sequential |
| `SSPUR_GPU=0` | Run GPU kernels on the CPU even when Metal is available |
| `SSPUR_GPU_TRACE=1` | Print a line per GPU kernel launch |
| `SSPUR_GC_STATS=1` | Print memory statistics to stderr: each collection, and when a native program exits, how many blocks were freed at their last use and how many were left for the collector |
| `SSPUR_MEM` | `gc` turns off freeing at last use (no drops compiled, nothing freed early; the 0.8.1 behavior). `check` poisons freed blocks instead of reusing them and stops on a double free, for finding memory bugs (ADR 0033) |
| `SSPUR_GC_MIN_MB`, `SSPUR_GC_FACTOR` | Collect after max(`MIN` MB, `FACTOR` x live) of allocation (defaults 256 and 4). Lower values trade speed for memory |
| `SSPUR_GC_STRESS` | Collect every N bytes, for testing the collector |

## Targets and tools

| Variable | Effect |
|---|---|
| `SSPUR_BARE_CC`, `SSPUR_LLD` | The clang and `ld.lld` for `sspur build --target` (Apple's clang has no riscv64 backend) |
| `SSPUR_GPU_CC` | The clang for `sspur gpu --emit spirv` and `--emit ptx` |
| `SSPUR_DIR` | The codebase directory for `sspur mcp`, like `--dir` |

## Installing

| Variable | Effect |
|---|---|
| `SSPUR_VERSION` | The release `install.sh` installs, such as `0.13.0` (default: the latest) |
| `SSPUR_INSTALL_DIR` | Where `install.sh` puts the binary (default `~/.local/bin`) |
