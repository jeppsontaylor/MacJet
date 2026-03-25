"""
MacJet — Metrics History & Reclaim Scoring Engine

Per-process ring buffers for sparklines, exponential smoothing for stable
sorting, and a multi-factor scoring engine for the Reclaim (Kill List) view.
"""

from __future__ import annotations

import time
from collections import deque
from dataclasses import dataclass

# ─── Sparkline Characters ────────────────────────────
SPARK_CHARS = "▁▂▃▄▅▆▇█"


# ─── Data Classes ────────────────────────────────────


@dataclass
class ProcessSample:
    """Single point-in-time sample for a process."""

    timestamp: float
    cpu_percent: float
    memory_mb: float


@dataclass
class ReclaimCandidate:
    """Scored recommendation for the Kill List."""

    group_key: str
    app_name: str
    icon: str
    score: int  # 0-100
    reclaim_cpu: float  # % CPU recovered
    reclaim_mem_mb: float  # MB RAM recovered
    risk: str  # "safe" | "review" | "danger"
    reason: str  # Human-readable explanation
    suggested_action: str  # "Quit App" | "Terminate" | "Force Kill" | "Pause"
    child_count: int = 0
    is_hidden: bool = False
    launch_age_s: float = 0.0


# ─── Ring Buffer Store ───────────────────────────────


