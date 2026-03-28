/// Headless collector loop for MCP mode (mirrors `AppState::tick` without TUI).
use crate::collectors::chrome_enricher::ChromeTabEnricher;
use crate::collectors::cpu_predictor::CpuPredictor;
use crate::collectors::energy_collector::EnergyCollector;
use crate::collectors::metrics_history::MetricsHistory;
use crate::collectors::network_collector::NetworkCollector;
use crate::collectors::process_collector::ProcessCollector;
use crate::collectors::system_stats::{SystemCollector, SystemSnapshot};
use crate::mcp::snapshot::{build_mcp_snapshot, McpSnapshot};
use rmcp::model::ResourceUpdatedNotificationParam;
use rmcp::service::Peer;
use rmcp::RoleServer;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, Duration};

pub struct McpCollectorState {
    system_collector: SystemCollector,
    pub system: SystemSnapshot,
    pub process_collector: ProcessCollector,
    network_collector: NetworkCollector,
    energy_collector: EnergyCollector,
    pub metrics_history: MetricsHistory,
    chrome_enricher: ChromeTabEnricher,
    pub cpu_predictor: CpuPredictor,
    ml_enabled: bool,
    tick_count: u64,
}

impl McpCollectorState {
    pub fn new(ml_enabled: bool) -> Self {
        let mut energy = EnergyCollector::new();
        energy.start();

        let mut system_collector = SystemCollector::new();
        let system = system_collector.collect();

        Self {
            system,
            system_collector,
            process_collector: ProcessCollector::new(),
            network_collector: NetworkCollector::new(),
            energy_collector: energy,
            metrics_history: MetricsHistory::new(),
            chrome_enricher: ChromeTabEnricher::new(),
            cpu_predictor: CpuPredictor::new(),
            ml_enabled,
            tick_count: 0,
        }
    }

    pub fn step(&mut self) {
        self.tick_count += 1;
        self.system = self.system_collector.collect();

        if self.ml_enabled {
            self.cpu_predictor.push_sample(self.system.cpu_percent);
            if self.cpu_predictor.should_train() {
                self.cpu_predictor.try_train();
            }
        }

        let (processes, _) = self
            .process_collector
            .collect_sync(&mut self.system_collector.sys);

        self.network_collector.collect();

        let now_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        self.metrics_history.set_time(now_s);

        for p in processes {
            self.metrics_history
                .record(p.pid, p.cpu_percent, p.memory_mb);
        }
        self.metrics_history.expire_stale();

        if self.chrome_enricher.should_refresh() {
            self.chrome_enricher.refresh();
        }
        {
            let groups = self.process_collector.groups_mut();
            self.chrome_enricher.enrich_groups(groups);
        }
    }

    pub fn build_snapshot(&self, refresh_secs: u64) -> McpSnapshot {
        let energy_snap = self.energy_collector.snapshot();
        let thermal = energy_snap.thermal.clone();
        let powermetrics = self.energy_collector.has_sudo();
        let groups = self.process_collector.groups();
        let tabs = self.chrome_enricher.tabs_cloned();
        let cdp = self.chrome_enricher.available;
        let pred = if self.ml_enabled {
            Some(self.cpu_predictor.stats())
        } else {
            None
        };
        build_mcp_snapshot(
            &self.system,
            thermal,
            groups,
            &self.network_collector.latest,
            &tabs,
            cdp,
            &energy_snap,
            pred,
            refresh_secs,
            powermetrics,
            self.ml_enabled,
        )
    }
}

/// Run periodic collection and update `snapshot`; optionally notify MCP subscribers.
pub fn spawn_collector_loop(
    collector: Arc<Mutex<McpCollectorState>>,
    snapshot: Arc<RwLock<McpSnapshot>>,
    refresh_secs: u64,
    peer: Arc<RwLock<Option<Peer<RoleServer>>>>,
    subscriptions: Arc<Mutex<HashSet<String>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(refresh_secs.max(1)));
        loop {
            tick.tick().await;
            {
                let mut c = collector.lock().await;
                c.step();
                *snapshot.write().await = c.build_snapshot(refresh_secs);
            }
            let subs: Vec<String> = {
                let g = subscriptions.lock().await;
                g.iter().cloned().collect()
            };
            if subs.is_empty() {
                continue;
            }
            let peer_opt = peer.read().await.clone();
            if let Some(p) = peer_opt {
                for uri in subs {
                    let _ = p
                        .notify_resource_updated(ResourceUpdatedNotificationParam { uri })
                        .await;
                }
            }
        }
    })
}
