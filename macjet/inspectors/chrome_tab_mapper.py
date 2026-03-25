"""
MacJet — Chrome Tab Mapper
Maps Chrome renderer PIDs to exact tab titles using CDP.

Strategy:
1. `/json` — instant tab list with titles, URLs, and individual WS endpoints
2. `SystemInfo.getProcessInfo` — renderer PIDs with cumulative CPU time
3. Per-tab WS `Performance.getMetrics` — JS heap per tab (fast 1s timeout)
4. Heuristic PID correlation via creation order when exact mapping unavailable
"""
from __future__ import annotations

import asyncio
import json
import urllib.request
from dataclasses import dataclass, field
from typing import Optional


@dataclass
class ChromeTab:
    """A Chrome tab with resource usage info."""
    target_id: str = ""
    title: str = ""
    url: str = ""
    favicon_url: str = ""
    tab_type: str = "page"  # page, background_page, service_worker, etc.
    ws_url: str = ""
    # Resource metrics (filled in asynchronously)
    js_heap_mb: float = 0.0
    task_duration: float = 0.0
    dom_nodes: int = 0
    # PID mapping (best-effort)
    renderer_pid: int = 0
    cpu_time_s: float = 0.0


@dataclass
class ChromeSnapshot:
    """Full Chrome state snapshot."""
    tabs: list[ChromeTab] = field(default_factory=list)
    renderer_pids: dict[int, float] = field(default_factory=dict)  # pid → cpuTime
    total_tabs: int = 0
    total_js_heap_mb: float = 0.0
    has_cdp: bool = False
    cdp_port: int = 0
    error: str = ""


