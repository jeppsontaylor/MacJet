"""
MacJet — Browser Inspector
Queries Chrome/Safari/Brave/Arc for open tabs via AppleScript.
Optional Chromium DevTools Protocol support for precision mode.
"""
from __future__ import annotations

import asyncio
import json
from dataclasses import dataclass, field
from typing import Optional


@dataclass
class TabInfo:
    title: str = ""
    url: str = ""
    is_active: bool = False
    window_index: int = 0


@dataclass
class BrowserContext:
    app_name: str = ""
    tabs: list[TabInfo] = field(default_factory=list)
    active_tab: Optional[TabInfo] = None
    window_count: int = 0
    tab_count: int = 0
    confidence: str = "app-exact"


# AppleScript templates for each browser
_CHROME_SCRIPT = """
tell application "Google Chrome"
    set tabData to ""
    set windowCount to count of windows
    repeat with w from 1 to windowCount
        set tabCount to count of tabs of window w
        repeat with t from 1 to tabCount
            set tabTitle to title of tab t of window w
            set tabURL to URL of tab t of window w
            set isActive to (active tab index of window w is t)
            set tabData to tabData & w & "\\t" & tabTitle & "\\t" & tabURL & "\\t" & isActive & "\\n"
        end repeat
    end repeat
    return tabData
end tell
"""

_BRAVE_SCRIPT = _CHROME_SCRIPT.replace("Google Chrome", "Brave Browser")

_ARC_SCRIPT = """
tell application "Arc"
    set tabData to ""
    set windowCount to count of windows
    repeat with w from 1 to windowCount
        set tabCount to count of tabs of window w
        repeat with t from 1 to tabCount
            set tabTitle to title of tab t of window w
            set tabURL to URL of tab t of window w
            set tabData to tabData & w & "\\t" & tabTitle & "\\t" & tabURL & "\\tfalse\\n"
        end repeat
    end repeat
    return tabData
end tell
"""

_SAFARI_SCRIPT = """
tell application "Safari"
    set tabData to ""
    set windowCount to count of windows
    repeat with w from 1 to windowCount
        set tabCount to count of tabs of window w
        repeat with t from 1 to tabCount
            set tabTitle to name of tab t of window w
            set tabURL to URL of tab t of window w
            set isActive to (current tab of window w is tab t of window w)
            set tabData to tabData & w & "\\t" & tabTitle & "\\t" & tabURL & "\\t" & isActive & "\\n"
        end repeat
    end repeat
    return tabData
end tell
"""

_BROWSER_SCRIPTS: dict[str, str] = {
    "Google Chrome": _CHROME_SCRIPT,
    "Brave Browser": _BRAVE_SCRIPT,
    "Arc": _ARC_SCRIPT,
    "Safari": _SAFARI_SCRIPT,
}


class BrowserInspector:
    """Inspects browser tabs via AppleScript with optional CDP support."""

    def __init__(self, cdp_port: int = 9222):
        self._cdp_port = cdp_port
        self._cache: dict[str, BrowserContext] = {}
        self._cache_age: dict[str, float] = {}

    async def inspect(self, app_name: str) -> Optional[BrowserContext]:
        """Get browser context for a running browser app."""
        # Normalize app name
        canonical = None
        for browser in _BROWSER_SCRIPTS:
            if browser.lower().split()[0] in app_name.lower():
                canonical = browser
                break

        if not canonical:
            return None

        # Try CDP first for Chromium browsers
        if canonical != "Safari":
            ctx = await self._try_cdp()
            if ctx:
                return ctx

        # Fall back to AppleScript
        return await self._query_applescript(canonical)

    async def _query_applescript(self, browser: str) -> Optional[BrowserContext]:
        """Query browser tabs via AppleScript."""
        script = _BROWSER_SCRIPTS.get(browser)
        if not script:
            return None

        try:
            proc = await asyncio.create_subprocess_exec(
                "osascript", "-e", script,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.DEVNULL,
            )
            stdout, _ = await asyncio.wait_for(proc.communicate(), timeout=3.0)
        except (asyncio.TimeoutError, OSError):
            return None

        if proc.returncode != 0 or not stdout:
            return None

        tabs = []
        active_tab = None
        windows = set()

        for line in stdout.decode("utf-8", errors="replace").strip().split("\n"):
            parts = line.split("\t")
            if len(parts) < 4:
                continue

            window_idx = int(parts[0]) if parts[0].isdigit() else 0
            windows.add(window_idx)
            tab = TabInfo(
                title=parts[1],
                url=parts[2],
                is_active=parts[3].strip().lower() == "true",
                window_index=window_idx,
            )
            tabs.append(tab)
            if tab.is_active and not active_tab:
                active_tab = tab

        ctx = BrowserContext(
            app_name=browser,
            tabs=tabs,
            active_tab=active_tab,
            window_count=len(windows),
            tab_count=len(tabs),
            confidence="app-exact",
        )
        self._cache[browser] = ctx
        return ctx

    async def _try_cdp(self) -> Optional[BrowserContext]:
        """Try Chromium DevTools Protocol for precise tab info."""
        try:
            proc = await asyncio.create_subprocess_exec(
                "curl", "-s", "--connect-timeout", "1",
                f"http://localhost:{self._cdp_port}/json",
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.DEVNULL,
            )
            stdout, _ = await asyncio.wait_for(proc.communicate(), timeout=2.0)
        except (asyncio.TimeoutError, OSError):
            return None

        if proc.returncode != 0 or not stdout:
            return None

        try:
            targets = json.loads(stdout)
        except json.JSONDecodeError:
            return None

        tabs = []
        for target in targets:
            if target.get("type") != "page":
                continue
            tabs.append(TabInfo(
                title=target.get("title", ""),
                url=target.get("url", ""),
                is_active=False,  # CDP doesn't indicate active tab easily
            ))

        if not tabs:
            return None

        return BrowserContext(
            app_name="Chrome (CDP)",
            tabs=tabs,
            tab_count=len(tabs),
            window_count=1,
            confidence="app-exact",
        )

    def get_cached(self, app_name: str) -> Optional[BrowserContext]:
        """Get cached browser context without re-querying."""
        for browser, ctx in self._cache.items():
            if browser.lower().split()[0] in app_name.lower():
                return ctx
        return None
