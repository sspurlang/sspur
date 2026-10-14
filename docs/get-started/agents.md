# SSPUR for AI agents

SSPUR is built for a reader that pays for every token and a writer that never gets tired of contracts. This page is the workflow I recommend for coding agents, how to connect them over MCP or the Claude Code plugin, and what the token benchmark measured, including where it falls short.

## The loop: one call to read, one call to change

An agent working on an SSPUR codebase needs three steps:

1. **`sspur start NAME...`** prints the core language spec (about 0.6k tokens; `sspur spec --more` has the rest) and then the code the task is about, in one call.
2. **`sspur q ...`** only if the agent needs more: names, text matches, bodies or a context pack.
3. **`sspur edit --test change.ssp`** after writing every definition the change touches to `change.ssp` with the agent's file tool, in the same turn. It typechecks the whole codebase, runs every test, and either applies everything or nothing.

The examples below run on a small codebase. They are checked by CI like the rest of these docs, so the output is what the current build prints. First the codebase is imported into a `.sspur/` store, which is what agents work on:

```sspur
{{#include ../snippets/get-started/agents/cart.ssp}}
```

```console
{{#include ../snippets/get-started/agents/session.out:start}}
```

When the source is at most 12,000 bytes, `start` prints all of it. For a larger codebase it prints the counts per kind, then `q pack` of every argument that names a definition and `q find` of the other arguments. So the right first call is `sspur start` followed by the names that appear in the task: on the 1,117-definition benchmark codebase, `start` with six names prints about 1,600 tokens, and that is everything the change needs.

### Queries

`q pack` gives the definitions to change with everything around them: the types and signatures they use, their tests and their callers, each printed once. The last line says whether the list of callers and tests is complete, so the agent does not spend a call checking:

```console
{{#include ../snippets/get-started/agents/session.out:pack}}
```

The other queries agents use: `q find 'ship|tax_*'` for names and signatures, `q grep TEXT` for definitions whose source contains TEXT, `q body A,B` for several bodies, and `q callers NAME`. On a large codebase `find` and `grep` print the first 25 and 12 matches in full and only the names of the rest, so a broad pattern can't flood the context. The [command line](../reference/cli.md#queries) page lists them.

### Edits

`edit` takes plain SSPUR definitions. Each one replaces the definition with the same name or adds a new one. Leading `rename OLD NEW` and `remove NAME` lines rename (scope-aware, callers included) or delete. A `remove X` plus a new definition that is X's with only the name changed counts as `rename X NEW`, when that pairing is the only one, so stored callers of X follow it. `--test` runs every test in the same call:

```console
{{#include ../snippets/get-started/agents/session.out:edit}}
```

`~total` was replaced, `+bulk` was added, and all three tests pass. A rejected edit changes nothing and prints each error as `definition:line:col CODE message`, with a hint when the mistake is a habit from another language:

```console
{{#include ../snippets/get-started/agents/session.out:rejected}}
```

A spelling from another language that has exactly one meaning is not rejected: `edit` stores the SSPUR form and says what it rewrote. This covers `&& || !`, `elif`, `let`, `+=`, `len(x)`, `.length`, `.toLowerCase` and similar method names, `None`/`Some`/`True`, `s.slice(a, b)` on a `Str`, `_` in a call that takes no function, and a function that now performs an effect it does not declare:

```console
{{#include ../snippets/get-started/agents/session.out:normalized}}
```

Put every change in one edit. Agents should write the definitions to a file with their file tool and run `sspur edit --test FILE`: agent sandboxes refuse long quoted arguments and heredocs, which cost run 9 a call or two in four cells. When two agents edit the same definition, the second gets `E_CONFLICT`, merges the other change and resends.

### What to tell the agent

This is close to what the benchmark prompt says, and it is a good default for your own instructions:

```text
This is an SSPUR codebase. Start with `sspur start NAME...`, passing the names of the
definitions and types the task mentions. Use `sspur q` only for what that did not show.
Write every new or changed definition and test to change.ssp with the Write tool, then run
`sspur edit --test change.ssp` in the same turn. If it is rejected, fix every listed error
in the file and run it again. Do not edit `.sspur/` directly.
```

`sspur spec` prints the core spec alone, `sspur spec --more` the rest of the builtins (Map, Set, regex, JSON, effect handlers) and `sspur spec --full` the complete reference.

## MCP

`sspur mcp` is a Model Context Protocol server over stdio. It serves the codebase in the directory it starts in, or the nearest parent with a `.sspur/`. In Claude Code:

```console
{{#include ../snippets/get-started/agents/setup.sh:mcp}}
```

The committed `.mcp.json` looks like this, so everyone on the repository gets the server:

```json
{
  "mcpServers": {
    "sspur": {"type": "stdio", "command": "sspur", "args": ["mcp"]}
  }
}
```

The tools are `start`, `spec`, `src`, `query`, `edit` (with `test: true`), `test`, `check`, `run`, `fuzz`, `fill`, `suggest` and `apply`, and they return the same compact text as the CLI. The server's instructions tell the agent to call `start` first with the names from the task. They do not carry the spec itself, because Claude Code cuts server instructions at 2,048 characters and a silently cut spec is worse than one call. `verify` and `deploy local` are CLI only.

Claude Desktop starts servers from its own directory and does not read your shell's `PATH`, so give it absolute paths:

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

[MCP setup](../agent/mcp.md) has the details, other clients and a one-line check from a shell.

## Claude Code plugin

The plugin bundles the MCP server with a skill that teaches the workflow above. The skill loads when you work with `.ssp` files or a `.sspur/` codebase, or ask about SSPUR. It needs `sspur` on your `PATH`.

```console
{{#include ../snippets/get-started/agents/setup.sh:plugin}}
```

From a local clone, `claude plugin marketplace add /path/to/sspur` works too.

## What the benchmark measured

The [agent benchmark](../design/agent-benchmarks.md) gives the same multi-step feature tasks to fresh Claude Code agents in SSPUR, Python, TypeScript and Go, and scores them on hidden tests. In run 15, Sonnet 5.5 used **0.67x** Python's total tokens, and **0.98x** TypeScript's and **0.89x** Go's, with every language passing 246/246 hidden tests. Per cell, the medians against TypeScript and Go are 1.02x and 1.01x: that is parity, not an advantage. On six tasks in codebases of about 1,000 definitions it is an advantage: Sonnet used 0.68x Python's tokens, 0.59x TypeScript's and 0.58x Go's, and Opus and Haiku used 0.85x and 0.68x TypeScript's, cheaper on every task.

The savings come from calls not made. When the agent follows the loop above, a task takes 3 calls, and each call it saves re-reads about 26k tokens of harness context. SSPUR loses when edits keep getting rejected: syntax from other languages, escaping inside strings, or an effect that ripples to callers. The [full report](../design/agent-benchmarks.md) has every run and cell, and the caveats, which matter: most cells ran once, I wrote the language and most of the tasks, and the compact spec covers no services, packages, concurrency or C.
