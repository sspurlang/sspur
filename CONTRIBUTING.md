# Contributing to SSPUR

Thanks for taking the time. Bug reports, design questions and patches are all welcome as GitHub issues and pull requests. For anything large, open an issue first so we can agree on the shape before you write the code.

## Setup

- Rust 1.88 or newer (`rustup` is easiest)
- `clang` for the native compiler
- `z3` for `sspur verify` and proven check elimination
- Optional: `qemu` and `lld` for `profile bare` kernels, Homebrew `llvm` for SPIR-V and PTX output

On macOS: `brew install z3 qemu llvm lld`. On Ubuntu: `sudo apt install clang lld llvm z3 libcurl4-openssl-dev`.

Tests that need QEMU, Metal or a GPU backend skip themselves when the tool is missing, so a machine with only clang and z3 runs the rest of the suite.

## Build and test

Every change has to pass all of these, the same commands CI runs:

```
cargo build --release
cargo test --release
cargo clippy --release --all-targets -- -D warnings
python3 tools/corpus_diff.py
```

The test suite is heavy (it compiles a lot of C). If memory is tight, limit parallelism with `CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=2`.

## The corpus check

`tools/corpus_diff.py` takes every program in `bench/eval/corpus.jsonl`, adds its hidden tests, and runs it in the interpreter and as native code. It passes only when every program gives identical output in both tiers, every function compiles natively, and no native build fails:

```
199/199 programs identical; 270/270 functions native; 0 native build failures
```

It uses `target/release/sspur` by default; set `SSPUR_BIN` to check another binary. A change that makes native output differ from the interpreter is a bug in the native compiler, not in the test.

## Documentation

The site is an mdBook in `docs/` (`docs/book.toml`). Build it and check its links with:

```
mdbook build docs
PULSE_FIXTURES=pulse/fixtures PULSE_OUT=docs/book/pulse PULSE_DATA=$(mktemp -d) pulse/run.sh
python3 tools/check_site_links.py docs/book
```

The second line builds [SSPUR Pulse](docs/pulse-how-it-works.md) into the site from its bundled samples, as CI does, so the links to `/pulse/` resolve; `pulse/test.sh` runs its tests.

The playground (`docs/play/` and `crates/sspur-wasm`) is built into the site by `tools/build_playground.sh`, after `mdbook build docs`. It needs the `wasm32-unknown-unknown` target and `wasm-bindgen-cli` at the version in `Cargo.lock`, which the script installs if they're missing, and npm for the editor bundle; `wasm-opt` is used when it's on the `PATH`. `node tools/playground_test.mjs` runs every example through the wasm build, and with `SSPUR_BIN` set it compares the results with `sspur run --interp`. To try the page, serve `docs/book` with `python3 -m http.server` and open `/play/`.

Code shown in the docs lives in `docs/snippets/`, never inline. The tutorial test (`cargo test --release -p sspur-cli --test tutorial`) runs every transcript (`*.out`) there against the compiler, fails if a page in `get-started/`, `handbook/`, `tutorials/`, `config/` or the cheat sheet has a code block that isn't an include of a snippet, if a `.ssp` snippet is never run, and if `docs/reference/errors.md` misses a diagnostic code the compiler can emit. When you change compiler output, update the transcripts it breaks.

## Verification

- `sspur verify file.ssp` proves contracts with Z3.
- `sspur fuzz --differential file.ssp` compares native code against the interpreter on generated inputs.
- `sspur explain-opt file.ssp` shows which proven rewrites were applied.

Run these on anything you touch in `sspur-native`, `sspur-smt` or the optimizer.

## Design records

Decisions with lasting impact get an ADR in `docs/adr/`. If your change alters behavior described in one, update it or add a new one, and keep `docs/reference/language.md` and `docs/agent/agent-spec.md` in sync.

## Commits

- One-line commit messages in the imperative or descriptive style already in the log, for example `Std: Complex type with arithmetic, polar form and the <complex> functions`.
- Prefix with the area when it helps (`Native:`, `Std:`, `GPU:`, `Docs:`).
- One logical change per commit. Keep comments in code short; put the reasoning in the pull request.

## Releasing

1. Bump `version` in the root `Cargo.toml`, run `cargo build --release` to refresh `Cargo.lock`, and move the `Unreleased` notes in `CHANGELOG.md` under the new version. The VS Code and Zed extensions and the Claude Code plugin (`packaging/claude-code/.claude-plugin/plugin.json`) carry the same version; `cargo test --test cli` fails until they do.
2. Push that commit to `main` and wait for CI to pass on it. CI doesn't run again for the tag.
3. Tag and push: `git tag -a v1.0.0 -m v1.0.0 && git push origin v1.0.0`.
4. `release.yml` first checks that CI passed on the tagged commit and stops if it didn't. Then it builds the six targets (macOS, Linux and Windows, each on arm64 and x86_64), packages the VS Code extension, the LSP4IJ template and the Zed extension, writes `SHA256SUMS`, renders the Homebrew formula with `packaging/homebrew/render.sh VERSION SHA256SUMS` and the Scoop manifest with `packaging/scoop/render.sh`, writes the CycloneDX SBOM `sspur-vX.Y.Z.cdx.json` with `cargo cyclonedx`, and creates a draft release with all of them attached. Its last job, `attest`, signs build provenance for every attached file and the SBOM for the six archives, then checks them with `gh attestation verify`.
5. Wait for the whole run to pass, `attest` included, then review the draft and publish it, then copy the rendered `sspur.rb` into `Formula/sspur.rb` of the `sspurlang/homebrew-sspur` tap.

## License

By contributing you agree that your work is dual-licensed under MIT or Apache-2.0, as described in the README.
