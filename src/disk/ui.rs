//! Disk view layout: metrics, progress, treemap/table, inspector, log + sparkline, confirm modal.

use crate::disk::model::{EntryRow, ScanPhase};
use crate::disk::treemap::{
    compute_neighbors, layout_entries, TreeMapState, TreeMapWidget, DEFAULT_MAX_TREEMAP_TILES,
};
use crate::disk::{DiskViewModel, DupReviewPhase};
use crate::ui::styles;
use ratatui::style::Color;
use ratatui::widgets::Clear;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table, Widget},
    Frame,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

pub fn format_bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", n as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if n >= 1024 * 1024 {
        format!("{:.2} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

fn marked_signature(marked: &std::collections::HashSet<PathBuf>) -> u64 {
    let mut paths: Vec<_> = marked.iter().collect();
    paths.sort();
    let mut h = DefaultHasher::new();
    for p in paths {
        p.hash(&mut h);
    }
    h.finish()
}

fn sparkline_ascii(samples: &std::collections::VecDeque<f64>, width: usize) -> String {
    if samples.is_empty() || width == 0 {
        return "—".repeat(width.min(20));
    }
    let max = samples.iter().cloned().fold(0.0_f64, f64::max).max(1.0);
    let chars = "▁▂▃▄▅▆▇█";
    let take = samples.len().min(width);
    let skip = samples.len().saturating_sub(take);
    let mut s = String::with_capacity(take);
    for v in samples.iter().skip(skip) {
        let i = ((v / max) * 7.0).floor() as usize;
        let i = i.min(7);
        s.push(chars.chars().nth(i).unwrap_or('▁'));
    }
    while s.len() < width {
        s.push(' ');
    }
    s
}

pub fn disk_use_reduced_color() -> bool {
    supports_color::on_cached(supports_color::Stream::Stdout)
        .map(|l| !(l.has_16m || l.has_256))
        .unwrap_or(true)
}

pub fn render_disk_view(f: &mut Frame, disk: &mut DiskViewModel, area: Rect) {
    disk.reduced_color = disk_use_reduced_color();
    disk.try_refresh_dup_quick_wins();
    let show_treemap = area.width >= 110;
    disk.layout_treemap = show_treemap;

    let search_h = if disk.search_bar_active { 1u16 } else { 0u16 };
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(search_h),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    render_metrics_strip(f, disk, outer[0]);
    render_progress_strip(f, disk, outer[1]);

    let (body_area, log_area) = if disk.search_bar_active {
        let search_area = outer[2];
        let line = Line::from(vec![
            Span::styled(" / ", styles::style_bold_cyan()),
            Span::styled(
                format!("{}▌", disk.search_buffer),
                Style::default().fg(styles::TEXT_BRIGHT),
            ),
        ]);
        Paragraph::new(line)
            .style(Style::default().bg(styles::BG_DARK))
            .render(search_area, f.buffer_mut());
        (outer[3], outer[4])
    } else {
        (outer[2], outer[3])
    };

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(3), Constraint::Fill(2)])
        .split(body_area);

    if show_treemap {
        if disk.other_drill.is_some() {
            disk.last_treemap_screen_rect = None;
            render_other_drill_list(f, disk, body[0]);
        } else if disk.pipeline_active() && disk.children.is_empty() {
            disk.last_treemap_screen_rect = None;
            render_treemap_placeholder(f, disk, body[0]);
        } else {
            disk.last_treemap_screen_rect = Some(body[0]);
            let key = (
                body[0].width,
                body[0].height,
                disk.layout_children_sig,
                marked_signature(&disk.marked),
            );
            if disk.last_treemap_key != Some(key) {
                disk.tiles = layout_entries(
                    body[0],
                    &disk.children,
                    &disk.marked,
                    DEFAULT_MAX_TREEMAP_TILES,
                );
                disk.tree_state.neighbors = compute_neighbors(&disk.tiles);
                disk.last_treemap_key = Some(key);
            }
            if disk.tree_state.focus >= disk.tiles.len() && !disk.tiles.is_empty() {
                disk.tree_state.focus = 0;
            }
            let widget = TreeMapWidget {
                tiles: disk.tiles.as_slice(),
                use_256_color: disk.reduced_color,
            };
            f.render_stateful_widget(widget, body[0], &mut disk.tree_state);
        }
    } else {
        disk.last_treemap_screen_rect = None;
        render_list_table(f, disk, body[0]);
    }

    render_inspector(f, disk, body[1]);
    render_log_strip(f, disk, log_area);

    if disk.dup_review.is_some() {
        render_dup_review_modal(f, disk, area);
    }
    if disk.confirm_delete {
        render_confirm_modal(f, disk, area);
    }
}

