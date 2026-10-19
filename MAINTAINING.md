# Maintaining SSPUR

The runbook for whoever looks after SSPUR, its repositories and its sites: the rules every change
follows, how to check a change against them, how to release, how to run the registry, and what to
do when something breaks. [CONTRIBUTING.md](CONTRIBUTING.md) covers building, testing and sending
a pull request; this page starts where it stops. Coding agents follow [AGENTS.md](AGENTS.md), which points here.

## Tenets

1. **Nothing that is a strength gets worse.** Speed (`check`, `fmt`, `run`, `test`, the native
   benchmarks, checking in the playground), memory (peak RSS of every command, failure cases
   included), startup, the playground's download and the agent spec's token count are measured
   before and after every change that could move them. A feature that costs something by default
   is made opt-in, or its cost is removed, before it merges.
2. **1.x never breaks working code.** The language, the standard library, the stable commands, the
   package and lock formats, content hashes and error codes are frozen
   ([stability policy](docs/reference/stability.md)). The compatibility suite (`tests/compat`)
   enforces it; a feature goes away only after a `W_DEPRECATED` release, and only in 2.0.
3. **No existing flow is affected.** Publishing packages, installing, CI, permissions and links
   keep working through every change, including repository moves and renames.
4. **Zero cost and zero upkeep.** Everything runs on GitHub's free tier for public repositories:
   no servers, no secrets beyond `GITHUB_TOKEN`, no paid services. Scheduled jobs keep themselves
   alive (see Workflows).
5. **Everything is public and verifiable.** Releases are signed with build provenance and an SBOM;
   numbers in docs and release notes are measured, not estimated, and say what they were measured
   against.
6. **Plain writing.** Docs, release notes and commit messages are plain human prose: short
   sentences, no emdashes, no marketing. Commits are one line, authored by the maintainer, with no
   AI attribution or `Co-Authored-By` trailers.

## The repositories

