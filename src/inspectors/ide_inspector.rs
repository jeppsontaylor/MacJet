/// MacJet — IDE Inspector
/// Detects projects/workspaces for VSCode, Cursor, Xcode, JetBrains IDEs.
use smol_str::SmolStr;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IDEContext {
    pub ide_name: SmolStr,
    pub project_path: SmolStr,
    pub project_name: SmolStr,
    pub active_file: SmolStr,
    pub window_title: SmolStr,
    pub confidence: SmolStr,
}

pub struct IDEInspector;

impl Default for IDEInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl IDEInspector {
    pub fn new() -> Self {
        Self
    }

    pub async fn inspect(
        &self,
        process_name: &str,
        cmdline: &[String],
        cwd: Option<&Path>,
    ) -> Option<IDEContext> {
        let ide_name = self.match_ide(process_name)?;

        match ide_name {
            "VSCode" | "Cursor" => self.inspect_vscode(ide_name, cmdline, cwd).await,
            "Xcode" => self.inspect_xcode().await,
            _ => self.inspect_jetbrains(ide_name, cmdline).await,
        }
    }

    fn match_ide(&self, process_name: &str) -> Option<&'static str> {
        let lower = process_name.to_lowercase();
        if lower.contains("xcode") {
            Some("Xcode")
        } else if lower.contains("cursor") {
            Some("Cursor") // matches "Cursor" and "Cursor Helper"
        } else if lower.contains("code") {
            Some("VSCode") // matches "Code" and "Code Helper"
        } else if lower.contains("idea") {
            Some("IntelliJ IDEA")
        } else if lower.contains("pycharm") {
            Some("PyCharm")
        } else if lower.contains("webstorm") {
            Some("WebStorm")
        } else if lower.contains("goland") {
            Some("GoLand")
        } else if lower.contains("clion") {
            Some("CLion")
        } else if lower.contains("rider") {
            Some("Rider")
        } else if lower.contains("rubymine") {
            Some("RubyMine")
        } else if lower.contains("phpstorm") {
            Some("PhpStorm")
        } else {
            None
        }
    }

    async fn inspect_vscode(
        &self,
        ide_name: &'static str,
        cmdline: &[String],
        cwd: Option<&Path>,
    ) -> Option<IDEContext> {
        let mut ctx = IDEContext {
            ide_name: SmolStr::new(ide_name),
            ..Default::default()
        };

        for arg in cmdline {
            if arg.starts_with("--folder-uri=file://") {
                let path_str = &arg[20..];
                ctx.project_path = SmolStr::new(path_str);
                if let Some(name) = Path::new(path_str).file_name() {
                    ctx.project_name = SmolStr::new(name.to_string_lossy().as_ref());
                }
                ctx.confidence = SmolStr::new("exact");
                return Some(ctx);
            }
        }

        // Try cwd
        if let Some(cwd_path) = cwd {
            let path_str = cwd_path.to_string_lossy();
            if path_str != "/" && !path_str.is_empty() {
                ctx.project_path = SmolStr::new(path_str.as_ref());
                if let Some(name) = cwd_path.file_name() {
                    ctx.project_name = SmolStr::new(name.to_string_lossy().as_ref());
                }
                ctx.confidence = SmolStr::new("inferred");
            }
        }

        // Try window title
        if let Some(title) = self.get_window_title(ide_name).await {
            ctx.window_title = SmolStr::new(&title);
            if let Some((file, proj)) = title.split_once(" — ") {
                ctx.active_file = SmolStr::new(file.trim());
                if ctx.project_name.is_empty() {
                    ctx.project_name = SmolStr::new(proj.trim());
                    ctx.confidence = SmolStr::new("window-exact");
                }
            }
        }

        Some(ctx)
    }

    async fn inspect_xcode(&self) -> Option<IDEContext> {
        let mut ctx = IDEContext {
            ide_name: SmolStr::new("Xcode"),
            ..Default::default()
        };

        if let Some(title) = self.get_window_title("Xcode").await {
            ctx.window_title = SmolStr::new(&title);
            // Xcode title format: ProjectName — FileName.swift
            if let Some((proj, file)) = title.split_once(" — ") {
                ctx.project_name = SmolStr::new(proj.trim());
                ctx.active_file = SmolStr::new(file.trim());
            } else {
                ctx.project_name = SmolStr::new(title.trim());
            }
            ctx.confidence = SmolStr::new("window-exact");
        }

        Some(ctx)
    }

    async fn inspect_jetbrains(
        &self,
        ide_name: &'static str,
        cmdline: &[String],
    ) -> Option<IDEContext> {
        let mut ctx = IDEContext {
            ide_name: SmolStr::new(ide_name),
            ..Default::default()
        };

        for arg in cmdline.iter().rev() {
            let p = Path::new(arg);
            if p.is_dir() {
                ctx.project_path = SmolStr::new(arg.as_str());
                if let Some(name) = p.file_name() {
                    ctx.project_name = SmolStr::new(name.to_string_lossy().as_ref());
                }
                ctx.confidence = SmolStr::new("exact");
                break;
            }
        }

        Some(ctx)
    }

    #[cfg(not(test))]
    async fn get_window_title(&self, app_name: &str) -> Option<String> {
        get_window_title_impl(app_name).await
    }

    #[cfg(test)]
    async fn get_window_title(&self, app_name: &str) -> Option<String> {
        tests::get_mock_map().lock().unwrap().get(app_name).cloned()
    }
}

