# SSPUR plugin for Claude Code

- `skills/sspur/SKILL.md`: the SSPUR workflow (`sspur start`, `q find|grep|body`, one `edit --test`, `verify`, `deploy local`). It loads when you work with `.ssp` files or a `.sspur/` codebase, or ask about SSPUR.
- `.mcp.json`: starts `sspur mcp` in the project directory, so the tools `mcp__sspur__start`, `query`, `edit`, `test`, ... are available. See [docs/agent/mcp.md](../../docs/agent/mcp.md).
- `.claude-plugin/plugin.json`: the manifest. The repository root has `.claude-plugin/marketplace.json`, which lists this plugin.

The plugin needs `sspur` on your `PATH` (see the main README).

## Install

From GitHub:

```
claude plugin marketplace add sspurlang/sspur
claude plugin install sspur@sspur
```

From a checkout:

```
claude plugin marketplace add /path/to/sspur
claude plugin install sspur@sspur
```

To try it for one session without installing: `claude --plugin-dir /path/to/sspur/packaging/claude-code`.

Check it with `claude plugin validate packaging/claude-code` (the manifest) and `claude plugin validate .` (the marketplace).
