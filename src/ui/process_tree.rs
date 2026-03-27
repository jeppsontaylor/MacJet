use crate::collectors::energy_collector::EnergyCollector;
use crate::collectors::metrics_history::MetricsHistory;
/// MacJet — Process Tree Widget
///
/// Hierarchical, expandable process tree with role bucketing,
/// group sparklines, severity rails, and alternating row colors.
use crate::collectors::process_collector::{ProcessGroup, ProcessInfo};
use crate::ui::styles;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Row, StatefulWidget, Table, TableState},
};
use rustc_hash::{FxHashMap, FxHashSet};
use smol_str::SmolStr;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub enum ProcessRowData<'a> {
    Group {
        group: &'a ProcessGroup,
        is_expanded: bool,
    },
    RoleBucket {
        group_key: SmolStr,
        role: SmolStr,
        count: usize,
        cpu: f64,
        mem_mb: f64,
        threads: u32,
        is_expanded: bool,
    },
    Process {
        process: &'a ProcessInfo,
        group: &'a ProcessGroup,
        depth: u8,
    },
    More {
        group_key: SmolStr,
        remaining: usize,
        cpu: f64,
        mem_mb: f64,
    },
}

pub struct ProcessTreeState {
    pub table_state: TableState,
    pub expanded_groups: FxHashSet<SmolStr>,
    pub expanded_roles: FxHashSet<String>, // Tracks "role-group_name-role"
    pub show_all_groups: FxHashSet<SmolStr>,
    pub user_moved_cursor: bool,
    pub row_keys: Vec<String>,
    pub last_selected_key: String,
}

impl Default for ProcessTreeState {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessTreeState {
    pub fn new() -> Self {
        Self {
            table_state: TableState::default(),
            expanded_groups: FxHashSet::default(),
            expanded_roles: FxHashSet::default(),
            show_all_groups: FxHashSet::default(),
            user_moved_cursor: false,
            row_keys: Vec::new(),
            last_selected_key: String::new(),
        }
    }

    pub fn toggle_selected(&mut self) -> bool {
        self.user_moved_cursor = true;
        let idx = match self.table_state.selected() {
            Some(i) => i,
            None => return false,
        };
        let key = match self.row_keys.get(idx) {
            Some(k) => k.clone(),
            None => return false,
        };

        if let Some(group_key) = key.strip_prefix("group-") {
            let gk = SmolStr::new(group_key);
            if self.expanded_groups.contains(&gk) {
                self.expanded_groups.remove(&gk);
            } else {
                self.expanded_groups.insert(gk);
            }
            return true;
        }

        if key.starts_with("role-") {
            if self.expanded_roles.contains(&key) {
                self.expanded_roles.remove(&key);
            } else {
                self.expanded_roles.insert(key);
            }
            return true;
        }

        if let Some(group_key) = key.strip_prefix("more-") {
            self.show_all_groups.insert(SmolStr::new(group_key));
            return true;
        }

        false
    }

    pub fn move_up(&mut self, lines: usize) {
        self.user_moved_cursor = true;
        let i = match self.table_state.selected() {
            Some(i) => i.saturating_sub(lines),
            None => 0,
        };
        self.table_state.select(Some(i));
        if i < self.row_keys.len() {
            self.last_selected_key = self.row_keys[i].clone();
        }
    }

