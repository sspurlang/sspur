# SSPUR for JetBrains IDEs

`sspur/` is a language server template for [LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij), the free LSP client for IntelliJ IDEA, PyCharm, GoLand, RustRover, WebStorm, CLion and the other JetBrains IDEs. It runs `sspur lsp` for `*.ssp` files.

Every release attaches it as `sspur-lsp4ij-vX.Y.Z.zip`. To use it, install LSP4IJ, then in *Settings > Languages & Frameworks > Language Servers* click **+**, **Import template...**, and choose the unzipped `sspur` folder. See [Editor setup](https://sspurlang.github.io/sspur/get-started/editors.html#jetbrains-ides).

You need `sspur` 0.7 or later, which has `sspur lsp`.
