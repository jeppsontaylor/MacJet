/// MacJet — UI Style Constants & Color Ramp Helpers
///
/// Ports the Afterburner CPU ramp, Aurora Memory ramp,
/// severity rails, and formatting utilities from the Python theme.
use ratatui::style::{Color, Modifier, Style};

// ─── Afterburner CPU Color Ramp ────────────────────
// Maps CPU% to a color from cool cyan to hot pink.
const CPU_RAMP: &[(f64, Color)] = &[
    (5.0, Color::Rgb(34, 211, 238)),    // #22D3EE — cyan (cool)
    (20.0, Color::Rgb(59, 130, 246)),   // #3B82F6 — blue
    (50.0, Color::Rgb(139, 92, 246)),   // #8B5CF6 — violet
    (80.0, Color::Rgb(217, 70, 239)),   // #D946EF — magenta
    (999.0, Color::Rgb(251, 113, 133)), // #FB7185 — hot pink (critical)
];

pub fn cpu_color(pct: f64) -> Color {
    for &(threshold, color) in CPU_RAMP {
        if pct <= threshold {
            return color;
        }
    }
    CPU_RAMP.last().unwrap().1
}

// ─── Aurora Memory Color Ramp ──────────────────────
// Maps memory (MB) to a color from green to red.
const MEM_RAMP: &[(f64, Color)] = &[
    (100.0, Color::Rgb(52, 211, 153)),  // #34D399 — green (light)
    (500.0, Color::Rgb(163, 230, 53)),  // #A3E635 — lime
    (1000.0, Color::Rgb(245, 158, 11)), // #F59E0B — amber
    (2000.0, Color::Rgb(249, 115, 22)), // #F97316 — orange
    (99999.0, Color::Rgb(239, 68, 68)), // #EF4444 — red (critical)
];

pub fn mem_color(mb: f64) -> Color {
    for &(threshold, color) in MEM_RAMP {
        if mb <= threshold {
            return color;
        }
    }
    MEM_RAMP.last().unwrap().1
}

// ─── Severity Rail ─────────────────────────────────
// A single-char block whose color encodes CPU severity.
pub fn severity_rail(cpu: f64) -> (&'static str, Style) {
    if cpu > 100.0 {
        ("█", Style::default().fg(Color::Rgb(255, 77, 109)))
    } else if cpu > 50.0 {
        ("█", Style::default().fg(Color::Rgb(255, 138, 76)))
    } else if cpu > 25.0 {
        ("▐", Style::default().fg(Color::Rgb(253, 186, 53)))
    } else if cpu > 5.0 {
        ("▏", Style::default().fg(Color::Rgb(127, 141, 179)))
    } else {
        (" ", Style::default())
    }
}

// ─── Severity Icon (emoji) ─────────────────────────
pub fn severity_icon(cpu: f64) -> &'static str {
    if cpu > 100.0 {
        "🔴"
    } else if cpu > 50.0 {
        "🟠"
    } else if cpu > 25.0 {
        "🟡"
    } else {
        "🟢"
    }
}

// ─── Memory Formatting ────────────────────────────
pub fn format_mem(mb: f64) -> String {
    if mb >= 1024.0 {
        format!("{:.1}G", mb / 1024.0)
    } else {
        format!("{:.0}M", mb)
    }
}

// ─── Named Colors (theme palette) ──────────────────
pub const BG_DARK: Color = Color::Rgb(10, 15, 30); // #0A0F1E
pub const BG_HEADER: Color = Color::Rgb(16, 24, 43); // #10182B
pub const BORDER_DIM: Color = Color::Rgb(26, 37, 64); // #1A2540
pub const TEXT_DIM: Color = Color::Rgb(127, 141, 179); // #7F8DB3
pub const TEXT_BRIGHT: Color = Color::Rgb(224, 232, 255); // #E0E8FF
pub const ACCENT_CYAN: Color = Color::Rgb(69, 214, 255); // #45D6FF
pub const ACCENT_GREEN: Color = Color::Rgb(50, 213, 131); // #32D583
pub const ACCENT_BLUE: Color = Color::Rgb(96, 165, 250); // #60A5FA
pub const ACCENT_VIOLET: Color = Color::Rgb(167, 139, 250); // #A78BFA
pub const ACCENT_AMBER: Color = Color::Rgb(253, 186, 53); // #FDBA35
pub const ACCENT_RED: Color = Color::Rgb(255, 77, 109); // #FF4D6D