fn render_metrics_strip(f: &mut Frame, disk: &DiskViewModel, area: Rect) {
    let watch = match disk.phase {
        ScanPhase::Watching => "watch on",
        ScanPhase::Ready => "ready",
        ScanPhase::Walking | ScanPhase::Aggregating => "scan",
        ScanPhase::Hashing => "hash",
        ScanPhase::Idle => "idle",
        ScanPhase::Error => "err",
    };
    let idx_part = if disk.pipeline_active() {
        if disk.read_conn.is_none() {
            format!("walked {} · DB opening…", format_bytes(disk.bytes),)
        } else {
            format!(
                "{} in DB (partial) · walked {}",
                format_bytes(disk.indexed_bytes_total),
                format_bytes(disk.bytes),
            )
        }
    } else {
        format!("{} indexed", format_bytes(disk.indexed_bytes_total))
    };
    let line = Line::from(vec![
        Span::styled(" idx ", styles::style_bold_cyan()),
        Span::styled(
            format!(
                "{}  ·  {} dup groups  ·  reclaim {}  ·  {}  ",
                idx_part,
                disk.dup_groups,
                format_bytes(disk.reclaim_bytes),
                watch
            ),
            Style::default().fg(styles::TEXT_BRIGHT),
        ),
    ]);
    Paragraph::new(line)
        .style(Style::default().bg(styles::BG_HEADER))
        .render(area, f.buffer_mut());
}

fn render_treemap_placeholder(f: &mut Frame, disk: &DiskViewModel, area: Rect) {
    let msg = if disk.read_conn.is_none() {
        "Index database is starting. Treemap appears once the first batches are committed."
    } else if disk.indexed_bytes_total > 0 {
        "No entries for this folder in the index yet (scan still filling the tree). \
         Treemap updates as data arrives. If this persists with data in the index bar, \
         the index root uses a canonical path so treemap keys match the database."
    } else {
        "No entries for this folder in the index yet (scan still filling the tree). \
         Treemap updates as data arrives."
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(styles::BORDER_DIM))
        .title(format!(" {} ", disk.phase.label()))
        .title_style(styles::style_bold_cyan())
        .style(Style::default().bg(styles::BG_DARK));
    let inner = block.inner(area);
    block.render(area, f.buffer_mut());
    let lines = vec![
        Line::from(Span::styled(msg, Style::default().fg(styles::TEXT_BRIGHT))),
        Line::from(""),
        Line::from(vec![
            Span::styled("Files ", styles::style_dim()),
            Span::styled(
                format!("{}  ", disk.files),
                Style::default().fg(styles::ACCENT_CYAN),
            ),
            Span::styled("Dirs ", styles::style_dim()),
            Span::styled(
                format!("{}  ", disk.dirs),
                Style::default().fg(styles::ACCENT_CYAN),
            ),
            Span::styled("Walked ", styles::style_dim()),
            Span::styled(format_bytes(disk.bytes), styles::style_dim()),
        ]),
    ];
    Paragraph::new(lines)
        .wrap(ratatui::widgets::Wrap { trim: true })
        .style(Style::default().bg(styles::BG_DARK))
        .render(inner, f.buffer_mut());
}

fn render_log_strip(f: &mut Frame, disk: &DiskViewModel, area: Rect) {
    let spark_w = area.width.saturating_sub(4) as usize;
    let spark = sparkline_ascii(&disk.throughput_samples, spark_w.min(32));
    let spark_line = Line::from(vec![
        Span::styled("hash Δ ", styles::style_dim()),
        Span::styled(spark, Style::default().fg(styles::ACCENT_CYAN)),
    ]);
    let mut log_text: Vec<Line> = vec![spark_line, Line::from("")];
    for s in disk.log_lines.iter().rev().take(2).rev() {
        log_text.push(Line::from(Span::styled(
            styles::truncate_ellipsis(s, area.width as usize),
            styles::style_dim(),
        )));
    }
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(styles::BORDER_DIM));
    let inner = block.inner(area);
    block.render(area, f.buffer_mut());
    Paragraph::new(log_text)
        .style(Style::default().bg(styles::BG_DARK))
        .render(inner, f.buffer_mut());
}