    pub fn move_down(&mut self, lines: usize, max_items: usize) {
        self.user_moved_cursor = true;
        let i = match self.table_state.selected() {
            Some(i) => {
                if i + lines >= max_items {
                    max_items.saturating_sub(1)
                } else {
                    i + lines
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
        if i < self.row_keys.len() {
            self.last_selected_key = self.row_keys[i].clone();
        }
    }

    pub fn home(&mut self) {
        self.user_moved_cursor = true;
        self.table_state.select(Some(0));
        if !self.row_keys.is_empty() {
            self.last_selected_key = self.row_keys[0].clone();
        }
    }

    pub fn end(&mut self, max_items: usize) {
        self.user_moved_cursor = true;
        let i = max_items.saturating_sub(1);
        self.table_state.select(Some(i));
        if i < self.row_keys.len() {
            self.last_selected_key = self.row_keys[i].clone();
        }
    }

    pub fn current_row_key(&self) -> Option<&str> {
        let idx = self.table_state.selected()?;
        self.row_keys.get(idx).map(|s| s.as_str())
    }

    pub fn build_rows<'a>(&mut self, groups: &'a [ProcessGroup]) -> Vec<ProcessRowData<'a>> {
        let mut rows = Vec::new();
        let mut keys = Vec::new();

        for group in groups {
            if group.processes.len() == 1 {
                // Render as single process
                let p = &group.processes[0];
                rows.push(ProcessRowData::Process {
                    process: p,
                    group,
                    depth: 0,
                });
                keys.push(format!("pid-{}", p.pid));
            } else {
                let group_key = group.name.clone();
                let is_expanded = self.expanded_groups.contains(&group_key);

                // Render as group
                rows.push(ProcessRowData::Group { group, is_expanded });
                keys.push(format!("group-{}", group.name));

                // If expanded, render children via role buckets or flat
                if is_expanded {
                    let mut children: Vec<&ProcessInfo> = group.processes.iter().collect();
                    children.sort_by(|a, b| {
                        b.cpu_percent
                            .partial_cmp(&a.cpu_percent)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });

                    let roleful = children.iter().filter(|p| !p.role_type.is_empty()).count();
                    let use_role_buckets = roleful > 3 && children.len() > 5;

                    if use_role_buckets {
                        let mut buckets: FxHashMap<&str, Vec<&ProcessInfo>> = FxHashMap::default();
                        for p in &children {
                            let role = if p.role_type.is_empty() {
                                "other"
                            } else {
                                p.role_type.as_str()
                            };
                            buckets.entry(role).or_default().push(*p);
                        }

                        // Sort buckets by CPU sum descending
                        let mut ordered_roles: Vec<(&str, f64, usize, f64, u32)> = buckets
                            .iter()
                            .map(|(&role, procs)| {
                                let mut cpu_sum = 0.0;
                                let mut mem_sum = 0.0;
                                let mut threads_sum = 0;
                                for p in procs {
                                    cpu_sum += p.cpu_percent;
                                    mem_sum += p.memory_mb;
                                    threads_sum += p.num_threads;
                                }
                                (role, cpu_sum, procs.len(), mem_sum, threads_sum)
                            })
                            .collect();
                        ordered_roles.sort_by(|a, b| {
                            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                        });

                        for (role, cpu, count, mem_mb, threads) in ordered_roles {
                            let role_key = format!("role-{}-{}", group.name, role);

                            rows.push(ProcessRowData::RoleBucket {
                                group_key: group.name.clone(),
                                role: SmolStr::new(role),
                                count,
                                cpu,
                                mem_mb,
                                threads,
                                is_expanded: self.expanded_roles.contains(&role_key),
                            });
                            keys.push(role_key.clone());

                            if self.expanded_roles.contains(&role_key) {
                                let procs = buckets.get(role).unwrap();
                                let show_all = self.show_all_groups.contains(&group_key);
                                let visible = if show_all {
                                    procs.len()
                                } else {
                                    procs.len().min(15)
                                };

                                for p in procs.iter().take(visible) {
                                    rows.push(ProcessRowData::Process {
                                        process: p,
                                        group,
                                        depth: 2,
                                    });
                                    keys.push(format!("child-{}", p.pid));
                                }

                                if procs.len() > visible {
                                    let rest = &procs[visible..];
                                    let rem_cpu: f64 = rest.iter().map(|p| p.cpu_percent).sum();
                                    let rem_mem: f64 = rest.iter().map(|p| p.memory_mb).sum();
                                    rows.push(ProcessRowData::More {
                                        group_key: group.name.clone(),
                                        remaining: rest.len(),
                                        cpu: rem_cpu,
                                        mem_mb: rem_mem,
                                    });
                                    keys.push(format!("more-{}", group.name));
                                }
                            }
                        }
                    } else {
                        let show_all = self.show_all_groups.contains(group.name.as_str());
                        let visible = if show_all {
                            children.len()
                        } else {
                            children.len().min(15)
                        };

                        for p in children.iter().take(visible) {
                            rows.push(ProcessRowData::Process {
                                process: p,
                                group,
                                depth: 1,
                            });
                            keys.push(format!("child-{}", p.pid));
                        }

                        if children.len() > visible {
                            let rest = &children[visible..];
                            let rem_cpu: f64 = rest.iter().map(|p| p.cpu_percent).sum();
                            let rem_mem: f64 = rest.iter().map(|p| p.memory_mb).sum();
                            rows.push(ProcessRowData::More {
                                group_key: group.name.clone(),
                                remaining: rest.len(),
                                cpu: rem_cpu,
                                mem_mb: rem_mem,
                            });
                            keys.push(format!("more-{}", group.name));
                        }
                    }
                }
            }
        }

        // Apply scroll-snap / selection preservation logic
        if !self.user_moved_cursor && !keys.is_empty() {
            self.table_state.select(Some(0));
        } else if self.user_moved_cursor && !keys.is_empty() {
            let mut new_pos = None;

            // Try to find the exactly matching key
            if !self.last_selected_key.is_empty() {
                if let Some(pos) = keys.iter().position(|k| k == &self.last_selected_key) {
                    new_pos = Some(pos);
                }
            }

            // If not found, clamp to the bottom of the list
            if new_pos.is_none() {
                if let Some(selected) = self.table_state.selected() {
                    new_pos = Some(selected.min(keys.len().saturating_sub(1)));
                } else {
                    new_pos = Some(0);
                }
            }

            self.table_state.select(new_pos);
        }

        // Update tracking
        if let Some(idx) = self.table_state.selected() {
            if idx < keys.len() {
                self.last_selected_key = keys[idx].clone();
            }
        }

        self.row_keys = keys;
        rows
    }
}

pub struct ProcessTreeWidget<'a> {
    pub metrics_history: &'a MetricsHistory,
    pub energy_collector: &'a EnergyCollector,
    pub rows_data: &'a [ProcessRowData<'a>],
    pub table_state: &'a mut TableState,
    pub interaction_pause_until: f64,
}

