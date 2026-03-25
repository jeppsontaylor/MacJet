# Contributing to MacJet

Thank you for your interest in contributing to MacJet! This document will help you get started.

## Development Setup

### Prerequisites
- macOS 12 or later
- Python 3.10+
- Git

### Getting Started

```bash
# 1. Fork and clone
git clone https://github.com/YOUR_USERNAME/macjet.git
cd macjet

# 2. Create a virtual environment
python3 -m venv venv
source venv/bin/activate

# 3. Install in development mode
pip install -e ".[dev,mcp]"

# 4. Run MacJet
sudo python -m macjet    # Full mode
python -m macjet          # Basic mode (no energy/thermal)
```

### Running the MCP Server

```bash
python macjet_mcp.py
```

## Code Style

We use [Black](https://github.com/psf/black) for formatting and [Ruff](https://github.com/astral-sh/ruff) for linting.

```bash
# Format
black .

# Lint
ruff check .

# Lint and auto-fix
ruff check --fix .
```

All PRs must pass these checks. The CI pipeline runs them automatically.

## Project Structure

```
macjet/
├── collectors/          # Data collection (psutil, powermetrics, network)
│   ├── process_collector.py   # Process enumeration and grouping
│   ├── energy_collector.py    # powermetrics integration
│   ├── network_collector.py   # Network I/O tracking
│   └── metrics_history.py     # Ring buffers, smoothing, scoring
├── inspectors/          # Context enrichment
│   ├── browser_inspector.py   # Browser tab detection
│   ├── ide_inspector.py       # IDE project detection
│   ├── chrome_tab_mapper.py   # Chrome CDP integration
│   └── ...
├── mcp/                 # MCP server components
│   ├── tools.py               # Tool handlers
│   ├── resources.py           # Resource handlers
│   ├── models.py              # Pydantic schemas
│   ├── safety.py              # PID validation + audit
│   └── cache.py               # Async TTL cache
├── ui/                  # Textual UI widgets
│   ├── header.py              # System header strip
│   ├── process_tree.py        # Process table with colormaps
│   ├── detail_panel.py        # Inspector pane
│   ├── reclaim_panel.py       # Kill List panel
│   └── theme.tcss             # Dark graphite stylesheet
├── native/              # macOS-specific helpers
├── macjet_app.py    # Main TUI application
└── __main__.py          # Entry point
```

## Pull Request Process

1. **Fork** the repository and create a feature branch from `main`.
2. **Write your code** following the existing patterns and code style.
3. **Test** your changes by running MacJet locally.
4. **Run linters**: `black --check .` and `ruff check .`
5. **Submit a PR** with a clear description of what you changed and why.

### PR Guidelines
- Keep PRs focused — one feature or fix per PR.
- Update documentation if you change behavior.
- Add a changelog entry for user-facing changes.
- Reference related issues using `Fixes #123` or `Closes #123`.

## Good First Issues

Looking for where to start? Here are some areas that are great for first contributions:

- **New inspectors** — Add context enrichment for more apps (Figma, Slack, Discord)
- **View enhancements** — Improve the Pressure or Energy views
- **MCP tools** — Add new tools (e.g., `get_disk_usage`, `get_bluetooth_devices`)
- **Documentation** — Improve docs, fix typos, add examples
- **Color themes** — Design alternative color palettes

## Reporting Bugs

Please use the [bug report template](https://github.com/jepsontaylor/macjet/issues/new?template=bug_report.yml) and include:
- macOS version
- Python version
- Apple Silicon or Intel
- Steps to reproduce
- Expected vs. actual behavior

## Questions?

Open a [Discussion](https://github.com/jepsontaylor/macjet/discussions) for questions, ideas, or general conversation.
