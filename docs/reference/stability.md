# Stability and deprecation

SSPUR follows [Semantic Versioning](https://semver.org/). From 1.0 on, a program, a package or a script that works with one 1.x release works the same with every later 1.x release. This page says what that promise covers, what is left out of it on purpose, how a feature is retired, and how CI holds the project to it.

## Stable in 1.x

These don't change incompatibly before 2.0:

| Area | What is frozen |
|---|---|
| Language | The syntax and the semantics in the [language reference](language.md) and the [effects](effects.md) page: a program that checks keeps checking, and it computes the same values, with the same effects, traps and output |
| Standard library | Every builtin function, method, type and std namespace in the [standard library reference](stdlib.md), with its signature, effects and documented behaviour |
| Commands | `sspur run`, `test`, `check`, `fmt`, `build`, `deps` (`fetch`, `update`, `tree`, and `sspur update`, the same as `deps update`) and `publish`, with the flags the usage text lists for them, except the ones marked experimental below; their exit codes; and the JSON of `--json` (see [JSON schemas](schema.md)), which can gain fields but doesn't lose or rename any |
| Packages | The format of [sspur.toml](../config/sspur-toml.md) and [sspur.lock](../config/sspur-lock.md): every lock and manifest a 1.x release writes is read by every later 1.x release |
| Hashes | Content hashes of definitions (`sspur hash`) and the export hashes of packages that `sspur.lock` pins. A definition whose canonical text is unchanged keeps its hash, so a lock written by 1.0 still verifies with 1.9 |
| Formatting | What `sspur fmt` writes for a given program, so a formatted codebase stays formatted. The canonical text that hashes are computed from doesn't change either |
| Error codes | Every code in [Error codes](errors.md) keeps its meaning. New codes can appear, and messages and hints can get better; a script should read `code` and not the text |
| Language server | The LSP features `sspur lsp` offers (see [Editor setup](../get-started/editors.md)) stay; new ones can be added |

A new 1.x minor can add things: syntax that didn't parse before, builtins, methods, flags, warnings, JSON fields. An addition can't change what an existing program means. A new warning can appear on old code, but a program that checked without errors still does, and warnings never fail a build.

## Experimental

These may change or go in any 1.x minor release, with a line in the changelog. The usage text marks them `(experimental)`:

| Feature | Commands and flags |
|---|---|
| GPU kernels | `sspur gpu`, and `kernel` fns natively ([GPU kernels](../tutorials/gpu-kernels.md)) |
| Deploy | `sspur deploy` and all its subcommands |
| Bare-metal targets | `sspur build --target riscv64-qemu`, `aarch64-qemu` and `thumbv7em-mps2`, and `profile bare` |
| LLVM backend | `sspur build --backend llvm` |
| Sync | `sspur sync` |
| PGO | `--pgo` and `--retrain` on `run`, `test` and `build` |
| Cranelift JIT | `--native` on `run` and `test` (the default native tier, through C and clang, is stable) |
| Agent tooling | The agent reference ([agent spec](../agent/agent-spec.md) and [more](../agent/agent-spec-more.md)) and its token budget, `sspur q`, `sspur start`, `sspur spec`, `sspur mcp` and its tools, and the Claude Code plugin in `packaging/claude-code` |

Everything else in the usage text (`new`, `init`, `add`, `search`, `source`, `yank`, `api`, `edit`, `apply`, `hash`, `verify`, `fuzz`, `fill`, `suggest`, `native`, `explain-opt`, `debug`, `debug-init`, `bind`, `export-c`, `src`, `log`, `export`) is supported: it isn't removed in 1.x, and its flags follow the deprecation rule below, but what it prints for people can change in a minor release. The hashes `sspur hash` prints and the API `sspur api --json` reports are stable values, whatever the layout around them.

## Deprecation

Nothing stable is removed or changes meaning in 1.x. To retire something:

1. A minor release deprecates it. A use of a deprecated builtin is the warning `W_DEPRECATED`, which names the release it was deprecated in and gives the replacement in the hint, so `sspur check --json` carries both and the code keeps working. A deprecated flag or command keeps working and prints a warning with its replacement on stderr. The changelog lists it under "Deprecated".
2. It stays, deprecated and working, for at least one minor release, and in practice until 2.0.
3. Only 2.0 removes it, listed under "Removed" in the changelog with its replacement.

Nothing is deprecated in 1.0.

## Bug fixes

A bug fix may change behaviour in a 1.x release only when the old behaviour contradicts what the documentation says: the reference, the standard library pages or the error codes. The fix is listed under "Changed" in the changelog, with what programs see before and after. A behaviour that is merely surprising, but documented, stays until 2.0; the fix for it is a new, differently named feature, and the old one gets deprecated.

## The compatibility suite

`crates/sspur-cli/tests/compat.rs`, part of `cargo test --release` and so of CI, enforces this. `tests/compat` holds frozen programs and packages: a sample of the test programs, the documentation snippets and the reference answers of the evaluation corpus, Pulse's library, a program that doesn't check and one with warnings, an unformatted program, and a two-package workspace with its `sspur.lock`. For each, `tests/compat/golden` records:

- the hash of the canonical text, and every definition's content hash;
- for a package, its export hash and the hash of each public definition, and the lock;
- the codes and definitions of the diagnostics of `sspur check --json`;
- the output of `sspur run` and `sspur test --full`, in the interpreter and natively (with `SSPUR_STRICT_NATIVE=1`);
- what `sspur fmt` writes.

The test runs the current build on a copy of each fixture and fails if any of these differ, naming the fixture, the fact and the first line that changed, and pointing here. The files under `tests/compat` are never edited to make a test pass; new fixtures can be added.

Updating the goldens is an explicit step:

```console
SSPUR_COMPAT_BLESS=1 cargo test --release -p sspur-cli --test compat
```

It is allowed only for a new major version, or for a bug fix as described above, in the same commit as the fix and its "Changed" line in the changelog, so a reviewer sees the golden diff next to the reason for it.
