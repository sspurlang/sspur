#!/bin/sh
# usage: run.sh <work-dir> <sspur-bin>... (STEPS="check test interp publish app edit small hello" by default)
# Times the synthetic package of gen.py, each step cold (an empty SSPUR_CACHE) and then warm
# (the step "edit" is a cold `sspur test`, then one after changing one function of a module),
# and prints one line per step and binary: wall seconds and the peak memory of the whole process
# tree. The binaries take turns on each step, so a slow spell of the machine hits them alike.
# "small" is `sspur test` of examples/packages/app and "hello" is `sspur run` of hello world.
here=$(cd "$(dirname "$0")" && pwd)
work=$1
shift
bins="$*"
steps=${STEPS:-"check test interp publish app edit small hello"}
repo=$(cd "$here/../.." && pwd)
mkdir -p "$work"
work=$(cd "$work" && pwd)
export WALL="$work/wall"
# Runs a command, and prints its wall time and the largest summed RSS of its process tree (MB),
# sampled every 0.1 s, so a command shorter than that shows none.
timed() {
  perl -MTime::HiRes=time -e '$s = time; $r = system @ARGV; open(my $f, ">", $ENV{WALL}); printf $f "%.3f", time - $s; exit($r >> 8)' "$@" > "$work/out.log" 2>&1 &
  pid=$!
  peak=0
  while kill -0 $pid 2> /dev/null; do
    t=$(ps -Ao pid=,ppid=,rss= | awk -v root=$pid '{p[$1] = $2; r[$1] = $3} END {for (k in p) {x = k; while (x != "" && x > 1) {if (x == root) {s += r[k]; break} x = p[x]}} print s + 0}')
    [ "$t" -gt "$peak" ] && peak=$t
    sleep 0.1
  done
  wait $pid
  st=$?
  printf "%7.2fs %6dMB%s" "$(cat "$WALL")" $((peak / 1024)) "$([ $st = 0 ] || echo " (exit $st: $(tail -1 "$work/out.log"))")"
}
fresh() {
  rm -rf "$work/src"
  python3 "$here/gen.py" "$work/src"
  cp -R "$repo/examples/packages" "$work/src/small"
  printf 'fn main() -> Unit ! log\n= log("hello")\n' > "$work/src/hello.ssp"
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
    edit) dir=bigpkg; set -- test ;;
    small) dir=small/app; set -- test ;;
    hello) dir=.; set -- run hello.ssp ;;
  esac
  cd "$work/src/$dir" || exit 1
  printf "%-8s %-28s cold " "$step" "$(basename "$bin")"
  timed "$bin" "$@"
  if [ "$step" = edit ]; then
    sed -i.bak 's/^= n \* 3 + /= n * 4 + /' "$work/src/bigpkg/mods/m050/lib.ssp"
    printf "   edit "
  else
    printf "   warm "
  fi
  timed "$bin" "$@"
  echo
  rm -rf "$SSPUR_CACHE"
done
done