class ChromeTabMapper:
    """Maps Chrome tabs to renderer PIDs via Chrome DevTools Protocol."""

    def __init__(self, cdp_port: int = 9222):
        self._port = cdp_port
        self._latest = ChromeSnapshot()
        self._ws_available = False

    @property
    def latest(self) -> ChromeSnapshot:
        return self._latest

    async def collect(self) -> ChromeSnapshot:
        """Collect Chrome tab data with PID mapping."""
        snapshot = ChromeSnapshot(cdp_port=self._port)

        # Step 1: Get tab list from /json (instant, no WS needed)
        tabs = await self._get_json_tabs()
        if tabs is None:
            snapshot.error = "CDP not available"
            self._latest = snapshot
            return snapshot

        snapshot.has_cdp = True
        snapshot.tabs = tabs
        snapshot.total_tabs = len(tabs)

        # Step 2: Get renderer PIDs from SystemInfo via browser WS
        renderer_pids = await self._get_renderer_pids()
        snapshot.renderer_pids = renderer_pids

        # Step 3: Try per-tab metrics (best-effort, may fail on some tabs)
        await self._enrich_tab_metrics(tabs)

        # Step 4: Correlate PIDs to tabs
        self._correlate_pids(tabs, renderer_pids)

        # Compute totals
        snapshot.total_js_heap_mb = sum(t.js_heap_mb for t in tabs)

        # Sort: by JS heap if we have it, otherwise by cpu_time from PID correlation
        has_heap = any(t.js_heap_mb > 0 for t in tabs)
        if has_heap:
            tabs.sort(key=lambda t: t.js_heap_mb, reverse=True)
        else:
            tabs.sort(key=lambda t: t.cpu_time_s, reverse=True)

        self._latest = snapshot
        return snapshot

    async def _get_json_tabs(self) -> Optional[list[ChromeTab]]:
        """Get tab list from Chrome's /json endpoint."""
        loop = asyncio.get_event_loop()
        try:
            raw = await loop.run_in_executor(None, self._fetch_json, f"http://localhost:{self._port}/json")
            if raw is None:
                return None

            tabs = []
            for entry in raw:
                if entry.get("type") not in ("page", "background_page"):
                    continue
                tabs.append(ChromeTab(
                    target_id=entry.get("id", ""),
                    title=entry.get("title", ""),
                    url=entry.get("url", ""),
                    favicon_url=entry.get("faviconUrl", ""),
                    tab_type=entry.get("type", "page"),
                    ws_url=entry.get("webSocketDebuggerUrl", ""),
                ))
            return tabs
        except Exception:
            return None

    async def _get_renderer_pids(self) -> dict[int, float]:
        """Get renderer PID → cpuTime mapping from SystemInfo.getProcessInfo."""
        try:
            loop = asyncio.get_event_loop()
            version = await loop.run_in_executor(
                None, self._fetch_json, f"http://localhost:{self._port}/json/version"
            )
            if not version:
                return {}

            browser_ws = version.get("webSocketDebuggerUrl", "")
            if not browser_ws:
                return {}

            import websockets
            async with websockets.connect(browser_ws, close_timeout=2, max_size=10*1024*1024) as ws:
                await ws.send(json.dumps({"id": 1, "method": "SystemInfo.getProcessInfo"}))
                resp = json.loads(await asyncio.wait_for(ws.recv(), timeout=5))

                result = {}
                for p in resp.get("result", {}).get("processInfo", []):
                    if p.get("type") == "renderer":
                        result[p["id"]] = p.get("cpuTime", 0)
                return result

        except ImportError:
            return {}
        except Exception:
            return {}

    async def _enrich_tab_metrics(self, tabs: list[ChromeTab]):
        """Get JS heap and DOM node count for each tab via its individual WS endpoint."""
        try:
            import websockets
        except ImportError:
            return

        sem = asyncio.Semaphore(3)  # Max 3 concurrent WS connections

        async def get_metrics(tab: ChromeTab):
            if not tab.ws_url:
                return
            async with sem:
                try:
                    async with websockets.connect(
                        tab.ws_url, close_timeout=1, open_timeout=1.5, max_size=5*1024*1024
                    ) as ws:
                        await ws.send(json.dumps({
                            "id": 1, "method": "Performance.getMetrics"
                        }))
                        resp = json.loads(await asyncio.wait_for(ws.recv(), timeout=1.5))

                        for m in resp.get("result", {}).get("metrics", []):
                            name = m["name"]
                            val = m["value"]
                            if name == "JSHeapUsedSize":
                                tab.js_heap_mb = val / (1024 * 1024)
                            elif name == "TaskDuration":
                                tab.task_duration = val
                            elif name == "Nodes":
                                tab.dom_nodes = int(val)
                except Exception:
                    pass

        await asyncio.gather(*[get_metrics(t) for t in tabs], return_exceptions=True)

    def _correlate_pids(self, tabs: list[ChromeTab], renderer_pids: dict[int, float]):
        """Correlate renderer PIDs to tabs using best-effort heuristics.
        
        Strategy: 
        1. If we have task_duration from per-tab metrics, sort and pair by workload.
        2. If not, assign PIDs to tabs by index (tab order ≈ renderer creation order).
        Not perfect, but gives reasonable attribution for heaviest consumers.
        """
        if not renderer_pids or not tabs:
            return

        # Sort PIDs by cpuTime (heaviest first)
        sorted_pids = sorted(
            renderer_pids.items(),
            key=lambda x: x[1],
            reverse=True,
        )

        # Check if we have per-tab metrics
        tabs_with_duration = [t for t in tabs if t.task_duration > 0]
        
        if tabs_with_duration:
            # Sort tabs by task_duration (heaviest first) and pair
            tabs_with_duration.sort(key=lambda t: t.task_duration, reverse=True)
            for i, tab in enumerate(tabs_with_duration):
                if i < len(sorted_pids):
                    pid, cpu_time = sorted_pids[i]
                    tab.renderer_pid = pid
                    tab.cpu_time_s = cpu_time
        else:
            # Fallback: assign PIDs to tabs by index
            # This is a rough heuristic but at least shows which PIDs are hot
            for i, tab in enumerate(tabs):
                if i < len(sorted_pids):
                    pid, cpu_time = sorted_pids[i]
                    tab.renderer_pid = pid
                    tab.cpu_time_s = cpu_time

    @staticmethod
    def _fetch_json(url: str):
        """Fetch JSON from a URL synchronously."""
        try:
            req = urllib.request.Request(url, headers={"Accept": "application/json"})
            with urllib.request.urlopen(req, timeout=2) as resp:
                return json.loads(resp.read())
        except Exception:
            return None

    def get_tab_for_pid(self, pid: int) -> Optional[ChromeTab]:
        """Get the tab associated with a renderer PID."""
        for tab in self._latest.tabs:
            if tab.renderer_pid == pid:
                return tab
        return None

    def get_domain(self, url: str) -> str:
        """Extract domain from a URL."""
        try:
            from urllib.parse import urlparse
            parsed = urlparse(url)
            return parsed.netloc or parsed.path[:30]
        except Exception:
            return url[:30]

    def format_tab_label(self, tab: ChromeTab, max_len: int = 40) -> str:
        """Format a tab into a readable label for the process tree."""
        title = tab.title or self.get_domain(tab.url) or "untitled"
        if len(title) > max_len:
            title = title[:max_len - 1] + "…"

        parts = [title]
        if tab.js_heap_mb > 10:
            parts.append(f"[{tab.js_heap_mb:.0f}MB JS]")
        if tab.dom_nodes > 1000:
            parts.append(f"[{tab.dom_nodes} nodes]")

        return " ".join(parts)


async def auto_detect_cdp_port() -> int:
    """Auto-detect Chrome's CDP port from its process arguments."""
    import psutil
    for proc in psutil.process_iter(["name", "cmdline"]):
        try:
            name = proc.info.get("name", "")
            if "Google Chrome" not in name and "chrome" not in name.lower():
                continue
            cmdline = proc.info.get("cmdline", []) or []
            for arg in cmdline:
                if arg.startswith("--remote-debugging-port="):
                    port = int(arg.split("=")[1])
                    return port
        except (psutil.NoSuchProcess, psutil.AccessDenied, ValueError):
            continue
    return 0
