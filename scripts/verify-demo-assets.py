#!/usr/bin/env python3
"""
Reject VHS demo outputs that look like a failed recording (e.g. sudo Password: screen).

Uses only the Python standard library. Thresholds are conservative; if a real UI change
legitimately fails, adjust MIN_* constants or file an issue.
"""

from __future__ import annotations

import os
import shutil
import struct
import subprocess
import sys
import zlib
from pathlib import Path


# --- Size floors (bytes): catches empty / truncated writes ---------------------------------
MIN_GIF_BYTES = 18_000
MIN_PNG_BYTES = 12_000

# PNG must match VHS layout in scripts/demo.tape (Set Width / Set Height).
EXPECTED_PNG_WIDTH = 1680
EXPECTED_PNG_HEIGHT = 900

# Within-row luminance stdev (Molokai terminal). Full MacJet UI has many "busy" rows;
# a stuck sudo prompt is mostly blank with a couple of text lines.
MIN_ROW_STDEV = 8.0
MIN_FRAC_ROWS_BUSY = 0.035  # e.g. ~28 rows on an 800px-tall frame
MIN_MAX_ROW_STDEV = 45.0  # at least one row with strong horizontal variation (tree, bars, etc.)


def _fail(msg: str) -> None:
    print(f"verify-demo-assets: {msg}", file=sys.stderr)