fn render_progress_strip(f: &mut Frame, disk: &DiskViewModel, area: Rect) {
    enum GaugeMode {
        Determinate(f64),
        Indeterminate(&'static str),
        Hidden,
    }
    let gauge_mode = match disk.phase {
        ScanPhase::Hashing if disk.hash_total > 0 => {
            GaugeMode::Determinate(disk.hash_done as f64 / disk.hash_total as f64)
        }
        ScanPhase::Aggregating => GaugeMode::Indeterminate(
            "Aggregating folder sizes in SQLite (no % bar) — see line 1 for counts and pass status",
        ),
        ScanPhase::Walking => GaugeMode::Indeterminate(
            "No percent bar while scanning — line 1 shows file counts and walk rate when available",
        ),
        ScanPhase::Hashing => GaugeMode::Indeterminate(
            "Hashing duplicate candidates — line 1 shows status; hash bar appears when totals are known",
        ),
        ScanPhase::Ready | ScanPhase::Watching => GaugeMode::Determinate(1.0),
        _ => GaugeMode::Hidden,
    };

    let line1 = Line::from(vec![
        Span::styled(" Disk ", styles::style_bold_cyan()),
        Span::styled(format!("{}  ·  ", disk.phase.label()), styles::style_dim()),
        Span::styled(
            format!(
                "{} files  {} dirs  {}  ",
                disk.files,
                disk.dirs,
                format_bytes(disk.bytes)
            ),
            Style::default().fg(styles::TEXT_BRIGHT),
        ),
        Span::styled(&disk.status_msg, Style::default().fg(styles::ACCENT_AMBER)),
    ]);

    let focus_path = disk.selected_entry().map(|e| e.path.as_str()).unwrap_or("");
    let line2 = if disk.phase == ScanPhase::Hashing && disk.hash_total > 0 {
        Line::from(vec![Span::styled(
            format!(
                " Hash {} / {}  ·  dup groups ~{}  ·  reclaim {}",
                disk.hash_done,
                disk.hash_total,
                disk.dup_groups,
                format_bytes(disk.reclaim_bytes)
            ),
            styles::style_dim(),
        )])
    } else {
        let focus = if focus_path.is_empty() {
            String::new()
        } else {
            format!("focus {}  ·  ", styles::truncate_ellipsis(focus_path, 56))
        };
        Line::from(vec![Span::styled(
            format!(
                " {}{}  ·  marked {}  ·  {}",
                focus,
                disk.current_dir.display(),
                disk.marked.len(),
                if disk.reclaim_bytes > 0 {
                    format!("reclaimable {}", format_bytes(disk.reclaim_bytes))
                } else {
                    String::new()
                }
            ),
            styles::style_dim(),
        )])
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(styles::BORDER_DIM))
        .style(Style::default().bg(styles::BG_DARK));

    let inner = block.inner(area);
    block.render(area, f.buffer_mut());

    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    Paragraph::new(line1)
        .style(Style::default().bg(styles::BG_DARK))
        .render(inner_chunks[0], f.buffer_mut());

    match gauge_mode {
        GaugeMode::Determinate(ratio) => {
            let gauge = Gauge::default()
                .ratio(ratio.clamp(0.0, 1.0))
                .style(Style::default().fg(styles::ACCENT_CYAN))
                .block(Block::default());
            gauge.render(inner_chunks[1], f.buffer_mut());
        }
        GaugeMode::Indeterminate(hint) => {
            Paragraph::new(Line::from(vec![Span::styled(
                hint,
                Style::default().fg(styles::TEXT_DIM),
            )]))
            .style(Style::default().bg(styles::BG_DARK))
            .render(inner_chunks[1], f.buffer_mut());
        }
        GaugeMode::Hidden => {
            Paragraph::new(Line::from(""))
                .style(Style::default().bg(styles::BG_DARK))
                .render(inner_chunks[1], f.buffer_mut());
        }
    }

    Paragraph::new(line2)
        .style(Style::default().bg(styles::BG_DARK))
        .render(inner_chunks[2], f.buffer_mut());
}

fn render_other_drill_list(f: &mut Frame, disk: &mut DiskViewModel, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(styles::BORDER_DIM))
        .title(" Other (all items) — Enter: open · Esc/⌫: back ")
        .title_style(styles::style_bold_cyan())
        .style(Style::default().bg(styles::BG_DARK));

    let inner = block.inner(area);
    block.render(area, f.buffer_mut());

    let Some(ref indices) = disk.other_drill else {
        return;
    };
    if !indices.is_empty() && disk.other_table_state.selected().is_none() {
        disk.other_table_state.select(Some(0));
    }

    let header = Row::new(vec![
        Cell::from("Name"),
        Cell::from("Size"),
        Cell::from("Path"),
    ])
    .style(Style::default().fg(styles::TEXT_DIM).bg(styles::BG_HEADER));

    let mut rows = Vec::new();
    for (li, &ci) in indices.iter().enumerate() {
        let Some(row) = disk.children.get(ci) else {
            continue;
        };
        let mark = if disk.marked.contains(&row.path_buf()) {
            "● "
        } else {
            "  "
        };
        let name = format!("{mark}{}", styles::truncate_ellipsis(&row.name, 28));
        let path_disp =
            styles::truncate_ellipsis(&row.path, (inner.width.saturating_sub(50)) as usize);
        let cells = vec![
            Cell::from(name).style(Style::default().fg(styles::TEXT_BRIGHT)),
            Cell::from(format_bytes(row.size_bytes)),
            Cell::from(path_disp).style(styles::style_dim()),
        ];
        let mut bg = if li % 2 == 1 {
            styles::BG_ODD_ROW
        } else {
            styles::BG_DARK
        };
        if disk.marked.contains(&row.path_buf()) {
            bg = Color::Rgb(45, 40, 28);
        }
        rows.push(Row::new(cells).style(Style::default().bg(bg)));
    }

    let table = Table::new(
        rows,
        &[
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Fill(2),
        ],
    )
    .header(header)
    .row_highlight_style(
        Style::default()
            .fg(styles::TEXT_BRIGHT)
            .bg(Color::Rgb(18, 59, 87))
            .add_modifier(Modifier::BOLD),
    )
    .block(
        Block::default()
            .borders(Borders::NONE)
            .style(Style::default().bg(styles::BG_DARK)),
    );

    f.render_stateful_widget(table, inner, &mut disk.other_table_state);
}

