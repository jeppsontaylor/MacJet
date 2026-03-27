#!/usr/bin/env bash
# Build the release binary and run the VHS demo tape from the repo root.
# See docs/vhs-demo-recording.md — tape runs without sudo so VHS never blocks on Password:.
#
# After VHS, we require minimum output sizes so a stuck prompt or broken run (tiny GIF)
# cannot pass silently. Override with VHS_DEMO_MIN_GIF_BYTES / VHS_DEMO_MIN_PNG_BYTES if needed.

set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MIN_GIF_BYTES="${VHS_DEMO_MIN_GIF_BYTES:-70000}"
MIN_PNG_BYTES="${VHS_DEMO_MIN_PNG_BYTES:-12000}"

die() {
	echo "record-demo.sh: ERROR: $*" >&2
	exit 1
}

validate_demo_assets() {
	local gif="assets/macjet_demo.gif"
	local pngs=(
		assets/view_apps.png
		assets/view_energy.png
		assets/view_reclaim.png
	)
	local f sz

	for f in "$gif" "${pngs[@]}"; do
		[[ -f "$f" ]] || die "missing output file $f (VHS did not produce expected assets)"
	done

	sz=$(wc -c <"$gif" | tr -d ' ')
	((sz >= MIN_GIF_BYTES)) || die \
		"$gif is only ${sz} bytes (minimum ${MIN_GIF_BYTES}). Recording failed or never showed a real session — do not commit. If you switched the tape to sudo, see docs/vhs-demo-recording.md."

	for f in "${pngs[@]}"; do
		sz=$(wc -c <"$f" | tr -d ' ')
		((sz >= MIN_PNG_BYTES)) || die \
			"$f is only ${sz} bytes (minimum ${MIN_PNG_BYTES}). Demo screenshots look broken — do not commit."
	done
}

cargo build --release
# Inherited NO_COLOR would make crossterm emit no ANSI colors (monochrome GIF).
unset NO_COLOR 2>/dev/null || true
export COLORTERM=truecolor
export TERM=xterm-256color
vhs scripts/demo.tape
validate_demo_assets
echo "record-demo.sh: OK — asset sizes look sane (GIF >= ${MIN_GIF_BYTES} B, PNGs >= ${MIN_PNG_BYTES} B)."
