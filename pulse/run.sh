#!/usr/bin/env bash
# Build SSPUR Pulse: run every section program, then the overview, into $PULSE_OUT.
# Each section fetches on its own; a section that fails keeps its last good data.
# Sections run in the interpreter: native json.decode runs out of memory on large
# feeds (see the Pulse docs page).
set -u
cd "$(dirname "$0")/.."
SSPUR=${SSPUR:-./target/release/sspur}
export PULSE_ROOT=pulse
export PULSE_DATA=${PULSE_DATA:-pulse/data}
export PULSE_OUT=${PULSE_OUT:-pulse/out}
SECTIONS=${PULSE_SECTIONS:-"reading launches quakes repos hn papers weather holidays"}
TMO=""
if command -v timeout >/dev/null 2>&1; then TMO="timeout 600"; fi

mkdir -p "$PULSE_OUT/assets" "$PULSE_OUT/og" "$PULSE_DATA"
cp pulse/web/pulse.js pulse/web/eggs.js pulse/web/here.js pulse/web/icon.svg pulse/web/icon-192.png pulse/web/icon-512.png pulse/web/fonts/pulse-display.woff2 "$PULSE_OUT/assets/"
cp pulse/web/manifest.webmanifest "$PULSE_OUT/manifest.webmanifest"

failed=""
for s in $SECTIONS; do
  echo "== $s"
  if ! $TMO "$SSPUR" run --interp "pulse/$s/main.ssp"; then
    echo "$s: the run failed, rendering its last good data"
    failed="$failed $s"
    PULSE_SKIP=1 $TMO "$SSPUR" run --interp "pulse/$s/main.ssp" || echo "$s: no page this run"
  fi
done
echo "== overview"
$TMO "$SSPUR" run --interp pulse/overview/main.ssp || failed="$failed overview"

# Social images: SVG to PNG when a converter is installed, else the bundled card.
for svg in "$PULSE_OUT"/og/*.svg; do
  [ -e "$svg" ] || continue
  png="${svg%.svg}.png"
  if command -v rsvg-convert >/dev/null 2>&1; then
    rsvg-convert -w 1200 -h 630 "$svg" -o "$png" || cp pulse/web/og.png "$png"
  else
    cp pulse/web/og.png "$png"
  fi
done

if [ -n "$failed" ]; then echo "pulse: these sections fell back to their last good data:$failed"; fi
echo "pulse: built $(find "$PULSE_OUT" -type f | wc -l | tr -d ' ') files in $PULSE_OUT"
