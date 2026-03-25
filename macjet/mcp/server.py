#!/usr/bin/env python3
"""
MacJet MCP Server
=====================
Exposes MacJet's process monitor as an MCP server for AI agents.

Usage:
    python -m macjet.mcp.server     # Run via module
    ./macjet.sh --mcp               # Via launcher

Compatible with: Claude Desktop, Cursor, GARY, any MCP client.
"""
from __future__ import annotations

from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from dataclasses import dataclass

from mcp.server.fastmcp import Context, FastMCP
from mcp.server.fastmcp.prompts import base as prompt_base

from macjet.collectors.energy_collector import EnergyCollector
from macjet.collectors.process_collector import ProcessCollector
from macjet.inspectors.chrome_tab_mapper import ChromeTabMapper
from macjet.mcp import resources as resource_handlers
from macjet.mcp import tools as tool_handlers
from macjet.mcp.cache import AsyncTTLCache
from macjet.mcp.models import (
    ChromeTabsResult,
    EnergyReport,
    HeatExplanation,
    KillConfirmation,
    KillResult,
    NetworkReport,
    ProcessDetail,
    ProcessListResult,
    SuspendResult,
    SystemOverview,
)

# ── Lifespan Context ─────────────────────────────────────────


@dataclass
class AppContext:
    """Shared state initialized once at server boot."""

    proc_collector: ProcessCollector
    energy_collector: EnergyCollector
    chrome_mapper: ChromeTabMapper
    cache: AsyncTTLCache


@asynccontextmanager
async def app_lifespan(server: FastMCP) -> AsyncIterator[AppContext]:
    """Boot collectors at startup, clean up on shutdown."""
    pc = ProcessCollector()
    ec = EnergyCollector()
    cm = ChromeTabMapper()

    # Start the energy collector (async, launches powermetrics subprocess)
    await ec.start()
    # ChromeTabMapper is stateless — just call collect() per-request

    try:
        yield AppContext(
            proc_collector=pc,
            energy_collector=ec,
            chrome_mapper=cm,
            cache=AsyncTTLCache(ttl=2.0),
        )
    finally:
        await ec.stop()


# ── Server Instance ──────────────────────────────────────────

mcp = FastMCP(
    "MacJet",
    instructions="macOS process, energy, and thermal monitor for AI agents. "
    "Query CPU/memory/energy usage, Chrome tabs, and kill processes.",
    lifespan=app_lifespan,
)


# ── Helper ───────────────────────────────────────────────────


def _ctx(ctx: Context) -> AppContext:
    """Extract the typed lifespan context."""
    return ctx.request_context.lifespan_context  # type: ignore[return-value]


# ═══════════════════════════════════════════════════════════════
# TOOLS
# ═══════════════════════════════════════════════════════════════


@mcp.tool()
async def get_system_overview(ctx: Context) -> SystemOverview:
    """Get a concise snapshot of system health: CPU, memory, thermals, top process, and a plain-English verdict."""
    app = _ctx(ctx)
    await ctx.info("Collecting system overview...")
    result = await tool_handlers.handle_get_system_overview(
        app.proc_collector,
        app.energy_collector,
        app.chrome_mapper,
        app.cache,
    )
    await ctx.info(f"System: {result.verdict}")
    return result


@mcp.tool()
async def list_processes(
    ctx: Context,
    sort_by: str = "cpu",
    filter: str = "",
    limit: int = 25,
) -> ProcessListResult:
    """List running process groups sorted by resource usage. Supports filtering by name."""
    app = _ctx(ctx)
    await ctx.info(f"Listing processes (sort={sort_by}, filter='{filter}', limit={limit})")
    return await tool_handlers.handle_list_processes(
        app.proc_collector,
        app.energy_collector,
        app.chrome_mapper,
        app.cache,
        sort_by=sort_by,
        filter=filter,
        limit=limit,
    )


@mcp.tool()
async def get_process_detail(
    ctx: Context,
    name: str = "",
    pid: int = 0,
) -> ProcessDetail:
    """Deep-dive into a process group by name or specific PID. Returns children, cmdlines, tabs, energy."""
    if not name and pid <= 0:
        return ProcessDetail(
            name="",
            total_cpu=0,
            total_memory_mb=0,
            process_count=0,
            children=[],
            why_hot="Provide either 'name' or 'pid' parameter.",
        )
    app = _ctx(ctx)
    await ctx.report_progress(0.2, 1.0, "Collecting process data")
    result = await tool_handlers.handle_get_process_detail(
        app.proc_collector,
        app.energy_collector,
        app.chrome_mapper,
        app.cache,
        name=name,
        pid=pid,
    )
    await ctx.report_progress(1.0, 1.0, "Done")
    return result


@mcp.tool()
async def get_chrome_tabs(
    ctx: Context,
    sort_by: str = "cpu",
    limit: int = 30,
) -> ChromeTabsResult:
    """List Chrome tabs with renderer PIDs and CPU time. Requires Chrome to be running with remote debugging."""
    app = _ctx(ctx)
    await ctx.info("Querying Chrome DevTools Protocol...")
    return await tool_handlers.handle_get_chrome_tabs(
        app.chrome_mapper, sort_by=sort_by, limit=limit
    )


