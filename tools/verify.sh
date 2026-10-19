#!/usr/bin/env bash
# The full pre-push check of the working tree: what CI runs and more. Run it from the repository
# root before every push to main (MAINTAINING.md). The steps that only read the built binary run
# side by side and print in a fixed order. Needs clang, z3, mdbook, node, wasm-bindgen (the
# playground build installs it) and, for the Windows clippy, the x86_64-pc-windows-gnu target.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-8}" RUST_TEST_THREADS="${RUST_TEST_THREADS:-2}"
git log --oneline -1
cargo clippy --release --all-targets -q -- -D warnings
cargo clippy --release -q --target x86_64-pc-windows-gnu -p sspur-cli -- -D warnings
cargo clippy --release -q --target wasm32-unknown-unknown -p sspur-wasm -- -D warnings
cargo clippy --release -q --target wasm32-unknown-unknown -p sspur-wasm --features registry -- -D warnings
cargo build --release 2>&1 | tail -1
SSPUR_FILL_MS=5000 SSPUR_STRICT_NATIVE=1 cargo test --release --no-fail-fast 2>&1 |
  awk '/test result/{split($0,a," "); p+=a[4]; f+=a[6]} /FAILED|panicked/{print} END{print "tests: passed",p,"failed",f}'
B="$(cd "${CARGO_TARGET_DIR:-target}/release" && pwd)/sspur"
L="$(mktemp -d)"
(SSPUR_BIN="$B" SSPUR_STRICT_NATIVE=1 python3 tools/corpus_diff.py 2>&1 | tail -1) > "$L/1" &
("$B" fuzz --gen 200 --seed 2026 2>&1 | tail -1) > "$L/2" &
("$B" fuzz --gen 200 --seed 99 2>&1 | tail -1) > "$L/3" &
( (cd pulse && SSPUR="$B" ./test.sh > "$L/pulse.out" 2>&1) || true
  echo "pulse: $(grep -c "0 failed" "$L/pulse.out") sections passing, $(grep -cE "E_[A-Z_]+|note:" "$L/pulse.out") errors or notes") > "$L/4" &
( mdbook build docs > /dev/null 2>&1 && tools/build_playground.sh 2>&1 | tail -1
  SSPUR="$B" PULSE_FIXTURES=pulse/fixtures PULSE_OUT=docs/book/pulse PULSE_DATA="$(mktemp -d)" pulse/run.sh 2>&1 | tail -1
  python3 tools/check_site_links.py docs/book 2>&1 | tail -1
  SSPUR_BIN="$B" node tools/playground_test.mjs 2>&1 | tail -1 ) > "$L/5" 2>&1 &
wait
cat "$L/1" "$L/2" "$L/3" "$L/4" "$L/5"
rm -rf "$L"
