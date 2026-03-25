"""
Tests for the Safety module — PID validation, signal dispatch, and audit logging.
"""

from __future__ import annotations

import json
import os
import signal
import tempfile
from pathlib import Path
from unittest.mock import MagicMock, patch

from macjet.mcp.safety import MIN_SAFE_PID, get_audit_log, resolve_pid, validate_pid, send_signal


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


class TestSendSignal:
    """Signal dispatch and audit logging."""

    @patch("macjet.mcp.safety.validate_pid")
    def test_invalid_pid_fails_early(self, mock_validate):
        mock_validate.return_value = (False, "Bad PID")
        success, err = send_signal(999, signal.SIGTERM, "Test")
        assert not success
        assert err == "Bad PID"

    @patch("macjet.mcp.safety.validate_pid")
    @patch("macjet.mcp.safety.resolve_pid")
    @patch("os.kill")
    @patch("macjet.mcp.safety.AUDIT_LOG_PATH")
    def test_successful_signal_logs_audit(self, mock_audit_path, mock_kill, mock_resolve, mock_validate, tmp_path):
        mock_validate.return_value = (True, "")
        mock_resolve.return_value = {"name": "TestApp"}
        
        # Use a temporary file for the audit log
        log_file = tmp_path / "audit.jsonl"
        mock_audit_path.return_value = log_file
        with patch("macjet.mcp.safety.AUDIT_LOG_PATH", log_file):
            success, audit_id = send_signal(1234, signal.SIGTERM, "Cleanup")
            
            assert success
            assert audit_id.startswith("mcp-sigterm-")
            mock_kill.assert_called_once_with(1234, signal.SIGTERM)
            
            # Verify log was written
            content = log_file.read_text()
            assert "TestApp" in content
            assert "Cleanup" in content

    @patch("macjet.mcp.safety.validate_pid")
    @patch("macjet.mcp.safety.resolve_pid")
    @patch("os.kill")
    @patch("macjet.mcp.safety.AUDIT_LOG_PATH")
    def test_permission_error_handled(self, mock_audit_path, mock_kill, mock_resolve, mock_validate, tmp_path):
        mock_validate.return_value = (True, "")
        mock_resolve.return_value = {"name": "RootApp"}
        mock_kill.side_effect = PermissionError("EPERM")
        
        log_file = tmp_path / "audit.jsonl"
        with patch("macjet.mcp.safety.AUDIT_LOG_PATH", log_file):
            success, err = send_signal(1234, signal.SIGKILL, "Force")
            
            assert not success
            assert "Permission denied" in err
            
            # Still logs the failure
            content = log_file.read_text()
            assert "false" in content.lower() or "success\": false" in content

    @patch("macjet.mcp.safety.validate_pid")
    @patch("macjet.mcp.safety.resolve_pid")
    @patch("os.kill")
    @patch("macjet.mcp.safety.AUDIT_LOG_PATH")
    def test_process_lookup_error_handled(self, mock_audit_path, mock_kill, mock_resolve, mock_validate, tmp_path):
        mock_validate.return_value = (True, "")
        mock_resolve.return_value = {"name": "DeadApp"}
        mock_kill.side_effect = ProcessLookupError("ESRCH")
        
        log_file = tmp_path / "audit.jsonl"
        with patch("macjet.mcp.safety.AUDIT_LOG_PATH", log_file):
            success, err = send_signal(1234, signal.SIGSTOP, "Pause")
            
            assert not success
            assert "no longer exists" in err

    @patch("macjet.mcp.safety.validate_pid")
    @patch("macjet.mcp.safety.resolve_pid")
    @patch("os.kill")
    @patch("macjet.mcp.safety.AUDIT_LOG_PATH")
    def test_os_error_handled(self, mock_audit_path, mock_kill, mock_resolve, mock_validate, tmp_path):
        mock_validate.return_value = (True, "")
        mock_resolve.return_value = {"name": "WeirdApp"}
        mock_kill.side_effect = OSError("Unknown OS error")
        
        log_file = tmp_path / "audit.jsonl"
        with patch("macjet.mcp.safety.AUDIT_LOG_PATH", log_file):
            success, err = send_signal(1234, signal.SIGCONT, "Resume")
            
            assert not success
            assert "Unknown OS error" in err
