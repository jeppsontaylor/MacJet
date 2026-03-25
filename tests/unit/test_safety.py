"""
Tests for the Safety module — PID validation, signal dispatch, and audit logging.
"""
from __future__ import annotations

import json
import os
import signal
import tempfile
from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest

from macjet.mcp.safety import validate_pid, resolve_pid, send_signal, get_audit_log, MIN_SAFE_PID


class TestValidatePid:
    """PID validation rules: reject invalid, system, and self PIDs."""

    def test_negative_pid_rejected(self):
        ok, err = validate_pid(-1)
        assert not ok
        assert "Invalid" in err

    def test_zero_pid_rejected(self):
        ok, err = validate_pid(0)
        assert not ok

    def test_system_pid_rejected(self):
        ok, err = validate_pid(1)
        assert not ok
        assert "system" in err.lower() or str(MIN_SAFE_PID) in err

    def test_boundary_pid_rejected(self):
        ok, err = validate_pid(499)
        assert not ok

    def test_self_pid_rejected(self):
        ok, err = validate_pid(os.getpid())
        assert not ok
        assert "self" in err.lower() or "MCP" in err

    @patch("macjet.mcp.safety.psutil")
    def test_nonexistent_pid_rejected(self, mock_psutil):
        mock_psutil.pid_exists.return_value = False
        ok, err = validate_pid(50000)
        assert not ok
        assert "does not exist" in err

    @patch("macjet.mcp.safety.psutil")
    def test_valid_pid_accepted(self, mock_psutil):
        mock_psutil.pid_exists.return_value = True
        ok, err = validate_pid(1000)
        assert ok
        assert err == ""


class TestResolvePid:
    """PID resolution for preview/confirmation."""

    @patch("macjet.mcp.safety.psutil.Process")
    def test_resolves_existing_process(self, MockProcess):
        proc = MockProcess.return_value
        proc.name.return_value = "TestApp"
        proc.cmdline.return_value = ["/usr/bin/testapp", "--flag"]
        proc.cpu_percent.return_value = 12.5
        proc.memory_info.return_value = MagicMock(rss=100 * 1024 * 1024)
        proc.username.return_value = "testuser"
        proc.status.return_value = "running"

        result = resolve_pid(1000)
        assert result["pid"] == 1000
        assert result["name"] == "TestApp"
        assert result["cpu_percent"] == 12.5

    @patch("macjet.mcp.safety.psutil.Process")
    def test_handles_no_such_process(self, MockProcess):
        import psutil
        MockProcess.side_effect = psutil.NoSuchProcess(9999)
        result = resolve_pid(9999)
        assert result["name"] == "unknown"
        assert "error" in result


class TestAuditLog:
    """Audit log reading."""

    def test_no_log_file_returns_message(self):
        with patch("macjet.mcp.safety.AUDIT_LOG_PATH", Path("/tmp/nonexistent_macjet_audit.jsonl")):
            result = get_audit_log()
            assert "No audit" in result

    def test_reads_existing_log(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
            entry = {"ts": "2025-01-01T00:00:00Z", "tool": "kill_process", "pid": 1000}
            f.write(json.dumps(entry) + "\n")
            f.flush()
            tmp_path = Path(f.name)

        try:
            with patch("macjet.mcp.safety.AUDIT_LOG_PATH", tmp_path):
                result = get_audit_log()
                assert "kill_process" in result
        finally:
            tmp_path.unlink()
