# ADR 0034: Running an unchanged program from the cache

Status: accepted, 2026-10-13

## Problem

Every `sspur run file.ssp` parsed and typechecked the program, elaborated it, ran the proven rewrites (ADR 0022), generated the whole C program to find its cached objects, and loaded the cached library, before `main` started. Measured inside the process on Apple M1 Pro, that was 3.9 ms for hello world, 5.9 ms for `compute_big`, 8.9 ms for `typical`, 9.4 ms for `app` and 11.8 ms for `gpu`, paid on every run of a program that hadn't changed. On `compute_big` it was most of the difference to C++ (1.03s against 1.02s).

## Options

1. **Make each phase faster.** The phases are already cached where they can be (objects, links, the check cache of stores); what's left is real work on the whole program, and halving it would still leave several milliseconds.
2. **Cache the checked program and reload it.** Saves the parse and the check, but code generation needs the whole typed program, so most of the cost (deserializing it, generating C to look the objects up) stays.
3. **Remember what the last run ran, and run that** (chosen). When nothing the build depends on has changed, the result of the whole pipeline is the same library, so the next run can call its `main` directly.

## Decision

After the normal path has built or found the library and before it calls `main`, it writes an entry under `~/.cache/sspur/run/` (`SSPUR_CACHE/run/`). The entry's name is a hash of:
- the source file's bytes;
- the `sspur` version and the path, size and modification time of the `sspur` binary;
- every `SSPUR_*` variable, and `CC`, `PATH`, `HOME`, `LOCALAPPDATA`, `ProgramFiles`, `SDKROOT`, `MACOSX_DEPLOYMENT_TARGET`, `CPATH`, `C_INCLUDE_PATH` and `LIBRARY_PATH` (the compiler, its search paths and the cache root).

The entry holds the library's path and the hash of its contents, how `main` is entered (the scalar entry, or the encoded one for a `main` that can raise), and the path of the inlining profile (`sspur run --profile`) with the hash of its contents, or a mark that it doesn't exist.

`sspur run file.ssp [args]` with no flags first computes the name, reads the entry and checks that the library still has the recorded contents, that the profile is still absent or unchanged, and that no `sspur.toml` exists in the file's directory or above it. If anything is missing or different, it does nothing and the normal path runs (and writes a new entry). Otherwise it loads the library, installs the host functions and the program's arguments exactly as the normal path does, and calls `main` with the same depth limit, with `log` printing through the same `println!`.

An entry is written only when the normal path would print nothing before `main` runs: no diagnostics (warnings are printed on every run, so a program with a warning always takes the normal path), no fallback warning, a `main` with no parameters that runs natively, and no package or store. It is never written for a run with flags, so `--interp`, `--native`, `--O3`, `--pgo` and the others always take the normal path, and `sspur run --no-cache` forces it.

### Failures

When the cached `main` traps or raises, the cached path keeps the status the library returned and hands over to the normal path, which loads the program as before (finding the same library) and describes that status with `Interp::run_main_failed`, exactly as `run_main` would have: the same `runtime error:` line, the same contract values, the same `unhandled error:` value, exit code 1. `main` itself is never run twice.

### What isn't covered

- Programs inside a package (`sspur.toml` with dependencies) and the codebase of `.sspur/`. Their inputs are the manifest, the lock, every dependency and the store, and checking all of them would cost much of what the cached path saves.
- `sspur test`. Tests and examples are expressions evaluated by the interpreter, which calls native functions; running them needs the checked program, so there is nothing to skip.

## Verification

- `crates/sspur-cli/tests/runcache.rs` runs a program that reads its arguments and its input, logs, writes to stderr, traps on a contract, raises and divides by zero, three times each way (`--no-cache`, first run, cached run) and compares stdout, stderr and the exit code; and checks that editing the source, changing `SSPUR_MEM` or `SSPUR_OPT`, deleting the libraries, corrupting the entries, a program with a warning and a new `sspur.toml` all give the normal path's result.
- The whole test suite, the corpus (199/199, also under `SSPUR_MEM=check`) and `sspur fuzz` pass, as do the benchmarks under AddressSanitizer.

## Result

Apple M1 Pro, best of 7 alternating runs, wall time and peak RSS, `sspur run --no-cache` (the normal path) against a plain `sspur run` of the unchanged program:

| Program | Normal path | Cached path |
|---|---|---|
| hello world | 12.2 ms, 5.8 MB | 9.3 ms, 2.5 MB |
| `app` | 122.8 ms, 22 MB | 114.5 ms, 17 MB |
| `churn` | 30.1 ms, 6.6 MB | 26.6 ms, 2.7 MB |
| `compute_big` | 1028.6 ms, 7.0 MB | 1021.8 ms, 2.6 MB |
| `gpu` | 278.8 ms, 97 MB | 267.6 ms, 92 MB |
| `parallel` | 316.5 ms, 23 MB | 317.9 ms, 18 MB |
| `simd` | 165.7 ms, 39 MB | 160.4 ms, 34 MB |
| `strings_big` | 58.3 ms, 28 MB | 54.6 ms, 25 MB |
| `typical` | 114.3 ms, 16 MB | 106.0 ms, 11 MB |

Peak memory also drops by 4 to 5 MB, because the checked program and the generated C are no longer in memory while `main` runs. A C++ hello world takes 8.3 ms on the same machine and `sspur --version` 8.9 ms, so what is left of SSPUR's startup is mostly starting the `sspur` binary itself; the checks and loading the library take about 1.3 ms.

`compute_big` doesn't reach C++ (1.022s against 1.008s to 1.016s across runs). With the startup gone, the rest is in the four functions, and it moves with code placement: timed inside the process, SSPUR's versions take 1.01s, and a C++ build of the same functions with only timing calls added takes 1.03s to 1.06s, against about 1.01s for the benchmark's C++ binary.

## Update, 2026-10-13: where the rest of the startup goes

Measured on the same machine with `/usr/bin/time -l` (instructions retired, best of 8) and by stopping the cached path at each step:
- Before `main`, `sspur` retires about 22 million instructions against 15 million for a minimal Rust program and 12 million for a C hello world. The binary has no static initializers and parses nothing before the cached path; the difference is the dynamic linker binding its 200 libc imports (Rust's standard library alone needs 75), about 40,000 instructions each, on every start. Binary size by itself costs little: 8 MB of data that isn't touched adds 0.2 million. Dropping the unused libiconv made no measurable difference.
- The cached path's own checks (reading the source and the entry, hashing the library, the profile and the `sspur.toml` lookup) take under 0.1 ms in wall time. Loading the library took 3.5 million instructions, about 0.5 ms.

Native libraries are now linked with `-dead_strip` on macOS, part of their cache key. Runtime functions a program never reaches are dropped with the libc functions only they import: hello world's library goes from 72 KB and 40 imports to 52 KB and 16, loading it costs 1.2 million instructions less, and `sspur run` of hello world takes 9.6 ms instead of 10.6 ms (best of 7; C hello world 7.2 ms, `sspur --version` 8.3 ms). Bigger programs reach more of the runtime and gain less (`app`'s library goes from 41 imports to 29). The exported entry points are the roots, and they are the only symbols the host looks up.

With the faster code of the ADR 0007 update, `compute_big` now runs in 0.91s against 1.02s for C++.
