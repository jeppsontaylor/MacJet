#!/usr/bin/env bash
# ~6 minute benchmark_compare run (same methodology as long runs, shorter wall clock).
# Wall time ≈ (37 − 1) × 10s = 360s of sleeps between samples, plus first sample + overhead.
#
# Requires free disk space — JSON is written at the end to benchmarks/results/ (default).
# Usage (from repo root):
#   ./scripts/benchmark_quick_6min.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
exec ./macjet.sh bench --no-ml --max-samples 37 --refresh 10
