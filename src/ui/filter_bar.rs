/// MacJet — Filter Bar Widget
///
/// Renders the filter input line beneath the tab bar when active.
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Widget,
};

use crate::ui::styles;

pub struct FilterBarWidget<'a> {
    pub value: &'a str,
}

impl<'a> Widget for FilterBarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        buf.set_style(
            area,
            Style::default().bg(styles::BG_DARK).fg(styles::TEXT_BRIGHT),
        );

        let line = Line::from(vec![
            Span::styled(" ", Style::default().bg(styles::BG_DARK)),
            Span::styled("/", Style::default().fg(styles::ACCENT_BLUE)),
            Span::styled(
                self.value.to_string(),
                Style::default().fg(styles::TEXT_BRIGHT),
            ),
            Span::styled("█", Style::default().fg(styles::ACCENT_BLUE)),
        ]);

        buf.set_line(area.x, area.y, &line, area.width);
    }
}
