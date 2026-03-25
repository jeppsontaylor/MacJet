.PHONY: install test lint format typecheck clean all

# Default: run lint + tests
all: lint test

# Install all dependencies (including dev and mcp extras)
install:
	pip install -e ".[dev,mcp]"

# Run the full test suite with coverage
test:
	pytest tests/ -v --tb=short --cov=macjet --cov-report=term-missing

# Run tests matching a keyword (usage: make test-k K=sparkline)
test-k:
	pytest tests/ -v -k "$(K)"

# Lint with ruff
lint:
	ruff check .

# Auto-format with black
format:
	black .

# Check formatting without modifying files
format-check:
	black --check .

# Type checking (requires pyright or mypy)
typecheck:
	python -m mypy macjet/ --ignore-missing-imports

# Run all CI checks locally (mirrors what GitHub Actions does)
ci: format-check lint test

# Remove build artifacts and caches
clean:
	find . -type d -name __pycache__ -exec rm -rf {} +
	find . -type d -name .pytest_cache -exec rm -rf {} +
	find . -type d -name "*.egg-info" -exec rm -rf {} +
	rm -rf .coverage htmlcov/ dist/ build/
