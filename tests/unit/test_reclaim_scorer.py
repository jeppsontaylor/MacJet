"""
Tests for the Reclaim Scoring Engine — multi-factor process scoring logic.
"""
from __future__ import annotations

from macjet.collectors.metrics_history import MetricsHistory, ReclaimCandidate


class TestReclaimScoring:
    """Validate the scoring algorithm produces correct scores and risk levels."""

    def test_idle_hidden_process_scores_for_background(self, metrics, clock):
        """A hidden, idle process should score for the background factor (15 pts)."""
        result = metrics.compute_reclaim_score(
            group_key="TestApp",
            app_name="TestApp",
            icon="",
            pids=[1],
            total_cpu=0.1,
            total_memory_mb=50.0,
            child_count=1,
            is_hidden=True,
        )
        assert result.score >= 15
        assert result.risk == "safe"

    def test_high_memory_process_scores_for_memory(self, metrics, clock):
        """A process using 2.5GB should get ~25 points for memory alone."""
        result = metrics.compute_reclaim_score(
            group_key="BigApp",
            app_name="BigApp",
            icon="",
            pids=[1],
            total_cpu=0.0,
            total_memory_mb=2500.0,
            child_count=1,
        )
        assert result.score >= 25

    def test_process_storm_adds_10_points(self, metrics, clock):
        """A process with >10 children should get 10 bonus points."""
        result = metrics.compute_reclaim_score(
            group_key="StormApp",
            app_name="StormApp",
            icon="",
            pids=[1],
            total_cpu=0.0,
            total_memory_mb=100.0,
            child_count=15,
        )
        result_no_storm = metrics.compute_reclaim_score(
            group_key="CalmApp",
            app_name="CalmApp",
            icon="",
            pids=[1],
            total_cpu=0.0,
            total_memory_mb=100.0,
            child_count=3,
        )
        assert result.score >= result_no_storm.score + 10

    def test_high_wakeups_adds_5_points(self, metrics, clock):
        """High wakeups should add 5 points."""
        base = metrics.compute_reclaim_score(
            group_key="A", app_name="A", icon="", pids=[1],
            total_cpu=0.0, total_memory_mb=100.0, child_count=1,
        )
        with_wakeups = metrics.compute_reclaim_score(
            group_key="A", app_name="A", icon="", pids=[1],
            total_cpu=0.0, total_memory_mb=100.0, child_count=1,
            has_high_wakeups=True,
        )
        assert with_wakeups.score == base.score + 5

    def test_system_process_gets_danger_risk(self, metrics, clock):
        """System processes should always be flagged as danger."""
        result = metrics.compute_reclaim_score(
            group_key="kernel_task",
            app_name="kernel_task",
            icon="",
            pids=[1],
            total_cpu=5.0,
            total_memory_mb=1000.0,
            child_count=1,
            is_system=True,
        )
        assert result.risk == "danger"
        assert result.suggested_action == "Review First" or "Review" in result.suggested_action

    def test_visible_high_cpu_gets_review_risk(self, metrics, clock):
        """A visible process with >5% CPU should be flagged for review."""
        result = metrics.compute_reclaim_score(
            group_key="ActiveApp",
            app_name="ActiveApp",
            icon="",
            pids=[1],
            total_cpu=30.0,
            total_memory_mb=200.0,
            child_count=1,
            is_hidden=False,
        )
        assert result.risk == "review"

    def test_score_never_exceeds_100(self, metrics, clock):
        """Score must be capped at 100 regardless of how many factors fire."""
        # Feed sustained CPU data
        for _ in range(30):
            metrics.record(1, cpu_percent=100.0, memory_mb=5000.0)
            clock.advance(1)

        result = metrics.compute_reclaim_score(
            group_key="Monster",
            app_name="Monster",
            icon="",
            pids=[1],
            total_cpu=100.0,
            total_memory_mb=5000.0,
            child_count=50,
            is_hidden=True,
            has_high_wakeups=True,
        )
        assert result.score <= 100

    def test_result_is_reclaim_candidate(self, metrics, clock):
        """The return type should be a ReclaimCandidate dataclass."""
        result = metrics.compute_reclaim_score(
            group_key="App",
            app_name="App",
            icon="icon",
            pids=[1],
            total_cpu=10.0,
            total_memory_mb=200.0,
            child_count=2,
        )
        assert isinstance(result, ReclaimCandidate)
        assert result.group_key == "App"
        assert result.reclaim_cpu == 10.0
        assert result.reclaim_mem_mb == 200.0


class TestReasonGeneration:
    """Verify the human-readable reason string generation."""

    def test_hidden_process_mentions_hidden(self, metrics, clock):
        result = metrics.compute_reclaim_score(
            group_key="Hidden",
            app_name="Hidden",
            icon="",
            pids=[1],
            total_cpu=0.1,
            total_memory_mb=50.0,
            child_count=1,
            is_hidden=True,
            launch_age_s=600.0,
        )
        assert "Hidden" in result.reason or "hidden" in result.reason.lower()

    def test_high_memory_mentions_resident(self, metrics, clock):
        result = metrics.compute_reclaim_score(
            group_key="BigMem",
            app_name="BigMem",
            icon="",
            pids=[1],
            total_cpu=1.0,
            total_memory_mb=2048.0,
            child_count=1,
        )
        assert "G" in result.reason or "resident" in result.reason

    def test_process_storm_mentions_child_count(self, metrics, clock):
        result = metrics.compute_reclaim_score(
            group_key="Storm",
            app_name="Storm",
            icon="",
            pids=[1],
            total_cpu=1.0,
            total_memory_mb=100.0,
            child_count=25,
        )
        assert "25" in result.reason or "child" in result.reason
