#!/usr/bin/env python3
"""
MacJet Demo Recorder
Drives the MacJet TUI via a pty, injects keypresses at timed intervals,
records via asciinema v2 cast format, and converts to GIF via agg.

Features:
  - Cinematic intro: types "sudo macjet" on a realistic shell prompt
  - Auto-copies the final GIF to assets/ for README embedding

Usage:
    sudo python3 scripts/record_demo.py

Output:
    scripts/macjet_demo.cast  — raw asciinema recording
    assets/macjet_demo.gif    — final GIF for README
"""

from __future__ import annotations

import asyncio
import json
import os
import pty
import random
import shutil
import subprocess
import sys
import time
from pathlib import Path

# ─── Configuration ───────────────────────────────────
SCRIPT_DIR = Path(__file__).parent
REPO_ROOT = SCRIPT_DIR.parent
CAST_FILE = SCRIPT_DIR / "macjet_demo.cast"
GIF_FILE = SCRIPT_DIR / "macjet_demo.gif"
ASSETS_GIF = REPO_ROOT / "assets" / "macjet_demo.gif"

# Terminal size for the recording — wide enough for dual-pane
COLS = 160
ROWS = 40

# ─── Cinematic intro ─────────────────────────────────
SHELL_PROMPT = "\033[38;2;100;220;100m❯\033[0m "  # Green chevron prompt
TYPING_COMMAND = "sudo macjet"
CHAR_DELAY_MIN = 0.04  # Seconds between keystrokes (min)
CHAR_DELAY_MAX = 0.09  # Seconds between keystrokes (max)
POST_ENTER_PAUSE = 0.8  # Pause after "pressing enter" before TUI loads

# ─── Key escape sequences ────────────────────────────
KEY_DOWN = b"\x1b[B"
KEY_UP = b"\x1b[A"
KEY_ENTER = b"\r"
KEY_1 = b"1"
KEY_2 = b"2"
KEY_3 = b"3"
KEY_4 = b"4"
KEY_5 = b"5"
KEY_TAB = b"\t"
KEY_H = b"h"
KEY_Q = b"q"
KEY_SLASH = b"/"

# The demo script: list of (delay_seconds, keypress)
# delay is how long to WAIT before sending the key
DEMO_SCRIPT = [
    # Let app start and populate
    (3.5, None),
    # Navigate down 3 rows
    (0.4, KEY_DOWN),
    (0.4, KEY_DOWN),
    (0.4, KEY_DOWN),
    (0.8, None),
    # Expand a group (Chrome or top app)
    (0.0, KEY_ENTER),
    (1.2, None),
    # Navigate into children
    (0.3, KEY_DOWN),
    (0.3, KEY_DOWN),
    (0.3, KEY_DOWN),
    (0.5, None),
    # Second expand (role bucket)
    (0.0, KEY_ENTER),
    (1.0, None),
    (0.3, KEY_DOWN),
    (0.3, KEY_DOWN),
    (0.5, None),
    # Switch to Tree view
    (0.2, KEY_2),
    (1.0, None),
    (0.3, KEY_DOWN),
    (0.3, KEY_DOWN),
    (0.3, KEY_DOWN),
    (0.5, None),
    # Switch to Reclaim view
    (0.3, KEY_5),
    (1.5, None),
    # Navigate reclaim list
    (0.4, KEY_DOWN),
    (0.4, KEY_DOWN),
    (0.4, KEY_DOWN),
    (0.4, KEY_DOWN),
    (0.8, None),
    # Back to Apps view
    (0.3, KEY_1),
    (1.0, None),
    # Hide system processes
    (0.3, KEY_H),
    (0.8, None),
    # Show again
    (0.3, KEY_H),
    (1.0, None),
    # Scroll through top apps
    (0.3, KEY_DOWN),
    (0.3, KEY_DOWN),
    (0.3, KEY_DOWN),
    (0.3, KEY_DOWN),
    (0.3, KEY_DOWN),
    (0.8, None),
    # Quit (end recording)
    (0.5, KEY_Q),
]


