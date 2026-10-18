# Editor setup

`sspur lsp` is a language server: any editor that speaks the Language Server Protocol gets the compiler's diagnostics and quick fixes, typed-hole candidates, completion with signatures and docs, hover types, signature help while typing a call, inlay hints with inferred types and parameter names, formatting, go to definition, references, rename, the outline and semantic highlighting. It talks over stdin and stdout, so the setup is always the same: run `sspur lsp` for `*.ssp` files. A file inside a directory with an `sspur.toml` is checked with that package's dependencies, as `sspur check` does.

Install `sspur` first ([Installation](installation.md)) and check that `sspur --version` works in the shell your editor starts. If the editor can't find it, give the full path instead of `sspur`.

## VS Code

The extension in [`packaging/vscode/`](https://github.com/sspurlang/sspur/tree/main/packaging/vscode) adds highlighting, the language server, and the commands *SSPUR: Run File*, *SSPUR: Test File* and *SSPUR: Fill Holes*. Each [release](https://github.com/sspurlang/sspur/releases/latest) has it as `sspur-vX.Y.Z.vsix`, with its checksum in `SHA256SUMS`. Download it and install it, with no account or marketplace:

```sh
code --install-extension sspur-v1.0.0.vsix
```

or use *Extensions: Install from VSIX...* in the command palette. To update, install the file from a newer release. It needs `sspur` 0.7 or later.

`sspur.path` in the settings points it at a binary that isn't on `PATH`. `sspur.inlayHints.types`, `sspur.inlayHints.lambdas` and `sspur.inlayHints.params` turn each kind of inlay hint off, and VS Code's `editor.inlayHints.enabled` turns them all off or shows them only while you hold Ctrl+Alt.

## Cursor, Windsurf, VSCodium and other VS Code forks

The same `.vsix` works: run the fork's command line with `--install-extension` (`cursor --install-extension sspur-v1.0.0.vsix`, `codium --install-extension ...`), or use *Extensions: Install from VSIX...* in the command palette.

## Neovim

Neovim 0.11 or later, in `init.lua`:

```lua
vim.filetype.add({ extension = { ssp = "sspur" } })
vim.lsp.config("sspur", {
  cmd = { "sspur", "lsp" },
  filetypes = { "sspur" },
  root_markers = { "sspur.toml", ".git" },
})
vim.lsp.enable("sspur")
```

With nvim-lspconfig on an older Neovim, register it as a custom server:

```lua
vim.filetype.add({ extension = { ssp = "sspur" } })
local configs = require("lspconfig.configs")
if not configs.sspur then
  configs.sspur = {
    default_config = {
      cmd = { "sspur", "lsp" },
      filetypes = { "sspur" },
      root_dir = require("lspconfig.util").root_pattern("sspur.toml", ".git"),
      single_file_support = true,
    },
  }
end
require("lspconfig").sspur.setup({})
```

Highlighting comes from the server's semantic tokens: the Tree-sitter grammar in `packaging/zed/tree-sitter-sspur/` is only packaged for Zed so far. Neovim leaves inlay hints off; `vim.lsp.inlay_hint.enable(true)` turns them on.

## Vim

With [vim-lsp](https://github.com/prabirshrestha/vim-lsp), in `.vimrc`:

```vim
au BufRead,BufNewFile *.ssp setfiletype sspur
au User lsp_setup call lsp#register_server({
      \ 'name': 'sspur',
      \ 'cmd': {server_info -> ['sspur', 'lsp']},
      \ 'allowlist': ['sspur'],
      \ })
```

With [coc.nvim](https://github.com/neoclide/coc.nvim), in `coc-settings.json` (and the same `setfiletype` line):

```json
{
  "languageserver": {
    "sspur": {
      "command": "sspur",
      "args": ["lsp"],
      "filetypes": ["sspur"],
      "rootPatterns": ["sspur.toml"]
    }
  }
}
```

## Helix

In `~/.config/helix/languages.toml`:

```toml
{{#include ../snippets/get-started/helix-languages.toml}}
```

Helix highlights only with Tree-sitter, and the SSPUR grammar (in `packaging/zed/tree-sitter-sspur/`) is only packaged for Zed so far, so you get everything but colors.

## Emacs

With eglot (built in since Emacs 29), in `init.el`:

```elisp
(define-derived-mode sspur-mode prog-mode "SSPUR"
  (setq-local comment-start "// "))
(add-to-list 'auto-mode-alist '("\\.ssp\\'" . sspur-mode))
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs '(sspur-mode "sspur" "lsp")))
(add-hook 'sspur-mode-hook #'eglot-ensure)
```

With lsp-mode, keep the mode and register the client instead:

```elisp
(with-eval-after-load 'lsp-mode
  (add-to-list 'lsp-language-id-configuration '(sspur-mode . "sspur"))
  (lsp-register-client
   (make-lsp-client :new-connection (lsp-stdio-connection '("sspur" "lsp"))
                    :activation-fn (lsp-activate-on "sspur")
                    :server-id 'sspur)))
(add-hook 'sspur-mode-hook #'lsp-deferred)
```

## Sublime Text

Install the LSP package, then convert the VS Code grammar into a syntax Sublime loads (it reads `.tmLanguage` files as plists):

```sh
python3 -c 'import json, plistlib, sys; plistlib.dump(json.load(open(sys.argv[1])), open(sys.argv[2], "wb"))' \
  packaging/vscode/syntaxes/sspur.tmLanguage.json "$HOME/Library/Application Support/Sublime Text/Packages/User/SSPUR.tmLanguage"
```

(On Linux the folder is `~/.config/sublime-text/Packages/User`, on Windows `%APPDATA%\Sublime Text\Packages\User`.) Then in *Preferences: LSP Settings*:

```json
{
  "clients": {
    "sspur": {
      "enabled": true,
      "command": ["sspur", "lsp"],
      "selector": "source.sspur"
    }
  }
}
```

## JetBrains IDEs

IntelliJ IDEA, PyCharm, GoLand, RustRover, WebStorm, CLion and the rest, Community editions included, through the free [LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) plugin:

1. Install LSP4IJ from *Settings > Plugins > Marketplace*. It needs no account.
2. Download `sspur-lsp4ij-vX.Y.Z.zip` from the [latest release](https://github.com/sspurlang/sspur/releases/latest) and unzip it. It holds a folder, `sspur`, with the server definition.
3. In *Settings > Languages & Frameworks > Language Servers*, click **+**, then **Import template...**, and choose that `sspur` folder.
4. Click **OK**. Opening a `.ssp` file starts `sspur lsp`: highlighting, errors, completion, hover, quick fixes and hole filling all come from it.

If `sspur` isn't on the `PATH` that the IDE sees, edit the server's *Command* to the full path, for example `/opt/homebrew/bin/sspur lsp`. The template is the same as setting it up by hand: a server running `sspur lsp`, mapped to `*.ssp` with the language id `sspur`.

## Zed

The extension in [`packaging/zed/`](https://github.com/sspurlang/sspur/tree/main/packaging/zed) adds a Tree-sitter grammar (highlighting, brackets, indentation, and an outline of the `fn`, `type`, `test`, `trait`, `impl`, `effect`, `rule`, `store` and `svc` definitions) and starts `sspur lsp`. Each [release](https://github.com/sspurlang/sspur/releases/latest) has it as `sspur-zed-vX.Y.Z.zip`, with its checksum in `SHA256SUMS`, and it installs as a dev extension, with no account or registry:

1. Unzip `sspur-zed-vX.Y.Z.zip`.
2. In Zed, run **zed: install dev extension** from the command palette and pick the unzipped `sspur-zed` folder.

Zed compiles the extension with Rust (from rustup, which lets it add the `wasm32-wasip2` target itself) and fetches the grammar from GitHub at the release's commit. If `sspur` isn't on the `PATH` that Zed sees, set its path in `settings.json`:

```json
{ "lsp": { "sspur": { "binary": { "path": "/opt/homebrew/bin/sspur" } } } }
```

To try the grammar from a checkout instead, commit, run `packaging/zed/package.sh /tmp/sspur-zed "file://$PWD"` in the repository, and install `/tmp/sspur-zed` the same way: Zed then reads the grammar from your local repository at `HEAD`.

## Kate

In *Settings > Configure Kate > LSP Client > User Server Settings*:

```json
{
  "servers": {
    "sspur": {
      "command": ["sspur", "lsp"],
      "highlightingModeRegex": "^SSPUR$"
    }
  }
}
```

Kate starts a server for the files whose highlighting mode matches, and there is no SSPUR highlighting definition for Kate yet, so this only takes effect once one is installed; until then use another editor on this page.

## Eclipse

With LSP4E and TM4E (both in the Eclipse IDE for Java developers, or from the Marketplace):

1. *Preferences > General > Content Types*: under *Text*, add a content type `SSPUR` for `*.ssp`.
2. *Preferences > TextMate > Grammar*: add `packaging/vscode/syntaxes/sspur.tmLanguage.json` and map it to that content type.
3. *Run > External Tools > External Tools Configurations*: add a program `sspur` with the argument `lsp`.
4. *Preferences > Language Servers*: add the `SSPUR` content type with that launch configuration.

Open `.ssp` files with the Generic Text Editor.

## Visual Studio and Xcode

Visual Studio has no setting for a third-party language server; it needs an extension built on its language server SDK, and there isn't one for SSPUR. Xcode doesn't support third-party language servers. Use VS Code, Zed or one of the editors above for now.
