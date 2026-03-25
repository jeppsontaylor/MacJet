"""
MacJet MCP — Tool handler implementations.
All business logic for the 10 MCP tools.
"""
from __future__ import annotations

import signal
from typing import Any

import psutil

from macjet.collectors.process_collector import ProcessCollector, ProcessGroup, get_system_stats
from macjet.collectors.energy_collector import EnergyCollector
from macjet.inspectors.chrome_tab_mapper import ChromeTabMapper
from macjet.mcp.cache import AsyncTTLCache
from macjet.mcp.models import (
    ChildProcess,
    ChromeTab,
    ChromeTabsResult,
    EnergyEntry,
    EnergyReport,
    HeatExplanation,
    KillResult,
    NetworkEntry,
    NetworkReport,
    ProcessDetail,
    ProcessListResult,
    ProcessSummary,
    SuspendResult,
    SystemOverview,
)
from macjet.mcp import safety


# ── Helpers ──────────────────────────────────────────────────

async def _get_groups(
    proc_collector: ProcessCollector,
    energy_collector: EnergyCollector,
    chrome_mapper: ChromeTabMapper,
    cache: AsyncTTLCache,
) -> dict[str, ProcessGroup]:
    """Collect enriched process groups via cache."""
    async def _collect():
        procs, groups = await proc_collector.collect()

        # Enrich with energy
        if energy_collector.has_sudo:
            energy = energy_collector.latest
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

        # Enrich Chrome tabs
        if chrome_mapper.latest.has_cdp:
            for key, group in groups.items():
                for p in group.processes:
                    if "renderer" in p.name.lower() or "Helper" in p.name:
                        tab = chrome_mapper.get_tab_for_pid(p.pid)
                        if tab:
                            label = chrome_mapper.format_tab_label(tab, max_len=60)
                            p.context_label = f"🌐 {label}"
                            p.confidence = "exact"

        return groups

    return await cache.get("process_groups", _collect)


def _group_to_summary(name: str, group: ProcessGroup) -> ProcessSummary:
    """Convert a ProcessGroup to a ProcessSummary model."""
    top_proc = max(group.processes, key=lambda p: p.cpu_percent) if group.processes else None
    child_energies = [p.energy_impact for p in group.processes if p.energy_impact]
    energy_order = {"HIGH": 3, "MED": 2, "LOW": 1}
    worst_energy = max(child_energies, key=lambda e: energy_order.get(e, 0)) if child_energies else ""

    return ProcessSummary(
        name=name,
        pid_count=len(group.processes),
        top_pid=top_proc.pid if top_proc else 0,
        total_cpu=round(group.total_cpu, 1),
        total_memory_mb=round(group.total_memory_mb, 1),
        energy_impact=worst_energy,
        context_label=group.context_label or "",
    )


def _thermal_pressure() -> str:
    """Get macOS thermal pressure level."""
    try:
        import subprocess
        result = subprocess.run(
            ["sysctl", "-n", "machdep.xcpm.cpu_thermal_level"],
            capture_output=True, text=True, timeout=2,
        )
        level = int(result.stdout.strip()) if result.returncode == 0 else 0
        if level >= 100:
            return "critical"
        elif level >= 70:
            return "heavy"
        elif level >= 30:
            return "moderate"
        return "nominal"
    except Exception:
        return "unknown"


# ── Tool Implementations ────────────────────────────────────

async def handle_get_system_overview(
    proc_collector: ProcessCollector,
    energy_collector: EnergyCollector,
    chrome_mapper: ChromeTabMapper,
    cache: AsyncTTLCache,
) -> SystemOverview:
    """Collect system-wide metrics."""
    stats = get_system_stats()
    groups = await _get_groups(proc_collector, energy_collector, chrome_mapper, cache)

    # Find top process
    sorted_groups = sorted(groups.items(), key=lambda x: x[1].total_cpu, reverse=True)
    top_name = sorted_groups[0][0] if sorted_groups else "none"
    top_cpu = sorted_groups[0][1].total_cpu if sorted_groups else 0.0

    # Build verdict
    cpu = stats["cpu_percent"]
    pressure = _thermal_pressure()
    if cpu > 80 or pressure in ("heavy", "critical"):
        verdict = f"🔴 High load — {top_name} using {top_cpu:.0f}% CPU, thermal: {pressure}"
    elif cpu > 50:
        verdict = f"🟡 Moderate load — {top_name} is top at {top_cpu:.0f}% CPU"
    else:
        verdict = f"🟢 System is cool — {cpu:.0f}% CPU, thermal: {pressure}"

    return SystemOverview(
        cpu_percent=stats["cpu_percent"],
        memory_used_gb=round(stats["mem_used_gb"], 1),
        memory_total_gb=round(stats["mem_total_gb"], 1),
        memory_percent=stats["mem_percent"],
        thermal_pressure=pressure,
        fan_rpm=None,  # Populated by energy collector if available
        top_process=top_name,
        top_cpu_percent=round(top_cpu, 1),
        process_count=sum(len(g.processes) for g in groups.values()),
        verdict=verdict,
    )