pub async fn get_window_title_impl(app_name: &str) -> Option<String> {
    let script = format!(
        r#"
tell application "System Events"
    tell process "{}"
        try
            return name of front window
        on error
            return ""
        end try
    end tell
end tell"#,
        app_name
    );

    let child = Command::new("osascript")
        .args(["-e", &script])
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

    let stdout = String::from_utf8_lossy(&result.stdout).trim().to_string();
    if stdout.is_empty() {
        None
    } else {
        Some(stdout)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use rustc_hash::FxHashMap;
    use std::sync::{Mutex, OnceLock};

    pub static MOCK_TITLE: OnceLock<Mutex<FxHashMap<String, String>>> = OnceLock::new();

    pub fn get_mock_map() -> &'static Mutex<FxHashMap<String, String>> {
        MOCK_TITLE.get_or_init(|| Mutex::new(FxHashMap::default()))
    }

    fn set_mock_title(app: &str, title: Option<&str>) {
        if let Some(t) = title {
            get_mock_map()
                .lock()
                .unwrap()
                .insert(app.to_string(), t.to_string());
        } else {
            get_mock_map().lock().unwrap().remove(app);
        }
    }

    #[test]
    fn test_match_ide() {
        let inspector = IDEInspector::new();
        assert_eq!(inspector.match_ide("CodeHelper (Renderer)"), Some("VSCode"));
        assert_eq!(inspector.match_ide("Cursor Helper"), Some("Cursor"));
        assert_eq!(inspector.match_ide("Xcode"), Some("Xcode"));
        assert_eq!(inspector.match_ide("idea"), Some("IntelliJ IDEA"));
        assert_eq!(inspector.match_ide("pycharm"), Some("PyCharm"));
        assert_eq!(inspector.match_ide("Brave Browser"), None);
    }

    #[tokio::test]
    async fn test_inspect_unmatched() {
        let inspector = IDEInspector::new();
        let ctx = inspector.inspect("Safari", &[], None).await;
        assert!(ctx.is_none());
    }

    #[tokio::test]
    async fn test_inspect_vscode_folder_uri() {
        let inspector = IDEInspector::new();
        let cmdline = vec![
            "/opt/cursor".to_string(),
            "--folder-uri=file:///Users/dev/my-project".to_string(),
            "--no-sandbox".to_string(),
        ];

        let ctx = inspector.inspect("Cursor", &cmdline, None).await.unwrap();

        assert_eq!(ctx.ide_name, "Cursor");
        assert_eq!(ctx.project_path, "/Users/dev/my-project");
        assert_eq!(ctx.project_name, "my-project");
        assert_eq!(ctx.confidence, "exact");
    }

    #[tokio::test]
    async fn test_inspect_vscode_fallback() {
        set_mock_title("VSCode", Some("main.py — some-other-project"));

        let inspector = IDEInspector::new();
        let cwd = std::path::Path::new("/Users/dev/fallback-project");
        let ctx = inspector.inspect("Code", &[], Some(cwd)).await.unwrap();

        assert_eq!(ctx.ide_name, "VSCode");
        assert_eq!(ctx.project_path, "/Users/dev/fallback-project");
        assert_eq!(ctx.project_name, "fallback-project");
        assert_eq!(ctx.confidence, "inferred");
        assert_eq!(ctx.active_file, "main.py");
        assert_eq!(ctx.window_title, "main.py — some-other-project");

        set_mock_title("VSCode", None);
    }

    #[tokio::test]
    async fn test_inspect_xcode() {
        set_mock_title("Xcode", Some("MacJet — App.swift"));

        let inspector = IDEInspector::new();
        let ctx = inspector.inspect("Xcode", &[], None).await.unwrap();

        assert_eq!(ctx.ide_name, "Xcode");
        assert_eq!(ctx.project_name, "MacJet");
        assert_eq!(ctx.active_file, "App.swift");
        assert_eq!(ctx.confidence, "window-exact");

        set_mock_title("Xcode", None);
    }

    #[tokio::test]
    async fn test_inspect_jetbrains() {
        let proj_dir = std::env::temp_dir().join("my-java-project");
        let _ = std::fs::create_dir(&proj_dir);

        let cmdline = vec![
            "java".to_string(),
            "-jar".to_string(),
            "idea.jar".to_string(),
            proj_dir.to_string_lossy().to_string(),
        ];

        let inspector = IDEInspector::new();
        let ctx = inspector.inspect("idea", &cmdline, None).await.unwrap();

        assert_eq!(ctx.ide_name, "IntelliJ IDEA");
        assert_eq!(ctx.project_path.as_str(), proj_dir.to_string_lossy());
        assert_eq!(ctx.project_name, "my-java-project");
        assert_eq!(ctx.confidence, "exact");
    }
}
