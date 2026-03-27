<p align="center">
  <h1 align="center">🔥 MacJet v2.0.1</h1>
  <p align="center">
    <strong>The flight deck for your Mac. 100% Rust. 0.0% CPU.</strong>
  </p>
  <p align="center">
    Real-time process monitoring, thermal intelligence, and an AI-native MCP server — all in your terminal.
  </p>
  <p align="center">
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="MIT License"></a>
    <img src="https://img.shields.io/badge/rust-1.75+-000000.svg?logo=rust&logoColor=white" alt="Rust 1.75+">
    <img src="https://img.shields.io/badge/platform-macOS-000000.svg?logo=apple" alt="macOS">
    <img src="https://img.shields.io/badge/MCP-compatible-8A2BE2.svg" alt="MCP Compatible">
    <a href="https://github.com/jepsontaylor/macjet/pulls"><img src="https://img.shields.io/badge/PRs-welcome-brightgreen.svg" alt="PRs Welcome"></a>
  </p>
</p>

<p align="center">
  <img src="assets/macjet_demo.gif" alt="Screen recording: MacJet terminal UI cycling through Apps, Tree, Pressure, Energy, and Reclaim views" width="800">
</p>

<p align="center"><strong>Still frames</strong> — Apps, Energy, Reclaim</p>
<table>
  <tr>
    <td align="center" width="33%">
      <img src="assets/view_apps.png" alt="MacJet Apps view: processes grouped by application with CPU and memory columns" width="100%">
      <br><sub>Apps</sub>
    </td>
    <td align="center" width="33%">
      <img src="assets/view_energy.png" alt="MacJet Energy view: wakeups, thermal state, and battery impact metrics" width="100%">
      <br><sub>Energy</sub>
    </td>
    <td align="center" width="33%">
      <img src="assets/view_reclaim.png" alt="MacJet Reclaim view: scored kill list with risk bands" width="100%">
      <br><sub>Reclaim</sub>
    </td>
  </tr>
</table>

---

MacJet is a high-performance, developer-first terminal dashboard designed to answer the one question every Mac user asks eventually: **"Why does my laptop sound like a jet engine?"**

Version **2.0.1** is a complete ground-up rewrite in **Rust** using `ratatui` and `tokio`. On our reference machine (M4 Max, macOS 15.5), average CPU while idle matches **Activity Monitor** at **~0%**; the big win is versus the old **Python/Textual** build (**~14.7%** average CPU in the same setup). Full tables and methodology: [docs/benchmarks.md](docs/benchmarks.md).

## ⚡ Quick Start

```bash
git clone https://github.com/jepsontaylor/macjet.git
cd macjet
cargo install --path .
```

Then, launch it:
```bash
sudo macjet
```

### From GitHub Releases

Each tagged release attaches **`macjet-macos-universal.tar.gz`** (fat binary: Apple Silicon + Intel). Extract `macjet`, place it on your `PATH`, and clear Gatekeeper quarantine if macOS blocks it: `xattr -dr com.apple.quarantine /path/to/macjet`.

### To `sudo` or not to `sudo`?

MacJet is designed to run gracefully with or without root privileges, but `sudo` unlocks its true power by granting access to Apple's low-level sensors.

**With `sudo` (Recommended):**
- 🌡️ **Thermal Data**: Access CPU/GPU die temperatures and Fan speeds (RPM).
- 🔋 **Energy Impact**: Accurate, hardware-level energy scoring via `powermetrics`.
- 🛡️ **Full Control**: Ability to analyze and manage any process on the system, not just your own user processes.

**Without `sudo`:**
- 📊 **App Grouping & UI**: Full access to the terminal UI, App-centric grouping, and Chrome tab mapping.
- 📉 **Basic Metrics**: Standard CPU/Memory metrics.
- 🔒 **Restricted Control**: Can only interact with your own user-level processes. Thermal data and energy metrics will be disabled.

---

## 🚀 Performance: Activity Monitor vs MacJet v1 (Python) vs v2.0.1 (Rust)

