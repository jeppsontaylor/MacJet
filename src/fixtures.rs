/// Shared mock data and fixtures for unit tests.
use crate::collectors::process_collector::ProcessInfo;
use smol_str::SmolStr;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn make_process_info(
    pid: u32,
    name: &str,
    cpu_percent: f64,
    memory_mb: f64,
    cmdline: Vec<&str>,
    username: &str,
    exe: &str,
    ppid: u32,
    is_hidden: bool,
    is_system: bool,
) -> ProcessInfo {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    ProcessInfo {
        pid,
        name: SmolStr::new(name),
        cpu_percent,
        memory_mb,
        memory_percent: 0.0,
        num_threads: 1,
        cmdline: cmdline.into_iter().map(SmolStr::new).collect(),
        cwd: SmolStr::default(),
        exe: SmolStr::new(exe),
        ppid,
        status: SmolStr::default(),
        create_time: now - 100.0,
        username: SmolStr::new(username),
        children_pids: Vec::new(),
        context_label: SmolStr::default(),
        confidence: SmolStr::new("grouped"),
        energy_impact: SmolStr::default(),
        net_bytes_sent: 0,
        net_bytes_recv: 0,
        role_type: SmolStr::default(),
        is_hidden,
        launch_age_s: 100.0,
        is_system,
    }
}
