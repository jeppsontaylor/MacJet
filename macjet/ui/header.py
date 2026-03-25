"""
MacJet — System Header Widget (Flight Deck Strip)
Compact 2-line branded header with CPU bar, memory, thermal, network.
"""

from __future__ import annotations

import platform
import subprocess

from textual.app import ComposeResult
from textual.reactive import reactive
from textual.widget import Widget
from textual.widgets import Static


def _get_machine_model() -> str:
    """Get the Mac model name from sysctl (cached)."""
    try:
        result = subprocess.run(
            ["sysctl", "-n", "hw.model"], capture_output=True, text=True, timeout=2
        )
        model = result.stdout.strip()
        if model:
            return model
    except Exception:
        pass
    return platform.machine()


# ─── Afterburner CPU Color Ramp ──────────────────────
_CPU_RAMP = [
    (5, "#22D3EE"),  # cyan
    (20, "#3B82F6"),  # blue
    (50, "#8B5CF6"),  # violet
    (80, "#D946EF"),  # magenta
    (999, "#FB7185"),  # hot pink
]


def _cpu_color(pct: float) -> str:
    """Get color from the Afterburner CPU ramp."""
    for threshold, color in _CPU_RAMP:
        if pct <= threshold:
            return color
    return _CPU_RAMP[-1][1]


class SystemHeader(Widget):
    """2-line system overview header with branded strip."""

    DEFAULT_CSS = """
    SystemHeader {
        height: 2;
        background: #10182B;
        border-bottom: solid #1A2540;
        padding: 0 1;
    }
    SystemHeader .header-line {
        height: 1;
        width: 1fr;
    }
    """

    cpu_percent = reactive(0.0)
    gpu_percent = reactive(0.0)
    mem_used = reactive(0.0)
    mem_total = reactive(0.0)
    fan_rpm = reactive(0)
    die_temp = reactive(0.0)
    thermal_pressure = reactive("nominal")
    net_down = reactive("")
    net_up = reactive("")
    self_cpu = reactive(0.0)
    paused = reactive(False)
    swap_used = reactive(0.0)

    _machine_model: str = ""

    def compose(self) -> ComposeResult:
        yield Static("", id="header-line-1", classes="header-line")
        yield Static("", id="header-line-2", classes="header-line")

    def on_mount(self):
        if not self._machine_model:
            self._machine_model = _get_machine_model()

    def _cpu_bar(self, pct: float, width: int = 10) -> str:
        """Half-block CPU bar with semantic coloring."""
        filled = int(pct / 100 * width)
        empty = width - filled
        color = _cpu_color(pct)
        bar = "█" * filled + "░" * empty
        return f"[{color}]{bar}[/]"

    def _thermal_dot(self) -> str:
        tp = self.thermal_pressure.lower()
        if tp in ("heavy", "critical", "sleeping"):
            return "[#FF4D6D]●[/]"
        elif tp in ("moderate", "elevated"):
            return "[#FF8A4C]●[/]"
        return "[#32D583]●[/]"

    def update_display(self):
        """Refresh the header display with current values."""
        icon = "⏸" if self.paused else "🔥"
        model = self._machine_model or "Mac"

        # Line 1: Brand + CPU + Memory + Swap
        cpu_bar = self._cpu_bar(self.cpu_percent)
        cpu_color = _cpu_color(self.cpu_percent)
        line1 = (
            f"  {icon} [bold #E6ECFF]MacJet[/]  "
            f"[#7F8DB3]•[/]  [#7F8DB3]{model}[/]  "
            f"[#7F8DB3]•[/]  CPU {cpu_bar} [{cpu_color}]{self.cpu_percent:5.1f}%[/]  "
            f"[#7F8DB3]•[/]  [#7F8DB3]Mem[/] [#E6ECFF]{self.mem_used:.1f}/{self.mem_total:.1f}GB[/]"
        )
        if self.swap_used > 0.01:
            line1 += f"  [#7F8DB3]•[/]  [#7F8DB3]Swap[/] [#FDBA35]{self.swap_used:.1f}GB[/]"

        # Line 2: Thermal + Temp + Fan + GPU + Network + Self
        if self.paused:
            line2 = "  [#FF8A4C]⏸  PAUSED[/] [#7F8DB3]— list frozen. Press [bold]Space[/bold] to resume.[/]"
        else:
            thermal_dot = self._thermal_dot()
            tp_label = self.thermal_pressure.capitalize()

            parts = [f"  Thermal: {thermal_dot} [#E6ECFF]{tp_label}[/]"]

            if self.die_temp > 0:
                temp_color = (
                    "#FF4D6D"
                    if self.die_temp > 90
                    else ("#FF8A4C" if self.die_temp > 70 else "#32D583")
                )
                parts.append(f"[{temp_color}]{self.die_temp:.0f}°C[/]")

            if self.fan_rpm > 0:
                parts.append(f"[#7F8DB3]Fan[/] [#E6ECFF]{self.fan_rpm}rpm[/]")

            if self.gpu_percent > 0:
                gpu_color = _cpu_color(self.gpu_percent)
                parts.append(f"[#7F8DB3]GPU[/] [{gpu_color}]{self.gpu_percent:.0f}%[/]")

            net_down = self.net_down or "—"
            net_up = self.net_up or "—"
            parts.append(f"[#7F8DB3]Net[/] [#45D6FF]↓{net_down}[/] [#A78BFA]↑{net_up}[/]")

            if self.self_cpu > 0.5:
                parts.append(f"[#7F8DB3]Self:[/] [#7F8DB3]{self.self_cpu:.1f}%[/]")

            line2 = "  [#7F8DB3]•[/]  ".join(parts)

        try:
            self.query_one("#header-line-1", Static).update(line1)
            self.query_one("#header-line-2", Static).update(line2)
        except Exception:
            pass
