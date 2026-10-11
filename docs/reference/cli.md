# Command line

Everything SSPUR does goes through one binary, `sspur`. This is its usage text, exactly as the current build prints it (CI checks this page against `sspur --help` and `sspur deploy`):

```console
{{#include ../snippets/reference/cli.out}}
```

## By task

| Task | Commands |
|---|---|
| Run and test a file | `sspur run FILE`, `sspur test FILE`, `sspur check FILE [--json]`, `sspur fmt FILE [--write]` |
| Find bugs and prove properties | `sspur fuzz FILE` (contracts as property tests), `sspur fuzz --differential FILE` (native against the interpreter), `sspur verify FILE` (Z3) |
| Write code and contracts from tests | `sspur fill [FILE] [?HOLE] [--apply]` (typed holes), `sspur suggest [FILE] [FN] [--apply]` (`pre` and `post` from runs) |
| Work on a codebase | `sspur init [FILE]`, `sspur start [NAME...]`, `sspur q QUERY TARGET`, `sspur edit --test -e SRC`, `sspur log`, `sspur src` |
| Agents and MCP | `sspur spec [--more|--full]`, `sspur start`, `sspur mcp [--dir PATH]` |
| Editors | `sspur lsp` (the language server, see [Editor setup](../get-started/editors.md)) |
| Packages | `sspur init --pkg NAME`, `sspur add PATH-OR-URL[@REV]`, `sspur deps fetch`, `sspur deps update [--force]`, `sspur deps tree` |
| Native code and C | `sspur native FILE`, `sspur run --O3`, `--pgo`, `--lto`, `sspur explain-opt FILE`, `sspur bind HEADER.h`, `sspur export-c FILE` |
| Bare metal and GPU | `sspur build --target riscv64-qemu\|aarch64-qemu\|thumbv7em-mps2 FILE`, `sspur gpu FILE --emit metal\|opencl\|spirv\|ptx` |
| Services | `sspur deploy plan`, `local`, `migrate`, `replay`, `swap`, `promote`, `rollback`, `backfill`, `status` |
| Many agents and replicas | `sspur sync serve`, `push`, `pull`, `status`, `resolve` |

## Execution tiers

`run` and `test` compile to native code through C and clang, cached by content hash under `~/.cache/sspur/native/`. `--interp` uses the reference interpreter and `--native` the Cranelift JIT. If a native build fails, SSPUR prints one warning and falls back to the interpreter; `--strict-native` or `SSPUR_STRICT_NATIVE=1` makes that an error, which is how CI runs.

## Queries

The query API is how an agent reads a large codebase without printing all of it. The [graph model](../design/02-graph-model.md#6-query-api) describes every query; these are the ones agents use most:

| Query | Prints |
|---|---|
| `q find 'ship\|tax_*'` | definitions whose names match (substring, `*` glob, `^` and `$` anchors), with signatures |
| `q grep TEXT` | definitions whose source contains TEXT, with up to 3 matching lines each |
| `q body A,B` | the source of several definitions |
| `q pack A,B,C` | each definition with the types and signatures it uses, its tests and its callers, and a line saying whether that list is complete |
| `q callers NAME`, `q callees NAME` | the call graph around one definition |
| `q effects NAME` | the effect row, with the call that introduced each effect |
| `q diag`, `q log` | every diagnostic, and the history of edits |

## Holes and suggested contracts

`sspur check` lists up to three candidates under each typed hole when the program has no errors, and `--json` puts them in the hole's `fix` as `fill` ops with an `evidence` string and `ok`. The search has 400 ms per hole (`--fill-ms N` changes it, `--no-fill` turns it off); files without holes are checked exactly as before.

| Command | Prints |
|---|---|
| `sspur fill [FILE] [?NAME\|FN]` | The best candidate for each hole (or the named one, or every hole in `FN`): `?total: items.map(_.qty * _.price).sum (passes 3 tests, post)`, with `2 others also fit` added when other candidates pass the same checks, so the tests don't decide between them. `--all` shows up to 3, `--json` adds the search size and time. `--apply` writes the passing ones into the file, in parentheses where precedence needs them, or applies them to the codebase as `fill` ops. The budget is 1000 ms per hole |
| `sspur suggest [FILE] [FN]` | For each function, the `pre` and `post` lines that held on every run, ready to paste: `  post r.len <= xs.len  // proved`. `--json` gives `refine` fixes, `--apply` adds them, `--cases N` sets the number of fuzzed inputs (200) |

The MCP server has both as the tools `fill` and `suggest` ([MCP](../agent/mcp.md)).

## The language server

`sspur lsp` speaks the Language Server Protocol over stdin and stdout, for VS Code and any other editor with an LSP client ([Editor setup](../get-started/editors.md), [ADR 0031](../adr/0031-language-server.md)). It publishes diagnostics as you type, a short pause after the last edit, and then the candidates for each typed hole (600 ms a hole, or `SSPUR_FILL_MS`). It answers completion, hover, code actions (the compiler's fixes and the hole fills), formatting, definition, references, document highlights, rename, the outline and semantic tokens, all within one file, and the command `sspur.fill` fills every hole that has a passing candidate. A file under a directory with `sspur.toml` is checked with that package's dependencies. It takes no options, and ignores `--stdio`, which some clients add.
