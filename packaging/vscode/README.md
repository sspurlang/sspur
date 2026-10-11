# SSPUR for VS Code

Language support for [SSPUR](https://github.com/utkarshavardhana/sspur), the statically typed, effect-tracked language for AI agents. The extension runs `sspur lsp`, so everything comes from the compiler you have installed:

- Diagnostics as you type, with the compiler's quick fixes: canonical spellings, missing effects, missing match arms, unreachable arms and did-you-mean names.
- Typed holes: a `?hole` lists the expressions that fit it and pass your tests and contracts, and the quick fix fills it in. *SSPUR: Fill Holes* fills every hole that has a passing candidate.
- Completion of locals, functions, constructors, fields, methods after `.`, effects after `!`, keywords and templates, with signatures and the standard library's docs.
- Hover types and signatures, go to definition, find references, rename, the outline, and formatting (*Format Document* runs `sspur fmt`).
- Highlighting from a TextMate grammar, refined by the server's semantic tokens, in the same colors as the [playground](https://utkarshavardhana.github.io/sspur/play/).
- *SSPUR: Run File* and *SSPUR: Test File* (also the run button in the editor title) save the file and run `sspur run` or `sspur test` on it.

Files inside a directory with an `sspur.toml` are checked with that package's dependencies.

## Install

You need `sspur` 0.7 or later, which has `sspur lsp`: `brew install utkarshavardhana/sspur/sspur`, the install script, or a release binary (see [Installation](https://utkarshavardhana.github.io/sspur/get-started/installation.html)). Check that `sspur --version` works.

Then download `sspur-vX.Y.Z.vsix` from the [latest release](https://github.com/utkarshavardhana/sspur/releases/latest) and install it with *Extensions: Install from VSIX...* in the command palette, or `code --install-extension sspur-vX.Y.Z.vsix`. Cursor, Windsurf, VSCodium and other VS Code forks take the same file (`cursor --install-extension ...`, `codium --install-extension ...`). No account or marketplace is needed. To update, install the file from a newer release.

## Settings

| Setting | Default | |
|---|---|---|
| `sspur.path` | empty | The `sspur` binary. Empty means `sspur` on your `PATH`. |
| `sspur.trace.server` | `off` | `messages` or `verbose` logs the protocol in the *SSPUR* output channel. |

*SSPUR: Restart Language Server* restarts it, for example after you upgrade `sspur`.

Other editors are covered in [Editor setup](https://utkarshavardhana.github.io/sspur/get-started/editors.html).

## Building and publishing

For maintainers. From this directory, with Node 18 or later:

```console
npm install
npx @vscode/vsce package          # writes sspur-<version>.vsix
code --install-extension sspur-<version>.vsix
```

The release workflow does this for every tag: it sets `version` to the release, builds `sspur-vX.Y.Z.vsix`, adds it to `SHA256SUMS` and attaches it to the GitHub release.

Listing it on a marketplace as well is optional and only adds automatic updates. To do it:

1. VS Code Marketplace: create the publisher at <https://marketplace.visualstudio.com/manage> (it needs a Microsoft account and an Azure DevOps personal access token with the *Marketplace: Manage* scope), then `npx @vscode/vsce login <publisher-id>` and `npx @vscode/vsce publish`, or upload the `.vsix` on the manage page.
2. Open VSX, for Cursor, Windsurf and VSCodium: sign in at <https://open-vsx.org> with GitHub, create an access token and the namespace (`npx ovsx create-namespace <publisher-id> -p <token>`), then `npx ovsx publish sspur-vX.Y.Z.vsix -p <token>`.

The `.vsix` and `node_modules/` are not committed.