impl<'a> ProcessTreeWidget<'a> {
    pub fn new(
        metrics_history: &'a MetricsHistory,
        energy_collector: &'a EnergyCollector,
        rows_data: &'a [ProcessRowData<'a>],
        table_state: &'a mut TableState,
        interaction_pause_until: f64,
    ) -> Self {
        Self {
            metrics_history,
            energy_collector,
            rows_data,
            table_state,
            interaction_pause_until,
        }
    }

    fn format_mem(mb: f64) -> String {
        styles::format_mem(mb)
    }

    fn group_sparkline(&self, pids: &[u32]) -> String {
        self.metrics_history.sparkline_for_group(pids, 10)
    }

    fn process_sparkline(&self, pid: u32) -> String {
        self.metrics_history.sparkline(pid, 10, "cpu")
    }
}

impl<'a> ratatui::widgets::Widget for ProcessTreeWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let header = Row::new(vec![
            Cell::from(""), // Rail
            Cell::from(""), // Icon
            Cell::from("Process"),
            Cell::from("CPU %"),
            Cell::from("Memory"),
            Cell::from("Trend"),
            Cell::from("Threads"),
            Cell::from("Energy"),
            Cell::from("Context"),
        ])
        .style(Style::default().fg(styles::TEXT_DIM).bg(styles::BG_HEADER))
        .height(1)
        .bottom_margin(0);

        let mut ui_rows = Vec::new();

        for (idx, row_data) in self.rows_data.iter().enumerate() {
            // Alternating backgrounds (like Python version)
            let base_bg = if idx % 2 == 1 {
                styles::BG_ODD_ROW
            } else {
                styles::BG_DARK
            };

            let style = Style::default().bg(base_bg);

            let (
                rail,
                icon_cell,
                name_cell,
                cpu_cell,
                mem_cell,
                trend_cell,
                threads_cell,
                energy_cell,
                context_cell,
            ) = match row_data {
                ProcessRowData::Group { group, is_expanded } => {
                    let expand = if *is_expanded { "▾" } else { "▸" };
                    let display_name = {
                        let s = format!("{} ({} processes)", group.name, group.processes.len());
                        styles::truncate_ellipsis(&s, 30)
                    };
                    let pids: Vec<u32> = group.processes.iter().map(|p| p.pid).collect();
                    let spark = self.group_sparkline(&pids);

                    // Aggregate energy for the group
                    let child_energies: Vec<&str> = group
                        .processes
                        .iter()
                        .map(|p| p.energy_impact.as_str())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let group_energy = if child_energies.iter().any(|e| *e == "HIGH") {
                        "HIGH"
                    } else if child_energies.iter().any(|e| *e == "MED") {
                        "MED"
                    } else if child_energies.iter().any(|e| *e == "LOW") {
                        "LOW"
                    } else {
                        ""
                    };

                    let (r_text, r_style) = styles::severity_rail(group.total_cpu);
                    let energy_color = match group_energy {
                        "HIGH" => styles::ACCENT_RED,
                        "MED" => styles::ACCENT_AMBER,
                        "LOW" => styles::ACCENT_GREEN,
                        _ => styles::TEXT_DIM,
                    };

                    (
                        Cell::from(Span::styled(r_text, r_style)),
                        Cell::from(Span::styled(
                            format!("{}{}", expand, group.icon),
                            Style::default(),
                        )),
                        Cell::from(Span::styled(
                            display_name,
                            Style::default()
                                .fg(styles::TEXT_BRIGHT)
                                .add_modifier(Modifier::BOLD),
                        )),
                        Cell::from(Span::styled(
                            format!("{:.1}", group.total_cpu),
                            Style::default().fg(styles::cpu_color(group.total_cpu)),
                        )),
                        Cell::from(Span::styled(
                            Self::format_mem(group.total_memory_mb),
                            Style::default().fg(styles::mem_color(group.total_memory_mb)),
                        )),
                        Cell::from(Span::styled(
                            spark,
                            Style::default().fg(styles::ACCENT_CYAN),
                        )),
                        Cell::from(
                            group
                                .processes
                                .iter()
                                .map(|p| p.num_threads)
                                .sum::<u32>()
                                .to_string(),
                        ),
                        Cell::from(Span::styled(
                            group_energy.to_string(),
                            Style::default().fg(energy_color),
                        )),
                        Cell::from(Span::styled(
                            group.context_label.to_string(),
                            styles::style_dim(),
                        )),
                    )
                }
                ProcessRowData::RoleBucket {
                    group_key: _,
                    role,
                    count,
                    cpu,
                    mem_mb,
                    threads,
                    is_expanded,
                } => {
                    let expand = if *is_expanded { "▾" } else { "▸" };
                    let label = format!("  ├─ {}{} ×{}", expand, pretty_role(role), count);

                    let (r_text, r_style) = styles::severity_rail(*cpu);

                    (
                        Cell::from(Span::styled(r_text, r_style)),
                        Cell::from("  "),
                        Cell::from(Span::styled(label, styles::style_dim())),
                        Cell::from(Span::styled(
                            format!("{:.1}", cpu),
                            Style::default().fg(styles::cpu_color(*cpu)),
                        )),
                        Cell::from(Span::styled(
                            Self::format_mem(*mem_mb),
                            Style::default().fg(styles::mem_color(*mem_mb)),
                        )),
                        Cell::from(""),
                        Cell::from(threads.to_string()),
                        Cell::from(""),
                        Cell::from(Span::styled(
                            format!("{} processes", count),
                            Style::default().fg(styles::TEXT_DIM),
                        )),
                    )
                }
                ProcessRowData::Process {
                    process: p,
                    group: g,
                    depth,
                } => {
                    let indent = match depth {
                        0 => "",
                        1 => "  ├─ ",
                        _ => "    ├─ ",
                    };
                    let proc_name = if !p.context_label.is_empty() {
                        p.context_label.to_string()
                    } else if !p.role_type.is_empty() && *depth > 0 {
                        format!("{} #{}", pretty_role(p.role_type.as_str()), p.pid)
                    } else {
                        p.name.to_string()
                    };
                    let display_name =
                        styles::truncate_ellipsis(&format!("{}{}", indent, proc_name), 30);

                    let spark = self.process_sparkline(p.pid);
                    let energy_label = self.energy_collector.get_energy_label(p.pid);
                    let energy_color = match energy_label {
                        "HIGH" => styles::ACCENT_RED,
                        "MED" => styles::ACCENT_AMBER,
                        "LOW" => styles::ACCENT_GREEN,
                        _ => styles::TEXT_DIM,
                    };

                    let (r_text, r_style) = styles::severity_rail(p.cpu_percent);

                    let mut text_style = Style::default();
                    if p.is_system {
                        text_style = text_style.fg(Color::DarkGray);
                    } else {
                        text_style = text_style.fg(styles::TEXT_BRIGHT);
                    }

                    let context_cell = if !p.context_label.is_empty() && p.context_label != p.name {
                        Cell::from(Span::styled(
                            styles::truncate_ellipsis(p.context_label.as_str(), 20),
                            styles::style_dim(),
                        ))
                    } else {
                        Cell::from(Span::styled(
                            format!("[{}]", p.confidence),
                            styles::confidence_style(p.confidence.as_str()),
                        ))
                    };

                    (
                        Cell::from(Span::styled(r_text, r_style)),
                        if g.processes.len() == 1 {
                            Cell::from(format!("  {}", g.icon))
                        } else {
                            Cell::from("")
                        },
                        Cell::from(Span::styled(display_name, text_style)),
                        Cell::from(Span::styled(
                            format!("{:.1}", p.cpu_percent),
                            Style::default().fg(styles::cpu_color(p.cpu_percent)),
                        )),
                        Cell::from(Span::styled(
                            Self::format_mem(p.memory_mb),
                            Style::default().fg(styles::mem_color(p.memory_mb)),
                        )),
                        Cell::from(Span::styled(
                            spark,
                            Style::default().fg(styles::ACCENT_CYAN),
                        )),
                        Cell::from(p.num_threads.to_string()),
                        Cell::from(Span::styled(
                            energy_label.to_string(),
                            Style::default().fg(energy_color),
                        )),
                        context_cell,
                    )
                }
                ProcessRowData::More {
                    remaining,
                    cpu,
                    mem_mb,
                    ..
                } => (
                    Cell::from(" "),
                    Cell::from("  "),
                    Cell::from(Span::styled(
                        format!("  └─ {} hidden", remaining),
                        styles::style_dim(),
                    )),
                    Cell::from(Span::styled(
                        format!("{:.1}", cpu),
                        Style::default().fg(styles::TEXT_DIM),
                    )),
                    Cell::from(Span::styled(
                        Self::format_mem(*mem_mb),
                        Style::default().fg(styles::TEXT_DIM),
                    )),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(Span::styled(
                        "Enter to expand",
                        Style::default().fg(styles::TEXT_DIM),
                    )),
                ),
            };

            let cells = vec![
                rail,
                icon_cell,
                name_cell,
                cpu_cell,
                mem_cell,
                trend_cell,
                threads_cell,
                energy_cell,
                context_cell,
            ];

            ui_rows.push(Row::new(cells).style(style).height(1));
        }

        let table = Table::new(
            ui_rows,
            [
                Constraint::Length(1),  // rail
                Constraint::Length(3),  // icon
                Constraint::Length(32), // Process
                Constraint::Length(8),  // CPU
                Constraint::Length(8),  // Mem
                Constraint::Length(12), // Trend (Sparkline)
                Constraint::Length(7),  // Threads
                Constraint::Length(7),  // Energy
                Constraint::Min(10),    // Context
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::NONE))
        .row_highlight_style(
            Style::default()
                .fg(styles::ACCENT_BLUE)
                .bg(Color::Rgb(18, 59, 87))
                .add_modifier(Modifier::BOLD),
        ) // #123B57
        .highlight_symbol("");

        ratatui::widgets::StatefulWidget::render(table, area, buf, self.table_state);

        // Interaction Pause Overlay
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        if now < self.interaction_pause_until {
            let remaining = self.interaction_pause_until - now;
            let pause_text = format!(" Paused ({:.1}s) ", remaining);
            let pause_len = pause_text.chars().count() as u16;

            if area.width > pause_len + 2 && area.height > 2 {
                let rect = Rect {
                    x: area.x + area.width - pause_len - 2,
                    y: area.y + area.height - 2,
                    width: pause_len + 2,
                    height: 1,
                };

                let p = ratatui::widgets::Paragraph::new(pause_text).style(
                    Style::default()
                        .fg(Color::Rgb(253, 186, 53))
                        .bg(styles::BG_DARK),
                );
                ratatui::widgets::Widget::render(p, rect, buf);
            }
        }
    }
}