fn render_list_table(f: &mut Frame, disk: &mut DiskViewModel, area: Rect) {
    let header = Row::new(vec![
        Cell::from("Name"),
        Cell::from("Size"),
        Cell::from("Flags"),
    ])
    .style(Style::default().fg(styles::TEXT_DIM).bg(styles::BG_HEADER));

    let mut rows = Vec::new();
    for (i, row) in disk.children.iter().enumerate() {
        let mark = if disk.marked.contains(&row.path_buf()) {
            "● "
        } else {
            "  "
        };
        let flags = if row.likely_delete {
            "DUPE?"
        } else if row.keep_winner {
            "KEEP"
        } else {
            ""
        };
        let name = format!("{mark}{}", styles::truncate_ellipsis(&row.name, 36));
        let cells = vec![
            Cell::from(name).style(Style::default().fg(styles::TEXT_BRIGHT)),
            Cell::from(format_bytes(row.size_bytes)),
            Cell::from(flags).style(Style::default().fg(styles::ACCENT_AMBER)),
        ];
        let mut bg = if i % 2 == 1 {
            styles::BG_ODD_ROW
        } else {
            styles::BG_DARK
        };
        if disk.marked.contains(&row.path_buf()) {
            bg = Color::Rgb(45, 40, 28);
        }
        rows.push(Row::new(cells).style(Style::default().bg(bg)));
    }

    let table = Table::new(
        rows,
        &[
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .row_highlight_style(
        Style::default()
            .fg(styles::TEXT_BRIGHT)
            .bg(Color::Rgb(18, 59, 87))
            .add_modifier(Modifier::BOLD),
    )
    .block(
        Block::default()
            .borders(Borders::NONE)
            .style(Style::default().bg(styles::BG_DARK)),
    );

    f.render_stateful_widget(table, area, &mut disk.list_state);
}

fn render_inspector(f: &mut Frame, disk: &DiskViewModel, area: Rect) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(styles::BORDER_DIM))
        .title(" Inspector ")
        .title_style(styles::style_bold_cyan())
        .style(Style::default().bg(styles::BG_DARK));

    let inner = block.inner(area);
    block.render(area, f.buffer_mut());

    let mut lines: Vec<Line> = if disk.other_drill.is_some() {
        if let Some(e) = disk.selected_entry() {
            inspector_entry_lines(e)
        } else {
            vec![Line::from(Span::styled(
                "Select an item",
                styles::style_dim(),
            ))]
        }
    } else if disk.layout_treemap {
        if let Some(tile) = disk.selected_tile() {
            if tile.child_indices.len() > 1 {
                vec![
                    Line::from(vec![Span::styled("Other bucket", styles::style_dim())]),
                    Line::from(Span::styled(
                        format!(
                            "{} items · {} total",
                            tile.child_indices.len(),
                            format_bytes(tile.aggregate_bytes)
                        ),
                        Style::default().fg(styles::TEXT_BRIGHT),
                    )),
                    Line::from(""),
                    Line::from(vec![Span::styled(
                        "Enter — list every item in this bucket",
                        styles::style_dim(),
                    )]),
                ]
            } else if let Some(e) = disk.selected_entry() {
                inspector_entry_lines(e)
            } else {
                vec![Line::from(Span::styled(
                    "Select an item",
                    styles::style_dim(),
                ))]
            }
        } else {
            vec![Line::from(Span::styled(
                "Select an item",
                styles::style_dim(),
            ))]
        }
    } else if let Some(e) = disk.selected_entry() {
        inspector_entry_lines(e)
    } else {
        vec![Line::from(Span::styled(
            "Select an item",
            styles::style_dim(),
        ))]
    };

    if matches!(disk.phase, ScanPhase::Watching | ScanPhase::Ready)
        && !disk.dup_quick_wins.is_empty()
    {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Dup quick wins ", styles::style_bold_cyan()),
            Span::styled("(d = review)", styles::style_dim()),
        ]));
        for row in disk.dup_quick_wins.iter().take(8) {
            let extra = row.member_count.saturating_sub(1);
            let reclaim = if row.reclaim_bytes > 0 {
                format_bytes(row.reclaim_bytes)
            } else {
                "—".into()
            };
            lines.push(Line::from(Span::styled(
                format!(
                    " {} · {} · {} (+{})",
                    reclaim,
                    row.member_count,
                    styles::truncate_ellipsis(&row.preview_name, 20),
                    extra
                ),
                Style::default().fg(styles::TEXT_DIM),
            )));
        }
    }

    Paragraph::new(lines)
        .style(Style::default().bg(styles::BG_DARK))
        .render(inner, f.buffer_mut());
}

