# Changelog

All notable changes to MacJet will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Changed
- **MCP server** — Headless collector loop shares the TUI data path; tools and resources return live JSON with a `meta` envelope (`schema_version`, `capabilities`, timestamps). Added resource templates, subscribe + `notifications/resources/updated`, prompts, completions stub, logging `setLevel`, `kill_process` elicitation (with fallback when unsupported), `MACJET_MCP_READONLY`, and `AsyncTTLCache` for reads. Documentation and audit path (`~/.macjet/mcp_audit.jsonl`) aligned with the implementation.

## [2.0.1] — 2026-03-26

First **Rust-only** release and the supported baseline going forward. This tag supersedes the Python/Textual line for production use.

### Added
- **100% Rust rewrite** — The entire application has been rewritten from Python to Rust.
- **Ratatui interface** — Terminal UI on `ratatui` with a high-frequency render loop.
- **Idle CPU** — Background collectors use Tokio scheduling; on our M4 Max reference setup, average CPU while idle matches Activity Monitor (~0%); see [docs/benchmarks.md](docs/benchmarks.md).
- **Gatekeeper** — Notes for `com.apple.quarantine` on downloaded binaries.

### Changed
- Removed `Textual` and all Python packaging from the repository.
- Replaced `psutil` with Rust `sysinfo` and native collectors.
- Higher baseline RSS than v1 (~109 MB vs ~27 MB in benchmarks) in exchange for far lower sustained CPU when idle.

### Migration (from pre-2.0 / Python)
- Older installs used **Python + Textual** (e.g. `pip`, `python -m macjet`). Use **`cargo install --path .`** or a **v2.0.1+** GitHub release asset instead; the Python codebase is **not** maintained in this repo after 2.0.1.
- MCP config: point `command` at the `macjet` binary (for example `~/.cargo/bin/macjet` after `cargo install`).

## [0.4.0] — 2026-03-25

**Historical (Python line):** This version describes the last **pre-Rust** feature set. The same capabilities ship in **2.0.1** as a native binary; keep this entry for changelog continuity only.

### Added
- **Flight Deck layout** — Adaptive master-detail layout with 5 views (Apps, Tree, Pressure, Energy, Reclaim)
- **Reclaim (Kill List)** — Intelligent scoring engine ranks processes by reclaimability with risk bands (Safe/Review/Danger)
- **MCP Server** — (Superseded by Unreleased: live MCP upgrade above.) Earlier docs referenced 10 tools / 6 resources; see current [docs/mcp.md](docs/mcp.md).
