# Installation

SSPUR runs on macOS (arm64, x86_64), Linux (x86_64, aarch64) and Windows (x86_64, arm64). It is one binary, `sspur` (`sspur.exe` on Windows). Native compilation needs `clang` when you run code, and `sspur verify` needs `z3`.

## Homebrew or the install script

```console
{{#include ../snippets/get-started/install.sh}}
```

The install script downloads the release tarball for your platform, checks its SHA-256 checksum and puts `sspur` in `~/.local/bin`. If the GitHub CLI `gh` is installed and signed in, it also checks the tarball's build attestation (see [Verifying a release](#verifying-a-release)) and stops if that fails; without `gh` it relies on the checksum. Set `SSPUR_VERSION` to pin a version or `SSPUR_INSTALL_DIR` to install somewhere else.

## Windows

In PowerShell:

```powershell
irm https://raw.githubusercontent.com/sspurlang/sspur/main/install.ps1 | iex
```

The script downloads the release zip for your CPU, checks its SHA-256 checksum against `SHA256SUMS`, puts `sspur.exe` in `%LOCALAPPDATA%\sspur\bin` and adds that folder to your user `PATH`. `SSPUR_VERSION` and `SSPUR_INSTALL_DIR` work as they do for `install.sh`. You can also unpack `sspur-vX.Y.Z-x86_64-pc-windows-msvc.zip` (or `aarch64-pc-windows-msvc`) from a release yourself, or install with Scoop from the manifest attached to each release:

```powershell
scoop install https://github.com/sspurlang/sspur/releases/latest/download/sspur.json
```

Native code on Windows is compiled by LLVM's `clang` into a DLL, so install LLVM (`winget install LLVM.LLVM`, which puts `clang.exe` in `C:\Program Files\LLVM\bin`) and the Visual Studio Build Tools with the C++ workload, which provide the C runtime and linker that `clang` uses. sspur looks for `clang` on `PATH`, then next to `clang-cl`, then in the LLVM install folder; `CC` overrides it. The cache lives in `%LOCALAPPDATA%\sspur` (`SSPUR_CACHE` overrides it). Windows has no system time zone database, so to use zone names such as `Europe/Paris` set `TZDIR` to a `zoneinfo` folder, for example the one in Python's `tzdata` package.

## Prebuilt binaries

Each [release](https://github.com/sspurlang/sspur/releases) has tarballs for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`, zips for `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`, and a `SHA256SUMS` file:

```console
{{#include ../snippets/get-started/install-other.sh:binary}}
```

## Verifying a release

`SHA256SUMS` catches a damaged download, but it comes from the same place as the file. Releases from 1.0 on also carry GitHub artifact attestations, signed through Sigstore by the release workflow: a build provenance attestation for every file attached to the release (archives, editor extensions, `SHA256SUMS`, the Homebrew formula, the Scoop manifest, `install.sh` and `install.ps1`), and an SBOM attestation for each `sspur` archive, which points at the release's `sspur-vX.Y.Z.cdx.json` (the CycloneDX list of every Rust crate in the binary). With the GitHub CLI (2.68 or newer, signed in with `gh auth login`):

```console
{{#include ../snippets/get-started/install-other.sh:verify}}
```

A good file prints `Verification succeeded!` and exits with status 0. An error (no attestation for the file's digest, or one from another workflow or ref) means the file did not come from that release's build, so don't run it.

`install.sh` runs the first check itself when `gh` is installed and signed in, and refuses to install if it fails; set `SSPUR_SKIP_ATTESTATION=1` to install with the checksum alone. Without `gh`, with a `gh` that isn't signed in or is older than 2.68, or for 0.15 and older, it checks the checksum only and says so. `install.ps1` checks the checksum only; run `gh attestation verify` on the zip yourself.

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
