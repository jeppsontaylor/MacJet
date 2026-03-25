# AI Agent Integration (MCP Server)

MacJet ships a full **Model Context Protocol (MCP)** server natively. Your AI agent can query system health, diagnose performance issues, explore browser tabs, and even safely manage processes.

## What is MCP?
The Model Context Protocol (MCP) is an open standard that enables AI models to securely interact with local tools and data sources. MacJet acts as an MCP server, giving AI clients native visibility into the macOS state.

## Quick Setup

Add this configuration to your MCP client (like Claude Desktop or Cursor):

```json
{
  "mcpServers": {
    "macjet": {
      "command": "/absolute/path/to/macjet/macjet.sh",
      "args": ["--mcp"],
      "description": "macOS process monitor — CPU, memory, energy, Chrome tabs, process management"
    }
  }
}
```

## Available Tools (10)

MacJet provides 10 tools to AI clients:

| Tool | Full Description | Parameters |
|------|------------------|------------|
| `get_system_overview` | Retrieves a high-level picture of system CPU, memory usage, thermals, and the top most intense process. | None |
| `list_processes` | Gets a summarized list of process groups, sortable and filterable. | `sort_by` (optional), `limit` (optional) |
| `get_process_detail` | Gets an atomic breakdown of a specific process or app group, giving memory maps, children, and context. | `app_name` or `pid` |
| `search_processes` | Search processes by command line string, name, or metadata. | `query` |
| `explain_heat` | Diagnose why the Mac is heating up, drawing from `powermetrics` and CPU logs. | None |
| `get_chrome_tabs` | Fetches memory/CPU usage for Chrome tabs via CDP integration. | None |
| `get_energy_report` | Detailed app-by-app energy impact values (requires sudo `powermetrics`). | None |
| `get_network_activity` | Ranks processes by network byte throughput. | None |
| `kill_process` | Send a SIGTERM/SIGKILL to a particular PID or process group. **Protected by human elicitation.** | `pid`, `force` (bool) |
| `suspend_process` | Sends SIGSTOP/SIGCONT to temporarily suspend or resume a process without quitting it. | `pid`, `action` ("suspend" or "resume") |

## Resources (6)

These act as URI endpoints AI models can request to immediately ingest context.

| URI | Data Provided |
|-----|---------------|
| `macjet://system/overview` | A live, Markdown-formatted snapshot of overall system health. |
| `macjet://processes/top` | A detailed Markdown list of the top 25 process culprits by CPU time. |
| `macjet://processes/{name}` | Live detail sheet for an app (e.g. `macjet://processes/Chrome`). |
| `macjet://chrome/tabs` | A markdown table of all open Chrome tabs and their hardware cost. |
| `macjet://energy/report` | A summary of total power drain by application. |
| `macjet://audit/log` | A continuous historical log of all MCP write actions (kills, suspends). |

## Prompts (3)

MacJet registers built-in prompt templates to guide AI models when troubleshooting.

| Prompt Name | Purpose |
|-------------|---------|
| **Troubleshoot Performance** | Instantly attaches `macjet://system/overview` and `macjet://processes/top`, asking the agent to act as a sysadmin and diagnose lag. |
| **Optimize Chrome Memory** | Drops `macjet://chrome/tabs` into context and frames the agent to recommend tab pruning. |
| **Generate System Report** | Asks the agent to generate a highly structured Markdown system report. |

## Security Model: Extremely Safe

Exposing process-termination to AI models is risky. MacJet handles this gracefully:
1. **PID Validation**: MacJet intrinsically blocks any request to interact with PIDs < 500 (core system) and its own processes.
2. **Elicitation Guard**: The `kill_process` and `suspend_process` tools trigger an MCP native Elicitation Request. The MCP client pauses model execution, presents an *"Approve SIGTERM for PID XXX?"* dialog to the user, and will only proceed upon manual human click.
3. **Audit Trails**: Every interaction writes permanently to `~/.config/macjet/audit.log`, queryable via the `macjet://audit/log` resource.
