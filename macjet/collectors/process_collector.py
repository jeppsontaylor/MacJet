"""
MacJet — Fast-lane Process Collector (250ms)
Uses psutil to enumerate processes, build trees, and group by coalition/app.
"""

from __future__ import annotations

import asyncio
import time
from dataclasses import dataclass, field

import psutil

from .metrics_history import MetricsHistory


@dataclass
class ProcessInfo:
    """Snapshot of a single process."""

    pid: int
    name: str
    cpu_percent: float = 0.0
    memory_mb: float = 0.0
    memory_percent: float = 0.0
    num_threads: int = 0
    cmdline: list[str] = field(default_factory=list)
    cwd: str = ""
    exe: str = ""
    ppid: int = 0
    status: str = ""
    create_time: float = 0.0
    username: str = ""
    children_pids: list[int] = field(default_factory=list)
    # Enriched later
    context_label: str = ""
    confidence: str = "grouped"  # exact|window-exact|app-exact|inferred|grouped
    energy_impact: str = ""
    net_bytes_sent: int = 0
    net_bytes_recv: int = 0
    # New fields for Flight Deck
    role_type: str = ""  # renderer|gpu-process|utility|extension|"" for non-helper
    is_hidden: bool = False
    launch_age_s: float = 0.0
    is_system: bool = False


@dataclass
class ProcessGroup:
    """A group of related processes (coalition/app/tree root)."""

    name: str
    icon: str = "🟢"
    total_cpu: float = 0.0
    total_memory_mb: float = 0.0
    total_net_recv: int = 0
    total_net_sent: int = 0
    energy_impact: str = ""
    processes: list[ProcessInfo] = field(default_factory=list)
    context_label: str = ""
    confidence: str = "grouped"
    why_hot: str = ""
    is_expanded: bool = False


def _severity_icon(cpu: float) -> str:
    if cpu > 100:
        return "🔴"
    elif cpu > 50:
        return "🟠"
    elif cpu > 25:
        return "🟡"
    return "🟢"


def _safe_get(proc: psutil.Process, attr: str, default=None):
    """Safely get process attribute, handling AccessDenied / ZombieProcess."""
    try:
        val = getattr(proc, attr)
        return val() if callable(val) else val
    except (psutil.AccessDenied, psutil.ZombieProcess, psutil.NoSuchProcess, OSError):
        return default


def _parse_app_name(proc_info: ProcessInfo) -> str:
    """Extract a meaningful app name from process info."""
    name = proc_info.name
    cmdline = proc_info.cmdline

    # Chrome / Brave / Arc helpers
    if "Helper" in name and cmdline:
        for arg in cmdline:
            if arg.startswith("--type="):
                helper_type = arg.split("=", 1)[1]
                parent_name = name.split(" Helper")[0]
                return f"{parent_name} ({helper_type})"

    # Node.js — show script path
    if name in ("node", "Node") and len(cmdline) > 1:
        for arg in cmdline[1:]:
            if not arg.startswith("-"):
                return f"node {arg}"
        return "node"

    # Python — show script path
    if name.startswith("python") or name == "Python":
        for arg in cmdline[1:] if len(cmdline) > 1 else []:
            if not arg.startswith("-") and arg != "-m":
                return f"python {arg}"

    # Java — show jar or main class
    if name == "java" and cmdline:
        for i, arg in enumerate(cmdline):
            if arg == "-jar" and i + 1 < len(cmdline):
                return f"java -jar {cmdline[i+1]}"

    return name


def _determine_group_key(proc_info: ProcessInfo) -> str:
    """Determine the app/coalition group for a process."""
    name = proc_info.name
    cmdline = proc_info.cmdline

    # Browser helpers → group under parent browser
    for browser in ("Google Chrome", "Brave Browser", "Arc", "Safari", "Firefox"):
        if browser.split()[0].lower() in name.lower():
            return browser

    # Electron apps with --type=renderer
    if cmdline:
        for arg in cmdline:
            if "--type=" in arg:
                # Walk up to find parent app
                try:
                    parent = psutil.Process(proc_info.ppid)
                    parent_name = parent.name()
                    if parent_name and "Helper" not in parent_name:
                        return parent_name
                except (psutil.NoSuchProcess, psutil.AccessDenied):
                    pass

    # Docker
    if name in ("com.docker.vmnetd", "com.docker.backend", "Docker", "docker"):
        return "Docker Desktop"

    # VSCode / Cursor helpers
    for ide in ("Code Helper", "Cursor Helper"):
        if ide in name:
            return ide.split(" Helper")[0]

    return name


