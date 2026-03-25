"""
MacJet — Terminal Inspector
Detects foreground processes in Terminal.app, iTerm2, Ghostty, Kitty, etc.
"""
from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Optional


@dataclass
class TerminalContext:
    app_name: str = ""
    tab_title: str = ""
    foreground_process: str = ""
    window_title: str = ""
    confidence: str = "window-exact"


class TerminalInspector:
    """Inspects terminal emulators for tab titles and foreground processes."""

    _TERMINAL_APPS = {
        "Terminal", "iTerm2", "iTerm", "Ghostty", "kitty",
        "Alacritty", "WezTerm", "Hyper",
    }

    def is_terminal(self, app_name: str) -> bool:
        return any(t.lower() in app_name.lower() for t in self._TERMINAL_APPS)

    async def inspect(self, app_name: str) -> Optional[TerminalContext]:
        """Get terminal window/tab title."""
        if not self.is_terminal(app_name):
            return None

        title = await self._get_window_title(app_name)
        return TerminalContext(
            app_name=app_name,
            tab_title=title,
            window_title=title,
            confidence="window-exact" if title else "grouped",
        )

    async def _get_window_title(self, app_name: str) -> str:
        # Normalize name for System Events
        se_name = app_name
        if "iterm" in app_name.lower():
            se_name = "iTerm2"
        elif "terminal" in app_name.lower():
            se_name = "Terminal"

        script = f'''
tell application "System Events"
    tell process "{se_name}"
        try
            return name of front window
        on error
            return ""
        end try
    end tell
end tell
'''
        try:
            proc = await asyncio.create_subprocess_exec(
                "osascript", "-e", script,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.DEVNULL,
            )
            stdout, _ = await asyncio.wait_for(proc.communicate(), timeout=2.0)
            if proc.returncode == 0 and stdout:
                return stdout.decode("utf-8", errors="replace").strip()
        except (asyncio.TimeoutError, OSError):
            pass
        return ""
