"""
MacJet — Network Collector
Delta-sampling of per-process network I/O via psutil.
"""

from __future__ import annotations

import asyncio
import time
from dataclasses import dataclass

import psutil


@dataclass
class NetSnapshot:
    """System-wide network snapshot with deltas."""

    bytes_sent: int = 0
    bytes_recv: int = 0
    bytes_sent_per_s: float = 0.0
    bytes_recv_per_s: float = 0.0
    timestamp: float = 0.0


class NetworkCollector:
    """Tracks network I/O deltas at the system level."""

    def __init__(self):
        self._prev_sent = 0
        self._prev_recv = 0
        self._prev_time = 0.0
        self._latest = NetSnapshot()

    @property
    def latest(self) -> NetSnapshot:
        return self._latest

    async def collect(self) -> NetSnapshot:
        """Collect network stats and compute deltas."""
        loop = asyncio.get_event_loop()
        return await loop.run_in_executor(None, self._collect_sync)

    def _collect_sync(self) -> NetSnapshot:
        now = time.time()
        net = psutil.net_io_counters()

        dt = now - self._prev_time if self._prev_time > 0 else 1.0
        if dt <= 0:
            dt = 1.0

        snapshot = NetSnapshot(
            bytes_sent=net.bytes_sent,
            bytes_recv=net.bytes_recv,
            bytes_sent_per_s=(net.bytes_sent - self._prev_sent) / dt if self._prev_sent else 0,
            bytes_recv_per_s=(net.bytes_recv - self._prev_recv) / dt if self._prev_recv else 0,
            timestamp=now,
        )

        self._prev_sent = net.bytes_sent
        self._prev_recv = net.bytes_recv
        self._prev_time = now
        self._latest = snapshot
        return snapshot


def format_bytes_per_s(bps: float) -> str:
    """Format bytes/s into human-readable string."""
    if bps >= 1024 * 1024 * 1024:
        return f"{bps / (1024**3):.1f} GB/s"
    elif bps >= 1024 * 1024:
        return f"{bps / (1024**2):.1f} MB/s"
    elif bps >= 1024:
        return f"{bps / 1024:.1f} KB/s"
    return f"{bps:.0f} B/s"


def format_bytes(b: float) -> str:
    """Format bytes into human-readable string."""
    if b >= 1024 * 1024 * 1024:
        return f"{b / (1024**3):.1f} GB"
    elif b >= 1024 * 1024:
        return f"{b / (1024**2):.1f} MB"
    elif b >= 1024:
        return f"{b / 1024:.1f} KB"
    return f"{b:.0f} B"
