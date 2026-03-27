/// MacJet — Reclaim Panel (Kill List)
/// Scored recommendation view showing which processes to kill and why.
use crate::collectors::metrics_history::ReclaimCandidate;
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Cell, Row, Table, TableState},
};

pub struct ReclaimPanelWidget<'a> {
    pub candidates: &'a [ReclaimCandidate],
    pub table_state: &'a mut TableState,
}

impl<'a> ReclaimPanelWidget<'a> {
    pub fn new(candidates: &'a [ReclaimCandidate], table_state: &'a mut TableState) -> Self {
        Self {
            candidates,
            table_state,
        }
    }

    fn score_color(score: u8) -> Color {
        if score >= 80 {
            Color::Rgb(255, 77, 109) // Danger
        } else if score >= 60 {
            Color::Rgb(255, 138, 76) // Warning
        } else if score >= 40 {
            Color::Rgb(253, 186, 53) // Orange
        } else if score >= 20 {
            Color::Rgb(167, 139, 250) // Purple
        } else {
            Color::Rgb(127, 141, 179) // Dim
        }
    }

    fn risk_badge(risk: &str) -> Span<'static> {
        match risk.to_lowercase().as_str() {
            "safe" => Span::styled("SAFE", Style::default().fg(Color::Rgb(50, 213, 131))),
            "review" => Span::styled("REVIEW", Style::default().fg(Color::Rgb(253, 186, 53))),
            "danger" => Span::styled("DANGER", Style::default().fg(Color::Rgb(255, 77, 109))),
            _ => Span::raw(risk.to_string()),
        }
    }

    fn format_mem(mb: f64) -> String {
        if mb >= 1024.0 {
            format!("{:.1}GB", mb / 1024.0)
        } else {
            format!("{:.0}MB", mb)
        }
    }
}

impl<'a> ratatui::widgets::Widget for ReclaimPanelWidget<'a> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let block = Block::default()
            .borders(Borders::NONE)
            .style(Style::default().bg(Color::Rgb(10, 15, 30)));

        let header_cells = vec![
            Cell::from(""),
            Cell::from("Score"),
            Cell::from("App"),
            Cell::from("Reclaim"),
            Cell::from("Risk"),
            Cell::from("Reason"),
            Cell::from("Action"),
        ];

        let header = Row::new(header_cells)
            .style(
                Style::default()
                    .fg(Color::Rgb(127, 141, 179))
                    .bg(Color::Rgb(16, 24, 43)),
            )
            .height(1)
            .bottom_margin(0);

        let mut rows = Vec::new();
        for (idx, candidate) in self.candidates.iter().filter(|c| c.score >= 5).enumerate() {
            let sc = Self::score_color(candidate.score);
            let rail = Span::styled("█", Style::default().fg(sc));
            let score_str = Span::styled(format!("{:3}", candidate.score), Style::default().fg(sc));

            let mut app_str = format!("{} {}", candidate.icon, candidate.app_name);
            if app_str.chars().count() > 23 {
                app_str = format!("{}…", app_str.chars().take(22).collect::<String>());
            }

            let reclaim_str = format!(
                "~{:.0}% / {}",
                candidate.reclaim_cpu,
                Self::format_mem(candidate.reclaim_mem_mb)
            );
            let risk_str = Self::risk_badge(candidate.risk.as_str());

            let mut reason = candidate.reason.to_string();
            if reason.chars().count() > 35 {
                reason = format!("{}…", reason.chars().take(34).collect::<String>());
            }

            let cells = vec![
                Cell::from(rail),
                Cell::from(score_str),
                Cell::from(app_str).style(Style::default().fg(Color::Rgb(230, 236, 255))),
                Cell::from(reclaim_str).style(Style::default().fg(Color::Rgb(230, 236, 255))),
                Cell::from(risk_str),
                Cell::from(reason).style(Style::default().fg(Color::Rgb(230, 236, 255))),
                Cell::from(candidate.suggested_action.as_str())
                    .style(Style::default().fg(Color::Rgb(96, 165, 250))),
            ];

            let base_bg = if idx % 2 == 1 {
                Color::Rgb(14, 20, 37) // #0E1425
            } else {
                Color::Rgb(10, 15, 30) // #0A0F1E
            };

            rows.push(
                Row::new(cells)
                    .style(Style::default().bg(base_bg))
                    .height(1),
            );
        }

        let table = Table::new(
            rows,
            &[
                Constraint::Length(1),
                Constraint::Length(6),
                Constraint::Length(24),
                Constraint::Length(16),
                Constraint::Length(8),
                Constraint::Length(36),
                Constraint::Length(14),
            ],
        )
        .header(header)
        .block(block)
        .row_highlight_style(
            Style::default()
                .fg(Color::Rgb(96, 165, 250))
                .bg(Color::Rgb(18, 59, 87))
                .add_modifier(Modifier::BOLD),
        ) // #123B57 cursor
        .highlight_symbol("");

        // Use standard StatefulWidget rendering pattern directly
        ratatui::widgets::StatefulWidget::render(table, area, buf, self.table_state);
    }
}
