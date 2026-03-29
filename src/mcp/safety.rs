/// MacJet MCP — Safety module for destructive operations.
/// Kill guard, audit logging, PID validation, and self-protection.
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::{Pid, System};

pub const MIN_SAFE_PID: u32 = 500;

#[derive(Serialize, Deserialize, Debug)]
pub struct AuditEntry {
    pub ts: String,
    pub tool: String,
    pub pid: u32,
    pub name: String,
    pub signal: String,
    pub reason: String,
    pub success: bool,
    pub error: String,
    pub client_id: String,
    pub request_id: String,
    pub audit_id: String,
}

fn audit_log_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let mut path = PathBuf::from(home);
    path.push(".macjet");
    let _ = std::fs::create_dir_all(&path);
    path.push("mcp_audit.jsonl");
    path
}

fn pid_exists(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn validate_pid(pid: u32) -> Result<(), String> {
    if pid == 0 {
        return Err(format!("Invalid PID: {}", pid));
    }

    if pid < MIN_SAFE_PID {
        return Err(format!(
            "PID {} is a system/kernel process (PID < {}). Refusing.",
            pid, MIN_SAFE_PID
        ));
    }

    if pid == std::process::id() {
        return Err(format!(
            "PID {} is the MCP server itself. Refusing to self-terminate.",
            pid
        ));
    }

    if !pid_exists(pid) {
        return Err(format!("PID {} does not exist.", pid));
    }

    Ok(())
}

pub fn resolve_pid(pid: u32) -> serde_json::Value {
    let mut sys = System::new_with_specifics(
        sysinfo::RefreshKind::nothing().with_processes(sysinfo::ProcessRefreshKind::everything()),
    );
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    if let Some(proc) = sys.process(Pid::from_u32(pid)) {
        let memory_mb = proc.memory() as f64 / (1024.0 * 1024.0);
        let cmdline = proc
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let truncated_cmd = if cmdline.chars().count() > 100 {
            format!("{}...", cmdline.chars().take(97).collect::<String>())
        } else {
            cmdline
        };

        serde_json::json!({
            "pid": pid,
            "name": proc.name().to_string_lossy(),
            "cmdline": truncated_cmd,
            "cpu_percent": proc.cpu_usage(),
            "memory_mb": memory_mb,
            "username": "",
            "status": "running",
        })
    } else {
        serde_json::json!({
            "pid": pid,
            "name": "unknown",
            "error": format!("Process {} not found during resolve", pid),
        })
    }
}

pub fn send_signal(
    pid: u32,
    sig: i32,
    reason: &str,
    client_id: &str,
    request_id: &str,
) -> Result<String, String> {
    if let Err(e) = validate_pid(pid) {
        return Err(e);
    }

    let info = resolve_pid(pid);
    let name = info
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let (sig_name, sig_arg, tool) = match sig {
        15 => ("SIGTERM", "-15", "kill_process"),
        9 => ("SIGKILL", "-9", "kill_process"),
        17 | 19 => ("SIGSTOP", "-STOP", "suspend_process"),
        18 | 21 => ("SIGCONT", "-CONT", "resume_process"),
        _ => ("UNKNOWN", "", "kill_process"),
    };

    let (success, error) = if sig_arg.is_empty() {
        (false, format!("Unsupported signal {}", sig))
    } else {
        match Command::new("kill")
            .arg(sig_arg)
            .arg(pid.to_string())
            .status()
        {
            Ok(status) => {
                if status.success() {
                    (true, "".to_string())
                } else {
                    (false, format!("Failed to send {} to {}", sig_name, pid))
                }
            }
            Err(e) => (false, e.to_string()),
        }
    };

    // Format ISO string manually to limit dependencies if needed, or use a basic format.
    // For skeleton we omit strict native chrono/date limits but let's just make one.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let audit_id = format!("mcp-{}-{}", sig_name.to_lowercase(), now);

    let entry = AuditEntry {
        ts: format!("timestamp-{}", now),
        tool: tool.to_string(),
        pid,
        name,
        signal: sig_name.to_string(),
        reason: reason.to_string(),
        success,
        error: error.clone(),
        client_id: client_id.to_string(),
        request_id: request_id.to_string(),
        audit_id: audit_id.clone(),
    };

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(audit_log_path())
    {
        if let Ok(json_entry) = serde_json::to_string(&entry) {
            let _ = writeln!(file, "{}", json_entry);
        }
    }

    if success {
        Ok(audit_id)
    } else {
        Err(error)
    }
}

/// Append a disk-trash MCP action to the same JSONL audit log as `kill_process`.
pub fn audit_disk_trash(paths: &[String], success: bool, error: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let entry = serde_json::json!({
        "ts": format!("timestamp-{}", now),
        "tool": "trash_disk_paths",
        "paths": paths,
        "success": success,
        "error": error,
    });
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(audit_log_path())
    {
        if let Ok(line) = serde_json::to_string(&entry) {
            let _ = writeln!(file, "{}", line);
        }
    }
}

pub fn get_audit_log(limit: usize) -> String {
    let path = audit_log_path();
    if !path.exists() {
        return "No audit entries yet.".to_string();
    }

    if let Ok(content) = std::fs::read_to_string(path) {
        let lines: Vec<&str> = content.trim().split('\n').collect();
        let skip = lines.len().saturating_sub(limit);
        lines[skip..].join("\n")
    } else {
        "Failed to read audit log.".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_pid_rejected() {
        assert!(validate_pid(0).is_err());
    }

    #[test]
    fn test_system_pid_rejected() {
        let err = validate_pid(1).unwrap_err();
        assert!(err.contains("system"));
    }

    #[test]
    fn test_boundary_pid_rejected() {
        assert!(validate_pid(499).is_err());
    }

    #[test]
    fn test_self_pid_rejected() {
        let err = validate_pid(std::process::id()).unwrap_err();
        assert!(err.contains("MCP server itself"));
    }
}
