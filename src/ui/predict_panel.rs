/// MacJet — CPU Prediction Panel
///
/// Renders the ML prediction dashboard with:
/// - Braille-based dual-line chart (actual CPU + predicted horizon)
/// - ±1σ confidence band behind prediction
/// - Bright "now" divider
/// - Stats bar with rows, features, inference latency, countdown, MAE
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use super::styles;
use crate::collectors::cpu_predictor::PredictorStats;

pub struct PredictPanelWidget<'a> {
    pub stats: &'a PredictorStats,
    pub current_cpu: f64,
    /// When `false`, show a grayed-out disabled state (started with `--no-ml`).
    pub ml_enabled: bool,
}

impl<'a> PredictPanelWidget<'a> {
    pub fn new(stats: &'a PredictorStats, current_cpu: f64, ml_enabled: bool) -> Self {
        Self {
            stats,
            current_cpu,
            ml_enabled,
        }
    }
}

impl<'a> Widget for PredictPanelWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 6 || area.width < 30 {
            return;
        }

        if !self.ml_enabled {
            self.render_ml_disabled(area, buf);
            return;
        }

        // Fill background
        buf.set_style(area, Style::default().bg(styles::BG_DARK));

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(&[
                Constraint::Length(1), // Title bar
                Constraint::Length(1), // Spacer
                Constraint::Min(6),    // Chart area
                Constraint::Length(1), // X-axis labels
                Constraint::Length(1), // Spacer
                Constraint::Length(1), // Stats bar
            ])
            .split(area);

        // ─── Title Bar ───────────────────────
        self.render_title(buf, layout[0]);

        // ─── Chart ───────────────────────────
        let chart_area = layout[2];
        self.render_chart(buf, chart_area);

        // ─── X-Axis Labels ───────────────────
        self.render_x_axis(buf, layout[3]);

        // ─── Stats Bar ──────────────────────
        self.render_stats(buf, layout[5]);
    }
}

