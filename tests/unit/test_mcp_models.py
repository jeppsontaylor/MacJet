import pytest
from pydantic import ValidationError

from macjet.mcp.models import (
    SystemOverview,
    ProcessSummary,
    ProcessListResult,
    ChildProcess,
    ProcessDetail,
    ChromeTab,
    ChromeTabsResult,
    HeatExplanation,
    KillConfirmation,
    KillResult,
    SuspendResult,
    EnergyEntry,
    EnergyReport,
    NetworkEntry,
    NetworkReport,
)


def test_system_overview():
    model = SystemOverview(
        cpu_percent=45.5,
        memory_used_gb=12.0,
        memory_total_gb=16.0,
        memory_percent=75.0,
        thermal_pressure="nominal",
        fan_rpm=2000,
        top_process="Google Chrome",
        top_cpu_percent=15.2,
        process_count=350,
        verdict="System is running smoothly.",
    )
    assert model.cpu_percent == 45.5
    assert model.fan_rpm == 2000

    # Test without optional fan_rpm
    model_no_fan = SystemOverview(
        cpu_percent=45.5,
        memory_used_gb=12.0,
        memory_total_gb=16.0,
        memory_percent=75.0,
        thermal_pressure="nominal",
        top_process="Google Chrome",
        top_cpu_percent=15.2,
        process_count=350,
        verdict="System is running smoothly.",
    )
    assert model_no_fan.fan_rpm is None


def test_process_summary():
    model = ProcessSummary(
        name="Docker",
        pid_count=5,
        top_pid=1234,
        total_cpu=10.5,
        total_memory_mb=1024.5,
        energy_impact="HIGH",
        context_label="Container Runtime",
    )
    assert model.name == "Docker"
    assert model.energy_impact == "HIGH"


def test_process_list_result():
    summary = ProcessSummary(
        name="Docker",
        pid_count=5,
        top_pid=1234,
        total_cpu=10.5,
        total_memory_mb=1024.5,
    )
    model = ProcessListResult(
        groups=[summary],
        total_groups=50,
        sort_by="cpu",
        filter_applied="dock",
    )
    assert len(model.groups) == 1
    assert model.total_groups == 50


def test_child_process():
    model = ChildProcess(
        pid=999,
        name="Helper",
        cpu_percent=1.2,
        memory_mb=45.0,
        threads=8,
        cmdline="--type=renderer",
    )
    assert model.pid == 999
    assert model.energy_impact == ""  # Default


def test_process_detail():
    child = ChildProcess(
        pid=999, name="Helper", cpu_percent=1.2, memory_mb=45.0, threads=8
    )
    tab = ChromeTab(rank=1, title="GitHub", url="https://github.com", renderer_pid=999)
    model = ProcessDetail(
        name="Chrome",
        total_cpu=50.0,
        total_memory_mb=2048.0,
        process_count=10,
        children=[child],
        chrome_tabs=[tab],
        why_hot="Heavy rendering",
    )
    assert model.name == "Chrome"
    assert len(model.children) == 1
    assert model.chrome_tabs[0].title == "GitHub"


def test_chrome_tab():
    model = ChromeTab(
        rank=1,
        title="Google",
        url="https://google.com",
        domain="google.com",
        renderer_pid=1234,
        cpu_time_s=15.5,
    )
    assert model.renderer_pid == 1234
    assert model.cpu_time_s == 15.5


def test_chrome_tabs_result():
    tab = ChromeTab(rank=1, title="Google", url="https://google.com")
    model = ChromeTabsResult(
        tabs=[tab],
        total_tabs=1,
        cdp_connected=True,
    )
    assert model.cdp_connected is True
    assert len(model.tabs) == 1


def test_heat_explanation():
    model = HeatExplanation(
        severity="hot",
        cpu_percent=95.0,
        primary_culprit="Docker",
        primary_cpu_percent=80.0,
        secondary_culprits=["Chrome"],
        recommendations=["Kill Docker"],
        detailed_report="# Report",
    )
    assert model.severity == "hot"
    assert len(model.secondary_culprits) == 1


def test_kill_confirmation():
    model = KillConfirmation(confirm=True)
    assert model.confirm is True


def test_kill_result():
    model = KillResult(
        action="SIGTERM",
        pid=1234,
        name="BadApp",
        success=True,
    )
    assert model.success is True
    assert model.error == ""


def test_suspend_result():
    model = SuspendResult(
        action="SIGSTOP",
        pid=1234,
        name="BusyApp",
        success=False,
        error="Permission denied",
    )
    assert model.success is False
    assert model.error == "Permission denied"


def test_energy_report():
    entry = EnergyEntry(name="HeavyApp", energy_impact=500.5, category="HIGH")
    model = EnergyReport(
        available=True,
        entries=[entry],
        cpu_power_w=15.5,
        gpu_power_w=2.0,
    )
    assert model.available is True
    assert len(model.entries) == 1
    assert model.cpu_power_w == 15.5


def test_network_report():
    entry = NetworkEntry(
        name="Downloader", bytes_sent=100, bytes_recv=5000, total_bytes=5100
    )
    model = NetworkReport(
        entries=[entry],
        system_bytes_sent=1000,
        system_bytes_recv=10000,
    )
    assert len(model.entries) == 1
    assert model.system_bytes_sent == 1000