def find_python() -> str:
    """Find a working python that can import macjet."""
    candidates = [
        str(Path.home() / ".macjet" / "venv" / "bin" / "python"),
        "python3",
        sys.executable,
    ]
    for candidate in candidates:
        try:
            result = subprocess.run(
                [candidate, "-c", "import macjet"],
                capture_output=True,
                timeout=5,
            )
            if result.returncode == 0:
                return candidate
        except (FileNotFoundError, subprocess.TimeoutExpired):
            continue
    print("ERROR: Could not find a python with macjet installed.")
    print("  Install with: pip install -e . (from the repo root)")
    sys.exit(1)


def find_agg() -> str:
    """Find the agg binary."""
    # Check common homebrew location first
    homebrew_agg = Path("/opt/homebrew/bin/agg")
    if homebrew_agg.exists():
        return str(homebrew_agg)
    # Fall back to PATH
    result = subprocess.run(["which", "agg"], capture_output=True, text=True)
    if result.returncode == 0:
        return result.stdout.strip()
    print("ERROR: agg not found. Install with: brew install agg")
    sys.exit(1)


def write_cast_header(f, cols: int, rows: int):
    """Write asciinema v2 header."""
    header = {
        "version": 2,
        "width": cols,
        "height": rows,
        "timestamp": int(time.time()),
        "env": {"TERM": "xterm-256color", "SHELL": "/bin/zsh"},
        "title": "MacJet — Flight Deck Demo",
    }
    f.write(json.dumps(header) + "\n")


def write_cast_event(f, ts: float, event_type: str, data: str):
    """Write a single asciinema event."""
    event = [round(ts, 6), event_type, data]
    f.write(json.dumps(event) + "\n")


def record_typing_intro(f) -> float:
    """Record a cinematic shell intro: prompt + typing 'sudo macjet' + enter.

    Writes directly to the cast file as synthetic output events.
    Returns the total duration consumed by the intro.
    """
    ts = 0.0

    # Initial blank line pause (feels like terminal just opened)
    ts += 0.6
    write_cast_event(f, ts, "o", SHELL_PROMPT)

    # Brief pause after prompt appears (human reaction time)
    ts += 0.4

    # Type the command character by character
    for ch in TYPING_COMMAND:
        delay = random.uniform(CHAR_DELAY_MIN, CHAR_DELAY_MAX)
        ts += delay
        write_cast_event(f, ts, "o", ch)

    # Pause before pressing enter (human thinks: "looks right")
    ts += 0.35

    # "Press enter" — show a newline
    write_cast_event(f, ts, "o", "\r\n")

    # Small loading pause before TUI takes over
    ts += POST_ENTER_PAUSE

    return ts


async def run_demo(python_path: str):
    """Run the demo: record typing intro, then launch MacJet in a pty."""
    print(f"  Recording to {CAST_FILE}")
    print(f"  Terminal: {COLS}×{ROWS}")
    print(f"  Python: {python_path}")
    print()

    # Set our terminal size env
    env = os.environ.copy()
    env["TERM"] = "xterm-256color"
    env["COLORTERM"] = "truecolor"
    env["COLUMNS"] = str(COLS)
    env["LINES"] = str(ROWS)

    # Open cast file and write header
    cast_file = open(CAST_FILE, "w", encoding="utf-8")
    write_cast_header(cast_file, COLS, ROWS)

    # ─── Phase 1: Cinematic typing intro ─────────────
    print("  Phase 1: Recording typing intro...")
    intro_duration = record_typing_intro(cast_file)
    print(f"  Intro duration: {intro_duration:.1f}s")

    # ─── Phase 2: Launch MacJet in a pty ─────────────
    print("  Phase 2: Launching MacJet TUI...")

    # Create a pty pair
    master_fd, slave_fd = pty.openpty()

    # Set terminal size on the slave
    try:
        import fcntl
        import struct
        import termios

        winsize = struct.pack("HHHH", ROWS, COLS, 0, 0)
        fcntl.ioctl(slave_fd, termios.TIOCSWINSZ, winsize)
    except Exception as e:
        print(f"  Warning: couldn't set terminal size: {e}")

    # Launch MacJet
    proc = await asyncio.create_subprocess_exec(
        python_path,
        "-m",
        "macjet",
        stdin=slave_fd,
        stdout=slave_fd,
        stderr=slave_fd,
        env=env,
        cwd=str(REPO_ROOT),
    )
    os.close(slave_fd)

    wall_start = time.time()

    # Background reader task — captures TUI output with timestamps offset
    # by the intro duration so the timeline is seamless
    async def reader():
        loop = asyncio.get_event_loop()
        while True:
            try:
                data = await loop.run_in_executor(None, lambda: os.read(master_fd, 4096))
                if not data:
                    break
                ts = intro_duration + (time.time() - wall_start)
                try:
                    text = data.decode("utf-8", errors="replace")
                    write_cast_event(cast_file, ts, "o", text)
                    cast_file.flush()
                except Exception:
                    pass
            except OSError:
                break

    reader_task = asyncio.create_task(reader())

    # Execute demo script
    for delay, key in DEMO_SCRIPT:
        if delay > 0:
            await asyncio.sleep(delay)
        if key is not None:
            try:
                os.write(master_fd, key)
            except OSError:
                break

    # Wait for quit to propagate
    await asyncio.sleep(1.5)

    # Stop reader
    reader_task.cancel()
    try:
        await reader_task
    except asyncio.CancelledError:
        pass

    # Cleanup
    try:
        proc.terminate()
        await asyncio.wait_for(proc.wait(), timeout=3)
    except Exception:
        try:
            proc.kill()
        except Exception:
            pass

    try:
        os.close(master_fd)
    except OSError:
        pass

    cast_file.close()
    total = intro_duration + (time.time() - wall_start)
    print(f"  Recording complete: {CAST_FILE}")
    print(f"  Total duration: {total:.1f}s")
    print(f"  Cast size: {CAST_FILE.stat().st_size} bytes")


