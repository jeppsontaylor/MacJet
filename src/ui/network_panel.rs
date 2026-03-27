use crate::collectors::network_collector::{format_bytes, format_bytes_per_s, NetSnapshot};
use crate::ui::styles;
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table, Widget},
};

pub struct NetworkPanelWidget<'a> {
    pub snapshot: &'a NetSnapshot,
}

impl<'a> NetworkPanelWidget<'a> {
    pub fn new(snapshot: &'a NetSnapshot) -> Self {
        Self { snapshot }
    }
}

impl<'a> Widget for NetworkPanelWidget<'a> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let header_cells = [
            "Interface",
            "Received/s",
            "Sent/s",
            "Total Recv",
            "Total Sent",
        ]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(styles::TEXT_DIM)));
        let header = Row::new(header_cells)
            .style(Style::default().bg(styles::BG_DARK))
            .height(1);

        let rows = self
            .snapshot
            .interfaces
            .iter()
            .enumerate()
            .map(|(i, inter)| {
                let bg = if i % 2 == 0 {
                    styles::BG_DARK
                } else {
                    styles::BG_MEDIUM
                };

                let recv_style = if inter.bytes_recv_per_s > 1024.0 * 10.0 {
                    Style::default()
                        .fg(styles::POOL_BLUE)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(styles::TEXT_BRIGHT)
                };

                let sent_style = if inter.bytes_sent_per_s > 1024.0 * 10.0 {
                    Style::default()
                        .fg(styles::AURORA_PINK)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(styles::TEXT_BRIGHT)
                };

                Row::new(vec![
                    Cell::from(inter.name.as_str()).style(Style::default().fg(styles::POOL_CYAN)),
                    Cell::from(format_bytes_per_s(inter.bytes_recv_per_s)).style(recv_style),
                    Cell::from(format_bytes_per_s(inter.bytes_sent_per_s)).style(sent_style),
                    Cell::from(format_bytes(inter.bytes_recv as f64))
                        .style(Style::default().fg(styles::TEXT_DIM)),
                    Cell::from(format_bytes(inter.bytes_sent as f64))
                        .style(Style::default().fg(styles::TEXT_DIM)),
                ])
                .style(Style::default().bg(bg))
                .height(1)
            });

        let table = Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Length(15),
                Constraint::Length(15),
                Constraint::Length(15),
                Constraint::Length(15),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Network Interfaces ")
                .border_style(Style::default().fg(styles::BORDER_DIM)),
        );

        ratatui::widgets::Widget::render(table, area, buf);
    }
}
