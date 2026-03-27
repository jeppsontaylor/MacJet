/// MacJet — Container Inspector
/// Docker / OrbStack / Colima container stats integration.
use serde::Deserialize;
use smol_str::SmolStr;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContainerInfo {
    pub name: SmolStr,
    pub container_id: SmolStr,
    pub image: SmolStr,
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub memory_limit_mb: f64,
    pub net_input: SmolStr,
    pub net_output: SmolStr,
    pub status: SmolStr,
}

pub struct ContainerInspector {
    pub docker_available: Option<bool>,
    pub containers: Vec<ContainerInfo>,
}

#[derive(Deserialize)]
struct DockerStat {
    #[serde(default)]
    name: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    cpu: String,
    #[serde(default)]
    mem_usage: String,
    #[serde(default)]
    net: String,
    #[serde(default)]
    status: String,
}

impl Default for ContainerInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerInspector {
    pub fn new() -> Self {
        Self {
            docker_available: None,
            containers: Vec::new(),
        }
    }

    pub async fn inspect(&mut self) -> &[ContainerInfo] {
        if self.docker_available == Some(false) {
            return &self.containers;
        }

        if let Some(containers) = self.query_docker_stats().await {
            self.containers = containers;
            self.docker_available = Some(true);
        } else {
            self.docker_available = Some(false);
            self.containers.clear();
        }

        &self.containers
    }

    #[cfg(not(test))]
    async fn query_docker_stats(&self) -> Option<Vec<ContainerInfo>> {
        let child = Command::new("docker")
            .args([
                "stats",
                "--no-stream",
                "--format",
                r#"{"name":"{{.Name}}","id":"{{.ID}}","cpu":"{{.CPUPerc}}","mem_usage":"{{.MemUsage}}","net":"{{.NetIO}}","status":"running"}"#,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        let result = timeout(Duration::from_secs(5), child.wait_with_output())
            .await
            .ok()?
            .ok()?;

        if (!result.status.success()) || result.stdout.is_empty() {
            return None;
        }

        Some(parse_docker_stats(&result.stdout))
    }

    #[cfg(test)]
    async fn query_docker_stats(&self) -> Option<Vec<ContainerInfo>> {
        tests::get_mock_stats().lock().unwrap().clone()
    }

    pub fn find_container_for_process(&self, process_name: &str) -> Option<&ContainerInfo> {
        let lower = process_name.to_lowercase();
        self.containers.iter().find(|&c| {
            let c_lower = c.name.to_lowercase();
            c_lower.contains(&lower) || lower.contains(&c_lower)
        })
    }
}

pub fn parse_mem(s: &str) -> f64 {
    let s = s.trim();
    if s.contains("GiB") {
        s.replace("GiB", "").trim().parse::<f64>().unwrap_or(0.0) * 1024.0
    } else if s.contains("MiB") {
        s.replace("MiB", "").trim().parse::<f64>().unwrap_or(0.0)
    } else if s.contains("KiB") {
        s.replace("KiB", "").trim().parse::<f64>().unwrap_or(0.0) / 1024.0
    } else if s.contains("B") {
        s.replace("B", "").trim().parse::<f64>().unwrap_or(0.0) / (1024.0 * 1024.0)
    } else {
        0.0
    }
}

pub fn parse_docker_stats(stdout: &[u8]) -> Vec<ContainerInfo> {
    let mut containers = Vec::new();
    let stdout_str = String::from_utf8_lossy(stdout);

    for line in stdout_str.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let Ok(data) = serde_json::from_str::<DockerStat>(line) else {
            continue;
        };

        let cpu = data.cpu.trim_end_matches('%').parse::<f64>().unwrap_or(0.0);

        let mem_parts: Vec<&str> = data.mem_usage.split(" / ").collect();
        let mem_used = if mem_parts.is_empty() {
            0.0
        } else {
            parse_mem(mem_parts[0])
        };

        let net_parts: Vec<&str> = data.net.split(" / ").collect();
        let net_input = if net_parts.is_empty() {
            ""
        } else {
            net_parts[0]
        };
        let net_output = if net_parts.len() > 1 {
            net_parts[1]
        } else {
            ""
        };

        containers.push(ContainerInfo {
            name: SmolStr::new(&data.name),
            container_id: SmolStr::new(&data.id),
            image: SmolStr::new(""),
            cpu_percent: cpu,
            memory_mb: mem_used,
            memory_limit_mb: 0.0,
            net_input: SmolStr::new(net_input),
            net_output: SmolStr::new(net_output),
            status: SmolStr::new(&data.status),
        });
    }

    containers
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    pub static MOCK_STATS: OnceLock<Mutex<Option<Vec<ContainerInfo>>>> = OnceLock::new();

    pub fn get_mock_stats() -> &'static Mutex<Option<Vec<ContainerInfo>>> {
        MOCK_STATS.get_or_init(|| Mutex::new(None))
    }

