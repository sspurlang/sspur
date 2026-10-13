# Changelog

All notable changes to SSPUR are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/). While SSPUR is below 1.0, minor releases can change the syntax and the standard library.

## [Unreleased]

## [0.8.1] - 2026-10-13

### Added
- Five more large-codebase tasks in the agent benchmark (s2 to s6), on the shop codebase and two new generated codebases of about 1,000 definitions (library and fleet), in SSPUR, Python, TypeScript and Go, with hidden tests and reference solutions. `bench/agent/large/` sets them up and scores them, and `report5.py --large` reports them.
- Benchmark runs L1 to L3 on the six large tasks. Sonnet 5.5 used 0.68x Python's tokens, 0.59x TypeScript's and 0.58x Go's; Opus 5.5 0.85x TypeScript's and Haiku 5.5 0.68x, cheaper than TypeScript on every task for all three models, with every hidden test passing.

## [0.8.0] - 2026-10-13

### Added
- `while e is p` loops: `while q.pop_front is some((x, rest)) and x > 0` binds `x` and `rest` in the body, matches again before each pass and stops when the pattern doesn't match. Tests join with `and` as they do after `if`, and the interpreter and native code run it the same way. A pattern that always matches warns `W_ARM_UNREACHABLE`, and the body must be `Unit` like any `while` body.

- A Zed extension in `packaging/zed/`, attached to every release as `sspur-zed-vX.Y.Z.zip` (and in `SHA256SUMS`) so it installs with **zed: install dev extension** and no registry: a Tree-sitter grammar for SSPUR (`packaging/zed/tree-sitter-sspur/`) for highlighting, brackets, indentation and the outline of `fn`, `type`, `test`, `trait` and `impl` definitions, and `sspur lsp` found on `PATH` or set in Zed's settings. The grammar reads every `.ssp` file in the repository without an error node. `packaging/zed/package.sh` pins the grammar to a commit, the release's or a local checkout's (`file://`).

### Fixed
- A signature with more than one effect clause, `fn calc(s: Str) -> Int ! fail[CalcErr] ! div`, failed with `E_PARSE_EXPECTED expected '=', found '!'`. It is now accepted (also in function types) and stored as `! fail[CalcErr], div`, with the note `stored as: ! a ! b -> ! a, b`.
- `sspur edit` with `remove subtotal` and a new `fn gross` whose text is subtotal's with only the name changed was rejected with `E_UNKNOWN_NAME` whenever a stored definition outside the edit still called `subtotal`. The pair is now read as `rename subtotal gross` (plus a replace of `gross` with the text given), so stored callers follow it and the output shows `subtotal->gross`, as for an explicit rename. It pairs only when the removed definition has exactly one such new definition and that definition matches no other removal; several pairs in one edit (a function and its test) are matched together. When nothing pairs, the `E_UNKNOWN_NAME` error for a caller of a removed name now has a hint that names the caller and suggests `rename X NEW` or updating or removing the caller.
- Recursion in the playground went only about 100 calls deep in Safari, 500 in Chrome and 1,000 in Firefox before the page reported a stack overflow. It now goes 20,000 calls deep in all three, as `sspur run --interp` does, and the call after that is the same `stack overflow in f` error. The build runs Binaryen's asyncify pass, so once the interpreter's frames fill a segment of the browser's stack (measured when the page starts, smallest in Safari), the next call saves the frames under it to memory and runs on an empty stack. The interpreter's frames are smaller and the playground's stack in WebAssembly memory is 32 MiB instead of 16, so calls with large bodies or through lambdas reach 10,000 to 15,000 before the same error. The WebAssembly file is 37 KiB larger gzipped (981 KiB), loops run about 15% slower in the playground and calls about 10% faster. A build without `wasm-opt` behaves as before.
- A string written with escaped quotes inside an interpolation, `"cycle {path.join(\" -> \")}"`, printed the braces as text; it now means the same as `"cycle {path.join(" -> ")}"`.
- An arm of `catch do` or `match ... do` written at the block's own indent (`catch do`, the lines, then `| _ => false` under them) was read as a statement and failed with `E_PARSE_EXPR`; a `|` line now ends the block and starts the arms.
- A test written `catch e | Err => false`, which expects `e` to finish without an error, was stored as `catch do (_ = e) false | ..` and so always failed; when every arm is `false` it is now stored as `catch do (_ = e) true | ..`, and a failing run shows the error that was raised.

### Changed
- Hole filling reaches longer predicates: a lambda for `filter`, `any`, `all` or `find` can now have up to 5 nodes, as in `xs.filter(_ % 2 == 0)`, where other lambda bodies still stop at 3. The longer bodies are only built once a candidate has room for them, and only from parts that can still end in a `Bool`, so holes that settle on something small cost the same as before. An equality or comparison with a literal is written with the literal on the right, and `x - -y` counts as unlikely. The benchmark goes from 41 to 42 of 51 at 5000 ms (`evens`); at 200 ms it stays at 41, because that case needs about 400 ms.
- A parse error at a `,` right after a `)` that closes a parenthesized record update with several fields, as in `then (g with rooms := rs, inventory := inv), "Taken.")`, now has the hint that the `)` closes more than it opens and that a tuple holding a record update is written `((r with a := x, b := y), other)`. Before, `E_PARSE_TRAILING unexpected ',' after definition` gave no clue that the tuple was one `(` short.
- The core agent spec is 584 tokens again (0.7.1: 698): the pattern line is shorter, and block lambdas and traits are in `sspur spec --more` only.

## [0.7.1] - 2026-10-12