# System usernames that indicate a system/daemon process
_SYSTEM_USERS = frozenset(
    {
        "root",
        "_windowserver",
        "_mdnsresponder",
        "_coreaudiod",
        "_locationd",
        "_spotlight",
        "_securityagent",
        "_usbmuxd",
        "_distnoted",
        "_networkd",
        "_appleevents",
        "_softwareupdate",
        "_nsurlsessiond",
        "_trustd",
        "_timed",
        "nobody",
        "daemon",
    }
)


def _extract_role_type(cmdline: list[str]) -> str:
    """Extract subprocess role type from cmdline (e.g., renderer, gpu-process)."""
    for arg in cmdline:
        if arg.startswith("--type="):
            return arg.split("=", 1)[1]
    return ""


def _is_system_process(username: str, exe: str) -> bool:
    """Determine if a process is a system/daemon process."""
    if username in _SYSTEM_USERS:
        return True
    if exe and (
        exe.startswith("/usr/")
        or exe.startswith("/System/")
        or exe.startswith("/sbin/")
        or exe.startswith("/Library/Apple/")
    ):
        return True
    return False


class ProcessCollector:
    """Async process data collector using psutil."""

    def __init__(self):
        self._prev_snapshot: dict[int, ProcessInfo] = {}
        self._groups: dict[str, ProcessGroup] = {}
        self._all_procs: list[ProcessInfo] = []
        self._sort_key = "cpu"  # cpu|memory|name|pid
        self._filter_text = ""
        self._grouping_mode = "app"  # coalition|app|tree|flat
        self.metrics_history = MetricsHistory()

    @property
    def groups(self) -> dict[str, ProcessGroup]:
        return self._groups

    @property
    def all_procs(self) -> list[ProcessInfo]:
        return self._all_procs

    @property
    def sort_key(self) -> str:
        return self._sort_key

    @sort_key.setter
    def sort_key(self, value: str):
        self._sort_key = value

    @property
    def filter_text(self) -> str:
        return self._filter_text

    @filter_text.setter
    def filter_text(self, value: str):
        self._filter_text = value.lower()

    @property
    def grouping_mode(self) -> str:
        return self._grouping_mode

    @grouping_mode.setter
    def grouping_mode(self, value: str):
        self._grouping_mode = value

    def cycle_sort(self) -> str:
        """Cycle through sort modes."""
        modes = ["cpu", "memory", "name", "pid", "threads"]
        idx = modes.index(self._sort_key) if self._sort_key in modes else 0
        self._sort_key = modes[(idx + 1) % len(modes)]
        return self._sort_key

    def cycle_grouping(self) -> str:
        """Cycle through grouping modes."""
        modes = ["app", "tree", "flat"]
        idx = modes.index(self._grouping_mode) if self._grouping_mode in modes else 0
        self._grouping_mode = modes[(idx + 1) % len(modes)]
        return self._grouping_mode

    async def collect(self) -> tuple[list[ProcessInfo], dict[str, ProcessGroup]]:
        """Collect process data asynchronously."""
        loop = asyncio.get_event_loop()
        return await loop.run_in_executor(None, self._collect_sync)

    def _collect_sync(self) -> tuple[list[ProcessInfo], dict[str, ProcessGroup]]:
        """Synchronous collection (runs in executor)."""
        procs: list[ProcessInfo] = []

        for proc in psutil.process_iter():
            try:
                with proc.oneshot():
                    info = ProcessInfo(
                        pid=proc.pid,
                        name=proc.name() or "",
                        cpu_percent=proc.cpu_percent() or 0.0,
                        memory_mb=(_safe_get(proc, "memory_info") or type("", (), {"rss": 0})).rss
                        / (1024 * 1024),
                        memory_percent=proc.memory_percent() or 0.0,
                        num_threads=_safe_get(proc, "num_threads") or 0,
                        cmdline=_safe_get(proc, "cmdline") or [],
                        cwd=_safe_get(proc, "cwd") or "",
                        exe=_safe_get(proc, "exe") or "",
                        ppid=_safe_get(proc, "ppid") or 0,
                        status=_safe_get(proc, "status") or "",
                        create_time=_safe_get(proc, "create_time") or 0.0,
                        username=_safe_get(proc, "username") or "",
                    )
                    info.context_label = _parse_app_name(info)
                    if info.context_label != info.name:
                        info.confidence = "exact"
                    # New Flight Deck fields
                    info.role_type = _extract_role_type(info.cmdline)
                    info.is_system = _is_system_process(info.username, info.exe)
                    info.launch_age_s = (
                        time.time() - info.create_time if info.create_time > 0 else 0.0
                    )
                    # Record into ring buffer
                    self.metrics_history.record(info.pid, info.cpu_percent, info.memory_mb)
                    procs.append(info)
            except (psutil.NoSuchProcess, psutil.AccessDenied, psutil.ZombieProcess):
                continue

        # Sort
        sort_funcs = {
            "cpu": lambda p: p.cpu_percent,
            "memory": lambda p: p.memory_mb,
            "name": lambda p: p.name.lower(),
            "pid": lambda p: p.pid,
            "threads": lambda p: p.num_threads,
        }
        sort_fn = sort_funcs.get(self._sort_key, sort_funcs["cpu"])
        reverse = self._sort_key not in ("name", "pid")
        procs.sort(key=sort_fn, reverse=reverse)

        # Filter
        if self._filter_text:
            procs = [
                p
                for p in procs
                if self._filter_text in p.name.lower()
                or self._filter_text in p.context_label.lower()
                or self._filter_text in " ".join(p.cmdline).lower()
            ]

        # Group
        groups: dict[str, ProcessGroup] = {}
        if self._grouping_mode == "flat":
            for p in procs:
                groups[str(p.pid)] = ProcessGroup(
                    name=p.context_label,
                    icon=_severity_icon(p.cpu_percent),
                    total_cpu=p.cpu_percent,
                    total_memory_mb=p.memory_mb,
                    processes=[p],
                    confidence=p.confidence,
                )
        else:  # app or tree grouping
            for p in procs:
                key = _determine_group_key(p) if self._grouping_mode == "app" else p.name
                if key not in groups:
                    groups[key] = ProcessGroup(name=key, processes=[])
                g = groups[key]
                g.processes.append(p)
                g.total_cpu += p.cpu_percent
                g.total_memory_mb += p.memory_mb
                g.total_net_recv += p.net_bytes_recv
                g.total_net_sent += p.net_bytes_sent

            # Update icons and sort groups
            for g in groups.values():
                g.icon = _severity_icon(g.total_cpu)
                if len(g.processes) > 1:
                    g.confidence = (
                        "app-exact"
                        if any(p.confidence in ("exact", "window-exact") for p in g.processes)
                        else "grouped"
                    )
                elif g.processes:
                    g.confidence = g.processes[0].confidence

        # Sort groups by total CPU
        sorted_groups = dict(sorted(groups.items(), key=lambda x: x[1].total_cpu, reverse=True))

        self._all_procs = procs
        self._groups = sorted_groups

        # Expire stale ring buffer entries
        self.metrics_history.expire_stale()

        return procs, sorted_groups


def get_system_stats() -> dict:
    """Get overall system stats."""
    cpu_percent = psutil.cpu_percent(interval=None)
    mem = psutil.virtual_memory()

    # Network
    net = psutil.net_io_counters()

    # CPU frequency
    freq = psutil.cpu_freq()

    # CPU count
    cpu_count = psutil.cpu_count()
    cpu_count_logical = psutil.cpu_count(logical=True)

    return {
        "cpu_percent": cpu_percent,
        "cpu_count": cpu_count,
        "cpu_count_logical": cpu_count_logical,
        "cpu_freq_current": freq.current if freq else 0,
        "cpu_freq_max": freq.max if freq else 0,
        "mem_total_gb": mem.total / (1024**3),
        "mem_used_gb": mem.used / (1024**3),
        "mem_percent": mem.percent,
        "net_bytes_sent": net.bytes_sent,
        "net_bytes_recv": net.bytes_recv,
    }