fn path_lines(path: &str, max_chars: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut rest = path;
    while !rest.is_empty() {
        let chunk = if rest.chars().count() <= max_chars {
            let s = rest.to_string();
            rest = "";
            s
        } else {
            let take = rest
                .char_indices()
                .map(|(i, _)| i)
                .nth(max_chars)
                .unwrap_or(rest.len());
            let (a, b) = rest.split_at(take);
            rest = b;
            a.to_string()
        };
        out.push(Line::from(Span::styled(
            chunk,
            Style::default().fg(styles::TEXT_BRIGHT),
        )));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(
            "—",
            Style::default().fg(styles::TEXT_BRIGHT),
        )));
    }
    out
}

#[allow(mismatched_lifetime_syntaxes)]
fn inspector_entry_lines(e: &EntryRow) -> Vec<Line> {
    let mtime = e
        .mtime_ms
        .map(|ms| {
            let secs = ms / 1000;
            format!("mtime epoch_ms ~{secs}s")
        })
        .unwrap_or_else(|| "mtime —".into());
    let dup_id = e
        .dup_group_id
        .map(|id| format!("{id}"))
        .unwrap_or_else(|| "—".into());
    let mut lines = vec![Line::from(vec![Span::styled("Path", styles::style_dim())])];
    lines.extend(path_lines(&e.path, 52));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Size: ", styles::style_dim()),
        Span::styled(format_bytes(e.size_bytes), styles::style_header()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Bytes (exact): ", styles::style_dim()),
        Span::styled(
            format!("{}", e.size_bytes),
            Style::default().fg(styles::TEXT_BRIGHT),
        ),
    ]));
    lines.extend(vec![
        Line::from(vec![
            Span::styled("Mtime: ", styles::style_dim()),
            Span::raw(mtime),
        ]),
        Line::from(vec![
            Span::styled("Dir: ", styles::style_dim()),
            Span::raw(if e.is_dir { "yes" } else { "no" }),
        ]),
        Line::from(vec![
            Span::styled("Dup group: ", styles::style_dim()),
            Span::raw(dup_id),
        ]),
        Line::from(vec![
            Span::styled("Dup: ", styles::style_dim()),
            Span::styled(
                if e.likely_delete {
                    "likely delete"
                } else if e.keep_winner {
                    "keep"
                } else {
                    "—"
                },
                Style::default().fg(if e.likely_delete {
                    styles::ACCENT_RED
                } else {
                    styles::ACCENT_GREEN
                }),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "o — Reveal in Finder",
            styles::style_dim(),
        )]),
    ]);
    lines
}

