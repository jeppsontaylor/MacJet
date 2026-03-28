/// Live MCP snapshot built each collector tick (serde-friendly, TUI-independent).
use crate::collectors::chrome_enricher::TabEntry;
use crate::collectors::energy_collector::{EnergyInfo, EnergySnapshot, ThermalInfo};
use crate::collectors::metrics_history::MetricsHistory;
use crate::collectors::network_collector::NetSnapshot;
use crate::collectors::process_collector::{ProcessGroup, ProcessInfo};
use crate::collectors::system_stats::SystemSnapshot;
use serde::Serialize;
use smol_str::SmolStr;

pub const MCP_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct McpCapabilities {
    pub powermetrics: bool,
    pub chrome_cdp: bool,
    pub ml_predictor: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpMeta {
    pub schema_version: u32,
    pub collected_at_unix: f64,
    pub refresh_interval_secs: u64,
    pub hostname: String,
    pub macjet_version: &'static str,
    pub capabilities: McpCapabilities,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnergyInfoRow {
    pub pid: u32,
    pub name: String,
    pub wakeups_per_s: f64,
    pub energy_impact: f64,
    pub cpu_ms_per_s: f64,
}

/// Full collector state exposed to MCP tools/resources.
#[derive(Clone)]
pub struct McpSnapshot {
    pub meta: McpMeta,
    pub system: SystemSnapshot,
    pub thermal: ThermalInfo,
    pub groups: Vec<ProcessGroup>,
    pub network: NetSnapshot,
    pub chrome_tabs: Vec<TabEntry>,
    pub chrome_cdp_available: bool,
    pub energy: EnergySnapshot,
    pub energy_top: Vec<EnergyInfoRow>,
    pub predictor_stats: Option<crate::collectors::cpu_predictor::PredictorStats>,
}

impl Default for McpSnapshot {
    fn default() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        Self {
            meta: McpMeta {
                schema_version: MCP_SCHEMA_VERSION,
                collected_at_unix: now,
                refresh_interval_secs: 1,
                hostname: String::new(),
                macjet_version: env!("CARGO_PKG_VERSION"),
                capabilities: McpCapabilities {
                    powermetrics: false,
                    chrome_cdp: false,
                    ml_predictor: false,
                },
            },
            system: SystemSnapshot {
                hostname: String::new(),
                cpu_brand: String::new(),
                cpu_count_physical: 0,
                cpu_percent: 0.0,
                mem_total_gb: 0.0,
                mem_used_gb: 0.0,
                mem_percent: 0.0,
                swap_total_gb: 0.0,
                swap_used_gb: 0.0,
            },
            thermal: ThermalInfo::default(),
            groups: Vec::new(),
            network: NetSnapshot::default(),
            chrome_tabs: Vec::new(),
            chrome_cdp_available: false,
            energy: EnergySnapshot::default(),
            energy_top: Vec::new(),
            predictor_stats: None,
        }
    }
}

fn top_energy_rows(processes: &rustc_hash::FxHashMap<u32, EnergyInfo>, limit: usize) -> Vec<EnergyInfoRow> {
    let mut v: Vec<&EnergyInfo> = processes.values().collect();
    v.sort_by(|a, b| {
        b.wakeups_per_s
            .partial_cmp(&a.wakeups_per_s)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v.into_iter()
        .take(limit)
        .map(|e| EnergyInfoRow {
            pid: e.pid,
            name: e.name.to_string(),
            wakeups_per_s: e.wakeups_per_s,
            energy_impact: e.energy_impact,
            cpu_ms_per_s: e.cpu_ms_per_s,
        })
        .collect()
}

/// Build snapshot after a collector step.
pub fn build_mcp_snapshot(
    system: &SystemSnapshot,
    thermal: ThermalInfo,
    groups: &[ProcessGroup],
    network: &NetSnapshot,
    chrome_tabs: &[TabEntry],
    chrome_cdp_available: bool,
    energy: &EnergySnapshot,
    predictor_stats: Option<crate::collectors::cpu_predictor::PredictorStats>,
    refresh_interval_secs: u64,
    powermetrics: bool,
    ml_enabled: bool,
) -> McpSnapshot {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let energy_top = top_energy_rows(&energy.processes, 64);
    McpSnapshot {
        meta: McpMeta {
            schema_version: MCP_SCHEMA_VERSION,
            collected_at_unix: now,
            refresh_interval_secs,
            hostname: system.hostname.clone(),
            macjet_version: env!("CARGO_PKG_VERSION"),
            capabilities: McpCapabilities {
                powermetrics,
                chrome_cdp: chrome_cdp_available,
                ml_predictor: ml_enabled,
            },
        },
        system: system.clone(),
        thermal,
        groups: groups.to_vec(),
        network: network.clone(),
        chrome_tabs: chrome_tabs.to_vec(),
        chrome_cdp_available,
        energy: energy.clone(),
        energy_top,
        predictor_stats,
    }
}

pub fn wrap<T: Serialize>(snapshot: &McpSnapshot, data: T) -> serde_json::Value {
    serde_json::json!({
        "meta": snapshot.meta,
        "data": serde_json::to_value(&data).unwrap_or_default(),
    })
}

/// Map process group to MCP process list row.
pub fn group_to_summary(g: &ProcessGroup) -> crate::mcp::models::ProcessSummary {
    let top_pid = g
        .processes
        .iter()
        .max_by(|a, b| {
            a.cpu_percent
                .partial_cmp(&b.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|p| p.pid)
        .unwrap_or(0);
    crate::mcp::models::ProcessSummary {
        name: g.name.to_string(),
        pid_count: g.processes.len(),
        top_pid,
        total_cpu: g.total_cpu,
        total_memory_mb: g.total_memory_mb,
        energy_impact: g.energy_impact.to_string(),
        context_label: g.context_label.to_string(),
    }
}

pub fn process_to_child(p: &ProcessInfo, include_cmdline: bool) -> crate::mcp::models::ChildProcess {
    let cmdline = if include_cmdline {
        p.cmdline
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        String::new()
    };
    crate::mcp::models::ChildProcess {
        pid: p.pid,
        name: p.name.to_string(),
        cpu_percent: p.cpu_percent,
        memory_mb: p.memory_mb,
        threads: p.num_threads as usize,
        cmdline,
    }
}

pub fn group_to_detail(g: &ProcessGroup, include_cmdline: bool) -> crate::mcp::models::ProcessDetail {
    let children: Vec<_> = g
        .processes
        .iter()
        .map(|p| process_to_child(p, include_cmdline))
        .collect();
    let chrome_tabs = chrome_tabs_for_group(g);
    crate::mcp::models::ProcessDetail {
        name: g.name.to_string(),
        total_cpu: g.total_cpu,
        total_memory_mb: g.total_memory_mb,
        process_count: g.processes.len(),
        children,
        chrome_tabs,
        why_hot: g.why_hot.to_string(),
    }
}

fn chrome_tabs_for_group(g: &ProcessGroup) -> Vec<crate::mcp::models::ChromeTab> {
    let mut out = Vec::new();
    let name_lower = g.name.to_lowercase();
    let is_chrome = name_lower.contains("chrome")
        || name_lower.contains("brave")
        || name_lower.contains("arc");
    if !is_chrome {
        return out;
    }
    let mut rank = 0usize;
    for p in &g.processes {
        let pname = p.name.to_lowercase();
        if !pname.contains("renderer") {
            continue;
        }
        let is_renderer = p
            .cmdline
            .iter()
            .any(|arg| arg.as_str() == "--type=renderer");
        if !is_renderer {
            continue;
        }
        rank += 1;
        let title = p.context_label.to_string();
        out.push(crate::mcp::models::ChromeTab {
            rank,
            title: title.trim().to_string(),
            url: String::new(),
            domain: String::new(),
            renderer_pid: p.pid,
            cpu_time_s: p.cpu_percent as f64 / 100.0,
        });
    }
    out
}

pub fn find_group_by_name<'a>(groups: &'a [ProcessGroup], name: &str) -> Option<&'a ProcessGroup> {
    let n = name.trim();
    let n_lower = n.to_lowercase();
    groups
        .iter()
        .find(|g| g.name.as_str().eq_ignore_ascii_case(n))
        .or_else(|| {
            groups.iter().find(|g| {
                g.name
                    .to_lowercase()
                    .contains(n_lower.as_str())
            })
        })
}

pub fn find_group_by_pid<'a>(groups: &'a [ProcessGroup], pid: u32) -> Option<&'a ProcessGroup> {
    groups.iter().find(|g| g.processes.iter().any(|p| p.pid == pid))
}

pub fn sorted_groups(
    groups: &[ProcessGroup],
    sort: &str,
    filter: &str,
    include_system: bool,
) -> Vec<ProcessGroup> {
    let mut v: Vec<ProcessGroup> = groups
        .iter()
        .filter(|g| {
            if !include_system && g.processes.iter().any(|p| p.is_system) {
                return false;
            }
            if filter.is_empty() {
                return true;
            }
            let f = filter.to_lowercase();
            g.name.to_lowercase().contains(&f)
                || g.processes
                    .iter()
                    .any(|p| p.name.to_lowercase().contains(&f))
        })
        .cloned()
        .collect();
    match sort {
        "mem" | "memory" => {
            v.sort_by(|a, b| {
                b.total_memory_mb
                    .partial_cmp(&a.total_memory_mb)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        "name" => {
            v.sort_by(|a, b| a.name.cmp(&b.name));
        }
        _ => {
            v.sort_by(|a, b| {
                b.total_cpu
                    .partial_cmp(&a.total_cpu)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }
    v
}

pub fn explain_heat(
    snapshot: &McpSnapshot,
    focus_pid: Option<u32>,
    _metrics: &MetricsHistory,
) -> crate::mcp::models::HeatExplanation {
    let groups = &snapshot.groups;
    let mut sorted: Vec<_> = groups.iter().collect();
    sorted.sort_by(|a, b| {
        b.total_cpu
            .partial_cmp(&a.total_cpu)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let primary = focus_pid
        .and_then(|pid| find_group_by_pid(groups, pid))
        .or_else(|| sorted.first().copied());
    let primary_name = primary
        .map(|g| g.name.to_string())
        .unwrap_or_else(|| "unknown".into());
    let primary_cpu = primary.map(|g| g.total_cpu).unwrap_or(0.0);
    let secondary: Vec<String> = sorted
        .iter()
        .skip(1)
        .take(5)
        .map(|g| g.name.to_string())
        .collect();
    let cpu_pct = snapshot.system.cpu_percent;
    let severity = if cpu_pct > 80.0 {
        "critical"
    } else if cpu_pct > 50.0 {
        "high"
    } else if cpu_pct > 25.0 {
        "elevated"
    } else {
        "normal"
    };
    let mut recommendations = vec![
        "Call list_process_groups with sort=cpu to see current consumers.".to_string(),
        "Check thermal_pressure in get_system_overview when powermetrics is available.".to_string(),
    ];
    if snapshot.meta.capabilities.chrome_cdp {
        recommendations.push("Use list_chrome_tabs to correlate heavy Chrome renderer CPU with tabs.".to_string());
    }
    let detailed = format!(
        "System CPU {:.1}%, memory {:.1}% used. Top group: {} at {:.1}% CPU. Thermal: {}.",
        cpu_pct,
        snapshot.system.mem_percent,
        primary_name,
        primary_cpu,
        snapshot.thermal.thermal_pressure
    );
    crate::mcp::models::HeatExplanation {
        severity: severity.to_string(),
        cpu_percent: cpu_pct,
        primary_culprit: primary_name.clone(),
        primary_cpu_percent: primary_cpu,
        secondary_culprits: secondary,
        recommendations,
        detailed_report: detailed,
    }
}

pub fn network_report_from_snapshot(
    snapshot: &McpSnapshot,
    top_n: usize,
) -> crate::mcp::models::NetworkReport {
    let mut entries: Vec<crate::mcp::models::NetworkEntry> = snapshot
        .groups
        .iter()
        .flat_map(|g| g.processes.iter().map(move |p| (g.name.clone(), p)))
        .map(|(gname, p)| crate::mcp::models::NetworkEntry {
            pid: p.pid,
            name: format!("{} / {}", gname, p.name),
            bytes_in: p.net_bytes_recv,
            bytes_out: p.net_bytes_sent,
        })
        .collect();
    entries.sort_by(|a, b| (b.bytes_in + b.bytes_out).cmp(&(a.bytes_in + a.bytes_out)));
    entries.truncate(top_n.max(1));
    crate::mcp::models::NetworkReport {
        available: true,
        entries,
        system_bytes_in_per_s: snapshot.network.bytes_recv_per_s,
        system_bytes_out_per_s: snapshot.network.bytes_sent_per_s,
    }
}

pub fn energy_report_from_snapshot(
    snapshot: &McpSnapshot,
    limit: usize,
) -> crate::mcp::models::EnergyReport {
    let powermetrics = snapshot.meta.capabilities.powermetrics;
    if !powermetrics {
        return crate::mcp::models::EnergyReport {
            available: false,
            unavailable_reason: "Powermetrics requires root; run macjet with appropriate privileges for detailed energy rows.".to_string(),
            entries: vec![],
        };
    }
    let entries: Vec<crate::mcp::models::EnergyEntry> = snapshot
        .energy_top
        .iter()
        .take(limit.max(1))
        .map(|e| crate::mcp::models::EnergyEntry {
            pid: e.pid,
            command: e.name.clone(),
            interrupt_wakeups_per_sec: e.wakeups_per_s,
        })
        .collect();
    crate::mcp::models::EnergyReport {
        available: true,
        unavailable_reason: String::new(),
        entries,
    }
}

pub fn chrome_tabs_result(snapshot: &McpSnapshot) -> crate::mcp::models::ChromeTabsResult {
    let tabs: Vec<crate::mcp::models::ChromeTab> = snapshot
        .chrome_tabs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let domain = domain_from_url(&t.url);
            crate::mcp::models::ChromeTab {
                rank: i + 1,
                title: t.title.clone(),
                url: t.url.clone(),
                domain,
                renderer_pid: t.renderer_pid,
                cpu_time_s: 0.0,
            }
        })
        .collect();
    let total = tabs.len();
    crate::mcp::models::ChromeTabsResult {
        tabs,
        total_tabs: total,
        cdp_connected: snapshot.chrome_cdp_available,
    }
}

fn domain_from_url(url: &str) -> String {
    if let Some(start) = url.find("://") {
        let rest = &url[start + 3..];
        if let Some(end) = rest.find('/') {
            return rest[..end].to_string();
        }
        return rest.to_string();
    }
    url.chars().take(64).collect()
}

pub fn system_overview_extended(
    snapshot: &McpSnapshot,
    include_swap: bool,
    include_thermal: bool,
) -> crate::mcp::models::SystemOverviewExtended {
    let t = &snapshot.thermal;
    crate::mcp::models::SystemOverviewExtended {
        cpu_percent: snapshot.system.cpu_percent,
        memory_used_gb: snapshot.system.mem_used_gb,
        memory_total_gb: snapshot.system.mem_total_gb,
        memory_percent: snapshot.system.mem_percent,
        swap_total_gb: if include_swap {
            snapshot.system.swap_total_gb
        } else {
            0.0
        },
        swap_used_gb: if include_swap {
            snapshot.system.swap_used_gb
        } else {
            0.0
        },
        cpu_brand: snapshot.system.cpu_brand.clone(),
        cpu_count_physical: snapshot.system.cpu_count_physical,
        thermal_pressure: if include_thermal {
            t.thermal_pressure.to_string()
        } else {
            String::new()
        },
        fan_rpm: if include_thermal {
            Some(t.fan_speed_rpm)
        } else {
            None
        },
        fan_rpm_max: if include_thermal {
            Some(t.fan_speed_max)
        } else {
            None
        },
        cpu_die_temp_c: if include_thermal {
            t.cpu_die_temp
        } else {
            0.0
        },
        gpu_die_temp_c: if include_thermal {
            t.gpu_die_temp
        } else {
            0.0
        },
        gpu_active_percent: if include_thermal {
            t.gpu_active_percent
        } else {
            0.0
        },
        top_process: snapshot
            .groups
            .iter()
            .max_by(|a, b| {
                a.total_cpu
                    .partial_cmp(&b.total_cpu)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|g| g.name.to_string())
            .unwrap_or_default(),
        top_cpu_percent: snapshot
            .groups
            .iter()
            .map(|g| g.total_cpu)
            .fold(0.0f64, f64::max),
        process_count: snapshot.groups.iter().map(|g| g.processes.len()).sum(),
        verdict: verdict_for(snapshot),
    }
}

fn verdict_for(snapshot: &McpSnapshot) -> String {
    let c = snapshot.system.cpu_percent;
    let m = snapshot.system.mem_percent;
    if c > 85.0 || m > 92.0 {
        "Heavy load — inspect top CPU/memory groups.".to_string()
    } else if c > 50.0 || m > 80.0 {
        "Elevated — worth checking reclaim candidates.".to_string()
    } else {
        "OK".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collectors::metrics_history::MetricsHistory;

    fn sample_group(name: &str, cpu: f64) -> ProcessGroup {
        ProcessGroup {
            name: SmolStr::new(name),
            icon: "🟢",
            total_cpu: cpu,
            total_memory_mb: 100.0,
            total_net_recv: 0,
            total_net_sent: 0,
            energy_impact: SmolStr::new("LOW"),
            processes: vec![],
            context_label: SmolStr::new(""),
            confidence: SmolStr::new(""),
            why_hot: SmolStr::new(""),
            is_expanded: false,
        }
    }

    #[test]
    fn sorted_groups_cpu_order() {
        let g = vec![
            sample_group("a", 10.0),
            sample_group("b", 50.0),
        ];
        let s = sorted_groups(&g, "cpu", "", true);
        assert_eq!(s[0].name.as_str(), "b");
    }

    #[test]
    fn find_group_substring() {
        let g = vec![sample_group("Google Chrome", 5.0)];
        assert!(find_group_by_name(&g, "chrome").is_some());
    }

    #[test]
    fn wrap_json_includes_meta_schema_version() {
        let snap = McpSnapshot::default();
        let v = wrap(&snap, serde_json::json!({"x": 1}));
        assert_eq!(v["meta"]["schema_version"], MCP_SCHEMA_VERSION);
        assert_eq!(v["data"]["x"], 1);
    }

    #[test]
    fn explain_heat_has_keys() {
        let mut snap = McpSnapshot::default();
        snap.groups = vec![sample_group("App", 40.0)];
        snap.system.cpu_percent = 55.0;
        let m = MetricsHistory::new();
        let h = explain_heat(&snap, None, &m);
        assert!(!h.primary_culprit.is_empty());
        assert!(!h.severity.is_empty());
    }
}
