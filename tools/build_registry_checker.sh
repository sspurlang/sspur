#!/usr/bin/env bash
# Builds the browser checker of the registry's Publish page into the directory given (default
# target/registry-checker): the wasm build of the checker and interpreter with package facts
# (sspur-wasm with the registry feature, so the playground's own build stays as it is), and the
# registry checks program it runs. The release workflow ships it as sspur-registry-web-vX.tar.gz.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/target/registry-checker}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
cd "$root"

rustup target list --installed | grep -qx wasm32-unknown-unknown || rustup target add wasm32-unknown-unknown
want="$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p')"
have="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)"
if [ "$have" != "$want" ]; then
  cargo install wasm-bindgen-cli --version "$want" --locked
fi

# The same settings as the playground (tools/build_playground.sh).
CARGO_PROFILE_RELEASE_OPT_LEVEL=2 \
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C link-arg=-zstack-size=33554432" \
  cargo build --release -p sspur-wasm --features registry --target wasm32-unknown-unknown

wasm-bindgen --target web --no-typescript --out-name regcheck --out-dir "$out" "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/sspur_wasm.wasm"
wasm="$out/regcheck_bg.wasm"
if command -v wasm-opt >/dev/null; then
  wasm-opt --asyncify --pass-arg='asyncify-imports@*.__wbg_sspur_hop_*' --pass-arg=asyncify-ignore-indirect -O3 \
    --enable-bulk-memory --enable-sign-ext --enable-mutable-globals --enable-nontrapping-float-to-int \
    --enable-reference-types --enable-multivalue "$wasm" -o "$wasm.opt"
  mv "$wasm.opt" "$wasm"
else
  echo "wasm-opt not found; shipping the wasm without it"
fi
cp registry/packages-template/tools/registry/registry.ssp "$out/registry.ssp"
raw=$(wc -c < "$wasm" | tr -d ' ')
gz=$(gzip -9 -c "$wasm" | wc -c | tr -d ' ')
echo "registry checker in $out: wasm $((raw / 1024)) KiB, $((gz / 1024)) KiB gzipped"
