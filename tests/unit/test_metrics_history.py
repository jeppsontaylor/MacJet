"""
Tests for MetricsHistory — ring buffers, EMA smoothing, sparklines, and expiry.
"""
from __future__ import annotations

import time
from unittest.mock import patch

from macjet.collectors.metrics_history import MetricsHistory, SPARK_CHARS


class TestRecordAndSmoothing:
    """Verify that recording samples updates smoothed values correctly."""

    def test_first_sample_initializes_smoothed_values(self, metrics, clock):
        metrics.record(1, cpu_percent=50.0, memory_mb=200.0)
        # First sample: smoothed = alpha * value + (1 - alpha) * value = value
        assert metrics.smoothed_cpu(1) == 50.0
        assert metrics.smoothed_mem(1) == 200.0

    def test_smoothing_blends_new_and_old(self, metrics, clock):
        metrics.record(1, cpu_percent=100.0, memory_mb=100.0)
        clock.advance(1)
        metrics.record(1, cpu_percent=0.0, memory_mb=0.0)
        # After second sample: smoothed = 0.3 * 0 + 0.7 * 100 = 70
        assert abs(metrics.smoothed_cpu(1) - 70.0) < 0.01
        assert abs(metrics.smoothed_mem(1) - 70.0) < 0.01

    def test_smoothing_converges_over_many_samples(self, metrics, clock):
        # Feed constant 50% CPU for many samples
        for _ in range(20):
            metrics.record(1, cpu_percent=50.0, memory_mb=100.0)
            clock.advance(1)
        assert abs(metrics.smoothed_cpu(1) - 50.0) < 0.1

    def test_unknown_pid_returns_zero(self, metrics):
        assert metrics.smoothed_cpu(9999) == 0.0
        assert metrics.smoothed_mem(9999) == 0.0


class TestRingBuffer:
    """Verify ring buffer behavior: capacity, FIFO eviction."""

    def test_buffer_respects_max_size(self, metrics, clock):
        for i in range(100):
            metrics.record(1, cpu_percent=float(i), memory_mb=0.0)
            clock.advance(1)
        # Buffer should be capped at BUFFER_SIZE (60)
        assert len(metrics._buffers[1]) == MetricsHistory.BUFFER_SIZE

    def test_oldest_samples_are_evicted(self, metrics, clock):
        for i in range(70):
            metrics.record(1, cpu_percent=float(i), memory_mb=0.0)
            clock.advance(1)
        # First sample should be i=10 (70 - 60 = 10)
        assert metrics._buffers[1][0].cpu_percent == 10.0


class TestSustainedCPU:
    """Verify window-based sustained CPU averaging."""

    def test_sustained_cpu_averages_within_window(self, metrics, clock):
        for i in range(10):
            metrics.record(1, cpu_percent=10.0, memory_mb=0.0)
            clock.advance(1)
        assert abs(metrics.sustained_cpu(1, window_s=30.0) - 10.0) < 0.01

    def test_sustained_cpu_ignores_old_samples(self, metrics, clock):
        # Record old samples
        for _ in range(5):
            metrics.record(1, cpu_percent=100.0, memory_mb=0.0)
            clock.advance(1)
        # Advance past the window
        clock.advance(40)
        # Record new samples
        for _ in range(5):
            metrics.record(1, cpu_percent=10.0, memory_mb=0.0)
            clock.advance(1)
        assert abs(metrics.sustained_cpu(1, window_s=30.0) - 10.0) < 0.01

    def test_sustained_cpu_unknown_pid(self, metrics):
        assert metrics.sustained_cpu(9999) == 0.0


class TestMemoryGrowthRate:
    """Verify memory leak detection via growth rate calculation."""

    def test_positive_growth(self, metrics, clock):
        # Memory grows from 100MB to 200MB over 60 seconds
        for i in range(60):
            mem = 100.0 + (100.0 * i / 59.0)
            metrics.record(1, cpu_percent=0, memory_mb=mem)
            clock.advance(1)
        rate = metrics.memory_growth_rate(1, window_s=300.0)
        # 100MB over 60s = ~100 MB/min
        assert rate > 90.0

    def test_stable_memory_returns_near_zero(self, metrics, clock):
        for _ in range(30):
            metrics.record(1, cpu_percent=0, memory_mb=500.0)
            clock.advance(1)
        rate = metrics.memory_growth_rate(1, window_s=300.0)
        assert abs(rate) < 0.01

    def test_insufficient_data_returns_zero(self, metrics, clock):
        metrics.record(1, cpu_percent=0, memory_mb=100.0)
        assert metrics.memory_growth_rate(1) == 0.0


class TestSparkline:
    """Verify sparkline string generation."""

    def test_empty_pid_returns_spaces(self, metrics):
        result = metrics.sparkline(9999, width=10)
        assert result == " " * 10

    def test_sparkline_length_matches_width(self, metrics, clock):
        for i in range(30):
            metrics.record(1, cpu_percent=float(i), memory_mb=0.0)
            clock.advance(1)
        for width in [5, 10, 20, 40]:
            assert len(metrics.sparkline(1, width=width)) == width

    def test_sparkline_uses_valid_characters(self, metrics, clock):
        for i in range(20):
            metrics.record(1, cpu_percent=float(i * 5), memory_mb=0.0)
            clock.advance(1)
        result = metrics.sparkline(1, width=20)
        for ch in result:
            assert ch in SPARK_CHARS

    def test_constant_values_produce_uniform_sparkline(self, metrics, clock):
        for _ in range(20):
            metrics.record(1, cpu_percent=50.0, memory_mb=0.0)
            clock.advance(1)
        result = metrics.sparkline(1, width=20)
        # All characters should be the highest bar since all values are equal
        assert len(set(result)) == 1
        assert result[0] == SPARK_CHARS[-1]


class TestExpiry:
    """Verify stale process entry cleanup."""

    def test_recent_entries_are_kept(self, metrics, clock):
        metrics.record(1, cpu_percent=10.0, memory_mb=100.0)
        clock.advance(10)
        metrics.expire_stale()
        assert 1 in metrics._buffers

    def test_stale_entries_are_removed(self, metrics, clock):
        metrics.record(1, cpu_percent=10.0, memory_mb=100.0)
        clock.advance(MetricsHistory.EXPIRY_S + 1)
        metrics.expire_stale()
        assert 1 not in metrics._buffers
        assert 1 not in metrics._smoothed_cpu
        assert 1 not in metrics._smoothed_mem

    def test_mixed_fresh_and_stale(self, metrics, clock):
        metrics.record(1, cpu_percent=10.0, memory_mb=100.0)
        clock.advance(MetricsHistory.EXPIRY_S + 1)
        metrics.record(2, cpu_percent=20.0, memory_mb=200.0)
        metrics.expire_stale()
        assert 1 not in metrics._buffers
        assert 2 in metrics._buffers


class TestFormatDuration:
    """Verify human-readable duration formatting."""

    def test_seconds(self):
        assert MetricsHistory._format_duration(30) == "30s"

    def test_minutes(self):
        assert MetricsHistory._format_duration(120) == "2m"

    def test_hours_with_minutes(self):
        assert MetricsHistory._format_duration(3720) == "1h2m"

    def test_hours_exact(self):
        assert MetricsHistory._format_duration(7200) == "2h"
