# Contributing to MacJet

Thank you for your interest in contributing to MacJet! MacJet is written in 100% Rust. This document will help you get started.

## Development Setup

### Prerequisites
- macOS 12 or later
- Apple Silicon or Intel Mac
- [Rust 1.75+](https://rustup.rs/)
- Git

### Getting Started

```bash
# 1. Fork and clone
git clone https://github.com/YOUR_USERNAME/macjet.git
cd macjet

# 2. Build the project
cargo build

# 3. Run MacJet natively
sudo cargo run --release       # Full mode (recommended for thermals)
cargo run --release            # Basic mode (no energy/thermal)
```

### Running the MCP Server in the Background
```bash
cargo run --release -- --mcp
```

## Demo assets (GIF and screenshots)

The README demo media is generated with **[VHS](https://github.com/charmbracelet/vhs)** from [scripts/demo.tape](scripts/demo.tape).

1. Cache sudo credentials once: `sudo -v`
2. From the repo root: `vhs scripts/demo.tape`

That writes **`assets/macjet_demo.gif`** plus **`assets/view_apps.png`**, **`assets/view_energy.png`**, and **`assets/view_reclaim.png`**. Re-run after UI changes so marketing assets match the product.

Performance tables and methodology for README charts live in **[docs/benchmarks.md](docs/benchmarks.md)**.

## Code Style

We use the standard Rust toolchain for formatting and linting.

```bash
# Format
cargo fmt

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Test
cargo test
```

All PRs must pass these checks. The CI pipeline runs them automatically.

## Project Structure

```
macjet/
├── Cargo.toml            # Dependencies and metadata
├── src/
│   ├── main.rs           # Entry point
│   ├── app.rs            # State management
│   ├── collectors/       # Data collection (sysinfo, powermetrics)
│   ├── inspectors/       # Context enrichment (Chrome CDP)
│   ├── mcp/              # MCP server components (tools, resources)
│   └── ui/               # Ratatui UI widgets (tree, inspector)
└── docs/                 # Documentation
```

## Pull Request Process

1. **Fork** the repository and create a feature branch from `main`.
2. **Write your code** following the existing rust patterns.
3. **Test** your changes lightly by running MacJet locally.
4. **Run checks**: `cargo fmt --check` and `cargo clippy`.
5. **Submit a PR** with a clear description of what you changed.

## Good First Issues
- **New inspectors**
- **View enhancements**
- **MCP tools**
