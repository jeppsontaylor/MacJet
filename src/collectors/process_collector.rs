/// MacJet — Fast-lane Process Collector
///
/// Uses sysinfo to enumerate processes, build trees, and group by coalition/app.
use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::System;

use super::metrics_history::MetricsHistory;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: SmolStr,
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub memory_percent: f64,
    pub num_threads: u32,
    pub cmdline: Vec<SmolStr>,
    pub cwd: SmolStr,
    pub exe: SmolStr,
    pub ppid: u32,
    pub status: SmolStr,
    pub create_time: f64,
    pub username: SmolStr,
    pub children_pids: Vec<u32>,
    pub context_label: SmolStr,
    pub confidence: SmolStr,
    pub energy_impact: SmolStr,
    pub net_bytes_sent: u64,
    pub net_bytes_recv: u64,
    pub role_type: SmolStr,
    pub is_hidden: bool,
    pub launch_age_s: f64,
    pub is_system: bool,
}

#[derive(Debug, Clone)]
pub struct ProcessGroup {
    pub name: SmolStr,
    pub icon: &'static str,
    pub total_cpu: f64,
    pub total_memory_mb: f64,
    pub total_net_recv: u64,
    pub total_net_sent: u64,
    pub energy_impact: SmolStr,
    pub processes: Vec<ProcessInfo>,
    pub context_label: SmolStr,
    pub confidence: SmolStr,
    pub why_hot: SmolStr,
    pub is_expanded: bool,
}

pub fn severity_icon(cpu: f64) -> &'static str {
    if cpu > 100.0 {
        "🔴"
    } else if cpu > 50.0 {
        "🟠"
    } else if cpu > 25.0 {
        "🟡"
    } else {
        "🟢"
    }
}

pub fn parse_app_name(proc: &ProcessInfo) -> SmolStr {
    let name = proc.name.as_str();

    // Chrome / Brave / Arc helpers
    if name.contains("Helper") && !proc.cmdline.is_empty() {
        for arg in &proc.cmdline {
            if arg.starts_with("--type=") {
                let helper_type = &arg[7..];
                if let Some(parent_name) = name.split(" Helper").next() {
                    return SmolStr::new(format!("{} ({})", parent_name, helper_type));
                }
            }
        }
    }

    // Node.js — show script path
    if (name == "node" || name == "Node") && proc.cmdline.len() > 1 {
        for arg in &proc.cmdline[1..] {
            if !arg.starts_with("-") {
                return SmolStr::new(format!("node {}", arg));
            }
        }
        return SmolStr::new("node");
    }

    // Python — show script path
    if name.starts_with("python") || name == "Python" {
        if proc.cmdline.len() > 1 {
            for arg in &proc.cmdline[1..] {
                if !arg.starts_with("-") && arg != "-m" {
                    return SmolStr::new(format!("python {}", arg));
                }
            }
        }
    }

    // Java — show jar or main class
    if name == "java" && !proc.cmdline.is_empty() {
        for (i, arg) in proc.cmdline.iter().enumerate() {
            if arg == "-jar" && i + 1 < proc.cmdline.len() {
                return SmolStr::new(format!("java -jar {}", proc.cmdline[i + 1]));
            }
        }
    }

    proc.name.clone()
}

pub fn determine_group_key(
    proc: &ProcessInfo,
    all_procs_names: &FxHashMap<u32, SmolStr>,
) -> SmolStr {
    let name = proc.name.as_str();

    // Browser helpers → group under parent browser
    for browser in &["Google Chrome", "Brave Browser", "Arc", "Safari", "Firefox"] {
        if name
            .to_lowercase()
            .contains(&browser.split(' ').next().unwrap().to_lowercase())
        {
            return SmolStr::new(*browser);
        }
    }

    // Electron apps with --type=renderer
    if !proc.cmdline.is_empty() {
        for arg in &proc.cmdline {
            if arg.contains("--type=") {
                if let Some(parent_name) = all_procs_names.get(&proc.ppid) {
                    if !parent_name.contains("Helper") {
                        return parent_name.clone();
                    }
                }
            }
        }
    }

    // Docker
    if name == "com.docker.vmnetd"
        || name == "com.docker.backend"
        || name == "Docker"
        || name == "docker"
    {
        return SmolStr::new("Docker Desktop");
    }

    // VSCode / Cursor helpers
    for ide in &["Code Helper", "Cursor Helper"] {
        if name.contains(ide) {
            return SmolStr::new(ide.split(" Helper").next().unwrap());
        }
    }

    proc.name.clone()
}

