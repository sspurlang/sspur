#!/bin/sh
# The name ends in -clang so rustc passes it cc arguments.
# Linker for sspur on macOS: lays out the functions in hot.txt first, in that order (ADR 0043).
# hot.txt has demangled names without crate hashes; the mangled names come from this link's objects.
# Any other link, or any failure here, links unchanged.
out= prev=
for a in "$@"; do
  [ "$prev" = -o ] && out=$a
  prev=$a
done
case "$out" in
*/release/deps/sspur-*) ;;
*) exec cc "$@" ;;
esac
list="$(dirname "$0")/hot.txt"
tmp=$(mktemp -d 2>/dev/null) || exec cc "$@"
objs="$tmp/objs"
: > "$objs"
for a in "$@"; do
  case "$a" in *.o | *.rlib) printf '%s\n' "$a" >> "$objs" ;; esac
done
order=
if [ -f "$list" ] && [ -s "$objs" ] &&
  tr '\n' '\0' < "$objs" | xargs -0 nm -U -j 2>/dev/null | grep '^_' | sort -u > "$tmp/m" &&
  c++filt -_ < "$tmp/m" | paste "$tmp/m" - > "$tmp/md" &&
  awk -F '\t' '
    function key(s) { sub(/ \(\.[^)]*\)$/, "", s); sub(/^_?OUTLINED_FUNCTION_[0-9]+$/, "OUTLINED_FUNCTION_", s); return s }
    NR == FNR { if ($0 !~ /^#/ && $0 != "" && !($0 in r)) r[$0] = FNR; next }
    { k = key($2); if (k in r) print r[k] "\t" $1 }
  ' "$list" "$tmp/md" | sort -s -n -k1,1 | cut -f2 > "$tmp/order" && [ -s "$tmp/order" ]; then
  order="-Wl,-order_file,$tmp/order"
fi
cc "$@" $order
s=$?
rm -rf "$tmp"
exit $s