fn pretty_role(role: &str) -> &'static str {
    match role {
        "renderer" => "Renderer",
        "gpu-process" => "GPU",
        "utility" => "Utility",
        "extension" => "Extension",
        "crashpad-handler" => "Crashpad",
        "ppapi" => "Plugin",
        "broker" => "Broker",
        "other" => "Other",
        _ => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collectors::process_collector::ProcessInfo;

    fn make_mock_group(name: &str, count: u32) -> ProcessGroup {
        let mut procs = Vec::new();
        for i in 0..count {
            procs.push(ProcessInfo {
                pid: 1000 + i,
                name: SmolStr::new(format!("{}-{}", name, i)),
                cpu_percent: i as f64,
                memory_mb: 50.0,
                memory_percent: 0.0,
                num_threads: 2,
                cmdline: vec![],
                cwd: SmolStr::default(),
                exe: SmolStr::default(),
                ppid: 1,
                status: SmolStr::default(),
                create_time: 0.0,
                username: SmolStr::default(),
                children_pids: vec![],
                context_label: SmolStr::default(),
                confidence: SmolStr::new("exact"),
                energy_impact: SmolStr::default(),
                net_bytes_sent: 0,
                net_bytes_recv: 0,
                role_type: SmolStr::default(),
                is_hidden: false,
                launch_age_s: 0.0,
                is_system: false,
            });
        }

        ProcessGroup {
            name: SmolStr::new(name),
            icon: "🟢",
            total_cpu: 0.0,
            total_memory_mb: 0.0,
            total_net_recv: 0,
            total_net_sent: 0,
            energy_impact: SmolStr::default(),
            processes: procs,
            context_label: SmolStr::default(),
            confidence: SmolStr::default(),
            why_hot: SmolStr::default(),
            is_expanded: false,
        }
    }

    #[test]
    fn test_user_moved_cursor_starts_false() {
        let tree = ProcessTreeState::new();
        assert_eq!(tree.user_moved_cursor, false);
    }

    #[test]
    fn test_toggle_selected_sets_user_moved() {
        let mut tree = ProcessTreeState::new();
        tree.table_state.select(Some(0));
        tree.row_keys = vec!["group-test".to_string()];

        tree.toggle_selected();
        assert_eq!(tree.user_moved_cursor, true);
        assert!(tree.expanded_groups.contains("test"));

        tree.toggle_selected();
        assert!(!tree.expanded_groups.contains("test"));
    }

    #[test]
    fn test_nav_keys_set_user_moved() {
        let mut tree = ProcessTreeState::new();

        tree.move_down(1, 10);
        assert_eq!(tree.user_moved_cursor, true);
        assert_eq!(tree.table_state.selected(), Some(0)); // None + Down = 0

        tree.move_down(1, 10);
        assert_eq!(tree.table_state.selected(), Some(1)); // 0 + Down = 1

        tree.move_up(1);
        assert_eq!(tree.table_state.selected(), Some(0));

        tree.end(10);
        assert_eq!(tree.table_state.selected(), Some(9));

        tree.home();
        assert_eq!(tree.table_state.selected(), Some(0));
    }

    #[test]
    fn test_build_rows_pins_to_home_when_idle() {
        let mut tree = ProcessTreeState::new();

        let groups = vec![
            make_mock_group("A", 1),
            make_mock_group("B", 1),
            make_mock_group("C", 1),
        ];

        tree.build_rows(&groups);

        // Assert it pinned to 0
        assert_eq!(tree.table_state.selected(), Some(0));
        assert_eq!(tree.user_moved_cursor, false);
    }

    #[test]
    fn test_build_rows_restores_cursor_when_user_interacted() {
        let mut tree = ProcessTreeState::new();
        tree.user_moved_cursor = true;
        tree.last_selected_key = "pid-1002".to_string();

        let mut g1 = make_mock_group("A", 1);
        g1.processes[0].pid = 1000;
        let mut g2 = make_mock_group("B", 1);
        g2.processes[0].pid = 1001;
        let mut g3 = make_mock_group("C", 1);
        g3.processes[0].pid = 1002;

        let custom_groups = vec![g1, g2, g3];
        tree.build_rows(&custom_groups);

        assert_eq!(tree.table_state.selected(), Some(2));
    }

    #[test]
    fn test_build_rows_clamps_on_shrink() {
        let mut tree = ProcessTreeState::new();
        tree.user_moved_cursor = true;
        tree.table_state.select(Some(99));
        tree.last_selected_key = "vanished".to_string();

        let mut g1 = make_mock_group("A", 1);
        g1.processes[0].pid = 1000;
        let mut g2 = make_mock_group("B", 1);
        g2.processes[0].pid = 1001;
        let custom_groups = vec![g1, g2];

        tree.build_rows(&custom_groups);

        // Clamps to last row which is index 1
        assert_eq!(tree.table_state.selected(), Some(1));
    }

    #[test]
    fn test_toggle_more_expands_show_all() {
        let mut tree = ProcessTreeState::new();
        tree.table_state.select(Some(0));
        tree.row_keys = vec!["more-Chrome".to_string()];

        tree.toggle_selected();
        assert!(tree.show_all_groups.contains("Chrome"));
    }
}
