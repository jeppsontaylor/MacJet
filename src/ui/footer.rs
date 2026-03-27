/// MacJet — Footer Bar Widget
///
/// Renders the keybinding hints at the bottom.
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

use super::styles;

pub struct Footer {
    pub paused: bool,
    pub ml_enabled: bool,
}

impl Widget for Footer {
    fn render(self, area: Rect, buf: &mut Buffer) {
        buf.set_style(area, Style::default().bg(styles::BG_HEADER));

        let pause_label = if self.paused { "Resume" } else { "Pause" };

        let mut spans = vec![
            Span::styled(" q", styles::style_bold_cyan()),
            Span::styled(" Quit  ", styles::style_dim()),
            Span::styled("space", styles::style_bold_cyan()),
            Span::styled(format!(" {}  ", pause_label), styles::style_dim()),
            Span::styled("Enter", styles::style_bold_cyan()),
            Span::styled(" Expand  ", styles::style_dim()),
            Span::styled("s", styles::style_bold_cyan()),
            Span::styled(" Sort  ", styles::style_dim()),
            Span::styled("Tab", styles::style_bold_cyan()),
            Span::styled(" Views  ", styles::style_dim()),
            Span::styled("/", styles::style_bold_cyan()),
            Span::styled(" Filter  ", styles::style_dim()),
            Span::styled("?", styles::style_bold_cyan()),
            Span::styled(" Help", styles::style_dim()),
        ];
        if !self.ml_enabled {
            spans.push(Span::styled("  │  ", styles::style_dim()));
            spans.push(Span::styled(
                "Predict ML off (--no-ml)",
                Style::default()
                    .fg(styles::ACCENT_RED)
                    .add_modifier(Modifier::ITALIC),
            ));
        }

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}
