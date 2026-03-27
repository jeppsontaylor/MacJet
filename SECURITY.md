# Security Policy

## Reporting a Vulnerability
If you discover a security vulnerability in MacJet, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email: **security@jepsontaylor.com**

You will receive an acknowledgment within 48 hours and a detailed response within 5 business days.

## Security Model

MacJet operates with elevated privileges when run with `sudo`. Here is how we protect users:

### Process Management Safety
The MCP server can kill and suspend processes on behalf of AI agents. These operations are protected by multiple layers:
1. **PID Validation** — System-critical processes (PID < 500), the MCP server itself, and known macOS daemons are blocked from termination.
2. **Human-in-the-Loop Confirmation** — The `kill_process` tool uses MCP elicitation to request explicit user confirmation before executing any kill signal.
3. **Audit Logging** — Every kill, suspend, and resume action is logged with timestamp, client ID, request ID, PID, process name, signal, and reason.
4. **Graceful Escalation** — Default signal is `SIGTERM` (allows cleanup). `SIGKILL` requires explicit force.

### Data Privacy
- MacJet collects process data **locally only**. No data is sent to external servers.
- The MCP server communicates over **stdio** (local pipe). No network sockets are opened.
- Chrome tab data is queried via **localhost-only** DevTools Protocol connections.

### Supported Versions

| Version | Supported | Notes |
|---------|-----------|-------|
| 2.0.x | ✅ | 100% Rust Rewrite |
| < 2.0 | ❌ | Python Textual (Deprecated) |
