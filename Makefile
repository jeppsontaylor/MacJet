.PHONY: all build release test lint clippy clean mcp run install format check
.DEFAULT_GOAL := help

# --- Colors ---
RED=\033[0;31m
GREEN=\033[0;32m
YELLOW=\033[0;33m
BLUE=\033[0;34m
NC=\033[0m # No Color

# --- Config ---
BIN=macjet

help: ## Show this help message
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n\nTargets:\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

build: ## Build the native binary in debug mode (fast compile)
	@echo "${BLUE}Building ${BIN} (Debug)...${NC}"
	cargo build

release: ## Build the optimized production binary
	@echo "${BLUE}Building ${BIN} (Release)...${NC}"
	cargo build --release

run: ## Run the TUI natively
	cargo run --release

mcp: ## Run the background MCP server natively
	cargo run --release -- --mcp

test: ## Run the entire test suite equivalent to 1:1 python specs
	@echo "${GREEN}Running test suite...${NC}"
	cargo test -- --show-output

test-mcp: ## Test specifically the MCP models and caching boundaries
	cargo test mcp:: -- --show-output

clippy: lint ## Alias for lint
lint: ## Run cargo clippy strict lints
	@echo "${YELLOW}Running clippy lints...${NC}"
	cargo clippy --all-targets --all-features -- -D warnings

format: ## Run cargo fmt
	cargo fmt --all

check: format lint test ## Run format, lint, and tests (CI prep)

clean: ## Remove the target/ compilation directory
	cargo clean

install: release ## Install the binary to ~/.cargo/bin
	cargo install --path .
