# ANCHOR: mcp
# Claude Code, from the project directory (--scope project writes a .mcp.json you can commit)
claude mcp add sspur -- sspur mcp
# ANCHOR_END: mcp
# ANCHOR: plugin
# the Claude Code plugin: the MCP server plus a skill that teaches the workflow
claude plugin marketplace add sspurlang/sspur
claude plugin install sspur@sspur

# or, for one session from a clone, without installing
claude --plugin-dir /path/to/sspur/packaging/claude-code
# ANCHOR_END: plugin