async def handle_list_processes(
    proc_collector: ProcessCollector,
    energy_collector: EnergyCollector,
    chrome_mapper: ChromeTabMapper,
    cache: AsyncTTLCache,
    sort_by: str = "cpu",
    filter: str = "",
    limit: int = 25,
) -> ProcessListResult:
    """List process groups with filtering and sorting."""
    groups = await _get_groups(proc_collector, energy_collector, chrome_mapper, cache)

    # Filter
    if filter:
        fl = filter.lower()
        groups = {
            k: v for k, v in groups.items()
            if fl in k.lower() or any(fl in p.name.lower() or fl in p.context_label.lower() for p in v.processes)
        }

    # Sort
    sort_fns = {
        "cpu": lambda x: x[1].total_cpu,
        "memory": lambda x: x[1].total_memory_mb,
        "name": lambda x: x[0].lower(),
        "energy": lambda x: max(({"HIGH": 3, "MED": 2, "LOW": 1}.get(p.energy_impact, 0) for p in x[1].processes), default=0),
    }
    sort_fn = sort_fns.get(sort_by, sort_fns["cpu"])
    reverse = sort_by != "name"
    sorted_items = sorted(groups.items(), key=sort_fn, reverse=reverse)

    total = len(sorted_items)
    limited = sorted_items[:limit]

    return ProcessListResult(
        groups=[_group_to_summary(name, group) for name, group in limited],
        total_groups=total,
        sort_by=sort_by,
        filter_applied=filter,
    )


async def handle_get_process_detail(
    proc_collector: ProcessCollector,
    energy_collector: EnergyCollector,
    chrome_mapper: ChromeTabMapper,
    cache: AsyncTTLCache,
    name: str = "",
    pid: int = 0,
) -> ProcessDetail:
    """Deep-dive into a specific process or group."""
    groups = await _get_groups(proc_collector, energy_collector, chrome_mapper, cache)

    # Find the matching group
    target_group = None
    target_name = ""

    if pid > 0:
        for gname, group in groups.items():
            for p in group.processes:
                if p.pid == pid:
                    target_group = group
                    target_name = gname
                    break
            if target_group:
                break
    elif name:
        nl = name.lower()
        for gname, group in groups.items():
            if nl in gname.lower():
                target_group = group
                target_name = gname
                break

    if not target_group:
        return ProcessDetail(
            name=name or str(pid),
            total_cpu=0, total_memory_mb=0, process_count=0,
            children=[], why_hot="Process not found.",
        )

    # Build children
    sorted_procs = sorted(target_group.processes, key=lambda p: p.cpu_percent, reverse=True)
    children = [
        ChildProcess(
            pid=p.pid,
            name=p.name,
            cpu_percent=round(p.cpu_percent, 1),
            memory_mb=round(p.memory_mb, 1),
            threads=p.num_threads,
            energy_impact=p.energy_impact,
            context_label=p.context_label,
            cmdline=" ".join(p.cmdline[:5]) if p.cmdline else "",
        )
        for p in sorted_procs[:50]
    ]

    # Chrome tabs if applicable
    chrome_tabs = None
    if "chrome" in target_name.lower() and chrome_mapper.latest.has_cdp:
        tabs = chrome_mapper.latest.tabs
        chrome_tabs = [
            ChromeTab(
                rank=i + 1,
                title=t.title,
                url=t.url,
                domain=t.url.split("/")[2] if len(t.url.split("/")) > 2 else "",
                renderer_pid=t.pid,
                cpu_time_s=t.cpu_time,
            )
            for i, t in enumerate(sorted(tabs, key=lambda t: t.cpu_time, reverse=True)[:30])
        ]

    # Why hot explanation
    why = ""
    if target_group.total_cpu > 100:
        why = f"{target_name} is using {target_group.total_cpu:.0f}% CPU across {len(target_group.processes)} processes."
        if children:
            why += f" Top child: {children[0].name} (PID {children[0].pid}) at {children[0].cpu_percent}% CPU."
    elif target_group.total_cpu > 50:
        why = f"{target_name} is moderately active at {target_group.total_cpu:.0f}% CPU."
    else:
        why = f"{target_name} is idle ({target_group.total_cpu:.1f}% CPU)."

    return ProcessDetail(
        name=target_name,
        total_cpu=round(target_group.total_cpu, 1),
        total_memory_mb=round(target_group.total_memory_mb, 1),
        energy_impact=target_group.energy_impact,
        process_count=len(target_group.processes),
        children=children,
        chrome_tabs=chrome_tabs,
        why_hot=why,
    )


