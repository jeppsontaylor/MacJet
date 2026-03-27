# AI Agent Integration (MCP Server)

MacJet ships a full **Model Context Protocol (MCP)** server natively. Your AI agent can query system health, diagnose performance issues, explore browser tabs, and securely manage processes.

## Quick Setup

Add this configuration to your MCP client (like Claude Desktop or Cursor):

```json
{
  "mcpServers": {
    "macjet": {
      "command": "/absolute/path/to/macjet",
      "args": ["--mcp"],
      "description": "macOS process monitor — CPU, memory, energy, Chrome tabs, process management"
    }
  }
}
```

Set `command` to a real path to the **`macjet` binary**:

- After **`cargo install --path .`**, that is usually **`~/.cargo/bin/macjet`** (expand to an absolute path in JSON).
- If you use a **GitHub Release** tarball, use the path where you extracted the universal `macjet` binary.

A sample fragment for Claude Desktop is in [examples/claude_desktop_config.json](examples/claude_desktop_config.json).

## Tools & Resources
MacJet registers 10 tools and 6 dynamic resources natively through the `rmcp` library. Tools range from `get_system_overview` to `kill_process`.

## Security Model: Extremely Safe
Exposing process-termination to AI models is risky. MacJet handles this gracefully:
1. **PID Validation**: Inherently blocks access to OS-level PIDs (< 500).
2. **Elicitation Guard**: The `kill_process` tool uses the MCP native Elicitation Request to actively halt the AI and prompt the human for authorization.
3. **Audit Trails**: Every interaction writes permanently to `~/.config/macjet/audit.log`.
