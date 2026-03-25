# MacJet Architecture

MacJet is a high-performance terminal UI and MCP server built for macOS. It collects system metrics in real-time, processes them for UI rendering, and exposes them to AI agents via the Model Context Protocol.

## 🏗 High-Level Architecture

```mermaid
graph TD
    subgraph Collectors
        PC["ProcessCollector<br/>(psutil, 1s)"]
        EC["EnergyCollector<br/>(powermetrics)"]
        NC["NetworkCollector<br/>(psutil)"]
    end

    subgraph Data Layer
        MH["MetricsHistory<br/>Ring Buffers + Smoother"]
        RS["Reclaim Scorer"]
    end

    subgraph UI
        H["SystemHeader<br/>2-line branded strip"]
        PT["ProcessTree<br/>Semantic colormaps"]
        DP["DetailPanel<br/>Inspector"]
        RP["ReclaimPanel<br/>Kill List"]
    end

    subgraph MCP Server
        FMP["FastMCP<br/>10 tools, 6 resources"]
        SAF["Safety Layer<br/>PID validation + audit"]
    end

    PC -->|feeds| MH
    MH -->|sparklines| PT
    MH -->|scoring| RS
    RS -->|candidates| RP
    PC -->|groups| PT
    PC -->|groups| DP
    EC -->|thermals| H
    NC -->|throughput| H
    PC -->|data| FMP
    FMP -->|kill| SAF
```

## 🔄 Data Flow

1. **ProcessCollector**: The core engine that runs a collection loop every second using `psutil`. It fetches process info, handles Chrome renderer identification, identifies parent-child relationships, and groups them by application name.
2. **MetricsHistory**: Stores historical context using ring buffers, allowing MacJet to display 60-second sparklines and calculate memory growth over time.
3. **UI Widgets**: `Textual` reads from `MetricsHistory` to render the UI. Views like `ProcessTree` subscribe to these updates and apply semantic colormaps based on process metrics.
4. **Reclaim Scorer**: A specialized heuristic engine that continuously evaluates process groups on a 100-point scale based on CPU, memory, leaks, and process characteristics to feed the Reclaim (Kill List) view.

## 🤖 MCP Server Architecture

The MCP Server lets AI agents interface directly with MacJet.
- **Lifespan**: Managed through `FastMCP`.
- **Tools**: 10 purpose-built tools allowing read/write system interactions.
- **Safety Layer**: All write/destructive actions (like `kill_process`) are routed through an elicitation process, prompting the human to confirm before executing. System PIDs (< 500) are heavily restricted.
- **Audit Logging**: Every action is saved into an audit log (accessible via the `macjet://audit/log` resource).

## 🔎 Inspectors

MacJet connects to several domains for enriched data:
- **Terminal (psutil)**: Baseline tree, IO, memory, threading.
- **Hardware (powermetrics)**: Optional `sudo` integration for deep CPU die temps, fan speed, thermal pressure, and strict per-process energy impact.
- **Browser (Chrome CDP)**: Identifies tab titles for Chrome renderer PIDs.
