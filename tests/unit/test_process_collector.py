"""
Tests for ProcessCollector helper functions — app name parsing, group keys,
system process detection, role extraction.

These test the pure functions without requiring a live psutil connection.
"""

from __future__ import annotations

from macjet.collectors.process_collector import (
    _determine_group_key,
    _extract_role_type,
    _is_system_process,
    _parse_app_name,
    _severity_icon,
)
from tests.conftest import make_process_info


class TestSeverityIcon:
    """Verify CPU severity icon thresholds."""

    def test_low_cpu(self):
        assert _severity_icon(5.0) == "\U0001f7e2"  # green

    def test_medium_cpu(self):
        assert _severity_icon(30.0) == "\U0001f7e1"  # yellow

    def test_high_cpu(self):
        assert _severity_icon(60.0) == "\U0001f7e0"  # orange

    def test_critical_cpu(self):
        assert _severity_icon(150.0) == "\U0001f534"  # red


class TestParseAppName:
    """Verify app name extraction from process info."""

    def test_chrome_renderer_helper(self):
        proc = make_process_info(
            name="Google Chrome Helper (Renderer)", cmdline=["--type=renderer"]
        )
        result = _parse_app_name(proc)
        assert "Google Chrome" in result
        assert "renderer" in result

    def test_node_with_script(self):
        proc = make_process_info(name="node", cmdline=["node", "server.js"])
        result = _parse_app_name(proc)
        assert "server.js" in result

    def test_python_with_script(self):
        proc = make_process_info(name="python3", cmdline=["python3", "train.py"])
        result = _parse_app_name(proc)
        assert "train.py" in result

    def test_java_with_jar(self):
        proc = make_process_info(name="java", cmdline=["java", "-jar", "app.jar"])
        result = _parse_app_name(proc)
        assert "app.jar" in result

    def test_plain_process(self):
        proc = make_process_info(name="Finder", cmdline=["/System/Library/CoreServices/Finder.app"])
        result = _parse_app_name(proc)
        assert result == "Finder"


class TestExtractRoleType:
    """Verify subprocess role type extraction from cmdline."""

    def test_renderer_type(self):
        assert _extract_role_type(["--type=renderer"]) == "renderer"

    def test_gpu_process_type(self):
        assert _extract_role_type(["--type=gpu-process"]) == "gpu-process"

    def test_no_type_flag(self):
        assert _extract_role_type(["--flag", "--other"]) == ""

    def test_empty_cmdline(self):
        assert _extract_role_type([]) == ""


class TestIsSystemProcess:
    """Verify system process detection."""

    def test_root_user(self):
        assert _is_system_process("root", "/usr/sbin/syslogd")

    def test_system_exe_path(self):
        assert _is_system_process("someuser", "/usr/libexec/something")

    def test_library_apple_path(self):
        assert _is_system_process("someuser", "/Library/Apple/something")

    def test_regular_user_app(self):
        assert not _is_system_process("testuser", "/Applications/MyApp.app/Contents/MacOS/MyApp")

    def test_windowserver_user(self):
        assert _is_system_process("_windowserver", "")


class TestDetermineGroupKey:
    """Verify process grouping logic."""

    def test_chrome_helper_groups_under_chrome(self):
        proc = make_process_info(name="Google Chrome Helper", cmdline=["--type=renderer"], ppid=100)
        result = _determine_group_key(proc)
        assert result == "Google Chrome"

    def test_docker_groups_together(self):
        for name in ("com.docker.vmnetd", "com.docker.backend", "Docker"):
            proc = make_process_info(name=name)
            assert _determine_group_key(proc) == "Docker Desktop"

    def test_vscode_helper_groups(self):
        proc = make_process_info(name="Code Helper (Renderer)")
        assert _determine_group_key(proc) == "Code"

    def test_standalone_app_keeps_name(self):
        proc = make_process_info(name="Spotify")
        assert _determine_group_key(proc) == "Spotify"
