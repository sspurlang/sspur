# ANCHOR: binary
# a prebuilt tarball from https://github.com/utkarshavardhana/sspur/releases
shasum -a 256 -c SHA256SUMS --ignore-missing
tar -xzf sspur-v0.13.0-aarch64-apple-darwin.tar.gz
sudo install sspur-v0.13.0-aarch64-apple-darwin/sspur /usr/local/bin/
# ANCHOR_END: binary
# ANCHOR: source
# from source: Rust 1.88 or newer, and clang
git clone https://github.com/utkarshavardhana/sspur.git
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
