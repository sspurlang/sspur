# Instructions for coding agents

You are working on SSPUR, a programming language and its toolchain (Rust, in `crates/`), with its
docs, playground, Pulse and the registry template. Read these before you change anything:

- [CONTRIBUTING.md](CONTRIBUTING.md): setup, building, testing, docs, commit style.
- [MAINTAINING.md](MAINTAINING.md): the tenets, how to check and measure a change, releasing, the
  registry, the sites, Pulse, and what to do when something breaks.
- [docs/agent/agent-spec.md](docs/agent/agent-spec.md): the language, for writing `.ssp` code.

## Rules that are never bent

1. **Nothing that is a strength gets worse**: speed, memory, startup, the playground's download,
   the agent spec's token count. Measure before and after, as MAINTAINING.md describes, and report
   the numbers. A change that costs something by default is made opt-in or reworked.
2. **1.x never breaks working code.** If `tests/compat` fails, fix the code, not the golden. Never
   change canonical text or content hashes of existing programs. Remove nothing stable; deprecate
   with `W_DEPRECATED` first.
3. **No existing flow breaks**: publishing, installing, CI, links, permissions.
4. **Never weaken a test to make it pass**, and never mark a failure as flaky without running the
   test alone and saying so.

## How to work on an issue

1. Reproduce it with a failing test first, in the crate's tests, `tests/programs/` or
   `docs/snippets/` as fits.
2. Fix it at the root cause, and look for the same mistake elsewhere.
3. Run `tools/verify.sh` on the final commit. Every line must be clean. For playground changes also
   run `node tools/playground_ui_test.mjs docs/book`.
4. Add a line under `[Unreleased]` in CHANGELOG.md when users would notice the change; update the
   docs, and an ADR for a lasting decision.
5. Commit with one-line messages in the style of `git log`, under the maintainer's name. Add no AI
   attribution and no `Co-Authored-By` trailers.
6. Open a pull request with what you changed, why, and the before and after numbers. Never push to
   `main` or create tags yourself; the maintainer merges and releases.

## Releases

Only when the maintainer asks. Follow "Releasing" in MAINTAINING.md step by step, check every claim
in the release notes against a measurement, and stop for the maintainer's approval before merging
the release commit and before pushing the tag.

## Don't

- Edit the live registry repository directly: change `registry/packages-template/` and copy it.
- Commit screenshots, build output, downloaded data or other generated files.
- Add a dependency, a service, a secret or anything that costs money or needs upkeep.
- Use emdashes or marketing language in docs, notes or messages. Write plain, short sentences.