### Fixed
- A binding broken after its `=` (`(_, total) =` with the value on the next line, which pressing Enter in an editor produces) failed with `E_PARSE_EXPR`; a line ending in `=` now continues on the next line.
- A sum type can start with `|`, so the variants can each go on their own line under `type S =`.

## [0.7.0] - 2026-10-12

### Added
- `e is p` is a `Bool` expression anywhere (ADR 0032): `xs.filter(_ is some(_))`, `ok = r is ok(_)`, `test t = parse("x") is err(_)`, `o is some(v) and v > 0`. The names a test binds are in scope in the `and ...` after it, and an `if` condition can join several tests, `if a is Circle{r} and b is Circle{r: r2} then r + r2 else 0.0`, with the names of all of them in the `then` branch. A test that binds a name nothing can use is `E_IS_BINDS`, and one whose pattern can't match the value written there (`none is some(_)`) warns `W_NEVER_MATCHES`. `is` keeps the precedence it had after `if`: it takes everything before it, so `a and b is p` is still `E_PARSE_IS`, and so is a test used as an operand without parentheses, `e is p == x`.
- Guard clauses (ADR 0032): `some(user) = find(id) else raise NotFound{id}` in a block binds the pattern for the rest of the block, or ends the block with the `else` value when it doesn't match, so `else 0` returns 0 from a function body, `else ()` skips to the next element in a loop, and `else return err("bad")` works too. Rust's `let p = e else { ... }` and Swift's `guard let x = e else { ... }` are stored in this form. Record destructuring, `Point{x, y} = p`, now works outside `profile sys` as well, and `E_REFUTABLE_LET` suggests adding `else`.
- Hole filling proposes `s is Circle` (a variant of a sum) and `r is ok(_)` for `Bool` holes; the benchmark stays at 41 of 51.
- Long lines (ADR 0032): a line that ends in a binary operator (`and`, `+`, `==`, `,`, ...) continues on the next line, and so does a deeper-indented line that starts with `.name` or an operator, so `= xs` then `.map(_ * 2)` and `.sum` on their own lines is one expression. `sspur fmt` joins them back into one line.
- Pressing Enter after a line that ends in an operator, an open bracket, `=>`, `=` or `do` indents the next line one level, in the playground, in VS Code (`onEnterRules`) and through the language server (`textDocument/onTypeFormatting`, which LSP4IJ uses in JetBrains IDEs). A `.method` chain stays aligned, and the line after a continued one goes back to where its statement started.
- `sspur lsp`, a language server over stdio (ADR 0031). It publishes the checker's diagnostics as you type (checked off the main loop, a short pause after the last edit), then the candidates for each typed hole, and answers completion with signatures and the standard library's docs, hover, code actions with the compiler's fixes and the hole fills, formatting, go to definition, references, document highlights, rename, the outline and semantic tokens in the playground's classes. The command `sspur.fill` fills every hole that has a passing candidate. A file in a package is checked with its dependencies, and their names show as `pkg.name`; `pkg.` completes their public definitions.
- Signature help in the language server and the playground: after `(` or `,` the signature of the call shows with the argument being typed highlighted and its one-line doc, for the file's functions (the comment above one is its doc), effect operations, builtins, methods on a typed receiver and `pkg.f`, also while the call is half-typed and the file doesn't parse.
- Inlay hints in the language server, and in the playground behind a toggle: the inferred type after `let`, guard, `for` and `var` bindings whose type isn't written (`total = items.map(_.price * _.qty).sum` shows `total: Int`, `some(user) = find(id) else ...` shows `user: User`) and after lambda parameters, and the parameter name before a literal argument (`scale(3, total, true)` shows `n: 3, strict: true`). Literals and records built by name get none. `initializationOptions.inlayHints` or the `sspur.inlayHints` setting turn each kind off, and the VS Code extension has a setting for each.
- A VS Code extension in `packaging/vscode/`, attached to every release as `sspur-vX.Y.Z.vsix` so it installs with no marketplace account: a TextMate grammar that highlights like the playground, the language configuration, the language server, and the commands Run File, Test File and Fill Holes. It builds into a `.vsix` that also installs in Cursor, Windsurf and VSCodium.
- A template for LSP4IJ in `packaging/jetbrains/`, attached to every release as `sspur-lsp4ij-vX.Y.Z.zip`: in a JetBrains IDE with LSP4IJ, *Import template...* sets up the server in one step, with no account.
- [Editor setup](https://utkarshavardhana.github.io/sspur/get-started/editors.html): VS Code and its forks, Neovim, Vim, Helix, Emacs, Sublime Text, JetBrains IDEs through LSP4IJ, Kate and Eclipse, and what Zed, Visual Studio and Xcode lack.

### Changed
- Record destructuring, `Point{x, y} = p`, works in every profile, not only `profile sys`.
- The editor logic behind the playground moved from `crates/sspur-wasm` into a new crate, `sspur-ide`, which the language server uses too.
- Hole filling has more time per hole: 400 ms in `sspur check` and the playground, 600 ms in editors, where it runs in the background. Since it began counting the other candidates that fit, a classic hole such as a sum of field products took about 270 ms and was missed at the old 150 to 200 ms.

### Fixed
- The language server marks bindings and fields as read-only semantic tokens, so IntelliJ (through LSP4IJ) no longer underlines them as reassigned variables.

## [0.6.0] - 2026-10-11

### Added
- Hole filling (ADR 0030): for each typed hole, `sspur check` lists up to three complete expressions that fit, one line each, `?total: items.map(_.qty * _.price).sum (passes 3 tests, post)`, and `--json` gives them as `fill` fixes with `evidence` and `ok`. They come from a bottom-up search over the values in scope, literals, fields, constructors, the program's functions and trait methods, operators and the common methods of lists, strings, options and numbers (lambdas like `map(_.f * _.g)` included), limited to 7 nodes and 200 ms a hole (`--fill-ms N`, `--no-fill`). Candidates only use effects the function declares, and run against the tests and examples that reach the hole and against small and fuzzed inputs that check `post` and a refined result type. Expressions that agree on every observed input count once, which keeps the search fast. Files without holes are checked exactly as before. With weak tests the ranking uses a prior over what agents mean (every unused parameter used, no constant offsets or vacuous patterns, a `pre` or the empty input respected), constants from the tests, one-step `Opt` defaults and `with` updates, and the evidence says `N others also fit` when the tests don't decide; on the 51 holes of `bench/fill/holes.txt` the top candidate is the intended one for 41, up from 26.
- `sspur fill [file] [?hole|fn]` prints the best candidate for each hole and `--apply` writes it into the file (in parentheses where precedence needs them) or applies it to the codebase; with z3 installed a candidate whose `post` it proves says `post proved`.
- `sspur suggest [file] [fn]` proposes contracts from runs: the `post` clauses from a fixed set (`r >= 0`, `r.len <= xs.len`, `r == x`, sorted output, comparisons with each parameter) that held on every test-derived, small and fuzzed input, minus implied ones, and a `pre` such as `xs.len > 0` that rules out the inputs it traps on. Lines are ready to paste, marked `proved` (z3) or `held on N runs`; `--json` gives `refine` fixes and `--apply` adds them.
- The MCP server has `fill` and `suggest` tools, and its `check` shows the hole candidates. The playground's quick fix for a hole offers the checked candidates.
- The playground has an Input tab: Run passes its text to the program as stdin, for `read_line` and `read_lines`, and share links carry it. The From Python example reads its words from there.

- `SSPUR_FILL_MS` sets the time hole filling may spend per hole, for `check`, `fill` and the MCP tools.

### Changed
- The playground was redesigned for phones and desktops: a menu on phones, icons on every action, an examples menu that opens below its button, a copied-link toast instead of the link box, a collapsible output panel on phones, a resizable split on desktops, no layout shift while a program runs, no zoom when the editor is focused, and a creator credit. The docs header links to it.

### Fixed
- Concurrent native builds on Windows could fail to link (LNK1104) while another build published the same object file; outputs are now published without replacing a file that may be open.

## [0.5.0] - 2026-10-11

### Added
- Or-patterns (ADR 0029): `| Circle{r} | Ring{r} => r`, nested anywhere (`some(0 | 1)`, `(Red | Blue, _)`); every alternative binds the same names with the same types, or it is `E_PATTERN_OR_BINDS` with a hint naming the name or the two types. Rule cells accept them too.
- List patterns: `[]`, `[x]`, `[x, y]`, `[x, ..rest]`, `[..init, last]`, `[first, .., last]`, `[_, ..]`, with patterns as elements (`[ok(x), ..]`, `[(a, b), ..rest]`, `["GET", path]`). `rest` is a `List`; in native code it is a view of the matched list, so binding it allocates nothing. `[x, ...rest]` and `[x, *rest]` are stored as `[x, ..rest]`.
- `if e is p then a else b` tests one pattern, with an optional guard (`if o is some(v) and v > 0 then`), an optional `else` when the branch is `Unit`, and `else if e2 is q` chains. `if let p = e then` is accepted and stored as `if e is p then` (note `if let p = e -> if e is p`). `E_PARSE_IS` reports `if a and b is p`, with a hint.
- Exhaustiveness checking looks inside nested patterns, or-patterns and lists (lengths `[]` plus `[x, ..rest]` cover every list), and lists the missing cases as patterns (`some(none)`, `[_, _, ..]`, `(false, false)`). The new warning `W_ARM_UNREACHABLE` reports a `match` or `catch` arm, or an or-pattern alternative, that the arms above already cover, and an `if e is p` whose pattern always matches.
- All of it runs in the interpreter and in native code (strict native compiles every function), is printed canonically and hashed; the program generator emits list patterns, or-patterns and `if e is p` for differential fuzzing.
- A playground at [utkarshavardhana.github.io/sspur/play](https://utkarshavardhana.github.io/sspur/play/) runs the checker, formatter and interpreter in the browser through WebAssembly (`crates/sspur-wasm`), in Web Workers with a Stop button. It checks as you type, offers the checker's fix hints as quick fixes, completes names, fields and methods with their signatures, shows types on hover, and shares programs in the link. Files live in memory, `env` is empty, and `proc`, `extern fn` and native code report that they aren't available. The JIT, the C back end and FFI are behind the `jit` feature of `sspur-native` and the `native` feature of `sspur-eval`, on by default, so the interpreter builds for `wasm32-unknown-unknown`.
- The docs header links to the playground, and every SSPUR example in the docs has a Run button that opens it, or the whole snippet file it comes from, in the playground.
- A logo, used in the README, the docs, the favicon and the playground (`docs/assets/logo/`).
- A `NOTICE` file with authorship and the project name, included in every release archive.

## [0.4.1] - 2026-10-10

### Added
- Windows support (x86_64, and arm64 release builds). Native code compiles with LLVM's `clang` into a DLL that sspur loads: the runtime's GC heap reserves and commits with `VirtualAlloc`/`VirtualFree`, `par` and the parallel pool run on Win32 threads with SRW locks and condition variables, files go through the C runtime with UTF-8 paths, and `run_cmd` uses `CreateProcessW`. The Windows C is produced by rewriting the POSIX runtime, so the C generated on macOS and Linux is byte-identical to before. The interpreter's `extern` calls follow the Win64 calling convention, `export-c` writes a `.lib` (or a `.dll` and its import library) plus the header, `bind` maps `long` to 32 bits, and `sspur deploy local` builds its host with a small Winsock HTTP client instead of libcurl. The cache is `%LOCALAPPDATA%\sspur`.
- Releases ship `sspur-vX.Y.Z-x86_64-pc-windows-msvc.zip` and `aarch64-pc-windows-msvc.zip` with their checksums in `SHA256SUMS`, an `install.ps1` (`irm https://raw.githubusercontent.com/utkarshavardhana/sspur/main/install.ps1 | iex`) and a Scoop manifest; CI runs the build, strict native tests, clippy and the corpus check on `windows-latest`.

## [0.4.0] - 2026-10-10

### Added
- Traits (ADR 0027): `trait Shape` with indented method signatures (`= e` is a default body) and `impl Shape for Circle` with the methods, called as `c.area`, `c.area()` or `area(c)`; generic impls `impl[T: Show] Show for Box[T]`; a trait method's effect row bounds its impls. Bounds on type parameters, `fn max_of[T: Ord](xs: List[T])` and `[T: Eq + Show]`, are checked where the function is defined, and a call with a type that lacks an impl is an error at the call site that names the impl and how to add it. Operators are trait methods for user types: `Add Sub Mul Div` (`+ - * /`), `Neg` (unary `-`), `Eq` (`== !=`), `Ord` (`< <= > >=` through `cmp`) and `Index[K, V]` (`x[k]`); built-in types keep their semantics and overflow traps. `type P = {..} derive Eq, Ord, Show, Hash, Json` adds structural impls. Coherence is one impl per (trait, type), and an impl lives in its trait's or its type's package; `pub trait` and `pub impl` export them. Resolution is static: trait calls become calls of the impl, and bounded functions are specialized per type (`max_of__Int`) for the interpreter and native code alike, so the tiers agree, with every function native at `-O2` and `-O3`. New errors: `E_TRAIT_UNKNOWN E_TRAIT_ARGS E_TRAIT_PARAMS E_TRAIT_SELF E_TRAIT_MISSING E_TRAIT_AMBIGUOUS E_TRAIT_RECURSION E_IMPL_TARGET E_IMPL_ORPHAN E_IMPL_DUP E_IMPL_BUILTIN E_IMPL_EXTRA E_IMPL_MISSING E_IMPL_SIG E_IMPL_EFFECT E_DERIVE_UNKNOWN E_DERIVE_FIELD E_SELF E_PARSE_IMPL`, each with a fix hint; `E_OPERATOR` on a user type now suggests the impl or the derive. Codebases store impls as `impl Trait for Type` (`remove impl Trait for Type`). Suite programs `traits`, `constraints`, `operators` and `derive`; the program generator emits traits, impls and bounded generics.
- Block lambdas (ADR 0028): a lambda body can be `do` and a block indented past the line the lambda starts on, `xs.map(x => do` + lines + `y)`, with the existing captures (a captured `var` is shared), effects and both tiers; pure scalar ones fuse and vectorize in `map`/`filter`/`sum` pipelines like one-line lambdas. A `)` on its own line, `x =>` without `do`, `x => {` + block + `}` and a final `return e` are accepted and stored canonically; an early `return` inside a lambda stays `E_RETURN_IN_LAMBDA`, now with a hint. The formatter prints a lambda with a multi-line body as `x => do` and an indented block
- The agent benchmark covers TypeScript and Go: all 17 tasks are ported with matching hidden tests (`bench/agent/tasks`), so runs compare SSPUR with Python, TypeScript and Go under one harness.
- Trait methods as values: `xs.map(area)`, `xs.fold(0, add)` and `xs.sort_by(area)` resolve statically to the impl for the parameter type, in both tiers; where that type is not known yet the error hint shows `x => x.area`
- The program generator emits block lambdas (map, fold and fused range pipelines with captures, logs and match bodies)

### Fixed
- `sspur export-c` lists each linker flag once (Linux printed `-lm` twice).
- Native builds no longer fail with `incomplete type` when a list of tuples over a recursive sum type (for example `List[(Str, J)]` inside `J`) is generated before the sum: the list struct is now emitted ahead of its element definitions. Such programs fell back to the interpreter before. Found in agent bench run 11.

### Changed
- The repository tracks about half as many files: benchmark runs keep only their token and score summaries, the evaluation corpus is one `bench/eval/corpus.jsonl`, the 58 suite programs are 15 files by area with all 544 tests, and reject and fuzz cases are one table file per suite.
- Documentation site rebuilt along the lines of the TypeScript docs: a landing page of section cards, Get Started (five minutes, pages for TypeScript, Python, Go and Rust programmers, agents, installation), an eleven-page Handbook, Reference by area (standard library, effects, every error code with its fix, CLI, agent spec, JSON schemas), six Tutorials, Project Configuration, a Cheat Sheet, and Design and Performance. Every code block in the handbook and tutorials is a snippet file run by `tests/tutorial.rs`, which also checks that `docs/reference/errors.md` lists every diagnostic code in the compiler.
- Repository layout: `docs/` is the single mdBook source (the old `site/` merged into it, with `design/`, `reference/`, `agent/`, `adr/` and `snippets/` folders); the full reference that `sspur spec --full` prints is `docs/reference/language.md`; the JSON schemas moved to `docs/reference/schema/` and now describe what `apply` accepts and `check --json` prints; the Phase 1 token benchmark moved to `bench/tokens/`; the Claude Code plugin moved to `packaging/claude-code/` (the marketplace name and install commands are unchanged).
- `sspur edit` stores spellings from other languages that have one meaning in their SSPUR form and reports them on a `stored as:` line (at most three, then `and N more`), instead of rejecting the edit: `&& || !`, `elif`, `let`, `+=`, `Ctor{_}`, typed locals, `len(x)`, `.length`, `.toLowerCase` and similar method names, `None`/`Some`/`True`, `print`, `s.slice(a, b)` on `Str`, `.get` on a value that is not an `Opt`, `_` in a call that takes no function, `catch` of a non-`Bool` value in a test, and effects a function performs but does not declare. Other entry points still reject them, with the canonical spelling in the message (`N_FOREIGN`). Definition keywords (`queue`, `store`, `effect`, `pre`, `post`, ...) are identifiers elsewhere.
- Failing tests print the values of the comparison that failed (`left 7, right 8`), and a failed `pre` prints the parameters it mentions, in every tier.
- The agent spec is a core of about 600 tokens (was 1,631), with one line on traits; `sspur spec --more` prints the rest, including the builtins and a Traits section. It recommends writing edits to `change.ssp` with the agent's file tool and running `sspur edit --test change.ssp`.
- Agent benchmark run 10: 0.72x Python's tokens, 1.06x TypeScript's and 0.96x Go's on 17 cells with Sonnet 5.5 (run 9: 0.83x, 1.23x, 1.11x), 246/246 hidden tests.
- Agent benchmark run 11: 0.65x Python's tokens, 0.95x TypeScript's and 0.86x Go's in total, 0.59x, 1.01x and 1.00x at the median, 58 API calls against 88, 61 and 66, 246/246 hidden tests: parity with TypeScript and Go.

## [0.3.2] - 2026-10-07

### Fixed
- Deploy: CodeDeploy's role grants `cloudwatch:DescribeAlarms` on `*`. The action has no resource-level permissions, so the previous ARN list granted nothing and every canary failed on AWS (CloudFormation rolled back cleanly). Found by the first real canary run (ADR 0019).
- Deploy: `rollback.sh` lists in-flight deployments per deployment group, as the CodeDeploy API requires.

### Added
- Documentation site at https://utkarshavardhana.github.io/sspur/ (mdBook, published from `site/` by `.github/workflows/pages.yml`): a 10-minute tutorial whose every snippet is a file checked by `tests/tutorial.rs`, a guide for AI agents, the language and library reference, benchmarks, and the design records.
- `Ratio`, exact rationals over `BigInt`: `ratio(n, d)`, `ratio_big(n, d)`, `parse_ratio(s)`, always normalized, with `add sub mul div neg abs inv pow sign is_int floor ceil trunc round to_f64 to_dec`, value ordering for `<` and sorting, and `3/4` display. `to_f64` is correctly rounded and `round` is half to even (ADR 0018 decision 35).
- `Locale` values for en-US, en-GB, de-DE, fr-FR, ja-JP and hi-IN from `locale(tag)`, passed explicitly: `compare` and `sort` with UCA-style three-level collation (base letters, accents in DUCET mark order, case, then code points; NFC and NFD forms differ only at the last level), and `format_int`, `format_f64`, `format_dec`, `format_date`, `format_date_long` and `format_time` with each locale's separators, Indian digit grouping, date patterns, month names and am/pm markers. The rules are bundled in one Rust module that the C runtime's tables are generated from, so no host or ICU data is read (decisions 37 and 38, which also list what is not full DUCET).
- `copy_file`, `symlink`, `read_link`, `is_symlink`, `file_mode` and `set_mode` under `fs`, with `same file`, `not a symlink` and `mode out of range` errors.
- `tests/programs/std_collections.ssp` (the valarray section): `valarray` operations (elementwise arithmetic, broadcast, reductions, masks, indirect arrays, shifts, strided slices) are fused view and list pipelines in both tiers, documented as covered rather than added as a new type (decision 36). ADR 0018's coverage table is now 23 of 23 areas covered.
- `F64` in `profile bare` on aarch64-qemu (the FPU is enabled on every core and the exception entries save the FP registers) and thumbv7em-mps2 (AEABI software double precision, checked bit for bit against the host FPU on 3,000,000 operand pairs). `+ - * /`, comparisons, `Int.to_f64` and `abs sqrt floor ceil round trunc is_nan is_finite is_inf copysign` are allowed; other `F64` methods, `%` and `**` need libm and are `E_PROFILE_BARE`; riscv64-qemu has no FPU and fails at build time with a clear error. `tests/bare/floats.ssp` prints the same 13 results on both boards as the host interpreter (ADR 0017 decisions 12 to 15).
- Secondary cores on aarch64-qemu: `fn core_main(id: Int)` runs on cores started with `start_core(id)` (PSCI `CPU_ON`), `core_id()` reads `MPIDR_EL1`, and statics use atomic instructions when a module has `core_main`, with `E_STATIC_RACE` covering core code. `tests/bare/smp.ssp` adds from two cores and prints `total 200000 seen 3` (decisions 16 and 17). QEMU runs aarch64 kernels with `-smp 2`.
- `sspur start [NAME|PATTERN...]` reads the spec and the code in one call: it prints the agent spec, then the codebase, all of it when the source is at most 12,000 bytes, otherwise the counts per kind, `q pack` of the arguments that name a definition and `q find` of the others. The MCP server has a matching `start` tool (`names: "a,b,c"`), and its instructions tell the agent to call it first. `sspur spec [--full] [src] [QUERY TARGET...]` prints `src` and queries after the spec (`spec find 'a|b' pack A,B`).
- `q pack A,B,C` packs several definitions in one call, each one with the types and signatures it uses, its tests and its callers, and prints every definition only once (a target that is also another's caller is shown in full, not twice). This is what the agent benchmark's large-codebase task needed: the five definitions it has to change cost one call of 540 tokens instead of three rounds of queries.

### Changed
- `q pack` ends with `-- complete: every caller (N) and test (M) of NAME is above` when nothing was left out, so an agent does not spend a call re-checking callers (both run 8 s1_shop runs did).
- `q find` prints 25 signatures in full and `q grep` 12 definitions, then the names of the rest on one line (`-- 21 more: f11 f12 ...`), so a broad pattern on a large codebase no longer fills the context with near-identical matches. Definitions whose name is exactly the pattern come first. `--json` is unchanged.
- Agent benchmark run 7 (`bench/agent/README.md`): the large-codebase task went from 1.33x to 1.04x of Python's total tokens over two Sonnet 5.5 runs, 5 API calls instead of 6, still 10/10 hidden tests. The token attribution of the old run is in the README: one extra call was 57% of the gap, the spec 23% and broad query answers 21%. a1 and a7 were rerun unchanged (0.76x, 0.61x, 3 calls each).
- The agent spec (`docs/agent/agent-spec.md`) is 1,631 cl100k tokens, from 1,798: concurrency, services, packages and C, sys, bare and GPU, which no agent used in the 60 SSPUR transcripts of benchmark runs 4 to 7, are now one line that names their keywords and points to `spec --full`. The CLI line names `q pack`.
- The MCP server's instructions are 825 characters and point to `start`; they do not carry the spec, because Claude Code cuts server instructions at 2,048 characters. The Claude Code skill starts with `sspur start NAME...`.
- Agent benchmark run 8: with `sspur start NAME...` as the first step, the large-codebase task is 0.82x of Python's total tokens over two Sonnet 5.5 runs (1.04x in run 7), 4 API calls against Python's 5, 10/10 hidden tests. a1, a4 and a7 stay at 3 calls and 0.76x, 0.60x and 0.61x with every hidden test passing. The harness's SSPUR prompts start with `./sspur start` (`setup.py --r7` and `setup_scale.py --r7` give the old ones).

## [0.3.1] - 2026-10-06

### Fixed
- Concurrent native builds of the same program could fail with `No such file or directory`: the GPU runtime shim, the whole-program build and the OpenCL emit used fixed temporary file names in the shared cache, so one process renamed or deleted another's files. Every build now uses per-process, per-build temporary names. This also caused the intermittent macOS CI failure in `kernel_traps_are_identical_in_every_tier`.

## [0.3.0] - 2026-10-06

### Added
- Packages (ADR 0026). `sspur.toml` names a package and its dependencies (a local path, or a git URL with a tag or revision); `sspur.lock` pins each one by the hash of its exports. `pub fn`, `pub type` and `pub effect` export definitions; dependents call `lib.f(x)`, name `lib.T` and `lib.Ctor`, or import with `use lib.{f, T}` (a type brings its variants). Private names, unknown names and undeclared packages are diagnostics (`E_PKG_PRIVATE`, `E_PKG_NAME`, `E_PKG_UNKNOWN`).
- `sspur add <path|git-url>[@rev]`, `sspur deps fetch|update|tree`, and `sspur init --pkg NAME`. Fetches are verified: a dependency whose content hash differs from the lock fails with `E_DEP_HASH`. Dependencies are cached by hash in `~/.cache/sspur/pkgs`.
- `sspur deps update` prints a semantic diff of each dependency's exports (signatures, effects, contracts, body-only changes, added and removed names), typechecks the dependent against the new versions, and refuses the upgrade with the exact diagnostics unless `--force` is given.
- `check`, `run`, `test`, `native`, `verify` and `fuzz --differential` work across packages. Dependencies' effects appear in dependents' signatures and their contracts are proved at the dependent's call sites. Native objects of dependency definitions are shared between dependents through the per-definition cache.
- Codebase mode: `edit` accepts `use` lines and refuses to touch dependency code (`E_DEP_READONLY`), `q list lib` and `q sig lib.f` show a dependency's exports, and `sync push|pull` carries the dependencies with the commits, verified by hash.
- Queries for large codebases: `q find 'a|b*|^c'` lists definitions whose names match (substring, `*` glob, `^`/`$` anchors), with their signatures (exact names first, at most 60; a pattern with `->`, `[` or `(` is still a type-shape search), `q grep TEXT` lists the definitions whose source contains TEXT with up to 3 matching lines each, and `q body A,B` (or `q body A B`) prints several bodies. On the 1,117-definition `s1_shop` codebase, locating the task's functions and the callers of `ship_fee` takes 126 tokens of output with `q find` and `q grep`, against 1,875 for the `q list | grep` the agent used before.
- MCP: `sspur mcp --dir PATH` (or `SSPUR_DIR`) serves a codebase outside the working directory, as Claude Desktop needs; the store is created on the first edit instead of at startup. `docs/agent/mcp.md` covers Claude Code, Claude Desktop and generic stdio clients, with a project `.mcp.json`.
- A Claude Code plugin in `packaging/claude-code/` with an `sspur` skill (the spec, query, one `edit --test`, `verify`, `deploy local` workflow) and the MCP server, listed by `.claude-plugin/marketplace.json` so `claude plugin marketplace add utkarshavardhana/sspur` installs it.
- Fix hints for syntax and names from other languages, in `edit`, `check` and transactions: `&& || !`, `x -> e` and `|x|` lambdas, `elif`, `let`, `+=`, `?:`, braces after a signature, effect rows without commas, `Ctor{_}` and `Ctor{..}` patterns, named arguments, unknown string escapes (regex `\d` is `"\\d"`), `len(x)` and other methods called as functions, `None`/`Some`/`True`, `assert`, `print`, foreign method names (`head`, `max_by`, `to_string`, `compare`, ...), a record's fields on an unknown field, `Str + Int`, `Opt` where a value is expected, a missing return type, and a nested `match` that took the outer arms.
- A rejected `edit` reports the syntax errors of every definition, not just the first, and ends with `fix these and resend the whole edit in one call`.
- Agent benchmark run 6 (`bench/agent/README.md`): with the fix hints, Haiku 4.5's three worst SSPUR cells went from 57 calls to 30, and its 8-task total from 1.21x to 0.83x of Python's. The large-codebase task stayed at 1.33x with Sonnet (6 calls both times).

### Changed
- `q list` groups definitions by kind with counts (`# fns 891`). Tests are named on one line only when there are at most 20; otherwise `q list tests` (or `--tests`) lists them one per line, so `q list | grep WORD` no longer returns every test name.
- `q pack` with more than 8 callers includes 3 tests, 3 callers in full and 5 as signatures, then `-- not shown: N callers`. `pack money` on `s1_shop` is 178 tokens instead of 1,174.
- MCP tools are named `spec`, `src`, `query`, `edit`, `test`, `check`, `run`, `fuzz` and `apply`, with descriptions written for agents and read-only annotations; the old `sspur_*` names still work. `run` and `fuzz` return text instead of JSON, `check` sets `isError` when there are errors, and `resources/list` and `prompts/list` return empty lists.
- The `E_NONEXHAUSTIVE` hint suggests a bare constructor (`| B => ?`) instead of the invalid `B{..}`.
- The agent spec's CLI line names `q find|grep|body|callers` (1,798 tokens, unchanged).

## [0.2.1] - 2026-10-06

This release closes the open items of the 0.2.0 hardening pass (ADR 0025). Native build failures are no longer silent, the program generator covers more of the language, and most effect handlers now compile to native code.

### Added
- `--strict-native` for `run`, `test`, `fuzz --differential`, `native --release` and `build`, also settable with `SSPUR_STRICT_NATIVE=1`. A failed native build becomes an error with exit status 1. CI sets it for the test suite and the corpus check.
- `--quiet` hides the native build warning.
- The `fuzz --gen` generator now writes `F64` code, generic types and functions used at several types, `par` tasks, `profile sys` resources with borrows, moves and drops, and more handler shapes (aborting, raising, code after `resume`, `log` handlers). `--edge` feeds `F64` edge values such as `-0.0`, NaN, infinities and subnormals.

### Changed
- When a native build fails, `run` and `test` print one warning that names the function and the C error. A per-definition build failure falls back to the whole-program build, a whole-program failure to the interpreter.
- These handler shapes now compile natively (ADR 0010): arms that finish without resuming, arms that raise, code after `resume`, `log` handlers, lambdas with declared effects passed to `map`, `filter`, `fold` and similar, `return` or a raise inside a generator loop, and bodies that shadow a name their handler uses. In `tests/programs` the interpreted functions went from 25 to 15, and 14 of the 15 left are entry points that perform an operation.
- Every NaN compares equal to every other NaN and above infinity, whatever its sign bit, in both tiers.

### Fixed
- Interpreter: a closure made before a statement that rebinds a parameter or an outer name saw the new value.
- Native: assigning over a resource after a local function in the same function skipped the old value's destructor.
- Native: `--O3` builds could spend minutes in clang on code that indexes strings with a constant, such as `s.get(30)`.
- Native: NaN ordering and equality could differ from the interpreter, because the sign of a NaN is not stable across compilers and CPUs.
- Native: contract traps inside handled code named an internal `f__ev` copy instead of `f`.
- Printer: `(-0.0).fmt(1)` lost its parentheses and changed meaning, and a nested `if` inside string interpolation printed as a block that did not parse.

## [0.2.0] - 2026-10-05

This release covers Phases 3 to 7 of the roadmap and ADRs 0003 to 0024. SSPUR now compiles to native code by default, has a standard library close to C++ coverage, and ships the agent, deploy, GPU and sync tooling.

### Added

- `sspur fuzz --gen N --seed S`: random well-typed programs compared between the interpreter and three native builds, with a reducer (`--reduce`) and a CI campaign (ADR 0025).

**Agent loop** (ADR 0003, 0021)
- Content-addressed codebase in `.sspur/` with atomic, typechecked transactions (`add`, `replace`, `rename`, `remove`, `refine`, `fill`, `attach`), test gates and stale-base checks.
- `sspur edit` and `sspur apply -e` for small inline edits, with `--test` to gate on the suite.
- Query API (`sspur q`): `list sig body callers callees effects find pack why impact holes diag log`, with a token budget.
- MCP server over stdio (`sspur mcp`).
- Tolerant parsing of common agent mistakes, `with` deep updates, and `ex` examples in signatures.
- Concurrent transactions on CRDT definition registers with typecheck-on-merge and conflict reports, tested with 100 concurrent agents.
- Replica sync by hash over a directory or TCP (`sspur sync serve|push|pull|status|resolve`) and a global check cache.
- Agent spec (`sspur spec`) kept under 1.8k tokens.

**Language**
- Algebraic effect handlers with one-shot `resume`, and generators with `yield` and `for` (ADR 0010).
- Safety types, `Guess`, decision tables, `while`, and local functions.
- Structured concurrency: `par` tasks, `Atomic[Int]`, `Chan[T]` and the `conc` effect, with races rejected by the checker (ADR 0013).
- The `sys` profile: owned resources, borrows, `Ptr`, `unsafe`, and an ownership checker with a soundness suite (ADR 0012).
- The `bare` profile: no runtime, MMIO, interrupt handlers, fixed arrays `Array[T, N]` and `[v; N]`, static module state with atomic access and the `E_STATIC_RACE` rule (ADR 0017, 0024).
- Inline asm with named operands and clobbers in `sys` and `bare`.
- `store` and `svc` definitions with `db.read` and `db.write` effects.
- `kernel fn` GPU kernels with `@grid` and `@grid2`, slice borrows and a device subset checker that rejects races (ADR 0020, 0023).

**Native compiler** (ADR 0004 to 0008, 0011, 0015, 0022, 0024)
- Cranelift JIT tier, and a C tier through clang that is now the default for `run` and `test`, covering the full language.
- Garbage collector for native code with bounded memory for long-running programs.
- Runtime checks the compiler can prove are removed, using intervals and Z3.
- In-place reuse for owned trees and lists, pipeline fusion, self tail calls as loops, and traps that unwind via longjmp.
- Pure fused pipelines run in parallel on a pthread pool; Int and F64 pipelines vectorize with an exact sequential rerun on flagged blocks.
- Per-definition translation units cached by content, proven rewrites with `sspur explain-opt`, and profile-guided inlining via `run --profile`.
- Opt-in PGO (`--pgo`, `--retrain`) and explicit `--lto off|thin|full`.
- A direct LLVM IR prototype for the scalar, control-flow and record subset (`sspur build --backend llvm`). C stays the production path.
- Bare-metal targets: riscv64 and aarch64 QEMU virt, and Cortex-M4 (`thumbv7em-mps2`) with vector table, SysTick and NVIC.

**Contracts and verification**
- `sspur verify` proves `pre`, `post` and `where` clauses with Z3 and reports counterexamples (ADR 0009).
- `sspur fuzz` turns contracts into property tests, with `--differential` comparing native code against the interpreter.

**Standard library** (ADR 0018)
- Collections: `Set`, `Heap`, `HashMap`, `HashSet`, `Bits`, `FlatMap`, `MdSpan`, lazy `View` pipelines, stepped ranges and more list algorithms.
- Text: `StrBuf`, format specifiers, Unicode case folding, and a Pike VM `Regex` with captures, replace and split.
- Numbers: the `<cmath>` set, integer bit ops, `BigInt`, fixed-point `Dec`, `Complex`, seeded distributions and a splittable `Rng`.
- System: files, streaming `File` handles, directories, stdin, `Time`, `Duration`, IANA time zones, environment, child processes and `exit`, each behind its own effect.
- `json.encode` and `json.decode[T]` for any data type.
- Every builtin behaves the same in the interpreter and native code, traps included.

**Interop**
- C FFI: `extern fn` with the `ffi` effect, `sspur bind` for C headers, and `sspur export-c` to build a C library and header (ADR 0014).

**GPU** (ADR 0020, 0023)
- Kernels run on Metal with exact F32 and identical traps, and fall back to C on the CPU. Device buffers, async launches, shared memory and barriers, deterministic atomics and inlined helper functions.
- `sspur gpu --emit metal|opencl|spirv|ptx`.

**Deploy** (ADR 0016, 0019)
- `sspur deploy plan|local` generates infrastructure and least-privilege IAM from a service's effects and runs it locally.
- Store migrations with lazy read-time upgrades and backfill, hot swap with canary and rollback, and request replay.
- Container-free Lambda builds through zig cross-compilation.

**Benchmarks**
- Agent benchmark of multi-step feature tasks in SSPUR and Python with hidden tests, and native benchmarks against C++ at `-O2` and `-O3`.

**Release tooling**
- `sspur --version`.
- GitHub Actions CI on macOS arm64 and Linux x86_64 running build, clippy, tests and the corpus differential check (`tools/corpus_diff.py`).
- Release workflow building tarballs for aarch64 and x86_64 macOS and Linux with checksums, a Homebrew formula and `install.sh`.

### Changed
- `run` and `test` compile to native code by default; `--interp` forces the interpreter.
- Dual-licensed under MIT or Apache-2.0.
- Native builds link `libm` and `libpthread` explicitly off macOS, and LTO uses `lld` on Linux when it is available.

### Fixed
- Twelve bugs found by the hardening pass (ADR 0025), among them a printer dangling `else` that the optimizer turned into wrong native results, lexical capture across same-block shadowing in the interpreter, a data race through `for x in par(xs)` over closures, removals that could not sync, and a `str_find` segfault.
- Checker stack overflow on field access of an unknown-typed value.
- Native `List.sort` on primitive elements.
- GC large-allocation trigger and page reuse.
- Huge ranges, lists and repeats trap with out of memory instead of aborting; single allocations are capped below machine memory.
- `Secret` type search no longer loops on alias cycles.
- Records with C keyword field names compile natively.
- F64 sums start from -0.0 natively, matching the interpreter.

## [0.1.0] - 2026-10-02

### Added
- Language design spec, JSON schemas and the token benchmark.
- Compiler core: parser, checker, content hashing, interpreter and the `sspur` CLI, with an 11-program suite.

[Unreleased]: https://github.com/utkarshavardhana/sspur/compare/v0.8.1...HEAD
[0.8.1]: https://github.com/utkarshavardhana/sspur/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/utkarshavardhana/sspur/compare/v0.7.1...v0.8.0
[0.7.1]: https://github.com/utkarshavardhana/sspur/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/utkarshavardhana/sspur/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/utkarshavardhana/sspur/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/utkarshavardhana/sspur/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/utkarshavardhana/sspur/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/utkarshavardhana/sspur/compare/v0.3.2...v0.4.0
[0.3.2]: https://github.com/utkarshavardhana/sspur/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/utkarshavardhana/sspur/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/utkarshavardhana/sspur/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/utkarshavardhana/sspur/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/utkarshavardhana/sspur/releases/tag/v0.2.0
[0.1.0]: https://github.com/utkarshavardhana/sspur/tree/6c55224
