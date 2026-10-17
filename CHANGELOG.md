# Changelog

All notable changes to SSPUR are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/). While SSPUR is below 1.0, minor releases can change the syntax and the standard library.

## [Unreleased]

### Added
- Lambda parameters can name their types: `(x: Int) => x + 1`, `(k: Str, v) => ...`.
- A type alias can take an effect-row parameter, `type Handler[e] = Request -> Response ! e`, used as `fn apply[e](h: Handler[e], r: Request) -> Response ! e` or `Handler[log]`; it was `E_UNKNOWN_TYPE unknown type 'e'`.
- `BigInt`, `Dec` and `Ratio` have the arithmetic operators their methods implied: `big(2) + big(3)`, `-d`, `ratio(1, 2) / ratio(1, 4)` (no `/` for `Dec`, whose division takes a scale), including through `[T: Add]`; they were `E_OPERATOR`.
- `W_UNUSED_RESULT`: a statement that calls a pure function or method and drops a value that isn't `Unit` is a warning, with the fix in the hint. `b.add(s)` or `xs.push(x)` on a line of its own compiled silently and did nothing, since values never change in place; `_ = f(x)` drops a value on purpose.
- `Opt.and_then(f)` and `Res.and_then(f)` chain a function that gives an `Opt` or a `Res`; `List.repeat(n)` is the list `n` times over (`[0].repeat(n)` is `n` zeros), named like `Str.repeat`; `Map.is_empty`. In both tiers.
- `req.peer`: a server's `Request` carries the address of the client the connection comes from (IPv4 as `1.2.3.4`, IPv6 without a zone), the same in both tiers and on Windows; the network reference says when `X-Forwarded-For` can be trusted. `Response` has `data: Bytes`, sent after `body`, and `http.bytes(status, b)` builds one, so a handler or a middleware can serve binary or compressed content. Literals may leave `peer` and `data` out, so existing `Request{...}` and `Response{...}` literals keep working, and content hashes don't change.
- One rule for `Str`, `Bytes` and `Secret` arguments, in the language reference: the calls of the std namespaces (`crypto.*`, `gzip.*`, `http.*` and the rest) take a `Str` for `Bytes`, an `X` for `Secret[X]`, and a `Str` or `Secret[Str]` for `Secret[Bytes]`; everything else, your own functions included, converts explicitly (`s.to_bytes`, `secret(x)`, `k.map(t => t.to_bytes)`). `http.bytes` follows it.
- `sspur fmt` keeps comments (ADR 0042), and so do the language server's formatting, the playground's Reformat and the wasm `fmt`. A comment on its own line stays on its own line before the same code, a comment after code stays after it, comments between definitions keep their blank lines, and a short list, call, record, parameter list, pattern or sum type with a comment inside is written one element per line so the comment stays next to its element. Comments are still not part of the canonical text or any hash. A property test formats every `.ssp` file in the repository and every corpus program at 40 and 100 columns and checks that every comment comes back once, in order, with the same code before it, and that the canonical text, the hashes and idempotency hold.
- Binding annotations: `x: T = e` and `var x: T = e` are checked (`e` against `T`) and kept in the canonical text, so `var seen: List[Int] = []` survives `sspur fmt` and `build.fmt`. They used to be accepted and dropped.
- `\u{...}` escapes in strings (1 to 6 hex digits, `"caf\u{e9}"`), `\}` for a literal `}` and `\'` for a quote in a `'...'` string.
- `_ => e` and `(acc, _) => e`: lambdas that ignore a parameter.
- `pkg.models.Doc` names a public module's definition in expressions, types, record literals and patterns, without a `use`.

