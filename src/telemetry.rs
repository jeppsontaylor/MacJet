/// MacJet — Self-Telemetry Module
///
/// Writes rolling 1-minute JSON logs to benchmarks/telemetry/
/// with prefix `macjet_rs_telemetry_` so analyze.py can compare both.
/// Keeps at most 5 log files, deleting the oldest when exceeded.
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const BUFFER_SIZE: usize = 60;
const MAX_LOG_FILES: usize = 5;
const LOG_PREFIX: &str = "macjet_rs_telemetry_";

#[derive(Debug, Clone, Serialize)]
pub struct TelemetrySample {
    pub timestamp: f64,
    pub cpu_percent: f64,
    pub rss_mb: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub am_cpu_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub am_rss_mb: Option<f64>,
}

pub struct SelfTelemetry {
    buffer: Vec<TelemetrySample>,
    log_dir: PathBuf,
    pid: u32,
    am_pid: Option<u32>,
}

impl SelfTelemetry {
    pub fn new() -> Self {
        // Find the project root by looking for benchmarks/ relative to the binary
        let log_dir = Self::find_log_dir();
        fs::create_dir_all(&log_dir).ok();

        Self {
            buffer: Vec::with_capacity(BUFFER_SIZE),
            log_dir,
            pid: std::process::id(),
            am_pid: None,
        }
    }

    fn find_log_dir() -> PathBuf {
        // Try relative to CWD first, then walk up
        let mut dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        for _ in 0..5 {
            let candidate = dir.join("benchmarks").join("telemetry");
            if dir.join("benchmarks").exists() {
                return candidate;
            }
            if !dir.pop() {
                break;
            }
        }
        // Fallback: use CWD/benchmarks/telemetry
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("benchmarks")
            .join("telemetry")
    }

    /// Record a sample. Called every 1s from the fast-lane tick.
    pub fn record(
        &mut self,
        cpu_percent: f64,
        rss_mb: f64,
        am_cpu_percent: Option<f64>,
        am_rss_mb: Option<f64>,
    ) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        self.buffer.push(TelemetrySample {
            timestamp,
            cpu_percent,
            rss_mb,
            am_cpu_percent,
            am_rss_mb,
        });

        if self.buffer.len() >= BUFFER_SIZE {
            self.flush();
        }
    }

    /// Flush the buffer to a JSON file and enforce the rolling limit.
    pub fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let filename = format!("{}{}_{}.json", LOG_PREFIX, self.pid, timestamp);
        let path = self.log_dir.join(&filename);

        if let Ok(json) = serde_json::to_string_pretty(&self.buffer) {
            fs::write(&path, json).ok();
        }

        self.buffer.clear();
        self.enforce_limit();
    }

    fn enforce_limit(&self) {
        let mut logs: Vec<_> = fs::read_dir(&self.log_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map_or(false, |n| n.starts_with(LOG_PREFIX) && n.ends_with(".json"))
            })
            .collect();

        if logs.len() <= MAX_LOG_FILES {
            return;
        }

        // Sort by modified time, oldest first
        logs.sort_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        });

        let to_remove = logs.len() - MAX_LOG_FILES;
        for entry in logs.into_iter().take(to_remove) {
            fs::remove_file(entry.path()).ok();
        }
    }

    /// Get current RSS of our own process in MB.
    pub fn own_rss_mb() -> f64 {
        use sysinfo::System;
        let pid = sysinfo::Pid::from_u32(std::process::id());
        let mut sys = System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        sys.process(pid)
            .map(|p| p.memory() as f64 / (1024.0 * 1024.0))
            .unwrap_or(0.0)
    }

    /// Get current CPU and RSS of Activity Monitor, using cached PID to avoid overhead.
    pub fn activity_monitor_stats(&mut self) -> (Option<f64>, Option<f64>) {
        use sysinfo::{Pid, System};
        if self.am_pid.is_none() {
            let mut sys = System::new();
            sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            for (pid, process) in sys.processes() {
                if process.name() == "Activity Monitor" {
                    self.am_pid = Some(pid.as_u32());
                    break;
                }
            }
        }

        if let Some(am) = self.am_pid {
            let pid = Pid::from_u32(am);
            let mut sys = System::new();
            sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
            if let Some(process) = sys.process(pid) {
                return (
                    Some(process.cpu_usage() as f64),
                    Some(process.memory() as f64 / (1024.0 * 1024.0)),
                );
            } else {
                self.am_pid = None; // Process likely closed
            }
        }
        (None, None)
    }
}