async def handle_get_chrome_tabs(
    chrome_mapper: ChromeTabMapper,
    sort_by: str = "cpu",
    limit: int = 30,
) -> ChromeTabsResult:
    """List Chrome tabs mapped to PIDs."""
    data = chrome_mapper.latest

    if not data.has_cdp:
        return ChromeTabsResult(tabs=[], total_tabs=0, cdp_connected=False)

    tabs = data.tabs
    if sort_by == "cpu":
        tabs = sorted(tabs, key=lambda t: t.cpu_time, reverse=True)
    elif sort_by == "title":
        tabs = sorted(tabs, key=lambda t: t.title.lower())

    chrome_tabs = [
        ChromeTab(
            rank=i + 1,
            title=t.title,
            url=t.url,
            domain=t.url.split("/")[2] if len(t.url.split("/")) > 2 else "",
            renderer_pid=t.pid,
            cpu_time_s=t.cpu_time,
        )
        for i, t in enumerate(tabs[:limit])
    ]

    return ChromeTabsResult(
        tabs=chrome_tabs,
        total_tabs=len(data.tabs),
        cdp_connected=True,
    )


async def handle_explain_heat(
    proc_collector: ProcessCollector,
    energy_collector: EnergyCollector,
    chrome_mapper: ChromeTabMapper,
    cache: AsyncTTLCache,
) -> HeatExplanation:
    """Diagnose why the Mac is hot."""
    stats = get_system_stats()
    groups = await _get_groups(proc_collector, energy_collector, chrome_mapper, cache)
    cpu = stats["cpu_percent"]

    sorted_groups = sorted(groups.items(), key=lambda x: x[1].total_cpu, reverse=True)

    # Severity
    if cpu > 80:
        severity = "critical"
    elif cpu > 60:
        severity = "hot"
    elif cpu > 40:
        severity = "warm"
    else:
        severity = "cool"

    primary = sorted_groups[0] if sorted_groups else ("idle", ProcessGroup(name="idle"))
    secondary = [f"{name} ({g.total_cpu:.0f}% CPU)" for name, g in sorted_groups[1:4]]

    # Build markdown report
    report_lines = [f"## System Heat Diagnosis", f"", f"**Overall CPU:** {cpu:.1f}%", f"**Thermal:** {_thermal_pressure()}", ""]
    report_lines.append(f"### Primary: {primary[0]} ({primary[1].total_cpu:.0f}% CPU)")
    for p in sorted(primary[1].processes, key=lambda p: p.cpu_percent, reverse=True)[:5]:
        label = p.context_label or p.name
        report_lines.append(f"- {label} → PID {p.pid} → {p.cpu_percent:.1f}% CPU")

    if secondary:
        report_lines.append("")
        report_lines.append("### Also notable:")
        for s in secondary:
            report_lines.append(f"- {s}")

    # Recommendations
    recs = []
    if primary[1].total_cpu > 100:
        recs.append(f"Consider closing or restarting {primary[0]} (using {primary[1].total_cpu:.0f}% CPU)")
    if "chrome" in primary[0].lower() and chrome_mapper.latest.has_cdp:
        top_tabs = sorted(chrome_mapper.latest.tabs, key=lambda t: t.cpu_time, reverse=True)[:3]
        for t in top_tabs:
            recs.append(f"Close Chrome tab: \"{t.title}\" ({t.cpu_time:.0f}s CPU time)")

    return HeatExplanation(
        severity=severity,
        cpu_percent=cpu,
        primary_culprit=primary[0],
        primary_cpu_percent=round(primary[1].total_cpu, 1),
        secondary_culprits=secondary,
        recommendations=recs,
        detailed_report="\n".join(report_lines),
    )


