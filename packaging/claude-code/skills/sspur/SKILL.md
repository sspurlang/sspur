---
name: sspur
description: Write, read and change SSPUR code (the AI-native language whose code lives in a typechecked .sspur/ store). Use when the user works with .ssp files or a .sspur/ directory, mentions SSPUR or the sspur CLI, or asks to add, fix, test, verify or deploy SSPUR functions, types, tests or services.
---

# Working with SSPUR

SSPUR code lives in a content-addressed store (`.sspur/`), not in files you edit. You read it with queries and change it with atomic, typechecked edits. The language is new to you: learn it from its spec (`sspur start` prints it), not from other languages.

Use the `sspur` MCP tools when they are available (`mcp__sspur__start`, `query`, `edit`, `test`, ...); otherwise the CLI below. They do the same thing.

## Workflow

1. **Learn the language and find the code in one call.** `sspur start NAME...` (MCP: the `start` tool with `names`), passing the definition names the task mentions. It prints the core spec (about 0.6k tokens; read it fully before writing any code; `sspur spec --more` has the rest of the builtins), then the codebase: all of it if it is small, otherwise counts per kind, `q pack` of each NAME (the definition, what it uses, its tests and its callers) and `q find` of other words. `sspur spec --full` is the long reference; open it only for what the spec points to (services, packages, concurrency, C FFI, GPU, bare metal).
   - No `.sspur/` yet but a `.ssp` file: `sspur init file.ssp` imports it. With neither, the first `edit` creates the store.
2. **Search more only if you need to.** On a large codebase, don't print everything (`src`). Use
   - `sspur q find 'ship|tax_*'` for names with their signatures,
   - `sspur q grep TEXT` for definitions whose source contains TEXT, with those lines (this is how to find callers to update),
   - `sspur q body A,B`, `q callers NAME`, `q pack A,B`.
3. **Make every change in one edit.** Write all new and changed definitions, and new tests, to `change.ssp` with the Write tool, then run `sspur edit --test change.ssp` in the same turn (a long quoted `-e` argument or a heredoc gets refused by agent sandboxes):
   ```
   fn f(x: Int) -> Int
   = x + 1

   test f_one = f(1) == 2
   ```
   Each definition replaces the one with the same name or is added. Start with `rename OLD NEW` or `remove NAME` lines for those changes (an impl is named `impl Show for P`). Spellings from other languages that have one meaning (`&&`, `len(x)`, `None`, `s.slice(a, b)`, a missing effect) are stored in SSPUR form and listed after `stored as:`.
4. **If it is rejected, nothing changed.** Every error line has a `hint:` with the exact fix. Fix them all in `change.ssp` and run it again in one call. Don't split it into one definition per call, and don't run `check` or `src` to confirm an edit that printed `ok`.
5. **Tests.** `--test` runs every test after the edit and prints only failures, each with the values it compared (`left 7, right 8`), plus `N passed, M failed`. `sspur test` runs them again. A test is a Bool expression: `test name = expr == expected`.
6. **Prove contracts** when functions have `pre`, `post` or `where` clauses: `sspur verify` (needs Z3) reports each clause as proved, counterexample or unknown. `sspur fuzz` property-tests them, and `sspur suggest NAME` proposes `pre` and `post` lines from runs of a function. A `?name` hole where you don't know an expression yet gets checked candidates from `sspur check`; `sspur fill --apply` writes the best one.
7. **Services.** For a `svc` with `store`s, `sspur deploy local file.ssp` serves it on 127.0.0.1 with emulated Lambda and DynamoDB (`sspur export > app.ssp` gives the file). `sspur deploy plan` writes the templates. Nothing here calls AWS.

## Rules

- Never edit `.sspur/` or create `.ssp` files to change a codebase; use `edit`. `sspur export` writes the codebase out as source when the user asks for a file.
- Syntax that looks familiar often isn't SSPUR: `and or not` (not `&& || !`), `x.len` (not `len(x)`), `none some(x) ok(x)` (lowercase), `x => e` lambdas, `if c then a else b`, effect rows with commas (`! fail[E], log`), a bare `Ctor` pattern to ignore fields, a literal `{` in a string is `\{`.
- A `match` nested in an arm takes every arm below it; put the inner match in a helper fn.
- Calling a fn that raises means the caller declares `fail[E]` too, unless it handles every variant with `catch`.
- See `docs/agent/mcp.md` in the SSPUR repository for MCP setup.
