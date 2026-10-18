# ANCHOR: binary
# a prebuilt tarball from https://github.com/sspurlang/sspur/releases
shasum -a 256 -c SHA256SUMS --ignore-missing
tar -xzf sspur-v1.0.0-aarch64-apple-darwin.tar.gz
sudo install sspur-v1.0.0-aarch64-apple-darwin/sspur /usr/local/bin/
# ANCHOR_END: binary
# ANCHOR: verify
# was this file built from sspurlang/sspur by its release workflow, from the release tag?
gh attestation verify sspur-v1.0.0-aarch64-apple-darwin.tar.gz --repo sspurlang/sspur \
  --signer-workflow sspurlang/sspur/.github/workflows/release.yml --source-ref refs/tags/v1.0.0
# the dependency list (CycloneDX SBOM) attested for the same file
gh attestation verify sspur-v1.0.0-aarch64-apple-darwin.tar.gz --repo sspurlang/sspur \
  --predicate-type https://cyclonedx.org/bom
# ANCHOR_END: verify
# ANCHOR: source
# from source: Rust 1.88 or newer, and clang
git clone https://github.com/sspurlang/sspur.git
cd sspur
cargo build --release
export PATH="$PWD/target/release:$PATH"
# ANCHOR_END: source
# ANCHOR: tools
# optional tools on macOS
brew install z3 qemu llvm lld
# and on Debian or Ubuntu
sudo apt install clang z3 qemu-system-misc qemu-system-arm lld llvm libcurl4-openssl-dev
# ANCHOR_END: tools
