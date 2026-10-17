# Homebrew (macOS and Linux)
brew install sspurlang/sspur/sspur

# or the install script, which checks the release checksum and installs into ~/.local/bin
curl -fsSL https://raw.githubusercontent.com/sspurlang/sspur/main/install.sh | sh

# native compilation needs clang; verify needs z3
xcode-select --install        # macOS: clang
brew install z3               # or: sudo apt install clang z3

sspur --version