def convert_to_gif(agg_path: str) -> bool:
    """Convert the .cast file to an animated GIF using agg."""
    print("\n  Converting to GIF...")

    cmd = [
        agg_path,
        "--cols",
        str(COLS),
        "--rows",
        str(ROWS),
        "--font-family",
        "JetBrains Mono,SF Mono,Menlo,Consolas",
        "--font-size",
        "13",
        "--speed",
        "1.3",  # Slightly faster for reel feel
        "--idle-time-limit",
        "2",  # Cap idle pauses at 2s
        "--last-frame-duration",
        "3",  # Hold last frame 3s
        "--theme",
        "monokai",  # Rich colors matching flight deck palette
        str(CAST_FILE),
        str(GIF_FILE),
    ]

    print(f"  Running: {' '.join(cmd)}")
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode == 0:
        size_mb = GIF_FILE.stat().st_size / (1024 * 1024)
        print(f"  GIF created: {GIF_FILE} ({size_mb:.1f} MB)")
        return True
    else:
        print(f"  agg error: {result.stderr}")
        return False


def copy_to_assets() -> bool:
    """Copy the generated GIF to the assets directory."""
    try:
        ASSETS_GIF.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(GIF_FILE, ASSETS_GIF)
        size_mb = ASSETS_GIF.stat().st_size / (1024 * 1024)
        print(f"  Copied to: {ASSETS_GIF} ({size_mb:.1f} MB)")
        return True
    except Exception as e:
        print(f"  Error copying to assets: {e}")
        return False


if __name__ == "__main__":
    import platform

    if platform.system() != "Darwin":
        print("This script is macOS-only")
        sys.exit(1)

    print("=" * 60)
    print("  MacJet Demo Recorder")
    print("=" * 60)
    print()

    # Pre-flight checks
    python_path = find_python()
    agg_path = find_agg()
    print(f"  ✓ Python: {python_path}")
    print(f"  ✓ agg:    {agg_path}")
    print()

    # Record
    asyncio.run(run_demo(python_path))

    # Convert
    success = convert_to_gif(agg_path)

    if success:
        copy_to_assets()
        print()
        print("  ✅ Demo complete!")
        print(f"     Cast:   {CAST_FILE}")
        print(f"     GIF:    {GIF_FILE}")
        print(f"     Assets: {ASSETS_GIF}")
        print()
        print("  For README.md:")
        print("     ![MacJet Demo](assets/macjet_demo.gif)")
    else:
        print()
        print("  ❌ GIF conversion failed. Check cast file manually:")
        print(f"     agg --theme monokai {CAST_FILE} {GIF_FILE}")