def _decode_png_rgba8(path: Path) -> tuple[int, int, bytes]:
    data = path.read_bytes()
    if len(data) < 32 or data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path.name}: not a PNG")
    i = 8
    idat = bytearray()
    width = height = 0
    bit_depth = color_type = interlace = None
    while i < len(data):
        if i + 8 > len(data):
            break
        length = struct.unpack(">I", data[i : i + 4])[0]
        chunk_type = data[i + 4 : i + 8]
        i += 8
        chunk = data[i : i + length]
        i += length
        i += 4  # CRC
        if chunk_type == b"IHDR":
            width, height, bit_depth, color_type, _, _, interlace = struct.unpack(
                ">IIBBBBB", chunk
            )
        elif chunk_type == b"IDAT":
            idat.extend(chunk)
        elif chunk_type == b"IEND":
            break
    if interlace != 0:
        raise ValueError(f"{path.name}: interlaced PNG not supported")
    if bit_depth != 8 or color_type not in (2, 6):
        raise ValueError(f"{path.name}: expected 8-bit RGB or RGBA (got ct={color_type})")
    bpp = 3 if color_type == 2 else 4
    raw = zlib.decompress(bytes(idat))
    stride = width * bpp
    out = bytearray(height * stride)
    prev = bytearray(stride)
    j = 0
    for y in range(height):
        if j >= len(raw):
            raise ValueError(f"{path.name}: truncated IDAT")
        filt = raw[j]
        j += 1
        line = bytearray(raw[j : j + stride])
        j += stride
        if filt == 1:
            for x in range(stride):
                left = line[x - bpp] if x >= bpp else 0
                line[x] = (line[x] + left) & 255
        elif filt == 2:
            for x in range(stride):
                line[x] = (line[x] + prev[x]) & 255
        elif filt == 3:
            for x in range(stride):
                left = line[x - bpp] if x >= bpp else 0
                line[x] = (line[x] + ((left + prev[x]) // 2)) & 255
        elif filt == 4:
            def paeth(a: int, b: int, c: int) -> int:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                if pa <= pb and pa <= pc:
                    return a
                if pb <= pc:
                    return b
                return c

            for x in range(stride):
                a = line[x - bpp] if x >= bpp else 0
                b = prev[x]
                c = prev[x - bpp] if x >= bpp else 0
                line[x] = (line[x] + paeth(a, b, c)) & 255
        elif filt != 0:
            raise ValueError(f"{path.name}: unsupported filter {filt}")
        out[y * stride : (y + 1) * stride] = line
        prev = line
    return width, height, bytes(out)


def _png_row_metrics(path: Path) -> tuple[float, float]:
    w, h, px = _decode_png_rgba8(path)
    if w != EXPECTED_PNG_WIDTH or h != EXPECTED_PNG_HEIGHT:
        raise ValueError(f"{path.name}: expected {EXPECTED_PNG_WIDTH}x{EXPECTED_PNG_HEIGHT}, got {w}x{h}")
    if len(px) % (w * h) != 0:
        raise ValueError(f"{path.name}: unexpected pixel buffer size")
    bpp = len(px) // (w * h)
    if bpp not in (3, 4):
        raise ValueError(f"{path.name}: expected RGB or RGBA bytes-per-pixel, got {bpp}")
    busy_rows = 0
    max_row = 0.0
    for y in range(h):
        row = px[y * w * bpp : (y + 1) * w * bpp]
        lums: list[float] = []
        for x in range(0, len(row), bpp):
            r, g, b = row[x], row[x + 1], row[x + 2]
            lums.append(0.299 * r + 0.587 * g + 0.114 * b)
        n = len(lums)
        if n < 2:
            continue
        mean = sum(lums) / n
        var = sum((L - mean) ** 2 for L in lums) / n
        stdev = var**0.5
        max_row = max(max_row, stdev)
        if stdev >= MIN_ROW_STDEV:
            busy_rows += 1
    frac_busy = busy_rows / h
    return frac_busy, max_row


def _gif_frame_count_scan(data: bytes) -> int:
    if len(data) < 13 or data[:6] not in (b"GIF87a", b"GIF89a"):
        return 0
    i = 13
    flags = data[10]
    if flags & 0x80:
        i += 3 * (1 << ((flags & 7) + 1))
    count = 0
    while i < len(data):
        b = data[i]
        if b == 0x3B:
            break
        if b == 0x21:
            i += 2
            while i < len(data):
                bs = data[i]
                i += 1
                if bs == 0:
                    break
                i += bs
            continue
        if b == 0x2C:
            count += 1
            i += 1
            if i + 9 > len(data):
                break
            packed = data[i + 8]
            i += 9
            if packed & 0x80:
                i += 3 * (1 << ((packed & 7) + 1))
            if i < len(data):
                i += 1
            while i < len(data):
                bs = data[i]
                i += 1
                if bs == 0:
                    break
                i += bs
            continue
        i += 1
    return count


def _gif_frame_count(path: Path) -> int:
    ffprobe = shutil.which("ffprobe")
    if ffprobe:
        try:
            proc = subprocess.run(
                [
                    ffprobe,
                    "-v",
                    "error",
                    "-count_frames",
                    "-select_streams",
                    "v:0",
                    "-show_entries",
                    "stream=nb_read_frames",
                    "-of",
                    "default=nokey=1:noprint_wrappers=1",
                    str(path),
                ],
                capture_output=True,
                text=True,
                timeout=60,
                check=False,
            )
            if proc.returncode == 0 and proc.stdout.strip().isdigit():
                return int(proc.stdout.strip())
        except (OSError, subprocess.TimeoutExpired):
            pass
    return _gif_frame_count_scan(path.read_bytes())


def main() -> int:
    if len(sys.argv) != 2:
        _fail("usage: verify-demo-assets.py <assets-directory>")
        return 2
    assets = Path(sys.argv[1]).resolve()
    if not assets.is_dir():
        _fail(f"not a directory: {assets}")
        return 2

    gif = assets / "macjet_demo.gif"
    pngs = [
        assets / "view_apps.png",
        assets / "view_energy.png",
        assets / "view_reclaim.png",
    ]

    for p in [gif, *pngs]:
        if not p.is_file():
            _fail(f"missing output file: {p} (VHS did not produce expected assets)")
            return 1
        size = p.stat().st_size
        if p.suffix.lower() == ".gif" and size < MIN_GIF_BYTES:
            _fail(
                f"{p.name} is only {size} bytes (minimum {MIN_GIF_BYTES}). "
                "Recording likely failed early (common cause: sudo password prompt inside VHS)."
            )
            return 1
        if p.suffix.lower() == ".png" and size < MIN_PNG_BYTES:
            _fail(
                f"{p.name} is only {size} bytes (minimum {MIN_PNG_BYTES}). "
                "Screenshot likely corrupt or empty."
            )
            return 1

    frames = _gif_frame_count(gif)
    if frames < 120:
        _fail(
            f"{gif.name} has only {frames} frame(s); expected a full-length demo. "
            "Do not commit this output."
        )
        return 1

    for png in pngs:
        try:
            frac, mx = _png_row_metrics(png)
        except ValueError as e:
            _fail(str(e))
            return 1
        if frac < MIN_FRAC_ROWS_BUSY or mx < MIN_MAX_ROW_STDEV:
            _fail(
                f"{png.name} looks like a sparse terminal (e.g. stuck on `sudo` Password:), "
                f"not MacJet UI (busy_row_frac={frac:.4f}, max_row_stdev={mx:.2f}; "
                f"need >={MIN_FRAC_ROWS_BUSY} and >={MIN_MAX_ROW_STDEV}). "
                "Fix sudo for VHS: docs/vhs-demo-recording.md — then re-run ./scripts/record-demo.sh"
            )
            return 1

    print(
        f"verify-demo-assets: OK ({gif.name} {gif.stat().st_size} bytes, {frames} frames; "
        "PNG heuristics passed)"
    )
    return 0


if __name__ == "__main__":
    if os.environ.get("MACJET_SKIP_DEMO_VERIFY") == "1":
        print("verify-demo-assets: SKIPPED (MACJET_SKIP_DEMO_VERIFY=1)", file=sys.stderr)
        sys.exit(0)
    raise SystemExit(main())
