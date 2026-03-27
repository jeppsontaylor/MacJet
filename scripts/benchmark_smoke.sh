#!/usr/bin/env bash
# ~2–3 second sanity check: 3 samples × 1 s, writes to /tmp (writable even when repo dirs are not).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="/tmp/macjet_benchmark_smoke.json"
(cargo build --release --bin benchmark_compare --quiet) || exit $?
"$ROOT/target/release/benchmark_compare" --no-ml --max-samples 3 --refresh 1 --output "$OUT"
echo "OK — wrote $OUT"