### Changed
- Cold native builds of big packages are 5 to 7 times faster (ADR 0041). On a generated package of 200 modules and 2.5 MB of source (`bench/bigpkg`), a cold `sspur test` went from 282 s to 40 s, `sspur publish --dry-run` from 291 s to 59 s and a dependent's first `sspur run` from 251 s to 41 s; testing again after editing one function of a module went from 36 s to 7 s. The functions of a package or module with 16 or more functions now share C units of up to about 32 instead of one each, so clang no longer parses and compiles the same declarations and helpers thousands of times; a function of the program itself keeps its own unit, and a dependency's units are the same in every program that uses it, so dependents still share its objects. A build with enough C runs one clang process per core (at most 4 before), fewer when memory is short, and `SSPUR_JOBS` still sets the number; the units are made on every core; and the Z3 questions of a cold build are answered on every core. Programs without such packages compile to the same objects and libraries as before, so run times and the cached `sspur run` are unchanged.
- The SMT cache is `smt2/` in the cache directory: each answer is an empty file named by the question and the answer, and a build lists a directory once instead of opening a file per question, which takes a warm `sspur test` of the package above from 9.8 s to 4.9 s. Answers in the old `smt/` directory aren't read.
- The Homebrew tap moved to the sspurlang organization: `brew install sspurlang/sspur/sspur`. `packaging/homebrew/core/sspur.rb` is the formula for homebrew-core, so that after 1.0 `brew install sspur` works.
- The interpreter appends in place when the receiver is a local variable's last use (`xs := xs.push(x)`, a fold's accumulator, `b := b.add(s)`, `m := m.put(k, v)`, `Set.add`, `concat`, `remove`), so building a list, string or map one item at a time is linear under `--interp` as it is natively: 40,000 pushes in a loop went from 6.1 s to 0.12 s, a fold that pushes 40,000 times from 6.0 s to 0.10 s, 40,000 `Map.put` from 14.8 s to 0.10 s, and 80,000 `StrBuf.add` from 1.75 s to 0.14 s (whole `sspur run --interp`). A value still in use elsewhere is copied as before, and so is everything in the playground.
- The interpreter's recursion limit is a stack budget, not 20,000 calls: `sspur run --interp` and `sspur test --interp` may use 256 MB of stack (`SSPUR_STACK=512M` or `2G` changes it), about 130,000 calls of a simple recursive function against 20,000 before, and then trap with `stack overflow in f`; infinite recursion traps after 0.1 s at about 300 MB of memory. The thread keeps native code's 512 MB, `par` tasks and other threads keep their limits, and startup is unchanged.
- F64 semantics for 1.0, the same in both tiers and documented under Numbers in the standard library reference: comparison stays a total order (NaN equals itself and sorts above `inf()`), but `-0.0 == 0.0` now, so `-0.0 < 0.0` is false and `where _ >= 0.0` accepts `-0.0`; the zeros stay distinct values (`-0.0` prints as `-0.0`). `round floor ceil trunc` trap with `integer overflow` on NaN, an infinity or a result outside `Int`, like `Int` arithmetic, instead of saturating silently (`nan().round` was 0, `1.0e19.round` was `Int` max); `checked_round checked_floor checked_ceil checked_trunc` give `Opt[Int]`.
- A `{` in a string interpolates only an expression that is not a lone literal and doesn't start with `{`: `"a{3}"`, `"^a{2,4}$"` and `"{{x}}"` are text, as `"{}"` and CSS braces already were. A brace that is text no longer swallows the code after it: `s.contains("{") or s.contains("}")` is two calls and `"{" + x + "}"` three strings (they were one string). No program in the repository or the corpus changes meaning.
- `with` binds tighter than comparisons: `p == q with a := 1` is `p == (q with a := 1)` (it updated the `Bool`, an error). The reference lists the precedence of every operator.
- A `match` in a match arm ends at the first arm indented less than its own, so `| some(x) => match x` with its arms indented deeper no longer takes the outer `| none`. Arms at the same column still belong to the inner match.
- `build.fmt` counts comments now that `sspur fmt` keeps them: each one where `sspur fmt` puts it (indent and blank lines don't count). It also reads a long sum type broken one variant per line and a broken list pattern with `..`.
- The playground's WebAssembly stays under its budget although it gained the 1.0 additions and keeps comments: hole filling sorts names instead of table entries, expression clones and walks are no longer inlined into every caller, and the checker, the editor features and the runtime helpers shared with native code are built for size. Running takes the same time.

### Fixed
- A program whose only use of `Bits` is `List.to_bits` builds natively; it failed with `unknown type name 'SBits'` and fell back to the interpreter. Every C runtime section now declares the types it uses, and the test that compiles each section alone no longer declares them for it.
- The native compiler no longer overflows its stack on a recursive function passed by name to `map` and also mapped inside a lambda (`cs.map(depth)` with `gs.map(g => g.map(depth))`): the cost estimate follows a function named as an argument at most three levels deep and never into a recursive one, as it already did for calls.
- A dependency's audit notes and warnings are no longer printed against the dependent's file at lines that don't exist, on every run. Its warnings are its own (its own check shows them); its audit notes are listed by the dependent's `sspur check`, not by `run` or `test`, at the dependency's file and line (`frlib/lib.ssp:3:3 audit A_DECLASSIFY ...`). The rule is in the error reference.
- A path dependency outside the package that changed since `sspur.lock` pinned it is no longer silent: `sspur run`, `test` and `check` print a note naming it and `sspur deps update NAME`, and an `E_PKG_NAME` for a name only the new version has (a renamed export) carries the same hint instead of only listing the old exports. A change that leaves the exports as they are (a comment) gives no note.
- An effect-row parameter shared by several arguments is the union of their effects: `twice(n => add_log(n), n => add_time(n), id)` with `fn twice[e](a: (Int -> Int ! e) -> (Int -> Int ! e), b: ..., f: Int -> Int ! e)` no longer fails with `expected Int -> Int ! log, found Int -> Int ! log, time`, and the same for a list of such lambdas.
- `fail[E]` in a parameter's row takes `E` from the argument: `fn rescue[E, e](next: Int -> Int ! fail[E], e) -> (Int -> Int ! e)` catching `E` gives a pure `rescue(failing)`; the argument's `fail[X]` used to land in `e`.
- A `catch` whose only arms are `_` handles every error its body raises, of any type, in both tiers; with two error types it was `E_CATCH_TYPE`.
- `db.tx` works inside a function generic in its result or error type (`fn txg[A, e](c: Db, f: () -> A ! e) -> A ! db, fail[DbErr], e`), when the lambda ends in `raise`, and when it returns `ok(())` as a `Res` the function's result type names: the expected type now reaches the lambda. A non-sum error type such as `Str` raised in a transaction also rolls back and is raised again; these were internal `E_DEP` errors.
- A `catch` with a binding arm `| e => f(e)` over a body that raises several error types handles the one `f` takes.
- A call's expected result type informs its arguments, so `spans.fold([], (acc, s) => match acc | [..init, last] => ...)` knows the accumulator's element type from the function's result type.
- `==` is consistent with hand-written `Eq` impls: a type that derives `Eq` compares a field whose type has its own impl with that impl, and so do `Opt`, `List`, `Res` and tuples of it, so `some(k) == some(k2)` works for a type with an `impl Eq` around a secret, where it was `E_SECRET_COMPARE`. A derived `Eq` over a newtype of `Secret` used to compare the secret structurally and ignore the newtype's impl. Comparing a newtype of a `Secret` that has no impl with `==` is now `E_SECRET_COMPARE`, like the secret itself.
- `json.decode[T]`, `r.json[T]`, `db.query[T]`, `db.one[T]`, `db.opt[T]` and the `csv` readers work with a type parameter `T` of a generic function, which is then compiled once per type it is called with; each call checks that its type can be decoded (`E_JSON`, `E_SQL_ROW`, `E_CSV_TYPE`). A call can name the type argument, `dec[Point](s)`, where it used to read as indexing. `json.encode(x)` with `x: T` takes `[T: Json]`. A method's type argument may be a tuple, `json.decode[(Int, Str)](s)`, without an alias.
- A package whose function takes a function value with one of the package's own effects (`pub fn handled(f: () -> Int ! tick) -> Int`) builds as a dependency; the effect inside the parameter's type was not renamed with the package. A package may also export functions named like a built-in effect (`pub fn fail`, `pub fn log`): its `fail[E]` and `log` rows stay the built-in effects. A user effect can't take a built-in effect's name (`E_DUPLICATE`).
- `"\\{x}"` is a backslash and the value of `x` (it was the text `\{x}`).
- The canonical text of corpus program t46, a `catch` ending a match arm, read back as a different program; a `match`, `catch` or `handle` that ends an arm other than the last now prints inside `do` on its own line. Its content hash is unchanged.
- `_` in a `pre` or `post` condition is a placeholder lambda, as in any call argument: `post r.map(_ + 1).len == xs.len` checks (it was `E_PLACEHOLDER`).
- Error spans of names interpolated after an escaped quote point at the name.
- A `match` or `catch` with arms inside a string's `{...}` prints on one line, so `sspur fmt` keeps the string whole and a program using `http`, `db` or `crypto` with one builds; it failed with `E_DEP internal: the elaborated network program does not parse: unterminated string`.

## [0.14.0] - 2026-10-17

### Added
- `sspur fmt` breaks lines over 100 columns (ADR 0040): lists, tuples, calls, records and parameter lists one element per line with a trailing comma, `with` updates one per line, `if` chains with each `else` on its own line, conditions after `and` and `or`, other operators and method chains where the pieces then fit. `if` blocks are written with `else` under the `if`. The canonical text that definitions are hashed and stored as is unchanged, so no content or export hash moves and every `sspur.lock` and published package still verifies. The language server's formatting and the playground use the same formatter.
- Public modules: a package marks a module `public = true` in its `sspur.toml` (`models = { path = "models", public = true }`), and dependents import its definitions with `use pkg.models.{Doc}`. The registry's API listing, semver check and site include them as `models.Doc`. Without the flag a module stays private, so no published package's API or export hash changes.
- The playground's Format is Reformat Code: Cmd+Option+L or Ctrl+Alt+L as well as Shift+Alt+F, the definitions a selection touches, the cursor kept after the same code and on the same screen line, one undo to go back, and code that doesn't parse left alone with the error shown in place. A ruler marks column 100 when lines don't wrap, phones get a Format button in the editor's corner, and an optional "Format on run" (remembered, off by default) formats before Run, Test and Share. Enter keeps a list element's indent, and a closing bracket typed at the start of a line goes back to its opener's indent (also in VS Code and the language server).

### Changed
- The registry's site renders README tables (stacked on phones), shows the top 12 packages with descriptions capped at three lines, and its 404 page, like sspurlang.github.io's, reads as a compiler error with a quick fix to the closest page or package. The docs link the registry and the home page from the top of the packages chapters.
- The playground scrolls long lines sideways on phones instead of wrapping them.
- The registry check `build.fmt` accepts what `sspur fmt` now writes, as well as the one-line form of 0.13, and a statement broken anywhere the lexer reads as one line.
- The Claude Code plugin's version is the release version (it was 0.2.1); a test keeps it and the editor extensions there.
- The playground's WebAssembly is 3 KB smaller gzipped although it now breaks lines: the editor docs keep only the tables hover reads, and wasm-opt folds constant offsets into loads and stores.

### Fixed
- A function imported with `use` no longer shadows a record field of the same name: `t.digest` is the field when `t` has one, as it already was for local functions, in both tiers.
- A number after `.` is always a tuple index, so `p.0.1` reads, and `(p.0).1` next to a trait-bounded function no longer fails with an internal error.
- In a string literal, escapes apply inside literal braces too: `"{x\\y}"` has one backslash, as the reference says.
- `sspur q` names scoped dependencies by their key (`pkg.f`), not their link prefix.
- The playground's Reformat returns to the left edge after breaking long lines.

## [0.13.1] - 2026-10-17

### Added
- Registry: a public member of an organization may publish the first version of a package from the organization's repository, and a package whose repository was transferred or renamed on GitHub keeps publishing from its new home (GitHub resolves the old name). `sspur publish`, the Publish page and the workflow all ask GitHub.
- The registry check `pkg.fork` (blocking): the first version of a new flat name can't come from a fork, so nobody claims a name with a fork of someone else's repository. A fork can still publish under its owner's scope. `sspur publish`, the Publish page and the workflow all ask GitHub, and the Publish page now runs the index's own checks.

### Fixed
- The registry's hourly sweep reruns unanswered requests one at a time; during a burst of requests, GitHub kept only one queued run of the index writer and cancelled the rest.

### Changed
- SSPUR moved to the GitHub organization [sspurlang](https://github.com/sspurlang): the repository is github.com/sspurlang/sspur, the docs, playground and Pulse are at sspurlang.github.io/sspur, and the registry is sspurlang/sspur-packages at sspurlang.github.io/sspur-packages. Old repository, release and raw URLs redirect, and the old Pages URLs forward to the new ones, so existing installs, lock files and links keep working. `sspur` reads the registry from its new home, and the Homebrew tap stays `utkarshavardhana/sspur`.
- The registry's home page shows the top 10 packages (most depended on, then most recently updated) under a "Top packages" header with a count; search looks through all of them. The site's header and favicon use the SSPUR logo.
- CI runs on branch pushes only. A release first checks that CI passed on the tagged commit and stops otherwise, instead of running CI again for the tag.

### Fixed
- A record field wins over a function imported with `use` of the same name, as it does over a local function: `t.digest` with `use sessions.{Token, digest}` read the field `digest` of `Token` as the call `digest(t)` and failed to typecheck.
- `(p.0).1`: a number after `.` is always a tuple index, so `p.0.1` no longer lexes as `p` and the float `0.1`. `sspur fmt` printed that text and broke the program, and the elaborator, which prints and re-parses programs with traits, failed with `expected a field or method name, found '0.1'`.
- Escapes inside braces that are not an interpolation work as anywhere in a string: `"{x\\y}".len` is 5, as `"\{x\\y}".len` is, in both tiers (the backslashes stayed doubled, which corrupted JSON written that way).
- `sspur q` names a scoped dependency by its key, `pkg.f`, and accepts it in queries; it showed and needed the link prefix (`acme_pkg_.f`).
- `build.fmt` accepted only part of what `sspur fmt` printed: a `match` or `catch` as a call argument or a parenthesized operand, and `if` blocks in a `for` body.
- Docs: ADR 0033 gave the `gpu` benchmark's peak as 97 MB and the benchmark table 92 MB; both are right, for `sspur run --no-cache` and for the cached run that is the default since ADR 0034, and the ADR now says so.

## [0.13.0] - 2026-10-16

### Added
- Scoped package names tied to GitHub owners (ADR 0039), so two companies can each have `pkg`: `name = "acme/pkg"` in `sspur.toml`, where `acme` is the GitHub user or organization that owns the repository. The new blocking check `pkg.scope` verifies it: the request must come from that user or, for an organization, from a public member (a private member is told how to make the membership public). Scoped packages live at `packages/@acme/pkg.json` in the index, with releases named `acme--pkg-VERSION`; flat names and existing index files are unchanged, and 0.12.0 clients keep installing flat packages. Every new name, scoped or not, still waits for a maintainer's `/approve`.
- `sspur add acme/pkg` writes `pkg = { package = "acme/pkg", version = "^1.0.0" }`, using the name after the scope as the dependency key; when the key is taken it says so and `--as NAME` picks another, so two scopes' `pkg` work in one program. Locks record full names, `sspur deps tree` shows the key a package is used under, and `sspur publish`, `yank`, `search`, `source`, the issue forms, the package site (pages at `packages/acme/pkg/`) and the Publish page know scoped names.
- Published packages can contain module packages: path dependencies inside the package's directory, as the web-app template's `models = { path = "models" }`. Every check covers every module (parse, typecheck, formatting, docs, tests in both tiers, secrets, review rules) with paths from the repository's top; `sspur publish` runs the modules' tests side by side in a copy of the commit. Dependents build the modules under the package's name (`acme/pkg:models`, `source = "module"` in the lock), so they never clash with a dependent's own modules. The package's API is the root module's exports. The Publish page checks multi-module repositories too, listing the tree once with the git trees API.
- The registry credits authors: the index commit is authored by the requester with their GitHub noreply address and committed by the bot, and the merge uses `gh pr merge --squash --author-email` with a `Co-authored-by` trailer, so the commit on main counts in the author's contributions. The Publish page also lists a user's public organizations and their repositories, and shows the scope check in the browser.

### Changed
- The registry's size limits are 2000 files, 1 MiB per file, 4 MiB of SSPUR source and 20 MiB per repository (were 500 files, 512 KiB of source and 5 MiB), room for a library the size of Apache Commons Lang.
- `sspur add money@1.2.3` and `sspur source money@1.2.3` mean exactly that version (`=1.2.3`); requirements such as `^1.2` and `~1.2.0` are unchanged.
- Builds of path packages are reused: a module that many others depend on is built once per command instead of once per path to it, and a build with the same inputs comes from the cache. On a synthetic package of 2.5 MB in 200 modules, a cold `sspur check` went from 166 s to 11 s.

## [0.12.1] - 2026-10-16

### Changed
- The first version of a new package name waits for a maintainer of sspur-packages to comment `/approve` on its request, after every check has passed. The approval holds only for the commit the pull request showed. Later versions still merge on their checks alone.

### Fixed
- The registry's workflows verify sspur against the release's `SHA256SUMS`, keep their work files in the runner's temp directory, and the Publish page's username pattern is valid in Chrome and Firefox.
- `sspur fmt` keeps record shorthand: `Page{items, total, limit: r.limit}` stays as written instead of becoming `Page{items: items, total: total, ...}`, and a field written `x: x` prints as `x`, as record patterns already did. The registry's `build.fmt` check (and so `sspur publish`) no longer fails idiomatic packages, the web-app template's own included: it reads a statement broken across lines after an operator or before `.name`, and an `if` chain with each `else` on its own line, as the one line `sspur fmt` prints, and accepts match arms indented one level under their `match`.
- `sspur update` (and `sspur update NAME`, `sspur deps update`) reports a move to a newer version whose exports are unchanged, `~ pagination 0.1.0 -> 0.1.1 (no API change)`, instead of `up to date`, and `sspur deps tree` and the update diff show the locked version: versions with the same exports share a cache entry, which kept the first version's number.
- A native build no longer fails with `cannot load ...\native\` on Windows when another build of the same program is writing its library's name at that moment (seen as a CI failure of the GPU trap tests). Loads of native libraries also retry briefly on Windows, and a failed load keeps the system's reason.
- `sspur run`, `sspur test` and `sspur debug` set `SSPUR` to their own executable for the program, in both tiers, unless it is already set, so a program that runs `sspur` (such as the web-app template's `tests/run.ssp`) uses the same compiler instead of whichever is first on `PATH`.

## [0.12.0] - 2026-10-16

### Added
- A public package registry (ADR 0038), [sspur-packages](https://github.com/utkarshavardhana/sspur-packages), that runs entirely on GitHub at no cost: one JSON file per package, each version pinned to its tag, commit, the content hash of its exports and the SHA-256 of a deterministic source snapshot stored as a release asset, so a version survives its author deleting or rewriting the repository and a moved tag changes nothing for users. The template of the index repository, with its workflows, forms, schemas, standards and SSPUR programs, is in `registry/packages-template/`.
- `sspur add money` and `sspur add money@^1.2` add published packages (`money = { version = "^1.2" }`), with Cargo's requirement syntax and the one-version rule across the graph. `sspur update` moves them to the newest matching versions with the semantic diff. Installs download the snapshot, verify its SHA-256 and the export hash, fall back to the author's repository at the pinned commit only when the snapshot is missing, refuse a tampered snapshot, and never run package code. Builds stay offline and reproducible from `sspur.lock`, which records `git`, `snapshot` and `sha256` for registry packages.
- `sspur publish` runs the registry's 35 checks on the committed package (parse, typecheck, no warnings, formatting, tests natively and interpreted, version rules with an API diff by signature hashes, an effects audit that needs new capability effects acknowledged with `--ack`, licence, README, docs on every public definition, tests, naming, typosquatting, size, binaries, secrets, hooks, and review standards) and opens the publish request with the GitHub CLI, or prints the prefilled request form. `sspur yank`, `sspur search`, `sspur source` (download and verify without running), `sspur api` (the public API and the facts the checks read) and `sspur archive` (the snapshot of a commit).
- The registry workflow turns each request into a pull request whose description lists every check with file, line and fix, the effects per function, the API changes and how to submit the next revision, and merges it when every blocking check passes; package code runs only in a read-only job.
- The package site on sspur-packages' GitHub Pages, built by SSPUR programs: search, a page per package with its README, API docs, versions, effects, dependencies, dependents and licence, and a Publish page that lists your repositories and tags with GitHub's API, runs the checks in the browser with a separate WebAssembly build of the checker, and opens the request on github.com.
- The handbook chapter Publishing a package, CLI reference for the new commands, and ADR 0038.
- `sspur new web-app NAME` writes SSPUR's reference application, a notes service with accounts, from a template built into the binary (`templates/web-app/`): a JSON API and server-rendered HTML pages through one `svc`, SQLite with migrations, transactions and full-text search, Argon2id passwords, session tokens stored as SHA-256 digests, HMAC CSRF tokens, secure cookie defaults, login rate limiting per account and address, input rules as `where` refinements, `config.load` settings with a startup error for a missing production secret, structured JSON logs with request ids, gzip and graceful shutdown. Its modules are packages inside the app (`models`, `storage`, `auth`, `views`) and three are written to be reused and later published (`sessions`, `pagination`, `passwords`). It comes with unit tests for the storage, auth and views, handler tests that call the svc directly, an end-to-end test that starts the server on a free port and uses it with the HTTP client, a concurrency test (16 clients, no lost writes) and a load smoke test, all run natively and interpreted by `sspur run tests/run.ssp`; a zstd backup tool; a systemd unit and a Caddyfile; and a README covering running, testing, debugging with lldb, configuration, backups and deploying. `sspur new --list` lists the templates, and the new tutorial "Build a web app" walks through the code.
- A `svc` can take parameters: with `svc api(app: App)`, `http.serve(port, api(app))` serves it and every endpoint parameter named `app` gets the value, so handlers reach a database, keys and settings without globals; `api(app)` is a `Request -> Response` function that tests can call directly. `sspur deploy` rejects such a svc (`E_EP_PARAM`).
- `json.decode[T]`, `r.json[T]` and svc request bodies accept types with `where` refinements: the value is decoded, every clause checked, and one that fails is an `err` (a 400 for a body) naming the field and the clause, `body.title: must satisfy _.len <= 200`. Records, aliases, newtypes, lists and `Opt` are covered; refined sums and generic types are `E_JSON`.
- `config.load` reads `Secret[Str]` fields, and a field that breaks its `where` clause makes it return `err("port: must satisfy ...")` instead of trapping.
- `req.form(name)` reads a field of a form body and `req.cookie(name)` a cookie.
- `proc.spawn(prog, args, env)` starts a program without waiting for it, in both tiers; the `Proc` it returns has `line(timeout_ms)` for the next line of its output, `running()`, `stop()` (SIGTERM, then SIGKILL after 10 seconds) and `wait()`, and processes still running when the program ends are killed. This is what lets a program test its own server end to end.
- With `SSPUR_LOG=json`, a message that is a JSON object (`log.info(json.encode(event))`) puts its fields in the line instead of `msg`, and access log lines carry `method`, `path`, `status`, `ms` and `id` as fields.
- Path dependencies inside the package's own directory follow their source: an edit to such a module package takes effect at the next build, which locks it again, while dependencies from outside the directory stay pinned until `sspur deps update`.

### Changed
- svc endpoints may return and take any type `http.serve` can encode and decode, generic records such as `Page[Note]` and `Time` included, instead of only the types a deployed service supported.
- `Time` values bound as SQL parameters are ISO 8601 text with milliseconds (`2026-10-15T09:30:00.000Z`), so text order in SQL is time order.
- Commands run in a package without `src` in its manifest use `lib.ssp`, as dependents do, before `main.ssp`.
- The playground, which can't serve, leaves out svc parameters, refined JSON decoding and the interpreter's typed `catch` below, so its WebAssembly stays at 972 KiB gzipped.

### Fixed
- In the interpreter a `catch` whose arms name constructors of one error type also caught other error types in its `_` arm, unlike native code (the playground still does). A svc endpoint that raised its own error type next to `DbErr` answered 500 interpreted; it now answers as in native code.
- Debug builds of programs that elaboration rewrites (networking, the data libraries, traits) mapped their lines to the rewritten program rather than the file, so with comments or other formatting above a function a breakpoint by line stopped in the wrong place. Lines now map to the file.

## [0.11.0] - 2026-10-15

### Added
- `Bytes`, an immutable byte string: `bytes(xs)`, `s.to_bytes`, `b.len`, `get`, `slice`, `concat`, `to_list`, `to_str`, `hex`, `base64` and `base64url`, `from_hex`, `from_base64` and `from_base64url`, and `read_bin` and `write_bin` for binary files. It compares byte by byte, shows as `0x` and hex, and is a base64 string in JSON. In the interpreter its memory is wiped when freed.
- SQLite (ADR 0037), vendored as its amalgamation and compiled into the cache on first use, behind a new `db` effect: `db.open(path)` (or `":memory:"`), `db.exec(c, sql, params)`, `db.query[T]`, `db.one[T]` and `db.opt[T]`, which map columns to a record's fields by name (or to a tuple or one column type) and raise `Mismatch` with the column and the reason when a value doesn't fit, `db.tx(c, f)`, which commits or rolls back when `f` raises (savepoints when nested), and `db.migrate(c, steps)`, which applies ordered SQL steps and refuses applied steps that changed. Values reach SQL only through `?` parameters; a list literal of parameters may mix types. Errors are `DbErr` (`Sql`, `Constraint`, `Busy`, `NoRow`, `Mismatch`). Files use WAL, `synchronous=NORMAL`, foreign keys and a 5 second busy timeout; prepared statements are cached per connection; a database is a pool of connections that the HTTP server's concurrent handlers share, with transactions queued for the write lock, and opening the same file again returns the same pool. `http.serve` answers a raised `DbErr` with 404, 409, 503 or a 500 without SQL text.
- Crypto (ADR 0037), from Monocypher and libsodium, compiled into the cache on first use: `crypto.sha256`, `sha512` and `blake2b`, `hmac256` and `hmac512`, `equal` (constant time), `seal` and `open` (XChaCha20-Poly1305 with a random nonce), `hash_password` and `verify_password` (Argon2id at OWASP's parameters, PHC strings), `keypair`, `public_key`, `sign` and `verify` (Ed25519), and `random`, `token` and `key`, with `uuid.v4()` and `uuid.v7()`. Randomness comes from the operating system behind a new `rand` effect; keys and passwords are `Secret` values, and a plain value is accepted where one is expected. Known-answer vectors from NIST and the RFCs are tested in both tiers.
- Compression: `gzip`, `deflate` and `zstd` with `compress`, `compress_level` and `decompress` (a `Res`, capped at 1 GiB), and `compress_file` and `decompress_file` for gzip and zstd, which stream with constant memory and never leave a partial output file.
- CSV: `csv.parse` and `csv.write` (RFC 4180, with CRLF output), `csv.decode[T]` and `csv.read[T]` for records by header name with errors that name the row and column, `csv.fold[T]` to stream a large file, and `csv.encode` for records.
- The `\r` string escape.
- Handbook chapters Databases, Security and Crypto, and Files, Compression and CSV; a reference page for the four libraries; ADR 0037; and `E_SQL_PARAM`, `E_SQL_ROW`, `E_CSV_TYPE` and `E_DB_SHADOW`.

### Changed
- The HTTP server's gzip encoding uses miniz (level 1) instead of a hand-written encoder; responses are as before.
- `sspur fuzz` skips the functions of the network and library preludes.
- The playground leaves out `Bytes` and the four libraries, which need the C runtimes, so its WebAssembly file stays at 972 KiB gzipped.

## [0.10.0] - 2026-10-15

### Added
- Source-level debugging with lldb and gdb (ADR 0036). `sspur build --debug`, `run --debug`, `test --debug` or `SSPUR_DEBUG=1` build native code with DWARF (CodeView on Windows) at `-O0` whose lines are the `.ssp` lines: breakpoints by `prog.ssp:12` and by function name work, the backtrace shows SSPUR functions and lines, and parameters and locals have their SSPUR names (`acc`, not `v_acc_$6`), with the generated temporaries hidden. `sspur debug prog.ssp [args]` builds and starts lldb (macOS) or gdb (Linux) with breakpoints on `main` and on `sspur_trap`, which every trap and contract failure calls. `sspur debug-init` writes lldb and gdb formatters that show `Str` as text, `Bool` as `true` or `false`, `List` elements, `Opt` as `some(3)` or `none` and sum values as `Circle{r: 2}`, plus a `sspur-locals` command. A codebase's projected source is written to the cache so the debugger can show it. Normal builds are unchanged: the same generated C and the same speed. The new Debugging page covers lldb, gdb, VS Code with CodeLLDB and Windows, and the VS Code extension allows breakpoints in `.ssp` files.
- Networking (ADR 0035): an HTTP client and an HTTP/1.1 server with TLS, behind a new `net` effect, the same in native code and the interpreter. `http.get(url)`, `http.post(url, body)` and `http.send(req)` return `Res[Response, HttpErr]` (`Dns`, `Connect`, `Tls`, `Timeout`, `HttpStatus`, `Io`); `http.fetch(url)` returns the body and raises for anything but a 2xx; `http.req(method, url)` gives a `Request` whose timeout, retries (with backoff and `Retry-After`) and redirects `with` changes; `http.download(req, path)` streams to a file; `r.header(name)` and `r.json[T]` read responses. Certificates are verified by default (`SSL_CERT_FILE` adds roots), proxies come from the environment and gzip is decoded. The client is libcurl on macOS and Linux and WinHTTP on Windows.
- `http.serve(port, handler)` and `http.serve_with(server, handler)` serve a function from `Request` to `Response`, or a `svc`, whose endpoints now also run in-process: path, query (GET and DELETE) and JSON body parameters bind as in a deployed service, and results and raised errors map to the same statuses. The `Server` record (`http.server(port)` and `with`) sets the host (`127.0.0.1` by default), worker threads, body limit, timeouts, CORS origins, gzip, static files, a health path, TLS certificate and key (OpenSSL 3), `max_requests` and the access log. Every request gets an id, connections are kept alive, and SIGINT or SIGTERM shuts down gracefully. `http.text`, `http.html` and `http.json(status, v)` build responses; `req.path`, `req.query(name)`, `req.header(name)` and `req.json[T]` read requests. Native handlers run on a thread pool, and the collector runs while none of them is running. A natively served 200-byte answer takes about 28,000 requests a second at concurrency 64 on an Apple M1 Pro.
- `config.load(defaults, path, prefix)` loads a record from a JSON file and `PREFIX_FIELD` environment variables, falling back to the defaults, with `where` clauses as validation.
- `log.debug`, `log.info`, `log.warn` and `log.error` write levelled lines; `SSPUR_LOG_LEVEL` drops lower levels and `SSPUR_LOG=json` prints each line as a JSON object with time, level and message.
- The handbook has a Networking chapter, and the reference a Networking page.

### Changed
- `svc` endpoints may take a `Request` or return a `Response`, bind GET and DELETE parameters from the query string, and perform any effect but `db.*` and user-defined ones when served with `http.serve`; `sspur deploy` still accepts only what Lambda supports and says so (`E_EP_EFFECT`, `E_EP_BODY`, `E_EP_TYPE`).
- SSPUR Pulse fetches with the HTTP client instead of running `curl`; its pages are unchanged except for the sentence that said so.
- The playground's WebAssembly file is 972 KiB gzipped, below 0.9.0's 982 KiB (networking had taken it to 1,025 KiB): it leaves out the generator of svc dispatchers, since the playground can't serve, keeps one copy of the builtin signature tables instead of two, and is built at opt-level 2, which also runs slightly fewer instructions.
- On macOS `sspur` is linked with chained fixups, so the dynamic linker no longer writes every pointer in the binary at launch: the cached `sspur run` of hello world peaks at 2.45 MiB instead of 2.56 MiB.

## [0.9.0] - 2026-10-14

### Added
- SSPUR Pulse (`pulse/`), a live dashboard of the world today at `/pulse/` on the docs site: eight boards (Wikipedia reading, space launches with a globe of the satellites in orbit, earthquakes, rising GitHub repos, a Hacker News time machine, AI papers, world weather, public holidays) fetched, checked and rendered by SSPUR programs every 6 hours by the Pages workflow, with snapshots kept on a `pulse-data` branch. Every page works without JavaScript; a small script adds chart toggles, tooltips, keyboard reading, the weather where you are (from your IP or, if you allow it, your device) and a few hidden secrets. Each board falls back to its last good data when a fetch fails and publishes JSON; the quakes, papers and reading boards publish RSS, the launches and holidays boards ICS calendars. The docs page "SSPUR Pulse: how it works" describes the design, and CI runs the Pulse tests on bundled fixtures.

### Changed
- The docs, README and benchmark pages are current for 0.8 and 0.9: native speed and memory, the large-codebase agent results, Pulse, `while e is p`, the cached run and the Zed extension.

### Fixed
- Native `json.decode` allocated every decoded string with room for the rest of the input, so decoding a large array was quadratic in memory: 1.3 MB of 20,000 small records hit the 8 GB heap ceiling and stopped with "out of memory". Each string now gets room for its own source text. The same decode takes 0.02s and 19 MB of heap (the interpreter 0.06s), and Pulse's section programs run natively again.
- `g.OBJECT_NAME`, a field whose name starts with a capital, parsed as the package-qualified name `g.OBJECT_NAME` and failed with `unknown name`, so records decoded from JSON with upper-case keys could only be read by destructuring. `x.NAME` in an expression is now a field access, and `pkg.Name` still resolves to the package's name when `pkg` is a dependency and not a local.

## [0.8.2] - 2026-10-14

### Added
- Native code frees list buffers and strings at their last use when the compiler can prove they have one owner (ADR 0033), in front of the collector, which still reclaims everything else. Locals and temporaries that are only read are freed after the last read, a `var` frees its old value when it gets a new one, a parameter a function only reads is borrowed so the caller frees a fresh argument after the call, a list of strings built from fresh strings frees them with it, a `Map.put` that takes an owned key frees it when the key was already there, and `var v = []` that is only pushed onto and then joined is built as a string directly. Freed blocks are reused by the next allocation of their size. Peak memory, 0.8.1 to 0.8.2 (C++): `simd` 273 to 39 MB (40), `churn` 268 to 7 MB, `strings_big` 92 to 29 MB (23), `app` 137 to 22 MB (24, with the fused fold below), `gpu` 344 to 97 MB (54, with the GPU changes below); every benchmark is as fast as before or faster (`simd` 0.20s to 0.18s, `churn` 0.08s to 0.03s).
- `sspur run file.ssp` of a program that hasn't changed runs the cached native library's `main` directly, without parsing, checking or generating code (ADR 0034). The run is keyed by the source, the `sspur` binary, the `SSPUR_*` variables and the compiler's environment, checks the cached library's contents, and falls back to the normal path on any difference, inside a package, with any flag (`--no-cache` forces it) or when the program has warnings to print. A trap or raise in `main` is described by the normal path, so the output and exit code are the same. Hello world takes 10.3 ms instead of 12.7 ms (C++ 8.0 ms), each benchmark 3 to 11 ms less, and peak memory is 4 to 5 MB lower; `compute_big` is within 1% of C++ (1.02s against 1.01s).
- `SSPUR_MEM=gc` turns the frees off (the 0.8.1 behavior) and `SSPUR_MEM=check` poisons freed blocks and stops on a double free. `SSPUR_GC_STATS=1` also reports the blocks freed at last use and the blocks left for the collector. `SSPUR_GC_MIN_MB` and `SSPUR_GC_FACTOR` tune when the collector runs (defaults 256 and 4).
- `SSPUR_CFLAGS` adds C compiler flags to native builds and their cache key, for example `-fsanitize=address,undefined`.

### Changed
- The interval analysis of ADR 0007 sees locals again. Since per-definition translation units, local names contain a `$` and were ignored, so `let`, `var` and `for` facts, and the proofs of a callee's contract from computed arguments, never applied; the in-place map update after a lookup of the same key (ADR 0008) had the same problem. `app` is 14% faster.
- `a / b` and `a % b` with `a` proven non-negative and `b` proven positive compile to unsigned division, and to a shift or a mask for a power of two.
- A small `if` over `Int` whose arms have overflow checks compiles to a select of both arms, computed with wrapping arithmetic and exact overflow flags, instead of a branch; the chosen arm traps exactly as before. With the two changes above, `compute_big` takes 0.91s instead of 1.02s (C++ 1.02s), `compute` 14% less, `parallel` 7% and `simd` 5%.
- Native libraries are linked with dead stripping on macOS, which drops the runtime functions a program never reaches and the libc imports only they need. `sspur run` of an unchanged hello world takes 9.6 ms instead of 10.6 ms.
- `xs.map(f).fold(init, g)` and `xs.filter(p).fold(init, g)` run as one native loop, over blocks of 64 elements, when `g` has no effects and captures nothing: the mapped list is never built. A trap in `g` is held until `f` has run on every element, so the program fails exactly as the unfused order does. `app` peaks at 22 MB instead of 41 MB (C++ 24 MB) and is about 2% faster.
- `dev_f32((a..b).map(f))` and the other `dev_*` constructors of a mapped range write each value straight into the device buffer instead of building a host list first, and `d.to_list.sum` sums the buffer without building the list. The GPU launch queue no longer clears each new mask of flagged threads (Metal zero-fills new buffers), which kept 512 KB resident per queued launch. `gpu` peaks at 97 MB instead of 178 MB and is about 7% faster; what remains is the device buffers, the host copies that make CPU reruns exact, and Metal.
- On macOS, native code uses `_setjmp` and `_longjmp`, which don't save the signal mask with a system call, so each call from the interpreter into native code is about 0.3 microseconds cheaper.
- `text.lower.words.counts`, written without parentheses, now counts while it scans, as ADR 0008 describes, instead of building the whole word list first; non-ASCII text is detected during the scan instead of in a pass before it.
- Interpolation and string building are about 7% faster: a string builder's growth is in a separate cold function, and it frees the buffer it outgrew.
- The runtime frees its own temporaries: sort buffers, `sort_by` key pairs and `counts` hash tables.

### Fixed
- `split` on a one-byte separator allocated a buffer and threw it away.
- The allocator's fast path did pointer arithmetic on a null cursor, which UndefinedBehaviorSanitizer reports.

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

[Unreleased]: https://github.com/sspurlang/sspur/compare/v0.14.0...HEAD
[0.14.0]: https://github.com/sspurlang/sspur/compare/v0.13.1...v0.14.0
[0.13.1]: https://github.com/sspurlang/sspur/compare/v0.13.0...v0.13.1
[0.13.0]: https://github.com/sspurlang/sspur/compare/v0.12.1...v0.13.0
[0.12.1]: https://github.com/sspurlang/sspur/compare/v0.12.0...v0.12.1
[0.12.0]: https://github.com/sspurlang/sspur/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/sspurlang/sspur/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/sspurlang/sspur/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/sspurlang/sspur/compare/v0.8.2...v0.9.0
[0.8.2]: https://github.com/sspurlang/sspur/compare/v0.8.1...v0.8.2
[0.8.1]: https://github.com/sspurlang/sspur/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/sspurlang/sspur/compare/v0.7.1...v0.8.0
[0.7.1]: https://github.com/sspurlang/sspur/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/sspurlang/sspur/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/sspurlang/sspur/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/sspurlang/sspur/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/sspurlang/sspur/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/sspurlang/sspur/compare/v0.3.2...v0.4.0
[0.3.2]: https://github.com/sspurlang/sspur/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/sspurlang/sspur/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/sspurlang/sspur/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/sspurlang/sspur/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/sspurlang/sspur/releases/tag/v0.2.0
[0.1.0]: https://github.com/sspurlang/sspur/tree/6c55224
