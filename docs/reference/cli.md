# Command line

Everything SSPUR does goes through one binary, `sspur`. This is its usage text, exactly as the current build prints it (CI checks this page against `sspur --help` and `sspur deploy`). Commands and flags marked `(experimental)` may change in a 1.x minor release; [Stability and deprecation](stability.md) lists what 1.x keeps:

```console
{{#include ../snippets/reference/cli.out}}
```

## By task

| Task | Commands |
|---|---|
| Start a project | `sspur new web-app NAME` (the reference web app, see [Build a web app](../tutorials/web-app.md)), `sspur new --list` |
| Run and test a file | `sspur run FILE`, `sspur test FILE`, `sspur check FILE [--json]`, `sspur fmt FILE [--write]` |
| Find bugs and prove properties | `sspur fuzz FILE` (contracts as property tests), `sspur fuzz --differential FILE` (native against the interpreter), `sspur verify FILE` (Z3) |
| Write code and contracts from tests | `sspur fill [FILE] [?HOLE] [--apply]` (typed holes), `sspur suggest [FILE] [FN] [--apply]` (`pre` and `post` from runs) |
| Work on a codebase | `sspur init [FILE]`, `sspur start [NAME...]`, `sspur q QUERY TARGET`, `sspur edit --test -e SRC`, `sspur log`, `sspur src` |
| Agents and MCP | `sspur spec [--more|--full]`, `sspur start`, `sspur mcp [--dir PATH]` |
| Editors | `sspur lsp` (the language server, see [Editor setup](../get-started/editors.md)) |
| Packages | `sspur init --pkg NAME`, `sspur add NAME[@REQ] [--as KEY]`, `sspur add PATH-OR-URL[@REV]`, `sspur deps fetch`, `sspur update [NAME...] [--force]` (also `sspur deps update`), `sspur deps tree` |
| The registry | `sspur search [QUERY]`, `sspur source NAME[@VERSION] [DIR]`, `sspur publish [--dry-run]`, `sspur yank NAME@VERSION`, `sspur api [--json]`, `sspur archive` (see [Publishing a package](../handbook/publishing.md)) |
| Native code and C | `sspur native FILE`, `sspur run --O3`, `--pgo`, `--lto`, `sspur explain-opt FILE`, `sspur bind HEADER.h`, `sspur export-c FILE` |
| Debug with lldb or gdb | `sspur debug FILE [ARGS]`, `sspur build --debug FILE`, `sspur run --debug FILE`, `sspur debug-init [--install]` (see [Debugging](debugging.md)) |
| Bare metal and GPU | `sspur build --target riscv64-qemu\|aarch64-qemu\|thumbv7em-mps2 FILE`, `sspur gpu FILE --emit metal\|opencl\|spirv\|ptx` |
| Services | `sspur deploy plan`, `local`, `migrate`, `replay`, `swap`, `promote`, `rollback`, `backfill`, `status` |
| Many agents and replicas | `sspur sync serve`, `push`, `pull`, `status`, `resolve` |
| Move code from 0.14 to 1.0 | `sspur fix [FILE\|DIR...] [--check]` (see [Migrating to 1.0](../get-started/migrating-to-1.0.md)) |

## Flags the usage text leaves out

These are for working on SSPUR itself or for scripts, so the usage text doesn't list them:

| Flag | Does |
|---|---|
| `sspur native --emit-c FILE` | Prints the generated C instead of building it |
| `sspur native --release FILE` | Builds each definition at `-O2` and prints which functions are native and which were skipped, and why |
| `sspur export-c --keep-c FILE` | Keeps the generated `.c` file next to the library |
| `sspur q list --tests` | Lists the tests only |
| `sspur fuzz --gen N --from K` | Starts the generated programs at index `K` (to rerun a range) |
| `sspur fuzz --reduce FILE` | Shrinks a program while it still typechecks and still shows the behavior: a mismatch between `--modes`, `--want-skip TEXT` (a native skip reason), `--want-build TEXT` (a build error) or `--want-hang "ARGS FILE"` (a child `sspur ARGS` that crashes or hangs) |
| `sspur sync serve --max N` | Stops after `N` requests |
| `sspur sync push\|pull REMOTE --agent NAME` | Records `NAME` as the agent of the merge (default `sync`) |
| `sspur publish --tests-json` | Runs the package's tests in both tiers and prints them as JSON, for the registry's test job |

## Starting a project

`sspur new web-app NAME` writes a new directory `NAME` (which must not exist or be empty; its last component is the package name, so `notes` or `my_notes`) with SSPUR's reference application, a notes service with accounts: `main.ssp` with the routes and handlers, module packages for the data types, SQLite storage, accounts and HTML views, three reusable packages, tests that `sspur run tests/run.ssp` runs in both tiers, a backup tool, a systemd unit and a Caddyfile, and a README that covers running, testing, debugging, configuring, backups and deploying. The template is built into `sspur`, so nothing is downloaded. `sspur new --list` lists the templates.

## Migrating from 0.14

`sspur fix` takes files or directories (all the `.ssp` files under them; with none, the package the current directory is in) and rewrites code written for 0.14 so that 1.0 reads it as 0.14 did: string literals whose braces or escapes changed meaning, arms that 0.14 gave to a nested `match` or `catch`, and binding annotations that 0.14 ignored and that don't hold. It edits only those spans, so comments and layout stay, and running it again changes nothing. It also lists, as `check:` lines, the places whose meaning changed and that it can't rewrite: `==` through a hand-written `Eq` inside an `Opt`, `List`, `Res`, tuple or derived type, `round floor ceil trunc` on an `F64`, and comparisons with `0.0`. `--check` writes nothing and exits with 1 if it would rewrite something. [Migrating to 1.0](../get-started/migrating-to-1.0.md) has each change.

