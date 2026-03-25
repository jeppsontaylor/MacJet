"""
MacJet — Process Tree Widget (Flight Deck Edition)
Interactive DataTable with semantic colormaps, role-bucket grouping,
severity rails, and inline sparklines.
"""

from __future__ import annotations

from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import DataTable, Static

from ..collectors.metrics_history import MetricsHistory
from ..collectors.process_collector import ProcessGroup, ProcessInfo

# ─── Afterburner CPU Color Ramp ──────────────────────
_CPU_RAMP = [
    (5, "#22D3EE"),  # cyan — cool
    (20, "#3B82F6"),  # blue
    (50, "#8B5CF6"),  # violet
    (80, "#D946EF"),  # magenta
    (999, "#FB7185"),  # hot pink — critical
]


def _cpu_color(pct: float) -> str:
    """Afterburner ramp for CPU values."""
    for threshold, color in _CPU_RAMP:
        if pct <= threshold:
            return color
    return _CPU_RAMP[-1][1]


# ─── Aurora Memory Color Ramp ────────────────────────
_MEM_RAMP = [
    (100, "#34D399"),  # green — light
    (500, "#A3E635"),  # lime
    (1000, "#F59E0B"),  # amber
    (2000, "#F97316"),  # orange
    (99999, "#EF4444"),  # red — critical
]


def _mem_color(mb: float) -> str:
    """Aurora ramp for memory values."""
    for threshold, color in _MEM_RAMP:
        if mb <= threshold:
            return color
    return _MEM_RAMP[-1][1]


# ─── Severity Rail ───────────────────────────────────
def _severity_rail(cpu: float) -> str:
    """Single-char colored severity indicator."""
    if cpu > 100:
        return "[#FF4D6D]█[/]"
    elif cpu > 50:
        return "[#FF8A4C]█[/]"
    elif cpu > 25:
        return "[#FDBA35]▐[/]"
    elif cpu > 5:
        return "[#7F8DB3]▏[/]"
    return " "


def _format_mem(mb: float) -> str:
    if mb >= 1024:
        return f"{mb / 1024:.1f}G"
    return f"{mb:.0f}M"


def _confidence_badge(conf: str) -> str:
    colors = {
        "exact": "[#32D583]",
        "window-exact": "[#60A5FA]",
        "app-exact": "[#A78BFA]",
        "inferred": "[#FDBA35]",
        "grouped": "[#7F8DB3]",
    }
    color = colors.get(conf, "[#7F8DB3]")
    return f"{color}[{conf}][/]"


# ─── Role Bucket Helpers ─────────────────────────────
_ROLE_LABELS = {
    "renderer": "Renderer",
    "gpu-process": "GPU",
    "utility": "Utility",
    "extension": "Extension",
    "crashpad-handler": "Crashpad",
    "ppapi": "Plugin",
    "broker": "Broker",
}


def _build_role_buckets(processes: list[ProcessInfo]) -> dict[str, list[ProcessInfo]]:
    """Group child processes by their role_type."""
    buckets: dict[str, list[ProcessInfo]] = {}
    for p in processes:
        role = p.role_type or "other"
        if role not in buckets:
            buckets[role] = []
        buckets[role].append(p)
    return buckets