@mcp.tool()
async def explain_heat(ctx: Context) -> HeatExplanation:
    """Diagnose why the Mac is hot. Returns severity, culprits, and actionable recommendations."""
    app = _ctx(ctx)
    await ctx.info("Analyzing system heat...")
    return await tool_handlers.handle_explain_heat(
        app.proc_collector,
        app.energy_collector,
        app.chrome_mapper,
        app.cache,
    )


@mcp.tool()
async def search_processes(
    ctx: Context,
    query: str,
    limit: int = 20,
) -> ProcessListResult:
    """Search processes by name, command line, or context label."""
    app = _ctx(ctx)
    await ctx.info(f"Searching for '{query}'...")
    return await tool_handlers.handle_list_processes(
        app.proc_collector,
        app.energy_collector,
        app.chrome_mapper,
        app.cache,
        filter=query,
        limit=limit,
    )


@mcp.tool()
async def kill_process(
    ctx: Context,
    pid: int,
    reason: str = "Agent-requested termination",
    force: bool = False,
) -> KillResult:
    """Kill a process by PID (SIGTERM, or SIGKILL if force=True).
    Will attempt to confirm with the user before executing.
    Refuses system processes (PID < 500) and the MCP server itself."""
    from macjet.mcp import safety

    # Step 1: Validate
    is_safe, err = safety.validate_pid(pid)
    if not is_safe:
        await ctx.error(f"Refused: {err}")
        return KillResult(action="error", pid=pid, name="unknown", success=False, error=err)

    info = safety.resolve_pid(pid)
    sig_name = "SIGKILL" if force else "SIGTERM"

    # Step 2: Try elicitation for human confirmation
    try:
        result = await ctx.elicit(
            message=f"Kill PID {pid} ({info.get('name', '?')}, {info.get('cpu_percent', 0):.1f}% CPU)?\n"
            f"Signal: {sig_name}\nReason: {reason}",
            schema=KillConfirmation,
        )
        if result.action != "accept" or not result.data or not result.data.confirm:
            await ctx.info(f"Kill declined by user for PID {pid}")
            return KillResult(
                action="declined", pid=pid, name=info.get("name", "unknown"), success=False
            )
    except Exception:
        # Client doesn't support elicitation — proceed with warning
        await ctx.warning(f"Client doesn't support elicitation. Proceeding with kill of PID {pid}.")

    # Step 3: Execute
    await ctx.warning(f"Killing PID {pid} ({info.get('name', '?')}) with {sig_name}")
    client_id = str(ctx.client_id) if ctx.client_id else "unknown"
    request_id = str(ctx.request_id) if ctx.request_id else ""

    return await tool_handlers.handle_kill_process(
        pid=pid,
        reason=reason,
        force=force,
        client_id=client_id,
        request_id=request_id,
    )


@mcp.tool()
async def suspend_process(
    ctx: Context,
    pid: int,
    reason: str = "Agent-requested suspension",
) -> SuspendResult:
    """Suspend (SIGSTOP) a process without terminating it. Use resume_process to continue."""
    from macjet.mcp import safety

    is_safe, err = safety.validate_pid(pid)
    if not is_safe:
        return SuspendResult(action="error", pid=pid, name="unknown", success=False, error=err)

    await ctx.warning(f"Suspending PID {pid}")
    client_id = str(ctx.client_id) if ctx.client_id else "unknown"
    request_id = str(ctx.request_id) if ctx.request_id else ""

    return await tool_handlers.handle_suspend_process(
        pid=pid,
        resume=False,
        reason=reason,
        client_id=client_id,
        request_id=request_id,
    )


@mcp.tool()
async def resume_process(
    ctx: Context,
    pid: int,
    reason: str = "Agent-requested resume",
) -> SuspendResult:
    """Resume (SIGCONT) a previously suspended process."""
    client_id = str(ctx.client_id) if ctx.client_id else "unknown"
    request_id = str(ctx.request_id) if ctx.request_id else ""

    return await tool_handlers.handle_suspend_process(
        pid=pid,
        resume=True,
        reason=reason,
        client_id=client_id,
        request_id=request_id,
    )


@mcp.tool()
async def get_energy_report(
    ctx: Context,
    limit: int = 15,
) -> EnergyReport:
    """Get per-app energy impact scores from powermetrics. Requires sudo."""
    app = _ctx(ctx)
    return await tool_handlers.handle_get_energy_report(app.energy_collector, limit=limit)


@mcp.tool()
async def get_network_activity(
    ctx: Context,
    sort_by: str = "total",
    limit: int = 15,
) -> NetworkReport:
    """Get top processes by network bytes sent/received."""
    app = _ctx(ctx)
    return await tool_handlers.handle_get_network_activity(
        app.proc_collector,
        app.cache,
        sort_by=sort_by,
        limit=limit,
    )


# ═══════════════════════════════════════════════════════════════
# RESOURCES
# ═══════════════════════════════════════════════════════════════


