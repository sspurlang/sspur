#!/usr/bin/env bash
# Builds the browser playground into docs/book/play, or the directory given: the WebAssembly
# build of the checker and interpreter, the bundled editor, and examples from docs/snippets.
# Run it after `mdbook build docs`, which copies the page itself from docs/play. With
# --no-editor it skips the editor bundle, which needs npm; the API smoke test doesn't use it.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
editor=1
if [ "${1:-}" = "--no-editor" ]; then
  editor=0
  shift
fi
out="${1:-$root/docs/book/play}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"

# The examples menu, in order: snippet files the docs already test.
examples=(
  get-started/hello
  get-started/shapes
  handbook/matching
  handbook/records
  handbook/collections
  handbook/errors
  handbook/traits
  handbook/concurrency
  get-started/from-python=get-started/words.txt
  handbook/missing
)

npm_pkgs=(
  @codemirror/view@6.43.14
  @codemirror/state@6.7.6
  @codemirror/language@6.13.1
  @codemirror/commands@6.11.1
  @codemirror/autocomplete@6.20.3
  @codemirror/lint@6.9.7
  @codemirror/search@6.7.2
  @lezer/highlight@1.2.5
  esbuild@0.28.2
)

cd "$root"

rustup target list --installed | grep -qx wasm32-unknown-unknown || rustup target add wasm32-unknown-unknown
want="$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p')"
have="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)"
if [ "$have" != "$want" ]; then
  cargo install wasm-bindgen-cli --version "$want" --locked
fi

# The interpreter keeps its frames on the shadow stack in linear memory, about 1.5 KiB a call,
# so give it 32 MiB rather than the 1 MiB default: enough for 20,000 small calls, the limit of
# `sspur run --interp`. A program that doesn't recurse deeply never touches most of it.
# opt-level 2 rather than the release profile's 3: the wasm is about 3% smaller gzipped, and
# the interpreter, checker and formatter run slightly fewer instructions under Node. The checker
# is built for size: about 2% smaller gzipped, and it runs once per edit, not per call.
CARGO_PROFILE_RELEASE_OPT_LEVEL=2 \
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C link-arg=-zstack-size=33554432" \
  cargo build --release -p sspur-wasm --target wasm32-unknown-unknown --config 'profile.release.package.sspur-check.opt-level="s"'

rm -rf "$out/pkg"
wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/sspur_wasm.wasm"
wasm="$out/pkg/sspur_wasm_bg.wasm"
if command -v wasm-opt >/dev/null; then
  # The asyncify pass lets an interpreter call move the frames under it off the browser's small
  # stack (crates/sspur-eval/src/hop.rs). Only sspur_hop unwinds, and never through an indirect
  # call. It runs before -O3: in the other order, Node 20 (an older V8 than current browsers have)
  # computes wrong results once it optimizes the interpreter. --low-memory-unused lets -O3 fold
  # constant offsets into loads and stores (smaller and no slower): the only accesses it changes
  # are ones whose address wraps around below 1 KiB, which Rust never makes; the stack comes
  # first in memory, and its last KiB is reached only as sp plus an offset, which doesn't wrap.
  wasm-opt --asyncify --pass-arg='asyncify-imports@*.__wbg_sspur_hop_*' --pass-arg=asyncify-ignore-indirect -O3 --low-memory-unused \
    --enable-bulk-memory --enable-sign-ext --enable-mutable-globals --enable-nontrapping-float-to-int \
    --enable-reference-types --enable-multivalue "$wasm" -o "$wasm.opt"
  mv "$wasm.opt" "$wasm"
else
  echo "wasm-opt not found; shipping the wasm without it"
fi

npm_dir="$root/target/playground-npm"
if [ "$editor" = 1 ]; then
  stamp="${npm_pkgs[*]}"
  if [ "$(cat "$npm_dir/.stamp" 2>/dev/null)" != "$stamp" ]; then
    rm -rf "$npm_dir"
    mkdir -p "$npm_dir"
    echo '{"private": true}' > "$npm_dir/package.json"
    npm install --prefix "$npm_dir" --no-audit --no-fund --loglevel=error "${npm_pkgs[@]}"
    echo "$stamp" > "$npm_dir/.stamp"
  fi
  NODE_PATH="$npm_dir/node_modules" "$npm_dir/node_modules/.bin/esbuild" docs/play/editor.js \
    --bundle --minify --format=esm --log-level=warning --outfile="$out/editor.bundle.js"
fi

for f in index.html play.css main.js worker.js share.js; do
  [ "$root/docs/play/$f" -ef "$out/$f" ] || cp "docs/play/$f" "$out/$f"
done

python3 - "$out/examples.json" "${examples[@]}" <<'EOF'
import json, os, sys
items = []
for entry in sys.argv[2:]:
    # An entry is a snippet, optionally `=` a file to put in the Input box.
    name, _, given = entry.partition("=")
    path = os.path.join("docs", "snippets", name + ".ssp")
    with open(path, encoding="utf-8") as f:
        lines = [l for l in f.read().split("\n") if not l.lstrip().startswith(("// ANCHOR:", "// ANCHOR_END:"))]
    item = {"name": name.split("/")[-1].replace("-", " "), "file": path, "source": "\n".join(lines)}
    if given:
        with open(os.path.join("docs", "snippets", given), encoding="utf-8") as g:
            item["input"] = g.read()
    items.append(item)
with open(sys.argv[1], "w", encoding="utf-8") as f:
    json.dump(items, f)
EOF

# Every docs snippet, so a docs code block that shows part of a file can open the whole file.
python3 - "$out/snippets.json" <<'EOF2'
import glob, json, sys
files = {}
for path in sorted(glob.glob("docs/snippets/**/*.ssp", recursive=True)):
    with open(path, encoding="utf-8") as f:
        files[path] = "\n".join(l for l in f.read().split("\n") if not l.lstrip().startswith(("// ANCHOR:", "// ANCHOR_END:")))
with open(sys.argv[1], "w", encoding="utf-8") as f:
    json.dump(files, f, separators=(",", ":"))
EOF2

raw=$(wc -c < "$wasm" | tr -d ' ')
gz=$(gzip -9 -c "$wasm" | wc -c | tr -d ' ')
echo "playground in $out: wasm $((raw / 1024)) KiB, $((gz / 1024)) KiB gzipped"
