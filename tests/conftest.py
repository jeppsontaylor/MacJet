"""
MacJet Test Suite — Shared Fixtures and Mocks

Provides mock objects for macOS-specific APIs (psutil, powermetrics) so the
test suite can run on any platform and in standard CI environments.
"""

from __future__ import annotations

import asyncio
import time
from unittest.mock import patch

import pytest

from macjet.collectors.energy_collector import (
    EnergyInfo,
    EnergySnapshot,
    ThermalInfo,
)
from macjet.collectors.metrics_history import MetricsHistory
from macjet.collectors.process_collector import ProcessInfo

# ── Time Control ─────────────────────────────────────────────


class FakeClock:
    """Controllable clock for deterministic testing of time-dependent logic."""

    def __init__(self, start: float = 1000000.0):
        self._now = start

    def time(self) -> float:
        return self._now

    def advance(self, seconds: float):
        self._now += seconds


@pytest.fixture
def clock():
    """Provide a FakeClock and patch time.time() to use it."""
    fake = FakeClock()
    with patch("macjet.collectors.metrics_history.time.time", side_effect=lambda: fake.time()):
        yield fake


# ── MetricsHistory ───────────────────────────────────────────


@pytest.fixture
def metrics(clock):
    """Provide a clean MetricsHistory instance with a controlled clock."""
    return MetricsHistory()


# ── Process Fixtures ─────────────────────────────────────────


def make_process_info(
    pid: int = 1000,
    name: str = "TestApp",
    cpu_percent: float = 10.0,
    memory_mb: float = 200.0,
    cmdline: list[str] | None = None,
    username: str = "testuser",
    exe: str = "/usr/local/bin/testapp",
    ppid: int = 1,
    is_hidden: bool = False,
    is_system: bool = False,
    create_time: float = 0.0,
) -> ProcessInfo:
    """Factory for creating ProcessInfo instances in tests."""
    return ProcessInfo(
        pid=pid,
        name=name,
        cpu_percent=cpu_percent,
        memory_mb=memory_mb,
        cmdline=cmdline or [],
        username=username,
        exe=exe,
        ppid=ppid,
        is_hidden=is_hidden,
        is_system=is_system,
        create_time=create_time or time.time(),
    )


@pytest.fixture
def sample_processes() -> list[ProcessInfo]:
    """A realistic set of process snapshots for testing grouping and scoring."""
    return [
        make_process_info(pid=100, name="Google Chrome", cpu_percent=45.0, memory_mb=800.0),
        make_process_info(
            pid=101,
            name="Google Chrome Helper (renderer)",
            cpu_percent=20.0,
            memory_mb=350.0,
            cmdline=["--type=renderer"],
            ppid=100,
        ),
        make_process_info(
            pid=102,
            name="Google Chrome Helper (gpu-process)",
            cpu_percent=15.0,
            memory_mb=120.0,
            cmdline=["--type=gpu-process"],
            ppid=100,
        ),
        make_process_info(
            pid=200, name="node", cpu_percent=5.0, memory_mb=150.0, cmdline=["node", "server.js"]
        ),
        make_process_info(
            pid=300,
            name="python3",
            cpu_percent=80.0,
            memory_mb=500.0,
            cmdline=["python3", "train.py"],
            is_hidden=True,
        ),
        make_process_info(pid=400, name="Finder", cpu_percent=0.5, memory_mb=60.0),
        make_process_info(
            pid=500,
            name="kernel_task",
            cpu_percent=2.0,
            memory_mb=1200.0,
            username="root",
            exe="/usr/libexec/kernel_task",
            is_system=True,
        ),
    ]


# ── Energy Fixtures ──────────────────────────────────────────


@pytest.fixture
def mock_energy_snapshot() -> EnergySnapshot:
    """A mock EnergySnapshot with realistic thermal and process data."""
    return EnergySnapshot(
        processes={
            100: EnergyInfo(pid=100, name="Google Chrome", energy_impact=65.0, wakeups_per_s=150.0),
            300: EnergyInfo(pid=300, name="python3", energy_impact=35.0, cpu_ms_per_s=400.0),
        },
        thermal=ThermalInfo(
            cpu_die_temp=78.5,
            fan_speed_rpm=3200,
            fan_speed_max=6000,
            thermal_pressure="moderate",
            gpu_active_percent=45.0,
        ),
        timestamp=time.time(),
    )


# ── Async Helpers ────────────────────────────────────────────


@pytest.fixture
def event_loop():
    """Provide a fresh event loop for async tests."""
    loop = asyncio.new_event_loop()
    yield loop
    loop.close()
