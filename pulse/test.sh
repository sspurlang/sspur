#!/usr/bin/env bash
# Run the tests of every Pulse package, offline, on the bundled fixtures.
set -u
cd "$(dirname "$0")"
SSPUR=${SSPUR:-../target/release/sspur}
case "$SSPUR" in /*) ;; *) SSPUR="$PWD/$SSPUR" ;; esac
status=0
for d in */; do
  d=${d%/}
  [ -f "$d/sspur.toml" ] || continue
  src=main.ssp; [ "$d" = lib ] && src=lib.ssp
  printf '%-10s ' "$d"
  (cd "$d" && "$SSPUR" test "$src") || status=1
done
exit $status