fn render_dup_review_modal(f: &mut Frame, disk: &mut DiskViewModel, full: Rect) {
    let Some(st) = disk.dup_review.as_mut() else {
        return;
    };
    Clear.render(full, f.buffer_mut());

    let w = (full.width * 4 / 5).min(96).max(52);
    let h = full.height.saturating_sub(2).min(30).max(16);
    let x = full.x + (full.width.saturating_sub(w)) / 2;
    let y = full.y + (full.height.saturating_sub(h)) / 2;
    let outer = Rect::new(x, y, w, h);

    let title = match st.phase {
        DupReviewPhase::ListGroups => " Duplicate quick wins ",
        DupReviewPhase::PickKeeper => " Pick keeper (same hash) ",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(styles::ACCENT_CYAN))
        .title(title)
        .title_style(styles::style_bold_cyan())
        .style(Style::default().bg(styles::BG_HEADER));

    let inner = block.inner(outer);
    block.render(outer, f.buffer_mut());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(3)])
        .split(inner);

    match st.phase {
        DupReviewPhase::ListGroups => {
            let header = Row::new(vec![
                Cell::from("Reclaim"),
                Cell::from("#"),
                Cell::from("Preview"),
                Cell::from("id"),
            ])
            .style(Style::default().fg(styles::TEXT_DIM).bg(styles::BG_HEADER));
            let mut rows = Vec::new();
            for g in &st.group_rows {
                let reclaim = if g.reclaim_bytes > 0 {
                    format_bytes(g.reclaim_bytes)
                } else {
                    "—".into()
                };
                rows.push(Row::new(vec![
                    Cell::from(reclaim),
                    Cell::from(format!("{}", g.member_count)),
                    Cell::from(styles::truncate_ellipsis(&g.preview_name, 28)),
                    Cell::from(format!("{}", g.dup_group_id)).style(styles::style_dim()),
                ]));
            }
            let table = Table::new(
                rows,
                &[
                    Constraint::Length(10),
                    Constraint::Length(4),
                    Constraint::Fill(1),
                    Constraint::Length(6),
                ],
            )
            .header(header)
            .row_highlight_style(
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .bg(Color::Rgb(18, 59, 87))
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().style(Style::default().bg(styles::BG_DARK)));
            f.render_stateful_widget(table, chunks[0], &mut st.groups_table);
        }
        DupReviewPhase::PickKeeper => {
            let header = Row::new(vec![
                Cell::from("Flag"),
                Cell::from("Size"),
                Cell::from("Name"),
                Cell::from("Path"),
            ])
            .style(Style::default().fg(styles::TEXT_DIM).bg(styles::BG_HEADER));
            let mut rows = Vec::new();
            for m in &st.member_rows {
                let flag = if m.keep_winner {
                    "KEEP"
                } else if m.likely_delete {
                    "DUPE?"
                } else {
                    "—"
                };
                rows.push(Row::new(vec![
                    Cell::from(flag).style(Style::default().fg(if m.likely_delete {
                        styles::ACCENT_RED
                    } else {
                        styles::ACCENT_GREEN
                    })),
                    Cell::from(format_bytes(m.size_bytes)),
                    Cell::from(styles::truncate_ellipsis(&m.name, 18)),
                    Cell::from(styles::truncate_ellipsis(
                        &m.path,
                        (w.saturating_sub(36)) as usize,
                    ))
                    .style(styles::style_dim()),
                ]));
            }
            let table = Table::new(
                rows,
                &[
                    Constraint::Length(7),
                    Constraint::Length(10),
                    Constraint::Length(20),
                    Constraint::Fill(1),
                ],
            )
            .header(header)
            .row_highlight_style(
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .bg(Color::Rgb(18, 59, 87))
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().style(Style::default().bg(styles::BG_DARK)));
            f.render_stateful_widget(table, chunks[0], &mut st.members_table);
        }
    }

    let hint_lines = match st.phase {
        DupReviewPhase::ListGroups => vec![
            Line::from(vec![
                Span::styled("↑↓  ", styles::style_dim()),
                Span::styled("Enter", styles::style_bold_cyan()),
                Span::styled(" open  ·  ", styles::style_dim()),
                Span::styled("Esc", styles::style_bold_cyan()),
                Span::styled(" close", styles::style_dim()),
            ]),
            Line::from(Span::styled(
                "Sorted by reclaimable bytes (DUPE? sizes) then total size.",
                styles::style_dim(),
            )),
        ],
        DupReviewPhase::PickKeeper => vec![
            Line::from(vec![
                Span::styled("↑↓  ", styles::style_dim()),
                Span::styled("Enter", styles::style_bold_cyan()),
                Span::styled(" save keeper  ·  ", styles::style_dim()),
                Span::styled("Esc", styles::style_bold_cyan()),
                Span::styled(" back", styles::style_dim()),
            ]),
            Line::from(Span::styled(
                "Chosen path = KEEP; all other copies in this group → DUPE? (then t → Trash).",
                styles::style_dim(),
            )),
        ],
    };
    Paragraph::new(hint_lines)
        .style(Style::default().bg(styles::BG_HEADER))
        .render(chunks[1], f.buffer_mut());
}

