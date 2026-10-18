#!/bin/sh
# Regenerates tools/order/hot.txt (ADR 0043): builds sspur with symbols, runs the hot commands under
# lldb with a breakpoint on every function, and lists the functions in order of first call.
# macOS with Xcode's lldb. Usage: tools/order/gen.sh [target-dir]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
target=${1:-$root/target/order-gen}
CARGO_TARGET_DIR=$target CARGO_PROFILE_RELEASE_STRIP=false cargo build --release --locked -p sspur-cli --manifest-path "$root/Cargo.toml"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
exe=$work/sspur
cp "$target/release/sspur" "$exe"
printf 'fn main() -> Unit ! log\n= log("hi")\n' > "$work/h.ssp"
cp "$root/pulse/lib/lib.ssp" "$work/lib.ssp"
python3 - "$work/lib.ssp" > "$work/lsp.in" <<'PY'
import json, sys
uri = "file://" + sys.argv[1]
msgs = [
    {"id": 1, "method": "initialize", "params": {"processId": None, "rootUri": None, "capabilities": {}}},
    {"method": "initialized", "params": {}},
    {"method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri, "languageId": "sspur", "version": 1, "text": open(sys.argv[1]).read()}}},
    {"id": 2, "method": "textDocument/hover", "params": {"textDocument": {"uri": uri}, "position": {"line": 4, "character": 10}}},
    {"id": 3, "method": "shutdown"},
    {"method": "exit"},
]
for m in msgs:
    b = json.dumps(dict(jsonrpc="2.0", **m)).encode()
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(b) + b)
PY
cd "$work"
# Fill the run and test caches, so the traces see the cached paths.
"$exe" run h.ssp > /dev/null
"$exe" test lib.ssp > /dev/null
env -0 > "$work/env0"
# Smallest footprint first: each command's new functions follow those of the commands before it.
for c in "version:--version" "run:run h.ssp" "fmt:fmt lib.ssp" "interp:run --interp h.ssp" "check:check lib.ssp" "lsp:lsp" "test:test lib.ssp"; do
  n=${c%%:*}
  stdin=/dev/null
  [ "$n" = lsp ] && stdin=$work/lsp.in
  FTRACE_ENV=$work/env0 lldb --batch -o "command script import $here/trace.py" -o "ftrace $work/t_$n.txt $stdin $exe ${c#*:}" | grep functions
done
python3 "$here/hot.py" "$exe" "$work"/t_version.txt "$work"/t_run.txt "$work"/t_fmt.txt "$work"/t_interp.txt "$work"/t_check.txt "$work"/t_lsp.txt "$work"/t_test.txt > "$here/hot.txt"
wc -l "$here/hot.txt"
