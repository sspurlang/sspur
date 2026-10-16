#!/bin/sh
# usage: run.sh <work-dir> <sspur-bin>... (STEPS="check test interp publish app" by default)
# Times the synthetic package of gen.py, each step cold (an empty SSPUR_CACHE) and then warm,
# and prints one line per step and binary: wall seconds and the peak memory of the whole process
# tree. The binaries take turns on each step, so a slow spell of the machine hits them alike.
here=$(cd "$(dirname "$0")" && pwd)
work=$1
shift
bins="$*"
steps=${STEPS:-"check test interp publish app"}
mkdir -p "$work"
work=$(cd "$work" && pwd)
now() { perl -MTime::HiRes=time -e 'printf "%.3f", time'; }
# Runs a command, and prints its wall time and the largest summed RSS of its process tree (MB).
timed() {
  "$@" > "$work/out.log" 2>&1 &
  pid=$!
  peak=0
  s=$(now)
  while kill -0 $pid 2> /dev/null; do
    t=$(ps -Ao pid=,ppid=,rss= | awk -v root=$pid '{p[$1] = $2; r[$1] = $3} END {for (k in p) {x = k; while (x != "" && x > 1) {if (x == root) {s += r[k]; break} x = p[x]}} print s + 0}')
    [ "$t" -gt "$peak" ] && peak=$t
    sleep 0.2
  done
  wait $pid
  st=$?
  e=$(now)
  printf "%7.1fs %6dMB%s" "$(echo "$e - $s" | bc)" $((peak / 1024)) "$([ $st = 0 ] || echo " (exit $st: $(tail -1 "$work/out.log"))")"
}
fresh() {
  rm -rf "$work/src"
  python3 "$here/gen.py" "$work/src"
  (cd "$work/src/bigpkg" && git init -q && git add -A && git -c user.name=bench -c user.email=bench@example.com commit -qm bench && git tag v1.0.0)
}
n=0
for step in $steps; do
for bin in $bins; do
  n=$((n + 1))
  fresh
  export SSPUR_CACHE="$work/cache$n"
  case $step in
    check) dir=bigpkg; set -- check ;;
    test) dir=bigpkg; set -- test ;;
    interp) dir=bigpkg; set -- test --interp ;;
    publish) dir=bigpkg; set -- publish --dry-run --no-gh ;;
    app) dir=app; set -- run main.ssp ;;
  esac
  cd "$work/src/$dir" || exit 1
  printf "%-8s %-28s cold " "$step" "$(basename "$bin")"
  timed "$bin" "$@"
  printf "   warm "
  timed "$bin" "$@"
  echo
  rm -rf "$SSPUR_CACHE"
done
done
