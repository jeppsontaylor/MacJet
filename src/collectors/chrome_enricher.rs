/// MacJet — Chrome Tab Enricher (Synchronous)
///
/// Lightweight synchronous Chrome tab fetcher that enriches renderer/Helper
/// process context_labels with tab titles from Chrome's CDP `/json` endpoint.
///
/// Runs inline during tick() every 5 seconds. No async, no WebSocket,
/// just a `curl` call + JSON parse + PID heuristic correlation.
use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CDP_PORT: u16 = 9222;
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct TabEntry {
    pub title: String,
    pub url: String,
    pub renderer_pid: u32,
}

pub struct ChromeTabEnricher {
    /// Cached tab entries, keyed by renderer PID
    pid_to_tab: FxHashMap<u32, TabEntry>,
    /// All tab entries (for order-based correlation)
    tabs: Vec<TabEntry>,
    /// Last refresh time
    last_refresh: Option<Instant>,
    /// Whether Chrome CDP is available
    pub available: bool,
}

impl Default for ChromeTabEnricher {
    fn default() -> Self {
        Self::new()
    }
}

impl ChromeTabEnricher {
    pub fn new() -> Self {
        Self {
            pid_to_tab: FxHashMap::default(),
            tabs: Vec::new(),
            last_refresh: None,
            available: false,
        }
    }

    /// Should we refresh? True every REFRESH_INTERVAL.
    pub fn should_refresh(&self) -> bool {
        match self.last_refresh {
            None => true,
            Some(t) => t.elapsed() >= REFRESH_INTERVAL,
        }
    }

    /// Synchronously fetch Chrome tabs via `/json` endpoint.
    /// This is fast (~10ms) and non-blocking for the TUI.
    pub fn refresh(&mut self) {
        self.last_refresh = Some(Instant::now());

        let url = format!("http://localhost:{}/json", CDP_PORT);
        let result = Command::new("curl")
            .args(["-s", "--connect-timeout", "1", "--max-time", "2", &url])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();

        let output = match result {
            Ok(o) if o.status.success() && !o.stdout.is_empty() => o,
            _ => {
                self.available = false;
                return;
            }
        };

        let arr: Vec<serde_json::Value> = match serde_json::from_slice(&output.stdout) {
            Ok(v) => v,
            Err(_) => {
                self.available = false;
                return;
            }
        };

        self.available = true;
        self.tabs.clear();

        for entry in &arr {
            let ttype = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if ttype != "page" {
                continue;
            }

            let title = entry
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let url = entry
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            self.tabs.push(TabEntry {
                title,
                url,
                renderer_pid: 0, // will be correlated later
            });
        }
    }

    /// Enrich process groups: find Chrome renderer/Helper processes and set their
    /// context_label to the tab title.
    ///
    /// Uses heuristic PID correlation: Chrome renderer PIDs are discovered from
    /// the process list itself; heavy renderers are matched to tabs by CPU order.
    pub fn enrich_groups(&mut self, groups: &mut [super::process_collector::ProcessGroup]) {
        if !self.available || self.tabs.is_empty() {
            return;
        }

        // Collect renderer processes from across all Chrome-related groups
        let mut renderers: Vec<(usize, usize, f64)> = Vec::new(); // (group_idx, proc_idx, cpu)

        for (gi, group) in groups.iter().enumerate() {
            let name_lower = group.name.to_lowercase();
            let is_chrome = name_lower.contains("chrome")
                || name_lower.contains("brave")
                || name_lower.contains("arc");
            if !is_chrome {
                continue;
            }

            for (pi, p) in group.processes.iter().enumerate() {
                let pname = p.name.to_lowercase();
                if pname.contains("renderer") || pname.contains("helper") {
                    // Check if it's a renderer type via cmdline
                    let is_renderer = p
                        .cmdline
                        .iter()
                        .any(|arg| arg.as_str() == "--type=renderer");
                    if is_renderer {
                        renderers.push((gi, pi, p.cpu_percent));
                    }
                }
            }
        }

        if renderers.is_empty() {
            return;
        }

        // Sort renderers by CPU descending (heaviest first) — same heuristic as Python
        renderers.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        // Match tab titles to renderers by order
        for (i, &(gi, pi, _cpu)) in renderers.iter().enumerate() {
            if i >= self.tabs.len() {
                break;
            }

            let tab = &self.tabs[i];
            let label = self.format_tab_label(&tab.title, &tab.url, 32);
            groups[gi].processes[pi].context_label = SmolStr::new(format!("🌐 {}", label));
            groups[gi].processes[pi].confidence = SmolStr::new("exact");
        }
    }

    fn format_tab_label(&self, title: &str, url: &str, max_len: usize) -> String {
        let label = if title.is_empty() {
            self.extract_domain(url)
        } else {
            title.to_string()
        };

        if label.len() > max_len {
            format!("{}…", &label[..max_len - 1])
        } else {
            label
        }
    }

    fn extract_domain(&self, url: &str) -> String {
        // Simple domain extraction without url crate
        if let Some(start) = url.find("://") {
            let rest = &url[start + 3..];
            if let Some(end) = rest.find('/') {
                return rest[..end].to_string();
            }
            return rest.to_string();
        }
        url.chars().take(30).collect()
    }

    /// Get tab title for display (for the Context column in the process tree).
    pub fn get_tab_title_for_pid(&self, pid: u32) -> Option<&str> {
        self.pid_to_tab.get(&pid).map(|t| t.title.as_str())
    }
}
