/// MacJet MCP — Data Models
/// Serde schemas for JSON-RPC serialization, replacing Pydantic from Python.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SystemOverview {
    pub cpu_percent: f64,
    pub memory_used_gb: f64,
    pub memory_total_gb: f64,
    pub memory_percent: f64,
    pub thermal_pressure: String,
    pub fan_rpm: Option<u32>,
    pub top_process: String,
    pub top_cpu_percent: f64,
    pub process_count: usize,
    pub verdict: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProcessSummary {
    pub name: String,
    pub pid_count: usize,
    pub top_pid: u32,
    pub total_cpu: f64,
    pub total_memory_mb: f64,
    #[serde(default)]
    pub energy_impact: String,
    #[serde(default)]
    pub context_label: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProcessListResult {
    pub groups: Vec<ProcessSummary>,
    pub total_groups: usize,
    pub sort_by: String,
    #[serde(default)]
    pub filter_applied: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChildProcess {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub threads: usize,
    #[serde(default)]
    pub cmdline: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChromeTab {
    pub rank: usize,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub renderer_pid: u32,
    #[serde(default)]
    pub cpu_time_s: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChromeTabsResult {
    pub tabs: Vec<ChromeTab>,
    pub total_tabs: usize,
    pub cdp_connected: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProcessDetail {
    pub name: String,
    pub total_cpu: f64,
    pub total_memory_mb: f64,
    pub process_count: usize,
    pub children: Vec<ChildProcess>,
    #[serde(default)]
    pub chrome_tabs: Vec<ChromeTab>,
    #[serde(default)]
    pub why_hot: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HeatExplanation {
    pub severity: String,
    pub cpu_percent: f64,
    pub primary_culprit: String,
    pub primary_cpu_percent: f64,
    pub secondary_culprits: Vec<String>,
    pub recommendations: Vec<String>,
    pub detailed_report: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KillConfirmation {
    pub success: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KillResult {
    pub success: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SuspendResult {
    pub success: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnergyEntry {
    pub pid: u32,
    pub command: String,
    pub interrupt_wakeups_per_sec: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnergyReport {
    pub available: bool,
    #[serde(default)]
    pub unavailable_reason: String,
    pub entries: Vec<EnergyEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NetworkEntry {
    pub pid: u32,
    pub name: String,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NetworkReport {
    pub available: bool,
    pub entries: Vec<NetworkEntry>,
    #[serde(default)]
    pub system_bytes_in_per_s: f64,
    #[serde(default)]
    pub system_bytes_out_per_s: f64,
}

/// Extended system overview aligned with collectors + thermal (MCP).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SystemOverviewExtended {
    pub cpu_percent: f64,
    pub memory_used_gb: f64,
    pub memory_total_gb: f64,
    pub memory_percent: f64,
    pub swap_total_gb: f64,
    pub swap_used_gb: f64,
    pub cpu_brand: String,
    pub cpu_count_physical: usize,
    pub thermal_pressure: String,
    pub fan_rpm: Option<u32>,
    pub fan_rpm_max: Option<u32>,
    pub cpu_die_temp_c: f64,
    pub gpu_die_temp_c: f64,
    pub gpu_active_percent: f64,
    pub top_process: String,
    pub top_cpu_percent: f64,
    pub process_count: usize,
    pub verdict: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReclaimCandidateMcp {
    pub group_key: String,
    pub app_name: String,
    pub icon: String,
    pub score: u8,
    pub reclaim_cpu: f64,
    pub reclaim_mem_mb: f64,
    pub risk: String,
    pub reason: String,
    pub suggested_action: String,
    pub child_count: usize,
    pub is_hidden: bool,
    pub launch_age_s: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReclaimListResult {
    pub candidates: Vec<ReclaimCandidateMcp>,
    pub total_considered: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_overview_model() {
        let model = SystemOverview {
            cpu_percent: 45.5,
            memory_used_gb: 12.0,
            memory_total_gb: 16.0,
            memory_percent: 75.0,
            thermal_pressure: "nominal".to_string(),
            fan_rpm: Some(2000),
            top_process: "Google Chrome".to_string(),
            top_cpu_percent: 15.2,
            process_count: 350,
            verdict: "System is running smoothly.".to_string(),
        };

        assert_eq!(model.cpu_percent, 45.5);
        assert_eq!(model.fan_rpm, Some(2000));
    }
}
