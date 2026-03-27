//! Rich system fingerprint for `benchmark_compare` JSON (macOS-first, privacy-safe).
//!
//! Omits serial numbers, UUIDs, and provisioning IDs from committed artifacts.

use std::collections::BTreeMap;
use std::process::Command;

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sysinfo::System;

/// Top-level schema version for benchmark JSON files.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct ToolMeta {
    pub name: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMeta {
    pub started_at_utc: String,
    pub finished_at_utc: String,
    pub wall_seconds: f64,
    pub argv: Vec<String>,
    pub max_samples: usize,
    pub interval_secs: f64,
    pub no_ml_flag: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemFingerprint {
    pub platform: String,
    pub arch: String,
    pub hostname: Option<String>,
    pub os_version: Option<String>,
    pub kernel_version: Option<String>,
    pub sysinfo: SysinfoSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macos: Option<MacosFingerprint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SysinfoSnapshot {
    pub total_memory_bytes: u64,
    pub physical_core_count: Option<usize>,
    pub logical_cpu_count: usize,
    pub sysinfo_long_os_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MacosFingerprint {
    pub machine_name: Option<String>,
    pub machine_model: Option<String>,
    pub chip_type: Option<String>,
    pub model_number: Option<String>,
    pub number_processors: Option<String>,
    pub physical_memory: Option<String>,
    pub boot_rom_version: Option<String>,
    pub os_version_full: Option<String>,
    pub kernel_version: Option<String>,
    pub local_host_name: Option<String>,
    pub memory_dimm_type: Option<String>,
    pub memory_technology: Option<String>,
    /// Stable, sorted sysctl keys useful for reproducibility (no secrets).
    pub sysctl: BTreeMap<String, String>,
    /// Redacted `system_profiler` hardware + software + memory (no serial/UUID).
    pub system_profiler: Value,
}

pub fn tool_meta() -> ToolMeta {
    ToolMeta {
        name: "benchmark_compare",
        version: env!("CARGO_PKG_VERSION"),
    }
}

pub fn utc_now_rfc3339_millis() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn sysctl_n(key: &str) -> Option<String> {
    let out = Command::new("sysctl").args(["-n", key]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(target_os = "macos")]
fn sysctl_map_macos() -> BTreeMap<String, String> {
    const KEYS: &[&str] = &[
        "hw.model",
        "hw.machine",
        "hw.memsize",
        "hw.ncpu",
        "hw.physicalcpu",
        "hw.logicalcpu",
        "hw.perflevel0.physicalcpu",
        "hw.perflevel0.logicalcpu",
        "hw.perflevel1.physicalcpu",
        "hw.perflevel1.logicalcpu",
        "hw.optional.arm64",
        "hw.optional.arm64_1",
        "hw.optional.armv8_1a",
        "kern.osrelease",
        "kern.osversion",
        "kern.version",
    ];
    let mut m = BTreeMap::new();
    for k in KEYS {
        if let Some(v) = sysctl_n(k) {
            m.insert((*k).to_string(), v);
        }
    }
    m
}

#[cfg(not(target_os = "macos"))]
fn sysctl_map_macos() -> BTreeMap<String, String> {
    BTreeMap::new()
}

#[cfg(target_os = "macos")]
fn system_profiler_json() -> Option<Value> {
    let out = Command::new("system_profiler")
        .args([
            "SPHardwareDataType",
            "SPSoftwareDataType",
            "SPMemoryDataType",
            "-json",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

#[cfg(not(target_os = "macos"))]
fn system_profiler_json() -> Option<Value> {
    None
}

/// Remove identifiers that should not be committed to git.
fn redact_system_profiler(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(obj) = v.as_object_mut() {
        if let Some(arr) = obj.get_mut("SPHardwareDataType").and_then(|x| x.as_array_mut()) {
            for item in arr.iter_mut().filter_map(|x| x.as_object_mut()) {
                item.remove("serial_number");
                item.remove("platform_UUID");
                item.remove("provisioning_UDID");
            }
        }
    }
    v
}

#[cfg(target_os = "macos")]
fn macos_fingerprint_from_profiler(sp: &Value) -> MacosFingerprint {
    let hw = sp
        .get("SPHardwareDataType")
        .and_then(|x| x.as_array())
        .and_then(|a| a.first())
        .and_then(|x| x.as_object());

    let sw = sp
        .get("SPSoftwareDataType")
        .and_then(|x| x.as_array())
        .and_then(|a| a.first())
        .and_then(|x| x.as_object());

    let mem = sp.get("SPMemoryDataType").and_then(|x| x.as_array());

    let (dimm_type, mem_tech) = mem
        .and_then(|rows| rows.first())
        .and_then(|x| x.as_object())
        .map(|o| {
            (
                o.get("dimm_type").and_then(|v| v.as_str()).map(String::from),
                o.get("SPMemoryDataType")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            )
        })
        .unwrap_or((None, None));

    MacosFingerprint {
        machine_name: hw.and_then(|o| o.get("machine_name")).and_then(|v| v.as_str()).map(String::from),
        machine_model: hw.and_then(|o| o.get("machine_model")).and_then(|v| v.as_str()).map(String::from),
        chip_type: hw.and_then(|o| o.get("chip_type")).and_then(|v| v.as_str()).map(String::from),
        model_number: hw.and_then(|o| o.get("model_number")).and_then(|v| v.as_str()).map(String::from),
        number_processors: hw
            .and_then(|o| o.get("number_processors"))
            .and_then(|v| v.as_str())
            .map(String::from),
        physical_memory: hw
            .and_then(|o| o.get("physical_memory"))
            .and_then(|v| v.as_str())
            .map(String::from),
        boot_rom_version: hw
            .and_then(|o| o.get("boot_rom_version"))
            .and_then(|v| v.as_str())
            .map(String::from),
        os_version_full: sw.and_then(|o| o.get("os_version")).and_then(|v| v.as_str()).map(String::from),
        kernel_version: sw
            .and_then(|o| o.get("kernel_version"))
            .and_then(|v| v.as_str())
            .map(String::from),
        local_host_name: sw
            .and_then(|o| o.get("local_host_name"))
            .and_then(|v| v.as_str())
            .map(String::from),
        memory_dimm_type: dimm_type,
        memory_technology: mem_tech,
        sysctl: sysctl_map_macos(),
        system_profiler: redact_system_profiler(sp),
    }
}

pub fn collect_system_fingerprint(sys: &mut System) -> SystemFingerprint {
    sys.refresh_memory();

    let hostname = System::host_name();
    let os_version = System::long_os_version();
    let arch = std::env::consts::ARCH.to_string();

    let sysinfo = SysinfoSnapshot {
        total_memory_bytes: sys.total_memory(),
        physical_core_count: sys.physical_core_count(),
        logical_cpu_count: sys.cpus().len(),
        sysinfo_long_os_version: os_version,
    };

    #[cfg(target_os = "macos")]
    {
        let macos = system_profiler_json().map(|sp| macos_fingerprint_from_profiler(&sp));
        let os_version = macos
            .as_ref()
            .and_then(|m| m.os_version_full.clone())
            .or_else(|| sysinfo.sysinfo_long_os_version.clone());
        SystemFingerprint {
            platform: "macOS".to_string(),
            arch,
            hostname,
            os_version,
            kernel_version: macos.as_ref().and_then(|m| m.kernel_version.clone()),
            sysinfo,
            macos,
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        SystemFingerprint {
            platform: std::env::consts::OS.to_string(),
            arch,
            hostname,
            os_version: sysinfo.sysinfo_long_os_version.clone(),
            kernel_version: None,
            sysinfo,
            macos: None,
        }
    }
}

/// Merge `system` + `run` + `tool` + `schema_version` into an existing JSON value (summary + samples).
pub fn merge_report_shell(
    mut base: Value,
    tool: ToolMeta,
    run: RunMeta,
    system: SystemFingerprint,
) -> Value {
    let o = base
        .as_object_mut()
        .expect("benchmark JSON must be an object");
    o.insert("schema_version".to_string(), json!(SCHEMA_VERSION));
    o.insert("tool".to_string(), serde_json::to_value(&tool).unwrap());
    o.insert("run".to_string(), serde_json::to_value(&run).unwrap());
    o.insert("system".to_string(), serde_json::to_value(&system).unwrap());
    base
}

/// Resolve repo root by walking parents for `Cargo.toml` with `name = "macjet"`.
pub fn find_macjet_repo_root() -> Option<std::path::PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    for _ in 0..10 {
        let cargo = dir.join("Cargo.toml");
        if cargo.is_file() {
            if let Ok(toml) = std::fs::read_to_string(&cargo) {
                if toml.lines().any(|l| l.trim() == "name = \"macjet\"") {
                    return Some(dir);
                }
            }
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

/// Relative segment under the repo (or cwd) for committed `benchmark_compare` JSON artifacts.
pub const DEFAULT_BENCHMARK_RESULTS_SUBDIR: &str = "benchmarks/results";

/// Default artifact path: `<repo>/benchmarks/results/benchmark_compare_<unix_ts>.json` when repo root
/// is found; otherwise `./benchmarks/results/...` under the current directory.
pub fn default_benchmark_json_path(unix_ts: u64) -> std::path::PathBuf {
    let base = find_macjet_repo_root()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")));
    base.join(DEFAULT_BENCHMARK_RESULTS_SUBDIR)
        .join(format!("benchmark_compare_{}.json", unix_ts))
}

/// Try to make `preferred` writable: `create_dir_all` on its parent. On failure (permission denied,
/// `benchmarks` is a file, read-only tree, etc.), return the **same filename** in the **current
/// working directory** so the run still produces JSON.
pub fn resolve_default_benchmark_path(preferred: std::path::PathBuf) -> std::path::PathBuf {
    let Some(parent) = preferred
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    else {
        return preferred;
    };
    match std::fs::create_dir_all(parent) {
        Ok(()) => preferred,
        Err(e) => {
            let name = preferred.file_name().unwrap_or_else(|| std::ffi::OsStr::new(
                "benchmark_compare.json",
            ));
            let fallback = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(name);
            eprintln!(
                "benchmark_compare: cannot create output directory {} ({}). \
                 Often `benchmarks` is missing, not writable, or not a directory — fix with \
                 `mkdir -p benchmarks/results` or `ls -la benchmarks`. \
                 Writing to {} instead.",
                parent.display(),
                e,
                fallback.display()
            );
            fallback
        }
    }
}