async def handle_kill_process(
    pid: int,
    reason: str,
    force: bool,
    client_id: str,
    request_id: str,
) -> KillResult:
    """Execute kill after validation."""
    is_safe, err = safety.validate_pid(pid)
    if not is_safe:
        info = safety.resolve_pid(pid)
        return KillResult(
            action="error", pid=pid, name=info.get("name", "unknown"),
            success=False, error=err,
        )

    info = safety.resolve_pid(pid)
    sig = signal.SIGKILL if force else signal.SIGTERM
    sig_name = "SIGKILL" if force else "SIGTERM"

    success, result = safety.send_signal(pid, sig, reason, client_id, request_id)

    return KillResult(
        action=sig_name,
        pid=pid,
        name=info.get("name", "unknown"),
        success=success,
        error="" if success else result,
        audit_id=result if success else None,
    )


async def handle_suspend_process(
    pid: int,
    resume: bool,
    reason: str,
    client_id: str,
    request_id: str,
) -> SuspendResult:
    """Suspend or resume a process."""
    is_safe, err = safety.validate_pid(pid)
    if not is_safe:
        return SuspendResult(
            action="error", pid=pid, name="unknown",
            success=False, error=err,
        )

    info = safety.resolve_pid(pid)
    sig = signal.SIGCONT if resume else signal.SIGSTOP
    sig_name = "SIGCONT" if resume else "SIGSTOP"

    success, result = safety.send_signal(pid, sig, reason, client_id, request_id)

    return SuspendResult(
        action=sig_name,
        pid=pid,
        name=info.get("name", "unknown"),
        success=success,
        error="" if success else result,
    )


async def handle_get_energy_report(
    energy_collector: EnergyCollector,
    limit: int = 15,
) -> EnergyReport:
    """Get energy impact breakdown."""
    if not energy_collector.has_sudo:
        return EnergyReport(available=False)

    data = energy_collector.latest
    entries = []
    for pid, einfo in sorted(data.processes.items(), key=lambda x: x[1].energy_impact, reverse=True)[:limit]:
        cat = "HIGH" if einfo.energy_impact > 50 else "MED" if einfo.energy_impact > 20 else "LOW" if einfo.energy_impact > 5 else ""
        entries.append(EnergyEntry(
            name=einfo.name,
            energy_impact=round(einfo.energy_impact, 1),
            category=cat,
        ))

    return EnergyReport(
        available=True,
        entries=entries,
        cpu_power_w=data.cpu_power if hasattr(data, 'cpu_power') else None,
        gpu_power_w=data.gpu_power if hasattr(data, 'gpu_power') else None,
    )


async def handle_get_network_activity(
    proc_collector: ProcessCollector,
    cache: AsyncTTLCache,
    sort_by: str = "total",
    limit: int = 15,
) -> NetworkReport:
    """Get network I/O by process group."""
    stats = get_system_stats()

    async def _collect():
        _, groups = await proc_collector.collect()
        return groups

    groups = await cache.get("process_groups_net", _collect)

    entries = []
    for name, group in groups.items():
        sent = group.total_net_sent
        recv = group.total_net_recv
        if sent + recv > 0:
            entries.append(NetworkEntry(
                name=name,
                bytes_sent=sent,
                bytes_recv=recv,
                total_bytes=sent + recv,
            ))

    sort_fns = {
        "total": lambda e: e.total_bytes,
        "sent": lambda e: e.bytes_sent,
        "recv": lambda e: e.bytes_recv,
    }
    entries.sort(key=sort_fns.get(sort_by, sort_fns["total"]), reverse=True)

    return NetworkReport(
        entries=entries[:limit],
        system_bytes_sent=stats.get("net_bytes_sent", 0),
        system_bytes_recv=stats.get("net_bytes_recv", 0),
    )
