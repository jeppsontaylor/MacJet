# Changelog

All notable changes to MacJet will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] — 2026-03-25

### Added
- **Flight Deck layout** — Adaptive master-detail layout with 5 views (Apps, Tree, Pressure, Energy, Reclaim)
- **Reclaim (Kill List)** — Intelligent scoring engine ranks processes by reclaimability with risk bands (Safe/Review/Danger)
- **Semantic colormaps** — Afterburner (CPU: cyan → hot pink) and Aurora (Memory: green → red) color ramps
- **Role-bucket grouping** — Chrome children grouped by role (Renderer ×12, GPU ×1, Utility ×3) with two-level expansion
- **Per-process sparklines** — 60-second CPU trend lines using braille characters
- **Exponential smoothing** — Stable row ordering via α=0.3 smoothed metrics
- **Ring buffer infrastructure** — Per-PID `MetricsHistory` store powering sparklines, trends, and scoring
- **2-line branded header** — Machine model, CPU bar, swap, thermal dot, network throughput
- **View switching** — `1-5` keys and `Tab` to cycle between Apps/Tree/Pressure/Energy/Reclaim
- **System process hiding** — `h` key toggles system daemon visibility
- **MCP Server** — 10 tools, 6 resources, 3 prompts for AI agent integration
- **Chrome CDP tab mapping** — Maps renderer PIDs to tab titles via DevTools Protocol
- **Inspector pane** — Sparklines, why-hot analysis, role breakdowns, memory trends
- **Dark graphite theme** — `#0A0F1E` base with OKLCH-informed semantic colors

### Changed
- Header reduced from 3 lines to 2 lines
- HeatChart top section removed (history data moved to per-process ring buffers)
- Why-Hot panel absorbed into the inspector pane
- Process tree now shows severity rail, inline sparklines, and enriched "more" rows

## [0.3.0] — 2026-03-24

### Added
- Initial TUI with process tree and heat chart
- Energy collector via `powermetrics`
- Network collector
- Browser, IDE, container, and terminal inspectors
- `macjet.sh` launcher with `--doctor`, `--update`, `--mcp` modes
