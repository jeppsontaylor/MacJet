/// MacJet — Network Collector
/// Delta-sampling of system-wide network I/O via sysinfo.
use crate::collectors::clock::Clock;
use sysinfo::Networks;

use rustc_hash::FxHashMap;
use smol_str::SmolStr;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct InterfaceSnapshot {
    pub name: SmolStr,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub bytes_sent_per_s: f64,
    pub bytes_recv_per_s: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetSnapshot {
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub bytes_sent_per_s: f64,
    pub bytes_recv_per_s: f64,
    pub interfaces: Vec<InterfaceSnapshot>,
    pub timestamp: f64,
}

pub struct NetworkCollector {
    prev_sent: u64,
    prev_recv: u64,
    prev_interfaces: FxHashMap<SmolStr, (u64, u64)>,
    prev_time: f64,
    pub latest: NetSnapshot,
    networks: Networks,
}

impl Default for NetworkCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkCollector {
    pub fn new() -> Self {
        Self {
            prev_sent: 0,
            prev_recv: 0,
            prev_interfaces: FxHashMap::default(),
            prev_time: 0.0,
            latest: NetSnapshot::default(),
            networks: Networks::new_with_refreshed_list(),
        }
    }

    pub fn collect(&mut self) -> &NetSnapshot {
        self.networks.refresh(true);

        let now = crate::collectors::clock::SystemClock::default().now();
        let mut total_sent = 0;
        let mut total_recv = 0;
        for (_, data) in &self.networks {
            total_sent += data.total_transmitted();
            total_recv += data.total_received();
        }

        self.collect_internal(now, total_sent, total_recv)
    }

    pub fn collect_internal(&mut self, now: f64, total_sent: u64, total_recv: u64) -> &NetSnapshot {
        let mut dt = now - self.prev_time;
        if self.prev_time == 0.0 || dt <= 0.0 {
            dt = 1.0;
        }

        let mut interface_snapshots = Vec::new();

        for (name, data) in &self.networks {
            let sent = data.total_transmitted();
            let recv = data.total_received();

            let name_str = SmolStr::new(name);
            let (p_sent, p_recv) = self
                .prev_interfaces
                .get(&name_str)
                .cloned()
                .unwrap_or((sent, recv));

            let s_per_s = (sent.saturating_sub(p_sent)) as f64 / dt;
            let r_per_s = (recv.saturating_sub(p_recv)) as f64 / dt;

            interface_snapshots.push(InterfaceSnapshot {
                name: name_str.clone(),
                bytes_sent: sent,
                bytes_recv: recv,
                bytes_sent_per_s: s_per_s,
                bytes_recv_per_s: r_per_s,
            });

            self.prev_interfaces.insert(name_str, (sent, recv));
        }

        // Sort interfaces by throughput
        interface_snapshots.sort_by(|a, b| {
            let a_total = a.bytes_sent_per_s + a.bytes_recv_per_s;
            let b_total = b.bytes_sent_per_s + b.bytes_recv_per_s;
            b_total
                .partial_cmp(&a_total)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let sent_per_s = if self.prev_sent > 0 {
            (total_sent.saturating_sub(self.prev_sent)) as f64 / dt
        } else {
            0.0
        };

        let recv_per_s = if self.prev_recv > 0 {
            (total_recv.saturating_sub(self.prev_recv)) as f64 / dt
        } else {
            0.0
        };

        self.latest = NetSnapshot {
            bytes_sent: total_sent,
            bytes_recv: total_recv,
            bytes_sent_per_s: sent_per_s,
            bytes_recv_per_s: recv_per_s,
            interfaces: interface_snapshots,
            timestamp: now,
        };

        self.prev_sent = total_sent;
        self.prev_recv = total_recv;
        self.prev_time = now;

        &self.latest
    }
}

pub fn format_bytes_per_s(bps: f64) -> String {
    if bps >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} GB/s", bps / (1024.0_f64.powi(3)))
    } else if bps >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bps / (1024.0_f64.powi(2)))
    } else if bps >= 1024.0 {
        format!("{:.1} KB/s", bps / 1024.0)
    } else {
        format!("{:.0} B/s", bps)
    }
}

pub fn format_bytes(b: f64) -> String {
    if b >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} GB", b / (1024.0_f64.powi(3)))
    } else if b >= 1024.0 * 1024.0 {
        format!("{:.1} MB", b / (1024.0_f64.powi(2)))
    } else if b >= 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else {
        format!("{:.0} B", b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes_per_s() {
        assert_eq!(format_bytes_per_s(500.0), "500 B/s");
        assert_eq!(format_bytes_per_s(1024.0), "1.0 KB/s");
        assert_eq!(format_bytes_per_s(1500.0), "1.5 KB/s");
        assert_eq!(format_bytes_per_s(1024.0 * 1024.0), "1.0 MB/s");
        assert_eq!(format_bytes_per_s(1.5 * 1024.0 * 1024.0), "1.5 MB/s");
        assert_eq!(format_bytes_per_s(1024.0 * 1024.0 * 1024.0), "1.0 GB/s");
        assert_eq!(
            format_bytes_per_s(2.3 * 1024.0 * 1024.0 * 1024.0),
            "2.3 GB/s"
        );
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500.0), "500 B");
        assert_eq!(format_bytes(1024.0), "1.0 KB");
        assert_eq!(format_bytes(1500.0), "1.5 KB");
        assert_eq!(format_bytes(1024.0 * 1024.0), "1.0 MB");
        assert_eq!(format_bytes(1.5 * 1024.0 * 1024.0), "1.5 MB");
        assert_eq!(format_bytes(1024.0 * 1024.0 * 1024.0), "1.0 GB");
        assert_eq!(format_bytes(2.3 * 1024.0 * 1024.0 * 1024.0), "2.3 GB");
    }

    #[test]
    fn test_network_collector_first_run() {
        let mut collector = NetworkCollector::new();

        let snapshot = collector.collect_internal(1000.0, 10000, 20000).clone();

        assert_eq!(snapshot.bytes_sent, 10000);
        assert_eq!(snapshot.bytes_recv, 20000);
        assert_eq!(snapshot.bytes_sent_per_s, 0.0);
        assert_eq!(snapshot.bytes_recv_per_s, 0.0);
        assert_eq!(snapshot.timestamp, 1000.0);

        assert_eq!(collector.latest, snapshot);
    }

    #[test]
    fn test_network_collector_second_run() {
        let mut collector = NetworkCollector::new();

        // First run
        collector.collect_internal(1000.0, 10000, 20000);

        // Second run (+2s)
        let snapshot = collector.collect_internal(1002.0, 11000, 24000).clone();

        assert_eq!(snapshot.bytes_sent, 11000);
        assert_eq!(snapshot.bytes_recv, 24000);
        // Sent 1000 over 2s -> 500 B/s
        assert_eq!(snapshot.bytes_sent_per_s, 500.0);
        // Recv 4000 over 2s -> 2000 B/s
        assert_eq!(snapshot.bytes_recv_per_s, 2000.0);
        assert_eq!(snapshot.timestamp, 1002.0);
    }

    #[test]
    fn test_network_collector_fast_dt_fix() {
        let mut collector = NetworkCollector::new();

        // First run
        collector.collect_internal(1000.0, 10000, 20000);

        // Second run immediately dt=0
        let snapshot = collector.collect_internal(1000.0, 11000, 24000).clone();

        // dt=0 floors to 1.0s, so diffs over 1
        assert_eq!(snapshot.bytes_sent_per_s, 1000.0);
        assert_eq!(snapshot.bytes_recv_per_s, 4000.0);
    }
}
