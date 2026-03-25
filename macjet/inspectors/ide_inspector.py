"""
MacJet — IDE Inspector
Detects projects/workspaces for VSCode, Cursor, Xcode, JetBrains IDEs.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

import psutil


@dataclass
class IDEContext:
    ide_name: str = ""
    project_path: str = ""
    project_name: str = ""
    active_file: str = ""
    window_title: str = ""
    confidence: str = "exact"


class IDEInspector:
    """Inspects IDE processes for project context."""

    # Map of process name patterns → IDE name
    _IDE_PATTERNS = {
        "Cursor": "Cursor",
        "Code": "VSCode",
        "code": "VSCode",
        "Code Helper": "VSCode",
        "Cursor Helper": "Cursor",
        "Xcode": "Xcode",
        "idea": "IntelliJ IDEA",
        "pycharm": "PyCharm",
        "webstorm": "WebStorm",
        "goland": "GoLand",
        "clion": "CLion",
        "rider": "Rider",
        "rubymine": "RubyMine",
        "phpstorm": "PhpStorm",
    }

    async def inspect(
        self, process_name: str, cmdline: list[str], pid: int
    ) -> Optional[IDEContext]:
        """Extract IDE project context from process info."""
        ide_name = self._match_ide(process_name)
        if not ide_name:
            return None

        if ide_name in ("VSCode", "Cursor"):
            return await self._inspect_vscode(ide_name, cmdline, pid)
        elif ide_name == "Xcode":
            return await self._inspect_xcode(pid)
        else:
            return await self._inspect_jetbrains(ide_name, cmdline, pid)

    def _match_ide(self, process_name: str) -> Optional[str]:
        for pattern, name in self._IDE_PATTERNS.items():
            if pattern.lower() in process_name.lower():
                return name
        return None

    async def _inspect_vscode(self, ide_name: str, cmdline: list[str], pid: int) -> IDEContext:
        """Extract VSCode/Cursor project from --folder-uri or cwd."""
        ctx = IDEContext(ide_name=ide_name)

        for arg in cmdline:
            if "--folder-uri=" in arg:
                uri = arg.split("=", 1)[1]
                if uri.startswith("file://"):
                    path = uri[7:]
                    ctx.project_path = path
                    ctx.project_name = Path(path).name
                    ctx.confidence = "exact"
                    return ctx

        # Try cwd as fallback
        try:
            proc = psutil.Process(pid)
            cwd = proc.cwd()
            if cwd and cwd != "/":
                ctx.project_path = cwd
                ctx.project_name = Path(cwd).name
                ctx.confidence = "inferred"
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            pass

        # Try window title via AppleScript
        title = await self._get_window_title(ide_name)
        if title:
            ctx.window_title = title
            # Window title often contains "filename — project_name"
            if " — " in title:
                parts = title.split(" — ")
                ctx.active_file = parts[0].strip()
                if len(parts) > 1 and not ctx.project_name:
                    ctx.project_name = parts[-1].strip()
                    ctx.confidence = "window-exact"

        return ctx

    async def _inspect_xcode(self, pid: int) -> IDEContext:
        """Extract Xcode project from window title."""
        ctx = IDEContext(ide_name="Xcode")

        title = await self._get_window_title("Xcode")
        if title:
            ctx.window_title = title
            # Xcode title: "ProjectName — FileName.swift"
            if " — " in title:
                parts = title.split(" — ")
                ctx.project_name = parts[0].strip()
                if len(parts) > 1:
                    ctx.active_file = parts[1].strip()
            else:
                ctx.project_name = title.strip()
            ctx.confidence = "window-exact"

        return ctx

    async def _inspect_jetbrains(self, ide_name: str, cmdline: list[str], pid: int) -> IDEContext:
        """Extract JetBrains IDE project from cmdline args."""
        ctx = IDEContext(ide_name=ide_name)

        # JetBrains IDEs often have the project path as the last arg
        for arg in reversed(cmdline):
            p = Path(arg)
            if p.exists() and p.is_dir():
                ctx.project_path = str(p)
                ctx.project_name = p.name
                ctx.confidence = "exact"
                return ctx

        return ctx

    async def _get_window_title(self, app_name: str) -> str:
        """Get the frontmost window title for an app via osascript."""
        script = f"""
tell application "System Events"
    tell process "{app_name}"
        try
            return name of front window
        on error
            return ""
        end try
    end tell
end tell
"""
        try:
            proc = await asyncio.create_subprocess_exec(
                "osascript",
                "-e",
                script,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.DEVNULL,
            )
            stdout, _ = await asyncio.wait_for(proc.communicate(), timeout=2.0)
            if proc.returncode == 0 and stdout:
                return stdout.decode("utf-8", errors="replace").strip()
        except (asyncio.TimeoutError, OSError):
            pass
        return ""
