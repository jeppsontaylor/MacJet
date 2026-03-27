# MacJet Architecture (v2.0 Rust)

MacJet is a high-performance terminal UI and MCP server built for macOS using 100% Rust. It collects system metrics in real-time (`sysinfo`), processes them for UI rendering (`ratatui`), and exposes them to AI agents via the Model Context Protocol (`rmcp`).

**From Python to Rust:** Earlier versions used Python with Textual and `psutil`. **v2.0.1** replaces that stack with a single native binary: Tokio for scheduling collectors and MCP I/O, `sysinfo` plus macOS-specific helpers for metrics, and Ratatui for the terminal UI. The module boundaries (collectors → history → UI / MCP) mirror the old design, but everything ships as Rust crates in `src/`.

## 🏗 High-Level Architecture

```mermaid
graph TD
    subgraph Collectors
        PC["ProcessCollector<br/>(sysinfo, 1s)"]
        EC["EnergyCollector<br/>(powermetrics)"]
        NC["NetworkCollector<br/>(sysinfo)"]
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
        FMP["rmcp Server<br/>10 tools, 6 resources"]
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

1. **ProcessCollector**: The core engine that runs a non-blocking background task every second using `tokio` and `sysinfo`.
2. **MetricsHistory**: Stores historical context using lock-free ring buffers, allowing MacJet to display 60-second sparklines.
3. **UI Widgets**: `ratatui` renders the UI at 60FPS using a clean message-passing architecture (`mpsc` channels).
4. **Reclaim Scorer**: A specialized heuristic engine that continuously evaluates process groups on a 100-point scale.
