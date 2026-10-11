# Editor setup

`sspur lsp` is a language server: any editor that speaks the Language Server Protocol gets the compiler's diagnostics and quick fixes, typed-hole candidates, completion with signatures and docs, hover types, formatting, go to definition, references, rename, the outline and semantic highlighting. It talks over stdin and stdout, so the setup is always the same: run `sspur lsp` for `*.ssp` files. A file inside a directory with an `sspur.toml` is checked with that package's dependencies, as `sspur check` does.

Install `sspur` first ([Installation](installation.md)) and check that `sspur --version` works in the shell your editor starts. If the editor can't find it, give the full path instead of `sspur`.

## VS Code

The extension in [`packaging/vscode/`](https://github.com/utkarshavardhana/sspur/tree/main/packaging/vscode) adds highlighting, the language server, and the commands *SSPUR: Run File*, *SSPUR: Test File* and *SSPUR: Fill Holes*. Until it is on the Marketplace, build it and install the `.vsix`:

```sh
cd packaging/vscode
npm install
npx @vscode/vsce package
code --install-extension sspur-0.6.0.vsix
```

`sspur.path` in the settings points it at a binary that isn't on `PATH`.

## Cursor, Windsurf, VSCodium and other VS Code forks

The same `.vsix` works: run the fork's command line with `--install-extension` (`cursor --install-extension sspur-0.6.0.vsix`, `codium --install-extension ...`), or use *Extensions: Install from VSIX...* in the command palette. These editors search [Open VSX](https://open-vsx.org) rather than the Microsoft Marketplace, which is where the extension will be listed for them.

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

Highlighting comes from the server's semantic tokens, since there is no Tree-sitter grammar for SSPUR yet.

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

Helix highlights only with Tree-sitter, so until SSPUR has a grammar you get everything but colors.

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

1. Install LSP4IJ from *Settings > Plugins*.
2. In *Settings > Languages & Frameworks > Language Servers*, add a server named `SSPUR` with the command `sspur lsp`.
3. On its *Mappings* tab, add the file name pattern `*.ssp` with the language id `sspur`.
4. For highlighting, add the extension folder as a TextMate bundle in *Settings > Editor > TextMate Bundles* (`+`, then pick `packaging/vscode`). JetBrains reads the grammar from its `package.json`.

A plugin for the JetBrains Marketplace that does this in one step is planned.

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

## Zed, Visual Studio and Xcode

Zed loads languages only from extensions, and a Zed language needs a Tree-sitter grammar, which SSPUR doesn't have yet. Visual Studio has no setting for a third-party language server; it needs an extension built on its language server SDK, and there isn't one for SSPUR. Xcode doesn't support third-party language servers. Use VS Code or one of the editors above for now.
