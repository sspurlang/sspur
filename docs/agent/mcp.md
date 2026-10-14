# Using SSPUR from an MCP client

`sspur mcp` is a Model Context Protocol server over stdio (JSON-RPC, one message per line, protocol `2025-06-18`). It serves the SSPUR codebase (`.sspur/`) in the directory it starts in, or the nearest parent that has one. Use `--dir PATH` or the `SSPUR_DIR` environment variable when the client starts servers somewhere else (Claude Desktop does). If there is no codebase yet, the first `edit` creates one.

Install `sspur` first (`brew install utkarshavardhana/sspur/sspur`, `./install.sh`, or `cargo install --path crates/sspur-cli`) and check that `sspur --version` works in the shell your client starts.

## Claude Code

From the project directory:

```
claude mcp add sspur -- sspur mcp
```

That adds it for you in this project. `--scope project` writes a `.mcp.json` that you can commit, so everyone working on the repository gets the server; it looks like this:

```json
{
  "mcpServers": {
    "sspur": {"type": "stdio", "command": "sspur", "args": ["mcp"]}
  }
}
```

The tools show up as `mcp__sspur__edit`, `mcp__sspur__query` and so on. The Claude Code plugin in [`packaging/claude-code/`](https://github.com/utkarshavardhana/sspur/tree/main/packaging/claude-code) bundles this server with a skill that teaches the workflow; see its README.

## Claude Desktop

Claude Desktop starts servers from its own directory, so give the codebase path. In `claude_desktop_config.json` (macOS: `~/Library/Application Support/Claude/`, Windows: `%APPDATA%\Claude\`):

```json
{
  "mcpServers": {
    "sspur": {
      "command": "/opt/homebrew/bin/sspur",
      "args": ["mcp", "--dir", "/Users/you/code/myproject"]
    }
  }
}
```

Use the absolute path of `sspur` (`which sspur`), because Desktop does not read your shell's `PATH`. Restart Claude Desktop after editing the file.

## Other clients

Any client that speaks MCP over stdio works: run `sspur mcp [--dir PATH]` as the command. The server answers `initialize`, `ping`, `tools/list`, `tools/call`, and empty `resources/list`, `resources/templates/list` and `prompts/list`; it ignores notifications. A quick check from a shell:

```
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"start","arguments":{}}}' | sspur mcp
```

## Tools

Every tool returns compact text, the same as the CLI (`json: true` on `query` and `apply` gives the machine format). `isError` is true when an edit is rejected, a test fails, or the call is wrong.

| Tool | CLI | What it does |
|---|---|---|
| `start` | `sspur start [NAME...]` | Call it first, with `names` from the task (`"a,b,c"`): the compact spec, then the codebase (all of it if small; otherwise counts, `pack` of the named definitions and `find` of other words) |
| `spec` | `sspur spec [--more|--full]` | The core language spec alone (about 0.6k tokens); `more` the rest of the builtins |
| `src` | `sspur src` | The whole codebase as source; for small codebases |
| `query` | `sspur q QUERY TARGET` | `list`, `find 'a\|b*'`, `grep TEXT`, `body A,B`, `sig`, `callers`, `callees`, `effects`, `impact`, `pack A,B`, `why`, `holes`, `diag`, `log` |
| `edit` | `sspur edit --test -e SRC` | Add or replace definitions by name (plus `rename A B`, `remove A` lines), atomically; `test: true` runs every test after it |
| `test` | `sspur test` | Run the tests: failures and a count |
| `check` | `sspur check` | Typecheck and list diagnostics, with up to 3 checked candidates under each typed hole |
| `run` | `sspur run` | Run `main` and return what it logged |
| `fuzz` | `sspur fuzz` | Property-test functions from their contracts |
| `fill` | `sspur fill [?hole] [--apply]` | The best expression for each typed hole, run against the tests and contracts; `apply: true` writes the passing ones, `all: true` lists up to 3 |
| `suggest` | `sspur suggest [fn] [--apply]` | `pre` and `post` clauses that held on every run of a function, marked `proved` when Z3 proves them; `apply: true` adds them |
| `apply` | `sspur apply` | Low-level JSON transaction ops; prefer `edit` |

The older tool names (`sspur_edit`, `sspur_query`, `sspur_export`, ...) still work.

The workflow the server's instructions describe: `start` first (the spec and the code the task names, in one call), more `query` only if needed, then every change in one `edit` with `test: true`. The instructions themselves are short (825 characters, about 210 tokens) and do not carry the spec: Claude Code cuts server instructions at 2,048 characters (checked with Claude Code 2.1.288; instructions with the spec appended, 5.8k characters, were cut mid-sentence), and a spec that is silently cut is worse than one call to `start`. A rejected edit changes nothing and lists each error with a hint (`hint: no '&&' operator: write 'and'`), ending with `fix these and resend the whole edit in one call`. `verify` (Z3 proofs) and `deploy local` are CLI only.
