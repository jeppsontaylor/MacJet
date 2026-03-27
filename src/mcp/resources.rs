/// MacJet MCP — Resource Endpoints
/// Mapping domain logic for exposing application state to RMCP
use crate::mcp::models::{
    ChromeTabsResult, EnergyReport, ProcessDetail, ProcessListResult, SystemOverview,
};

pub async fn resource_system_overview() -> String {
    let mock_model = SystemOverview {
        cpu_percent: 10.0,
        memory_used_gb: 8.0,
        memory_total_gb: 16.0,
        memory_percent: 50.0,
        thermal_pressure: "nominal".to_string(),
        fan_rpm: None,
        top_process: "foo".to_string(),
        top_cpu_percent: 5.0,
        process_count: 100,
        verdict: "OK".to_string(),
    };
    serde_json::to_string(&mock_model).unwrap_or_default()
}

pub async fn resource_processes_top() -> String {
    let mock_model = ProcessListResult {
        groups: vec![],
        total_groups: 0,
        sort_by: "cpu".to_string(),
        filter_applied: "".to_string(),
    };
    serde_json::to_string(&mock_model).unwrap_or_default()
}

pub async fn resource_process_by_name(name: &str) -> String {
    let mock_model = ProcessDetail {
        name: name.to_string(),
        total_cpu: 5.0,
        total_memory_mb: 100.0,
        process_count: 1,
        children: vec![],
        chrome_tabs: vec![],
        why_hot: "".to_string(),
    };
    serde_json::to_string(&mock_model).unwrap_or_default()
}

pub async fn resource_chrome_tabs() -> String {
    let mock_model = ChromeTabsResult {
        tabs: vec![],
        total_tabs: 0,
        cdp_connected: true,
    };
    serde_json::to_string(&mock_model).unwrap_or_default()
}

pub async fn resource_energy_report() -> String {
    let mock_model = EnergyReport {
        available: false,
        entries: vec![],
    };
    serde_json::to_string(&mock_model).unwrap_or_default()
}

pub async fn resource_audit_log() -> String {
    crate::mcp::safety::get_audit_log(50)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_resource_system_overview() {
        let result = resource_system_overview().await;
        assert!(result.contains("cpu_percent"));
        assert!(result.contains("10.0"));
    }

    #[tokio::test]
    async fn test_resource_processes_top() {
        let result = resource_processes_top().await;
        assert!(result.contains("groups"));
    }

    #[tokio::test]
    async fn test_resource_process_by_name() {
        let result = resource_process_by_name("testapp").await;
        assert!(result.contains("testapp"));
    }
}
