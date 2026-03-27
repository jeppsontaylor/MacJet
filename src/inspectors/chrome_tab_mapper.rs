use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
/// MacJet — Chrome Tab Mapper
/// Maps Chrome renderer PIDs to exact tab titles using CDP.
use smol_str::SmolStr;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;

#[derive(Debug, Clone, Default)]
pub struct ChromeTab {
    pub target_id: SmolStr,
    pub title: SmolStr,
    pub url: SmolStr,
    pub favicon_url: SmolStr,
    pub tab_type: SmolStr,
    pub ws_url: SmolStr,
    pub js_heap_mb: f64,
    pub task_duration: f64,
    pub dom_nodes: u32,
    pub renderer_pid: u32,
    pub cpu_time_s: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ChromeSnapshot {
    pub tabs: Vec<ChromeTab>,
    pub renderer_pids: rustc_hash::FxHashMap<u32, f64>,
    pub total_tabs: u32,
    pub total_js_heap_mb: f64,
    pub has_cdp: bool,
    pub cdp_port: u16,
    pub error: SmolStr,
}

pub struct ChromeTabMapper {
    cdp_port: u16,
    pub latest: ChromeSnapshot,
}

impl Default for ChromeTabMapper {
    fn default() -> Self {
        Self::new(9222)
    }
}

impl ChromeTabMapper {
    pub fn new(cdp_port: u16) -> Self {
        Self {
            cdp_port,
            latest: ChromeSnapshot::default(),
        }
    }

