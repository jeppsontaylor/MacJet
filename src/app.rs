use crate::collectors::chrome_enricher::ChromeTabEnricher;
use crate::collectors::cpu_predictor::CpuPredictor;
use crate::collectors::energy_collector::EnergyCollector;
use crate::collectors::metrics_history::{MetricsHistory, ReclaimCandidate};
use crate::collectors::network_collector::NetworkCollector;
use crate::collectors::process_collector::{ProcessCollector, ProcessGroup, ProcessInfo};
/// MacJet — Application State
///
/// Central state struct, view enum, tick orchestration,
/// per-view tree states, filter, notifications, and selection context.
use crate::collectors::system_stats::{SystemCollector, SystemSnapshot};
use crate::telemetry::SelfTelemetry;
use crate::ui::notifications::NotificationCenter;
use crate::ui::process_tree::ProcessTreeState;
use ratatui::widgets::TableState as ReclaimTableState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Processes,
    Reclaim,
    Energy,
    Network,
    Predict,
    Help,
}

impl View {
    pub fn label(&self) -> &'static str {
        match self {
            View::Processes => "Processes",
            View::Reclaim => "Kill List",
            View::Energy => "Energy",
            View::Network => "Network",
            View::Predict => "Predict",
            View::Help => "Help",
        }
    }

    pub fn all() -> &'static [View] {
        &[
            View::Processes,
            View::Reclaim,
            View::Energy,
            View::Network,
            View::Predict,
            View::Help,
        ]
    }

    pub fn shortcut(&self) -> &'static str {
        match self {
            View::Processes => "1",
            View::Reclaim => "2",
            View::Energy => "3",
            View::Network => "4",
            View::Predict => "5",
            View::Help => "?",
        }
    }

    /// Cycle to the next view (Tab / Right arrow)
    pub fn next(self) -> View {
        match self {
            View::Processes => View::Reclaim,
            View::Reclaim => View::Energy,
            View::Energy => View::Network,
            View::Network => View::Predict,
            View::Predict => View::Processes,
            View::Help => View::Processes,
        }
    }

    /// Cycle to the previous view (Left arrow)
    pub fn prev(self) -> View {
        match self {
            View::Processes => View::Predict,
            View::Reclaim => View::Processes,
            View::Energy => View::Reclaim,
            View::Network => View::Energy,
            View::Predict => View::Network,
            View::Help => View::Processes,
        }
    }
}

pub struct AppState {
    pub active_view: View,
    pub system: SystemSnapshot,
    pub system_collector: SystemCollector,
    pub process_collector: ProcessCollector,
    pub network_collector: NetworkCollector,
    pub energy_collector: EnergyCollector,
    pub metrics_history: MetricsHistory,
    pub cpu_predictor: CpuPredictor,
    pub chrome_enricher: ChromeTabEnricher,
    pub telemetry: SelfTelemetry,
    pub should_quit: bool,
    pub paused: bool,
    pub interaction_pause_until: f64,
    pub tick_count: u64,

    // --- Per-view tree states (independent scroll/expansion) ---
    pub processes_tree: ProcessTreeState,
    pub energy_tree: ProcessTreeState,
    pub reclaim_state: ReclaimTableState,

    // --- Filter ---
    pub filter_visible: bool,
    pub filter_input: String,

    // --- Notifications ---
    pub notifications: NotificationCenter,

    // --- Selection context (cloned snapshots for inspector panel) ---
    pub selected_process: Option<ProcessInfo>,
    pub selected_group: Option<ProcessGroup>,
    pub selected_reclaim_candidate: Option<ReclaimCandidate>,
    pub selected_reclaim_group: Option<ProcessGroup>,

    /// When `false`, the online CPU predictor (RLS) does not sample or train (`--no-ml`).
    pub ml_enabled: bool,
}

impl AppState {
    /// `ml_enabled`: set `false` to disable CPU prediction sampling/training (for benchmarking).
    pub fn new(ml_enabled: bool) -> Self {
        let mut collector = SystemCollector::new();
        let initial_snapshot = collector.collect();

        let mut energy = EnergyCollector::new();
        energy.start();

        Self {
            active_view: View::Processes,
            system: initial_snapshot,
            system_collector: collector,
            process_collector: ProcessCollector::new(),
            network_collector: NetworkCollector::new(),
            energy_collector: energy,
            metrics_history: MetricsHistory::new(),
            cpu_predictor: CpuPredictor::new(),
            chrome_enricher: ChromeTabEnricher::new(),
            telemetry: SelfTelemetry::new(),
            should_quit: false,
            paused: false,
            interaction_pause_until: 0.0,
            tick_count: 0,

            processes_tree: ProcessTreeState::new(),
            energy_tree: ProcessTreeState::new(),
            reclaim_state: ReclaimTableState::default(),

            filter_visible: false,
            filter_input: String::new(),

            notifications: NotificationCenter::default(),

            selected_process: None,
            selected_group: None,
            selected_reclaim_candidate: None,
            selected_reclaim_group: None,

            ml_enabled,
        }
    }

