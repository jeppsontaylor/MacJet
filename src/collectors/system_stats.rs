/// MacJet — System Stats Collector
///
/// Uses the `sysinfo` crate to gather CPU%, memory, swap, hostname.
/// Designed to be called from a background tokio task on a 1s tick.
use serde::Serialize;
use sysinfo::System;

#[derive(Debug, Clone, Serialize)]
pub struct SystemSnapshot {
    pub hostname: String,
    pub cpu_brand: String,
    pub cpu_count_physical: usize,
    pub cpu_percent: f64,
    pub mem_total_gb: f64,
    pub mem_used_gb: f64,
    pub mem_percent: f64,
    pub swap_total_gb: f64,
    pub swap_used_gb: f64,
}

pub struct SystemCollector {
    pub sys: System,
    hostname: String,
    cpu_brand: String,
    cpu_count_physical: usize,
}

impl SystemCollector {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();

        let hostname = System::host_name().unwrap_or_else(|| "unknown".into());
        let cpu_brand = sys
            .cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_default();
        let cpu_count_physical = sys.cpus().len();

        Self {
            sys,
            hostname,
            cpu_brand,
            cpu_count_physical,
        }
    }

    pub fn collect(&mut self) -> SystemSnapshot {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();

        let cpu_percent: f64 = {
            let cpus = self.sys.cpus();
            if cpus.is_empty() {
                0.0
            } else {
                cpus.iter().map(|c| c.cpu_usage() as f64).sum::<f64>() / cpus.len() as f64
            }
        };

        let mem_total = self.sys.total_memory() as f64;
        let mem_used = self.sys.used_memory() as f64;
        let swap_total = self.sys.total_swap() as f64;
        let swap_used = self.sys.used_swap() as f64;

        let gb = 1024.0 * 1024.0 * 1024.0;

        SystemSnapshot {
            hostname: self.hostname.clone(),
            cpu_brand: self.cpu_brand.clone(),
            cpu_count_physical: self.cpu_count_physical,
            cpu_percent,
            mem_total_gb: mem_total / gb,
            mem_used_gb: mem_used / gb,
            mem_percent: if mem_total > 0.0 {
                (mem_used / mem_total) * 100.0
            } else {
                0.0
            },
            swap_total_gb: swap_total / gb,
            swap_used_gb: swap_used / gb,
        }
    }
}