class ProcessTree(Widget):
    """Interactive process table with semantic colors and role-bucket grouping."""

    DEFAULT_CSS = """
    ProcessTree {
        height: 1fr;
        background: #0A0F1E;
    }
    ProcessTree #process-toolbar {
        height: 1;
        background: #10182B;
        border-bottom: solid #1A2540;
        padding: 0 1;
        color: #7F8DB3;
    }
    ProcessTree DataTable {
        height: 1fr;
    }
    """

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self._table: DataTable | None = None
        self._expanded_groups: set[str] = set()
        self._expanded_roles: set[tuple[str, str]] = set()  # (group_key, role)
        self._show_all_groups: set[str] = set()
        self._row_keys: list[str] = []
        self._current_groups: dict[str, ProcessGroup] = {}
        self._metrics: MetricsHistory | None = None
        self._sort_label = "cpu"
        self._view_label = "apps"
        self._filter_text = ""

    def compose(self) -> ComposeResult:
        yield Static(
            "  PROCESSES  [#7F8DB3]Enter:expand  s:sort  k:kill  ?:help[/]", id="process-toolbar"
        )
        table = DataTable(id="process-table", cursor_type="row")
        table.add_column("", key="rail", width=1)
        table.add_column("", key="icon", width=3)
        table.add_column("Process", key="name", width=28)
        table.add_column("CPU%", key="cpu", width=8)
        table.add_column("Memory", key="mem", width=8)
        table.add_column("Trend", key="trend", width=12)
        table.add_column("Threads", key="threads", width=7)
        table.add_column("Energy", key="energy", width=7)
        table.add_column("Context", key="context", width=22)
        self._table = table
        yield table

    def update_toolbar(self, sort_key: str, view: str, filter_text: str):
        """Update the toolbar labels."""
        self._sort_label = sort_key
        self._view_label = view
        self._filter_text = filter_text
        filter_display = f"  /{filter_text}" if filter_text else ""
        try:
            toolbar = self.query_one("#process-toolbar", Static)
            toolbar.update(
                f"  PROCESSES [{view.upper()}] sort:{sort_key}{filter_display}  "
                f"[#7F8DB3]Enter:expand  s:sort  k:kill  ?:help[/]"
            )
        except Exception:
            pass

    def update_data(self, groups: dict[str, ProcessGroup], metrics: MetricsHistory | None = None):
        """Update the table with new process group data."""
        if not self._table:
            return

        self._current_groups = groups
        if metrics:
            self._metrics = metrics
        table = self._table

        # Save cursor
        saved_cursor_row = table.cursor_row
        saved_cursor_key = ""
        if 0 <= saved_cursor_row < len(self._row_keys):
            saved_cursor_key = self._row_keys[saved_cursor_row]

        table.clear()
        self._row_keys = []

        for group_key, group in groups.items():
            procs_count = len(group.processes)

            if procs_count == 1:
                # Single process
                p = group.processes[0]
                rail = _severity_rail(p.cpu_percent)
                icon = group.icon
                name = p.context_label or p.name
                if len(name) > 27:
                    name = name[:26] + "…"
                cpu_color = _cpu_color(p.cpu_percent)
                cpu_str = f"[{cpu_color}]{p.cpu_percent:.1f}[/]"
                mem_color = _mem_color(p.memory_mb)
                mem_str = f"[{mem_color}]{_format_mem(p.memory_mb)}[/]"
                trend = self._get_sparkline(p.pid) if self._metrics else ""
                threads = str(p.num_threads)
                energy = p.energy_impact or ""
                ctx = p.context_label if p.context_label and p.context_label != p.name else ""
                context = ctx or _confidence_badge(p.confidence)

                row_key = f"pid-{p.pid}"
                table.add_row(
                    rail,
                    icon,
                    name,
                    cpu_str,
                    mem_str,
                    trend,
                    threads,
                    energy,
                    context,
                    key=row_key,
                )
                self._row_keys.append(row_key)
            else:
                # Group header
                rail = _severity_rail(group.total_cpu)
                icon = group.icon
                name = f"{group.name} ({procs_count})"
                if len(name) > 27:
                    name = name[:26] + "…"
                cpu_color = _cpu_color(group.total_cpu)
                cpu_str = f"[{cpu_color}]{group.total_cpu:.1f}[/]"
                mem_color = _mem_color(group.total_memory_mb)
                mem_str = f"[{mem_color}]{_format_mem(group.total_memory_mb)}[/]"

                # Group sparkline
                pids = [p.pid for p in group.processes]
                trend = self._get_group_sparkline(pids) if self._metrics else ""

                # Energy from worst child
                child_energies = [p.energy_impact for p in group.processes if p.energy_impact]
                energy_order = {"HIGH": 3, "MED": 2, "LOW": 1}
                if child_energies:
                    energy = max(child_energies, key=lambda e: energy_order.get(e, 0))
                else:
                    energy = group.energy_impact or ""

                # Context
                hot_children = [p for p in group.processes if p.energy_impact == "HIGH"]
                if hot_children:
                    context = f"[#FF4D6D]{len(hot_children)} HIGH energy[/]"
                elif group.confidence in ("exact", "window-exact", "app-exact"):
                    context = _confidence_badge(group.confidence)
                else:
                    context = ""

                is_expanded = group_key in self._expanded_groups
                expand_icon = "▾" if is_expanded else "▸"

                row_key = f"group-{group_key}"
                table.add_row(
                    rail,
                    f"{expand_icon}{icon}",
                    name,
                    cpu_str,
                    mem_str,
                    trend,
                    "",
                    energy,
                    context,
                    key=row_key,
                )
                self._row_keys.append(row_key)

                # Show children if expanded
                if is_expanded:
                    self._render_children(table, group_key, group)

        # Restore cursor
        if saved_cursor_key and saved_cursor_key in self._row_keys:
            new_idx = self._row_keys.index(saved_cursor_key)
            table.move_cursor(row=new_idx)
        elif saved_cursor_row < len(self._row_keys):
            table.move_cursor(row=saved_cursor_row)

    def _render_children(self, table: DataTable, group_key: str, group: ProcessGroup):
        """Render children with role-bucket grouping."""
        sorted_children = sorted(group.processes, key=lambda p: p.cpu_percent, reverse=True)

        # Check if role-bucket grouping makes sense (>5 children with roles)
        has_roles = sum(1 for p in sorted_children if p.role_type) > 3
        use_role_buckets = has_roles and len(sorted_children) > 5

        if use_role_buckets:
            self._render_role_buckets(table, group_key, sorted_children)
        else:
            self._render_flat_children(table, group_key, sorted_children)

    def _render_role_buckets(self, table: DataTable, group_key: str, children: list[ProcessInfo]):
        """Render children grouped by role type."""
        buckets = _build_role_buckets(children)

        # Sort buckets by total CPU
        sorted_buckets = sorted(
            buckets.items(),
            key=lambda x: sum(p.cpu_percent for p in x[1]),
            reverse=True,
        )

        for i, (role, procs) in enumerate(sorted_buckets):
            is_last_bucket = i == len(sorted_buckets) - 1
            connector = "└─" if is_last_bucket else "├─"
            role_label = _ROLE_LABELS.get(role, role.capitalize())

            total_cpu = sum(p.cpu_percent for p in procs)
            total_mem = sum(p.memory_mb for p in procs)
            count = len(procs)

            cpu_color = _cpu_color(total_cpu)
            mem_color = _mem_color(total_mem)

            # Check if this role bucket is expanded
            role_expanded = (group_key, role) in self._expanded_roles

            if count == 1:
                # Single process in bucket — show directly
                p = procs[0]
                rail = _severity_rail(p.cpu_percent)
                name = f"  {connector} {role_label}"
                cpu_str = f"[{cpu_color}]{p.cpu_percent:.1f}[/]"
                mem_str = f"[{mem_color}]{_format_mem(p.memory_mb)}[/]"
                trend = self._get_sparkline(p.pid) if self._metrics else ""

                child_key = f"child-{p.pid}"
                table.add_row(
                    rail,
                    "  ",
                    name,
                    cpu_str,
                    mem_str,
                    trend,
                    str(p.num_threads),
                    p.energy_impact or "",
                    "",
                    key=child_key,
                )
                self._row_keys.append(child_key)
            else:
                # Role bucket header
                expand_char = "▾" if role_expanded else "▸"
                rail = _severity_rail(total_cpu)
                name = f"  {connector} {expand_char}{role_label} ×{count}"
                cpu_str = f"[{cpu_color}]{total_cpu:.1f}[/]"
                mem_str = f"[{mem_color}]{_format_mem(total_mem)}[/]"

                pids = [p.pid for p in procs]
                trend = self._get_group_sparkline(pids) if self._metrics else ""

                role_key = f"role-{group_key}-{role}"
                table.add_row(
                    rail,
                    "  ",
                    name,
                    cpu_str,
                    mem_str,
                    trend,
                    "",
                    "",
                    f"[#7F8DB3]{count} processes[/]",
                    key=role_key,
                )
                self._row_keys.append(role_key)

                # Show individual PIDs if role bucket is expanded
                if role_expanded:
                    sorted_procs = sorted(procs, key=lambda p: p.cpu_percent, reverse=True)
                    for j, p in enumerate(sorted_procs[:15]):
                        is_last = j == min(len(sorted_procs), 15) - 1
                        sub_conn = "└─" if is_last else "├─"
                        p_cpu_color = _cpu_color(p.cpu_percent)
                        p_mem_color = _mem_color(p.memory_mb)
                        p_name = p.context_label or f"#{p.pid}"
                        if len(p_name) > 20:
                            p_name = p_name[:19] + "…"

                        p_key = f"child-{p.pid}"
                        table.add_row(
                            _severity_rail(p.cpu_percent),
                            "    ",
                            f"    {sub_conn} {p_name}",
                            f"[{p_cpu_color}]{p.cpu_percent:.1f}[/]",
                            f"[{p_mem_color}]{_format_mem(p.memory_mb)}[/]",
                            self._get_sparkline(p.pid) if self._metrics else "",
                            str(p.num_threads),
                            p.energy_impact or "",
                            "",
                            key=p_key,
                        )
                        self._row_keys.append(p_key)

                    remaining = len(sorted_procs) - 15
                    if remaining > 0:
                        rem_cpu = sum(p.cpu_percent for p in sorted_procs[15:])
                        rem_mem = sum(p.memory_mb for p in sorted_procs[15:])
                        more_key = f"more-{group_key}-{role}"
                        table.add_row(
                            " ",
                            "    ",
                            f"    └─ {remaining} more",
                            f"[#7F8DB3]{rem_cpu:.1f}[/]",
                            f"[#7F8DB3]{_format_mem(rem_mem)}[/]",
                            "",
                            "",
                            "",
                            "",
                            key=more_key,
                        )
                        self._row_keys.append(more_key)

    def _render_flat_children(self, table: DataTable, group_key: str, children: list[ProcessInfo]):
        """Render children as a flat list (no role grouping)."""
        show_all = group_key in self._show_all_groups
        display_limit = len(children) if show_all else 15

        for i, p in enumerate(children[:display_limit]):
            is_last = i == min(len(children), display_limit) - 1
            connector = "└─" if is_last and (show_all or len(children) <= display_limit) else "├─"

            rail = _severity_rail(p.cpu_percent)
            if p.context_label:
                child_name = p.context_label
            elif p.role_type:
                child_name = f"{_ROLE_LABELS.get(p.role_type, p.role_type)} #{p.pid}"
            else:
                child_name = p.name or "?"

            if len(child_name) > 22:
                child_name = child_name[:21] + "…"

            cpu_color = _cpu_color(p.cpu_percent)
            mem_color = _mem_color(p.memory_mb)

            child_key = f"child-{p.pid}"
            table.add_row(
                rail,
                "  ",
                f"  {connector} {child_name}",
                f"[{cpu_color}]{p.cpu_percent:.1f}[/]",
                f"[{mem_color}]{_format_mem(p.memory_mb)}[/]",
                self._get_sparkline(p.pid) if self._metrics else "",
                str(p.num_threads),
                p.energy_impact or "",
                _confidence_badge(p.confidence),
                key=child_key,
            )
            self._row_keys.append(child_key)

        remaining = len(children) - display_limit
        if remaining > 0:
            rem_cpu = sum(p.cpu_percent for p in children[display_limit:])
            rem_mem = sum(p.memory_mb for p in children[display_limit:])
            more_key = f"more-{group_key}"
            table.add_row(
                " ",
                "  ",
                f"  └─ {remaining} hidden",
                f"[#7F8DB3]{rem_cpu:.1f}%[/]",
                f"[#7F8DB3]{_format_mem(rem_mem)}[/]",
                "",
                "",
                "",
                "[#7F8DB3]Enter to expand[/]",
                key=more_key,
            )
            self._row_keys.append(more_key)

    def _get_sparkline(self, pid: int) -> str:
        """Get a sparkline for a single PID."""
        if not self._metrics:
            return ""
        spark = self._metrics.sparkline(pid, width=10)
        return f"[#45D6FF]{spark}[/]"

    def _get_group_sparkline(self, pids: list[int]) -> str:
        """Get a combined sparkline for a group of PIDs."""
        if not self._metrics:
            return ""
        spark = self._metrics.sparkline_for_group(pids, width=10)
        return f"[#45D6FF]{spark}[/]"

    # ─── Interaction ─────────────────────────────────

    def toggle_selected(self) -> str | None:
        """Toggle expansion of the currently selected item."""
        row_key = self._get_current_row_key()
        if not row_key:
            return None

        if row_key.startswith("group-"):
            group_key = row_key[6:]
            if group_key in self._expanded_groups:
                self._expanded_groups.discard(group_key)
            else:
                self._expanded_groups.add(group_key)
            self.update_data(self._current_groups, self._metrics)
            return group_key

        if row_key.startswith("role-"):
            # Parse role-{group_key}-{role}
            parts = row_key[5:]
            # Find the last dash that separates group_key from role
            # Role names don't contain dashes, but group keys might
            for role_name in _ROLE_LABELS:
                suffix = f"-{role_name}"
                if parts.endswith(suffix):
                    group_key = parts[: -len(suffix)]
                    role = role_name
                    break
            else:
                # Try "other"
                if parts.endswith("-other"):
                    group_key = parts[:-6]
                    role = "other"
                else:
                    return None

            role_tuple = (group_key, role)
            if role_tuple in self._expanded_roles:
                self._expanded_roles.discard(role_tuple)
            else:
                self._expanded_roles.add(role_tuple)
            self.update_data(self._current_groups, self._metrics)
            return group_key

        if row_key.startswith("more-"):
            group_key = row_key[5:]
            self._show_all_groups.add(group_key)
            self.update_data(self._current_groups, self._metrics)
            return group_key

        return None

    def _get_current_row_key(self) -> str | None:
        if not self._table or not self._row_keys:
            return None
        cursor_row = self._table.cursor_row
        if 0 <= cursor_row < len(self._row_keys):
            return self._row_keys[cursor_row]
        return None

    def get_selected_pid(self) -> int | None:
        row_key = self._get_current_row_key()
        if not row_key:
            return None
        if row_key.startswith("pid-"):
            try:
                return int(row_key[4:])
            except ValueError:
                return None
        elif row_key.startswith("child-"):
            try:
                return int(row_key[6:])
            except ValueError:
                return None
        if row_key.startswith("group-"):
            group_key = row_key[6:]
            group = self._current_groups.get(group_key)
            if group and group.processes:
                return group.processes[0].pid
        return None

    def get_selected_group_key(self) -> str | None:
        row_key = self._get_current_row_key()
        if not row_key:
            return None
        if row_key.startswith("group-"):
            return row_key[6:]
        return None

    def get_selected_group(self) -> ProcessGroup | None:
        gk = self.get_selected_group_key()
        if gk:
            return self._current_groups.get(gk)
        return None