| Repository | What it holds | Deploys |
|---|---|---|
| [sspurlang/sspur](https://github.com/sspurlang/sspur) | The compiler, tools, docs, playground, Pulse, packaging, the registry template | Docs, playground and Pulse to [sspurlang.github.io/sspur](https://sspurlang.github.io/sspur/) |
| [sspurlang/sspur-packages](https://github.com/sspurlang/sspur-packages) | The package index, the publish workflows, the registry site | [sspurlang.github.io/sspur-packages](https://sspurlang.github.io/sspur-packages/) |
| [sspurlang/sspurlang.github.io](https://github.com/sspurlang/sspurlang.github.io) | The landing page, built by `build.ssp` | [sspurlang.github.io](https://sspurlang.github.io/) |
| [sspurlang/homebrew-sspur](https://github.com/sspurlang/homebrew-sspur) | The Homebrew tap (`brew install sspurlang/sspur/sspur`) | |
| [sspurlang/sspur-grammar](https://github.com/sspurlang/sspur-grammar) | The TextMate grammar, copied weekly from `packaging/vscode/syntaxes/` | |
| [sspurlang/.github](https://github.com/sspurlang/.github) | The organization's profile README | |
| `sspurlang/sspur-*` | Published packages, one repository each | |

`registry/packages-template/` in the main repository is the source of the registry's tools,
workflows and site. Change it there, then copy it into a clone of `sspur-packages` (everything but
`packages/` and `index.json`, for example with `rsync -a --exclude packages --exclude index.json`)
and merge that as a maintainer pull request. The two must never drift.

The three sites share one header, menu, footer, font and theme button. A change to one is made to
all three, previewed locally on a phone and a desktop, and checked in Chromium and WebKit, light
and dark, before it is pushed.

## Checking a change

Before every push to `main`, run the full check on the exact commit being pushed:

```
tools/verify.sh
```

It runs clippy (native, Windows and both WebAssembly builds), the whole test suite, the corpus
check, two fuzz seeds, Pulse's tests, the docs, playground and Pulse builds, the link check and the
playground's API test. Every line must be clean: `tests: passed N failed 0`, `199/199 programs
identical`, `0 findings`, `0 errors or notes`, `0 broken`. Then build for Windows once
(`cargo build --release --target x86_64-pc-windows-gnu`) and, for playground changes, run the
browser suite: `node tools/playground_ui_test.mjs docs/book` (Chromium, Firefox and WebKit,
desktop and two phones, light and dark).

Two tests are known to be sensitive to an overloaded machine: the deploy hot-swap test and the
bare-metal timer test. A failure in one of them is rerun alone before anything else is concluded,
and a repeat failure is a bug.

### Measuring performance

- **Interleave.** Alternate the old and new binaries run by run, at least 80 runs each, and compare
  medians. Never compare two batches run one after the other.
- **Copy each binary twice.** Endpoint security software can slow a binary it hasn't seen before;
  two copies of the same file show how much of a difference is the machine, not the code.
- **CPU time over wall time.** On a loaded machine, compare user plus system time (`wait4`) and
  peak RSS (`/usr/bin/time -l`), not elapsed time.
- **The baseline is the previous `main`** for regressions, and the last release for release notes.
- **The playground's size** is the gzip -9 size of `pkg/sspur_wasm_bg.wasm`, built on the same
  machine for both sides. `tools/build_playground.sh` prints it and enforces the budget.
- **The macOS binary's layout** is ordered by `tools/order/hot.txt` (ADR 0043). Regenerate it with
  `tools/order/gen.sh` when a hot command's code path changes, and check RSS again.

### Changing the language or the tools

- A change of behavior gets a CHANGELOG line under `[Unreleased]`, and an ADR in `docs/adr/` when
  it is a lasting decision.
- Code in the docs lives in `docs/snippets/`, and the tutorial test runs it.
- If `tests/compat` fails, the change broke the 1.x promise: change the code, not the golden. A
  golden is blessed (`SSPUR_COMPAT_BLESS=1 cargo test --release -p sspur-cli --test compat`) only
  for a documented addition, such as a new warning, and the commit says so.
- A change to `pulse/lib` needs `pulse/relock.sh`, or every section warns that its lock is stale.
- Code that 0.14 read differently belongs in `sspur fix` and the
  [migration guide](docs/get-started/migrating-to-1.0.md).

## Releasing

1. **Plan it.** Decide what goes in, write the release notes, and check every claim in them against
   a measurement.
2. **Bump the version** in the root `Cargo.toml`, `Cargo.lock` (`cargo update --workspace`), the
   VS Code extension (`packaging/vscode/package.json` and its lock file), the Zed extension
   (`packaging/zed/`, its `Cargo.lock` and the tree-sitter package), the Claude Code plugin,
   `registry/packages-template/.sspur-version`, and the version in the README, the docs index,
   SECURITY.md, `docs/config/environment.md`, `docs/get-started/editors.md` and
   `docs/snippets/get-started/install-other.sh`. `git grep` the old version afterwards. Move
   `[Unreleased]` in CHANGELOG.md under the new version and add its compare link.
3. **Verify, push to `main`, and wait for CI** to pass on that exact commit. CI doesn't run again
   for the tag. Push nothing else to `main` until the release is published.
4. **Tag and push**: `git tag -a vX.Y.Z -m vX.Y.Z && git push origin vX.Y.Z`. `release.yml` checks
   that CI passed on the commit, builds the six targets, the extensions, the browser checker,
   `SHA256SUMS`, the installers, the Homebrew formula, the Scoop manifest and the SBOM, creates a
   draft release, and then the `attest` job signs every file.
5. **Check the draft** once every job, `attest` included, is green: download an archive and verify it
   with `shasum -a 256 -c SHA256SUMS --ignore-missing` and
   `gh attestation verify FILE --repo sspurlang/sspur --signer-workflow sspurlang/sspur/.github/workflows/release.yml --source-ref refs/tags/vX.Y.Z`.
   Run the release binary against the previous release (startup, memory) and test every published
   package with it.
6. **Publish** the draft with the notes, as Latest.
7. **Homebrew**: copy the release's rendered `sspur.rb` into `Formula/sspur.rb` of the tap, commit
   `sspur X.Y.Z` and push. Check with `brew info sspurlang/sspur/sspur` and `brew fetch`.
8. **The registry**: once every published package passes under the new version, set
   `.sspur-version` in `sspur-packages` to it through a maintainer pull request.

The formula for homebrew-core is `packaging/homebrew/core/sspur.rb`; update its `url` and `sha256`
(the source tarball of the tag) with each release. Submit it to Homebrew/homebrew-core only once
SSPUR meets Homebrew's notability bar (about 75 stars, or 30 forks or watchers, and outside use);
a premature submission is closed publicly.

## Running the registry

How publishing works for authors is in the registry's
[CONTRIBUTING.md](registry/packages-template/CONTRIBUTING.md). As a maintainer:

- **New package names wait for you.** Read the request's pull request (the checks, the effects, the
  API) and comment `/approve` on the request. Approval is bound to the commit shown; a moved tag
  needs a new approval. Later versions of an approved name need none.
- **Stuck requests.** Comment `/recheck` on the request to run it again. The hourly sweep reruns
  requests whose ref moved or whose run GitHub cancelled.
- **Yanks and deprecations** come from owners (`sspur yank NAME@VERSION`, `--deprecate MSG`).
- **Changes to the registry itself** (workflows, checks, schema, site) are made in
  `registry/packages-template/`, tested with `cargo test --release -p sspur-cli --test registry`,
  copied to `sspur-packages` and merged as a maintainer pull request
  (`gh pr merge N --squash --admin` once the guard passes).

### Maintaining a package

The packages under `sspurlang/sspur-*` follow the same rules as everyone else's. To ship a fix: make
the change, run `sspur test` and `sspur test --interp` (both must pass), `sspur fmt` and
`sspur publish --dry-run`; bump the version (a patch for a fix, a minor release for an addition,
which `publish --dry-run` tells you); commit, tag `vX.Y.Z` (a lightweight tag, or set your git
identity first), push both, and run `sspur publish`. A package should run on the oldest SSPUR the
registry still checks with, so avoid new standard library calls in a patch release.

## Workflows and schedules

| Repository | Workflow | When | If it stops |
|---|---|---|---|
| sspur | CI | Every push and pull request | Merges and releases wait for it |
| sspur | Pages | Every push, every 3 hours (`17 */3`), by hand | Pulse's data and the docs stop refreshing |
| sspur | Release | Tag `v*` | No release |
| sspur-packages | Publish, Guard | Requests, comments, pull requests | Publishing stops |
| sspur-packages | Sweep | Hourly (`41 *`) | Cancelled requests aren't retried; `/recheck` still works |
| sspur-packages | Pages | Every push | The registry site goes stale |
| sspurlang.github.io | Pages | Every push, hourly (`41 *`), by hand | The landing page's packages go stale |
| sspur-grammar | Sync | Weekly | The grammar copy falls behind |

GitHub pauses scheduled workflows after 60 days without a commit. Each scheduled workflow has a
`keepalive` job, run by a weekly schedule entry, that enables the workflow again through the API and
resets that clock; on every other run it shows as skipped, which is expected. To restart a paused
schedule by hand, click "Enable workflow" on the repository's Actions tab.

Scheduled runs can start hours late, or not at all, when GitHub is busy. That is why Pulse runs every
3 hours with a 2.5 hour minimum per board, so a delayed run still finds its boards due.

## SSPUR Pulse

Pulse ([how it works](docs/pulse-how-it-works.md)) is ten SSPUR programs under `pulse/`, one per
board, built by the Pages workflow. Each board keeps its last good data on the `pulse-data` branch
and refreshes when it is due (`pulse.due`, minutes per board). A board shows "Last good data" when
its source failed and it kept the previous snapshot.

- **Sources publish late.** Wikimedia's daily most-read list arrives one to several days after the
  day; the reading board looks back a week and is up to date, not stale, when nothing newer exists.
  A board that depends on a source's delay must look back far enough and treat "nothing newer yet"
  as success.
- **Respect the sources.** CelesTrak asks for one download every two hours at most, so the orbit
  board's minimum interval stays above that.
- **After changing `pulse/lib`**, run `pulse/relock.sh`, then `pulse/test.sh`.

## Security

- `main` and release tags of `sspur` are protected by rulesets; `sspur-grammar`'s `main` blocks
  force pushes and deletion. Secret scanning is on where GitHub offers it.
- Workflows run with `contents: read` by default and ask for more per job; only `attest` can mint an
  OIDC token.
- Reports follow [SECURITY.md](SECURITY.md); fixes go into the latest 1.x release.

## When something breaks

- **CI is stuck for hours.** Look at which step it is in. A runner that is still compiling after an
  hour is a slow or broken runner: cancel the run (its partial log becomes readable), then
  `gh run rerun ID --failed` to rerun only that job.
- **A Pulse board is stale.** Read the latest Pages run's log for the board's lines
  (`gh run view ID --log`): a source returning errors, a changed API, or a source publishing late.
  Fix the board so the case is handled, not just retried.
- **The landing page or registry site is stale.** Check that their Pages workflow still runs; if
  GitHub paused it, enable it.
- **A publish request is stuck.** Read the request's comment and its pull request; `/recheck` once
  the cause is fixed.
- **A release job failed.** Fix it on `main`, delete the draft release and the tag
  (`git push origin :refs/tags/vX.Y.Z`), and release again from a commit CI passed on.

## Waiting for later

- **homebrew-core**: when SSPUR reaches about 75 stars; the formula is ready.
- **GitHub language recognition** (Linguist): when about 100 people outside sspurlang use SSPUR. The
  grammar is in `sspurlang/sspur-grammar`; `.ssp` is also used by Scala Server Pages, so the
  submission needs SSPUR to be the main use of the extension.
- **Smaller items**: the hot-path order file for Linux and Windows, the conversion-proof cache in the
  language server, `W_DEPRECATED` for command-line flags, and highlighters that differ from the lexer
  on rare brace forms in strings.
