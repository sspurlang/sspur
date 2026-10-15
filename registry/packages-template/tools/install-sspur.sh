#!/usr/bin/env bash
# Installs the sspur release named in .sspur-version (or $1) for the workflows, verifying the
# archive against the release's SHA-256 file, and puts it on the PATH of later steps.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
v="${1:-$(tr -d '[:space:]' < "$here/../.sspur-version")}"
target="x86_64-unknown-linux-gnu"
name="sspur-v$v-$target"
dir="${RUNNER_TEMP:-/tmp}/sspur-$v"
base="https://github.com/utkarshavardhana/sspur/releases/download/v$v"
mkdir -p "$dir"
curl -fsSL --retry 3 -o "$dir/$name.tar.gz" "$base/$name.tar.gz"
curl -fsSL --retry 3 -o "$dir/$name.tar.gz.sha256" "$base/$name.tar.gz.sha256"
(cd "$dir" && sha256sum -c "$name.tar.gz.sha256")
tar -xzf "$dir/$name.tar.gz" -C "$dir"
"$dir/$name/sspur" --version
if [ -n "${GITHUB_PATH:-}" ]; then echo "$dir/$name" >> "$GITHUB_PATH"; fi
