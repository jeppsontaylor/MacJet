use crate::ui::styles;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

pub struct HelpWidget;

impl Widget for HelpWidget {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(48, 54, 61)))
            .style(Style::default().bg(styles::BG_DARK));

        let help_text = vec![
            Line::from(vec![Span::styled(
                "  MacJet — Flight Deck  ",
                Style::default()
                    .fg(styles::POOL_BLUE)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Views:",
                Style::default().fg(styles::TEXT_DIM),
            )]),
            Line::from("    1-6    Switch view (6 = Disk space)"),
            Line::from("    Tab    Cycle views"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Navigation:",
                Style::default().fg(styles::TEXT_DIM),
            )]),
            Line::from("    ↑/↓    Move selection"),
            Line::from("    PgUp   Jump up 10 rows"),
            Line::from("    PgDn   Jump down 10 rows"),
            Line::from("    Home   Jump to top"),
            Line::from("    End    Jump to bottom"),
            Line::from("    Enter  Expand / collapse"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Actions:",
                Style::default().fg(styles::TEXT_DIM),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Disk (6):",
                Style::default().fg(styles::TEXT_DIM),
            )]),
            Line::from("    space    Mark file for Trash"),
            Line::from("    t        Confirm Trash (modal)"),
            Line::from("    u        Hint about last trashed batch"),
            Line::from("    R        Rescan folder"),
            Line::from("    Enter    Folder / Other bucket list  ·  bksp ↑  ·  ← closes Other"),
            Line::from("    d        Duplicate quick wins (modal) — pick keeper, then t Trash"),
            Line::from(""),
            Line::from("    /      Filter processes"),
            Line::from("    Esc    Clear filter"),
            Line::from("    s      Cycle sort mode"),
            Line::from("    k      Kill (SIGTERM)"),
            Line::from("    K      Force kill (SIGKILL)"),
            Line::from("    z      Suspend / resume"),
            Line::from("    Space  Pause / resume"),
            Line::from("    q      Quit"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  * some actions require sudo",
                Style::default()
                    .fg(styles::TEXT_DIM)
                    .add_modifier(Modifier::ITALIC),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  CLI:",
                Style::default().fg(styles::TEXT_DIM),
            )]),
            Line::from("    --no-ml      Disable CPU prediction (Predict tab shows OFF)"),
            Line::from("    --refresh N  Seconds between data ticks (default 1)"),
        ];

        let p = Paragraph::new(help_text).alignment(ratatui::layout::Alignment::Left);

        // Center the help box
        let help_area = centered_rect(60, 80, area);
        Clear.render(help_area, buf);
        p.block(block).render(help_area, buf);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
