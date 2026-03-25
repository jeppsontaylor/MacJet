"""
MacJet V4 — Flight Deck Application

The ultimate macOS developer dashboard. Rebuilt with an adaptive
flight deck layout, app-first views, and intelligent Kill List.
"""
from __future__ import annotations

import asyncio
import os
import signal
import sys
import time
from pathlib import Path

from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.widgets import Static, Footer, Input, DataTable, ContentSwitcher

# Local imports
from .collectors.process_collector import ProcessCollector, ProcessGroup, get_system_stats
from .collectors.energy_collector import EnergyCollector
from .collectors.network_collector import NetworkCollector, format_bytes_per_s
from .collectors.metrics_history import MetricsHistory
from .inspectors.browser_inspector import BrowserInspector
from .inspectors.ide_inspector import IDEInspector
from .inspectors.container_inspector import ContainerInspector
from .inspectors.terminal_inspector import TerminalInspector
from .inspectors.generic_inspector import GenericInspector
from .inspectors.chrome_tab_mapper import ChromeTabMapper, auto_detect_cdp_port
from .ui.header import SystemHeader
from .ui.process_tree import ProcessTree
from .ui.detail_panel import DetailPanel
from .ui.reclaim_panel import ReclaimPanel
from .ui.drill_screens import SampleScreen, FsUsageScreen, NettopScreen, ScUsageScreen


CSS_PATH = Path(__file__).parent / "ui" / "theme.tcss"

# ─── View mode constants ─────────────────────────────
VIEW_APPS = "apps"
VIEW_TREE = "tree"
VIEW_PRESSURE = "pressure"
VIEW_ENERGY = "energy"
VIEW_RECLAIM = "reclaim"

VIEW_NAMES = {
    VIEW_APPS: "Apps",
    VIEW_TREE: "Tree",
    VIEW_PRESSURE: "Pressure",
    VIEW_ENERGY: "Energy",
    VIEW_RECLAIM: "Reclaim",
}

VIEW_KEYS = [VIEW_APPS, VIEW_TREE, VIEW_PRESSURE, VIEW_ENERGY, VIEW_RECLAIM]


