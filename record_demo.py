#!/usr/bin/env python3
"""
MacJet Demo Recorder
Drives the MacJet TUI via a pty, injects keypresses at timed intervals,
records via asciinema, and converts to GIF via agg.

Usage:
    sudo python3 record_demo.py

Output:
    macjet_demo.cast  — raw asciinema recording
    macjet_demo.gif   — final GIF for README
"""
from __future__ import annotations

import asyncio
import os
import pty
import sys
import time
import subprocess
import json
from pathlib import Path

# ─── Configuration ───────────────────────────────────
SCRIPT_DIR = Path(__file__).parent
CAST_FILE = SCRIPT_DIR / "macjet_demo.cast"
GIF_FILE = SCRIPT_DIR / "macjet_demo.gif"

# Terminal size for the recording — wide enough for dual-pane
COLS = 160
ROWS = 40

# ─── Key escape sequences ────────────────────────────
KEY_DOWN = b"\x1b[B"
KEY_UP   = b"\x1b[A"
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


def write_cast_header(f, cols: int, rows: int):
    """Write asciinema v2 header."""
    header = {
        "version": 2,
        "width": cols,
        "height": rows,
        "timestamp": int(time.time()),
        "env": {"TERM": "xterm-256color", "SHELL": "/bin/bash"},
        "title": "MacJet — Flight Deck Demo",
    }
    f.write(json.dumps(header) + "\n")


def write_cast_event(f, ts: float, event_type: str, data: str):
    """Write a single asciinema event."""
    event = [round(ts, 6), event_type, data]
    f.write(json.dumps(event) + "\n")


async def run_demo():
    """Run the demo: launch MacJet in a pty and inject keypresses."""
    print(f"Recording to {CAST_FILE}")
    print(f"Terminal: {COLS}×{ROWS}")
    print()

    # Set our terminal size env
    env = os.environ.copy()
    env["TERM"] = "xterm-256color"
    env["COLORTERM"] = "truecolor"
    env["COLUMNS"] = str(COLS)
    env["LINES"] = str(ROWS)

    # Find the venv python
    venv_python = Path.home() / ".macjet" / "venv" / "bin" / "python"
    if not venv_python.exists():
        venv_python = "python3"

    # Open cast file
    cast_file = open(CAST_FILE, "w", encoding="utf-8")
    write_cast_header(cast_file, COLS, ROWS)

    # Create a pty pair
    master_fd, slave_fd = pty.openpty()

    # Set terminal size on the slave
    try:
        import fcntl
        import termios
        import struct
        winsize = struct.pack("HHHH", ROWS, COLS, 0, 0)
        fcntl.ioctl(slave_fd, termios.TIOCSWINSZ, winsize)
    except Exception as e:
        print(f"Warning: couldn't set terminal size: {e}")

    # Launch MacJet
    proc = await asyncio.create_subprocess_exec(
        str(venv_python), "-m", "macjet",
        stdin=slave_fd,
        stdout=slave_fd,
        stderr=slave_fd,
        env=env,
        cwd=str(SCRIPT_DIR),
    )
    os.close(slave_fd)

    start_time = time.time()
    output_buffer = b""

    # Background reader task
    async def reader():
        nonlocal output_buffer
        loop = asyncio.get_event_loop()
        while True:
            try:
                data = await loop.run_in_executor(
                    None,
                    lambda: os.read(master_fd, 4096)
                )
                if not data:
                    break
                output_buffer += data
                ts = time.time() - start_time
                # Write to cast file
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
    duration = time.time() - start_time
    print(f"Recording complete: {CAST_FILE}")
    print(f"Duration: {duration:.1f}s")
    print(f"Size: {CAST_FILE.stat().st_size} bytes")


def convert_to_gif():
    """Convert the .cast file to an animated GIF using agg."""
    print(f"\nConverting to GIF: {GIF_FILE}")

    agg_path = subprocess.run(
        ["which", "agg"], capture_output=True, text=True
    ).stdout.strip() or "agg"

    cmd = [
        agg_path,
        "--cols", str(COLS),
        "--rows", str(ROWS),
        "--font-family", "JetBrains Mono,SF Mono,Menlo,Consolas",
        "--font-size", "13",
        "--speed", "1.3",          # Slightly faster playback for reel feel
        "--idle-time-limit", "2",  # Cap idle pauses at 2s
        "--theme", "github-dark",  # Clean dark theme matching our palette
        str(CAST_FILE),
        str(GIF_FILE),
    ]

    print(f"Running: {' '.join(cmd)}")
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode == 0:
        size_mb = GIF_FILE.stat().st_size / (1024 * 1024)
        print(f"GIF created: {GIF_FILE} ({size_mb:.1f} MB)")
        return True
    else:
        print(f"agg error: {result.stderr}")
        return False


if __name__ == "__main__":
    import platform
    if platform.system() != "Darwin":
        print("This script is macOS-only")
        sys.exit(1)

    print("=" * 60)
    print("  MacJet Demo Recorder")
    print("=" * 60)

    asyncio.run(run_demo())
    success = convert_to_gif()

    if success:
        print("\n✅ Demo complete!")
        print(f"   Cast: {CAST_FILE}")
        print(f"   GIF:  {GIF_FILE}")
        print(f"\nFor README.md:")
        print(f"   ![MacJet Demo](macjet_demo.gif)")
    else:
        print("\n❌ GIF conversion failed. Check cast file manually:")
        print(f"   agg --theme dracula {CAST_FILE} {GIF_FILE}")
