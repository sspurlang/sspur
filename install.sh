#!/bin/sh
# Install a prebuilt sspur binary from GitHub releases.
#   curl -fsSL https://raw.githubusercontent.com/sspurlang/sspur/main/install.sh | sh
# Environment: SSPUR_VERSION (default: latest), SSPUR_INSTALL_DIR (default: ~/.local/bin),
#   SSPUR_DOWNLOAD_BASE (a mirror holding the release tarballs and SHA256SUMS),
#   SSPUR_SKIP_ATTESTATION (set to skip the gh attestation check)
set -eu

repo="sspurlang/sspur"
dest="${SSPUR_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf 'sspur-install: %s\n' "$*" >&2; }
die() { say "$*"; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "needs '$1'"; }

need curl
need tar
need uname

case "$(uname -s)" in
  Darwin) os=apple-darwin ;;
  Linux) os=unknown-linux-gnu ;;
  *) die "unsupported OS: $(uname -s)" ;;
esac
case "$(uname -m)" in
  arm64 | aarch64) arch=aarch64 ;;
  x86_64 | amd64) arch=x86_64 ;;
  *) die "unsupported CPU: $(uname -m)" ;;
esac
target="$arch-$os"

version="${SSPUR_VERSION:-}"
if [ -z "$version" ]; then
  version=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)
  [ -n "$version" ] || die "could not find the latest release; set SSPUR_VERSION"
fi
case "$version" in v*) ;; *) version="v$version" ;; esac

name="sspur-$version-$target"
base="${SSPUR_DOWNLOAD_BASE:-https://github.com/$repo/releases/download/$version}"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

say "downloading $name.tar.gz"
curl -fsSL -o "$tmp/$name.tar.gz" "$base/$name.tar.gz" || die "download failed: $base/$name.tar.gz"
curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" || die "download failed: $base/SHA256SUMS"

want=$(awk -v f="$name.tar.gz" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS")
[ -n "$want" ] || die "no checksum for $name.tar.gz in SHA256SUMS"
if command -v sha256sum >/dev/null 2>&1; then
  got=$(sha256sum "$tmp/$name.tar.gz" | awk '{ print $1 }')
elif command -v shasum >/dev/null 2>&1; then
  got=$(shasum -a 256 "$tmp/$name.tar.gz" | awk '{ print $1 }')
else
  die "needs sha256sum or shasum to verify the download"
fi
[ "$want" = "$got" ] || die "checksum mismatch for $name.tar.gz (expected $want, got $got)"
say "checksum ok"

# Releases after 0.15 have GitHub build provenance; a signed-in gh checks it, and without gh the checksum is all.
attested() {
  v="${version#v}"
  major="${v%%.*}"
  minor="${v#*.}"
  minor="${minor%%.*}"
  case "$major.$minor" in *[!0-9.]* | .* | *.) return 0 ;; esac
  [ "$major" -gt 0 ] || [ "$minor" -gt 15 ]
}
if [ -n "${SSPUR_SKIP_ATTESTATION:-}" ]; then
  say "attestation check skipped (SSPUR_SKIP_ATTESTATION is set)"
elif ! attested; then
  say "$version predates attestations; checked the checksum only"
elif ! command -v gh >/dev/null 2>&1; then
  say "gh not found; checked the checksum only"
elif ! gh auth status >/dev/null 2>&1; then
  say "gh is not signed in; checked the checksum only"
elif ! gh attestation verify --help 2>/dev/null | grep -q -e --source-ref; then
  say "this gh is too old to check attestations (needs 2.68 or newer); checked the checksum only"
else
  gh attestation verify "$tmp/$name.tar.gz" --repo "$repo" --signer-workflow "$repo/.github/workflows/release.yml" \
    --source-ref "refs/tags/$version" >/dev/null 2>&1 ||
    die "attestation check failed for $name.tar.gz; run 'gh attestation verify' on it to see why, or set SSPUR_SKIP_ATTESTATION=1 to rely on the checksum alone"
  say "attestation ok"
fi

tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$dest"
install -m 755 "$tmp/$name/sspur" "$dest/sspur"
say "installed $("$dest/sspur" --version) to $dest/sspur"

case ":$PATH:" in
  *":$dest:"*) ;;
  *) say "add $dest to your PATH" ;;
esac
command -v clang >/dev/null 2>&1 || say "clang was not found; sspur needs it to compile native code"
