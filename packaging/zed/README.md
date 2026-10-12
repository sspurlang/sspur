# SSPUR for Zed

A [Zed](https://zed.dev) extension for `*.ssp` files: highlighting, brackets, indentation and an outline (`fn`, `type`, `test`, `trait`, `impl`, `effect`, `rule`, `store`, `svc`) from the Tree-sitter grammar in `tree-sitter-sspur/`, and diagnostics, completion, hover, formatting, rename and the rest of `sspur lsp`.

You need `sspur` 0.7 or later on your `PATH`, or its path in Zed's settings:

```json
{ "lsp": { "sspur": { "binary": { "path": "/path/to/sspur" } } } }
```

## Install from a release

Every release attaches the extension as `sspur-zed-vX.Y.Z.zip`, with the grammar pinned to the release's commit. Unzip it, then in Zed run **zed: install dev extension** and pick the unzipped `sspur-zed` folder. Zed builds the extension (it installs the `wasm32-wasip2` Rust target itself when Rust comes from rustup) and fetches the grammar from GitHub. Nothing is published to Zed's extension registry.

## Install from a checkout

`extension.toml` in this folder names the grammar's commit as `RELEASE_COMMIT`, which `package.sh` replaces. To try local changes, commit them, then point the grammar at your checkout:

```sh
packaging/zed/package.sh /tmp/sspur-zed "file://$PWD"
```

and install `/tmp/sspur-zed` with **zed: install dev extension**. The third argument picks another commit (the default is `HEAD`).

## The grammar

SSPUR is indentation-sensitive. The grammar doesn't track layout: it reads a file as definition headers, tokens and bracketed groups, which is enough for highlighting and outlines and stays tolerant of unfinished code. Strings use a small external scanner (`src/scanner.c`) so that `{` starts an interpolation only where the compiler's lexer would (`"{}"` and `"([{"` are literal text). `src/parser.c` is generated and committed, since Zed compiles grammars from source. After editing `grammar.js`:

```sh
cd packaging/zed/tree-sitter-sspur
npx --yes tree-sitter-cli@0.25.10 generate --abi 14
npx --yes tree-sitter-cli@0.25.10 test
```