*Measured with windows open and idle; averages over 65 samples (v1) and 300 samples (v2.0.1) on Apple M4 Max, macOS 15.5 — see [docs/benchmarks.md](docs/benchmarks.md).*

<p align="center">
  <img src="assets/benchmark_cpu.svg" alt="Bar chart: MacJet v1 Python average CPU 14.7 percent; Activity Monitor and MacJet v2 Rust near zero" width="640">
</p>

```mermaid
xychart-beta
    title "CPU usage — average % (lower is better)"
    x-axis ["Activity Monitor", "MacJet v1 (Python)", "MacJet v2.0.1 (Rust)"]
    y-axis "CPU %" 0 --> 30
    bar [0.0, 14.7, 0.0]
```

**Why two near-zero bars?** Both **Activity Monitor** and **MacJet v2.0.1** rounded to **0.0%** average CPU in these runs. The headline comparison is **legacy MacJet (Python) → Rust**: **~14.7% → ~0%** average CPU.

**Memory (RSS)**: v1 averaged **~27 MB**; v2.0.1 averaged **~109 MB** (Activity Monitor **~64 MB** in the same session). v2 trades higher baseline RAM for Tokio parallelism, sparkline history, and **no sustained CPU wakeups** — details in [docs/benchmarks.md](docs/benchmarks.md).

---

## ✨ Features

### 🎯 Flight Deck Layout
Five purpose-built views, switchable with `1`–`5` or `Tab`:

| View | Purpose |
|------|---------|
| **Apps** | Processes grouped by application with role-bucket expansion |
| **Tree** | Raw process hierarchy |
| **Pressure** | Memory pressure focus |
| **Energy** | Wakeups, thermal state, battery impact |
| **Reclaim** | Intelligent Kill List with scored recommendations |

### 🧠 Reclaim Engine (Kill List)
A multi-factor scoring engine ranks every process group on a 100-point scale based on: sustained CPU load, memory footprint, memory growth (leaks), process storms, and high wakeups. Target high-score apps to reclaim your system.

### 🌐 Chrome Tab Mapping
Connects to Chrome's DevTools Protocol to map every renderer PID to its actual website tab title. Stop guessing which "Google Chrome Helper (Renderer)" is drawing 100% CPU. Enable with `open -a "Google Chrome" --args --remote-debugging-port=9222`.

### 🤖 Built-in MCP Server
MacJet ships a native **Model Context Protocol (MCP)** server natively exposing 10 tools to AI Agents (like Claude Desktop). 

To use it, just configure your MCP client:
```json
{
  "mcpServers": {
    "macjet": {
      "command": "/Users/YOU/.cargo/bin/macjet",
      "args": ["--mcp"],
      "description": "macOS process monitor — CPU, memory, energy, Chrome tabs, process management"
    }
  }
}
```

> 📖 Read the full MCP capabilities in [docs/mcp.md](docs/mcp.md)

---

## ⌨️ Keybindings

| Key | Action |
|-----|--------|
| `1`–`5` | Switch view (Apps / Tree / Pressure / Energy / Reclaim) |
| `Tab` | Cycle through views |
| `↑` `↓` | Navigate rows |
| `Enter` | Expand / collapse group or role bucket |
| `s` | Cycle sort mode (CPU / Memory / Name / PID) |
| `/` | Filter processes by name |
| `Esc` | Clear filter |
| `h` | Hide / show system processes |
| `k` | Kill selected (SIGTERM) |
| `K` | Force kill (SIGKILL) |
| `z` | Suspend / Resume |
| `w` | Show context in inspector |
| `?` | Help |
| `q` | Quit |

---

## 🤝 Contributing & Architecture

We welcome contributions! See the [architecture doc](docs/architecture.md), [benchmarks](docs/benchmarks.md), and [CONTRIBUTING.md](CONTRIBUTING.md) to get started with the Rust codebase.

## 📜 License
[MIT](LICENSE) © [Jepson Taylor](https://github.com/jepsontaylor)
