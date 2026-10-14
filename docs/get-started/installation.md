# Installation

SSPUR runs on macOS (arm64, x86_64), Linux (x86_64, aarch64) and Windows (x86_64, arm64). It is one binary, `sspur` (`sspur.exe` on Windows). Native compilation needs `clang` when you run code, and `sspur verify` needs `z3`.

## Homebrew or the install script

```console
{{#include ../snippets/get-started/install.sh}}
```

The install script downloads the release tarball for your platform, checks its SHA-256 checksum and puts `sspur` in `~/.local/bin`. Set `SSPUR_VERSION` to pin a version or `SSPUR_INSTALL_DIR` to install somewhere else.

## Windows

In PowerShell:

```powershell
irm https://raw.githubusercontent.com/utkarshavardhana/sspur/main/install.ps1 | iex
```

The script downloads the release zip for your CPU, checks its SHA-256 checksum against `SHA256SUMS`, puts `sspur.exe` in `%LOCALAPPDATA%\sspur\bin` and adds that folder to your user `PATH`. `SSPUR_VERSION` and `SSPUR_INSTALL_DIR` work as they do for `install.sh`. You can also unpack `sspur-vX.Y.Z-x86_64-pc-windows-msvc.zip` (or `aarch64-pc-windows-msvc`) from a release yourself, or install with Scoop from the manifest attached to each release:

```powershell
scoop install https://github.com/utkarshavardhana/sspur/releases/latest/download/sspur.json
```

Native code on Windows is compiled by LLVM's `clang` into a DLL, so install LLVM (`winget install LLVM.LLVM`, which puts `clang.exe` in `C:\Program Files\LLVM\bin`) and the Visual Studio Build Tools with the C++ workload, which provide the C runtime and linker that `clang` uses. sspur looks for `clang` on `PATH`, then next to `clang-cl`, then in the LLVM install folder; `CC` overrides it. The cache lives in `%LOCALAPPDATA%\sspur` (`SSPUR_CACHE` overrides it). Windows has no system time zone database, so to use zone names such as `Europe/Paris` set `TZDIR` to a `zoneinfo` folder, for example the one in Python's `tzdata` package.

## Prebuilt binaries

Each [release](https://github.com/utkarshavardhana/sspur/releases) has tarballs for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`, zips for `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`, and a `SHA256SUMS` file:

```console
{{#include ../snippets/get-started/install-other.sh:binary}}
```

## From source

```console
{{#include ../snippets/get-started/install-other.sh:source}}
```

`cargo install --path crates/sspur-cli` from the clone installs it into `~/.cargo/bin` instead.

## Optional tools

| Tool | Used for |
|---|---|
| `clang` | `sspur run` and `sspur test`, which compile to native code by default. Without it they fall back to the interpreter with a warning |
| `z3` | `sspur verify` and the check elimination that uses its proofs |
| `qemu`, `lld` | building and booting `profile bare` kernels; `lld` also for `--lto` on Linux and Windows (LLVM for Windows ships it) |
| libcurl headers | `sspur deploy local` on Linux and macOS (on Windows the local host uses Winsock and needs nothing extra) |
| Homebrew `llvm` | `sspur gpu --emit spirv` and `--emit ptx`, which Apple's clang can't produce |

```console
{{#include ../snippets/get-started/install-other.sh:tools}}
```

GPU kernels run on Metal on macOS and on the CPU elsewhere (Linux and Windows report Metal as unavailable), with the same results.

## Editor and agent setup

`sspur lsp` is a language server, so SSPUR works in any editor with an LSP client. Each release has a VS Code extension (`sspur-vX.Y.Z.vsix`, also for Cursor, Windsurf and VSCodium), a template for JetBrains IDEs (`sspur-lsp4ij-vX.Y.Z.zip`) and a Zed extension (`sspur-zed-vX.Y.Z.zip`); all three install without any account. [Editor setup](editors.md) covers those and Neovim, Vim, Helix, Emacs, Sublime Text and Eclipse. For coding agents, `sspur mcp` is an MCP server and the Claude Code plugin bundles it with a skill; [SSPUR for AI agents](agents.md) shows both.

Next: [SSPUR in 5 minutes](five-minutes.md).
