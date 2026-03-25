"""
MacJet MCP — Safety module for destructive operations.
Kill guard, audit logging, PID validation, and self-protection.
"""
from __future__ import annotations

import json
import os
import signal
import time
from datetime import datetime, timezone
from pathlib import Path

import psutil


# ── Constants ────────────────────────────────────────────────
AUDIT_LOG_PATH = Path.home() / ".macjet" / "mcp_audit.jsonl"
MIN_SAFE_PID = 500  # System/kernel processes are below this
_SELF_PID = os.getpid()


def _ensure_audit_dir() -> None:
    """Ensure the audit log directory exists."""
    AUDIT_LOG_PATH.parent.mkdir(parents=True, exist_ok=True)


def validate_pid(pid: int) -> tuple[bool, str]:
    """Validate that a PID is safe to operate on.
    
    Returns (is_safe, error_message).
    """
    if pid <= 0:
        return False, f"Invalid PID: {pid}"

    if pid < MIN_SAFE_PID:
        return False, f"PID {pid} is a system/kernel process (PID < {MIN_SAFE_PID}). Refusing."

    if pid == _SELF_PID:
        return False, f"PID {pid} is the MCP server itself. Refusing to self-terminate."

    # Check if process exists
    if not psutil.pid_exists(pid):
        return False, f"PID {pid} does not exist."

    return True, ""


def resolve_pid(pid: int) -> dict:
    """Resolve a PID to process info for preview/confirmation.
    
    Returns a dict with name, cmdline, cpu_percent, memory_mb.
    """
    try:
        proc = psutil.Process(pid)
        return {
            "pid": pid,
            "name": proc.name(),
            "cmdline": " ".join(proc.cmdline()[:5]),  # Truncate
            "cpu_percent": proc.cpu_percent(),
            "memory_mb": (proc.memory_info().rss / (1024 * 1024)) if proc.memory_info() else 0,
            "username": proc.username(),
            "status": proc.status(),
        }
    except (psutil.NoSuchProcess, psutil.AccessDenied, psutil.ZombieProcess) as e:
        return {"pid": pid, "name": "unknown", "error": str(e)}


def send_signal(pid: int, sig: int, reason: str, client_id: str = "", request_id: str = "") -> tuple[bool, str]:
    """Send a signal to a process and log the action.
    
    Returns (success, error_message).
    """
    # Validate first
    is_safe, err = validate_pid(pid)
    if not is_safe:
        return False, err

    # Resolve for audit
    info = resolve_pid(pid)
    sig_name = "SIGTERM" if sig == signal.SIGTERM else "SIGKILL" if sig == signal.SIGKILL else \
               "SIGSTOP" if sig == signal.SIGSTOP else "SIGCONT" if sig == signal.SIGCONT else str(sig)

    # Execute signal
    try:
        os.kill(pid, sig)
        success = True
        error = ""
    except ProcessLookupError:
        success = False
        error = f"PID {pid} no longer exists"
    except PermissionError:
        success = False
        error = f"Permission denied for PID {pid}"
    except OSError as e:
        success = False
        error = str(e)

    # Audit log
    audit_entry = {
        "ts": datetime.now(timezone.utc).isoformat(),
        "tool": f"{'kill' if sig in (signal.SIGTERM, signal.SIGKILL) else 'suspend' if sig == signal.SIGSTOP else 'resume'}_process",
        "pid": pid,
        "name": info.get("name", "unknown"),
        "signal": sig_name,
        "reason": reason,
        "success": success,
        "error": error,
        "client_id": client_id,
        "request_id": request_id,
    }
    audit_id = f"mcp-{sig_name.lower()}-{int(time.time())}"
    audit_entry["audit_id"] = audit_id

    _ensure_audit_dir()
    with open(AUDIT_LOG_PATH, "a") as f:
        f.write(json.dumps(audit_entry) + "\n")

    return success, error if not success else audit_id


def get_audit_log(limit: int = 50) -> str:
    """Read recent audit log entries."""
    if not AUDIT_LOG_PATH.exists():
        return "No audit entries yet."

    lines = AUDIT_LOG_PATH.read_text().strip().split("\n")
    recent = lines[-limit:]
    return "\n".join(recent)
