#!/bin/sh
# Copies the Zed extension to OUT_DIR with the grammar pinned to a commit.
#
#   package.sh OUT_DIR [REPOSITORY [REV [VERSION]]]
#
# REPOSITORY defaults to the GitHub repository, REV to this checkout's HEAD,
# and VERSION to the one in extension.toml. For a local install from a
# checkout, use the checkout itself as the repository:
#
#   packaging/zed/package.sh /tmp/sspur-zed "file://$PWD"
#
# then run "zed: install dev extension" on /tmp/sspur-zed. The commit must
# contain the grammar, so commit first.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=$1
repo=${2:-https://github.com/utkarshavardhana/sspur}
rev=${3:-$(git -C "$root" rev-parse HEAD)}
version=${4:-}

rm -rf "$out"
mkdir -p "$out/tree-sitter-sspur"
cp -R "$here/Cargo.toml" "$here/Cargo.lock" "$here/README.md" "$here/src" "$here/languages" "$out/"
cp "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$out/"
cd "$here/tree-sitter-sspur"
cp -R grammar.js package.json tree-sitter.json src test "$out/tree-sitter-sspur/"

sed -e "/^\[grammars\.sspur\]/,/^\[language_servers/ s|^repository = .*|repository = \"$repo\"|" \
    -e "/^\[grammars\.sspur\]/,/^\[language_servers/ s|^rev = .*|rev = \"$rev\"|" \
    "$here/extension.toml" > "$out/extension.toml"
if [ -n "$version" ]; then
  sed -e "1,/^\[/ s|^version = .*|version = \"$version\"|" "$out/extension.toml" > "$out/extension.toml.new"
  mv "$out/extension.toml.new" "$out/extension.toml"
fi
grep -q "^rev = \"$rev\"" "$out/extension.toml"
