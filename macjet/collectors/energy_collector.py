"""
MacJet — Energy Collector (2s lane)
Persistent powermetrics subprocess in plist mode.
Parses per-process energy impact, GPU time, coalition data, SMC fan/temp.
"""

from __future__ import annotations

import asyncio
import os
import plistlib
from dataclasses import dataclass, field
from typing import Optional


@dataclass
class EnergyInfo:
    """Per-process energy data from powermetrics."""

    pid: int
    name: str
    energy_impact: float = 0.0
    cpu_ms_per_s: float = 0.0
    wakeups_per_s: float = 0.0
    gpu_ms_per_s: float = 0.0
    bytes_read_per_s: float = 0.0
    bytes_written_per_s: float = 0.0
    packets_in_per_s: float = 0.0
    packets_out_per_s: float = 0.0
    coalition: str = ""


@dataclass
class ThermalInfo:
    """System-wide thermal data."""

    cpu_die_temp: float = 0.0
    gpu_die_temp: float = 0.0
    fan_speed_rpm: int = 0
    fan_speed_max: int = 0
    thermal_pressure: str = "nominal"  # nominal|moderate|heavy|critical|sleeping
    gpu_active_percent: float = 0.0


@dataclass
class EnergySnapshot:
    """Complete energy snapshot from one powermetrics sample."""

    processes: dict[int, EnergyInfo] = field(default_factory=dict)
    coalitions: dict[str, list[EnergyInfo]] = field(default_factory=dict)
    thermal: ThermalInfo = field(default_factory=ThermalInfo)
    timestamp: float = 0.0