    fn set_mock_stats(stats: Option<Vec<ContainerInfo>>) {
        *get_mock_stats().lock().unwrap() = stats;
    }

    #[test]
    fn test_parse_mem() {
        assert_eq!(parse_mem("1024.0MiB"), 1024.0);
        assert_eq!(parse_mem("2.5GiB"), 2.5 * 1024.0);
        assert_eq!(parse_mem("512.0KiB"), 512.0 / 1024.0);
        assert_eq!(parse_mem("1048576B"), 1048576.0 / (1024.0 * 1024.0));
        assert_eq!(parse_mem("Invalid"), 0.0);
    }

    #[test]
    fn test_find_container_for_process() {
        let mut inspector = ContainerInspector::new();
        let c1 = ContainerInfo {
            name: SmolStr::new("redis-cache"),
            ..Default::default()
        };
        let c2 = ContainerInfo {
            name: SmolStr::new("postgres-db"),
            ..Default::default()
        };
        inspector.containers = vec![c1.clone(), c2.clone()];

        assert_eq!(inspector.find_container_for_process("redis"), Some(&c1));
        assert_eq!(inspector.find_container_for_process("POSTGRES"), Some(&c2));
        assert_eq!(inspector.find_container_for_process("nginx"), None);

        // Partial match
        assert_eq!(
            inspector
                .find_container_for_process("redis-cache-server")
                .unwrap()
                .name,
            "redis-cache"
        );
    }

    #[tokio::test]
    async fn test_inspect_docker_unavailable() {
        let mut inspector = ContainerInspector::new();
        inspector.docker_available = Some(false);

        let result = inspector.inspect().await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_query_docker_stats_success() {
        let mock_output = vec![
            r#"{"name": "redis", "id": "123", "cpu": "1.5%", "mem_usage": "100MiB / 2GiB", "net": "1kB / 2kB", "status": "running"}"#,
            r#"{"name": "postgres", "id": "456", "cpu": "3.5%", "mem_usage": "500MiB / 4GiB", "net": "10kB / 20kB", "status": "running"}"#,
            "",             // Empty line
            "invalid json", // Bad line
        ];

        let joined = mock_output.join("\n");
        let parsed = parse_docker_stats(joined.as_bytes());
        set_mock_stats(Some(parsed));

        let mut inspector = ContainerInspector::new();
        let containers = inspector.inspect().await.to_vec();

        assert_eq!(containers.len(), 2);
        assert_eq!(inspector.docker_available, Some(true));

        assert_eq!(containers[0].name, "redis");
        assert_eq!(containers[0].cpu_percent, 1.5);
        assert_eq!(containers[0].memory_mb, 100.0);
        assert_eq!(containers[0].net_input, "1kB");
        assert_eq!(containers[0].net_output, "2kB");

        assert_eq!(containers[1].name, "postgres");
        assert_eq!(containers[1].cpu_percent, 3.5);
        assert_eq!(containers[1].memory_mb, 500.0);
    }

    #[tokio::test]
    async fn test_query_docker_stats_failure() {
        set_mock_stats(None); // simulates failure like error code 1
        let mut inspector = ContainerInspector::new();
        inspector.docker_available = None; // initial state

        let containers = inspector.inspect().await;

        assert!(containers.is_empty());
        assert_eq!(inspector.docker_available, Some(false));
    }
}
