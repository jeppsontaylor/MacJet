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
4. **Graceful Escalation** — Default signal is `SIGTERM` (allows cleanup). `SIGKILL` requires explicit `force=True`.

### Data Privacy

- MacJet collects process data **locally only**. No data is sent to external servers.
- The MCP server communicates over **stdio** (local pipe). No network sockets are opened.
- Chrome tab data is queried via **localhost-only** DevTools Protocol connections.
- The audit log is stored locally at `~/.macjet/audit.log`.

### Privilege Model

| Mode | Privileges | Capabilities |
|------|-----------|--------------|
| `./macjet.sh` | User | CPU, memory, process tree |
| `sudo ./macjet.sh` | Root | Above + energy, thermals, fan, `fs_usage`, `sc_usage` |

### Supported Versions

| Version | Supported |
|---------|-----------|
| 0.4.x | ✅ |
| < 0.4 | ❌ |