class EnergyCollector:
    """Manages a persistent powermetrics subprocess for energy/thermal data."""

    def __init__(self):
        self._process: Optional[asyncio.subprocess.Process] = None
        self._latest: EnergySnapshot = EnergySnapshot()
        self._running = False
        self._has_sudo = False
        self._buffer = b""

    @property
    def latest(self) -> EnergySnapshot:
        return self._latest

    @property
    def has_sudo(self) -> bool:
        return self._has_sudo

    async def start(self) -> bool:
        """Start the persistent powermetrics subprocess.
        Returns True if started successfully (sudo available)."""
        if os.geteuid() != 0:
            self._has_sudo = False
            return False

        self._has_sudo = True
        self._running = True

        try:
            self._process = await asyncio.create_subprocess_exec(
                "powermetrics",
                "--format",
                "plist",
                "--samplers",
                "tasks,smc,gpu_power",
                "-i",
                "2000",  # 2 second interval
                "--show-process-energy",
                "--show-process-gpu",
                "--show-process-coalition",
                "--show-process-netstats",
                "--show-process-io",
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.DEVNULL,
            )
            asyncio.create_task(self._read_stream())
            return True
        except Exception:
            self._has_sudo = False
            return False

    async def stop(self):
        """Stop the powermetrics subprocess."""
        self._running = False
        if self._process:
            try:
                self._process.terminate()
                await asyncio.wait_for(self._process.wait(), timeout=3)
            except (ProcessLookupError, asyncio.TimeoutError):
                try:
                    self._process.kill()
                except ProcessLookupError:
                    pass
            self._process = None

    async def _read_stream(self):
        """Read plist data from the powermetrics stream."""
        if not self._process or not self._process.stdout:
            return

        plist_buffer = b""
        in_plist = False

        while self._running:
            try:
                line = await asyncio.wait_for(self._process.stdout.readline(), timeout=5)
                if not line:
                    break

                line_str = line.strip()
                if line_str == b"<?xml version" or b"<?xml" in line_str:
                    plist_buffer = line
                    in_plist = True
                elif in_plist:
                    plist_buffer += line
                    if b"</plist>" in line_str:
                        in_plist = False
                        try:
                            self._parse_plist(plist_buffer)
                        except Exception:
                            pass
                        plist_buffer = b""
            except asyncio.TimeoutError:
                continue
            except Exception:
                break

    def _parse_plist(self, data: bytes):
        """Parse a complete plist blob from powermetrics."""
        import time

        try:
            plist = plistlib.loads(data)
        except Exception:
            return

        snapshot = EnergySnapshot(timestamp=time.time())

        # Parse SMC / thermal data
        if "processor" in plist:
            proc_info = plist["processor"]
            # Thermal pressure
            tp = proc_info.get("thermal_pressure", "")
            if tp:
                snapshot.thermal.thermal_pressure = tp

        if "smc" in plist:
            smc = plist["smc"]
            # Fan info
            fans = smc.get("fan", [])
            if fans:
                fan = fans[0] if isinstance(fans, list) else fans
                if isinstance(fan, dict):
                    snapshot.thermal.fan_speed_rpm = int(
                        fan.get("speed", fan.get("actual_speed", 0))
                    )
                    snapshot.thermal.fan_speed_max = int(fan.get("max_speed", 0))

            # Temperature — try common keys
            cpu_temp = smc.get("cpu_die_temp", smc.get("CPU die temperature", 0))
            if isinstance(cpu_temp, (int, float)):
                snapshot.thermal.cpu_die_temp = float(cpu_temp)

        # Parse GPU
        if "gpu" in plist:
            gpu = plist["gpu"]
            if isinstance(gpu, dict):
                snapshot.thermal.gpu_active_percent = float(
                    gpu.get("gpu_active_percent", gpu.get("active_percent", 0))
                )

        # Parse per-process tasks
        tasks = plist.get("tasks", [])
        if isinstance(tasks, list):
            for task in tasks:
                if not isinstance(task, dict):
                    continue
                pid = task.get("pid", 0)
                if pid == 0:
                    continue

                info = EnergyInfo(
                    pid=pid,
                    name=task.get("name", ""),
                    energy_impact=float(task.get("energy_impact", 0)),
                    cpu_ms_per_s=float(
                        task.get("cpu_time_ms_per_s", task.get("cputime_ms_per_s", 0))
                    ),
                    wakeups_per_s=float(
                        task.get("wakeups_per_s", task.get("interrupt_wakeups_per_s", 0))
                    ),
                    gpu_ms_per_s=float(task.get("gpu_ms_per_s", task.get("gputime_ms_per_s", 0))),
                    bytes_read_per_s=float(
                        task.get("bytes_read_per_s", task.get("diskio_bytesread_per_s", 0))
                    ),
                    bytes_written_per_s=float(
                        task.get("bytes_written_per_s", task.get("diskio_byteswritten_per_s", 0))
                    ),
                    packets_in_per_s=float(task.get("packets_in_per_s", 0)),
                    packets_out_per_s=float(task.get("packets_out_per_s", 0)),
                )
                snapshot.processes[pid] = info

        # Parse coalitions
        coalitions = plist.get("coalitions", [])
        if isinstance(coalitions, list):
            for coal in coalitions:
                if not isinstance(coal, dict):
                    continue
                coal_name = coal.get("name", "unknown")
                tasks_in_coal = coal.get("tasks", [])
                coal_entries = []
                if isinstance(tasks_in_coal, list):
                    for t in tasks_in_coal:
                        if isinstance(t, dict):
                            coal_entries.append(
                                EnergyInfo(
                                    pid=t.get("pid", 0),
                                    name=t.get("name", ""),
                                    energy_impact=float(t.get("energy_impact", 0)),
                                    coalition=coal_name,
                                )
                            )
                snapshot.coalitions[coal_name] = coal_entries

        self._latest = snapshot

    def get_energy_for_pid(self, pid: int) -> Optional[EnergyInfo]:
        """Get energy info for a specific PID."""
        return self._latest.processes.get(pid)

    def get_energy_label(self, pid: int) -> str:
        """Get a human-readable energy impact label."""
        info = self.get_energy_for_pid(pid)
        if not info:
            return ""
        ei = info.energy_impact
        if ei > 50:
            return "HIGH"
        elif ei > 20:
            return "MED"
        elif ei > 5:
            return "LOW"
        return ""
