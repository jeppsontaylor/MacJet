/// MacJet — Header Bar Widget
///
/// Renders the top status strip:
///   🍎 MacJet · Mac16,5 · CPU ████░░░░ 15.6% · Mem 13.3/36.0GB · Swap 2.4GB
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Widget,
};

use super::styles;
use crate::app::AppState;
use crate::collectors::network_collector::format_bytes_per_s;

pub struct Header<'a> {
    pub app: &'a AppState,
}

impl<'a> Widget for Header<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Fill background
        buf.set_style(area, Style::default().bg(styles::BG_HEADER));

        let s = &self.app.system;

        // Build the CPU gauge bar (20 chars wide)
        let gauge_width = 20;
        let filled = ((s.cpu_percent / 100.0) * gauge_width as f64).round() as usize;
        let filled = filled.min(gauge_width);
        let empty = gauge_width - filled;
        let gauge_filled: String = "█".repeat(filled);
        let gauge_empty: String = "░".repeat(empty);

        let cpu_color = styles::cpu_color(s.cpu_percent);

        // --- Line 1: Brand + Hostname + CPU + Mem + Swap ---
        let mut spans1 = vec![
            Span::styled(
                if self.app.paused { " ⏸" } else { " 🔥" },
                styles::style_dim(),
            ),
            Span::styled(" MacJet", styles::style_bold_cyan()),
            Span::styled("  ·  ", styles::style_dim()),
            Span::styled(&s.hostname, Style::default().fg(styles::TEXT_BRIGHT)),
            Span::styled("  ·  CPU ", styles::style_dim()),
            Span::styled(gauge_filled, Style::default().fg(cpu_color)),
            Span::styled(gauge_empty, Style::default().fg(styles::BORDER_DIM)),
            Span::styled(
                format!("  {:.1}%", s.cpu_percent),
                Style::default().fg(cpu_color),
            ),
            Span::styled("  ·  Mem ", styles::style_dim()),
            Span::styled(
                format!("{:.1}/{:.1}GB", s.mem_used_gb, s.mem_total_gb),
                Style::default().fg(styles::mem_color(s.mem_used_gb * 1024.0)),
            ),
        ];

        if s.swap_used_gb > 0.01 {
            spans1.push(Span::styled("  ·  Swap ", styles::style_dim()));
            spans1.push(Span::styled(
                format!("{:.1}GB", s.swap_used_gb),
                Style::default().fg(if s.swap_used_gb > 1.0 {
                    styles::ACCENT_AMBER
                } else {
                    styles::ACCENT_GREEN
                }),
            ));
        }

        // --- Line 2: Thermal + Fan + GPU + Network ---
        let e = self.app.energy_collector.snapshot();
        let net = &self.app.network_collector.latest;

        let thermal_dot = match e.thermal.thermal_pressure.as_str() {
            "heavy" | "critical" | "sleeping" => {
                Span::styled("●", Style::default().fg(styles::ACCENT_RED))
            }
            "moderate" | "elevated" => Span::styled("●", Style::default().fg(styles::ACCENT_AMBER)),
            _ => Span::styled("●", Style::default().fg(styles::ACCENT_GREEN)),
        };

        let tp_label = if e.thermal.thermal_pressure.is_empty() {
            "Nominal".to_string()
        } else {
            let mut c = e.thermal.thermal_pressure.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        };

        let mut spans2 = vec![
            Span::styled("  Thermal: ", styles::style_dim()),
            thermal_dot,
            Span::styled(
                format!(" {} ", tp_label),
                Style::default().fg(styles::TEXT_BRIGHT),
            ),
        ];

        if e.thermal.cpu_die_temp > 0.0 {
            let temp_color = if e.thermal.cpu_die_temp > 90.0 {
                styles::ACCENT_RED
            } else if e.thermal.cpu_die_temp > 70.0 {
                styles::ACCENT_AMBER
            } else {
                styles::ACCENT_GREEN
            };
            spans2.push(Span::styled(" · ", styles::style_dim()));
            spans2.push(Span::styled(
                format!("{:.0}°C", e.thermal.cpu_die_temp),
                Style::default().fg(temp_color),
            ));
        }

        if e.thermal.fan_speed_rpm > 0 {
            spans2.push(Span::styled("  ·  Fan ", styles::style_dim()));
            spans2.push(Span::styled(
                format!("{}rpm", e.thermal.fan_speed_rpm),
                Style::default().fg(styles::TEXT_BRIGHT),
            ));
        }

        if e.thermal.gpu_active_percent > 0.0 {
            let gpu_color = styles::cpu_color(e.thermal.gpu_active_percent);
            spans2.push(Span::styled("  ·  GPU ", styles::style_dim()));
            spans2.push(Span::styled(
                format!("{:.0}%", e.thermal.gpu_active_percent),
                Style::default().fg(gpu_color),
            ));
        }

        let net_down = format_bytes_per_s(net.bytes_recv_per_s);
        let net_up = format_bytes_per_s(net.bytes_sent_per_s);

        spans2.push(Span::styled("  ·  Net ", styles::style_dim()));
        spans2.push(Span::styled(
            format!("↓{} ", net_down),
            Style::default().fg(styles::ACCENT_CYAN),
        ));
        spans2.push(Span::styled(
            format!("↑{}", net_up),
            Style::default().fg(styles::ACCENT_VIOLET),
        ));

        let line1 = Line::from(spans1);
        let line2 = Line::from(spans2);

        buf.set_line(area.x, area.y, &line1, area.width);
        if area.height > 1 {
            buf.set_line(area.x, area.y + 1, &line2, area.width);
        }
    }
}
