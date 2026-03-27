/// MacJet — Process Actions (Signals)
///
/// Wrappers for POSIX signals using safe `std::process::Command` to avoid
/// needing `unsafe` libc blocks, as our crate enforces `#![forbid(unsafe_code)]`.
use std::process::Command;

pub fn terminate_process(pid: u32) -> bool {
    Command::new("kill")
        .arg("-15")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn force_kill_process(pid: u32) -> bool {
    Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn suspend_process(pid: u32) -> bool {
    Command::new("kill")
        .arg("-STOP")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn resume_process(pid: u32) -> bool {
    Command::new("kill")
        .arg("-CONT")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
