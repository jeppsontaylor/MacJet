/// MacJet — Detail Panel (Inspector)
///
/// Shows dynamic metadata, sparklines, "Why Hot" analysis, role breakdowns,
/// memory growth rates, and action shortcuts for the selected process/group.
///
/// This panel occupies the right 38-char column of the main layout.
use crate::collectors::metrics_history::{MetricsHistory, ReclaimCandidate};
use crate::collectors::process_collector::{ProcessGroup, ProcessInfo};
use crate::ui::styles;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

pub struct DetailPanelWidget<'a> {
    pub process: Option<&'a ProcessInfo>,
    pub group: Option<&'a ProcessGroup>,
    pub reclaim_candidate: Option<&'a ReclaimCandidate>,
    pub reclaim_group: Option<&'a ProcessGroup>,
    pub metrics: &'a MetricsHistory,
}

impl<'a> DetailPanelWidget<'a> {
    pub fn new(
        process: Option<&'a ProcessInfo>,
        group: Option<&'a ProcessGroup>,
        metrics: &'a MetricsHistory,
    ) -> Self {
        Self {
            process,
            group,
            reclaim_candidate: None,
            reclaim_group: None,
            metrics,
        }
    }

    pub fn from_reclaim(
        candidate: Option<&'a ReclaimCandidate>,
        group: Option<&'a ProcessGroup>,
        metrics: &'a MetricsHistory,
    ) -> Self {
        Self {
            process: None,
            group: None,
            reclaim_candidate: candidate,
            reclaim_group: group,
            metrics,
        }
    }

    fn format_mem(mb: f64) -> String {
        if mb >= 1024.0 {
            format!("{:.1}GB", mb / 1024.0)
        } else {
            format!("{:.0}MB", mb)
        }
    }

    fn format_duration(seconds: f64) -> String {
        if seconds < 60.0 {
            format!("{:.0}s", seconds)
        } else if seconds < 3600.0 {
            format!("{:.0}m", seconds / 60.0)
        } else {
            let h = (seconds / 3600.0) as u32;
            let m = ((seconds % 3600.0) / 60.0) as u32;
            if m > 0 {
                format!("{}h{}m", h, m)
            } else {
                format!("{}h", h)
            }
        }
    }

    /// Analyzes why a hot group is burning CPU and returns human-readable reasons.
    fn why_hot(group: &ProcessGroup, metrics: &MetricsHistory) -> Vec<String> {
        let mut reasons = Vec::new();

        if group.total_cpu > 80.0 {
            reasons.push("Sustained high CPU usage".to_string());
        } else if group.total_cpu > 30.0 {
            reasons.push("Elevated CPU usage".to_string());
        }

        let renderers = group
            .processes
            .iter()
            .filter(|p| p.role_type.as_str() == "renderer")
            .count();
        if renderers > 10 {
            reasons.push(format!("Renderer storm: {} renderers", renderers));
        }

        let growth: f64 = group
            .processes
            .iter()
            .map(|p| metrics.memory_growth_rate(p.pid, 60.0).max(0.0))
            .sum();
        if growth > 20.0 {
            reasons.push(format!("Memory growing +{:.0}MB/min", growth));
        }

        if group.processes.iter().all(|p| p.is_hidden || p.is_system) {
            reasons.push("Running in background".to_string());
        }

        let high_energy = group
            .processes
            .iter()
            .filter(|p| p.energy_impact.as_str() == "HIGH")
            .count();
        if high_energy > 0 {
            reasons.push(format!("{} high-energy processes", high_energy));
        }

        if reasons.is_empty() {
            reasons.push("Active usage".to_string());
        }

        reasons
    }

    fn sparkline_for_pid(&self, pid: u32, width: usize) -> String {
        self.metrics.sparkline(pid, width, "cpu")
    }

    fn sparkline_for_group(&self, pids: &[u32], width: usize) -> String {
        self.metrics.sparkline_for_group(pids, width)
    }
}