// Aliases for better semantics in some views
pub const POOL_BLUE: Color = ACCENT_BLUE;
pub const POOL_CYAN: Color = ACCENT_CYAN;
pub const AURORA_PINK: Color = Color::Rgb(251, 113, 133);
pub const BG_MEDIUM: Color = Color::Rgb(16, 24, 43); // Same as BG_HEADER
pub const BG_ODD_ROW: Color = Color::Rgb(14, 20, 37); // #0E1425 — alternating row

// ─── Common Styles ─────────────────────────────────
pub fn style_bold_cyan() -> Style {
    Style::default()
        .fg(ACCENT_CYAN)
        .add_modifier(Modifier::BOLD)
}

pub fn style_dim() -> Style {
    Style::default().fg(TEXT_DIM)
}

pub fn style_header() -> Style {
    Style::default()
        .fg(TEXT_BRIGHT)
        .add_modifier(Modifier::BOLD)
}

// ─── Confidence Badge Styles ───────────────────────
pub fn confidence_style(conf: &str) -> Style {
    let color = match conf {
        "exact" => ACCENT_GREEN,
        "window-exact" => ACCENT_BLUE,
        "app-exact" => ACCENT_VIOLET,
        "inferred" => ACCENT_AMBER,
        "grouped" => TEXT_DIM,
        _ => TEXT_DIM,
    };
    Style::default().fg(color)
}

pub fn style_badge(conf: &str) -> Style {
    confidence_style(conf)
}

// ─── Text Utilities ────────────────────────────────
pub fn truncate_ellipsis(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let take = max.saturating_sub(1);
        format!("{}…", s.chars().take(take).collect::<String>())
    }
}

// ─── Sparkline Characters ──────────────────────────
pub const SPARK_CHARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

pub fn sparkline_str(values: &[f64], width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if values.is_empty() {
        return " ".repeat(width);
    }

    let max_val = values.iter().cloned().fold(1.0_f64, f64::max);

    // Resample or pad to fit width
    let fitted: Vec<f64> = if values.len() > width {
        let step = values.len() as f64 / width as f64;
        (0..width)
            .map(|i| {
                let idx = (i as f64 * step) as usize;
                values[idx.min(values.len() - 1)]
            })
            .collect()
    } else if values.len() < width {
        let mut padded = vec![0.0; width - values.len()];
        padded.extend_from_slice(values);
        padded
    } else {
        values.to_vec()
    };

    fitted
        .iter()
        .map(|&v| {
            let normalized = (v / max_val).clamp(0.0, 1.0);
            let idx = (normalized * (SPARK_CHARS.len() - 1) as f64) as usize;
            SPARK_CHARS[idx.min(SPARK_CHARS.len() - 1)]
        })
        .collect()
}

// ─── Hash Coloring ─────────────────────────────────
pub fn color_hash(s: &str) -> Color {
    let mut hash: u32 = 0;
    for b in s.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(b as u32);
    }

    let palette = [
        ACCENT_CYAN,
        ACCENT_GREEN,
        ACCENT_BLUE,
        ACCENT_VIOLET,
        ACCENT_AMBER,
        Color::Rgb(255, 138, 76),  // Orange
        Color::Rgb(251, 113, 133), // Pink
    ];

    palette[(hash as usize) % palette.len()]
}