pub fn extract_role_type(cmdline: &[SmolStr]) -> SmolStr {
    for arg in cmdline {
        if arg.starts_with("--type=") {
            return SmolStr::new(&arg[7..]);
        }
    }
    SmolStr::default()
}

pub fn is_system_process(username: &str, exe: &str) -> bool {
    const SYSTEM_USERS: &[&str] = &[
        "root",
        "_windowserver",
        "_mdnsresponder",
        "_coreaudiod",
        "_locationd",
        "_spotlight",
        "_securityagent",
        "_usbmuxd",
        "_distnoted",
        "_networkd",
        "_appleevents",
        "_softwareupdate",
        "_nsurlsessiond",
        "_trustd",
        "_timed",
        "nobody",
        "daemon",
    ];
    if SYSTEM_USERS.contains(&username) {
        return true;
    }
    if exe.starts_with("/usr/")
        || exe.starts_with("/System/")
        || exe.starts_with("/sbin/")
        || exe.starts_with("/Library/Apple/")
    {
        return true;
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Cpu,
    Memory,
    Name,
    Pid,
    Threads,
    Energy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupingMode {
    App,
    Tree,
    Flat,
}

pub struct ProcessCollector {
    pub sort_key: SortKey,
    pub filter_text: String,
    pub grouping_mode: GroupingMode,
    pub metrics_history: MetricsHistory,

    all_procs: Vec<ProcessInfo>,
    groups: Vec<ProcessGroup>, // sorted slice
}

impl Default for ProcessCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessCollector {
    pub fn new() -> Self {
        Self {
            sort_key: SortKey::Cpu,
            filter_text: String::new(),
            grouping_mode: GroupingMode::App,
            metrics_history: MetricsHistory::new(),
            all_procs: Vec::new(),
            groups: Vec::new(),
        }
    }

    pub fn cycle_sort(&mut self) -> SortKey {
        self.sort_key = match self.sort_key {
            SortKey::Cpu => SortKey::Memory,
            SortKey::Memory => SortKey::Name,
            SortKey::Name => SortKey::Pid,
            SortKey::Pid => SortKey::Threads,
            SortKey::Threads => SortKey::Energy,
            SortKey::Energy => SortKey::Cpu,
        };
        self.sort_key
    }

    pub fn cycle_grouping(&mut self) -> GroupingMode {
        self.grouping_mode = match self.grouping_mode {
            GroupingMode::App => GroupingMode::Tree,
            GroupingMode::Tree => GroupingMode::Flat,
            GroupingMode::Flat => GroupingMode::App,
        };
        self.grouping_mode
    }

    pub fn collect_sync(&mut self, sys: &mut System) -> (&[ProcessInfo], &[ProcessGroup]) {
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        let mut procs = Vec::with_capacity(sys.processes().len());

        for (pid, process) in sys.processes() {
            let pid = pid.as_u32();
            let mut info = ProcessInfo {
                pid,
                name: SmolStr::new(process.name().to_string_lossy().as_ref()),
                cpu_percent: process.cpu_usage() as f64,
                memory_mb: process.memory() as f64 / (1024.0 * 1024.0),
                memory_percent: 0.0, // sysinfo doesn't easily expose this globally per process
                num_threads: 0,      // Optional via extensions
                cmdline: process
                    .cmd()
                    .iter()
                    .map(|s| SmolStr::new(s.to_string_lossy().as_ref()))
                    .collect(),
                cwd: process
                    .cwd()
                    .map(|p| SmolStr::new(p.to_string_lossy().as_ref()))
                    .unwrap_or_default(),
                exe: process
                    .exe()
                    .map(|p| SmolStr::new(p.to_string_lossy().as_ref()))
                    .unwrap_or_default(),
                ppid: process.parent().map(|p| p.as_u32()).unwrap_or(0),
                status: SmolStr::default(),
                create_time: process.start_time() as f64,
                username: process
                    .user_id()
                    .map(|u| SmolStr::new(u.to_string().as_str()))
                    .unwrap_or_default(),
                children_pids: Vec::new(),
                context_label: SmolStr::default(),
                confidence: SmolStr::new("grouped"),
                energy_impact: SmolStr::default(),
                net_bytes_sent: 0,
                net_bytes_recv: 0,
                role_type: SmolStr::default(),
                is_hidden: false,
                launch_age_s: 0.0,
                is_system: false,
            };

            info.context_label = parse_app_name(&info);
            if info.context_label != info.name {
                info.confidence = SmolStr::new("exact");
            }

            info.role_type = extract_role_type(&info.cmdline);
            info.is_system = is_system_process(info.username.as_str(), info.exe.as_str());
            info.launch_age_s = if info.create_time > 0.0 {
                now - info.create_time
            } else {
                0.0
            };

            self.metrics_history
                .record(info.pid, info.cpu_percent, info.memory_mb);
            procs.push(info);
        }

        // Build lookup map for determine_group_key
        let mut all_procs_names_map = FxHashMap::default();
        for p in &procs {
            all_procs_names_map.insert(p.pid, p.name.clone());
        }

        // Sort
        match self.sort_key {
            SortKey::Cpu => {
                procs.sort_by(|a, b| b.cpu_percent.partial_cmp(&a.cpu_percent).unwrap())
            }
            SortKey::Memory => procs.sort_by(|a, b| b.memory_mb.partial_cmp(&a.memory_mb).unwrap()),
            SortKey::Name => {
                procs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }
            SortKey::Pid => procs.sort_by(|a, b| a.pid.cmp(&b.pid)),
            SortKey::Threads => procs.sort_by(|a, b| b.num_threads.cmp(&a.num_threads)),
            SortKey::Energy => procs.sort_by(|a, b| {
                let energy_weight = |e: &str| match e {
                    "HIGH" => 3,
                    "MED" => 2,
                    "LOW" => 1,
                    _ => 0,
                };
                energy_weight(b.energy_impact.as_str())
                    .cmp(&energy_weight(a.energy_impact.as_str()))
            }),
        }

        // Filter
        if !self.filter_text.is_empty() {
            let filter = self.filter_text.to_lowercase();
            procs.retain(|p| {
                p.name.to_lowercase().contains(&filter)
                    || p.context_label.to_lowercase().contains(&filter)
                    || p.cmdline.join(" ").to_lowercase().contains(&filter)
            });
        }

        // Group
        let mut groups_map: FxHashMap<SmolStr, ProcessGroup> = FxHashMap::default();

        if self.grouping_mode == GroupingMode::Flat {
            for p in &procs {
                let key = SmolStr::new(p.pid.to_string());
                groups_map.insert(
                    key.clone(),
                    ProcessGroup {
                        name: p.context_label.clone(),
                        icon: severity_icon(p.cpu_percent),
                        total_cpu: p.cpu_percent,
                        total_memory_mb: p.memory_mb,
                        total_net_recv: 0,
                        total_net_sent: 0,
                        energy_impact: SmolStr::default(),
                        confidence: p.confidence.clone(),
                        context_label: SmolStr::default(),
                        why_hot: SmolStr::default(),
                        is_expanded: false,
                        processes: vec![p.clone()],
                    },
                );
            }
        } else {
            for p in &procs {
                let key = if self.grouping_mode == GroupingMode::App {
                    determine_group_key(p, &all_procs_names_map)
                } else {
                    p.name.clone()
                };

                // we use entry to avoid double lookup
                let entry = groups_map
                    .entry(key.clone())
                    .or_insert_with(|| ProcessGroup {
                        name: key.clone(),
                        icon: "🟢",
                        total_cpu: 0.0,
                        total_memory_mb: 0.0,
                        total_net_recv: 0,
                        total_net_sent: 0,
                        energy_impact: SmolStr::default(),
                        processes: Vec::new(),
                        context_label: SmolStr::default(),
                        confidence: SmolStr::new("grouped"),
                        why_hot: SmolStr::default(),
                        is_expanded: false,
                    });

                entry.processes.push(p.clone());
                entry.total_cpu += p.cpu_percent;
                entry.total_memory_mb += p.memory_mb;
                entry.total_net_recv += p.net_bytes_recv;
                entry.total_net_sent += p.net_bytes_sent;
            }

            for g in groups_map.values_mut() {
                g.icon = severity_icon(g.total_cpu);
                if g.processes.len() > 1 {
                    let has_exact = g
                        .processes
                        .iter()
                        .any(|p| p.confidence == "exact" || p.confidence == "window-exact");
                    g.confidence = SmolStr::new(if has_exact { "app-exact" } else { "grouped" });
                } else if !g.processes.is_empty() {
                    g.confidence = g.processes[0].confidence.clone();
                }
            }
        }

        let mut sorted_groups: Vec<ProcessGroup> = groups_map.into_values().collect();
        sorted_groups.sort_by(|a, b| b.total_cpu.partial_cmp(&a.total_cpu).unwrap());

        self.all_procs = procs;
        self.groups = sorted_groups;

        self.metrics_history.expire_stale();

        (&self.all_procs, &self.groups)
    }

    pub fn groups(&self) -> &[ProcessGroup] {
        &self.groups
    }

    pub fn groups_mut(&mut self) -> &mut [ProcessGroup] {
        &mut self.groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::make_process_info;

    #[test]
    fn test_severity_icon() {
        assert_eq!(severity_icon(5.0), "🟢");
        assert_eq!(severity_icon(30.0), "🟡");
        assert_eq!(severity_icon(60.0), "🟠");
        assert_eq!(severity_icon(150.0), "🔴");
    }

    #[test]
    fn test_parse_app_name_chrome_renderer() {
        let proc = make_process_info(
            100,
            "Google Chrome Helper (Renderer)",
            0.0,
            0.0,
            vec!["--type=renderer"],
            "",
            "",
            0,
            false,
            false,
        );
        let result = parse_app_name(&proc);
        assert!(result.contains("Google Chrome"));
        assert!(result.contains("renderer"));
    }

    #[test]
    fn test_parse_app_name_node() {
        let proc = make_process_info(
            100,
            "node",
            0.0,
            0.0,
            vec!["node", "server.js"],
            "",
            "",
            0,
            false,
            false,
        );
        let result = parse_app_name(&proc);
        assert!(result.contains("server.js"));
    }

    #[test]
    fn test_parse_app_name_python() {
        let proc = make_process_info(
            100,
            "python3",
            0.0,
            0.0,
            vec!["python3", "train.py"],
            "",
            "",
            0,
            false,
            false,
        );
        let result = parse_app_name(&proc);
        assert!(result.contains("train.py"));
    }

    #[test]
    fn test_parse_app_name_java() {
        let proc = make_process_info(
            100,
            "java",
            0.0,
            0.0,
            vec!["java", "-jar", "app.jar"],
            "",
            "",
            0,
            false,
            false,
        );
        let result = parse_app_name(&proc);
        assert!(result.contains("app.jar"));
    }

    #[test]
    fn test_parse_app_name_plain() {
        let proc = make_process_info(
            100,
            "Finder",
            0.0,
            0.0,
            vec!["/System/Library/CoreServices/Finder.app"],
            "",
            "",
            0,
            false,
            false,
        );
        let result = parse_app_name(&proc);
        assert_eq!(result, "Finder");
    }

    #[test]
    fn test_extract_role_type() {
        assert_eq!(
            extract_role_type(&[SmolStr::new("--type=renderer")]),
            "renderer"
        );
        assert_eq!(
            extract_role_type(&[SmolStr::new("--type=gpu-process")]),
            "gpu-process"
        );
        assert_eq!(
            extract_role_type(&[SmolStr::new("--flag"), SmolStr::new("--other")]),
            ""
        );
        assert_eq!(extract_role_type(&[]), "");
    }

    #[test]
    fn test_is_system_process() {
        assert!(is_system_process("root", "/usr/sbin/syslogd"));
        assert!(is_system_process("someuser", "/usr/libexec/something"));
        assert!(is_system_process("someuser", "/Library/Apple/something"));
        assert!(!is_system_process(
            "testuser",
            "/Applications/MyApp.app/Contents/MacOS/MyApp"
        ));
        assert!(is_system_process("_windowserver", ""));
    }

    #[test]
    fn test_determine_group_key_chrome() {
        let mut map = FxHashMap::default();
        map.insert(100, SmolStr::new("Google Chrome"));

        let proc = make_process_info(
            200,
            "Google Chrome Helper",
            0.0,
            0.0,
            vec!["--type=renderer"],
            "",
            "",
            100,
            false,
            false,
        );
        assert_eq!(determine_group_key(&proc, &map), "Google Chrome");
    }

    #[test]
    fn test_determine_group_key_docker() {
        let map = FxHashMap::default();
        for name in &["com.docker.vmnetd", "com.docker.backend", "Docker"] {
            let proc = make_process_info(100, name, 0.0, 0.0, vec![], "", "", 1, false, false);
            assert_eq!(determine_group_key(&proc, &map), "Docker Desktop");
        }
    }

    #[test]
    fn test_determine_group_key_vscode() {
        let map = FxHashMap::default();
        let proc = make_process_info(
            100,
            "Code Helper (Renderer)",
            0.0,
            0.0,
            vec![],
            "",
            "",
            1,
            false,
            false,
        );
        assert_eq!(determine_group_key(&proc, &map), "Code");
    }

    #[test]
    fn test_determine_group_key_standalone() {
        let map = FxHashMap::default();
        let proc = make_process_info(100, "Spotify", 0.0, 0.0, vec![], "", "", 1, false, false);
        assert_eq!(determine_group_key(&proc, &map), "Spotify");
    }
}
