"""
MacJet — Drill-Down Screens
Full-screen views for sample, fs_usage, nettop, sc_usage.
"""

from __future__ import annotations

import asyncio

from textual.app import ComposeResult
from textual.containers import Vertical
from textual.screen import ModalScreen
from textual.widgets import Static


class DrillDownScreen(ModalScreen):
    """Base screen for drill-down tools."""

    BINDINGS = [
        ("escape", "dismiss", "Close"),
        ("q", "dismiss", "Close"),
    ]

    DEFAULT_CSS = """
    DrillDownScreen {
        align: center middle;
    }
    DrillDownScreen #drill-container {
        width: 90%;
        height: 90%;
        background: #0d1117;
        border: solid #30363d;
        padding: 1;
    }
    DrillDownScreen #drill-title {
        height: 1;
        text-style: bold;
        color: #58a6ff;
        background: #161b22;
        padding: 0 1;
    }
    DrillDownScreen #drill-output {
        height: 1fr;
        color: #e6edf3;
        background: #0d1117;
    }
    DrillDownScreen #drill-status {
        height: 1;
        color: #8b949e;
        background: #161b22;
        padding: 0 1;
    }
    """

    def __init__(self, title: str, command: list[str], requires_sudo: bool = False):
        super().__init__()
        self._title = title
        self._command = command
        self._requires_sudo = requires_sudo
        self._process: asyncio.subprocess.Process | None = None

    def compose(self) -> ComposeResult:
        with Vertical(id="drill-container"):
            yield Static(f"  {self._title}  (ESC to close)", id="drill-title")
            yield Static("  Loading...", id="drill-output")
            yield Static("  Running command...", id="drill-status")

    async def on_mount(self):
        self.run_worker(self._run_command())

    async def _run_command(self):
        """Execute the drill-down command and stream output."""
        output_widget = self.query_one("#drill-output", Static)
        status_widget = self.query_one("#drill-status", Static)

        try:
            self._process = await asyncio.create_subprocess_exec(
                *self._command,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )

            lines = []
            while True:
                try:
                    line = await asyncio.wait_for(self._process.stdout.readline(), timeout=10.0)
                    if not line:
                        break
                    text = line.decode("utf-8", errors="replace").rstrip()
                    lines.append(text)
                    # Keep last 40 lines
                    if len(lines) > 40:
                        lines = lines[-40:]
                    output_widget.update("\n".join(lines))
                except asyncio.TimeoutError:
                    break

            returncode = await self._process.wait()
            status_widget.update(f"  Command finished (exit code: {returncode})")

        except FileNotFoundError:
            output_widget.update(f"  ❌ Command not found: {self._command[0]}")
            status_widget.update("  Command failed")
        except Exception as e:
            output_widget.update(f"  ❌ Error: {e}")
            status_widget.update("  Error occurred")

    def action_dismiss(self):
        """Clean up and dismiss."""
        if self._process:
            try:
                self._process.terminate()
            except ProcessLookupError:
                pass
        self.app.pop_screen()


class SampleScreen(DrillDownScreen):
    """Profile a process using macOS `sample` command."""

    def __init__(self, pid: int, duration: int = 3):
        super().__init__(
            title=f"CPU Profile (sample) — PID {pid}",
            command=["sample", str(pid), str(duration)],
        )


class FsUsageScreen(DrillDownScreen):
    """Live file I/O trace using `fs_usage`."""

    def __init__(self, pid: int):
        super().__init__(
            title=f"File I/O Trace (fs_usage) — PID {pid}",
            command=["sudo", "fs_usage", "-f", "filesys", str(pid)],
            requires_sudo=True,
        )


class NettopScreen(DrillDownScreen):
    """Network inspector using `nettop`."""

    def __init__(self, pid: int | None = None):
        cmd = ["nettop", "-P", "-l", "1", "-n"]
        if pid:
            cmd.extend(["-p", str(pid)])
        super().__init__(
            title=f"Network Inspector (nettop){' — PID ' + str(pid) if pid else ''}",
            command=cmd,
        )


class ScUsageScreen(DrillDownScreen):
    """Syscall statistics using `sc_usage`."""

    def __init__(self, pid: int):
        super().__init__(
            title=f"Syscall Stats (sc_usage) — PID {pid}",
            command=["sudo", "sc_usage", str(pid)],
            requires_sudo=True,
        )