class MetricsHistory:
    """Per-process ring buffer store with smoothing and scoring."""

    BUFFER_SIZE = 60  # 60 samples ≈ 60 seconds at 1s interval
    EXPIRY_S = 90.0  # Remove entries after 90s of no updates
    SMOOTH_ALPHA = 0.3  # Exponential smoothing factor

    def __init__(self):
        # PID → deque of ProcessSample
        self._buffers: dict[int, deque[ProcessSample]] = {}
        # PID → last smoothed values
        self._smoothed_cpu: dict[int, float] = {}
        self._smoothed_mem: dict[int, float] = {}
        # PID → last update timestamp
        self._last_seen: dict[int, float] = {}

    def record(self, pid: int, cpu_percent: float, memory_mb: float):
        """Record a new sample for a process."""
        now = time.time()
        if pid not in self._buffers:
            self._buffers[pid] = deque(maxlen=self.BUFFER_SIZE)
            self._smoothed_cpu[pid] = cpu_percent
            self._smoothed_mem[pid] = memory_mb

        self._buffers[pid].append(
            ProcessSample(
                timestamp=now,
                cpu_percent=cpu_percent,
                memory_mb=memory_mb,
            )
        )
        self._last_seen[pid] = now

        # Exponential smoothing
        α = self.SMOOTH_ALPHA
        self._smoothed_cpu[pid] = α * cpu_percent + (1 - α) * self._smoothed_cpu.get(
            pid, cpu_percent
        )
        self._smoothed_mem[pid] = α * memory_mb + (1 - α) * self._smoothed_mem.get(pid, memory_mb)

    def smoothed_cpu(self, pid: int) -> float:
        """Get the exponentially smoothed CPU for a process."""
        return self._smoothed_cpu.get(pid, 0.0)

    def smoothed_mem(self, pid: int) -> float:
        """Get the exponentially smoothed memory for a process."""
        return self._smoothed_mem.get(pid, 0.0)

    def sustained_cpu(self, pid: int, window_s: float = 30.0) -> float:
        """Average CPU over the last `window_s` seconds."""
        buf = self._buffers.get(pid)
        if not buf:
            return 0.0
        now = time.time()
        cutoff = now - window_s
        samples = [s.cpu_percent for s in buf if s.timestamp >= cutoff]
        return sum(samples) / len(samples) if samples else 0.0

    def memory_growth_rate(self, pid: int, window_s: float = 300.0) -> float:
        """Memory growth rate in MB/min over the last `window_s` seconds."""
        buf = self._buffers.get(pid)
        if not buf or len(buf) < 2:
            return 0.0
        now = time.time()
        cutoff = now - window_s
        relevant = [s for s in buf if s.timestamp >= cutoff]
        if len(relevant) < 2:
            return 0.0
        oldest = relevant[0]
        newest = relevant[-1]
        dt = newest.timestamp - oldest.timestamp
        if dt < 5.0:  # Need at least 5s of data
            return 0.0
        dm = newest.memory_mb - oldest.memory_mb
        return (dm / dt) * 60.0  # MB per minute

    def sparkline(self, pid: int, width: int = 20, metric: str = "cpu") -> str:
        """Generate a braille sparkline string for the given PID.

        Args:
            pid: Process ID
            width: Number of characters in the sparkline
            metric: "cpu" or "mem"
        """
        buf = self._buffers.get(pid)
        if not buf:
            return " " * width

        if metric == "cpu":
            values = [s.cpu_percent for s in buf]
            max_val = max(max(values), 1.0)  # Avoid div by zero
        else:
            values = [s.memory_mb for s in buf]
            max_val = max(max(values), 1.0)

        # Resample to fit width
        if len(values) > width:
            step = len(values) / width
            resampled = []
            for i in range(width):
                idx = int(i * step)
                resampled.append(values[min(idx, len(values) - 1)])
            values = resampled
        elif len(values) < width:
            # Pad with zeros on the left
            values = [0.0] * (width - len(values)) + values

        chars = []
        for v in values:
            normalized = v / max_val
            idx = int(normalized * (len(SPARK_CHARS) - 1))
            idx = max(0, min(idx, len(SPARK_CHARS) - 1))
            chars.append(SPARK_CHARS[idx])

        return "".join(chars)

    def sparkline_for_group(self, pids: list[int], width: int = 20) -> str:
        """Generate a combined sparkline for a group of PIDs (summed CPU)."""
        if not pids:
            return " " * width

        # Find the max buffer length across all PIDs
        max_len = 0
        for pid in pids:
            buf = self._buffers.get(pid)
            if buf:
                max_len = max(max_len, len(buf))

        if max_len == 0:
            return " " * width

        # Sum CPU across all PIDs for each time slot
        combined = [0.0] * max_len
        for pid in pids:
            buf = self._buffers.get(pid)
            if not buf:
                continue
            offset = max_len - len(buf)
            for i, s in enumerate(buf):
                combined[offset + i] += s.cpu_percent

        max_val = max(max(combined), 1.0)

        # Resample to fit width
        if len(combined) > width:
            step = len(combined) / width
            resampled = []
            for i in range(width):
                idx = int(i * step)
                resampled.append(combined[min(idx, len(combined) - 1)])
            combined = resampled
        elif len(combined) < width:
            combined = [0.0] * (width - len(combined)) + combined

        chars = []
        for v in combined:
            normalized = min(v / max_val, 1.0)
            idx = int(normalized * (len(SPARK_CHARS) - 1))
            idx = max(0, min(idx, len(SPARK_CHARS) - 1))
            chars.append(SPARK_CHARS[idx])

        return "".join(chars)

    def expire_stale(self):
        """Remove entries for processes not seen recently."""
        now = time.time()
        stale_pids = [pid for pid, ts in self._last_seen.items() if now - ts > self.EXPIRY_S]
        for pid in stale_pids:
            self._buffers.pop(pid, None)
            self._smoothed_cpu.pop(pid, None)
            self._smoothed_mem.pop(pid, None)
            self._last_seen.pop(pid, None)

    # ─── Reclaim Scoring ─────────────────────────────

    def compute_reclaim_score(
        self,
        group_key: str,
        app_name: str,
        icon: str,
        pids: list[int],
        total_cpu: float,
        total_memory_mb: float,
        child_count: int,
        is_hidden: bool = False,
        is_system: bool = False,
        has_high_wakeups: bool = False,
        energy_impact: str = "",
        launch_age_s: float = 0.0,
    ) -> ReclaimCandidate:
        """Compute a reclaim score for a process group."""
        score = 0

        # Factor 1: Sustained CPU (up to 30 points)
        avg_sustained = 0.0
        for pid in pids:
            avg_sustained += self.sustained_cpu(pid, window_s=30.0)
        score += min(30, int(avg_sustained * 0.3))

        # Factor 2: Memory footprint (up to 25 points)
        score += min(25, int(total_memory_mb / 100))

        # Factor 3: Memory growth rate (up to 15 points)
        total_growth = 0.0
        for pid in pids:
            total_growth += max(0, self.memory_growth_rate(pid))
        score += min(15, int(total_growth * 3))

        # Factor 4: Hidden/background (15 points)
        if is_hidden:
            score += 15

        # Factor 5: Process storm (10 points)
        if child_count > 10:
            score += 10

        # Factor 6: High wakeups / energy (5 points)
        if has_high_wakeups or energy_impact == "HIGH":
            score += 5

        score = min(100, score)

        # Determine risk level
        if is_system:
            risk = "danger"
        elif not is_hidden and total_cpu > 5:
            risk = "review"
        else:
            risk = "safe"

        # Generate reason
        reason = self._generate_reason(
            pids,
            total_cpu,
            total_memory_mb,
            child_count,
            is_hidden,
            has_high_wakeups,
            total_growth,
            launch_age_s,
        )

        # Suggest action
        if risk == "danger":
            suggested_action = "⚠ Review First"
        elif is_hidden and total_cpu < 1:
            suggested_action = "Quit App"
        elif child_count > 10:
            suggested_action = "Quit App"
        else:
            suggested_action = "Terminate"

        return ReclaimCandidate(
            group_key=group_key,
            app_name=app_name,
            icon=icon,
            score=score,
            reclaim_cpu=total_cpu,
            reclaim_mem_mb=total_memory_mb,
            risk=risk,
            reason=reason,
            suggested_action=suggested_action,
            child_count=child_count,
            is_hidden=is_hidden,
            launch_age_s=launch_age_s,
        )

    def _generate_reason(
        self,
        pids: list[int],
        total_cpu: float,
        total_memory_mb: float,
        child_count: int,
        is_hidden: bool,
        has_high_wakeups: bool,
        growth_rate: float,
        launch_age_s: float,
    ) -> str:
        """Generate a human-readable reason string."""
        parts = []

        if is_hidden and launch_age_s > 300:
            age_str = self._format_duration(launch_age_s)
            parts.append(f"Hidden {age_str}")

        if total_memory_mb > 500:
            mem_str = (
                f"{total_memory_mb / 1024:.1f}G"
                if total_memory_mb >= 1024
                else f"{total_memory_mb:.0f}M"
            )
            parts.append(f"{mem_str} resident")

        if growth_rate > 10:
            parts.append(f"+{growth_rate:.0f}MB/min growth")

        if child_count > 10:
            parts.append(f"{child_count} child processes")

        if has_high_wakeups:
            parts.append("high wakeups")

        if total_cpu > 50:
            parts.append(f"sustained {total_cpu:.0f}% CPU")
        elif total_cpu < 1 and total_memory_mb > 200:
            parts.append(f"{total_cpu:.1f}% CPU")

        if not parts:
            parts.append(f"{total_cpu:.1f}% CPU, {total_memory_mb:.0f}MB")

        return ", ".join(parts)

    @staticmethod
    def _format_duration(seconds: float) -> str:
        """Format seconds into a human-readable duration."""
        if seconds < 60:
            return f"{int(seconds)}s"
        elif seconds < 3600:
            return f"{int(seconds / 60)}m"
        else:
            h = int(seconds / 3600)
            m = int((seconds % 3600) / 60)
            return f"{h}h{m}m" if m else f"{h}h"
