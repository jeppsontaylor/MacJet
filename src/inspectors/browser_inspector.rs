/// MacJet — Browser Inspector
/// Queries Chrome/Safari/Brave/Arc for open tabs via AppleScript.
/// Optional Chromium DevTools Protocol (CDP) support for precision mode.
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use smol_str::SmolStr;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TabInfo {
    pub title: String,
    pub url: String,
    pub is_active: bool,
    pub window_index: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BrowserContext {
    pub app_name: SmolStr,
    pub tabs: Vec<TabInfo>,
    pub active_tab: Option<TabInfo>,
    pub window_count: u32,
    pub tab_count: u32,
    pub confidence: SmolStr,
}

pub struct BrowserInspector {
    cdp_port: u16,
    cache: FxHashMap<SmolStr, BrowserContext>,
}

#[derive(Deserialize)]
struct CdpTarget {
    #[serde(rename = "type", default)]
    target_type: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
}

impl Default for BrowserInspector {
    fn default() -> Self {
        Self::new(9222)
    }
}

impl BrowserInspector {
    pub fn new(cdp_port: u16) -> Self {
        Self {
            cdp_port,
            cache: FxHashMap::default(),
        }
    }

    pub fn get_cached(&self, app_name: &str) -> Option<&BrowserContext> {
        let lower = app_name.to_lowercase();
        for (k, v) in &self.cache {
            if lower.contains(k.to_lowercase().split_whitespace().next().unwrap_or("")) {
                return Some(v);
            }
        }
        None
    }

    pub async fn inspect(&mut self, app_name: &str) -> Option<BrowserContext> {
        let app_lower = app_name.to_lowercase();
        let canonical = if app_lower.contains("chrome") {
            "Google Chrome"
        } else if app_lower.contains("brave") {
            "Brave Browser"
        } else if app_lower.contains("arc") {
            "Arc"
        } else if app_lower.contains("safari") {
            "Safari"
        } else {
            return None;
        };

        if canonical != "Safari" {
            if let Some(ctx) = self.try_cdp().await {
                self.cache.insert(SmolStr::new(canonical), ctx.clone());
                return Some(ctx);
            }
        }

        let ctx = self.query_applescript(canonical).await?;
        self.cache.insert(SmolStr::new(canonical), ctx.clone());
        Some(ctx)
    }

    async fn try_cdp(&self) -> Option<BrowserContext> {
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

        parse_cdp_output(&result.stdout)
    }

    async fn query_applescript(&self, browser: &str) -> Option<BrowserContext> {
        let script = match browser {
            "Google Chrome" => CHROME_SCRIPT,
            "Brave Browser" => BRAVE_SCRIPT,
            "Arc" => ARC_SCRIPT,
            "Safari" => SAFARI_SCRIPT,
            _ => return None,
        };

        let child = Command::new("osascript")
            .args(["-e", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        let result = timeout(Duration::from_secs(3), child.wait_with_output())
            .await
            .ok()?
            .ok()?;
        if !result.status.success() || result.stdout.is_empty() {
            return None;
        }

        let stdout_str = String::from_utf8_lossy(&result.stdout);
        parse_applescript_output(&stdout_str, browser)
    }
}

// Pure functions for testability
pub fn parse_cdp_output(stdout: &[u8]) -> Option<BrowserContext> {
    let targets: Vec<CdpTarget> = serde_json::from_slice(stdout).ok()?;
    let mut tabs = Vec::new();
    for t in targets {
        if t.target_type == "page" {
            tabs.push(TabInfo {
                title: t.title,
                url: t.url,
                is_active: false, // CDP doesn't give active easily
                window_index: 0,
            });
        }
    }
    if tabs.is_empty() {
        return None;
    }
    Some(BrowserContext {
        app_name: SmolStr::new("Chrome (CDP)"),
        tab_count: tabs.len() as u32,
        window_count: 1,
        active_tab: None,
        tabs,
        confidence: SmolStr::new("app-exact"),
    })
}

pub fn parse_applescript_output(stdout: &str, browser: &str) -> Option<BrowserContext> {
    let mut tabs = Vec::new();
    let mut active = None;
    let mut windows = FxHashSet::default();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            continue;
        }

        let window_idx = parts[0].parse().unwrap_or(0);
        windows.insert(window_idx);

        let is_active = parts[3].trim().eq_ignore_ascii_case("true");
        let tab = TabInfo {
            title: parts[1].to_string(),
            url: parts[2].to_string(),
            is_active,
            window_index: window_idx,
        };

        if is_active && active.is_none() {
            active = Some(tab.clone());
        }
        tabs.push(tab);
    }

    if tabs.is_empty() {
        return None;
    }

    Some(BrowserContext {
        app_name: SmolStr::new(browser),
        window_count: windows.len() as u32,
        tab_count: tabs.len() as u32,
        active_tab: active,
        tabs,
        confidence: SmolStr::new("app-exact"),
    })
}

const CHROME_SCRIPT: &str = r#"
tell application "Google Chrome"
    set tabData to ""
    set windowCount to count of windows
    repeat with w from 1 to windowCount
        set tabCount to count of tabs of window w
        repeat with t from 1 to tabCount
            set tabTitle to title of tab t of window w
            set tabURL to URL of tab t of window w
            set isActive to (active tab index of window w is t)
            set tabData to tabData & w & "\t" & tabTitle & "\t" & tabURL & "\t" & isActive & "\n"
        end repeat
    end repeat
    return tabData
end tell
"#;

const BRAVE_SCRIPT: &str = r#"
tell application "Brave Browser"
    set tabData to ""
    set windowCount to count of windows
    repeat with w from 1 to windowCount
        set tabCount to count of tabs of window w
        repeat with t from 1 to tabCount
            set tabTitle to title of tab t of window w
            set tabURL to URL of tab t of window w
            set isActive to (active tab index of window w is t)
            set tabData to tabData & w & "\t" & tabTitle & "\t" & tabURL & "\t" & isActive & "\n"
        end repeat
    end repeat
    return tabData
end tell
"#;

const ARC_SCRIPT: &str = r#"
tell application "Arc"
    set tabData to ""
    set windowCount to count of windows
    repeat with w from 1 to windowCount
        set tabCount to count of tabs of window w
        repeat with t from 1 to tabCount
            set tabTitle to title of tab t of window w
            set tabURL to URL of tab t of window w
            set tabData to tabData & w & "\t" & tabTitle & "\t" & tabURL & "\tfalse\n"
        end repeat
    end repeat
    return tabData
end tell
"#;

const SAFARI_SCRIPT: &str = r#"
tell application "Safari"
    set tabData to ""
    set windowCount to count of windows
    repeat with w from 1 to windowCount
        set tabCount to count of tabs of window w
        repeat with t from 1 to tabCount
            set tabTitle to name of tab t of window w
            set tabURL to URL of tab t of window w
            set isActive to (current tab of window w is tab t of window w)
            set tabData to tabData & w & "\t" & tabTitle & "\t" & tabURL & "\t" & isActive & "\n"
        end repeat
    end repeat
    return tabData
end tell
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cdp_success() {
        let json = r#"[
            {"type": "page", "title": "GitHub", "url": "https://github.com"},
            {"type": "background_page", "title": "Extension", "url": "chrome-extension://123"}
        ]"#;
        let ctx = parse_cdp_output(json.as_bytes()).unwrap();
        assert_eq!(ctx.app_name, "Chrome (CDP)");
        assert_eq!(ctx.tabs.len(), 1);
        assert_eq!(ctx.tabs[0].title, "GitHub");
    }

    #[test]
    fn test_parse_cdp_failure() {
        assert!(parse_cdp_output(b"invalid json").is_none());
        assert!(parse_cdp_output(b"[]").is_none());
    }

    #[test]
    fn test_parse_applescript_success() {
        let output =
            "1\tTest Title\thttp://test.com\ttrue\n1\tOther Tab\thttp://other.com\tfalse\n";
        let ctx = parse_applescript_output(output, "Google Chrome").unwrap();
        assert_eq!(ctx.app_name, "Google Chrome");
        assert_eq!(ctx.window_count, 1);
        assert_eq!(ctx.tab_count, 2);
        assert!(ctx.active_tab.is_some());
        assert_eq!(ctx.active_tab.unwrap().title, "Test Title");
    }

    #[test]
    fn test_get_cached() {
        let mut inspector = BrowserInspector::new(9222);
        let ctx = BrowserContext {
            app_name: SmolStr::new("Safari"),
            ..Default::default()
        };
        inspector.cache.insert(SmolStr::new("Safari"), ctx.clone());

        assert_eq!(inspector.get_cached("Safari Web Content"), Some(&ctx));
        assert_eq!(inspector.get_cached("Unknown"), None);
    }
}