    /// Called every 1s by the fast-lane tick.
    pub fn tick(&mut self) {
        if self.paused {
            return;
        }
        self.tick_count += 1;

        // Refresh system stats
        self.system = self.system_collector.collect();

        if self.ml_enabled {
            // Feed CPU to predictor (even during interaction pause)
            self.cpu_predictor.push_sample(self.system.cpu_percent);
            if self.cpu_predictor.should_train() {
                self.cpu_predictor.try_train();
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        if now < self.interaction_pause_until {
            // Record telemetry but skip churning processes
            let rss = SelfTelemetry::own_rss_mb();
            let (am_cpu, am_rss) = self.telemetry.activity_monitor_stats();
            self.telemetry
                .record(self.system.cpu_percent, rss, am_cpu, am_rss);
            return;
        }

        // Refresh processes
        let (processes, _) = self
            .process_collector
            .collect_sync(&mut self.system_collector.sys);

        // Refresh network
        self.network_collector.collect();

        // Update metrics
        for p in processes {
            self.metrics_history
                .record(p.pid, p.cpu_percent, p.memory_mb);
        }
        self.metrics_history.expire_stale();

        // Enrich Chrome renderer processes with tab titles
        if self.chrome_enricher.should_refresh() {
            self.chrome_enricher.refresh();
        }
        {
            let groups = self.process_collector.groups_mut();
            self.chrome_enricher.enrich_groups(groups);
        }

        // Record telemetry (our own process + optionally AM)
        let rss = SelfTelemetry::own_rss_mb();
        let (am_cpu, am_rss) = self.telemetry.activity_monitor_stats();
        self.telemetry
            .record(self.system.cpu_percent, rss, am_cpu, am_rss);
    }

    /// Returns the active tree state (mutable) for the current view.
    pub fn active_tree_mut(&mut self) -> Option<&mut ProcessTreeState> {
        match self.active_view {
            View::Processes => Some(&mut self.processes_tree),
            View::Energy => Some(&mut self.energy_tree),
            _ => None,
        }
    }

    /// Returns the active tree state (immutable) for the current view.
    pub fn active_tree(&self) -> Option<&ProcessTreeState> {
        match self.active_view {
            View::Processes => Some(&self.processes_tree),
            View::Energy => Some(&self.energy_tree),
            _ => None,
        }
    }

    /// Sync the inspector panel to whatever row is currently highlighted.
    /// Must be called after every key event AND after every tick.
    pub fn refresh_selection_context(&mut self) {
        self.selected_process = None;
        self.selected_group = None;
        self.selected_reclaim_candidate = None;
        self.selected_reclaim_group = None;

        match self.active_view {
            View::Reclaim => {
                let visible: Vec<ReclaimCandidate> = self
                    .metrics_history
                    .get_reclaim_candidates(self.process_collector.groups())
                    .into_iter()
                    .filter(|c| c.score >= 5)
                    .collect();
                if let Some(idx) = self.reclaim_state.selected() {
                    if idx < visible.len() {
                        let candidate = visible[idx].clone();
                        let group = self
                            .process_collector
                            .groups()
                            .iter()
                            .find(|g| g.name.as_str() == candidate.app_name.as_str())
                            .cloned();
                        self.selected_reclaim_candidate = Some(candidate);
                        self.selected_reclaim_group = group;
                    }
                }
            }

            View::Processes | View::Energy => {
                let key = {
                    let tree = match self.active_view {
                        View::Processes => &self.processes_tree,
                        View::Energy => &self.energy_tree,
                        _ => return,
                    };
                    match tree.current_row_key() {
                        Some(k) => k.to_string(),
                        None => return,
                    }
                };

                let groups = self.process_collector.groups();

                // Check for process selection by PID
                if let Some(pid_str) = key
                    .strip_prefix("pid-")
                    .or_else(|| key.strip_prefix("child-"))
                {
                    if let Ok(pid) = pid_str.parse::<u32>() {
                        for g in groups {
                            for p in &g.processes {
                                if p.pid == pid {
                                    self.selected_process = Some(p.clone());
                                    self.selected_group = Some(g.clone());
                                    return;
                                }
                            }
                        }
                    }
                }

                // Check for group selection
                if let Some(group_key) = key.strip_prefix("group-") {
                    if let Some(g) = groups.iter().find(|g| g.name.as_str() == group_key) {
                        self.selected_group = Some(g.clone());
                    }
                }

                // Check for role selection — extract group name from "role-GroupName-roletype"
                if let Some(rest) = key.strip_prefix("role-") {
                    if let Some(dash_pos) = rest.rfind('-') {
                        let gname = &rest[..dash_pos];
                        if let Some(g) = groups.iter().find(|g| g.name.as_str() == gname) {
                            self.selected_group = Some(g.clone());
                        }
                    }
                }

                // Check for "more" selection — links to the parent group
                if let Some(group_key) = key.strip_prefix("more-") {
                    if let Some(g) = groups.iter().find(|g| g.name.as_str() == group_key) {
                        self.selected_group = Some(g.clone());
                    }
                }
            }
            _ => {}
        }
    }

    /// Returns the PID of the currently-selected process (for kill/suspend actions).
    pub fn active_pid(&self) -> Option<u32> {
        self.selected_process.as_ref().map(|p| p.pid).or_else(|| {
            self.selected_group
                .as_ref()
                .and_then(|g| g.processes.first().map(|p| p.pid))
        })
    }

    /// Apply filter text across all views.
    pub fn set_filter_text(&mut self, text: &str) {
        self.filter_input = text.to_string();
    }

    /// Clear filter and hide the bar.
    pub fn clear_filter(&mut self) {
        self.filter_visible = false;
        self.filter_input.clear();
    }
}