    pub async fn collect(&mut self) -> &ChromeSnapshot {
        let mut snapshot = ChromeSnapshot {
            cdp_port: self.cdp_port,
            ..Default::default()
        };

        let json_tabs_opt = self.get_json_tabs().await;
        if json_tabs_opt.is_none() {
            snapshot.error = SmolStr::new("CDP not available");
            self.latest = snapshot;
            return &self.latest;
        }

        let mut tabs = json_tabs_opt.unwrap();
        snapshot.has_cdp = true;
        snapshot.total_tabs = tabs.len() as u32;

        let renderer_pids = self.get_renderer_pids().await;
        snapshot.renderer_pids = renderer_pids.clone();

        self.enrich_tab_metrics(&mut tabs).await;
        self.correlate_pids(&mut tabs, &renderer_pids);

        snapshot.total_js_heap_mb = tabs.iter().map(|t| t.js_heap_mb).sum();

        let has_heap = tabs.iter().any(|t| t.js_heap_mb > 0.0);
        if has_heap {
            tabs.sort_by(|a, b| {
                b.js_heap_mb
                    .partial_cmp(&a.js_heap_mb)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        } else {
            tabs.sort_by(|a, b| {
                b.cpu_time_s
                    .partial_cmp(&a.cpu_time_s)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        snapshot.tabs = tabs;
        self.latest = snapshot;
        &self.latest
    }

    async fn get_json_tabs(&self) -> Option<Vec<ChromeTab>> {
        let url = format!("http://localhost:{}/json", self.cdp_port);
        let child = Command::new("curl")
            .args(["-s", "--connect-timeout", "1", &url])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        let result = timeout(Duration::from_secs(2), child.wait_with_output())
            .await
            .ok()?
            .ok()?;
        if !result.status.success() || result.stdout.is_empty() {
            return None;
        }

        let arr: Vec<Value> = serde_json::from_slice(&result.stdout).ok()?;
        let mut tabs = Vec::new();

        for entry in arr {
            let ttype = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if ttype != "page" && ttype != "background_page" {
                continue;
            }

            tabs.push(ChromeTab {
                target_id: SmolStr::new(entry.get("id").and_then(|v| v.as_str()).unwrap_or("")),
                title: SmolStr::new(entry.get("title").and_then(|v| v.as_str()).unwrap_or("")),
                url: SmolStr::new(entry.get("url").and_then(|v| v.as_str()).unwrap_or("")),
                favicon_url: SmolStr::new(
                    entry
                        .get("faviconUrl")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                ),
                tab_type: SmolStr::new(ttype),
                ws_url: SmolStr::new(
                    entry
                        .get("webSocketDebuggerUrl")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                ),
                ..Default::default()
            });
        }

        Some(tabs)
    }

    async fn get_renderer_pids(&self) -> rustc_hash::FxHashMap<u32, f64> {
        let mut map = rustc_hash::FxHashMap::default();
        let url = format!("http://localhost:{}/json/version", self.cdp_port);

        let Some(child) = Command::new("curl")
            .args(["-s", "--connect-timeout", "1", &url])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()
        else {
            return map;
        };

        let Ok(Ok(result)) = timeout(Duration::from_secs(2), child.wait_with_output()).await else {
            return map;
        };
        if !result.status.success() || result.stdout.is_empty() {
            return map;
        }

        let Ok(version) = serde_json::from_slice::<Value>(&result.stdout) else {
            return map;
        };
        let Some(browser_ws) = version.get("webSocketDebuggerUrl").and_then(|v| v.as_str()) else {
            return map;
        };

        if let Ok(Ok((mut ws_stream, _))) =
            timeout(Duration::from_secs(2), connect_async(browser_ws)).await
        {
            let req = serde_json::json!({
                "id": 1,
                "method": "SystemInfo.getProcessInfo"
            });
            let _ = ws_stream.send(Message::Text(req.to_string().into())).await;

            if let Ok(Some(Ok(msg))) = timeout(Duration::from_secs(2), ws_stream.next()).await {
                if let Message::Text(text) = msg {
                    if let Ok(resp) = serde_json::from_str::<Value>(&text) {
                        if let Some(process_infos) = resp
                            .get("result")
                            .and_then(|r| r.get("processInfo"))
                            .and_then(|p| p.as_array())
                        {
                            for p in process_infos {
                                if p.get("type").and_then(|v| v.as_str()) == Some("renderer") {
                                    if let Some(pid) = p.get("id").and_then(|v| v.as_u64()) {
                                        let cpu_time = p
                                            .get("cpuTime")
                                            .and_then(|v| v.as_f64())
                                            .unwrap_or(0.0);
                                        map.insert(pid as u32, cpu_time);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let _ = ws_stream.close(None).await;
        }

        map
    }

    async fn enrich_tab_metrics(&self, tabs: &mut Vec<ChromeTab>) {
        let mut set = tokio::task::JoinSet::new();
        for (i, tab) in tabs.iter().enumerate() {
            if !tab.ws_url.is_empty() {
                let ws_url = tab.ws_url.to_string();
                set.spawn(async move {
                    if let Ok(Ok((mut ws_stream, _))) =
                        timeout(Duration::from_secs(1), connect_async(&ws_url)).await
                    {
                        let req = serde_json::json!({
                            "id": 1,
                            "method": "Performance.getMetrics"
                        });
                        let _ = ws_stream.send(Message::Text(req.to_string().into())).await;

                        if let Ok(Some(Ok(Message::Text(text)))) =
                            timeout(Duration::from_secs(1), ws_stream.next()).await
                        {
                            if let Ok(resp) = serde_json::from_str::<Value>(text.as_str()) {
                                if let Some(metrics) = resp
                                    .get("result")
                                    .and_then(|r| r.get("metrics"))
                                    .and_then(|m| m.as_array())
                                {
                                    let mut js_heap = 0.0;
                                    let mut dom_nodes = 0;
                                    let mut task_dur = 0.0;
                                    for m in metrics {
                                        let name =
                                            m.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                        let val =
                                            m.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);
                                        match name {
                                            "JSHeapUsedSize" => js_heap = val / (1024.0 * 1024.0),
                                            "Nodes" => dom_nodes = val as u32,
                                            "TaskDuration" => task_dur = val,
                                            _ => {}
                                        }
                                    }
                                    let _ = ws_stream.close(None).await;
                                    return (i, js_heap, dom_nodes, task_dur);
                                }
                            }
                        }
                        let _ = ws_stream.close(None).await;
                    }
                    (i, 0.0, 0, 0.0)
                });
            } else {
                set.spawn(async move { (i, 0.0, 0, 0.0) });
            }
        }

        while let Some(res) = set.join_next().await {
            if let Ok((i, js, dom, dur)) = res {
                tabs[i].js_heap_mb = js;
                tabs[i].dom_nodes = dom;
                tabs[i].task_duration = dur;
            }
        }
    }

    fn correlate_pids(
        &self,
        tabs: &mut Vec<ChromeTab>,
        renderer_pids: &rustc_hash::FxHashMap<u32, f64>,
    ) {
        if renderer_pids.is_empty() {
            return;
        }

        let mut available_pids: Vec<(u32, f64)> =
            renderer_pids.iter().map(|(&k, &v)| (k, v)).collect();
        // Sort by cpu_time descending as a heuristic
        available_pids.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut used_idx = 0;
        for tab in tabs.iter_mut() {
            if used_idx < available_pids.len() {
                tab.renderer_pid = available_pids[used_idx].0;
                tab.cpu_time_s = available_pids[used_idx].1;
                used_idx += 1;
            }
        }
    }
}