impl<'a> Widget for DetailPanelWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(styles::BORDER_DIM))
            .style(Style::default().bg(styles::BG_HEADER));

        let title = if let Some(c) = self.reclaim_candidate {
            format!("  ⚡ {}", c.app_name)
        } else if let Some(g) = self.group {
            format!("  📋 {} ({})", g.name, g.processes.len())
        } else if let Some(p) = self.process {
            let name = if p.context_label.is_empty() {
                p.name.as_str()
            } else {
                p.context_label.as_str()
            };
            format!("  📋 {}", name)
        } else {
            "  Inspector".to_string()
        };

        let mut lines: Vec<Line> = Vec::new();

        // --- Title Line ---
        lines.push(Line::from(Span::styled(
            title,
            Style::default()
                .fg(styles::ACCENT_BLUE)
                .add_modifier(Modifier::BOLD),
        )));

        // === Reclaim Candidate Mode ===
        if let Some(c) = self.reclaim_candidate {
            if let Some(g) = self.reclaim_group {
                let pids: Vec<u32> = g.processes.iter().map(|p| p.pid).collect();
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        self.sparkline_for_group(&pids, 28),
                        Style::default().fg(styles::ACCENT_CYAN),
                    ),
                ]));
                lines.push(Line::from(""));
            }

            let risk_color = match c.risk.as_str() {
                "safe" => styles::ACCENT_GREEN,
                "review" => styles::ACCENT_AMBER,
                "danger" => styles::ACCENT_RED,
                _ => styles::TEXT_DIM,
            };

            kv(
                &mut lines,
                "Score:",
                &format!("{}/100", c.score),
                styles::TEXT_BRIGHT,
            );
            kv(&mut lines, "Risk:", &c.risk.to_uppercase(), risk_color);
            kv(
                &mut lines,
                "Reclaim:",
                &format!(
                    "~{:.0}% CPU / {}",
                    c.reclaim_cpu,
                    Self::format_mem(c.reclaim_mem_mb)
                ),
                styles::TEXT_BRIGHT,
            );

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Reason:", Style::default().fg(risk_color)),
            ]));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    c.reason.to_string(),
                    Style::default().fg(styles::TEXT_BRIGHT),
                ),
            ]));

            lines.push(Line::from(""));
            kv(
                &mut lines,
                "Suggested:",
                c.suggested_action.as_str(),
                styles::ACCENT_BLUE,
            );
            kv(
                &mut lines,
                "Children:",
                &c.child_count.to_string(),
                styles::TEXT_BRIGHT,
            );

            if c.launch_age_s > 0.0 {
                kv(
                    &mut lines,
                    "Age:",
                    &Self::format_duration(c.launch_age_s),
                    styles::TEXT_BRIGHT,
                );
            }
        }
        // === Group Mode ===
        else if let Some(g) = self.group {
            let pids: Vec<u32> = g.processes.iter().map(|p| p.pid).collect();
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    self.sparkline_for_group(&pids, 28),
                    Style::default().fg(styles::ACCENT_CYAN),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "60s CPU trend (group)",
                    Style::default().fg(styles::TEXT_DIM),
                ),
            ]));
            lines.push(Line::from(""));

            kv(
                &mut lines,
                "CPU:",
                &format!("{:.1}%", g.total_cpu),
                styles::TEXT_BRIGHT,
            );
            kv(
                &mut lines,
                "Memory:",
                &Self::format_mem(g.total_memory_mb),
                styles::TEXT_BRIGHT,
            );
            kv(
                &mut lines,
                "Procs:",
                &g.processes.len().to_string(),
                styles::TEXT_BRIGHT,
            );

            // Memory growth rate
            let total_growth: f64 = g
                .processes
                .iter()
                .map(|p| self.metrics.memory_growth_rate(p.pid, 60.0).max(0.0))
                .sum();
            if total_growth > 1.0 {
                let color = if total_growth > 50.0 {
                    styles::ACCENT_RED
                } else {
                    styles::ACCENT_AMBER
                };
                kv(
                    &mut lines,
                    "Δ Mem:",
                    &format!("+{:.0}MB/min", total_growth),
                    color,
                );
            }

            // Why Hot analysis
            if g.total_cpu > 10.0 {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("🔥 Why hot:", Style::default().fg(Color::Rgb(255, 138, 76))),
                ]));
                for reason in Self::why_hot(g, self.metrics) {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(reason, Style::default().fg(styles::TEXT_BRIGHT)),
                    ]));
                }
            }

            // Role breakdown
            let mut roles: std::collections::BTreeMap<String, (usize, f64, f64)> =
                std::collections::BTreeMap::new();
            for p in &g.processes {
                let role = if p.role_type.is_empty() {
                    "main".to_string()
                } else {
                    p.role_type.to_string()
                };
                let entry = roles.entry(role).or_insert((0, 0.0, 0.0));
                entry.0 += 1;
                entry.1 += p.cpu_percent;
                entry.2 += p.memory_mb;
            }

            if roles.len() > 1 {
                lines.push(Line::from(""));
                section_header(&mut lines, "─── Breakdown ─────────");
                let mut sorted: Vec<_> = roles.into_iter().collect();
                sorted.sort_by(|a, b| {
                    b.1 .1
                        .partial_cmp(&a.1 .1)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                for (role, (count, cpu, mem)) in sorted {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(
                            format!("{} ×{}  ", titlecase(&role), count),
                            Style::default().fg(styles::TEXT_BRIGHT),
                        ),
                        Span::styled(
                            format!("{:.1}%  {}", cpu, Self::format_mem(mem)),
                            Style::default().fg(styles::TEXT_DIM),
                        ),
                    ]));
                }
            }
        }
        // === Single Process Mode ===
        else if let Some(p) = self.process {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    self.sparkline_for_pid(p.pid, 28),
                    Style::default().fg(styles::ACCENT_CYAN),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("60s CPU trend", Style::default().fg(styles::TEXT_DIM)),
            ]));
            lines.push(Line::from(""));

            kv(&mut lines, "PID:", &p.pid.to_string(), styles::TEXT_BRIGHT);
            kv(
                &mut lines,
                "CPU:",
                &format!("{:.1}%", p.cpu_percent),
                styles::TEXT_BRIGHT,
            );
            kv(
                &mut lines,
                "Memory:",
                &Self::format_mem(p.memory_mb),
                styles::TEXT_BRIGHT,
            );
            kv(
                &mut lines,
                "Threads:",
                &p.num_threads.to_string(),
                styles::TEXT_BRIGHT,
            );
            kv(
                &mut lines,
                "Status:",
                p.status.as_str(),
                styles::TEXT_BRIGHT,
            );

            if p.launch_age_s > 0.0 {
                kv(
                    &mut lines,
                    "Age:",
                    &Self::format_duration(p.launch_age_s),
                    styles::TEXT_BRIGHT,
                );
            }

            if !p.exe.is_empty() {
                let exe = if p.exe.chars().count() > 30 {
                    format!(
                        "…{}",
                        p.exe
                            .chars()
                            .rev()
                            .take(29)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect::<String>()
                    )
                } else {
                    p.exe.to_string()
                };
                kv(&mut lines, "Exe:", &exe, styles::TEXT_BRIGHT);
            }

            if !p.role_type.is_empty() {
                kv(
                    &mut lines,
                    "Role:",
                    p.role_type.as_str(),
                    styles::ACCENT_VIOLET,
                );
            }

            if p.is_system {
                kv(&mut lines, "Type:", "System", Color::Rgb(255, 138, 76));
            }

            if !p.energy_impact.is_empty() {
                let color = match p.energy_impact.as_str() {
                    "HIGH" => styles::ACCENT_RED,
                    "MED" => Color::Rgb(255, 138, 76),
                    _ => styles::ACCENT_GREEN,
                };
                kv(&mut lines, "Energy:", p.energy_impact.as_str(), color);
            }

            let growth = self.metrics.memory_growth_rate(p.pid, 60.0);
            if growth.abs() > 1.0 {
                let color = if growth > 10.0 {
                    styles::ACCENT_RED
                } else {
                    styles::ACCENT_AMBER
                };
                kv(
                    &mut lines,
                    "Δ Mem:",
                    &format!("{:+.0}MB/min", growth),
                    color,
                );
            }
        }
        // === Empty / Nothing Selected ===
        else {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Select a process to inspect",
                    Style::default().fg(styles::TEXT_DIM),
                ),
            ]));
        }

        // --- Actions Section ---
        lines.push(Line::from(""));
        section_header(&mut lines, "─── Actions ───────────");
        lines.push(actions_line("k", "Kill", "K", "Force Kill"));
        lines.push(actions_line("z", "Suspend", "/", "Filter"));

        let widget = Paragraph::new(lines).block(block);
        widget.render(area, buf);
    }
}

fn kv(lines: &mut Vec<Line<'static>>, key: &str, val: &str, color: Color) {
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{:<8}", key), Style::default().fg(styles::TEXT_DIM)),
        Span::styled(val.to_string(), Style::default().fg(color)),
    ]));
}

fn section_header(lines: &mut Vec<Line<'static>>, text: &str) {
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(text.to_string(), Style::default().fg(styles::TEXT_DIM)),
    ]));
}

fn actions_line(a1: &str, l1: &str, a2: &str, l2: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(a1.to_string(), Style::default().fg(styles::ACCENT_BLUE)),
        Span::raw(format!(" {}  ", l1)),
        Span::styled(a2.to_string(), Style::default().fg(styles::ACCENT_BLUE)),
        Span::raw(format!(" {}", l2)),
    ])
}

fn titlecase(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