class MacJetApp(App):
    """The MacJet TUI Application — Flight Deck Edition."""

    TITLE = "MacJet"
    SUB_TITLE = "Mac Flight Deck"

    CSS_PATH = str(CSS_PATH)

    BINDINGS = [
        Binding("q", "quit", "Quit", show=True, priority=True),
        Binding("space", "toggle_pause", "Pause", show=True),
        Binding("s", "cycle_sort", "Sort", show=True),
        Binding("t", "toggle_tree", "Expand", show=True),
        Binding("k", "kill_process", "Kill", show=True),
        Binding("shift+k", "force_kill", "Force Kill", show=False),
        Binding("z", "suspend_resume", "Suspend", show=False),
        Binding("p", "profile", "Profile", show=True),
        Binding("f", "file_io", "File I/O", show=False),
        Binding("n", "network", "Network", show=False),
        Binding("y", "syscalls", "Syscalls", show=False),
        Binding("w", "show_context", "Context", show=False),
        Binding("slash", "filter", "Filter", show=True),
        Binding("escape", "clear_filter", "Clear", show=False),
        Binding("question_mark", "help", "Help", show=True),
        # View switching
        Binding("1", "view_apps", "Apps", show=False),
        Binding("2", "view_tree", "Tree", show=False),
        Binding("3", "view_pressure", "Pressure", show=False),
        Binding("4", "view_energy", "Energy", show=False),
        Binding("5", "view_reclaim", "Reclaim", show=False),
        Binding("tab", "cycle_view", "Views", show=True),
        Binding("h", "toggle_system", "Hide Sys", show=False),
    ]

    def __init__(self):
        super().__init__()

        # Collectors
        self._proc_collector = ProcessCollector()
        self._energy_collector = EnergyCollector()
        self._net_collector = NetworkCollector()

        # Inspectors
        self._browser_inspector = BrowserInspector()
        self._ide_inspector = IDEInspector()
        self._container_inspector = ContainerInspector()
        self._terminal_inspector = TerminalInspector()
        self._generic_inspector = GenericInspector()
        self._chrome_mapper = ChromeTabMapper()

        # State
        self._groups: dict[str, ProcessGroup] = {}
        self._filter_visible = False
        self._self_pid = os.getpid()
        self._last_update = 0.0
        self._context_cache: dict[str, dict] = {}
        self._paused = False
        self._current_view = VIEW_APPS
        self._hide_system = False

    def compose(self) -> ComposeResult:
        # System header (2-line branded strip)
        yield SystemHeader(id="system-header")

        # View tabs bar
        yield Static(self._build_tab_bar(), id="view-tabs")

        # Filter input (hidden by default)
        yield Input(placeholder="Filter processes...", id="filter-input")

        # Main content: process list (left) + inspector (right)
        with Horizontal(id="middle-section", classes="horizontal-split"):
            # Left pane: content switcher for different views
            with ContentSwitcher(id="view-switcher", initial=VIEW_APPS):
                yield ProcessTree(id=VIEW_APPS)
                yield ProcessTree(id=VIEW_TREE)
                yield ProcessTree(id=VIEW_PRESSURE)
                yield ProcessTree(id=VIEW_ENERGY)
                yield ReclaimPanel(id=VIEW_RECLAIM)

            # Right pane: inspector
            yield DetailPanel(id="detail-panel")

        # Footer
        yield Footer()

    def _build_tab_bar(self) -> str:
        """Build the view tab bar with active indicator."""
        parts = ["  "]
        for key in VIEW_KEYS:
            label = VIEW_NAMES[key]
            idx = VIEW_KEYS.index(key) + 1
            if key == self._current_view:
                parts.append(f"[bold #60A5FA] {idx}:{label} [/]")
            else:
                parts.append(f"[#7F8DB3] {idx}:{label} [/]")
            parts.append("[#1A2540]│[/]")
        parts.append(f"  [#7F8DB3]Tab:cycle  /:filter  ?:help[/]")
        return "".join(parts)

    def _switch_view(self, view: str):
        """Switch to a different view."""
        self._current_view = view
        try:
            switcher = self.query_one("#view-switcher", ContentSwitcher)
            switcher.current = view
            tabs = self.query_one("#view-tabs", Static)
            tabs.update(self._build_tab_bar())
        except Exception:
            pass

        # Set grouping mode based on view
        if view == VIEW_APPS:
            self._proc_collector.grouping_mode = "app"
        elif view == VIEW_TREE:
            self._proc_collector.grouping_mode = "tree"
        elif view in (VIEW_PRESSURE, VIEW_ENERGY):
            self._proc_collector.grouping_mode = "app"

        # Focus the right widget
        try:
            if view == VIEW_RECLAIM:
                reclaim = self.query_one(f"#{VIEW_RECLAIM}", ReclaimPanel)
                table = reclaim.query_one(DataTable)
                table.focus()
            else:
                ptree = self.query_one(f"#{view}", ProcessTree)
                table = ptree.query_one(DataTable)
                table.focus()
        except Exception:
            pass

    async def on_mount(self):
        """Start all data collection workers."""
        # Start energy collector (if sudo)
        await self._energy_collector.start()

        # Focus the process table
        try:
            ptree = self.query_one(f"#{VIEW_APPS}", ProcessTree)
            table = ptree.query_one(DataTable)
            table.focus()
        except Exception:
            pass

        # Show sudo warning if needed
        if not self._energy_collector.has_sudo:
            try:
                detail = self.query_one("#detail-panel", DetailPanel)
                detail.show_message(
                    "⚠ Limited Mode",
                    "Run with sudo for\n"
                    "energy/thermal data:\n\n"
                    "  sudo ./macjet.sh\n\n"
                    "Basic CPU/memory\n"
                    "monitoring is active."
                )
            except Exception:
                pass

        # Prime CPU percent
        import psutil
        psutil.cpu_percent(interval=None)

        # Start collection lanes
        self.set_interval(1.0, self._fast_lane_tick)
        self.set_interval(3.0, self._context_lane_tick)
        self.set_interval(2.0, self._net_lane_tick)

        # Auto-detect Chrome CDP port
        self.run_worker(self._detect_cdp())

    async def _detect_cdp(self):
        """Auto-detect Chrome's CDP port."""
        port = await auto_detect_cdp_port()
        if port:
            self._chrome_mapper = ChromeTabMapper(cdp_port=port)
            self.notify(f"Chrome CDP on port {port}", timeout=2)

    async def _fast_lane_tick(self):
        """Fast lane: psutil process data + system stats (every 1s)."""
        if self._paused:
            return
        procs, groups = await self._proc_collector.collect()

        # Filter system processes if hidden
        if self._hide_system:
            groups = {
                k: g for k, g in groups.items()
                if not all(p.is_system for p in g.processes)
            }

        self._groups = groups

        # Enrich with energy data
        if self._energy_collector.has_sudo:
            energy = self._energy_collector.latest
            for key, group in groups.items():
                for p in group.processes:
                    einfo = energy.processes.get(p.pid)
                    if einfo:
                        if einfo.energy_impact > 50:
                            p.energy_impact = "HIGH"
                        elif einfo.energy_impact > 20:
                            p.energy_impact = "MED"
                        elif einfo.energy_impact > 5:
                            p.energy_impact = "LOW"

        # Enrich Chrome renderer processes with tab titles
        if self._chrome_mapper.latest.has_cdp:
            for key, group in groups.items():
                for p in group.processes:
                    if "renderer" in p.name.lower() or "Helper" in p.name:
                        tab = self._chrome_mapper.get_tab_for_pid(p.pid)
                        if tab:
                            label = self._chrome_mapper.format_tab_label(tab, max_len=32)
                            p.context_label = f"🌐 {label}"
                            p.confidence = "exact"

        # Update the active process view
        if self._current_view != VIEW_RECLAIM:
            try:
                ptree = self.query_one(f"#{self._current_view}", ProcessTree)
                ptree.update_data(groups, self._proc_collector.metrics_history)
                ptree.update_toolbar(
                    self._proc_collector.sort_key,
                    self._current_view,
                    self._proc_collector.filter_text,
                )
            except Exception:
                pass
        else:
            # Update Reclaim view
            self._update_reclaim()

        # Update system stats
        stats = get_system_stats()
        header = self.query_one("#system-header", SystemHeader)
        header.cpu_percent = stats["cpu_percent"]
        header.mem_used = stats["mem_used_gb"]
        header.mem_total = stats["mem_total_gb"]

        # Swap
        try:
            import psutil
            swap = psutil.swap_memory()
            header.swap_used = swap.used / (1024**3)
        except Exception:
            pass

        # Energy/thermal data
        if self._energy_collector.has_sudo:
            thermal = self._energy_collector.latest.thermal
            header.die_temp = thermal.cpu_die_temp
            header.fan_rpm = thermal.fan_speed_rpm
            header.thermal_pressure = thermal.thermal_pressure
            header.gpu_percent = thermal.gpu_active_percent

        # Network
        net = self._net_collector.latest
        header.net_down = format_bytes_per_s(net.bytes_recv_per_s)
        header.net_up = format_bytes_per_s(net.bytes_sent_per_s)

        # Self overhead
        try:
            import psutil
            self_proc = psutil.Process(self._self_pid)
            header.self_cpu = self_proc.cpu_percent()
        except Exception:
            pass

        header.update_display()

    def _update_reclaim(self):
        """Update the Reclaim (Kill List) view with scored candidates."""
        try:
            reclaim = self.query_one(f"#{VIEW_RECLAIM}", ReclaimPanel)
        except Exception:
            return

        history = self._proc_collector.metrics_history
        candidates = []

        for key, group in self._groups.items():
            pids = [p.pid for p in group.processes]
            has_high_wakeups = False
            if self._energy_collector.has_sudo:
                for p in group.processes:
                    einfo = self._energy_collector.latest.processes.get(p.pid)
                    if einfo and einfo.wakeups_per_s > 100:
                        has_high_wakeups = True
                        break

            # Determine if hidden (heuristic: no process is frontmost)
            is_hidden = all(
                p.is_hidden or p.is_system for p in group.processes
            )

            candidate = history.compute_reclaim_score(
                group_key=key,
                app_name=group.name,
                icon=group.icon,
                pids=pids,
                total_cpu=group.total_cpu,
                total_memory_mb=group.total_memory_mb,
                child_count=len(group.processes),
                is_hidden=is_hidden,
                is_system=all(p.is_system for p in group.processes),
                has_high_wakeups=has_high_wakeups,
                energy_impact=group.energy_impact,
                launch_age_s=group.processes[0].launch_age_s if group.processes else 0,
            )
            candidates.append(candidate)

        # Sort by score descending
        candidates.sort(key=lambda c: c.score, reverse=True)
        reclaim.update_data(candidates)

    async def _context_lane_tick(self):
        """Context lane: browser tabs, IDE projects, etc. (every 3s)."""
        if self._paused or not self._groups:
            return

        # Chrome tab mapping via CDP
        try:
            chrome_snapshot = await self._chrome_mapper.collect()
            if chrome_snapshot.has_cdp:
                self._context_cache["Google Chrome"] = {
                    "type": "chrome_cdp",
                    "data": chrome_snapshot,
                }
        except Exception:
            pass

        # Only inspect top 5 hottest groups
        top_groups = list(self._groups.values())[:5]

        for group in top_groups:
            name_lower = group.name.lower()

            # Browser inspection
            for browser in ("chrome", "brave", "arc", "safari", "firefox"):
                if browser in name_lower:
                    ctx = await self._browser_inspector.inspect(group.name)
                    if ctx:
                        self._context_cache[group.name] = {
                            "type": "browser",
                            "data": ctx,
                        }
                    break

            # IDE inspection
            for p in group.processes[:1]:
                ide_ctx = await self._ide_inspector.inspect(p.name, p.cmdline, p.pid)
                if ide_ctx:
                    self._context_cache[group.name] = {
                        "type": "ide",
                        "data": ide_ctx,
                    }
                    group.context_label = ide_ctx.project_name or ide_ctx.window_title
                    group.confidence = ide_ctx.confidence
                    break

        # Container inspection
        containers = await self._container_inspector.inspect()
        if containers:
            self._context_cache["Docker Desktop"] = {
                "type": "containers",
                "data": containers,
            }

    async def _net_lane_tick(self):
        """Network lane: system I/O deltas (every 2s)."""
        if self._paused:
            return
        await self._net_collector.collect()

    # ─── View Actions ────────────────────────────────

    def action_view_apps(self):
        self._switch_view(VIEW_APPS)

    def action_view_tree(self):
        self._switch_view(VIEW_TREE)

    def action_view_pressure(self):
        self._switch_view(VIEW_PRESSURE)

    def action_view_energy(self):
        self._switch_view(VIEW_ENERGY)

    def action_view_reclaim(self):
        self._switch_view(VIEW_RECLAIM)

    def action_cycle_view(self):
        """Cycle through views with Tab."""
        idx = VIEW_KEYS.index(self._current_view) if self._current_view in VIEW_KEYS else 0
        next_view = VIEW_KEYS[(idx + 1) % len(VIEW_KEYS)]
        self._switch_view(next_view)

    def action_toggle_system(self):
        """Toggle hiding of system processes."""
        self._hide_system = not self._hide_system
        label = "hidden" if self._hide_system else "visible"
        self.notify(f"System processes: {label}", timeout=2)

    # ─── Process Actions ─────────────────────────────

    def action_cycle_sort(self):
        new_sort = self._proc_collector.cycle_sort()
        self.notify(f"Sort: {new_sort}", timeout=1)

    def action_toggle_pause(self):
        """Toggle pause/resume."""
        self._paused = not self._paused
        try:
            header = self.query_one("#system-header", SystemHeader)
            header.paused = self._paused
            header.update_display()
        except Exception:
            pass
        if self._paused:
            self.notify("⏸  PAUSED — press Space to resume", timeout=99999, severity="warning")
        else:
            self.notify("▶  Resumed", timeout=2)

    def action_expand_selected(self):
        """Expand/collapse the selected group."""
        if self._current_view == VIEW_RECLAIM:
            return
        try:
            ptree = self.query_one(f"#{self._current_view}", ProcessTree)
            group_key = ptree.toggle_selected()
            if group_key:
                if not self._paused:
                    self.action_toggle_pause()
            self._update_detail_panel()
        except Exception:
            pass

    def action_toggle_tree(self):
        self.action_expand_selected()

    def on_data_table_row_selected(self, event):
        self.action_expand_selected()

    def action_kill_process(self):
        if not self._paused:
            self.action_toggle_pause()
        self._signal_selected(signal.SIGTERM, "Terminated")

    def action_force_kill(self):
        self._signal_selected(signal.SIGKILL, "Killed")

    def action_suspend_resume(self):
        try:
            pid = self._get_active_pid()
            if pid:
                import psutil
                proc = psutil.Process(pid)
                if proc.status() == "stopped":
                    proc.resume()
                    self.notify(f"Resumed PID {pid}", timeout=2)
                else:
                    proc.suspend()
                    self.notify(f"Suspended PID {pid}", timeout=2)
        except Exception as e:
            self.notify(f"Error: {e}", timeout=3, severity="error")

    def _get_active_pid(self) -> int | None:
        """Get PID from the currently active view."""
        if self._current_view == VIEW_RECLAIM:
            try:
                reclaim = self.query_one(f"#{VIEW_RECLAIM}", ReclaimPanel)
                gk = reclaim.get_selected_group_key()
                if gk and gk in self._groups:
                    group = self._groups[gk]
                    if group.processes:
                        return group.processes[0].pid
            except Exception:
                pass
            return None
        else:
            try:
                ptree = self.query_one(f"#{self._current_view}", ProcessTree)
                return ptree.get_selected_pid()
            except Exception:
                return None

    def _signal_selected(self, sig, action_name: str):
        pid = self._get_active_pid()
        if not pid:
            return
        try:
            if pid == self._self_pid:
                self.notify("Can't kill myself!", timeout=2, severity="warning")
                return
            import psutil
            proc = psutil.Process(pid)
            proc.send_signal(sig)
            self.notify(f"{action_name} PID {pid}", timeout=2)
        except Exception as e:
            self.notify(f"Error: {e}", timeout=3, severity="error")

    def action_profile(self):
        pid = self._get_active_pid()
        if pid:
            self.push_screen(SampleScreen(pid))
        else:
            self.notify("Select a process first", timeout=2)

    def action_file_io(self):
        pid = self._get_active_pid()
        if pid:
            if os.geteuid() != 0:
                self.notify("fs_usage requires sudo", timeout=2, severity="warning")
                return
            self.push_screen(FsUsageScreen(pid))

    def action_network(self):
        pid = self._get_active_pid()
        self.push_screen(NettopScreen(pid))

    def action_syscalls(self):
        pid = self._get_active_pid()
        if pid:
            if os.geteuid() != 0:
                self.notify("sc_usage requires sudo", timeout=2, severity="warning")
                return
            self.push_screen(ScUsageScreen(pid))

    def action_filter(self):
        try:
            filter_input = self.query_one("#filter-input", Input)
            filter_input.display = True
            filter_input.focus()
        except Exception:
            pass

    def action_clear_filter(self):
        try:
            filter_input = self.query_one("#filter-input", Input)
            filter_input.display = False
            filter_input.value = ""
            self._proc_collector.filter_text = ""
        except Exception:
            pass

    def on_input_submitted(self, event: Input.Submitted):
        if event.input.id == "filter-input":
            self._proc_collector.filter_text = event.value
            event.input.display = False
            self.notify(f"Filter: {event.value}" if event.value else "Filter cleared", timeout=1)

    def action_show_context(self):
        self._update_detail_panel()

    def _update_detail_panel(self):
        """Update the detail panel based on current selection."""
        try:
            detail = self.query_one("#detail-panel", DetailPanel)

            if self._current_view == VIEW_RECLAIM:
                reclaim = self.query_one(f"#{VIEW_RECLAIM}", ReclaimPanel)
                candidate = reclaim.get_selected_candidate()
                if candidate:
                    group = self._groups.get(candidate.group_key)
                    if group:
                        detail.show_reclaim(candidate, group, self._proc_collector.metrics_history)
                    else:
                        detail.show_empty()
                else:
                    detail.show_empty()
                return

            ptree = self.query_one(f"#{self._current_view}", ProcessTree)
            group = ptree.get_selected_group()
            pid = ptree.get_selected_pid()

            if group:
                ctx = self._context_cache.get(group.name)
                if ctx and ctx["type"] == "chrome_cdp":
                    detail.show_chrome_cdp(group, ctx["data"])
                elif ctx and ctx["type"] == "browser":
                    detail.show_browser(group, ctx["data"])
                else:
                    detail.show_group(group, self._proc_collector.metrics_history)
            elif pid:
                for g in self._groups.values():
                    for p in g.processes:
                        if p.pid == pid:
                            ctx = self._context_cache.get(g.name)
                            if ctx and ctx["type"] == "ide":
                                detail.show_ide(p, ctx["data"])
                            else:
                                detail.show_process(p, g, self._proc_collector.metrics_history)
                            return
            else:
                detail.show_empty()
        except Exception:
            pass

    def on_data_table_row_highlighted(self, event):
        self._update_detail_panel()

    def action_help(self):
        help_text = """
  ╔════════════════════════════════════╗
  ║    MacJet — Flight Deck       ║
  ╠════════════════════════════════════╣
  ║  Views:                           ║
  ║    1-5    Switch view mode        ║
  ║    Tab    Cycle views             ║
  ║                                   ║
  ║  Navigation:                      ║
  ║    ↑/↓    Move selection          ║
  ║    Enter  Expand/collapse         ║
  ║    /      Filter processes        ║
  ║    Esc    Clear filter            ║
  ║    h      Hide system processes   ║
  ║                                   ║
  ║  View Modes:                      ║
  ║    s      Cycle sort mode         ║
  ║    w      Show context            ║
  ║                                   ║
  ║  Actions:                         ║
  ║    k      Kill (SIGTERM)          ║
  ║    K      Force kill (SIGKILL)    ║
  ║    z      Suspend / resume        ║
  ║                                   ║
  ║  Drill-Down:                      ║
  ║    p      CPU profile (sample)    ║
  ║    f      File I/O (fs_usage)*    ║
  ║    n      Network (nettop)        ║
  ║    y      Syscalls (sc_usage)*    ║
  ║                                   ║
  ║  * requires sudo                  ║
  ║    Space  Pause / resume          ║
  ║    q      Quit                    ║
  ║    ?      This help               ║
  ╚════════════════════════════════════╝
"""
        self.notify(help_text, timeout=10)

    async def action_quit(self):
        """Clean shutdown."""
        await self._energy_collector.stop()
        self.exit()


def main():
    """Entry point for MacJet."""
    app = MacJetApp()
    app.run()


if __name__ == "__main__":
    main()
