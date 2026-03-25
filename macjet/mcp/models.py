"""
MacJet MCP — Pydantic response models for structured output.
Every tool returns one of these models so agents get typed JSON schemas.
"""

from __future__ import annotations

from pydantic import BaseModel, Field


# ── System Overview ──────────────────────────────────────────
class SystemOverview(BaseModel):
    """Concise snapshot of system health."""

    cpu_percent: float = Field(description="Overall CPU usage 0–100+")
    memory_used_gb: float = Field(description="RAM used in GB")
    memory_total_gb: float = Field(description="Total RAM in GB")
    memory_percent: float = Field(description="RAM usage percentage")
    thermal_pressure: str = Field(description="nominal | moderate | heavy | critical")
    fan_rpm: int | None = Field(default=None, description="Fan speed, None if unavailable")
    top_process: str = Field(description="Name of the highest-CPU process group")
    top_cpu_percent: float = Field(description="CPU% of the top process")
    process_count: int = Field(description="Total number of running processes")
    verdict: str = Field(description="Plain-English one-liner about system state")


# ── Process List ─────────────────────────────────────────────
class ProcessSummary(BaseModel):
    """Summary of one process group."""

    name: str
    pid_count: int = Field(description="Number of processes in this group")
    top_pid: int = Field(description="PID of the highest-CPU process in the group")
    total_cpu: float = Field(description="Sum of CPU% across all processes in group")
    total_memory_mb: float = Field(description="Sum of memory in MB")
    energy_impact: str = Field(default="", description="HIGH | MED | LOW | empty")
    context_label: str = Field(default="", description="Parsed app/script label")


class ProcessListResult(BaseModel):
    """Result from list_processes or search_processes."""

    groups: list[ProcessSummary]
    total_groups: int = Field(description="Total groups before limit was applied")
    sort_by: str
    filter_applied: str = ""


# ── Process Detail ───────────────────────────────────────────
class ChildProcess(BaseModel):
    """A child process within a group."""

    pid: int
    name: str
    cpu_percent: float
    memory_mb: float
    threads: int
    energy_impact: str = ""
    context_label: str = ""
    cmdline: str = Field(default="", description="Truncated command line")


class ProcessDetail(BaseModel):
    """Deep-dive into a specific process or group."""

    name: str
    total_cpu: float
    total_memory_mb: float
    energy_impact: str = ""
    process_count: int
    children: list[ChildProcess]
    chrome_tabs: list[ChromeTab] | None = Field(
        default=None, description="Chrome tabs if applicable"
    )
    why_hot: str = Field(default="", description="Plain-English explanation of resource usage")


# ── Chrome Tabs ──────────────────────────────────────────────
class ChromeTab(BaseModel):
    """A single Chrome tab mapped to a renderer PID."""

    rank: int
    title: str
    url: str
    domain: str = ""
    renderer_pid: int | None = None
    cpu_time_s: float = 0.0


class ChromeTabsResult(BaseModel):
    """Result from get_chrome_tabs."""

    tabs: list[ChromeTab]
    total_tabs: int
    cdp_connected: bool


# ── Heat Explanation ─────────────────────────────────────────
class HeatExplanation(BaseModel):
    """Structured diagnosis of why the machine is hot."""

    severity: str = Field(description="cool | warm | hot | critical")
    cpu_percent: float
    primary_culprit: str
    primary_cpu_percent: float
    secondary_culprits: list[str] = Field(default_factory=list)
    recommendations: list[str] = Field(default_factory=list)
    detailed_report: str = Field(description="Markdown-formatted full report")


# ── Kill / Suspend ───────────────────────────────────────────
class KillConfirmation(BaseModel):
    """Schema for elicitation-based kill confirmation."""

    confirm: bool = Field(default=False, description="Confirm killing this process?")


class KillResult(BaseModel):
    """Result from kill_process or force_kill_process."""

    action: str = Field(description="SIGTERM | SIGKILL | preview | declined | error")
    pid: int
    name: str
    success: bool
    error: str = ""
    audit_id: str | None = None


class SuspendResult(BaseModel):
    """Result from suspend_process or resume_process."""

    action: str = Field(description="SIGSTOP | SIGCONT | declined | error")
    pid: int
    name: str
    success: bool
    error: str = ""


# ── Energy Report ────────────────────────────────────────────
class EnergyEntry(BaseModel):
    """One app's energy impact."""

    name: str
    energy_impact: float
    category: str = Field(default="", description="HIGH | MED | LOW")


class EnergyReport(BaseModel):
    """Per-app energy breakdown from powermetrics."""

    available: bool = Field(description="Whether powermetrics data is available")
    entries: list[EnergyEntry] = Field(default_factory=list)
    cpu_power_w: float | None = None
    gpu_power_w: float | None = None


# ── Network Activity ─────────────────────────────────────────
class NetworkEntry(BaseModel):
    """One process group's network I/O."""

    name: str
    bytes_sent: int
    bytes_recv: int
    total_bytes: int


class NetworkReport(BaseModel):
    """Top processes by network bytes."""

    entries: list[NetworkEntry]
    system_bytes_sent: int = 0
    system_bytes_recv: int = 0