impl<'a> PredictPanelWidget<'a> {
    /// Grayed-out Predict tab when `--no-ml` is set: border, dim title, red centered message.
    fn render_ml_disabled(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(styles::BORDER_DIM))
            .title(Line::from(vec![
                Span::styled("  ⚡ ", Style::default().fg(styles::ACCENT_AMBER)),
                Span::styled(
                    "CPU Prediction Engine",
                    Style::default()
                        .fg(styles::TEXT_DIM)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled("  ○ OFF ", Style::default().fg(styles::ACCENT_RED)),
            ]))
            .style(Style::default().bg(styles::BG_DARK));

        let inner = block.inner(area);
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(38),
                Constraint::Length(8),
                Constraint::Percentage(38),
            ])
            .split(inner);

        let text = vec![
            Line::from(vec![Span::styled(
                "ML prediction disabled",
                Style::default()
                    .fg(styles::ACCENT_RED)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Online RLS (CPU forecast) is not running.",
                Style::default().fg(styles::TEXT_DIM),
            )]),
            Line::from(vec![Span::styled(
                "Restart without --no-ml to enable.",
                Style::default().fg(styles::TEXT_DIM),
            )]),
        ];

        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().bg(styles::BG_DARK))
            .render(vertical[1], buf);

        block.render(area, buf);
    }

    fn render_title(&self, buf: &mut Buffer, area: Rect) {
        let status_badge = if self.stats.trained {
            Span::styled(
                " 🟢 ONLINE ",
                Style::default()
                    .fg(styles::ACCENT_GREEN)
                    .add_modifier(Modifier::BOLD),
            )
        } else if self.stats.rows >= 10 {
            Span::styled(
                " ⏳ WARMING UP ",
                Style::default()
                    .fg(styles::ACCENT_AMBER)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(" ○ COLLECTING ", Style::default().fg(styles::TEXT_DIM))
        };

        let cpu_val = format!(" {:.1}%", self.current_cpu);
        let cpu_color = styles::cpu_color(self.current_cpu);

        let line = Line::from(vec![
            Span::styled("  ⚡ ", Style::default().fg(styles::ACCENT_AMBER)),
            Span::styled(
                "CPU Prediction Engine",
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ", styles::style_dim()),
            status_badge,
            Span::styled("  │  CPU:", styles::style_dim()),
            Span::styled(
                cpu_val,
                Style::default().fg(cpu_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if self.stats.mae > 0.0 {
                    format!("  │  MAE: {:.1}%", self.stats.mae)
                } else {
                    String::new()
                },
                Style::default().fg(styles::ACCENT_VIOLET),
            ),
        ]);

        buf.set_style(area, Style::default().bg(styles::BG_HEADER));
        buf.set_line(area.x, area.y, &line, area.width);
    }

    fn render_chart(&self, buf: &mut Buffer, area: Rect) {
        let chart_w = area.width as usize;
        let chart_h = area.height as usize;

        if chart_w < 10 || chart_h < 3 {
            return;
        }

        // Y-axis label width
        let y_label_w = 5; // "100%|"
        let plot_w = chart_w.saturating_sub(y_label_w + 1);
        if plot_w < 10 {
            return;
        }

        // Prepare data: history (left half) + horizon (right half)
        let half_w = plot_w / 2;
        let history_data = self.resample(&self.stats.history, half_w);
        let horizon_data = self.resample(&self.stats.horizon, plot_w - half_w);
        let confidence = self.resample_confidence(&self.stats.confidence_band, plot_w - half_w);

        // Y range: 0..100
        let y_min: f64 = 0.0;
        let y_max: f64 = 100.0;

        // Render Y-axis labels
        let y_labels = ["100%", " 75%", " 50%", " 25%", "  0%"];
        for (i, label) in y_labels.iter().enumerate() {
            let y_pos = area.y + (i as u16 * (area.height.saturating_sub(1))) / 4;
            if y_pos < area.y + area.height {
                buf.set_string(area.x, y_pos, label, styles::style_dim());
                buf.set_string(
                    area.x + 4,
                    y_pos,
                    "│",
                    Style::default().fg(styles::BORDER_DIM),
                );
            }
        }

        let plot_x = area.x + y_label_w as u16;

        // ─── Render confidence band (background) ─────
        if self.stats.trained && !confidence.is_empty() {
            for col in 0..confidence.len().min(plot_w - half_w) {
                let (lo, hi) = confidence[col];
                let y_lo = self.val_to_row(lo, y_min, y_max, chart_h);
                let y_hi = self.val_to_row(hi, y_min, y_max, chart_h);

                let row_top = y_hi.min(y_lo);
                let row_bot = y_hi.max(y_lo);

                for row in row_top..=row_bot.min(chart_h.saturating_sub(1)) {
                    let bx = plot_x + (half_w + col) as u16;
                    let by = area.y + row as u16;
                    if bx < area.x + area.width && by < area.y + area.height {
                        buf.set_string(bx, by, "░", Style::default().fg(styles::BORDER_DIM));
                    }
                }
            }
        }

        // ─── Render "now" divider ─────
        let divider_x = plot_x + half_w as u16;
        if divider_x < area.x + area.width {
            for row in 0..chart_h {
                let by = area.y + row as u16;
                if by < area.y + area.height {
                    buf.set_string(
                        divider_x,
                        by,
                        "│",
                        Style::default()
                            .fg(styles::ACCENT_AMBER)
                            .add_modifier(Modifier::BOLD),
                    );
                }
            }
        }

        // ─── Render actual CPU line (cyan) ─────
        self.render_line(
            buf,
            &history_data,
            plot_x,
            area.y,
            half_w,
            chart_h,
            y_min,
            y_max,
            styles::ACCENT_CYAN,
        );

        // ─── Render prediction line (magenta/pink) ─────
        if self.stats.trained && !horizon_data.is_empty() {
            self.render_line(
                buf,
                &horizon_data,
                plot_x + half_w as u16 + 1,
                area.y,
                (plot_w - half_w).saturating_sub(1),
                chart_h,
                y_min,
                y_max,
                styles::AURORA_PINK,
            );
        } else if !horizon_data.is_empty() {
            // Flat line at last known value
            let flat_val = self.current_cpu;
            let flat_data = vec![flat_val; (plot_w - half_w).saturating_sub(1)];
            self.render_line(
                buf,
                &flat_data,
                plot_x + half_w as u16 + 1,
                area.y,
                (plot_w - half_w).saturating_sub(1),
                chart_h,
                y_min,
                y_max,
                styles::TEXT_DIM,
            );
        }
    }

    /// Render a single line series using braille-like block drawing.
    /// We use a simpler approach: map each data point to a row and draw block characters.
    fn render_line(
        &self,
        buf: &mut Buffer,
        data: &[f64],
        x_start: u16,
        y_start: u16,
        width: usize,
        height: usize,
        y_min: f64,
        y_max: f64,
        color: ratatui::style::Color,
    ) {
        if data.is_empty() || width == 0 || height == 0 {
            return;
        }

        // Use Unicode block element drawing for smooth lines
        let blocks = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

        for (i, &val) in data.iter().enumerate().take(width) {
            let x = x_start + i as u16;
            let row = self.val_to_row(val, y_min, y_max, height);

            // Draw the line point
            let y = y_start + row as u16;
            if x < x_start + width as u16 {
                // Calculate sub-position within the cell for block character selection
                let normalized = ((val - y_min) / (y_max - y_min)).clamp(0.0, 1.0);
                let total_sub = (height * 8) as f64;
                let sub_pos = (normalized * total_sub) as usize;
                let block_idx = sub_pos % 8;
                let block_char = blocks[block_idx.min(7)];

                if y < y_start + height as u16 {
                    buf.set_string(x, y, &block_char.to_string(), Style::default().fg(color));
                }

                // Draw a dim trail below the point for filled-area effect
                for trail_row in (row + 1)..height {
                    let ty = y_start + trail_row as u16;
                    if ty < y_start + height as u16 {
                        buf.set_string(
                            x,
                            ty,
                            "│",
                            Style::default().fg(color).add_modifier(Modifier::DIM),
                        );
                    }
                }
            }
        }

        // Draw connecting line segments between adjacent points
        for i in 1..data.len().min(width) {
            let x = x_start + i as u16;
            let row_prev = self.val_to_row(data[i - 1], y_min, y_max, height);
            let row_curr = self.val_to_row(data[i], y_min, y_max, height);

            // Fill vertical gaps between consecutive points
            let (top, bot) = if row_prev < row_curr {
                (row_prev + 1, row_curr)
            } else {
                (row_curr + 1, row_prev)
            };

            for r in top..bot {
                let gy = y_start + r as u16;
                if gy < y_start + height as u16 && x < x_start + width as u16 {
                    buf.set_string(
                        x,
                        gy,
                        "┊",
                        Style::default().fg(color).add_modifier(Modifier::DIM),
                    );
                }
            }
        }
    }

    fn val_to_row(&self, val: f64, y_min: f64, y_max: f64, height: usize) -> usize {
        let range = y_max - y_min;
        if range <= 0.0 || height == 0 {
            return 0;
        }
        let normalized = ((val - y_min) / range).clamp(0.0, 1.0);
        // Invert: high values at top (row 0)
        let row = ((1.0 - normalized) * (height - 1) as f64).round() as usize;
        row.min(height - 1)
    }

    fn render_x_axis(&self, buf: &mut Buffer, area: Rect) {
        let line = Line::from(vec![
            Span::styled("     ", styles::style_dim()),
            Span::styled("◀── 60s history ", Style::default().fg(styles::ACCENT_CYAN)),
            Span::styled("────", styles::style_dim()),
            Span::styled(
                " NOW ",
                Style::default()
                    .fg(styles::ACCENT_AMBER)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("────", styles::style_dim()),
            Span::styled(
                " 60s forecast ──▶",
                Style::default().fg(styles::AURORA_PINK),
            ),
        ]);

        buf.set_line(area.x, area.y, &line, area.width);
    }

    fn render_stats(&self, buf: &mut Buffer, area: Rect) {
        let countdown_str = format!("{}s", self.stats.countdown_secs);
        let countdown_color = if self.stats.countdown_secs <= 5 {
            styles::ACCENT_GREEN
        } else {
            styles::TEXT_BRIGHT
        };

        let line = Line::from(vec![
            Span::styled("  📊 ", styles::style_dim()),
            Span::styled(
                format!("{}", self.stats.rows),
                Style::default()
                    .fg(styles::ACCENT_CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" rows", styles::style_dim()),
            Span::styled("  │  ", styles::style_dim()),
            Span::styled("🔧 ", styles::style_dim()),
            Span::styled(
                format!("{}", self.stats.cols),
                Style::default()
                    .fg(styles::ACCENT_BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" features", styles::style_dim()),
            Span::styled("  │  ", styles::style_dim()),
            Span::styled("⏱ ", styles::style_dim()),
            Span::styled(
                if self.stats.last_inference_us > 0 {
                    format!("{}µs", self.stats.last_inference_us)
                } else {
                    "—".to_string()
                },
                Style::default()
                    .fg(styles::ACCENT_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" inference", styles::style_dim()),
            Span::styled("  │  ", styles::style_dim()),
            Span::styled("🔄 ", styles::style_dim()),
            Span::styled(
                countdown_str,
                Style::default()
                    .fg(countdown_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" next train", styles::style_dim()),
            Span::styled("  │  ", styles::style_dim()),
            Span::styled("🎯 MAE ", styles::style_dim()),
            Span::styled(
                if self.stats.mae > 0.0 {
                    format!("{:.1}%", self.stats.mae)
                } else {
                    "—".to_string()
                },
                Style::default()
                    .fg(styles::ACCENT_VIOLET)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);

        buf.set_style(area, Style::default().bg(styles::BG_HEADER));
        buf.set_line(area.x, area.y, &line, area.width);
    }

    // ─── Data resampling helpers ─────────────────────

    fn resample(&self, data: &[f64], target_len: usize) -> Vec<f64> {
        if target_len == 0 {
            return vec![];
        }
        if data.is_empty() {
            return vec![self.current_cpu; target_len]; // fill with current cpu for visual
        }
        if data.len() == target_len {
            return data.to_vec();
        }
        if data.len() > target_len {
            // Downsample
            let step = data.len() as f64 / target_len as f64;
            (0..target_len)
                .map(|i| {
                    let idx = (i as f64 * step) as usize;
                    data[idx.min(data.len() - 1)]
                })
                .collect()
        } else {
            // Pad with first value on the left
            let mut result = vec![data[0]; target_len - data.len()];
            result.extend_from_slice(data);
            result
        }
    }

    fn resample_confidence(&self, data: &[(f64, f64)], target_len: usize) -> Vec<(f64, f64)> {
        if target_len == 0 || data.is_empty() {
            return vec![];
        }
        if data.len() == target_len {
            return data.to_vec();
        }
        if data.len() > target_len {
            let step = data.len() as f64 / target_len as f64;
            (0..target_len)
                .map(|i| {
                    let idx = (i as f64 * step) as usize;
                    data[idx.min(data.len() - 1)]
                })
                .collect()
        } else {
            let mut result = vec![data[0]; target_len - data.len()];
            result.extend_from_slice(data);
            result
        }
    }
}
