"""
Tests for the EnergyCollector — plist parsing and energy label generation.

These test the parser directly without requiring a live powermetrics subprocess.
"""
from __future__ import annotations

import plistlib

import pytest

from macjet.collectors.energy_collector import EnergyCollector, EnergySnapshot, EnergyInfo, ThermalInfo


def _make_plist(
    tasks: list[dict] | None = None,
    smc: dict | None = None,
    processor: dict | None = None,
    gpu: dict | None = None,
) -> bytes:
    """Build a well-formed plist blob for testing the parser."""
    data = {}
    if tasks is not None:
        data["tasks"] = tasks
    if smc is not None:
        data["smc"] = smc
    if processor is not None:
        data["processor"] = processor
    if gpu is not None:
        data["gpu"] = gpu
    return plistlib.dumps(data)


class TestPlistParsing:
    """Verify the EnergyCollector._parse_plist method."""

    def test_parses_tasks(self):
        ec = EnergyCollector()
        plist = _make_plist(tasks=[
            {"pid": 100, "name": "Chrome", "energy_impact": 42.5, "wakeups_per_s": 80.0},
            {"pid": 200, "name": "node", "energy_impact": 15.0},
        ])
        ec._parse_plist(plist)

        assert 100 in ec.latest.processes
        assert ec.latest.processes[100].energy_impact == 42.5
        assert ec.latest.processes[100].wakeups_per_s == 80.0
        assert 200 in ec.latest.processes

    def test_parses_thermal_pressure(self):
        ec = EnergyCollector()
        plist = _make_plist(processor={"thermal_pressure": "heavy"})
        ec._parse_plist(plist)
        assert ec.latest.thermal.thermal_pressure == "heavy"

    def test_parses_fan_speed(self):
        ec = EnergyCollector()
        plist = _make_plist(smc={"fan": [{"speed": 3200, "max_speed": 6000}]})
        ec._parse_plist(plist)
        assert ec.latest.thermal.fan_speed_rpm == 3200
        assert ec.latest.thermal.fan_speed_max == 6000

    def test_parses_cpu_temperature(self):
        ec = EnergyCollector()
        plist = _make_plist(smc={"cpu_die_temp": 78.5})
        ec._parse_plist(plist)
        assert ec.latest.thermal.cpu_die_temp == 78.5

    def test_parses_gpu_active_percent(self):
        ec = EnergyCollector()
        plist = _make_plist(gpu={"gpu_active_percent": 45.0})
        ec._parse_plist(plist)
        assert ec.latest.thermal.gpu_active_percent == 45.0

    def test_skips_pid_zero(self):
        ec = EnergyCollector()
        plist = _make_plist(tasks=[{"pid": 0, "name": "kernel_task"}])
        ec._parse_plist(plist)
        assert 0 not in ec.latest.processes

    def test_handles_empty_plist(self):
        ec = EnergyCollector()
        plist = _make_plist()
        ec._parse_plist(plist)
        assert len(ec.latest.processes) == 0

    def test_handles_malformed_data(self):
        ec = EnergyCollector()
        ec._parse_plist(b"not valid plist data")
        # Should not crash, latest should remain empty
        assert len(ec.latest.processes) == 0


class TestEnergyLabels:
    """Verify human-readable energy impact label generation."""

    def test_high_energy(self):
        ec = EnergyCollector()
        ec._latest = EnergySnapshot(processes={
            1: EnergyInfo(pid=1, name="Hot", energy_impact=60.0),
        })
        assert ec.get_energy_label(1) == "HIGH"

    def test_medium_energy(self):
        ec = EnergyCollector()
        ec._latest = EnergySnapshot(processes={
            1: EnergyInfo(pid=1, name="Warm", energy_impact=30.0),
        })
        assert ec.get_energy_label(1) == "MED"

    def test_low_energy(self):
        ec = EnergyCollector()
        ec._latest = EnergySnapshot(processes={
            1: EnergyInfo(pid=1, name="Cool", energy_impact=10.0),
        })
        assert ec.get_energy_label(1) == "LOW"

    def test_negligible_energy(self):
        ec = EnergyCollector()
        ec._latest = EnergySnapshot(processes={
            1: EnergyInfo(pid=1, name="Idle", energy_impact=2.0),
        })
        assert ec.get_energy_label(1) == ""

    def test_unknown_pid(self):
        ec = EnergyCollector()
        assert ec.get_energy_label(9999) == ""
