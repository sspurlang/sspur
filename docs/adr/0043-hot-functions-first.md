# ADR 0043: The hot functions first in the sspur binary

Status: accepted, 2026-10-18

## Problem

Adding `sspur fix` (900 lines that no other command runs) made the common commands use more memory. Peak RSS on an M1 Pro (`/usr/bin/time -l`, median of 25 interleaved runs of two copies of each binary): the cached `sspur run` of a hello world went from 2448 to 2528 KB, `sspur fmt` of the Pulse library from 5408 to 5472 KB and `sspur --version` from 1792 to 1856 KB. Marking the new functions `#[cold]` made `run` worse (2568 KB).

The release binary is one LTO unit (`lto = "fat"`, `codegen-units = 1`), and ld64 lays out its functions in the order LLVM wrote them, so new code anywhere moves the old code onto other pages. The commands used little code spread over much of the 8 MB of `__text`: the cached `run` called 141 functions, 98 KB of code, on 41 different 16 KB pages; `--version` called 50 functions, 34 KB, on 20 pages.

## Options

| Option | Why not, or why |
|---|---|
| `#[cold]` or `#[inline(never)]` on new code | Moves the layout again, by an amount no one controls; made `run` worse |
| An ld64 order file of mangled names | The names carry crate hashes, which change with the crate version, the toolchain and the profile: between a build with symbols and one without, 1376 of 1514 names no longer matched, and ld64 skips those silently |
| PGO (`-C profile-use`) | Changes the generated code, and its profile is keyed by the same mangled names |
| The list from an instrumented build (`-C profile-generate`, `-C instrument-coverage`) | Needs an `llvm-profdata` of rustc's LLVM version, sees the functions before inlining, and misses the standard library, which comes precompiled |
| The list from lldb, with a breakpoint on every function of the release binary | Sees the functions of the binary as linked, the standard library included, in the order they are first called; no root, no extra tools beyond Xcode's |

## Decision

| # | Decision | Why |
|---|---|---|
| 1 | `tools/order/hot.txt` lists the functions that `sspur --version`, the cached `sspur run`, `sspur fmt`, `sspur run --interp`, `sspur check`, `sspur lsp` (initialize, open, hover, shutdown) and `sspur test` call, in that order, each function once, at the first command that calls it, in the order of its first call. 1405 names, 118 KB | Smallest footprint first, so each command's code is one run of pages that starts where the smaller commands' code is |
| 2 | The names are demangled (`c++filt -_`) without crate hashes; LLVM's numbered suffixes (`.4270`) are dropped and the machine outliner's `OUTLINED_FUNCTION_N` count as one name | Version bumps, toolchain updates and new code don't change these names, so the list keeps matching until the hot code itself changes. A name that matches nothing is ignored, a name that matches several functions orders them all |
| 3 | `tools/order/gen.sh` regenerates the list: it builds the release binary with symbols (the same code), fills the run and test caches, and runs each command under lldb with `tools/order/trace.py`, which stops once at each function and records it. About a minute after the build; macOS only | Run it when the hot paths change; nothing else needs it |
| 4 | `tools/order/link-clang.sh` is the linker for `aarch64-apple-darwin` and `x86_64-apple-darwin` in `.cargo/config.toml`. For the release `sspur` binary it lists the symbols of the objects being linked (`nm`), demangles them, matches them against the list, and passes the mangled names to ld64 with `-order_file`. Every other link, and any failure on the way, links as before. `build.rs` relinks when the list or the script changes | The release workflow and local builds both read `.cargo/config.toml`, and the mangled names are only known at link time. The name ends in `-clang` so rustc passes it cc arguments |
| 5 | Linux and Windows link as before | lld's `--symbol-ordering-file` and link.exe's `/ORDER` could take the same list after the same matching, but neither was measured, and aarch64 Linux links with GNU ld, which has no such option |

## Result

Peak RSS in KB, median of 25 interleaved runs of two copies of each binary; time in ms, median of 100 interleaved runs. 0.15 is `sspur fix`'s parent commit, `fix` adds it, `ordered` adds this ADR.

| Command | RSS 0.15 | RSS fix | RSS ordered | Time 0.15 | Time fix | Time ordered |
|---|---|---|---|---|---|---|
| `run h.ssp` (cached) | 2448 | 2528 | 1968 | 10.94 | 10.85 | 10.78 |
| `run --interp h.ssp` | 5152 | 5072 | 4016 | 12.71 | 12.68 | 12.70 |
| `check lib.ssp` | 10120 | 10048 | 9176 | 31.65 | 31.87 | 31.58 |
| `fmt lib.ssp` | 5408 | 5472 | 5024 | 16.52 | 17.14 | 16.65 |
| `--version` | 1792 | 1856 | 1568 | 10.17 | 10.07 | 10.22 |
| `test lib.ssp` | 18928 | 18944 | 17152 | | | |
| `lsp` session | 11064 | 11136 | 9936 | | | |

The times are within the noise of this machine: a second series of 100 runs had 0.15 ahead of both other binaries by 0.3 to 0.9 ms, and `fix` behind `ordered`. The processes' own CPU time (user plus system, median of 120 runs) is lower with the list for every command: 4.56, 4.51 and 4.48 ms for `run`, 6.19, 6.16 and 6.10 for `run --interp`, 24.46, 24.18 and 24.30 for `check`, 9.60, 9.76 and 9.51 for `fmt`, 3.64, 3.65 and 3.63 for `--version`. Native programs run as fast as before (`bench/native`, cached `run`, median of 80: compute 82.4, 82.8 and 82.2 ms, typical 143.4, 144.2 and 140.5 ms). The binary is 49 KB larger (11,270,016 to 11,319,680 bytes): ld64 keeps each function at its original address modulo its alignment (up to 64 bytes, inferred from where LLVM put it), so a moved function brings up to 60 bytes of padding.

The text no longer moves with new code: with the same list, the parent commit and `fix` touch the same pages of `__TEXT` in `fmt` (36 and 37, counted with `mprotect` under lldb). `fmt`, `check` and `lsp` still differ by 120 to 180 KB between the two, in private memory (the peak footprint and the minor faults differ the same way), not in the binary's pages; that difference was there without the list as well (`check`'s footprint: 4016 KB at the parent, 4128 KB with `fix`).
