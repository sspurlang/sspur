# Agent token benchmark: SSPUR ops vs Python, TypeScript and Go

This measures the Phase 3 exit criterion: an agent completes multi-step feature tasks using SSPUR (CLI ops only) with fewer total tokens than the same agent using Python. Run 9 adds TypeScript and Go as controls, because beating Python alone does not show that the language is the reason.

**Latest result (run 12, 2026-10-12): after 0.6 and 0.7 (hole filling, `e is p`, guard clauses, long lines, the language server), SSPUR is still cheaper than Python and close to TypeScript and Go, but not better than run 11. On the 17 cells, Sonnet 5.5 used 0.79x Python's tokens, 1.16x TypeScript's and 1.05x Go's (medians 0.67x, 1.03x, 1.01x), with 246/246 hidden tests passing in every language. Every task was solved; the difference from run 11 is a few cells that took 2 to 4 more calls.** Details in [Results, run 12](#results-run-12).

**Run 11 (2026-10-09): SSPUR is cheaper than Python and at parity with TypeScript and Go. On the 17 cells, Sonnet 5.5 used 0.65x Python's tokens, 0.95x TypeScript's and 0.86x Go's (medians 0.59x, 1.01x, 1.00x), in 58 API calls against 88, 61 and 66, with 246/246 hidden tests passing.** The changes from run 10: failing tests print the values they compared, a 556-token core spec (was 777) and a shorter SSPUR prompt. The totals against TypeScript and Go are below 1.0x but the medians are not, so this is parity, not a win. The large codebase took one call more than in run 10 and is level with TypeScript (0.99x), still ahead of Python and Go. 20 pilot cells from four restarted attempts are excluded and listed. Details in [Results, run 11](#results-run-11).

