#!/usr/bin/env bash
# After changing pulse/lib, re-pin it in every section's sspur.lock.
set -u
cd "$(dirname "$0")"
SSPUR=${SSPUR:-../target/release/sspur}
case "$SSPUR" in /*) ;; *) SSPUR="$PWD/$SSPUR" ;; esac
for d in */; do
  d=${d%/}
  [ "$d" != lib ] && [ -f "$d/sspur.toml" ] && (cd "$d" && "$SSPUR" deps update >/dev/null && echo "relocked $d")
done
true
