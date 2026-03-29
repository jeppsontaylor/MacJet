/// Resource payload builders (shared with MCP tools; URI dispatch lives in `server.rs`).
use crate::mcp::models::ProcessListResult;
use crate::mcp::models::ReclaimCandidateMcp;
use crate::mcp::models::ReclaimListResult;
use crate::mcp::safety;
use crate::mcp::snapshot::{
    chrome_tabs_result, energy_report_from_snapshot, find_group_by_name, find_group_by_pid,
    network_report_from_snapshot, sorted_groups, system_overview_extended, wrap,
};
use crate::mcp::snapshot::{group_to_detail, group_to_summary, McpSnapshot};

pub fn json_system_overview(
    snap: &McpSnapshot,
    include_swap: bool,
    include_thermal: bool,
) -> String {
    let data = system_overview_extended(snap, include_swap, include_thermal);
    serde_json::to_string(&wrap(snap, data)).unwrap_or_default()
}

pub fn json_processes_top(snap: &McpSnapshot, limit: usize, _include_cmdline: bool) -> String {
    let sorted = sorted_groups(&snap.groups, "cpu", "", true);
    let total = sorted.len();
    let groups: Vec<_> = sorted
        .iter()
        .take(limit)
        .map(|g| group_to_summary(g))
        .collect();
    let data = ProcessListResult {
        total_groups: total,
        groups,
        sort_by: "cpu".to_string(),
        filter_applied: String::new(),
    };
    serde_json::to_string(&wrap(snap, data)).unwrap_or_default()
}

pub fn json_process_group(snap: &McpSnapshot, name: &str, include_cmdline: bool) -> Option<String> {
    let g = find_group_by_name(&snap.groups, name)?;
    let data = group_to_detail(g, include_cmdline);
    Some(serde_json::to_string(&wrap(snap, data)).unwrap_or_default())
}

pub fn json_process_pid(snap: &McpSnapshot, pid: u32, include_cmdline: bool) -> Option<String> {
    let g = find_group_by_pid(&snap.groups, pid)?;
    let data = group_to_detail(g, include_cmdline);
    Some(serde_json::to_string(&wrap(snap, data)).unwrap_or_default())
}

pub fn json_network(snap: &McpSnapshot, top_n: usize) -> String {
    let data = network_report_from_snapshot(snap, top_n);
    serde_json::to_string(&wrap(snap, data)).unwrap_or_default()
}

pub fn json_energy(snap: &McpSnapshot, limit: usize) -> String {
    let data = energy_report_from_snapshot(snap, limit);
    serde_json::to_string(&wrap(snap, data)).unwrap_or_default()
}

pub fn json_chrome(snap: &McpSnapshot) -> String {
    let data = chrome_tabs_result(snap);
    serde_json::to_string(&wrap(snap, data)).unwrap_or_default()
}

pub fn json_audit(limit: usize) -> String {
    safety::get_audit_log(limit)
}

/// Audit log wrapped like other resources (valid JSON object).
pub fn json_audit_wrapped(snap: &McpSnapshot, limit: usize) -> String {
    serde_json::to_string(&serde_json::json!({
        "meta": snap.meta,
        "data": { "log_text": json_audit(limit) }
    }))
    .unwrap_or_default()
}

pub fn json_reclaim(
    snap: &McpSnapshot,
    raw: Vec<crate::collectors::metrics_history::ReclaimCandidate>,
    min_score: u8,
    limit: usize,
) -> String {
    let total = raw.len();
    let candidates: Vec<ReclaimCandidateMcp> = raw
        .into_iter()
        .filter(|c| c.score >= min_score)
        .take(limit)
        .map(|c| ReclaimCandidateMcp {
            group_key: c.group_key.to_string(),
            app_name: c.app_name.to_string(),
            icon: c.icon.to_string(),
            score: c.score,
            reclaim_cpu: c.reclaim_cpu,
            reclaim_mem_mb: c.reclaim_mem_mb,
            risk: c.risk.to_string(),
            reason: c.reason.to_string(),
            suggested_action: c.suggested_action.to_string(),
            child_count: c.child_count,
            is_hidden: c.is_hidden,
            launch_age_s: c.launch_age_s,
        })
        .collect();
    let data = ReclaimListResult {
        candidates,
        total_considered: total,
    };
    serde_json::to_string(&wrap(snap, data)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processes_top_json_has_meta() {
        let snap = McpSnapshot::default();
        let s = json_processes_top(&snap, 10, false);
        assert!(s.contains("meta"));
        assert!(s.contains("data"));
    }

    #[test]
    fn json_reclaim_serializes_candidates() {
        let snap = McpSnapshot::default();
        let raw = vec![];
        let s = json_reclaim(&snap, raw, 0, 10);
        assert!(s.contains("candidates"));
        assert!(s.contains("total_considered"));
    }
}