fn render_confirm_modal(f: &mut Frame, disk: &mut DiskViewModel, full: Rect) {
    let w = (full.width * 3 / 4).min(76).max(40);
    let h = (full.height * 2 / 5)
        .max(12)
        .min(full.height.saturating_sub(2));
    let x = full.x + (full.width.saturating_sub(w)) / 2;
    let y = full.y + (full.height.saturating_sub(h)) / 2;
    let r = Rect::new(x, y, w, h);
    Clear.render(r, f.buffer_mut());

    let total: u64 = disk
        .marked
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
        .sum();

    let mut paths: Vec<_> = disk.marked.iter().cloned().collect();
    paths.sort();

    const PAGE: usize = 6;
    let visible = PAGE;
    let max_scroll = paths.len().saturating_sub(visible.max(1));
    let scroll = disk.confirm_scroll.min(max_scroll);
    let slice: Vec<String> = paths
        .into_iter()
        .skip(scroll)
        .take(visible.max(1))
        .map(|p| {
            let s = p.to_string_lossy().into_owned();
            styles::truncate_ellipsis(&s, (w.saturating_sub(4)) as usize)
        })
        .collect();

    let mut text = vec![
        Line::from(vec![Span::styled(
            " Move to Trash? ",
            Style::default()
                .fg(styles::ACCENT_AMBER)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "{} items  ·  {}  ·  paths {}–{}",
                disk.marked.len(),
                format_bytes(total),
                scroll + 1,
                scroll + slice.len()
            ),
            styles::style_dim(),
        )),
        Line::from(""),
    ];
    for line in slice {
        text.push(Line::from(Span::styled(
            line,
            Style::default().fg(styles::TEXT_BRIGHT),
        )));
    }
    text.push(Line::from(""));
    text.push(Line::from(Span::styled(
        "↑↓ scroll  ·  Enter confirm  ·  Esc cancel",
        styles::style_dim(),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(styles::ACCENT_CYAN))
        .style(Style::default().bg(styles::BG_HEADER));

    let inner = block.inner(r);
    block.render(r, f.buffer_mut());
    Paragraph::new(text).render(inner, f.buffer_mut());
}
