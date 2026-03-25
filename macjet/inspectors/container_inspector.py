"""
MacJet — Container Inspector
Docker / OrbStack / Colima container stats integration.
"""

from __future__ import annotations

import asyncio
import json
from dataclasses import dataclass
from typing import Optional


@dataclass
class ContainerInfo:
    name: str = ""
    container_id: str = ""
    image: str = ""
    cpu_percent: float = 0.0
    memory_mb: float = 0.0
    memory_limit_mb: float = 0.0
    net_input: str = ""
    net_output: str = ""
    status: str = ""


class ContainerInspector:
    """Inspects Docker/OrbStack containers for resource usage."""

    def __init__(self):
        self._docker_available: Optional[bool] = None
        self._containers: list[ContainerInfo] = []

    @property
    def containers(self) -> list[ContainerInfo]:
        return self._containers

    async def inspect(self) -> list[ContainerInfo]:
        """Query Docker/OrbStack for running container stats."""
        if self._docker_available is False:
            return []

        containers = await self._query_docker_stats()
        self._containers = containers
        return containers

    async def _query_docker_stats(self) -> list[ContainerInfo]:
        """Run docker stats --no-stream --format json."""
        try:
            proc = await asyncio.create_subprocess_exec(
                "docker",
                "stats",
                "--no-stream",
                "--format",
                '{"name":"{{.Name}}","id":"{{.ID}}","cpu":"{{.CPUPerc}}","mem_usage":"{{.MemUsage}}","net":"{{.NetIO}}","status":"running"}',
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.DEVNULL,
            )
            stdout, _ = await asyncio.wait_for(proc.communicate(), timeout=5.0)
        except (asyncio.TimeoutError, FileNotFoundError, OSError):
            self._docker_available = False
            return []

        if proc.returncode != 0 or not stdout:
            self._docker_available = False
            return []

        self._docker_available = True
        containers = []

        for line in stdout.decode("utf-8", errors="replace").strip().split("\n"):
            if not line:
                continue
            try:
                data = json.loads(line)
            except json.JSONDecodeError:
                continue

            cpu_str = data.get("cpu", "0%").rstrip("%")
            try:
                cpu = float(cpu_str)
            except ValueError:
                cpu = 0.0

            mem_usage = data.get("mem_usage", "0MiB / 0MiB")
            mem_parts = mem_usage.split(" / ")
            mem_used = self._parse_mem(mem_parts[0]) if mem_parts else 0

            net_io = data.get("net", "0B / 0B")
            net_parts = net_io.split(" / ")

            containers.append(
                ContainerInfo(
                    name=data.get("name", ""),
                    container_id=data.get("id", ""),
                    cpu_percent=cpu,
                    memory_mb=mem_used,
                    net_input=net_parts[0] if net_parts else "",
                    net_output=net_parts[1] if len(net_parts) > 1 else "",
                    status=data.get("status", ""),
                )
            )

        return containers

    @staticmethod
    def _parse_mem(s: str) -> float:
        """Parse memory string like '123.4MiB' to MB."""
        s = s.strip()
        try:
            if "GiB" in s:
                return float(s.replace("GiB", "").strip()) * 1024
            elif "MiB" in s:
                return float(s.replace("MiB", "").strip())
            elif "KiB" in s:
                return float(s.replace("KiB", "").strip()) / 1024
            elif "B" in s:
                return float(s.replace("B", "").strip()) / (1024 * 1024)
        except ValueError:
            pass
        return 0.0

    def find_container_for_process(self, process_name: str) -> Optional[ContainerInfo]:
        """Match a process to a running container by name."""
        for c in self._containers:
            if c.name.lower() in process_name.lower() or process_name.lower() in c.name.lower():
                return c
        return None
