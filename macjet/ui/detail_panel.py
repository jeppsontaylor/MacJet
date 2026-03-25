"""
MacJet — Detail Panel (Inspector Rail)
Shows sparklines, why-hot analysis, process metadata, and action shortcuts
for the selected process or group.
"""
from __future__ import annotations

from textual.widget import Widget
from textual.app import ComposeResult
from textual.widgets import Static

from ..collectors.process_collector import ProcessGroup, ProcessInfo
from ..collectors.metrics_history import MetricsHistory, ReclaimCandidate

# Optional imports for browser/IDE context
try:
    from ..inspectors.browser_inspector import BrowserContext
except ImportError:
    BrowserContext = None

try:
    from ..inspectors.ide_inspector import IDEContext
except ImportError:
    IDEContext = None

try:
    from ..inspectors.chrome_tab_mapper import ChromeSnapshot
except ImportError:
    ChromeSnapshot = None


def _format_mem(mb: float) -> str:
    if mb >= 1024:
        return f"{mb / 1024:.1f}GB"
    return f"{mb:.0f}MB"


def _format_duration(seconds: float) -> str:
    if seconds < 60:
        return f"{int(seconds)}s"
    elif seconds < 3600:
        return f"{int(seconds / 60)}m"
    else:
        h = int(seconds / 3600)
        m = int((seconds % 3600) / 60)
        return f"{h}h{m}m" if m else f"{h}h"


