"""
MacJet MCP — Resource URI handlers.
Read-only data endpoints for agents to subscribe to.
"""
from __future__ import annotations

import json

from macjet.collectors.process_collector import ProcessCollector, get_system_stats
from macjet.collectors.energy_collector import EnergyCollector
from macjet.inspectors.chrome_tab_mapper import ChromeTabMapper
from macjet.mcp.cache import AsyncTTLCache
from macjet.mcp import safety
from macjet.mcp.tools import (
    handle_get_system_overview,
    handle_list_processes,
    handle_get_chrome_tabs,
    handle_get_process_detail,
    handle_get_energy_report,
)


async def resource_system_overview(
    proc_collector: ProcessCollector,
    energy_collector: EnergyCollector,
    chrome_mapper: ChromeTabMapper,
    cache: AsyncTTLCache,
) -> str:
    """macjet://system/overview"""
    result = await handle_get_system_overview(proc_collector, energy_collector, chrome_mapper, cache)
    return result.model_dump_json(indent=2)


async def resource_processes_top(
    proc_collector: ProcessCollector,
    energy_collector: EnergyCollector,
    chrome_mapper: ChromeTabMapper,
    cache: AsyncTTLCache,
) -> str:
    """macjet://processes/top"""
    result = await handle_list_processes(proc_collector, energy_collector, chrome_mapper, cache, limit=25)
    return result.model_dump_json(indent=2)


async def resource_process_by_name(
    proc_collector: ProcessCollector,
    energy_collector: EnergyCollector,
    chrome_mapper: ChromeTabMapper,
    cache: AsyncTTLCache,
    name: str,
) -> str:
    """macjet://processes/{name}"""
    result = await handle_get_process_detail(proc_collector, energy_collector, chrome_mapper, cache, name=name)
    return result.model_dump_json(indent=2)


async def resource_chrome_tabs(chrome_mapper: ChromeTabMapper) -> str:
    """macjet://chrome/tabs"""
    result = await handle_get_chrome_tabs(chrome_mapper)
    return result.model_dump_json(indent=2)


async def resource_energy_report(energy_collector: EnergyCollector) -> str:
    """macjet://energy/report"""
    result = await handle_get_energy_report(energy_collector)
    return result.model_dump_json(indent=2)


async def resource_audit_log() -> str:
    """macjet://audit/log"""
    return safety.get_audit_log(limit=50)