## Execution tiers

`run` and `test` compile to native code through C and clang, cached by content hash under `~/.cache/sspur/native/`. A plain `sspur run FILE` of a program that hasn't changed since its last run skips parsing and checking and calls the cached library's `main` directly (ADR 0034); `--no-cache` takes the normal path. `--interp` uses the reference interpreter and `--native` the Cranelift JIT. If a native build fails, SSPUR prints one warning and falls back to the interpreter; `--strict-native` or `SSPUR_STRICT_NATIVE=1` makes that an error, which is how CI runs.

## Debugging

`--debug` on `run`, `test`, `build` and `native`, or `SSPUR_DEBUG=1`, builds native code with debug info that maps to the `.ssp` lines: DWARF (CodeView on Windows) at `-O0`, functions, parameters and locals under their SSPUR names where C allows it, cached separately from normal builds, which don't change. `sspur build --debug FILE` builds it and prints the library and the source file its lines refer to (for a codebase, a copy of its source under `~/.cache/sspur/debug/src/`). `sspur debug FILE [ARGS]` builds it and starts lldb (macOS) or gdb (Linux, or `--debugger gdb`) with the formatters loaded and breakpoints on `fn main` and on `sspur_trap`, which every trap calls. `sspur debug-init` writes the lldb and gdb formatters to `~/.cache/sspur/debug/` and prints the lines that load them; `--install` adds those lines to `~/.lldbinit` and `~/.gdbinit`. See [Debugging](debugging.md) and [ADR 0036](../adr/0036-source-level-debugging.md).

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

## The registry

| Command | Does |
|---|---|
| `sspur add NAME[@REQ]` | Adds a published package to `[deps]` as `{ version = "REQ" }` (default `^` the newest version) and locks the newest version that meets it. `NAME@1.2.3`, a whole version, writes `=1.2.3`: exactly that version. A scoped name, `sspur add acme/money`, is added under the key after the scope, `money = { package = "acme/money", version = "^1.0.0" }`; `--as KEY` chooses the key, and is needed when the key is taken. A file or directory called NAME is added by path instead |
| `sspur update [NAME...] [--force]` | `sspur deps update`: moves the named packages (or all) to the newest versions their requirements allow, prints the export diff, and writes the lock only if your code still checks |
| `sspur search [QUERY] [--json]` | Packages whose name, keywords or description match, with their newest version |
| `sspur source NAME[@VERSION] [DIR]` | Downloads a version's source into DIR (default `NAME-VERSION`, with `/` as `-` for a scoped name), verified against its snapshot SHA-256, without building or running it. `@1.2.3` is exactly that version, `@^1.2` the newest that matches |
| `sspur publish` | Runs the registry checks on the package at `HEAD` and every module in it (their tests natively and in the interpreter too, side by side, in a copy of the commit), prints the report, and when no blocking check fails, opens the publish request: with the GitHub CLI if it is signed in, else by printing the prefilled form URL. `--dry-run` only checks, `--ack net,fs` acknowledges new capability effects, `--ref TAG` names the tag (default the tag at `HEAD`), `--repo URL` the GitHub repository (default the `origin` remote), `--no-tests` skips the tests, `--no-gh` prints the URL, `--json` prints the report as JSON |
| `sspur yank NAME@VERSION` | Opens a request to yank the version (`acme/money@1.0.0` for a scoped name); `--undo` unyanks, `--deprecate MESSAGE` deprecates (with `--undo`, undeprecates), `--reason TEXT` explains |
| `sspur api [--json] [--rev REV]` | The package's public API with lines and doc comments; `--json` gives the facts the checks read (diagnostics, formatting, definitions, effects, export hash, and in a git repository the files of `REV`) |
| `sspur archive [--rev REV] [-o FILE]` | Builds the source snapshot of a commit (default `HEAD`) as the registry stores it, a deterministic tar, and prints its SHA-256 and size |

The index is read from `SSPUR_INDEX` (default sspur-packages on raw.githubusercontent.com) and cached under `~/.cache/sspur/index`; see [Environment variables](../config/environment.md).

## The language server

`sspur lsp` speaks the Language Server Protocol over stdin and stdout, for VS Code and any other editor with an LSP client ([Editor setup](../get-started/editors.md), [ADR 0031](../adr/0031-language-server.md)). It publishes diagnostics as you type, a short pause after the last edit, and then the candidates for each typed hole (600 ms a hole, or `SSPUR_FILL_MS`). It answers completion, hover, signature help (after `(` and `,`, with the argument being typed highlighted, on half-typed calls too), inlay hints (the inferred type after a binding whose type isn't written, and the parameter name before a literal argument), code actions (the compiler's fixes and the hole fills), formatting, definition, references, document highlights, rename, the outline and semantic tokens, all within one file, and the command `sspur.fill` fills every hole that has a passing candidate. A file under a directory with `sspur.toml` is checked with that package's dependencies. It takes no options, and ignores `--stdio`, which some clients add. Inlay hints can be narrowed with `initializationOptions` or the `sspur` settings section, `{"inlayHints": {"types": true, "lambdas": true, "params": false}}`, where `types` covers `let`, guard, `for` and `var` bindings, `lambdas` lambda parameters and `params` the names before literal arguments; all three are on by default.
