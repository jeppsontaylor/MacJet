# Security Policy

## Reporting a Vulnerability
If you discover a security vulnerability in MacJet, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email: **security@jepsontaylor.com**

You will receive an acknowledgment within 48 hours and a detailed response within 5 business days.

## Security Model

MacJet operates with elevated privileges when run with `sudo`. Here is how we protect users:

### Process Management Safety
The MCP server can terminate processes on behalf of AI agents (`kill_process` sends **SIGTERM**). Protections:
1. **PID validation** — PIDs **below 500**, the MCP server’s own PID, and nonexistent processes are rejected.
2. **Elicitation** — When the MCP client supports it, `kill_process` prompts for **`confirm_terminate`** before sending SIGTERM. Clients **without** elicitation support fall back to executing the kill after validation (agents must still supply `reason` for auditing).
3. **Read-only mode** — Set **`MACJET_MCP_READONLY=1`** to omit `kill_process` from the tool list.
4. **Audit logging** — Each attempt is appended to **`~/.macjet/mcp_audit.jsonl`** (JSON lines) with tool name, PID, process name, signal, reason, success, and client/request metadata.

### Data Privacy
- MacJet collects process data **locally only**. No data is sent to external servers.
- The MCP server communicates over **stdio** (local pipe). No network sockets are opened.
- Chrome tab data is queried via **localhost-only** DevTools Protocol connections.

### Supported Versions

| Version | Supported | Notes |
|---------|-----------|-------|
| 2.0.x | ✅ | 100% Rust Rewrite |
| < 2.0 | ❌ | Python Textual (Deprecated) |
