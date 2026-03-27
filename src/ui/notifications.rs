/// MacJet — Notification System
///
/// A toast-style notification overlay that appears in the bottom-right corner.
/// Notifications auto-dismiss after a configurable TTL (default 3s).
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

use crate::ui::styles;

#[derive(Clone, Debug)]
pub struct Notification {
    pub text: String,
    pub created_at: Instant,
    pub ttl: Duration,
}

#[derive(Default)]
pub struct NotificationCenter {
    queue: VecDeque<Notification>,
}

impl NotificationCenter {
    pub fn push<T: Into<String>>(&mut self, text: T) {
        self.queue.push_back(Notification {
            text: text.into(),
            created_at: Instant::now(),
            ttl: Duration::from_secs(3),
        });
    }

    pub fn current(&self) -> Option<&Notification> {
        self.queue.front()
    }

    pub fn prune(&mut self) {
        while let Some(front) = self.queue.front() {
            if front.created_at.elapsed() > front.ttl {
                self.queue.pop_front();
            } else {
                break;
            }
        }
    }
}

pub struct NotificationOverlay<'a> {
    pub notification: &'a Notification,
}

impl<'a> Widget for NotificationOverlay<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = (self.notification.text.len() as u16 + 4).min(area.width.saturating_sub(2));
        let rect = Rect {
            x: area.x + area.width.saturating_sub(width + 1),
            y: area.y + area.height.saturating_sub(3),
            width,
            height: 3,
        };

        Clear.render(rect, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(styles::BORDER_DIM))
            .style(Style::default().bg(styles::BG_HEADER));

        let inner = block.inner(rect);
        block.render(rect, buf);

        Paragraph::new(Line::from(vec![Span::styled(
            format!(" {}", self.notification.text),
            Style::default()
                .fg(styles::ACCENT_AMBER)
                .add_modifier(Modifier::BOLD),
        )]))
        .style(Style::default().bg(styles::BG_HEADER))
        .render(inner, buf);
    }
}