@mcp.resource("macjet://system/overview")
async def resource_system_overview() -> str:
    """Live system stats snapshot: CPU, memory, thermals."""
    app = _ctx(mcp.get_context())
    return await resource_handlers.resource_system_overview(
        app.proc_collector,
        app.energy_collector,
        app.chrome_mapper,
        app.cache,
    )


@mcp.resource("macjet://processes/top")
async def resource_processes_top() -> str:
    """Top 25 process groups by CPU usage."""
    app = _ctx(mcp.get_context())
    return await resource_handlers.resource_processes_top(
        app.proc_collector,
        app.energy_collector,
        app.chrome_mapper,
        app.cache,
    )


@mcp.resource("macjet://processes/{name}")
async def resource_process_by_name(name: str) -> str:
    """Detailed info about a specific process group."""
    app = _ctx(mcp.get_context())
    return await resource_handlers.resource_process_by_name(
        app.proc_collector,
        app.energy_collector,
        app.chrome_mapper,
        app.cache,
        name=name,
    )


@mcp.resource("macjet://chrome/tabs")
async def resource_chrome_tabs() -> str:
    """All Chrome tabs with renderer PIDs and CPU time."""
    app = _ctx(mcp.get_context())
    return await resource_handlers.resource_chrome_tabs(app.chrome_mapper)


@mcp.resource("macjet://energy/report")
async def resource_energy_report() -> str:
    """powermetrics energy breakdown by app."""
    app = _ctx(mcp.get_context())
    return await resource_handlers.resource_energy_report(app.energy_collector)


@mcp.resource("macjet://audit/log")
async def resource_audit_log() -> str:
    """Recent MCP kill/suspend actions from the audit log."""
    return await resource_handlers.resource_audit_log()


# ═══════════════════════════════════════════════════════════════
# PROMPTS
# ═══════════════════════════════════════════════════════════════


@mcp.prompt(title="Troubleshoot Performance")
async def troubleshoot_performance(ctx: Context) -> list[prompt_base.Message]:
    """Diagnose a slow or hot Mac. Automatically attaches system overview and top processes."""
    app = _ctx(ctx)
    overview = await resource_handlers.resource_system_overview(
        app.proc_collector,
        app.energy_collector,
        app.chrome_mapper,
        app.cache,
    )
    top = await resource_handlers.resource_processes_top(
        app.proc_collector,
        app.energy_collector,
        app.chrome_mapper,
        app.cache,
    )

    return [
        prompt_base.UserMessage(
            "My Mac is running hot/slow. Here is the current system state:\n\n"
            f"## System Overview\n```json\n{overview}\n```\n\n"
            f"## Top Processes\n```json\n{top}\n```\n\n"
            "Analyze this data. Identify the root cause of high CPU/thermal pressure. "
            "Propose specific processes to terminate with rationale. "
            "If Chrome is a culprit, use the get_chrome_tabs tool to identify specific tabs."
        ),
    ]


@mcp.prompt(title="Optimize Chrome Memory")
async def optimize_chrome_memory(ctx: Context) -> list[prompt_base.Message]:
    """Find heavy Chrome tabs and recommend which to close."""
    app = _ctx(ctx)
    tabs = await resource_handlers.resource_chrome_tabs(app.chrome_mapper)

    return [
        prompt_base.UserMessage(
            "Here are my Chrome tabs with CPU usage:\n\n"
            f"```json\n{tabs}\n```\n\n"
            "Identify tabs consuming excessive CPU time or memory. "
            "Rank by impact and recommend which ones I should close. "
            "If I approve, use the kill_process tool to terminate the renderer PIDs."
        ),
    ]


@mcp.prompt(title="Generate System Report")
async def generate_system_report(ctx: Context) -> list[prompt_base.Message]:
    """Generate a comprehensive system diagnostic report."""
    app = _ctx(ctx)
    overview = await resource_handlers.resource_system_overview(
        app.proc_collector,
        app.energy_collector,
        app.chrome_mapper,
        app.cache,
    )
    top = await resource_handlers.resource_processes_top(
        app.proc_collector,
        app.energy_collector,
        app.chrome_mapper,
        app.cache,
    )
    tabs = await resource_handlers.resource_chrome_tabs(app.chrome_mapper)
    energy = await resource_handlers.resource_energy_report(app.energy_collector)

    return [
        prompt_base.UserMessage(
            "Generate a comprehensive system health report from this data:\n\n"
            f"## System Overview\n```json\n{overview}\n```\n\n"
            f"## Top Processes\n```json\n{top}\n```\n\n"
            f"## Chrome Tabs\n```json\n{tabs}\n```\n\n"
            f"## Energy Report\n```json\n{energy}\n```\n\n"
            "Format as a clean markdown report covering CPU, memory, energy, and Chrome tabs. "
            "Include recommendations. Make it suitable for sharing in Slack or a support ticket."
        ),
    ]


# ═══════════════════════════════════════════════════════════════
# ENTRY POINT
# ═══════════════════════════════════════════════════════════════


def main():
    """Run the MCP server on stdio."""
    mcp.run()


if __name__ == "__main__":
    main()