**Run 10 (2026-10-09): 0.72x Python, 1.06x TypeScript, 0.96x Go (medians 0.60x, 1.02x, 1.01x).** Edits through a `change.ssp` written with the Write tool, canonical storage of unambiguous foreign spellings and a 777-token core spec. Details in [Results, run 10](#results-run-10).

**Run 9 (2026-10-08): the token advantage over Python did not transfer to TypeScript or Go: 0.83x Python, 1.23x TypeScript, 1.11x Go.**

**Run 8 (2026-10-06): SSPUR now wins the large codebase too, at 0.82x of Python's tokens over two runs (was 1.04x), after `sspur start NAME...`, one command that prints the spec and `q pack` of the named definitions.** Sonnet solves it in 4 calls against Python's 5 and still passes 10/10. The spec is 1,631 tokens (was 1,798). a1, a4 and a7 were rerun with `sspur start` in place of `spec && src`: still 3 calls each, all hidden tests pass, totals within 1% (0.76x, 0.60x, 0.61x). The 8-task numbers are otherwise unchanged (Sonnet 0.70x, Opus 0.76x, Haiku 0.83x). Details in [Results, run 8](#results-run-8).

**Run 7 (2026-10-06): the large codebase is at 1.04x over two runs (was 1.33x), after `q pack A,B,C` and a cap on `q find` and `q grep` text output.** Sonnet solved it in 5 calls, the same as Python. Details in [Results, run 7](#results-run-7).

**Run 6 (2026-10-06): after the error-hint and query changes, Haiku 4.5 is at 0.83x on the 8 tasks (was 1.21x).** Only 4 SSPUR cells were rerun, against the run 5 Python cells. Details in [Results, run 6](#results-run-6).

**Run 5 (2026-10-06): the claim holds for Sonnet 5.5 and Opus 5.5, not for Haiku 4.5, and not on the one large codebase tried.** Details in [Results, run 5](#results-run-5).
- Opus 5.5, the 8 tasks: SSPUR used **0.76x** Python's total tokens (median 0.76x), 28 API calls against 38, 95/95 hidden tests on both sides.
- Haiku 4.5, the 8 tasks: **2.31x** (median 1.22x), 150 calls against 93. SSPUR passed 95/95 and Python 84/95. After a spec fix and 3 SSPUR reruns: 1.21x (median 0.97x).
- Sonnet 5.5, 8 new tasks taken from neutral sources (LeetCode 146, 224 and 227, RFC 7396, the AWK book, ...): **0.74x** (median 0.60x), 141/141 on both sides; 0.66x after the spec fix (one cell rerun).
- Sonnet 5.5, one codebase of 1,117 definitions with four targeted changes: **1.34x** (6 calls against 5), 10/10 on both sides; 1.04x in run 7.
- The optional regex and JSON task (a9) costs more in SSPUR for every model tried: Opus 1.69x (1.37x after the fix), Haiku 43x (5.2x after the fix, still failing 1 of 17 hidden tests).

**Run 4 (2026-10-04, Sonnet 5.5): still met, after a spec fix.** With the agent spec as it stood after Phases 4 to 7 (1,799 tokens), SSPUR regressed to **1.24x** Python's total tokens (median 0.80x), although both languages still passed all 95 hidden tests. The spec had lost rules the tasks need. After restoring them (spec now 1,782 tokens) and rerunning the 4 affected SSPUR cells against the same Python cells, SSPUR used **0.70x** (median **0.61x**, range 0.42x to 1.35x), 26 API calls against 38, and both sides were again 95/95. SSPUR was cheaper on 6 of 8 tasks.

| Run | SSPUR interface | Hidden tests SSPUR / Py | Calls SSPUR / Py | Total SSPUR / Py |
|---|---|---|---|---|
| 1 | 3.7k-token reference, `q` JSON queries, `apply tx.json` | 95/95 / 95/95 | 64 / 40 | **1.96x** (median 1.68x) |
| 2 | 1.5k-token spec, `src`, `edit --test` via heredoc | 95/95 / 95/95 | 42 / 38 | **1.19x** (median 1.07x) |
| 3 | same, `edit --test -e '...'` | 95/95 / 95/95 | 27 / 46 | **0.59x** (median 0.60x) |
| 4 | 1.8k-token spec after Phases 4 to 7 | 95/95 / 95/95 | 42 / 38 | **1.24x** (median 0.80x) |
| 4, spec fix | 1.78k-token spec, 4 SSPUR cells rerun | 95/95 / 95/95 | 26 / 38 | **0.70x** (median 0.61x) |
| 5, Opus 5.5 | spec at `cfc9ace` (1,798 tokens) | 95/95 / 95/95 | 28 / 38 | **0.76x** (median 0.76x) |
| 5, Haiku 4.5 | same | 95/95 / 84/95 | 150 / 93 | **2.31x** (median 1.22x) |
| 5, Haiku 4.5, spec fix | spec at `4b75361` (1,798 tokens), 3 SSPUR cells rerun | 95/95 / 84/95 | 98 / 93 | **1.21x** (median 0.97x) |
| 6, Haiku 4.5 | fix hints, `q find|grep|body`, 3 SSPUR cells rerun | 95/95 / 84/95 | 71 / 93 | **0.83x** (median 0.88x) |
| 7, Sonnet 5.5, s1_shop only | `q pack A,B,C`, capped `find` and `grep` text, 2 runs | 10/10 / 10/10 | 5 / 5 | **1.04x** (1.33x before) |
| 8, Sonnet 5.5, s1_shop only | `sspur start NAME...`, spec 1,631 tokens, 2 runs | 10/10 / 10/10 | 4 / 5 | **0.82x** (1.04x before) |
| 9, Sonnet 5.5, 16 tasks + s1_shop | all SSPUR cells rerun at `aa61bf4`; TypeScript and Go added | 246/246 in each of the 4 languages | 70 / 88 | **0.83x** vs Python, **1.23x** vs TypeScript, **1.11x** vs Go |
| 10, Sonnet 5.5, 16 tasks + s1_shop | `change.ssp` edits, tolerant input, 777-token core spec, all 17 SSPUR cells rerun at `b7ef6fa` | 246/246 in each of the 4 languages | 63 / 88 | **0.72x** vs Python, **1.06x** vs TypeScript, **0.96x** vs Go |
| 11, Sonnet 5.5, 16 tasks + s1_shop | test failures print values, 556-token core spec, shorter prompt, all 17 SSPUR cells rerun at `ef439aa` | 246/246 in each of the 4 languages | 58 / 88 | **0.65x** vs Python, **0.95x** vs TypeScript (median 1.01x), **0.86x** vs Go (median 1.00x) |
| 12, Sonnet 5.5, 16 tasks + s1_shop | compiler at 0.7.1 (`11263bc`): hole filling, `e is p`, guard clauses, long lines; 698-token core spec (run 11: 556); same prompts as run 11 | 246/246 in each of the 4 languages | 68 / 88 | **0.79x** vs Python, **1.16x** vs TypeScript, **1.05x** vs Go (medians 0.67x, 1.03x, 1.01x) |

Python's side was identical in all runs; its totals were 1.27M, 1.20M, 1.48M and 1.22M. Against Python's cheapest run, SSPUR run 3 is 0.73x and run 4 after the fix is 0.71x.

An optional 9th task (`a9_events`: regex, JSON encode and decode, `Res` handling, an effect available for one step) is new in run 4 and kept out of these totals: both sides passed 17/17 hidden tests, SSPUR used 1.96x Python's tokens on the spec as fixed for the 8 tasks and 0.75x after one more spec fix.

## Tasks

`tasks.json` has 8 tasks. Each starts from an existing small codebase (30 to 60 lines) that exists in both languages with the same names, behavior, and visible tests (`tasks/<id>/start.ssp`, `start.py`). Each task has 4 to 7 steps that mix bug fixes, new functions, new error variants, signature changes that ripple through callers, a field added to a record used everywhere, a rename across callers, and new tests.

| Task | What changes |
|---|---|
| a1_inventory | fix an arithmetic bug, sort an output, add two fallible operations and an error variant |
| a2_wordstats | fix tie ordering, thread a new parameter through 5 functions and their tests, add a function |
| a3_bank | add a record field used by every constructor, a new error checked in two places, a fee rule, a new operation |
| a4_calc | add two expression variants and a new error through eval, show, simplify, run; add a traversal |
| a5_orders | rename a function across all callers, add coupons, change three dependent pricing rules and the invoice format |
| a6_todo | make an infallible function fallible and update its callers, change rendering, add three functions |
| a7_grades | fix two bugs and a tie rule, add three functions |
| a8_config | harden a parser four ways, add two typed getters with new error variants |

| a9_events (new in run 4, optional) | replace a split parser with a regex, add JSON export and import with a new error, email redaction, and the lines `ingest` skips (it logs them through the `log` effect) |

Run 5 added 8 tasks from neutral sources, b1 to b8 (`"set": "b"` in `tasks.json`, set up with `setup.py --b`, 141 hidden tests per language), and one large generated codebase, `tasks/s1_shop/` (its own `gen.py`, `setup_scale.py` and `score_scale.py`). Both are described under [Results, run 5](#results-run-5).

Run 9 added TypeScript and Go versions of all 16 small tasks (`tasks/<id>/ts/{start.ts, start.test.ts, ref.ts, hidden.test.ts, iface.txt}` and `tasks/<id>/go/{start.go, start_test.go, ref.go, hidden_test.go, iface.txt}`) and of the large codebase (`tasks/s1_shop/gen_xlang.py`, `hidden.test.ts`, `hidden_test.go`). Each port keeps the task semantics and checks the same behaviors, in that language's idioms: exported functions and `throw` with an error-class hierarchy in TypeScript, `(T, error)` with pointer error structs and a marker interface in Go. The task text is the same in all four languages; the per-language interface line and one prompt line say that names are camelCase (and, in Go, that "raise E" means return an error of type `*E`). `python3 score.py ref` checks all four languages.

Hidden tests (`tasks/<id>/hidden.ssp`, `hidden.py`, `ts/hidden.test.ts`, `go/hidden_test.go`, 95 per language for a1 to a8, 141 for b1 to b8, 17 more for a9) are equivalent across languages. a9 is marked `"new": true` in `tasks.json`; `setup.py` skips it unless `--new` is given, so the 8-task totals stay comparable. a9 has no TypeScript or Go port. `ref.*` are the author's solutions. `python3 score.py ref` checks that every reference passes its hidden tests and every starting codebase fails them.

## Method

- `setup.py <dir>` creates one fresh work directory per (language, task) outside the repo, so agents never see hidden tests or references. `--v1` gives the run 1 SSPUR instructions.
  - SSPUR (run 8 on): as below, but the first step is `./sspur start` (the spec and the whole codebase in one command; `setup.py --r7` gives the old `spec && src` line). The s1 prompt says to start with `./sspur start NAME...`, passing the names the task mentions (`setup_scale.py --r7` gives run 7's prompt).
  - SSPUR (runs 2 and 3): the starting program is imported into a `.sspur/` store and the `.ssp` file is deleted. The agent is told to start with `./sspur spec && ./sspur src` (the compact spec, `docs/agent/agent-spec.md`, and the whole codebase as source) and to change code only with `./sspur edit`; `q`, `test`, and `check` are also allowed. Direct access to `.sspur/`, `.ssp` files, and `init` are forbidden.
  - SSPUR (run 1): the starting program is imported into a `.sspur/` store with `sspur init`, and the `.ssp` file is deleted. The agent must read `./sspur spec` (the reference, `docs/reference/language.md`), then read code only with `./sspur q` and change it only with `./sspur apply tx.json`. Direct access to `.sspur/`, `.ssp` files, and `init` are forbidden. No agent broke these rules (checked in the transcripts). The MCP server was not used: the harness can't attach a new MCP server to a subagent, and the CLI exposes the same query and transaction surface.
  - Python: the agent edits `app.py` with any tool and runs pytest. Python 3.9.
  - TypeScript (run 9 on, `setup.py --langs=sspur,ts,go`): the agent edits `app.ts` and `app.test.ts` with any tool and runs `npx tsc && node --test --test-reporter=dot dist/`. Node 20.20, TypeScript 5.9 in `bench/agent/xlang/node_modules` (symlinked into each work directory), `strict: true`.
  - Go (run 9 on): the agent edits `app.go` and `app_test.go` with any tool and runs `go test`. Go 1.27, one `package main` per work directory, `GOPROXY=off`.
- All get the same task text and the same per-language interface line (exact names and signatures the hidden tests call). `setup.py` regenerates every prompt (`prompts.json` in the work directory). Each run directory keeps only its summary: `tokens.json`, the scores files, `excluded.txt` and `agents.json` where present; prompt dumps, transcripts and final codebases are not kept.
- Each (language, task) pair was solved by one fresh Claude Code `general-purpose` subagent on `claude-sonnet-5-5`, with no retries and no help. Runs 1 to 3 launched all 16 in parallel; run 4 ran them one at a time (a memory limit on the host).
- Compiler binary frozen before each run: `60d188d` (run 1), `34fddc3` (run 2), `9efe082` (run 3), `a8a9de4` (run 4). Run 4's spec fixes were rebuilt from `86d3611` (8 tasks) and `69e0d69` (a9); only `docs/agent/agent-spec.md`, which the binary embeds, changed.
- Run 4 differences in the setup, all outside the language: `setup.py --tmo` runs `./sspur` (a wrapper script instead of a symlink) and pytest under a 300 s timeout, which adds a short clause to each prompt. The subagents inherited a worktree sandbox that refused the Edit and Write tools outside the repository checkout, so the work directories were untracked directories inside it instead of `/tmp`; every transcript was checked, and no agent touched a path outside its own directory. Subagents now finish with a hand-back tool call inside their last API call, which adds no calls. A no-op subagent now reads 26,090 tokens; `net` keeps the old 25,750 so it stays comparable.
- `score.py run <dir>` exports each store to `final.ssp`, appends the hidden tests, and runs them. A task passes only if every hidden test passes. For TypeScript and Go (`xlang/xlang.py`) only the agent's `app.ts` or `app.go` is copied next to the hidden tests, so the agent's own tests never count; a typecheck or compile failure with the hidden tests scores 0, as a SSPUR `check` error does. Each Go hidden test runs in its own process, so a panic fails only that test.
- `tokens.py` reads each subagent's transcript (JSONL):
  - **input** is exact: the API usage of every call (`input + cache_creation + cache_read`).
  - **output** is estimated: transcripts keep the usage snapshot from the start of the stream (output 16 to 75), so output is re-counted from the recorded content with tiktoken `cl100k_base` (about 15% off Claude's tokenizer). Output is under 1% of the total.
  - **total** = input + output. This is the headline metric.
  - **net** = total minus 25,750 per call, the context of a no-op subagent (system prompt plus tools). It removes the fixed harness overhead.
  - **tool I/O** = cl100k tokens of everything the agent read from tools plus everything it wrote. It doesn't depend on the harness.

## What changed for runs 2 and 3

All changes are in the CLI and the spec. The language is unchanged.

- `sspur edit [file|-] [-e SRC] [--test]` takes plain SSPUR definitions. Each one replaces the definition with the same name or is added; leading `rename OLD NEW` and `remove NAME` lines rename (scope-aware, callers included) or delete. It's one atomic, typechecked transaction. `--test` runs every test in the same call. So the shortest SSPUR path is 3 calls (read spec and code, edit and test, DONE), against Python's read, edit, test, DONE.
- Output is one line where it can be: `ok ~total_value +add_item +add_item_ok`, then only failing tests and `10 passed, 0 failed`. A rejected edit prints `rejected, nothing changed` and `def:line:col CODE msg` lines (line relative to the definition), plus the hint if there is one. No hashes, no JSON, and changes that are only hash ripples from a dependency aren't listed. `q` prints source or signatures as text, `test` lists only failures, `init` and `check` print `ok N definitions`. `--json` keeps the machine format on `q`, `apply`, `init`, and `check`; `test --full` lists passes.
- `sspur apply` also takes ops inline (`-e '<json>'`) or on stdin, accepts a bare op array, and takes `--test`.
- `sspur spec` prints `docs/agent/agent-spec.md` (1,493 cl100k tokens, down from 3,676); `spec --full` prints the long reference `docs/reference/language.md`. The compact spec drops prose, safety types, decision tables, and the transaction and query tables, and keeps every rule the tasks needed.
- The MCP server gained `sspur_edit`, and its tools return the same compact text (`json: true` for the old format).

Run 2 used a heredoc (`./sspur edit --test <<'EOF'`). The sandbox refused that command in 6 of 8 runs: it treats `{a, b}` inside the heredoc (any record literal with two fields) as possible brace expansion and won't verify the command. Each refusal cost a retry plus a Write call to put the input in a file. Run 3's spec recommends one single-quoted `-e` argument instead, which passed in 7 of 8 runs (a8 was refused once, apparently because of a `#` inside a string).

## Results, run 12

Run 12 measures what 0.6 and 0.7 did for agents. Nothing in the harness changed: the same prompts as run 11, one fresh Sonnet 5.5 `general-purpose` subagent per cell, strictly one at a time, no retries, the compiler frozen at 0.7.1 (`11263bc`, `target/sspur-frozen-11263bc`). Python, TypeScript and Go are run 9's cells, as in runs 10 and 11. Run directory: `runs/2026-10-12-r12/`. No cell was excluded or repeated.

| | Run 11 | Run 12 |
|---|---|---|
| Hidden tests | 246/246 | 246/246 |
| Calls | 58 | 68 |
| Total vs Python | 0.65x | 0.79x |
| Total vs TypeScript | 0.95x | 1.16x |
| Total vs Go | 0.86x | 1.05x |
| Median vs TypeScript | 1.01x | 1.03x |

What moved the total:

- **Every call costs a little more.** The core spec grew from 556 to 698 cl100k tokens across 0.5 to 0.7 (patterns, `is`, guard clauses, holes), and every call re-reads it, so the 3-call cells cost 1,000 to 1,500 more tokens each than in run 11 (about 1.4%). Trimming the spec back is the cheapest lever.
- **A few cells took more calls, and a few took fewer.** a1 went from 3 to 5 calls, b4 from 4 to 7, b5 from 3 to 7, and b6, b8 and s1 one more each. a2, b1 and b2 went from 4 to 3. With one sample per cell, a swing of one or two calls is within run-to-run noise, but the three worst cells had causes worth fixing:
  - a1: the agent's own new test expected the wrong value. `FAIL total_value_sum: left 704, right 864` showed it at once, and it fixed the test.
  - b4: the agent wrote the arm of `catch do` at the same indent as the block's statements, so `| _ => false` was read as a statement (`E_PARSE_EXPR expected an expression, found '|'`). This was the same before 0.7.
  - b5: `"error: cycle {path.join(\" -> \")}"` printed the braces literally. An escaped quote inside an interpolation is not code, and SSPUR said nothing about it.
  - In b4 and b5 the agent also tried a `python3` one-liner to edit `change.ssp`, which failed with a locale error in that shell, one wasted call each. That is the machine, not the language.
- **Hole filling, `is` and guards were not exercised much.** The tasks don't use typed holes, and the agents used `is` and guard clauses in only a few places. Their benefit shows in code size and in fewer `match` blocks, not in these call counts.

The two language gaps from b4 and b5 are next to fix: an arm of `catch do` written at the block's indent, and escaped quotes inside an interpolation.

## Results, run 11

Run 10 left SSPUR at 1.06x TypeScript's and 0.96x Go's tokens, with medians of 1.02x and 1.01x. On 3-call cells the gap to TypeScript was the core spec and the longer prompt, re-read by every call. Run 11 shrinks both and adds one runtime change, and reruns all 17 SSPUR cells against the same Python, TypeScript and Go cells.

### What changed

- `591eff4` (before run 11, the base for everything below): a failing test prints the values of the comparison that failed (`FAIL t: left 7, right 8`), and a failed `pre` prints the parameters it mentions, in every tier. In run 10, a wrong expected value in an agent's own test was the most common extra call; the agent now sees the actual value.
- The SSPUR prompt says the same rules in fewer words (`8fb30a8`; `setup.py --r10` keeps run 10's).
- The core spec is 556 tokens (run 10: 777). `start` on a small codebase prints 854 to 1,148 tokens (run 10: 1,075 to 1,517), and 1,561 on s1_shop (run 10: 1,611).

### Setup

- Run: `runs/2026-10-09-r11/`, compiler frozen at `ef439aa` (`target/sspur-frozen-ef439aa`), `--tmo` prompts (`setup.py` defaults), one fresh Sonnet 5.5 `general-purpose` subagent per cell, strictly one at a time, no retries. All 17 SSPUR cells are new; nothing in the spec, the prompt or the binary changed between the first counted cell and the last.
- Python, TypeScript and Go cells are run 9's, unchanged. `xlang/report5.py runs/2026-10-09-r11` prints the table (its column header still says run 10).
- Exclusions: b6_adventure was aborted by the API three times with "Output blocked by content filtering policy" right after `sspur start`, before any edit (the store had only its import commit each time); the fourth attempt in the same directory is the one counted. Run 5 and run 9 hit the same failure on the same cell. `excluded.txt` lists the three.

### Spec and prompt changes between attempts (pilot cells)

Run 11 was restarted four times. Each restart changed the spec, the prompt or the binary after some cells had run, and the cells run before the change became pilot cells. They are in `pilot-agents.json` and `pilot-tokens.json` and are **not** in any total below.

| Attempt | Setup | Cells run | What it showed | Change made before the next attempt |
|---|---|---|---|---|
| 1 | `8fb30a8`: 403-token core spec (`e481b1c`), shorter prompt | a1 | 4 calls / 129,449: read `spec --more` for List and Str builtins | `8b284cf`: the common List and Str builtins go back into the core (482 tokens) |
| 2 | `8b284cf` | a1 to a8, b1 | a1 to a8 95/95; a1 4 calls, b1 6 calls / 208,685 (`spec --more`, then a rejected edit, a refused heredoc and a failed `sed`) | `3fd1ec8`: tuples named in the core again (503 tokens); `edit` lists at most three `stored as:` notes |
| 3 | `3fd1ec8`, prompt said "not a shell heredoc" | a1 to a5 | a5 5 calls / 163,394: tried a heredoc, which the sandbox refused | `00aa3e5`: the prompt says why ("which the sandbox refuses"), as run 10's did |
| 4 | `3fd1ec8` with that prompt | a1 to a5 | a1 and a3 took 4 calls each: a wrong expected value fixed with `sed` in a separate call, and a `python3` heredoc patch of `change.ssp` after the Write | `ef439aa`: the core spec repeats "run `edit --test` in the same turn" and says how `x.f(a)` calls `f` (556 tokens) |
| 5 | `ef439aa` | all 17 | counted, below | none |

The 20 pilot cells total 70 calls and 2,241,001 tokens. Per cell they ranged from 0.72x to 1.57x of the run 10 cell for the same task.

### Result

Each cell is "API calls / total tokens".

| Task | SSPUR run 11 | SSPUR run 10 | Python | TypeScript | Go | SSPUR/Py | SSPUR/TS | SSPUR/Go |
|---|---|---|---|---|---|---|---|---|
| a1_inventory | 3 / 93,721 | 3 / 94,620 | 4 / 127,502 | 4 / 127,407 | 3 / 93,453 | 0.74x | 0.74x | 1.00x |
| a2_wordstats | 4 / 128,604 | 3 / 94,561 | 3 / 91,171 | 4 / 125,943 | 3 / 94,190 | 1.41x | 1.02x | 1.37x |
| a3_bank | 3 / 95,396 | 3 / 96,544 | 5 / 163,080 | 3 / 94,390 | 5 / 172,940 | 0.58x | 1.01x | 0.55x |
| a4_calc | 3 / 95,406 | 3 / 96,259 | 5 / 162,871 | 3 / 94,322 | 4 / 136,461 | 0.59x | 1.01x | 0.70x |
| a5_orders | 3 / 93,336 | 4 / 129,839 | 7 / 230,852 | 4 / 127,797 | 5 / 160,732 | 0.40x | 0.73x | 0.58x |
| a6_todo | 3 / 94,206 | 3 / 94,904 | 5 / 160,083 | 3 / 93,219 | 3 / 94,413 | 0.59x | 1.01x | 1.00x |
| a7_grades | 3 / 93,284 | 3 / 94,157 | 5 / 157,553 | 3 / 92,057 | 4 / 130,585 | 0.59x | 1.01x | 0.71x |
| a8_config | 3 / 94,409 | 4 / 132,068 | 4 / 128,538 | 5 / 165,867 | 5 / 168,540 | 0.73x | 0.57x | 0.56x |
| b1_lru | 4 / 131,890 | 4 / 132,913 | 6 / 197,563 | 5 / 170,590 | 3 / 96,279 | 0.67x | 0.77x | 1.37x |
| b2_calc | 4 / 134,145 | 3 / 97,798 | 5 / 163,264 | 3 / 94,869 | 3 / 96,157 | 0.82x | 1.41x | 1.40x |
| b3_payroll | 3 / 95,343 | 7 / 250,322 | 6 / 198,721 | 3 / 94,117 | 3 / 95,601 | 0.48x | 1.01x | 1.00x |
| b4_ratelimit | 4 / 134,693 | 4 / 136,040 | 7 / 242,533 | 3 / 95,438 | 3 / 96,989 | 0.56x | 1.41x | 1.39x |
| b5_deps | 3 / 97,435 | 3 / 97,488 | 5 / 163,507 | 3 / 94,813 | 3 / 96,245 | 0.60x | 1.03x | 1.01x |
| b6_adventure | 4 / 131,826 | 4 / 134,821 | 7 / 247,687 | 3 / 95,420 | 5 / 168,218 | 0.53x | 1.38x | 0.78x |
| b7_ledger | 3 / 95,999 | 5 / 171,135 | 5 / 168,589 | 3 / 95,813 | 3 / 96,839 | 0.57x | 1.00x | 0.99x |
| b8_merge | 4 / 132,299 | 4 / 133,998 | 4 / 130,148 | 5 / 171,756 | 3 / 96,086 | 1.02x | 0.77x | 1.38x |
| s1_shop | 4 / 129,320 | 3 / 96,306 | 5 / 162,362 | 4 / 131,251 | 8 / 277,404 | 0.80x | 0.99x | 0.47x |
| **Total** | 58 / 1,871,312 | 63 / 2,083,773 | 88 / 2,896,024 | 61 / 1,965,069 | 66 / 2,171,132 | **0.65x** | **0.95x** | **0.86x** |

Every SSPUR cell passed every hidden test: **246/246** (95 for a1 to a8, 141 for b1 to b8, 10 for s1_shop; `scores-a.json`, `scores-b.json`, `scores-s.json`), as did the reused cells of the other three languages.

| Comparison | All 17 cells, total | Median per cell | 16 small tasks, total | 16 small tasks, median | SSPUR cheaper on |
|---|---|---|---|---|---|
| SSPUR / Python | **0.65x** (run 10: 0.72x) | 0.59x | 0.64x | 0.59x | 15 of 17 |
| SSPUR / TypeScript | **0.95x** (run 10: 1.06x) | 1.01x | 0.95x | 1.01x | 6 of 17 |
| SSPUR / Go | **0.86x** (run 10: 0.96x) | 1.00x | 0.92x | 1.00x | 10 of 17 |
| SSPUR run 11 / run 10 | 0.90x | 0.99x | | | 14 of 17 |

| | SSPUR run 11 | SSPUR run 10 | Python | TypeScript | Go |
|---|---|---|---|---|---|
| API calls | 58 | 63 | 88 | 61 | 66 |
| net (total minus 25,750 per call) | 377,812 | 461,523 | 630,024 | 394,319 | 471,632 |
| tool I/O (cl100k, read plus written) | 42,999 | 48,727 | 57,038 | 43,104 | 56,223 |

### From the transcripts

- **10 of 17 cells took the minimum 3 calls** (run 10: 9). On the 7 tasks where both SSPUR and TypeScript took 3 calls, SSPUR cost 1.00x to 1.03x TypeScript (run 10: 1.02x to 1.03x): the shorter spec and prompt took about 1k tokens per call off, and what is left is roughly the remaining spec.
- **The total moved because of the calls, not the spec.** b3 (7 calls in run 10), b7 (5) and a5 and a8 (4) took 3; a2, b2 and s1 took one more call than in run 10. With every cell at about 32k tokens per call, 5 fewer calls are most of the 212k saved.
- **No command was refused**, and one edit was rejected in 17 cells (b1, a parse error from a stray `==` line; the next edit was accepted). In b1 and b6 the agent patched `change.ssp` with a `python3` heredoc instead of a second Write, which the sandbox ran this time; in the pilot cells the same kind of heredoc was refused.
- **The extra calls:** a2 read `spec --more` once (Opt and sort). b2 and b4 each had a wrong expected value in a new test of their own; the failing line printed the actual value (`left -3, right 7`, `left 2, right 0`) and each fixed it in one more call. b8's `require_ok` test was false and needed one more edit. b6 corrected its own `change.ssp` in a separate call before the first edit. s1_shop ran one `q grep` for `"EU"` and `ship_fee` after `start`, though `q pack` had already printed the complete caller list (run 10 did not).
- **One runtime defect surfaced:** b8's last `edit --test` printed `warning: native build failed in the runtime: error: field has incomplete type 'L_T2_Z_S_J'` and fell back to the interpreter. The tests passed and the hidden tests passed, but the native code generator emits a struct for a list of `(Str, J)` tuples before the type it contains, which is a bug to fix.

### Conclusion and caveats

- **Against Python: 0.65x total, 0.59x median**, cheaper on 15 of 17 cells.
- **Against TypeScript: parity, not a win.** The total is 0.95x but the median is 1.01x, and SSPUR is cheaper on only 6 of 17 cells. The total is below 1.0x because of the TypeScript cells that took 4 or 5 calls (a1, a2, a5, a8, b1, b8, s1); on cells where both took 3 calls SSPUR is about 1% more. Both numbers are inside the plus or minus 20% run-to-run band.
- **The large codebase is no longer ahead of TypeScript in this run.** s1_shop took 4 calls (one `q grep` it did not need) and is 0.80x Python, 0.99x TypeScript and 0.47x Go; runs 9 and 10 took 3 calls (0.73x and 0.74x TypeScript). One cell per run, so the claim is "ahead of Python and Go, level with or ahead of TypeScript".
- **Against Go: 0.86x total, 1.00x median.** Also parity on the median; the total is helped by s1_shop, where Go's 8 calls are partly shell friction (see run 9).
- **The restarts bias this run.** Four times, a restart followed cells that had done worse than expected, and those cells are not counted. Even with every change being a spec or prompt fix that applies to all cells, that stopping rule favors the counted run. Averaging every attempt of each cell (pilot and counted) gives 0.99x TypeScript (median 1.01x) and 0.89x Go (median 1.00x): the same parity conclusion, with the total closer to 1.0x.
- One run per cell, one model; the Python, TypeScript and Go cells are not reruns, and their harness did not change. The 3-call cells already cost about what TypeScript does, so the remaining lever is calls: fewer wrong expected values in the agents' own tests, and nothing to look up outside the core spec.

### Recommendations for future runs

- **Freeze before the first cell, and do not restart on results.** Every restart in run 11 was a reasonable fix, but each was made after seeing cells. Pilot on a separate task set (or on 2 or 3 tasks that are then dropped), then freeze the spec, prompt and binary and run all 17 cells once.
- **Rerun the controls.** TypeScript and Go are single runs from run 9. A second TypeScript and Go run on the same tasks would show how much of the 0.95x and 0.86x is their own variance; until then, read anything within 20% of 1.0x as parity.
- **Run each SSPUR cell at least twice** and report the mean and the spread, so one 4-call cell does not move a ratio by 0.03x.
- **Keep b6_adventure but handle the filter**: it was blocked by the content filter in runs 5, 9 and 11. Record every abort, as here, or replace its wording with a neutral one in a new task version.
- **Fix the native tuple-in-list struct ordering** (b8 above) before the next run, so the warning does not cost a reading.

## Results, run 10

Run 9 left SSPUR at 1.23x TypeScript's and 1.11x Go's tokens. Read call by call, its SSPUR losses had three causes: the 1.6k-token spec that every later call re-reads, the sandbox refusing long `edit -e '...'` and heredoc commands (4 cells), and rejected edits for forms another language would accept (`Str.slice`, `.get` on a value that was already unwrapped, `_` in a nested call, and in four cells `main` not declaring an effect that a callee now had). Run 10 changes the CLI, the checker and the SSPUR prompt line, not the language's semantics.

### What changed

1. **The edit goes through a file.** The prompt and the spec say: write the definitions to `change.ssp` with the Write tool, then run `./sspur edit --test change.ssp` in the same turn. The Write call and the edit fit in one API call. `setup.py --r9` and `setup_scale.py --r9` keep the old line.
2. **Tolerant input, canonical storage (ADR 0003, decision 4, extended to the checker).** `sspur edit` now accepts spellings that have one meaning, stores the SSPUR form and prints one line, for example `stored as: && -> and, s.slice(a, b) -> s.drop(a).take(b - a), main now declares fail[CsvErr]`:
   - parser: `&& || !`, `elif`, `let`/`const`/`val`, `let mut`, `x += e`, `Ctor{_}` and `Ctor{..}` patterns, `var x: T = e` and `x: T = e`; `queue`, `store`, `effect`, `pre`, `post` and the other definition keywords are identifiers outside the place they start a construct;
   - checker (type-directed, so only where the receiver type makes it unambiguous): `.length .size .count` to `.len`, `len(x) str(x) sorted(x) sum(x)` and similar to methods, `.to_string .startsWith .toLowerCase .strip .isEmpty .includes .indexOf .unwrap .unwrap_or` and others to their SSPUR names, `True False None Some Ok Err` (when no user constructor has that name, also in patterns), `print` to `log`, `s.slice(a, b)` and `s.substring` on `Str`, `.get` on a value that is not an `Opt` or `Res`, `_` in a call whose parameter is not a function (it moves out to the enclosing call as `x => ...`), and `catch e | E => b` in a test where `e` is not a `Bool` (`catch do (_ = e) false`);
   - missing effects: a function that now performs `fail[E]`, `log` or `div` gets it added to its signature, transitively, instead of an `E_EFFECT_MISSING` rejection.
   Each normalization is a checker diagnostic (`N_FOREIGN`) with a fix op; `edit` applies the fixes and checks again, so other entry points (`check`, `run`, `apply` without `normalize`) still reject the foreign form with the canonical spelling as the message. Ambiguous forms stay errors with hints (`xs.head`, `f(g(_))` with nowhere to move the lambda). `crates/sspur-cli/tests/normalize.rs` checks that every normalization round-trips to the canonical form, that the canonical form is a fixed point, and that the printer never emits a foreign form.
3. **A 777-token core spec** (was 1,631): the example, one line each for types and effects, blocks, `match`/`catch`, lambdas and strings, the common builtins, and the edit loop. Everything else (Map, Set, regex, JSON, effect handlers, the full builtin list) moved to `sspur spec --more` (1,326 tokens). `start` on a small codebase prints the core spec and the whole code: 1,075 to 1,517 tokens on the 16 small tasks, against about 1,940 to 2,370 in run 9.

### Setup

- Run: `runs/2026-10-09-r10/`, compiler frozen at `b7ef6fa` (`target/sspur-frozen-b7ef6fa`), `--tmo` prompts with the run 10 edit line, one fresh Sonnet 5.5 `general-purpose` subagent per cell, strictly one at a time, no retries. All 17 SSPUR cells are new.
- Python, TypeScript and Go cells are run 9's, unchanged (their harness did not change). `xlang/report5.py` prints the table.
- Pilot: three cells (a1, a2, a3) first ran on `f1d5480`, whose prompt said "your file-writing tool". The a3 agent used a shell heredoc, which the sandbox refused. The prompt and spec were changed to name the Write tool, and all 17 cells were then run from scratch; the pilot transcripts are in `pilot-agents.json` and are not counted.

### Result

Each cell is "API calls / total tokens".

| Task | SSPUR run 10 | SSPUR run 9 | Python | TypeScript | Go | SSPUR/Py | SSPUR/TS | SSPUR/Go |
|---|---|---|---|---|---|---|---|---|
| a1_inventory | 3 / 94,620 | 3 / 96,212 | 4 / 127,502 | 4 / 127,407 | 3 / 93,453 | 0.74x | 0.74x | 1.01x |
| a2_wordstats | 3 / 94,561 | 3 / 95,967 | 3 / 91,171 | 4 / 125,943 | 3 / 94,190 | 1.04x | 0.75x | 1.00x |
| a3_bank | 3 / 96,544 | 3 / 98,283 | 5 / 163,080 | 3 / 94,390 | 5 / 172,940 | 0.59x | 1.02x | 0.56x |
| a4_calc | 3 / 96,259 | 3 / 98,128 | 5 / 162,871 | 3 / 94,322 | 4 / 136,461 | 0.59x | 1.02x | 0.71x |
| a5_orders | 4 / 129,839 | 3 / 96,215 | 7 / 230,852 | 4 / 127,797 | 5 / 160,732 | 0.56x | 1.02x | 0.81x |
| a6_todo | 3 / 94,904 | 3 / 96,909 | 5 / 160,083 | 3 / 93,219 | 3 / 94,413 | 0.59x | 1.02x | 1.01x |
| a7_grades | 3 / 94,157 | 3 / 95,815 | 5 / 157,553 | 3 / 92,057 | 4 / 130,585 | 0.60x | 1.02x | 0.72x |
| a8_config | 4 / 132,068 | 8 / 292,709 | 4 / 128,538 | 5 / 165,867 | 5 / 168,540 | 1.03x | 0.80x | 0.78x |
| b1_lru | 4 / 132,913 | 4 / 137,402 | 6 / 197,563 | 5 / 170,590 | 3 / 96,279 | 0.67x | 0.78x | 1.38x |
| b2_calc | 3 / 97,798 | 4 / 139,320 | 5 / 163,264 | 3 / 94,869 | 3 / 96,157 | 0.60x | 1.03x | 1.02x |
| b3_payroll | 7 / 250,322 | 8 / 297,310 | 6 / 198,721 | 3 / 94,117 | 3 / 95,601 | 1.26x | 2.66x | 2.62x |
| b4_ratelimit | 4 / 136,040 | 4 / 137,471 | 7 / 242,533 | 3 / 95,438 | 3 / 96,989 | 0.56x | 1.43x | 1.40x |
| b5_deps | 3 / 97,488 | 5 / 180,773 | 5 / 163,507 | 3 / 94,813 | 3 / 96,245 | 0.60x | 1.03x | 1.01x |
| b6_adventure | 4 / 134,821 | 6 / 212,054 | 7 / 247,687 | 3 / 95,420 | 5 / 168,218 | 0.54x | 1.41x | 0.80x |
| b7_ledger | 5 / 171,135 | 4 / 137,809 | 5 / 168,589 | 3 / 95,813 | 3 / 96,839 | 1.02x | 1.79x | 1.77x |
| b8_merge | 4 / 133,998 | 3 / 99,425 | 4 / 130,148 | 5 / 171,756 | 3 / 96,086 | 1.03x | 0.78x | 1.39x |
| s1_shop | 3 / 96,306 | 3 / 97,465 | 5 / 162,362 | 4 / 131,251 | 8 / 277,404 | 0.59x | 0.73x | 0.35x |
| **Total** | 63 / 2,083,773 | 70 / 2,409,267 | 88 / 2,896,024 | 61 / 1,965,069 | 66 / 2,171,132 | **0.72x** | **1.06x** | **0.96x** |

Every SSPUR cell passed every hidden test: **246/246** (95 for a1 to a8, 141 for b1 to b8, 10 for s1_shop), as did the reused cells of the other three languages.

| Comparison | All 17 cells, total | Median per cell | 16 small tasks, total | 16 small tasks, median | SSPUR cheaper on |
|---|---|---|---|---|---|
| SSPUR / Python | **0.72x** (run 9: 0.83x) | 0.60x | 0.73x | 0.60x | 12 of 17 |
| SSPUR / TypeScript | **1.06x** (run 9: 1.23x) | 1.02x | 1.08x | 1.02x | 6 of 17 |
| SSPUR / Go | **0.96x** (run 9: 1.11x) | 1.01x | 1.05x | 1.01x | 7 of 17 |
| SSPUR run 10 / run 9 | 0.86x | 0.98x | | | 14 of 17 |

| | SSPUR run 10 | SSPUR run 9 | Python | TypeScript | Go |
|---|---|---|---|---|---|
| API calls | 63 | 70 | 88 | 61 | 66 |
| net (total minus 25,750 per call) | 461,523 | 606,767 | 630,024 | 394,319 | 471,632 |
| tool I/O (cl100k, read plus written) | 48,727 | 67,584 | 57,038 | 43,104 | 56,223 |

### From the transcripts

- **No command was refused in 16 of 17 cells.** Every agent wrote `change.ssp` with the Write tool and ran the edit in the same API call. The one refusal (b3) was a later fix attempt that chained `sed` and a heredoc, after the first edit. Run 9 had 4 cells with refusals, 2 of them twice.
- **Normalization removed the rejections it targeted.** a8 (8 calls in run 9, `Str.slice`) took 4: it read `spec --more` once and its edit was accepted first time. b2, b5 and b7's effect rejections and b5's `queue` parameter did not recur. b3's first edit was rejected for `var rows: List[Row] = []` (a typed local, not yet accepted at `b7ef6fa`).
- **3-call cells now cost 1.02x TypeScript, not 1.04x.** 9 of 17 SSPUR cells took the minimum 3 calls (9 in run 9 too, but not the same ones) (`start`, Write plus `edit --test`, DONE). On those cells the remaining gap to TypeScript is the core spec plus the slightly longer prompt, about 1k tokens per call; TypeScript reads no reference.
- **The remaining losses are extra calls, mostly not language errors.** a5, b4 and b7 each wrote a wrong expected value in one of their own new tests (one more `edit --test` to fix it); b8 rewrote `change.ssp` to drop a stub it had left in; b6 had an unbalanced parenthesis; b1 and b4 wrote `catch f(x) | E => b` in a test where `f(x)` is not a `Bool`, the run 9 b1 mistake again. TypeScript's 17 cells had no failing test run at all in run 9.
- **s1_shop** took 3 calls again (96k, 0.59x Python, 0.73x TypeScript, 0.35x Go): `start` with the six names printed 1,611 tokens (run 9: 2,293), then one accepted edit with all four changes.

### Iteration: catch in tests and typed locals

The two language-shaped causes left were `catch e | E => b` on a non-`Bool` `e` in a test (b1, b4) and typed locals (b3). `26b5f3a` normalizes both (see the list above), and only those three SSPUR cells were rerun on it, one at a time, in fresh directories (`runs/2026-10-09-r10-iter/`):

| Cell | First run (`b7ef6fa`) | Rerun (`26b5f3a`) | Hidden tests |
|---|---|---|---|
| b1_lru | 4 / 132,913 | 5 / 169,371 | 21/21 |
| b3_payroll | 7 / 250,322 | 5 / 171,674 | 17/17 |
| b4_ratelimit | 4 / 136,040 | 4 / 135,869 | 17/17 |

- b3's first edit was accepted (`stored as: main now declares fail[CsvErr]`); the extra calls were `spec --more` and updating the old `reported` test for the new format.
- b1's first edit was accepted with every test passing; the agent then spent a call adding two more tests. b4 wrote no catch-in-test this time; its extra call was again a wrong expected value in its own test.
- With the three reruns in place of the first runs: **0.70x Python, 1.04x TypeScript (median 1.02x), 0.94x Go (median 1.01x)**, 62 calls. The reruns were chosen because they targeted those cells, so this number has the usual regression-to-the-mean bias; the first-run numbers above are the unbiased ones.

### Conclusion and caveats

- SSPUR now uses fewer tokens than Python (0.72x) and is level with Go (0.96x) and TypeScript (1.06x) on these 17 cells, from 1.23x and 1.11x in run 9. The medians against TypeScript and Go are 1.02x and 1.01x. That is inside the plus or minus 20% run-to-run band, so the honest claim against TypeScript and Go is still **no measured advantage on the small tasks**, now without the penalty run 9 showed. SSPUR stays ahead of all three on the large codebase.
- What separates SSPUR from TypeScript now is mostly one extra call in 6 cells, and in most of them the cause was a wrong expected value in an agent's own test, which no language feature in this run addresses.
- One run per cell, one model; the Python, TypeScript and Go cells are not reruns. The SSPUR prompt line changed (the edit path), the others did not.
- The pilot changed the prompt after three cells; those cells were rerun with everything else, so all 17 counted cells share one binary and one prompt.

## Results, run 9

Runs 1 to 8 compare SSPUR with Python only. Python is dynamically typed, has no compile step and (in these tasks) no explicit error types, so a win over Python can be a win over Python's verbosity and its agent loop rather than over "other languages". Run 9 adds two controls that the model also knows well and that are statically typed: TypeScript (Node 20, strict `tsc`, `node:test`) and Go (`go test`).

### Setup

- Ports: all 16 small tasks and the large codebase, as described under [Tasks](#tasks). `score.py ref` passes in all four languages: every reference solution passes its hidden tests, every starting codebase fails them (66 rows).
- Run: `runs/2026-10-08-r9-xlang/`, compiler frozen at `aa61bf4` (`target/sspur-frozen-aa61bf4`), `--tmo` prompts, one fresh Sonnet 5.5 `general-purpose` subagent per cell, strictly one at a time, no retries.
- **All SSPUR cells were rerun**, because the SSPUR harness changed after run 8: `4d05484` makes `q pack` mark a complete caller list. The spec is byte-identical to run 8's (1,631 tokens). So the 17 SSPUR cells here are new measurements, not run 4 to 8 numbers.
- Python cells are reused unchanged, since the Python harness has not changed: run 4 after its spec fix for a1 to a8, run 5 for b1 to b8 (including run 5's b8 cell), run 5 for s1_shop.
- `net` uses 25,750 per call for all four languages, as before. A no-op Sonnet subagent measured 26,118 tokens in this session, so `net` is about 370 per call pessimistic for every language equally.
- Exclusions: the SSPUR b6_adventure cell was aborted twice by the API with "Output blocked by content filtering policy" right after `sspur start` (nothing edited, store log confirmed unchanged); the third attempt in the same untouched directory is the one counted. This is the same failure run 5 hit on the same cell. `excluded.txt` lists both.

### Result

Each cell is "API calls / total tokens".

| Task | SSPUR | Python | TypeScript | Go | SSPUR/Py | SSPUR/TS | SSPUR/Go |
|---|---|---|---|---|---|---|---|
| a1_inventory | 3 / 96,212 | 4 / 127,502 | 4 / 127,407 | 3 / 93,453 | 0.75x | 0.76x | 1.03x |
| a2_wordstats | 3 / 95,967 | 3 / 91,171 | 4 / 125,943 | 3 / 94,190 | 1.05x | 0.76x | 1.02x |
| a3_bank | 3 / 98,283 | 5 / 163,080 | 3 / 94,390 | 5 / 172,940 | 0.60x | 1.04x | 0.57x |
| a4_calc | 3 / 98,128 | 5 / 162,871 | 3 / 94,322 | 4 / 136,461 | 0.60x | 1.04x | 0.72x |
| a5_orders | 3 / 96,215 | 7 / 230,852 | 4 / 127,797 | 5 / 160,732 | 0.42x | 0.75x | 0.60x |
| a6_todo | 3 / 96,909 | 5 / 160,083 | 3 / 93,219 | 3 / 94,413 | 0.61x | 1.04x | 1.03x |
| a7_grades | 3 / 95,815 | 5 / 157,553 | 3 / 92,057 | 4 / 130,585 | 0.61x | 1.04x | 0.73x |
| a8_config | 8 / 292,709 | 4 / 128,538 | 5 / 165,867 | 5 / 168,540 | 2.28x | 1.76x | 1.74x |
| b1_lru | 4 / 137,402 | 6 / 197,563 | 5 / 170,590 | 3 / 96,279 | 0.70x | 0.81x | 1.43x |
| b2_calc | 4 / 139,320 | 5 / 163,264 | 3 / 94,869 | 3 / 96,157 | 0.85x | 1.47x | 1.45x |
| b3_payroll | 8 / 297,310 | 6 / 198,721 | 3 / 94,117 | 3 / 95,601 | 1.50x | 3.16x | 3.11x |
| b4_ratelimit | 4 / 137,471 | 7 / 242,533 | 3 / 95,438 | 3 / 96,989 | 0.57x | 1.44x | 1.42x |
| b5_deps | 5 / 180,773 | 5 / 163,507 | 3 / 94,813 | 3 / 96,245 | 1.11x | 1.91x | 1.88x |
| b6_adventure | 6 / 212,054 | 7 / 247,687 | 3 / 95,420 | 5 / 168,218 | 0.86x | 2.22x | 1.26x |
| b7_ledger | 4 / 137,809 | 5 / 168,589 | 3 / 95,813 | 3 / 96,839 | 0.82x | 1.44x | 1.42x |
| b8_merge | 3 / 99,425 | 4 / 130,148 | 5 / 171,756 | 3 / 96,086 | 0.76x | 0.58x | 1.03x |
| s1_shop | 3 / 97,465 | 5 / 162,362 | 4 / 131,251 | 8 / 277,404 | 0.60x | 0.74x | 0.35x |
| **Total** | 70 / 2,409,267 | 88 / 2,896,024 | 61 / 1,965,069 | 66 / 2,171,132 | **0.83x** | **1.23x** | **1.11x** |

Every cell passed every hidden test: **246/246 in each of the four languages** (95 for a1 to a8, 141 for b1 to b8, 10 for s1_shop). `scores-a.json` and `scores-b.json` have the per-cell counts.

Ratios:

| Comparison | All 17 cells, total | Median per cell | 16 small tasks, total | 16 small tasks, median | SSPUR cheaper on |
|---|---|---|---|---|---|
| SSPUR / Python | **0.83x** | 0.75x | **0.85x** | 0.76x | 13 of 17 |
| SSPUR / TypeScript | **1.23x** | 1.04x | **1.26x** | 1.04x | 6 of 17 |
| SSPUR / Go | **1.11x** | 1.03x | **1.22x** | 1.15x | 5 of 17 |

Removing the harness's per-call context (`net`) and counting only what the agents read and wrote (tool I/O) does not change the direction:

| | SSPUR | Python | TypeScript | Go |
|---|---|---|---|---|
| net (total minus 25,750 per call) | 606,767 | 630,024 | 394,319 | 471,632 |
| tool I/O (cl100k, read plus written) | 67,584 | 57,038 | 43,104 | 56,223 |
| SSPUR / language, net | 1.00x | 0.96x | 1.54x | 1.29x |
| SSPUR / language, tool I/O | 1.00x | 1.18x | 1.57x | 1.20x |

So **the Phase 3 criterion as written (versus Python) still holds, and the broader reading of it does not**: against two statically typed languages the model already knows, SSPUR costs more, not less, on these small tasks. The one place SSPUR is ahead of all three is the 1,117-definition codebase.

### Source size per language

cl100k tokens of each reference solution plus its visible tests (the SSPUR and Python references hold both in one file, so the TypeScript and Go columns add `start.test.ts` and `start_test.go`). `xlang/sizes.py` prints this table.

| Task | SSPUR | Python | TypeScript | Go |
|---|---|---|---|---|
| a1_inventory | 639 | 713 | 787 | 997 |
| a2_wordstats | 407 | 434 | 533 | 711 |
| a3_bank | 853 | 941 | 1,013 | 1,329 |
| a4_calc | 964 | 975 | 1,078 | 1,389 |
| a5_orders | 487 | 495 | 612 | 576 |
| a6_todo | 649 | 733 | 799 | 1,064 |
| a7_grades | 479 | 486 | 659 | 800 |
| a8_config | 471 | 604 | 681 | 871 |
| b1_lru | 666 | 717 | 692 | 874 |
| b2_calc | 1,042 | 1,056 | 1,157 | 1,439 |
| b3_payroll | 708 | 780 | 898 | 1,179 |
| b4_ratelimit | 934 | 977 | 874 | 1,099 |
| b5_deps | 839 | 893 | 878 | 1,309 |
| b6_adventure | 1,023 | 1,050 | 1,178 | 1,334 |
| b7_ledger | 984 | 1,113 | 1,151 | 1,421 |
| b8_merge | 942 | 823 | 755 | 1,134 |
| **Total** | **12,087** | **12,790** | **13,745** | **17,526** |
| SSPUR / language | 1.00x | 0.95x | 0.88x | 0.69x |
| median per task | 1.00x | 0.94x | 0.85x | 0.67x |

The large codebase (start, not the reference): SSPUR 30,833, Python 35,788, TypeScript 39,274, Go 41,842.

So SSPUR source is the smallest of the four, by 5% over Python, 12% over TypeScript and 31% over Go. That is the static size of the code, and it is 400 to 1,400 tokens per task: far too small to decide a run where one API call costs about 30,000 tokens of context. Source size and agent-loop cost are nearly independent here.

### Where each language costs tokens, from the transcripts

Friction counted over the 17 cells of each language (`xlang/friction.py` counts sandbox refusals, rejected SSPUR edits, and test or typecheck runs that reported a failure):

| | SSPUR | TypeScript | Go |
|---|---|---|---|
| Commands the sandbox refused to run | 4 | 3 | 6 |
| Rejected edits (nothing applied) | 7 | n/a | n/a |
| Test or typecheck runs that failed | 5 | 0 | 5 |

- **TypeScript is the cheapest because nothing went wrong.** Not one of the 17 TypeScript cells had a failing typecheck or test run, and 10 took the minimum 3 calls: read the file, one or two edits, one combined `tsc && node --test`, done. Sonnet writes idiomatic strict TypeScript with no reference at all, so it reaches SSPUR's best case without SSPUR's 1.6k-token spec. Its 1,965k total is the lowest of the four, and its tool I/O (43k) the smallest.
- **Go pays for error plumbing and for shell friction, not for syntax.** Go also had no compile rejections from unfamiliar syntax, and 10 cells at 3 calls, but it lost calls in 7 cells: a5_orders (2 failing test runs) and b6_adventure (3) to logic and ripple errors after signatures changed, and a3_bank, a4_calc, a7_grades, a8_config and s1_shop to refused shell commands. Go also writes the most code per cell after Python (37,968 written tokens against SSPUR's 29,036), which follows from Go source being 1.45x the size of SSPUR's.
- **SSPUR's 3-call path still works, and is still what wins when it happens.** 9 of 17 SSPUR cells took the minimum 3 calls (`start`, one `edit --test` with every definition and test, DONE) and passed everything. s1_shop took 3 calls for the first time, one fewer than run 8: `q pack` now says the caller list is complete, so the agent no longer spends a call re-checking the callers of `ship_fee`, which the run 8 write-up had predicted would save about that much (0.60x of Python against 0.82x).
- **SSPUR's losses are concentrated in two cells**, a8_config (8 calls, 293k) and b3_payroll (8 calls, 297k). Together they are 590k of SSPUR's 2,409k. Both are the same pattern as earlier runs: the worktree sandbox refused the long single-quoted `edit -e '...'` command (2 refusals each), then the agent wrote the definitions to a file, and the first `edit --test FILE` was rejected for a method that does not exist (`Str.slice` in a8; `.get` on an `Opt` and `_` inside a nested call in b3). Each rejection costs a full call, and the refusals cost two more. Neither TypeScript nor Go can be rejected this way: their equivalent mistake is a compile error from the same call that already ran the tests.
- **The spec is still a real cost, and it no longer buys fewer calls than the controls.** The 1,631-token spec enters the context on call 1 and is re-read by every later call: about 6.5k tokens over a 4-call cell. Against Python that was repaid by 1 to 4 calls saved. Against TypeScript and Go, Sonnet needs no reference at all and reaches the same 3 calls, so the spec is pure overhead on the small tasks.
- **The large codebase is where the store pays off.** The agent ran `start tax_rate money ship_fee order_shipping warehouse_restock` and got back the spec, the counts per kind, and `q pack` of those five definitions with their types, tests and complete caller lists: 2,293 tokens, and it was everything the edit needed. Then one `edit --test` with all four changes and five new tests, accepted first time (187 tests passing), then DONE. TypeScript needed 4 calls: one grep over `shop` and `tests`, one call of targeted `sed -n`/`grep` on the four files it changes, one Python heredoc that patched them and then typechecked and tested, and one re-check. Go needed 8: the same two search calls, then two refused shell commands, one that failed on BSD `sed -i` syntax, a Write of a new `changes_test.go`, a grep to confirm, and `go test`. Go's 0.35x is therefore flattered by environment friction; on calls alone a fair Go run looks like 4 or 5, which would put SSPUR at about 0.55x to 0.65x of Go here, still ahead.
- **Sensitivity.** Over the 9 cells where the sandbox refused nothing in any of the three languages run here, the picture is the same: SSPUR is 0.75x of Python (median 0.82x), 1.25x of TypeScript (median 1.44x) and 1.20x of Go (median 1.26x). `xlang/clean.py` prints this.

### Caveats

- One run per cell, 17 cells, one model. Python's own total has moved by up to 24% between identical runs, so treat any single ratio as about plus or minus 20%. The 1.23x and 1.11x against TypeScript and Go are inside that band of 1.0x, so the honest claim is "no advantage", not "a 23% penalty".
- The two SSPUR outliers (a8, b3) are half of SSPUR's gap to TypeScript. They were not rerun, on purpose: rerunning only the cells that did badly is what earlier runs did, and it biases the total.
- The ports were written by the same model family as the language and its tasks. Each port was checked by `score.py ref` (reference passes, start fails, starting visible tests pass), but a port can still be easier or harder than the original in ways that check cannot see.
- TypeScript and Go got the camelCase and error-convention hints in their prompt; SSPUR and Python did not need them. That is one extra sentence of prompt, about 25 tokens per call.
- The sandbox refuses long or compound shell commands, unevenly across languages (6 Go, 4 SSPUR, 3 TypeScript cells). It is harness friction, not language cost, and it is counted in the totals.
- Go's `net` and tool I/O are measured with the same 25,750 per-call baseline as the others, which is right for Sonnet but was not re-measured per language (the harness context does not depend on the language).

## Results, run 8

Run 7 left one structural cost: the SSPUR agent spent its first call on `sspur spec` alone, while Python's first call is already a search, and every call re-reads about 30k tokens of harness context. Run 8 folds the spec read into the first useful call.

### Options considered

1. **A combined entry command (chosen).** `sspur start [NAME|PATTERN...]` prints the agent spec, then the codebase: all of it when the source is at most 12,000 bytes (every a- and b-task), otherwise the counts per kind, `q pack` of the arguments that name a definition and `q find` of the others. On s1_shop, `start tax_rate money ship_fee order_shipping warehouse_restock Warehouse` is 2,407 tokens (the spec plus 776 of code), and it is everything the edit needs. `sspur spec [src] [QUERY TARGET...]` also runs queries after the spec (`spec find 'a|b' pack A,B`), for agents that already know `spec`.
2. **Spec on first contact (not done).** Prepending the spec to the first `q` or `edit` needs state (a marker in `.sspur/`) that is wrong whenever two agents or two sessions share a store, and it changes what `q` prints. Option 1 removes the same call without state, so the data gave no reason to take the risk.
3. **MCP and skill.** The obvious move, the spec in the MCP `initialize` `instructions`, does not work in Claude Code: it cuts server instructions at 2,048 characters. Checked with Claude Code 2.1.288 and `claude -p --mcp-config`: instructions with the spec appended (5,768 characters) reached the model cut off mid-sentence after about 2,040 characters, ending in `[truncated]`. A silently truncated spec is worse than one call, so the instructions stay short (825 characters, 208 cl100k tokens) and tell the agent to call the new `start` tool first with the names from the task; the tool returns the same text as the CLI. The tool list grew by 139 tokens. One `claude -p` run on s1_shop with only the MCP tools (not comparable with the bench harness, which has no MCP) called `start` first with the right six names, then `query callers ship_fee`, one `edit` with `test: true` (187 passed) and DONE, 10/10 hidden tests. The skill (`packaging/claude-code/skills/sspur/SKILL.md`, 1,099 tokens, was 1,054) now starts with `sspur start NAME...` (or the `start` tool); it does not carry the spec, which would load 1.6k tokens into every session that triggers the skill whether or not it then writes code.
4. **A shorter spec.** In the 60 SSPUR transcripts of runs 4 to 7 (Sonnet, Opus and Haiku; a1 to a9, b1 to b8, s1), no agent wrote code with concurrency (`par`, `atomic`, `chan`), services (`store`, `svc`, `ep`, `db.*`, `migrate_`), packages (`use`, `pub`, `sspur add`) or C, sys, bare and GPU (`extern`, `profile`, `kernel`), and no starting codebase uses them. Those four parts became one pointer line to `spec --full` that keeps their keywords, so an agent still knows they exist. Everything that any transcript used stayed, including `catch`, `raise`, `var`, `for`, `while`, `fold`, `sort_with`, regex and JSON. The CLI line now names `q pack` (it listed only `find|grep|body|callers`). 1,798 to 1,631 cl100k tokens.

The harness's SSPUR instructions changed in one line each: the a- and b-tasks start with `./sspur start` instead of `./sspur spec && ./sspur src` (same content plus a one-line header), and s1 starts with `./sspur start NAME...`, "passing the names of the definitions and types the task mentions". The Python prompts did not change.

### Result

Run: `runs/2026-10-06-r8-start/`, compiler frozen at `bd6c1c8`. Fresh Sonnet 5.5 `general-purpose` subagents, strictly one at a time, `--tmo` prompts, no retries; Python cells are run 5's (s1) and run 4's (a-tasks), as before.

| Cell | Calls SSPUR / Py | Total SSPUR | Total Py | SSPUR / Py | Net SSPUR / Py | Tool I/O SSPUR / Py | Hidden tests |
|---|---|---|---|---|---|---|---|
| s1_shop, run 7, first run | 5 / 5 | 170,039 | 162,362 | 1.05x | 41,289 / 33,612 | 4,067 / 2,997 | 10/10 / 10/10 |
| s1_shop, run 7, second run | 5 / 5 | 167,624 | 162,362 | 1.03x | 38,874 / 33,612 | 3,556 / 2,997 | 10/10 / 10/10 |
| **s1_shop, run 8, first run** | 4 / 5 | 133,103 | 162,362 | **0.82x** | 30,103 / 33,612 | 3,236 / 2,997 | 10/10 / 10/10 |
| **s1_shop, run 8, second run** | 4 / 5 | 133,445 | 162,362 | **0.82x** | 30,445 / 33,612 | 3,373 / 2,997 | 10/10 / 10/10 |

**s1_shop is 0.82x over the two runs (both 0.82x), from 1.04x in run 7 and 1.33x in run 6.** This is the first run in which SSPUR uses fewer tokens than Python on the large codebase, and it is also lower on net tokens (0.90x), so the gain is not only the harness context.

Per call (input tokens, exact):

| Call | Run 8, first | Run 8, second | What it ran |
|---|---|---|---|
| 1 | 27,137 | 27,137 | `start tax_rate money ship_fee order_shipping warehouse_restock Warehouse` (2,407 tokens of output) |
| 2 | 34,519 | 34,524 | `q grep ship_fee` (and a `q grep` for the EU rate), "Need all ship_fee callers" |
| 3 | 34,842 | 34,895 | one `edit --test` with all four changes and the tests, accepted first time (187 and 188 passed), and a `test` alongside it |
| 4 | 35,906 | 36,099 | DONE |

From the two transcripts:
- Both agents passed exactly the six names the task mentions, and neither read the spec separately or listed the codebase.
- Both still spent call 2 confirming the callers of `ship_fee`, although the pack had printed all three (`parcel_shipping`, `rma_shipping`, and `order_shipping` as a target of its own). `q pack` says `-- not shown: N callers` when it cuts the list, but says nothing when the list is complete, so the agent can't tell. Marking a complete caller list would likely remove that call too (3 calls, about 0.62x); that is a query output change and is not in this run.
- Nothing was rejected; each edit was written once.

### Small tasks

The same changes reach the a-tasks through `start` (the whole codebase, as `src` printed it) and the shorter spec. Reran a1, a4 and a7 in SSPUR against the same Python cells:

| Cell | Before: calls, total, ratio | After: calls, total, ratio | Hidden tests |
|---|---|---|---|
| a1_inventory (before: run 7) | 3, 96,557, 0.76x | 3, 96,342, **0.76x** | 12/12 |
| a4_calc (before: run 4) | 3, 98,638, 0.61x | 3, 98,052, **0.60x** | 13/13 |
| a7_grades (before: run 7) | 3, 96,418, 0.61x | 3, 95,591, **0.61x** | 11/11 |

All three took the minimum 3 calls (`start`, one `edit --test`, DONE), passed every hidden test and moved by under 1%: the spec cut saves about 170 tokens per call, which is what the totals show. No regression. The cut parts of the spec (services, packages, concurrency, C) are not exercised by any task here, so this run cannot show what the cut costs an agent that needs them; such an agent now reads `spec --full` once.

Caveats: two runs for the s1 number and one for each small cell; the two s1 runs agree within 0.3%. The Python cells were not rerun (s1 since run 5). The MCP probe is one `claude -p` run outside the bench harness.

## Results, run 7

### Where s1_shop's tokens go (diagnosis)

Every input token of the run 6 SSPUR transcript and the run 5 Python transcript (the cells behind the 1.33x), attributed to what put it in the context. A piece that enters the context before call j is re-read by calls j to N, so it is counted once per call that reads it. The per-call input is exact (API usage); each increase from one call to the next is split between the agent's previous output and the tool results it got, in proportion to their cl100k counts. After the first tool result the harness adds about 3.8k tokens of its own on both sides (the same jump appears in every transcript); that is counted as harness. Script: the attribution follows `tokens.py`, one bucket per source.

| Where the tokens went | SSPUR (run 6) | Python (run 5) | SSPUR minus Python |
|---|---|---|---|
| Harness context (system prompt, tool schemas, harness additions), once per call | 177,434 (6 calls) | 146,860 (5 calls) | **+30,574** |
| Task prompt, re-read every call | 4,122 | 2,540 | +1,582 |
| Spec (read in call 1, re-read by calls 2 to 6) | 12,121 | 0 | **+12,121** |
| Search output (`q`, `grep`, `sed`), re-read | 17,908 | 6,714 | **+11,194** |
| Edit and test output, re-read | 320 | 942 | -622 |
| The agent's own commands and text, re-read | 3,078 | 3,989 | -911 |
| Output tokens, all calls | 965 | 1,303 | -338 |
| Total | 215,948 | 162,348 | +53,600 |

Per call (input is exact; tool output in cl100k tokens):

| Call | SSPUR: what it ran | Input | Tool output | Python: what it ran | Input | Tool output |
|---|---|---|---|---|---|---|
| 1 | `spec` | 27,101 | 1,798 | `grep -rn` for the five names and `19` | 26,848 | 609 |
| 2 | `q grep ship_fee`, `q find` with `^Warehouse` (unsupported then, so it printed nothing), `q grep tax_rate` (44 near-identical `*_tax` callers) | 33,384 | 1,075 | `sed -n` on two files, two greps | 31,558 | 719 |
| 3 | `q body` of five, `q find 'restock\|active'` (88 signatures), two greps | 35,626 | 1,122 | Python heredoc patching the files, crashed on the locale | 32,954 | 241 |
| 4 | `q callers ship_fee`, `q grep '19'`, `q grep 'money('`, `q find warehouse` | 37,794 | 980 | the same patch with `LC_ALL`, then pytest | 34,473 | 111 |
| 5 | `q body` of three, then one `edit --test` with all four changes and five tests | 39,935 | 199 | DONE | 35,212 | |
| 6 | DONE | 41,143 | | | | |

So the 53.6k gap is:
- **57%: one more call.** SSPUR spends its first call reading only the spec; Python's first call is already a search. Every call re-reads about 30k of harness context.
- **23%: the spec**, 1,798 cl100k tokens (about 2.4k Claude tokens) re-read by 5 later calls.
- **21%: exploration in three rounds instead of two**, with broad answers that stay in the context: `q grep tax_rate` returned all 44 callers with their lines (about 900 tokens), `q find 'restock|active'` 88 signatures. What the agent was looking for in rounds 2 and 3 is what `q pack` gives (the bodies, the record type, the callers, the tests), but `q pack` takes one name and the agent had five.
- Not a cost: `edit --test` on 187 tests printed two lines (the `ok` line and `187 passed, 0 failed`, 199 tokens with the `q body` before it); there were no rejections in run 6 (run 5 had one, 65 tokens); re-reading bodies just before the edit cost about 70 tokens. Running only the affected tests or capping failure output would save nothing measurable on this task.

### What changed, and the result

Two changes, both in the query output, none in the language, the checker or the `--json` formats:
- **`q pack A,B,C`** packs several definitions in one call: each one with the types and signatures it uses, its tests and its callers, with every definition printed only once (a target that is also another's caller is shown in full, not twice). The five definitions this task changes cost 540 tokens in one call. `q pack NAME` is unchanged.
- **`q find` prints 25 signatures and `q grep` 12 definitions in full**, then the names of the rest on one line (`-- 33 more: invoice_line_tax invoice_tax ...`), and definitions whose name is exactly the pattern come first. A broad pattern on a repetitive codebase no longer fills the context with near-identical matches.

Measured on `s1_shop` (cl100k tokens of the command output; "before" renders the same hits the way run 6 did):

| Command | Before | After |
|---|---|---|
| `q grep tax_rate` (44 callers) | 1,012 | 331 |
| `q find 'restock\|active'` (88 matches) | 869 | 472 |
| `q find 'tax_rate\|money\|ship_fee\|order_shipping\|warehouse_restock\|Warehouse'` | 351 | 351 |
| `q grep 19` | 275 | 275 |
| `q pack ship_fee,money,tax_rate,warehouse_restock,order_shipping` | not available | 540 |

The s1 prompt's search hint now names `q pack A,B,C` instead of `q pack NAME`. Nothing else in the prompts, the harness or the method changed; the Python cell is run 5's, rerun on nothing.

Run: `runs/2026-10-06-r7-s1/`, compiler frozen at `9be5226`. Two fresh Sonnet 5.5 `general-purpose` subagents on s1_shop, one at a time, and one each on a1_inventory and a7_grades as a regression check.

| Cell | Calls SSPUR / Py | Total SSPUR | Total Py | SSPUR / Py | Net SSPUR / Py | Tool I/O SSPUR / Py | Hidden tests |
|---|---|---|---|---|---|---|---|
| s1_shop, run 5 | 6 / 5 | 217,181 | 162,362 | 1.34x | 62,681 / 33,612 | 5,495 / 2,997 | 10/10 / 10/10 |
| s1_shop, run 6 | 6 / 5 | 215,964 | 162,362 | 1.33x | 61,464 / 33,612 | 6,153 / 2,997 | 10/10 / 10/10 |
| **s1_shop, run 7, first run** | 5 / 5 | 170,039 | 162,362 | **1.05x** | 41,289 / 33,612 | 4,067 / 2,997 | 10/10 / 10/10 |
| **s1_shop, run 7, second run** | 5 / 5 | 167,624 | 162,362 | **1.03x** | 38,874 / 33,612 | 3,556 / 2,997 | 10/10 / 10/10 |

**s1_shop is 1.04x over the two runs (1.05x and 1.03x), from 1.33x.** SSPUR still does not win this task; it is now within noise of Python. The remaining 6k tokens are the spec read in call 1 and carried by 4 later calls (9,692 of the 170,039) against Python's 0, partly offset by SSPUR's cheaper searches.

Where the two runs went (same attribution as above):

| Where the tokens went | Run 6 | Run 7, first | Run 7, second |
|---|---|---|---|
| Harness context, once per call | 177,434 (6 calls) | 147,180 (5) | 147,180 (5) |
| Task prompt, every call | 4,122 | 3,430 | 3,430 |
| Spec | 12,121 | 9,692 | 9,692 |
| Search output | 17,908 | 6,596 | 4,441 |
| Edit and test output | 320 | 102 | 98 |
| The agent's own commands and text | 3,078 | 2,209 | 2,014 |
| Output tokens | 965 | 830 | 769 |
| Total | 215,948 | 170,025 | 167,610 |

From the two transcripts:
- Both agents explored in **one round instead of three**: `q find` with all five names plus `Warehouse`, `q callers ship_fee`, `q grep`, then `q body A,B,..` of what they found, and then the edit. Both wrote one `edit --test` with all four changes and five tests, accepted first time, 187 tests passing. That is 5 calls: spec, search, search, edit, DONE.
- Neither used `q pack A,B,C`, although the prompt names it: both preferred `q find` plus `q body A,B`, which costs about the same here (cell B's whole search was 863 tokens). So the gain came from the caps and from the broad answers no longer inviting another round, not from `pack`.
- The caps bit where predicted: cell A's `q grep 19` and `q grep tax_rate` returned 639 and 331 tokens where run 6's equivalents returned 685 and about 900.
- One call is still spent on the spec alone, because the prompt tells the agent to start with it. Fusing the spec read with the first search would save about 30k (one call's harness context) and would make this task cheaper than Python; it needs a prompt change, which is a harness change, so it is not done here (run 8 does it, with `sspur start`).

### Small-task regression check

The same two changes could have hurt the small tasks (the a-task prompts tell agents to read the whole codebase with `src`, so `q` is rarely used, but `edit` output and the spec are shared). Reran a1 and a7 in SSPUR with Sonnet, same prompts as run 4, against run 4's Python cells:

| Cell | Before: calls, total, ratio | After: calls, total, ratio | Hidden tests |
|---|---|---|---|
| a1_inventory | 3, 96,943, 0.76x | 3, 96,557, **0.76x** | 12/12 |
| a7_grades | 3, 96,327, 0.61x | 3, 96,418, **0.61x** | 11/11 |

Both still take the minimum 3 calls and pass every hidden test; the totals move by under 0.5%, which is noise. No regression.

Caveats: two runs for the new s1 number and one for each old one, so part of the 1.33x to 1.04x move could be variance; the two run 7 runs agree within 1.4% of each other, which bounds it loosely. The Python cell has not been rerun since run 5. The s1 prompt's search hint changed one phrase.

## Results, run 6

Run 6 reruns only the cells that the changes in this round target, with the same harness, prompts and method as run 5 (one fresh `general-purpose` subagent per cell, model override `haiku` or `sonnet`, strictly one at a time, `setup.py --tmo`, no retries). The Python cells are run 5's. Compiler frozen at `647355f`. Run directory: `runs/2026-10-06-r6-agent-packaging/`.

What changed since run 5 (all in the CLI and checker, not the language):
- Fix hints for what the run 5 transcripts showed weak models writing: `&& || !`, `len(x)`, `Ctor{_}`, effect rows without commas, regex escapes, `elif`, `let`, `+=`, foreign method names (`head`, `max_by`, `compare`, `to_string`), `None`/`Some`, a missing return type, a nested `match`, and more. Each rejected edit now lists the syntax errors of every definition (not only the first) and ends with `fix these and resend the whole edit in one call`.
- `q list` groups by kind with counts and no longer prints all test names on one line; new `q find 'a|b*'`, `q grep TEXT`, `q body A,B`; `q pack` caps callers. The s1 prompt names `q find` and `q grep` instead of `q list | grep`.
- The agent spec changed only in its CLI line (`q find|grep|body|callers X`); it is still 1,798 tokens.

| Cell | Before: calls, total, ratio to Py | After: calls, total, ratio to Py | Hidden tests before / after |
|---|---|---|---|
| Sonnet s1_shop | 6, 217,181, 1.34x | 6, 215,964, **1.33x** | 10/10 / 10/10 |
| Haiku a1_inventory | 24, 775,681, 3.46x | 7, 198,889, **0.89x** | 12/12 / 12/12 |
| Haiku a2_wordstats | 22, 696,585, 1.75x | 15, 442,072, **1.11x** | 11/11 / 11/11 |
| Haiku a7_grades | 11, 307,487, 1.38x | 8, 214,498, **0.96x** | 11/11 / 11/11 |

Haiku 4.5 on the 8 tasks with these three cells replaced: **2,016,545 against 2,423,178 tokens, 0.83x** (median 0.88x), 71 calls against 93, cheaper on 6 of 8 (was 1.21x, median 0.97x, 98 calls). SSPUR still passes 95/95 and Python 84/95.

From the transcripts:
- **The hints did what they were for.** Haiku a1 wrote `&&` and then `len(x)`; each time it read the hint, fixed that one thing and resent the whole edit (3 edits, the third accepted with 10 tests passing). In run 5 the same cell fell back to one definition per edit after the first rejection (11 accepted single-definition edits, 24 calls). a2 had one rejection (`!`), fixed from the hint; a7 had none.
- **Haiku still verifies after success.** Every Haiku cell re-read `src` or ran `test` or `check` after an edit that had printed `ok` and `N passed, 0 failed`; a2 spent 7 of its 15 calls that way (`src`, `test`, `check`, five `q body`). That is the largest remaining cost and is not addressed yet.
- **s1 did not improve.** Sonnet used the new queries (`q grep ship_fee`, `q find 'money|tax_rate|...'`, `q body a,b,c`) but made the same 6 calls: spec, three exploratory query calls, one `edit --test` that passed, done. Cheap queries made it explore more, not less: it read 5,188 tokens of tool output against 4,108 in run 5 (a `q grep 'tax_rate'` matched 44 near-identical `*_tax` functions, a `q grep '19'` matched sample data). It also tried `q find '^Warehouse'`, which run 6's binary did not support; `^` and `$` anchors were added afterwards (`557bccd`). The fixed costs (the spec carried through every call, one more call than Python) still decide this task.
- Haiku a2 wrote its edit to `/tmp` with a heredoc, and a7 lost one call to a refused heredoc, then used the Write tool (sandbox friction, as in run 5).

Query output on s1_shop, measured directly (cl100k tokens of the command output, run 5 query code at `db3c345` against `647355f`):

| To locate | Run 5 commands | Tokens | Run 6 commands | Tokens |
|---|---|---|---|---|
| tax_rate, money, ship_fee, order_shipping, warehouse_restock and the callers of ship_fee | `q list \| grep -iE "ship\|money\|tax_rate\|restock\|warehouse\|express"` | 1,875 | `q find 'tax_rate\|money\|ship_fee\|order_shipping\|warehouse_restock'` and `q grep ship_fee` | 63 + 63 = 126 |
| everything | `q list` | 13,702 | `q list` | 13,165 |
| context to change money (45 callers) | `q pack money` | 1,174 | `q pack money` | 178 |
| context to change tax_rate (44 callers) | `q pack tax_rate` | 1,002 | `q pack tax_rate` | 175 |

So the targeted lookup is 15x cheaper, but on this task tool output was never the main cost (under 6k of 216k tokens either way).

Caveats: one run per cell; the rerun cells were picked because they did worst, so some of the improvement is regression to the mean (Haiku a6 went from 61 calls to 8 on a spec change that should not have mattered that much). Cells that were not rerun may have changed too, in either direction.

## Results, run 5

Run 5 (2026-10-06) widens the evaluation in three ways: two more models on the same tasks, 8 new tasks specified from neutral sources, and one large codebase.

Setup, as in run 4 unless stated:
- Compiler frozen at `cfc9ace` (the spec there is 1,798 tokens: run 4's fixed spec plus the a9 fix and the ADR 0026 Packages line; that commit also shortened the CLI line's "if the shell refuses that command, write a file" sentence to "Or save them to FILE"). The spec fix below was rebuilt at `4b75361`; only `docs/agent/agent-spec.md` changed.
- One fresh Claude Code `general-purpose` subagent per (model, language, task) cell, chosen with the Agent tool's model override (`haiku`, `opus`, `sonnet`), run strictly one at a time, with `setup.py --tmo` prompts. No retries, except one cell that the API aborted (below).
- New cells: Haiku 4.5 and Opus 5.5 on a1 to a8 and a9, both languages; Sonnet 5.5 on b1 to b8 and s1, both languages. The Sonnet a1 to a8 numbers are run 4's (after its spec fix); Sonnet was not rerun on them.
- `net` subtracts each model's own no-op context: Sonnet 25,750 (as before), Opus 26,100, Haiku 19,100 (measured with a one-line no-op subagent per model; Haiku's harness context is smaller). `tokens.py` takes it from `BASE`.
- Runs: `runs/2026-10-06-{haiku,opus}` (a1 to a8), `-{haiku,opus}-a9`, `-sonnet-b`, `-sonnet-scale`, and the spec-fix reruns in `-haiku-spec-fix`, `-haiku-a9-spec-fix`, `-opus-a9-spec-fix`, `-sonnet-b-spec-fix`.

### Per model, the 8 tasks

| Model | Hidden tests SSPUR / Py | Calls SSPUR / Py | Total SSPUR | Total Py | SSPUR / Py | Median | SSPUR cheaper on |
|---|---|---|---|---|---|---|---|
| Sonnet 5.5 (run 4, after its fix) | 95/95 / 95/95 | 26 / 38 | 854,818 | 1,221,650 | **0.70x** | 0.61x | 6 of 8 |
| Opus 5.5 | 95/95 / 95/95 | 28 / 38 | 928,412 | 1,226,632 | **0.76x** | 0.76x | 6 of 8 |
| Haiku 4.5 | 95/95 / 84/95 | 150 / 93 | 5,597,074 | 2,423,178 | **2.31x** | 1.22x | 3 of 8 |
| Haiku 4.5, after the spec fix (a1, a2, a6 rerun) | 95/95 / 84/95 | 98 / 93 | 2,940,839 | 2,423,178 | **1.21x** | 0.97x | 4 of 8 |

Opus 5.5:

| Task | Hidden tests SSPUR / Py | API calls SSPUR / Py | Total tokens SSPUR | Total tokens Py | SSPUR / Py | Net of fixed overhead SSPUR / Py | Tool I/O SSPUR / Py |
|---|---|---|---|---|---|---|---|
| a1_inventory | 12/12 / 12/12 | 3 / 5 | 96,750 | 159,457 | 0.61x | 18,450 / 28,957 | 3,080 / 2,479 |
| a2_wordstats | 11/11 / 11/11 | 3 / 4 | 96,588 | 126,550 | 0.76x | 18,288 / 22,150 | 2,949 / 2,620 |
| a3_bank | 13/13 / 13/13 | 3 / 5 | 98,916 | 166,337 | 0.59x | 20,616 / 35,837 | 3,749 / 3,397 |
| a4_calc | 13/13 / 13/13 | 3 / 4 | 98,672 | 128,982 | 0.77x | 20,372 / 24,582 | 3,809 / 2,644 |
| a5_orders | 11/11 / 11/11 | 4 / 5 | 133,181 | 161,067 | 0.83x | 28,781 / 30,567 | 3,223 / 2,953 |
| a6_todo | 11/11 / 11/11 | 4 / 4 | 133,417 | 127,159 | 1.05x | 29,017 / 22,759 | 3,284 / 2,273 |
| a7_grades | 11/11 / 11/11 | 3 / 6 | 96,469 | 194,216 | 0.50x | 18,169 / 37,616 | 2,900 / 2,754 |
| a8_config | 13/13 / 13/13 | 5 / 5 | 174,419 | 162,864 | 1.07x | 43,919 / 32,364 | 4,641 / 3,434 |
| **Total** | 95/95 / 95/95 | 28 / 38 | **928,412** | **1,226,632** | **0.76x** | 197,612 / 234,832 (0.84x) | 27,635 / 22,554 (1.23x) |

Haiku 4.5 (a1, a2 and a6 after the spec fix are in the next table):

| Task | Hidden tests SSPUR / Py | API calls SSPUR / Py | Total tokens SSPUR | Total tokens Py | SSPUR / Py | Net of fixed overhead SSPUR / Py | Tool I/O SSPUR / Py |
|---|---|---|---|---|---|---|---|
| a1_inventory | 12/12 / 12/12 | 25 / 9 | 823,869 | 224,454 | 3.67x | 346,369 / 52,554 | 11,724 / 4,339 |
| a2_wordstats | 11/11 / 11/11 | 20 / 15 | 607,521 | 398,489 | 1.52x | 225,521 / 111,989 | 9,149 / 5,678 |
| a3_bank | 13/13 / 13/13 | 7 / 14 | 191,307 | 368,525 | 0.52x | 57,607 / 101,125 | 6,614 / 6,120 |
| a4_calc | 13/13 / 13/13 | 10 / 16 | 293,576 | 445,218 | 0.66x | 102,576 / 139,618 | 8,270 / 7,738 |
| a5_orders | 11/11 / 11/11 | 6 / 9 | 154,479 | 228,196 | 0.68x | 39,879 / 56,296 | 4,368 / 4,646 |
| a6_todo | 11/11 / 11/11 | 61 / 10 | 2,916,444 | 251,737 | 11.59x | 1,751,344 / 60,737 | 27,687 / 4,327 |
| a7_grades | 11/11 / 11/11 | 11 / 9 | 307,487 | 223,114 | 1.38x | 97,387 / 51,214 | 7,493 / 4,128 |
| a8_config | 13/13 / 2/13 | 10 / 11 | 302,391 | 283,445 | 1.07x | 111,391 / 73,345 | 10,610 / 5,766 |
| **Total** | 95/95 / 84/95 | 150 / 93 | **5,597,074** | **2,423,178** | **2.31x** | 2,732,074 / 646,878 (4.22x) | 85,915 / 42,742 (2.01x) |

Haiku's one Python failure (a8, 2/13) is a logic error: it changed the entry separator from `;` to a newline.

### The 9th task (a9, regex and JSON)

| Model and spec | Hidden tests SSPUR / Py | API calls SSPUR / Py | Total tokens SSPUR | Total tokens Py | SSPUR / Py |
|---|---|---|---|---|---|
| Sonnet 5.5, run 4 (`69e0d69`) | 17/17 / 17/17 | 3 / 4 | 97,834 | 130,948 | 0.75x |
| Opus 5.5, `cfc9ace` | 17/17 / 17/17 | 6 / 4 | 223,320 | 132,415 | 1.69x |
| Opus 5.5, `4b75361` (spec fix) | 17/17 / 17/17 | 5 / 4 | 181,477 | 132,415 | 1.37x |
| Haiku 4.5, `cfc9ace` | 15/17 / 17/17 | 113 / 8 | 9,121,401 | 210,644 | 43.30x |
| Haiku 4.5, `4b75361` (spec fix) | 16/17 / 17/17 | 33 / 8 | 1,091,558 | 210,644 | 5.18x |

Haiku's first SSPUR attempt parsed JSON by hand instead of with `json.decode` and rejected JSON with spaces; the rerun used `json.decode` but its `to_json` did not escape quotes. Opus spent its extra calls reading `spec --full` for regex escapes and re-running `check` and `test` after an `edit --test` that had already passed.

### Independently specified tasks (b1 to b8)

The designer's tasks could favour the language. These 8 tasks are ports of well-known exercises and specs. They were written for this run by the same model family that designed the language (Opus 5.5), but each starts from a public source, and for each task the hidden tests were written from the task text before either reference solution. Each starting codebase exists in both languages with the same functions, behavior and visible tests (30 to 60 lines); `python3 score.py ref` checks that both references pass and both starts fail.

| Task | Source | What changes | Hidden tests |
|---|---|---|---|
| b1_lru | LeetCode 146 (LRU Cache) | capacity check with a new error, eviction, a signature change (lookup returns the updated cache) through callers, two counter fields, peek, resize | 21 |
| b2_calc | LeetCode 224 and 227 (Basic Calculator) | precedence, left associativity, truncating division, parentheses, unary minus, two new errors, rendering | 17 |
| b3_payroll | The AWK Programming Language, ch. 1 (emp.data), as CSV | header and blank lines, trimming, errors with line numbers, overtime, sorted report with a total, collect-all validation | 17 |
| b4_ratelimit | Token bucket (Wikipedia), fixed window | an off-by-one, a token bucket, a clock-skew error that makes two functions fallible, retry time, per-key buckets | 17 |
| b5_deps | Topological sort, Kahn's algorithm (LeetCode 210) | duplicates, missing packages, cycle paths, error rendering, a full order with a stuck error, reverse dependencies | 18 |
| b6_adventure | Colossal Cave style two-word parser | a forgiving parser, sorted descriptions, take/drop/inventory, a record field added everywhere for a locked door, a score | 17 |
| b7_ledger | Double-entry bookkeeping invariants | four ordered invariants, no negative asset accounts, a filtered sorted trial balance, reversals, running history | 17 |
| b8_merge | RFC 7396 (JSON Merge Patch), with its Appendix A examples | recursive merge patch, key order, string escaping, array paths, merge_all, a required-keys error | 17 |

Sonnet 5.5 (run `runs/2026-10-06-sonnet-b/`):

| Task | Hidden tests SSPUR / Py | API calls SSPUR / Py | Total tokens SSPUR | Total tokens Py | SSPUR / Py | Net of fixed overhead SSPUR / Py | Tool I/O SSPUR / Py |
|---|---|---|---|---|---|---|---|
| b1_lru | 21/21 / 21/21 | 3 / 6 | 98,884 | 197,563 | 0.50x | 21,634 / 43,063 | 3,527 / 3,372 |
| b2_calc | 17/17 / 17/17 | 3 / 5 | 99,962 | 163,264 | 0.61x | 22,712 / 34,514 | 3,963 / 3,300 |
| b3_payroll | 17/17 / 17/17 | 5 / 6 | 175,819 | 198,721 | 0.88x | 47,069 / 44,221 | 4,798 / 3,743 |
| b4_ratelimit | 17/17 / 17/17 | 4 / 7 | 138,746 | 242,533 | 0.57x | 35,746 / 62,283 | 4,052 / 4,258 |
| b5_deps | 18/18 / 18/18 | 4 / 5 | 136,944 | 163,507 | 0.84x | 33,944 / 34,757 | 3,707 / 3,219 |
| b6_adventure | 17/17 / 17/17 | 3 / 7 | 99,684 | 247,687 | 0.40x | 22,434 / 67,437 | 4,068 / 6,844 |
| b7_ledger | 17/17 / 17/17 | 3 / 5 | 99,662 | 168,589 | 0.59x | 22,412 / 39,839 | 3,744 / 4,344 |
| b8_merge | 17/17 / 17/17 | 7 / 4 | 268,082 | 130,148 | 2.06x | 87,832 / 27,148 | 8,542 / 3,543 |
| **Total** | 141/141 / 141/141 | 32 / 45 | **1,117,783** | **1,512,012** | **0.74x** | 293,783 / 353,262 (0.83x) | 36,401 / 32,623 (1.12x) |

Median 0.60x, cheaper on 7 of 8. With b8 rerun on the fixed spec (4 calls, 141,274 tokens, 1.09x), the total is **0.66x** (median 0.60x, 29 calls against 45). This is close to the designer's tasks with the same model (0.70x, median 0.61x), so on this evidence the run 3 and 4 results were not an artifact of who wrote the tasks.

The API aborted the SSPUR b6 cell twice with "Output blocked by content filtering policy" right after the agent read the spec and code (call 2, nothing edited); the third attempt in the same untouched directory is the one counted (`excluded.txt`). The Python b6 cell ran once.

### Large codebase (s1_shop)

`tasks/s1_shop/gen.py` generates one shop application in both languages: a common module (money formatting, tax rates, shipping fees) and 44 domain modules (customer, invoice, warehouse, ...) of one record type and 20 functions each, plus 182 tests. That is 1,117 definitions in SSPUR (one store, 31k cl100k tokens of source) and 935 functions and classes plus 182 test functions in Python (a `shop/` package of 45 modules and `tests/`, 36k tokens). The task makes four targeted changes: the EU tax rate, a money formatting bug, a new parameter on `ship_fee` with its 3 callers in three modules plus a new express function, and a new `warehouse_restock_all`. Both prompts say the codebase is large and say how to search it: `q list | grep`, `q body`, `q callers` and `q pack` for SSPUR, `grep -rn` for Python. Hidden tests: 10 per language (`hidden.ssp`, `hidden.py`, `score_scale.py`).

| Task | Hidden tests SSPUR / Py | API calls SSPUR / Py | Total tokens SSPUR | Total tokens Py | SSPUR / Py | Net of fixed overhead SSPUR / Py | Tool I/O SSPUR / Py |
|---|---|---|---|---|---|---|---|
| s1_shop | 10/10 / 10/10 | 6 / 5 | 217,181 | 162,362 | **1.34x** | 62,681 / 33,612 | 5,495 / 2,997 |

Neither agent read the whole codebase; both read under 6k tokens of tool output. Where the tokens went:
- SSPUR: the spec (1,798), one combined query (`q list | grep -iE "ship|money|tax_rate|restock|warehouse|express"`, then `callers` and `body`) that returned 2,045 tokens, a second query of 144, one `edit --test` with all four changes and five tests, which was rejected because a test called `len(x)` instead of `x.len`, the corrected edit, and a `test` run.
- Python: `grep -rn "ship_fee\|def money\|def tax_rate\|..."` (609 tokens), `sed -n` on the two files it needed plus another grep (719), a Python heredoc that patched the files and crashed on the locale, and the same patch with `LC_ALL` set.
- `q list` prints one signature per line, but all 182 test names on a single `tests:` line, so any grep pattern that matches a test name returns all of them; here that line and the 44 `*_restock` signatures were most of the 2,045 tokens. A Python `grep "def name"` returns exactly the lines asked for.
- `q pack` was not used. At this size targeted search costs about the same in both languages, so the fixed costs decide: the spec carried through every later call and one extra call.
- One run per side; a 1-call difference is within the noise seen elsewhere. What it does show is that the query surface gives no advantage over grep here, so the claim that SSPUR wins on large codebases is not supported yet.

### Spec fix

From the transcripts, five rules were missing or too easy to miss. `docs/agent/agent-spec.md` (`4b75361`) now says:
- effect rows are comma-separated, with an example `! fail[E], log` (Haiku a6 wrote `! log fail[TaskErr]` three times);
- a bare `Ctor` pattern ignores the fields (Haiku a6 wrote `NotFound{_}`);
- a `match` nested inside an arm takes the arms below it, so use a helper fn (Sonnet b8 lost two edits to it; writing the b8 reference solution hit it too);
- a literal `{` is `\{`, with a JSON example; the escapes are `\n \t \" \\ \{`; regex `\d` is written `"\\d"` (Haiku a9 had 23 edits rejected for an unescaped `{` in JSON text, and `\[` and `\r` escapes);
- `&& || !` are not operators (Haiku used them in a1 and a2).

To stay under 1.8k tokens (1,798 before and after), the C, sys, bare and GPU line became a pointer to `spec --full`, and `sync` and `deploy` left the CLI line. The affected SSPUR cells were rerun against the same Python cells:

| Cell | Before: calls, total, ratio | After: calls, total, ratio | Hidden tests before / after |
|---|---|---|---|
| Sonnet b8_merge | 7, 268,082, 2.06x | 4, 141,274, 1.09x | 17/17 / 17/17 |
| Opus a9_events | 6, 223,320, 1.69x | 5, 181,477, 1.37x | 17/17 / 17/17 |
| Haiku a1_inventory | 25, 823,869, 3.67x | 24, 775,681, 3.46x | 12/12 / 12/12 |
| Haiku a2_wordstats | 20, 607,521, 1.52x | 22, 696,585, 1.75x | 11/11 / 11/11 |
| Haiku a6_todo | 61, 2,916,444, 11.59x | 8, 219,333, 0.87x | 11/11 / 11/11 |
| Haiku a9_events | 113, 9,121,401, 43.30x | 33, 1,091,558, 5.18x | 15/17 / 16/17 |

- Sonnet b8 put the array index in a helper fn on the first try. Opus a9 still read `spec --full` once, for the regex syntax.
- Haiku a1 and a2 did not change: Haiku wrote `&&` and `!` again although the spec now says they are not operators, and then edited one definition per call. Haiku a6 and a9 improved a lot, but single Haiku runs vary widely (a6 went from 61 calls to 8), so most of that may be noise. Haiku a9 still fails one hidden test.
- Cells that were already at the 3-call minimum were not rerun on the new spec, so a regression there would not show.

### Where SSPUR wins and loses, from the transcripts

- **Wins come from one edit.** When the agent follows the spec's path (read spec and code, one `edit --test` with every definition and test, done), a task takes 3 calls; Python agents take 4 to 7 (read, several targeted edits, run pytest, sometimes fix). Opus used 3 calls on 5 of 8 tasks and Sonnet on 4 of 8 new tasks. SSPUR agents read more (the 1.8k spec) and write about as much, so tool I/O is 1.0x to 1.3x for Sonnet and Opus; the savings are the calls not made, each of which re-reads the 19k to 26k harness context.
- **Losses come from calls spent on the language.** Every SSPUR loss above 1.1x is a run of rejected edits: syntax from other languages (`&&`, `!`, `len(x)`, `.compare`, `.head`, `max_by`, `NotFound{_}`), string escaping in JSON and regex text, the nested `match`, the `catch do` block layout, `while` needing `div`, and effects that ripple to callers (b3: `main` had to declare `fail[CsvErr]`). Python agents mostly fail on logic, which costs a pytest run, not a rejected edit.
- **Haiku 4.5 does not take the one-edit path.** After one rejected edit it falls back to one definition per `edit` call, then runs `check` and `src` to confirm (a1: 25 calls, 13 of them single-definition edits). It also ignored rules the spec states. On Python it takes 8 to 16 calls. The spec-only evaluation (`bench/eval/`) already showed Haiku writing much less valid SSPUR than Sonnet and Opus; here that cost multiplies through the agent loop.
- **Data-format tasks are the weak spot for every model.** a9 (regex, JSON) and b8 (JSON merge) are SSPUR's worst tasks for Sonnet and Opus too. Python's `re`, `json` and dicts are known to the model; in SSPUR the agent must learn regex and JSON APIs and string escaping from a 1.8k-token spec, and b8's Python start uses plain dicts while SSPUR has an explicit JSON sum type.
- **Sandbox friction is unchanged.** Opus a8 and the Sonnet b8 rerun each lost a call to a refused heredoc, then wrote a file and ran `edit --test FILE`. Haiku a9 also ran `find .` from the worktree root once (outside its directory; it found nothing relevant), and some agents wrote scratch files in `/tmp`.
- **Compiler issues found:** the native build fails on b8's recursive JSON type (`field has incomplete type 'L_T2_Z_S_J'`) and falls back to the interpreter; the non-exhaustive-match hint suggests `JBool{..}`, which is not valid pattern syntax; `q list` puts every test on one line.

### Caveats

- One run per cell. Python's own totals moved by up to 24% between identical runs before, and Haiku varies far more (a6: 61 calls, then 8).
- The new tasks have neutral sources but were written by the same model family as the language, and one model (Sonnet) ran them. The scale task is one task, generated and repetitive (44 copies of one domain template), which makes grep unusually effective for both sides.
- The spec-fix totals mix rerun cells with first-run cells, and the rerun cells were chosen because they did badly, so they had room to improve on a second try.
- Only SSPUR cells were rerun; Python cells were run once per model.
- Totals include the harness's per-call context, which differs by model (19.1k for Haiku, about 26k for Sonnet and Opus); the net and tool I/O columns remove it.

## Results, run 4

Run 4 re-ran the same 16 cells on the CLI after Phases 4 to 7 (effects and handlers, sys and bare profiles, threads, the standard library, GPU, deploy). Between run 3 and run 4 the agent spec had been rewritten to cover those features while staying under 1.8k tokens (1,493 to 1,799).

### Before the spec fix

Run: `runs/2026-10-04-sonnet-r4/`.

| Task | Hidden tests SSPUR / Py | API calls SSPUR / Py | Total tokens SSPUR | Total tokens Py | SSPUR / Py | Net of fixed overhead SSPUR / Py | Tool I/O SSPUR / Py |
|---|---|---|---|---|---|---|---|
| a1_inventory | 12/12 / 12/12 | 3 / 4 | 96,943 | 127,502 | 0.76x | 19,693 / 24,502 | 3,130 / 2,517 |
| a2_wordstats | 11/11 / 11/11 | 4 / 3 | 133,842 | 91,171 | 1.47x | 30,842 / 13,921 | 3,930 / 1,458 |
| a3_bank | 13/13 / 13/13 | 4 / 5 | 137,753 | 163,080 | 0.84x | 34,753 / 34,330 | 4,745 / 3,044 |
| a4_calc | 13/13 / 13/13 | 3 / 5 | 98,638 | 162,871 | 0.61x | 21,388 / 34,121 | 3,755 / 3,141 |
| a5_orders | 11/11 / 11/11 | 3 / 7 | 96,962 | 230,852 | 0.42x | 19,712 / 50,602 | 3,139 / 3,387 |
| a6_todo | 11/11 / 11/11 | 5 / 5 | 174,567 | 160,083 | 1.09x | 45,817 / 31,333 | 5,008 / 2,602 |
| a7_grades | 11/11 / 11/11 | 3 / 5 | 96,327 | 157,553 | 0.61x | 19,077 / 28,803 | 2,881 / 2,096 |
| a8_config | 13/13 / 13/13 | 17 / 4 | 685,216 | 128,538 | 5.33x | 247,466 / 25,538 | 8,716 / 3,173 |
| **Total** | 95/95 / 95/95 | 42 / 38 | **1,520,248** | **1,221,650** | **1.24x** | 438,748 / 243,150 (1.80x) | 35,304 / 21,418 (1.65x) |

Median per task 0.80x. Correctness held; the cost went up on four tasks. From the transcripts:

- a8 (17 calls): the sandbox refused 8 of its `edit -e '...'` commands (most had `#` or `;` inside the quoted argument, a few had neither). The compressed spec no longer said that a file works too, so the agent split the work into a dozen small edits, at one point adding a helper `fn sc() -> Str = ";"` to avoid typing `;`. Each workaround also caused a type error.
- a3: called `.length` on a list. The spec's List line had dropped `len`, `is_empty`, `any`, `all`, `contains`, `take`, `drop`, `reverse`, `push`, `concat`, `zip` and `enumerate`.
- a2: ran `spec --full | grep stable` because "sorts are stable" and the `counts` order had been dropped, then wrote the rest in one edit.
- a6: wrote `catch complete(...)` with Bool arms in tests. The example `catch f(x) == [] | ... | _ => false` had been dropped from the spec, so the checker rejected six tests (`expected List[Task], found Bool`). Its first command, a heredoc into `/tmp`, was also refused.
- a1, a4, a5 and a7 used the minimum 3 calls. a4 no longer hits the checker slowdown found in run 3 (that was fixed).

### Spec fix

`docs/agent/agent-spec.md`, 1,799 to 1,782 cl100k tokens:
- Restored the `catch` test in the example, and said that the arms have the type of the caught expression, so a test compares inside the `catch`.
- Restored the core List and Str methods, "sorts are stable", the `counts` order, `is_none`, `values`, `abs`, `.str`, `min`/`max`, the `_`-in-nested-call rule, `if` without `else` for `raise`, and that a bare fn name is a value.
- CLI: all defs in one single-quoted argument, put every change in one edit, errors print as `def:line:col`, `--test` runs every test, and if the shell refuses the command, write the defs to a file with the file tool and run `./sspur edit --test FILE`.
- To pay for it, the C, sys, bare and GPU section became one line of syntax plus a pointer to `spec --full`, and the rarer builtins (bit ops, wrapping arithmetic, random numbers, dates beyond `date`, heaps, lazy views, file and process details) were cut. Agents that need them read `spec --full`.

An intermediate version (`10d8e0f`) fixed a2 and a3 (3 calls each) but not a6 (6 calls: it again omitted `== []` in the catch tests, and read "or a file name" as `-e FILE`). `86d3611` spelled both out; a2, a3, a6 and a8 were then rerun on it (`runs/2026-10-04-sonnet-r4-spec-v3/`, with `spec_v2_tokens.json` for the intermediate cells).

### After the spec fix

Run: `runs/2026-10-04-sonnet-r4-spec-v3/`. a2, a3, a6 and a8 are new SSPUR cells; a1, a4, a5 and a7 and all Python cells are from the run above.

| Task | Hidden tests SSPUR / Py | API calls SSPUR / Py | Total tokens SSPUR | Total tokens Py | SSPUR / Py | Net of fixed overhead SSPUR / Py | Tool I/O SSPUR / Py |
|---|---|---|---|---|---|---|---|
| a1_inventory | 12/12 / 12/12 | 3 / 4 | 96,943 | 127,502 | 0.76x | 19,693 / 24,502 | 3,130 / 2,517 |
| a2_wordstats | 11/11 / 11/11 | 3 / 3 | 96,428 | 91,171 | 1.06x | 19,178 / 13,921 | 2,897 / 1,458 |
| a3_bank | 13/13 / 13/13 | 3 / 5 | 98,834 | 163,080 | 0.61x | 21,584 / 34,330 | 3,724 / 3,044 |
| a4_calc | 13/13 / 13/13 | 3 / 5 | 98,638 | 162,871 | 0.61x | 21,388 / 34,121 | 3,755 / 3,141 |
| a5_orders | 11/11 / 11/11 | 3 / 7 | 96,962 | 230,852 | 0.42x | 19,712 / 50,602 | 3,139 / 3,387 |
| a6_todo | 11/11 / 11/11 | 3 / 5 | 97,128 | 160,083 | 0.61x | 19,878 / 31,333 | 3,124 / 2,602 |
| a7_grades | 11/11 / 11/11 | 3 / 5 | 96,327 | 157,553 | 0.61x | 19,077 / 28,803 | 2,881 / 2,096 |
| a8_config | 13/13 / 13/13 | 5 / 4 | 173,558 | 128,538 | 1.35x | 44,808 / 25,538 | 4,483 / 3,173 |
| **Total** | 95/95 / 95/95 | 26 / 38 | **854,818** | **1,221,650** | **0.70x** | 185,318 / 243,150 (0.76x) | 27,133 / 21,418 (1.27x) |

- Seven of eight SSPUR cells used the minimum 3 calls. a8 still lost two calls to the sandbox: its first command (a heredoc) was refused, then it wrote the file with the Write tool and passed it to `edit --test`, which applied everything in one go.
- Compared with run 3 (0.59x, median 0.60x): the median is the same, and the total is higher because a8 is 1.35x instead of 1.05x and Python's own total was lower this time (1.22M against 1.48M; Python a3 and a6 took 5 calls each instead of 7 and 9). SSPUR's total, 855k, is 2% below run 3's 871k.
- SSPUR agents read about 2.3k tokens per task here (the spec plus the code), against 2.0k in run 3.

### 9th task (new, not in the totals)

Runs: `runs/2026-10-04-sonnet-a9-new/` (spec at `86d3611`) and `runs/2026-10-04-sonnet-a9-new-spec-v4/` (SSPUR rerun on `69e0d69`, same Python cell).

| Task | Hidden tests SSPUR / Py | API calls SSPUR / Py | Total tokens SSPUR | Total tokens Py | SSPUR / Py | Net of fixed overhead SSPUR / Py | Tool I/O SSPUR / Py |
|---|---|---|---|---|---|---|---|
| a9_events, spec `86d3611` | 17/17 / 17/17 | 7 / 4 | 257,108 | 130,948 | 1.96x | 76,858 / 27,948 | 6,071 / 3,452 |
| a9_events, spec `69e0d69` | 17/17 / 17/17 | 3 / 4 | 97,834 | 130,948 | 0.75x | 20,584 / 27,948 | 3,217 / 3,452 |

- First run: a heredoc was refused, then `regex(p).get` was rejected because `Res.get` performs `fail[E]`, which the spec did not say (it listed `get` for `Res` next to `Opt`'s trapping `get`). The agent also tried a Python one-liner to patch its edit file, which failed on the locale. The spec also had no way to read regex groups.
- The fix says that `Res.get` raises its error, adds `Res.or(d)`, and lists `captures(s)`. The rerun used the minimum 3 calls.
- Neither SSPUR agent used an effect handler for `skipped`; both filtered with `parse_line`, as the Python agent did. So this task exercises regex, JSON and `Res`, not handlers.
- Fairness: both sides got the same task text and equivalent interfaces and hidden tests. Python's `re`, `json` and dataclasses are well known to the model, while SSPUR's regex and JSON APIs are learned from the spec. One run per side.

## Results, run 3

Run: `runs/2026-10-03-sonnet-edit-v2/` (`scores.json`, `tokens.json`).

| Task | Hidden tests SSPUR / Py | API calls SSPUR / Py | Total tokens SSPUR | Total tokens Py | SSPUR / Py | Net of fixed overhead SSPUR / Py | Tool I/O SSPUR / Py |
|---|---|---|---|---|---|---|---|
| a1_inventory | 12/12 / 12/12 | 3 / 5 | 94,271 | 157,379 | 0.60x | 17,021 / 28,629 | 2,662 / 2,412 |
| a2_wordstats | 11/11 / 11/11 | 3 / 5 | 94,295 | 156,807 | 0.60x | 17,045 / 28,057 | 2,580 / 2,384 |
| a3_bank | 13/13 / 13/13 | 3 / 7 | 96,320 | 235,196 | 0.41x | 19,070 / 54,946 | 3,308 / 4,462 |
| a4_calc | 13/13 / 13/13 | 4 / 5 | 134,360 | 161,331 | 0.83x | 31,360 / 32,581 | 3,672 / 3,132 |
| a5_orders | 11/11 / 11/11 | 3 / 6 | 94,664 | 191,442 | 0.49x | 17,414 / 36,942 | 2,777 / 2,819 |
| a6_todo | 11/11 / 11/11 | 4 / 9 | 131,470 | 300,396 | 0.44x | 28,470 / 68,646 | 3,513 / 3,929 |
| a7_grades | 11/11 / 11/11 | 3 / 5 | 94,009 | 155,365 | 0.61x | 16,759 / 26,615 | 2,527 / 1,912 |
| a8_config | 13/13 / 13/13 | 4 / 4 | 132,077 | 125,702 | 1.05x | 29,077 / 22,702 | 3,701 / 2,838 |
| **Total** | 95/95 / 95/95 | 27 / 46 | **871,466** | **1,483,618** | **0.59x** | 176,216 / 299,118 (0.59x) | 24,740 / 23,888 (1.04x) |

- Five of eight SSPUR agents used the minimum of 3 calls: one `spec && src`, one `edit --test` with every change and new test, then DONE. No agent read `.sspur/` or created `.ssp` files.
- The extra calls: a4's edit took over 2 minutes because the checker is slow on one of its tests (below), so the harness moved it to the background and the agent spent a call waiting. a5 wrote one wrong test and fixed it in a second edit. a6 hit `_` binding inside a nested call (`sort_by((pri_rank(_.pri), _.id))`) and fixed it in a second edit, even though the spec warns about it. a8's `-e` was refused once by the sandbox.
- SSPUR agents wrote less (8.4k tokens against 17.8k): one edit with whole definitions, against Python's several targeted edits plus test runs. They read more (16.3k against 6.1k), and 12k of that is the spec, read once per task.
- Python's calls varied from run to run on the same prompts (40, 38, 46). In run 3, a6 took 9 calls and a3 took 7.

## Results, run 2

Run: `runs/2026-10-03-sonnet-edit-v1/`.

| Task | Hidden tests SSPUR / Py | API calls SSPUR / Py | Total tokens SSPUR | Total tokens Py | SSPUR / Py | Net of fixed overhead SSPUR / Py | Tool I/O SSPUR / Py |
|---|---|---|---|---|---|---|---|
| a1_inventory | 12/12 / 12/12 | 7 / 5 | 241,684 | 156,963 | 1.54x | 61,434 / 28,213 | 4,405 / 2,347 |
| a2_wordstats | 11/11 / 11/11 | 3 / 3 | 93,977 | 89,857 | 1.05x | 16,727 / 12,607 | 2,465 / 1,363 |
| a3_bank | 13/13 / 13/13 | 5 / 5 | 171,001 | 160,988 | 1.06x | 42,251 / 32,238 | 4,615 / 2,884 |
| a4_calc | 13/13 / 13/13 | 4 / 5 | 135,359 | 161,390 | 0.84x | 32,359 / 32,640 | 4,888 / 3,200 |
| a5_orders | 11/11 / 11/11 | 5 / 5 | 166,581 | 157,342 | 1.06x | 37,831 / 28,592 | 3,362 / 2,561 |
| a6_todo | 11/11 / 11/11 | 6 / 6 | 206,052 | 190,350 | 1.08x | 51,552 / 35,850 | 3,903 / 2,533 |
| a7_grades | 11/11 / 11/11 | 5 / 5 | 166,598 | 155,419 | 1.07x | 37,848 / 26,669 | 3,376 / 2,014 |
| a8_config | 13/13 / 13/13 | 7 / 4 | 245,222 | 126,382 | 1.94x | 64,972 / 23,382 | 5,084 / 2,984 |
| **Total** | 95/95 / 95/95 | 42 / 38 | **1,426,474** | **1,198,691** | **1.19x** | 344,974 / 220,191 (1.57x) | 32,098 / 19,886 (1.61x) |

Median per task 1.07x. The gap to run 3 is almost entirely the heredoc refusals: a1 and a8 were refused twice each and also re-ran `check` or `test` while recovering.

## Remaining gap and open issues

- Run 10: the spec is now a 777-token core, edits go through `change.ssp`, and unambiguous foreign spellings are stored in canonical form. What separates SSPUR from TypeScript on the small tasks is about 1k tokens of spec per call and one extra call in about a third of the cells, mostly to fix an agent's own test.
- Before run 10, the spec was the largest fixed cost: 1.6k tokens (1.8k before run 8) read once and carried through every later call. Since run 8 it no longer costs a call of its own (`sspur start`). Putting it in the MCP server instructions does not work in Claude Code, which cuts them at 2,048 characters; one agent doing several tasks would pay it once.
- The spec budget is now tight. Run 4 showed that cutting the basics to make room for new features costs more than it saves: each missing rule cost a task one to fourteen extra calls. Any future spec change should be checked against these tasks before it lands.
- The checker slowdown on nested constructor literals found in run 3 is fixed; a4 ran in the minimum 3 calls in run 4.
- `_` in a nested call binds to the inner call. That is the documented semantics, but agents keep writing `sort_by((f(_.x), _.y))`; the error (`expected Priority, found Task -> Priority`) could carry a hint.
- Sandbox heuristics still refuse some commands (`#`, `;` and heredocs, more often in run 4's worktree sandbox). With the spec's file fallback, agents recover in one or two extra calls; `edit --test FILE` on a file the agent wrote is the robust path.

## Results, run 1

Run: `runs/2026-10-03-sonnet/` (`scores.json`, `tokens.json`).

| Task | Hidden tests SSPUR / Py | API calls SSPUR / Py | Total tokens SSPUR | Total tokens Py | SSPUR / Py | Net of fixed overhead SSPUR / Py | Tool I/O SSPUR / Py |
|---|---|---|---|---|---|---|---|
| a1_inventory | 12/12 / 12/12 | 7 / 5 | 262,075 | 156,930 | 1.67x | 81,825 / 28,180 | 8,198 / 2,325 |
| a2_wordstats | 11/11 / 11/11 | 7 / 3 | 264,212 | 89,853 | 2.94x | 83,962 / 12,603 | 8,566 / 1,354 |
| a3_bank | 13/13 / 13/13 | 7 / 6 | 275,447 | 193,270 | 1.43x | 95,197 / 38,770 | 10,926 / 2,813 |
| a4_calc | 13/13 / 13/13 | 7 / 5 | 272,969 | 160,731 | 1.70x | 92,719 / 31,981 | 10,012 / 2,943 |
| a5_orders | 11/11 / 11/11 | 8 / 6 | 307,503 | 194,204 | 1.58x | 101,503 / 39,704 | 8,081 / 3,639 |
| a6_todo | 11/11 / 11/11 | 9 / 5 | 346,809 | 158,089 | 2.19x | 115,059 / 29,339 | 9,578 / 2,566 |
| a7_grades | 11/11 / 11/11 | 8 / 6 | 300,977 | 187,044 | 1.61x | 94,977 / 32,544 | 8,806 / 1,975 |
| a8_config | 13/13 / 13/13 | 11 / 4 | 451,660 | 126,117 | 3.58x | 168,410 / 23,117 | 11,570 / 2,888 |
| **Total** | 95/95 / 95/95 | 64 / 40 | **2,481,652** | **1,266,238** | **1.96x** | 833,652 / 236,238 (3.53x) | 75,737 / 20,503 (3.69x) |

Pass rate: SSPUR 8/8 tasks (95/95 hidden tests), Python 8/8 (95/95). Only one SSPUR transaction was rejected by the checker (a8, a type error, fixed in one retry).

### Where run 1's extra tokens went

The SSPUR runs used 1.22M more tokens than the Python runs. Roughly:

| Cause | Tokens | Share |
|---|---|---|
| 24 more API calls, each re-reading the 25.75k fixed context | 618k | 51% |
| The reference (3,676 tokens) read once, then carried in the context of the 56 later calls | 206k | 17% |
| Larger tool output carried forward: `q list` and `q body` print pretty JSON with hashes (12.5k read in total), and `apply` prints every changed hash (12.1k); Python agents read `app.py` once (5.6k read in total, all tasks) | about 390k | 32% |

- **More calls.** The shortest SSPUR path is spec, query, write `tx.json`, apply, test. The shortest Python path is read, edit, test. Agents followed these paths closely: SSPUR averaged 8 calls and Python 5.
- **Writing is not cheaper either.** SSPUR agents wrote 20.6k tokens against Python's 14.9k (1.38x). A transaction resends the whole definition in a JSON string with escaped quotes and newlines, while Python agents sent targeted edits or short replace scripts.
- **The language itself is slightly smaller.** Counted with cl100k, the SSPUR starting code is 0.88x the Python code and the reference solutions are 0.92x. But a whole codebase here is 300 to 950 tokens, smaller than the reference agents must read first, so the per-call fixed costs outweigh the savings.
- **The query API wasn't used for targeting.** No agent used `pack`. Every agent ran `q list` and then `q body` on everything, which reads the whole codebase anyway. At this size that's rational.
- Environment friction hit both sides about equally. The sandbox refused heredoc writes 10 times on SSPUR runs (agents then used the Write tool) and twice on Python runs. Python agents also hit a locale crash 7 times when they ran `python3` without the documented `LC_ALL`.

### What would change the result (written after run 1; runs 2 and 3 tested the first point)

These were hypotheses:
- Cut per-call overhead. Compact text output for `q list`, `q body`, and `apply` (the hashes are rarely needed), and an `apply` that accepts ops inline or on stdin so no transaction file is needed. Together these would make the SSPUR path as short as Python's (query, apply, test).
- Amortize the reference. One agent doing many tasks, or the reference in a cached system prompt, pays its 3.7k once instead of once per task.
- Larger codebases. `pack` returns a slice of fixed size, while reading a Python file grows with the file. The crossover needs codebases where an agent would otherwise read thousands of irrelevant tokens, which these 8 tasks don't have.

## Caveats

- One run per cell per run, 8 tasks, one model. Python's own total moved by up to 24% between runs with identical prompts, so treat any single ratio as about +-20%. Run 3's 0.59x and run 4's fixed 0.70x are outside that band, and both SSPUR totals are below all four Python totals. Run 4's fixed total mixes 4 cells from before the fix (which were already at the 3-call minimum) with 4 rerun cells; the rerun cells were chosen because they regressed, so they had a chance to come out cheaper on a second try.
- The SSPUR interface was revised between runs after reading run 1 and run 2 transcripts, while the Python side stayed fixed. The tasks, hidden tests, and Python prompt never changed, except that run 4 put the pytest command (and `./sspur`) under a timeout wrapper on both sides.
- Totals include the Claude Code harness system prompt that every call re-reads. That overhead favors the side with fewer calls, which is a real cost for any agent using this harness. The net and tool I/O columns remove it. On those, SSPUR was 3.5x and 3.7x in run 1, 0.59x and 1.04x in run 3, and 0.76x and 1.27x in run 4 after the fix.
- Output tokens are estimated with cl100k, so they're approximate. They don't affect the conclusion.
- Python is a language the model already knows. SSPUR is learned from the reference in every run (214 lines in run 1, the 1.5k-token spec in runs 2 and 3, 1.8k in run 4, 1.6k since run 8). That cost is part of what the criterion measures. Run 9 shows it is also what the result turns on: against TypeScript and Go, which the model knows just as well as Python and which take the same 3 calls when nothing goes wrong, SSPUR has no token advantage on these small tasks.
- The tasks, references, and hidden tests were written by the language's designer (the same model family). The tasks were written before any agent run and weren't changed afterwards.
- Two compiler bugs were found and fixed while writing the reference solutions, before the runs (commits `455851d`, `60d188d`): a checker stack overflow on field access of a value whose type is unknown, such as an undefined function, and missing `cmp_*_p` helpers that broke native `List.sort` on `Str`, `Int`, and `F64` and dumped a long clang error before falling back to the interpreter.

## Reproduce

```
cargo build --release
python3 bench/agent/score.py ref
python3 bench/agent/setup.py /tmp/sspur-agent-r1     # writes prompts.json (--v1 for run 1's SSPUR instructions, --tmo for run 4's timeouts, --new for a9 only)
# run one subagent per prompt; map "<lang>/<task>" to its transcript path in agents.json
python3 bench/agent/score.py run /tmp/sspur-agent-r1
python3 bench/agent/tokens.py /tmp/sspur-agent-r1/agents.json /tmp/sspur-agent-r1/tokens.json
python3 bench/agent/report.py /tmp/sspur-agent-r1
# run 5: the new tasks and the large codebase
python3 bench/agent/setup.py /tmp/sspur-agent-b --tmo --b
python3 bench/agent/tasks/s1_shop/score_scale.py ref
python3 bench/agent/tasks/s1_shop/setup_scale.py /tmp/sspur-agent-s
python3 bench/agent/tasks/s1_shop/score_scale.py ssp FINAL.ssp   # or: score_scale.py py WORK_DIR
BASE=19100 python3 bench/agent/tokens.py agents.json tokens.json   # BASE = the model's no-op context, for the net column
# run 9: TypeScript and Go
(cd bench/agent/xlang && npm install typescript@5 @types/node@20)   # once, offline after that
python3 bench/agent/xlang/xlang.py a1_inventory b1_lru              # validate ports: ref passes, start fails, visible tests pass
python3 bench/agent/setup.py /tmp/sspur-agent-r9 --tmo --langs=sspur,ts,go
python3 bench/agent/tasks/s1_shop/setup_scale.py /tmp/sspur-agent-r9s --langs=ts,go
python3 bench/agent/xlang/record.py RUN_DIR ts/a1_inventory AGENT_ID   # one per finished cell
python3 bench/agent/xlang/report4.py        # the four-language table and the ratios
python3 bench/agent/xlang/sizes.py S1_GEN   # source size per language
python3 bench/agent/xlang/friction.py RUN_DIR
python3 bench/agent/xlang/trace.py RUN_DIR go/s1_shop   # per-call trace of one cell
# run 10: the change.ssp edit line is the default (--r9 gives runs 8 and 9's)
python3 bench/agent/setup.py /tmp/sspur-agent-r10 --tmo --langs=sspur        # and --b; setup_scale.py for s1
python3 bench/agent/xlang/report5.py runs/2026-10-09-r10   # run 10 SSPUR against run 9's Python, TypeScript and Go
# run 11: the shorter prompt is the default (--r10 gives run 10's); compiler frozen at ef439aa
python3 bench/agent/xlang/report5.py runs/2026-10-09-r11
```

`tokens.py` needs `tiktoken`. Set `LC_ALL=en_US.UTF-8` if Python fails with a locale error.