class DetailPanel(Widget):
    """Right-side inspector panel showing deep context for selected process/group."""

    DEFAULT_CSS = """
    DetailPanel {
        width: 38;
        border-left: solid #1A2540;
        background: #10182B;
        padding: 0 1;
    }
    DetailPanel #detail-title {
        height: 1;
        text-style: bold;
        color: #60A5FA;
    }
    DetailPanel #detail-body {
        height: 1fr;
        color: #E6ECFF;
    }
    """

    def compose(self) -> ComposeResult:
        yield Static("  [#60A5FA]Inspector[/]", id="detail-title")
        yield Static("  [#7F8DB3]Select a process to inspect[/]", id="detail-body")

    def _update(self, title: str, body: str):
        try:
            self.query_one("#detail-title", Static).update(title)
            self.query_one("#detail-body", Static).update(body)
        except Exception:
            pass

    def show_empty(self):
        self._update(
            "  [#60A5FA]Inspector[/]",
            "  [#7F8DB3]Select a process to inspect[/]"
        )

    def show_message(self, title: str, message: str):
        """Show a simple message in the inspector."""
        self._update(f"  [#FF8A4C]{title}[/]", f"  [#7F8DB3]{message}[/]")

    def show_process(self, proc: ProcessInfo, group: ProcessGroup | None = None,
                     metrics: MetricsHistory | None = None):
        """Show details for a single process."""
        title = f"  📋 {proc.context_label or proc.name}"

        lines = []
        # Sparkline
        if metrics:
            spark = metrics.sparkline(proc.pid, width=28)
            lines.append(f"  [#45D6FF]{spark}[/]")
            lines.append(f"  [#7F8DB3]60s CPU trend[/]")
            lines.append("")

        lines.append(f"  [#7F8DB3]PID:[/]     [#E6ECFF]{proc.pid}[/]")
        lines.append(f"  [#7F8DB3]CPU:[/]     [#E6ECFF]{proc.cpu_percent:.1f}%[/]")
        lines.append(f"  [#7F8DB3]Memory:[/]  [#E6ECFF]{_format_mem(proc.memory_mb)}[/]")
        lines.append(f"  [#7F8DB3]Threads:[/] [#E6ECFF]{proc.num_threads}[/]")
        lines.append(f"  [#7F8DB3]Status:[/]  [#E6ECFF]{proc.status}[/]")

        if proc.launch_age_s > 0:
            lines.append(f"  [#7F8DB3]Age:[/]     [#E6ECFF]{_format_duration(proc.launch_age_s)}[/]")

        if proc.exe:
            exe_short = proc.exe if len(proc.exe) <= 30 else "…" + proc.exe[-29:]
            lines.append(f"  [#7F8DB3]Exe:[/]  [#E6ECFF]{exe_short}[/]")

        if proc.role_type:
            lines.append(f"  [#7F8DB3]Role:[/]    [#A78BFA]{proc.role_type}[/]")

        if proc.is_system:
            lines.append(f"  [#7F8DB3]Type:[/]    [#FF8A4C]System[/]")

        if proc.energy_impact:
            energy_color = "#FF4D6D" if proc.energy_impact == "HIGH" else (
                "#FF8A4C" if proc.energy_impact == "MED" else "#32D583"
            )
            lines.append(f"  [#7F8DB3]Energy:[/]  [{energy_color}]{proc.energy_impact}[/]")

        # Memory trend
        if metrics:
            growth = metrics.memory_growth_rate(proc.pid)
            if abs(growth) > 1:
                growth_color = "#FF4D6D" if growth > 10 else "#FDBA35"
                lines.append(f"  [#7F8DB3]Δ Mem:[/]   [{growth_color}]{growth:+.0f}MB/min[/]")

        lines.append("")
        lines.append("  [#7F8DB3]─── Actions ───────────[/]")
        lines.append("  [#60A5FA]k[/] Kill  [#60A5FA]K[/] Force Kill")
        lines.append("  [#60A5FA]p[/] Profile  [#60A5FA]z[/] Suspend")

        self._update(title, "\n".join(lines))

    def show_group(self, group: ProcessGroup, metrics: MetricsHistory | None = None):
        """Show details for a process group with sparkline and role breakdown."""
        title = f"  📋 {group.name} ({len(group.processes)})"

        lines = []
        # Group sparkline
        if metrics:
            pids = [p.pid for p in group.processes]
            spark = metrics.sparkline_for_group(pids, width=28)
            lines.append(f"  [#45D6FF]{spark}[/]")
            lines.append(f"  [#7F8DB3]60s CPU trend (group)[/]")
            lines.append("")

        lines.append(f"  [#7F8DB3]CPU:[/]     [#E6ECFF]{group.total_cpu:.1f}%[/]")
        lines.append(f"  [#7F8DB3]Memory:[/]  [#E6ECFF]{_format_mem(group.total_memory_mb)}[/]")
        lines.append(f"  [#7F8DB3]Procs:[/]   [#E6ECFF]{len(group.processes)}[/]")

        # Memory trend
        if metrics:
            total_growth = sum(
                max(0, metrics.memory_growth_rate(p.pid))
                for p in group.processes
            )
            if total_growth > 1:
                growth_color = "#FF4D6D" if total_growth > 50 else "#FDBA35"
                lines.append(f"  [#7F8DB3]Δ Mem:[/]   [{growth_color}]{total_growth:+.0f}MB/min[/]")

        # Why hot analysis
        if group.total_cpu > 10:
            lines.append("")
            lines.append("  [#FF8A4C]🔥 Why hot:[/]")
            reasons = self._analyze_why_hot(group, metrics)
            for reason in reasons:
                lines.append(f"  [#E6ECFF]{reason}[/]")

        # Role breakdown
        roles: dict[str, tuple[int, float, float]] = {}
        for p in group.processes:
            role = p.role_type or "main"
            if role not in roles:
                roles[role] = (0, 0.0, 0.0)
            count, cpu, mem = roles[role]
            roles[role] = (count + 1, cpu + p.cpu_percent, mem + p.memory_mb)

        if len(roles) > 1:
            lines.append("")
            lines.append("  [#7F8DB3]─── Breakdown ─────────[/]")
            sorted_roles = sorted(roles.items(), key=lambda x: x[1][1], reverse=True)
            for role, (count, cpu, mem) in sorted_roles:
                label = role.capitalize()
                lines.append(f"  [#E6ECFF]{label} ×{count}[/]  [#7F8DB3]{cpu:.1f}%  {_format_mem(mem)}[/]")

        lines.append("")
        lines.append("  [#7F8DB3]─── Actions ───────────[/]")
        lines.append("  [#60A5FA]k[/] Kill  [#60A5FA]K[/] Force Kill")
        lines.append("  [#60A5FA]p[/] Profile  [#60A5FA]z[/] Suspend")

        self._update(title, "\n".join(lines))

    def _analyze_why_hot(self, group: ProcessGroup, metrics: MetricsHistory | None) -> list[str]:
        """Generate why-hot analysis reasons."""
        reasons = []

        if group.total_cpu > 80:
            reasons.append("Sustained high CPU usage")
        elif group.total_cpu > 30:
            reasons.append("Elevated CPU usage")

        # Check for process storms
        renderers = sum(1 for p in group.processes if p.role_type == "renderer")
        if renderers > 10:
            reasons.append(f"Renderer storm: {renderers} renderers")

        # Memory growth
        if metrics:
            growth = sum(
                max(0, metrics.memory_growth_rate(p.pid))
                for p in group.processes
            )
            if growth > 20:
                reasons.append(f"Memory growing +{growth:.0f}MB/min")

        # Hidden/background
        if all(not hasattr(p, 'is_hidden') or p.is_hidden for p in group.processes):
            reasons.append("Running in background")

        # High energy
        high_energy = sum(1 for p in group.processes if p.energy_impact == "HIGH")
        if high_energy:
            reasons.append(f"{high_energy} high-energy processes")

        if not reasons:
            reasons.append("Active usage")

        return reasons

    def show_reclaim(self, candidate: ReclaimCandidate, group: ProcessGroup,
                     metrics: MetricsHistory | None = None):
        """Show Reclaim details for a kill candidate."""
        risk_colors = {
            "safe": "#32D583",
            "review": "#FDBA35",
            "danger": "#FF4D6D",
        }
        risk_color = risk_colors.get(candidate.risk, "#7F8DB3")

        title = f"  ⚡ {candidate.app_name}"
        lines = []

        # Sparkline
        if metrics:
            pids = [p.pid for p in group.processes]
            spark = metrics.sparkline_for_group(pids, width=28)
            lines.append(f"  [#45D6FF]{spark}[/]")
            lines.append("")

        lines.append(f"  [#7F8DB3]Score:[/]   [#E6ECFF]{candidate.score}/100[/]")
        lines.append(f"  [#7F8DB3]Risk:[/]    [{risk_color}]{candidate.risk.upper()}[/]")
        lines.append(f"  [#7F8DB3]Reclaim:[/] [#E6ECFF]~{candidate.reclaim_cpu:.0f}% CPU / {_format_mem(candidate.reclaim_mem_mb)}[/]")
        lines.append("")
        lines.append(f"  [#FF8A4C]Reason:[/]")
        lines.append(f"  [#E6ECFF]{candidate.reason}[/]")
        lines.append("")
        lines.append(f"  [#7F8DB3]Suggested:[/] [#60A5FA]{candidate.suggested_action}[/]")
        lines.append(f"  [#7F8DB3]Children:[/]  [#E6ECFF]{candidate.child_count}[/]")

        if candidate.launch_age_s > 0:
            lines.append(f"  [#7F8DB3]Age:[/]       [#E6ECFF]{_format_duration(candidate.launch_age_s)}[/]")

        lines.append("")
        lines.append("  [#7F8DB3]─── Actions ───────────[/]")
        lines.append("  [#60A5FA]k[/] Terminate  [#60A5FA]K[/] Force Kill")
        lines.append("  [#60A5FA]p[/] Sample     [#60A5FA]z[/] Suspend")

        self._update(title, "\n".join(lines))

    def show_browser(self, group: ProcessGroup, browser_ctx):
        """Show browser-specific details."""
        title = f"  🌐 {browser_ctx.app_name}"
        lines = []
        lines.append(f"  [#7F8DB3]CPU:[/]     [#E6ECFF]{group.total_cpu:.1f}%[/]")
        lines.append(f"  [#7F8DB3]Memory:[/]  [#E6ECFF]{_format_mem(group.total_memory_mb)}[/]")
        lines.append(f"  [#7F8DB3]Windows:[/] [#E6ECFF]{browser_ctx.window_count}[/]")
        lines.append(f"  [#7F8DB3]Tabs:[/]    [#E6ECFF]{browser_ctx.tab_count}[/]")
        lines.append("")

        if browser_ctx.active_tab:
            lines.append("  [#60A5FA]▶ Active Tab:[/]")
            at = browser_ctx.active_tab
            lines.append(f"    [#E6ECFF]{at.title[:30]}[/]")
            lines.append("")

        lines.append("  [#7F8DB3]All Tabs:[/]")
        for i, tab in enumerate(browser_ctx.tabs[:10]):
            marker = "[#60A5FA]→[/] " if tab.is_active else "  "
            tab_title = tab.title[:28] if tab.title else "(untitled)"
            lines.append(f"  {marker}{i+1}. [#E6ECFF]{tab_title}[/]")

        remaining = browser_ctx.tab_count - 10
        if remaining > 0:
            lines.append(f"    [#7F8DB3]...and {remaining} more[/]")

        self._update(title, "\n".join(lines))

    def show_ide(self, proc: ProcessInfo, ide_ctx):
        """Show IDE-specific details."""
        title = f"  💻 {ide_ctx.ide_name}"
        lines = []
        if ide_ctx.project_name:
            lines.append(f"  [#7F8DB3]Project:[/] [#E6ECFF]{ide_ctx.project_name}[/]")
        if ide_ctx.project_path:
            lines.append(f"  [#7F8DB3]Path:[/]    [#E6ECFF]{ide_ctx.project_path}[/]")
        if ide_ctx.active_file:
            lines.append(f"  [#7F8DB3]File:[/]    [#E6ECFF]{ide_ctx.active_file}[/]")
        if ide_ctx.window_title:
            lines.append(f"  [#7F8DB3]Window:[/]  [#E6ECFF]{ide_ctx.window_title}[/]")
        lines.append("")
        lines.append(f"  [#7F8DB3]CPU:[/]     [#E6ECFF]{proc.cpu_percent:.1f}%[/]")
        lines.append(f"  [#7F8DB3]Memory:[/]  [#E6ECFF]{_format_mem(proc.memory_mb)}[/]")
        lines.append(f"  [#7F8DB3]PID:[/]     [#E6ECFF]{proc.pid}[/]")

        self._update(title, "\n".join(lines))

    def show_chrome_cdp(self, group: ProcessGroup, snapshot):
        """Show Chrome tabs with JS heap from CDP."""
        title = f"  🌐 Chrome ({snapshot.total_tabs} tabs)"
        lines = []
        lines.append(f"  [#7F8DB3]CPU:[/]     [#E6ECFF]{group.total_cpu:.1f}%[/]")
        lines.append(f"  [#7F8DB3]Memory:[/]  [#E6ECFF]{_format_mem(group.total_memory_mb)}[/]")
        lines.append(f"  [#7F8DB3]JS Heap:[/] [#E6ECFF]{snapshot.total_js_heap_mb:.0f}MB[/]")
        lines.append("")
        lines.append("  [#7F8DB3]Tabs (by JS heap):[/]")

        for tab in snapshot.tabs[:12]:
            heap_str = f"{tab.js_heap_mb:.0f}MB" if tab.js_heap_mb > 0.5 else "<1MB"
            tab_title = tab.title[:26] if tab.title else "(untitled)"

            if tab.js_heap_mb > 200:
                icon = "[#FF4D6D]●[/]"
            elif tab.js_heap_mb > 50:
                icon = "[#FF8A4C]●[/]"
            elif tab.js_heap_mb > 10:
                icon = "[#FDBA35]●[/]"
            else:
                icon = "[#32D583]●[/]"

            lines.append(f"  {icon} [#E6ECFF]{tab_title}[/]")
            lines.append(f"     [#7F8DB3]{heap_str}[/]")

        remaining = snapshot.total_tabs - 12
        if remaining > 0:
            lines.append(f"  [#7F8DB3]...and {remaining} more[/]")

        self._update(title, "\n".join(lines))
