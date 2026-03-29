#![forbid(unsafe_code)]
#![allow(unused_imports, unused_mut, dead_code)]
#![allow(clippy::all)]
/// MacJet — Main Entry Point
///
/// Sets up the ratatui terminal, tokio runtime, and the 250ms event loop.
/// Renders: Header | Tab Bar | [Filter Bar] | Content + Inspector | Footer
/// with notification toast overlays.
use std::io;
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseButton,
        MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Terminal,
};

use macjet::app::{AppState, View};
use macjet::disk::ui::render_disk_view;
use macjet::ui::{
    detail_panel::DetailPanelWidget, filter_bar::FilterBarWidget, footer::Footer, header::Header,
    help_panel::HelpWidget, network_panel::NetworkPanelWidget, notifications::NotificationOverlay,
    predict_panel::PredictPanelWidget, process_tree::ProcessTreeWidget,
    reclaim_panel::ReclaimPanelWidget, styles,
};

#[derive(Parser, Debug)]
#[command(
    name = "macjet",
    version,
    about = "MacJet — macOS process monitor (terminal UI)"
)]
struct Cli {
    /// Run the MCP JSON-RPC server and exit (no TUI)
    #[arg(long)]
    mcp: bool,
    /// Disable online CPU prediction (RLS): no sampling, no training. Predict tab shows disabled.
    #[arg(long = "no-ml", visible_alias = "noML")]
    no_ml: bool,
    /// Seconds between data collection ticks (default 1). Larger values reduce CPU wakeups.
    #[arg(long = "refresh", value_name = "SECS", default_value_t = 1)]
    refresh_secs: u64,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    if cli.refresh_secs < 1 {
        eprintln!("error: --refresh must be at least 1 second");
        std::process::exit(2);
    }

    if cli.mcp {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            macjet::mcp::server::run_mcp_server(cli.refresh_secs, !cli.no_ml).await;
        });
        return Ok(());
    }

    // Setup terminal
    enable_raw_mode().map_err(|e| {
        eprintln!("MacJet needs an interactive terminal (TTY). Raw mode failed: {e}");
        eprintln!(
            "Use Terminal.app, iTerm, Alacritty, etc. — not a pipe or non-interactive task output."
        );
        e
    })?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the app
    let result = run_app(
        &mut terminal,
        !cli.no_ml,
        Duration::from_secs(cli.refresh_secs),
    );

    // Restore terminal
    disable_raw_mode()?;
    let mut backend = terminal.backend_mut();
    let _ = execute!(backend, DisableMouseCapture);
    execute!(backend, LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn sync_disk_mouse_capture(app: &mut AppState) {
    let want = app.active_view == View::Disk;
    if want == app.disk_mouse_capture {
        return;
    }
    let mut out = io::stdout();
    if want {
        let _ = execute!(out, EnableMouseCapture);
    } else {
        let _ = execute!(out, DisableMouseCapture);
    }
    app.disk_mouse_capture = want;
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ml_enabled: bool,
    tick_interval: Duration,
) -> io::Result<()> {
    let mut app = AppState::new(ml_enabled);

    // Initial tick to populate data
    app.tick();
    app.refresh_selection_context();

    let mut last_tick = Instant::now();

    loop {
        // ─── Draw ──────────────────────────────────
        terminal.draw(|f| {
            let size = f.area();

            if app.active_view == View::Disk {
                app.disk.ensure_started();
                app.disk.poll_events();
            }

            // Compute filter bar height
            let filter_height = if app.filter_visible { 1 } else { 0 };

            // Outer vertical layout: Header | Tab Bar | [Filter] | Body | Footer
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints(&[
                    Constraint::Length(2),             // Header
                    Constraint::Length(1),             // Tab bar
                    Constraint::Length(filter_height), // Filter bar (0 or 1)
                    Constraint::Min(5),                // Body
                    Constraint::Length(1),             // Footer
                ])
                .split(size);

            // ─── Header ─────────────────────────
            f.render_widget(Header { app: &app }, outer[0]);

            // ─── Tab Bar ────────────────────────
            render_tab_bar(f, &app, outer[1]);

            // ─── Filter Bar ─────────────────────
            if app.filter_visible {
                f.render_widget(
                    FilterBarWidget {
                        value: &app.filter_input,
                    },
                    outer[2],
                );
            }

            // ─── Body (Content + Inspector) ─────
            let body_area = outer[3];

            match app.active_view {
                View::Processes => {
                    render_process_view(f, &mut app, body_area);
                }
                View::Energy => {
                    render_energy_view(f, &mut app, body_area);
                }
                View::Reclaim => {
                    render_reclaim_view(f, &mut app, body_area);
                }
                View::Network => {
                    let network_widget = NetworkPanelWidget::new(&app.network_collector.latest);
                    f.render_widget(network_widget, body_area);
                }
                View::Predict => {
                    let stats = app.cpu_predictor.stats();
                    let predict_widget =
                        PredictPanelWidget::new(&stats, app.system.cpu_percent, app.ml_enabled);
                    f.render_widget(predict_widget, body_area);
                }
                View::Disk => {
                    render_disk_view(f, &mut app.disk, body_area);
                }
                View::Help => {
                    f.render_widget(HelpWidget, body_area);
                }
            }

            // ─── Footer ─────────────────────────
            f.render_widget(
                Footer {
                    paused: app.paused,
                    ml_enabled: app.ml_enabled,
                    active_view: app.active_view,
                },
                outer[4],
            );

            // ─── Notification Overlay ────────────
            if let Some(notification) = app.notifications.current() {
                f.render_widget(NotificationOverlay { notification }, size);
            }
        })?;

        // ─── Event Handling ────────────────────────
        // Poll at 250ms for smoother UI, tick at 1s intervals
        if event::poll(Duration::from_millis(250))? {
            let ev = event::read()?;
            if let Event::Mouse(me) = ev {
                if app.active_view == View::Disk {
                    match me.kind {
                        MouseEventKind::ScrollDown => {
                            if app.disk.other_drill.is_some() {
                                app.disk.other_list_nav_down(3);
                            } else {
                                app.disk.list_nav_down(3);
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if app.disk.other_drill.is_some() {
                                app.disk.other_list_nav_up(3);
                            } else {
                                app.disk.list_nav_up(3);
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            if app.disk.layout_treemap {
                                for (i, t) in app.disk.tiles.iter().enumerate() {
                                    let r = t.rect;
                                    if me.column >= r.x
                                        && me.column < r.x.saturating_add(r.width)
                                        && me.row >= r.y
                                        && me.row < r.y.saturating_add(r.height)
                                    {
                                        app.disk.tree_state.focus = i;
                                        break;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                app.refresh_selection_context();
                continue;
            }
            if let Event::Key(key) = ev {
                let now_secs = || {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs_f64()
                };

                // ─── Filter Mode Input ─────────────
                if app.filter_visible {
                    match key.code {
                        KeyCode::Esc => {
                            app.clear_filter();
                        }
                        KeyCode::Enter => {
                            // Accept filter, hide input bar but keep filter active
                            app.filter_visible = false;
                        }
                        KeyCode::Backspace => {
                            app.filter_input.pop();
                        }
                        KeyCode::Char(c) => {
                            app.filter_input.push(c);
                        }
                        _ => {}
                    }
                    app.refresh_selection_context();
                    continue;
                }

                // ─── Normal Mode Input ─────────────
                if app.active_view == View::Disk && app.disk.confirm_delete {
                    const CONFIRM_PAGE: usize = 6;
                    let max_scroll = app.disk.marked.len().saturating_sub(CONFIRM_PAGE);
                    match key.code {
                        KeyCode::Esc => app.disk.confirm_delete = false,
                        KeyCode::Enter => match app.disk.trash_marked() {
                            Ok(n) => app
                                .notifications
                                .push(format!("Moved {n} item(s) to Trash")),
                            Err(e) => app.notifications.push(e),
                        },
                        KeyCode::Up => {
                            app.disk.confirm_scroll = app.disk.confirm_scroll.saturating_sub(1);
                        }
                        KeyCode::Down => {
                            app.disk.confirm_scroll = (app.disk.confirm_scroll + 1).min(max_scroll);
                        }
                        _ => {}
                    }
                    app.refresh_selection_context();
                    continue;
                }

                if app.active_view == View::Disk && app.disk.dup_review.is_some() {
                    match key.code {
                        KeyCode::Esc | KeyCode::Backspace => app.disk.dup_review_escape(),
                        KeyCode::Enter => {
                            if let Err(e) = app.disk.dup_review_on_enter() {
                                app.notifications.push(e);
                            }
                        }
                        KeyCode::Up => app.disk.dup_review_nav_up(1),
                        KeyCode::Down => app.disk.dup_review_nav_down(1),
                        KeyCode::PageUp => app.disk.dup_review_nav_up(10),
                        KeyCode::PageDown => app.disk.dup_review_nav_down(10),
                        KeyCode::Home => app.disk.dup_review_home(),
                        KeyCode::End => app.disk.dup_review_end(),
                        _ => {}
                    }
                    app.refresh_selection_context();
                    continue;
                }

                if app.active_view == View::Disk && app.disk.search_bar_active {
                    match key.code {
                        KeyCode::Esc => {
                            app.disk.search_bar_active = false;
                        }
                        KeyCode::Enter => {
                            app.disk.search_bar_active = false;
                        }
                        KeyCode::Backspace => {
                            app.disk.search_buffer.pop();
                            app.disk.set_disk_search(app.disk.search_buffer.clone());
                        }
                        KeyCode::Char(c) => {
                            app.disk.search_buffer.push(c);
                            app.disk.set_disk_search(app.disk.search_buffer.clone());
                        }
                        _ => {}
                    }
                    app.refresh_selection_context();
                    continue;
                }

                match key.code {
                    // Quit
                    KeyCode::Char('q') => {
                        app.telemetry.flush();
                        app.should_quit = true;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.telemetry.flush();
                        app.should_quit = true;
                    }

                    // Pause/Resume (Disk: mark for trash)
                    KeyCode::Char(' ') => {
                        if app.active_view == View::Disk {
                            app.disk.toggle_mark();
                        } else {
                            app.paused = !app.paused;
                            let label = if app.paused { "Paused" } else { "Resumed" };
                            app.notifications.push(label);
                        }
                    }

                    // View switching
                    KeyCode::Char('1') => app.active_view = View::Processes,
                    KeyCode::Char('2') => app.active_view = View::Reclaim,
                    KeyCode::Char('3') => app.active_view = View::Energy,
                    KeyCode::Char('4') => app.active_view = View::Network,
                    KeyCode::Char('5') => app.active_view = View::Predict,
                    KeyCode::Char('6') => app.active_view = View::Disk,
                    KeyCode::Char('?') => {
                        app.active_view = if app.active_view == View::Help {
                            View::Processes
                        } else {
                            View::Help
                        };
                    }
                    KeyCode::Tab => {
                        app.active_view = app.active_view.next();
                    }
                    KeyCode::Right => {
                        if app.active_view == View::Disk && app.disk.other_drill.is_some() {
                            // keep focus on Disk while browsing the Other list
                        } else if app.active_view == View::Disk && app.disk.layout_treemap {
                            app.disk.treemap_nav(2);
                        } else {
                            app.active_view = app.active_view.next();
                        }
                    }
                    KeyCode::Left => {
                        if app.active_view == View::Disk && app.disk.other_drill.is_some() {
                            app.disk.close_other_drill();
                        } else if app.active_view == View::Disk && app.disk.layout_treemap {
                            app.disk.treemap_nav(0);
                        } else {
                            app.active_view = app.active_view.prev();
                        }
                    }

                    // Filter
                    KeyCode::Char('/') => {
                        if app.active_view == View::Disk {
                            app.disk.search_bar_active = true;
                            app.disk.search_buffer.clear();
                            app.disk.set_disk_search(String::new());
                        } else {
                            app.filter_visible = true;
                            app.filter_input.clear();
                        }
                    }
                    KeyCode::Esc => {
                        if app.active_view == View::Disk && app.disk.other_drill.is_some() {
                            app.disk.close_other_drill();
                        } else {
                            app.clear_filter();
                        }
                    }

                    KeyCode::Backspace => {
                        if app.active_view == View::Disk {
                            app.disk.drill_up();
                        }
                    }

                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        if app.active_view == View::Disk {
                            app.disk.request_rescan();
                            app.notifications.push("Disk rescan started".to_string());
                        }
                    }

                    KeyCode::Char('d') => {
                        if app.active_view == View::Disk {
                            app.disk.open_dup_review();
                        }
                    }

                    KeyCode::Char('t') => {
                        if app.active_view == View::Disk && !app.disk.marked.is_empty() {
                            app.disk.confirm_scroll = 0;
                            app.disk.confirm_delete = true;
                        }
                    }

                    KeyCode::Char('o') => {
                        if app.active_view == View::Disk {
                            match app.disk.reveal_selected_in_finder() {
                                Ok(()) => app.notifications.push("Revealed in Finder"),
                                Err(e) => app.notifications.push(e),
                            }
                        }
                    }

                    KeyCode::Char('u') => {
                        if app.active_view == View::Disk && !app.disk.last_trashed.is_empty() {
                            app.notifications.push(format!(
                                "Last batch: {} item(s) — restore from Trash if needed",
                                app.disk.last_trashed.len()
                            ));
                        }
                    }

                    // Sort cycling
                    KeyCode::Char('s') => {
                        let mode = app.process_collector.cycle_sort();
                        app.notifications.push(format!("Sort: {:?}", mode));
                    }

                    // Navigation (Up/Down/PgUp/PgDn/Home/End)
                    KeyCode::Up => {
                        app.interaction_pause_until = now_secs() + 3.0;
                        if app.active_view == View::Disk {
                            if app.disk.other_drill.is_some() {
                                app.disk.other_list_nav_up(1);
                            } else if app.disk.layout_treemap {
                                app.disk.treemap_nav(1);
                            } else {
                                app.disk.list_nav_up(1);
                            }
                        } else {
                            handle_nav_up(&mut app, 1);
                        }
                    }
                    KeyCode::Down => {
                        app.interaction_pause_until = now_secs() + 3.0;
                        if app.active_view == View::Disk {
                            if app.disk.other_drill.is_some() {
                                app.disk.other_list_nav_down(1);
                            } else if app.disk.layout_treemap {
                                app.disk.treemap_nav(3);
                            } else {
                                app.disk.list_nav_down(1);
                            }
                        } else {
                            handle_nav_down(&mut app, 1);
                        }
                    }
                    KeyCode::PageUp => {
                        app.interaction_pause_until = now_secs() + 3.0;
                        if app.active_view == View::Disk {
                            if app.disk.other_drill.is_some() {
                                app.disk.other_list_nav_up(10);
                            } else {
                                app.disk.list_nav_up(10);
                            }
                        } else {
                            handle_nav_up(&mut app, 10);
                        }
                    }
                    KeyCode::PageDown => {
                        app.interaction_pause_until = now_secs() + 3.0;
                        if app.active_view == View::Disk {
                            if app.disk.other_drill.is_some() {
                                app.disk.other_list_nav_down(10);
                            } else {
                                app.disk.list_nav_down(10);
                            }
                        } else {
                            handle_nav_down(&mut app, 10);
                        }
                    }
                    KeyCode::Home => {
                        app.interaction_pause_until = now_secs() + 3.0;
                        if app.active_view == View::Disk {
                            if let Some(ref ix) = app.disk.other_drill {
                                if !ix.is_empty() {
                                    app.disk.other_table_state.select(Some(0));
                                }
                            } else {
                                app.disk.list_state.select(Some(0));
                                app.disk.tree_state.focus = 0;
                            }
                        } else if let Some(tree) = app.active_tree_mut() {
                            tree.home();
                        }
                    }
                    KeyCode::End => {
                        app.interaction_pause_until = now_secs() + 3.0;
                        if app.active_view == View::Disk {
                            if let Some(ref ix) = app.disk.other_drill {
                                let max = ix.len();
                                if max > 0 {
                                    app.disk.other_table_state.select(Some(max - 1));
                                }
                            } else {
                                let max = app.disk.children.len();
                                if max > 0 {
                                    app.disk.list_state.select(Some(max - 1));
                                    if !app.disk.tiles.is_empty() {
                                        app.disk.tree_state.focus = app.disk.tiles.len() - 1;
                                    }
                                }
                            }
                        } else {
                            match app.active_view {
                                View::Processes | View::Energy => {
                                    let max = match app.active_view {
                                        View::Processes => app.processes_tree.row_keys.len(),
                                        View::Energy => app.energy_tree.row_keys.len(),
                                        _ => 0,
                                    };
                                    if let Some(tree) = app.active_tree_mut() {
                                        tree.end(max);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    // Expand/Collapse (Disk: drill into folder)
                    KeyCode::Enter => {
                        app.interaction_pause_until = now_secs() + 3.0;
                        if app.active_view == View::Disk {
                            app.disk.drill_into();
                        } else {
                            match app.active_view {
                                View::Processes => {
                                    app.processes_tree.toggle_selected();
                                }
                                View::Energy => {
                                    app.energy_tree.toggle_selected();
                                }
                                _ => {}
                            }
                        }
                    }

                    _ => {}
                }

                // Update selection context after every key event
                app.refresh_selection_context();
                sync_disk_mouse_capture(&mut app);
            }
        }

        if app.should_quit {
            return Ok(());
        }

        // ─── Tick (configurable refresh interval) ────────────────────
        if last_tick.elapsed() >= tick_interval {
            app.tick();
            app.notifications.prune();
            app.refresh_selection_context();
            last_tick = Instant::now();
        }
    }
}

// ─── Rendering Helpers ─────────────────────────────────

fn render_tab_bar(f: &mut ratatui::Frame, app: &AppState, area: Rect) {
    let tab_spans: Vec<Span> = View::all()
        .iter()
        .flat_map(|v| {
            let is_active = *v == app.active_view;
            vec![
                Span::styled(
                    format!(" {} ", v.shortcut()),
                    if is_active {
                        styles::style_bold_cyan()
                    } else {
                        styles::style_dim()
                    },
                ),
                Span::styled(
                    format!("{} ", v.label()),
                    if is_active {
                        Style::default()
                            .fg(styles::TEXT_BRIGHT)
                            .add_modifier(ratatui::style::Modifier::BOLD)
                    } else {
                        styles::style_dim()
                    },
                ),
                Span::styled(" │ ", styles::style_dim()),
            ]
        })
        .collect();

    let tab_line = Line::from(tab_spans);
    f.render_widget(
        Paragraph::new(tab_line).style(Style::default().bg(styles::BG_HEADER)),
        area,
    );
}

fn render_process_view(f: &mut ratatui::Frame, app: &mut AppState, body_area: Rect) {
    // Split body into content (left) + inspector (right)
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(3), Constraint::Fill(2)])
        .split(body_area);

    // Left: Process tree
    let groups = app.process_collector.groups();

    // Apply filter
    let filtered: Vec<_> = if app.filter_input.is_empty() {
        groups.to_vec()
    } else {
        let f_lower = app.filter_input.to_lowercase();
        groups
            .iter()
            .filter(|g| {
                g.name.to_lowercase().contains(&f_lower)
                    || g.processes
                        .iter()
                        .any(|p| p.name.to_lowercase().contains(&f_lower))
            })
            .cloned()
            .collect()
    };

    let row_data = app.processes_tree.build_rows(&filtered);
    let tree_widget = ProcessTreeWidget::new(
        &app.metrics_history,
        &app.energy_collector,
        &row_data,
        &mut app.processes_tree.table_state,
        app.interaction_pause_until,
    );
    f.render_widget(tree_widget, body[0]);

    // Right: Inspector panel
    let detail_widget = DetailPanelWidget::new(
        app.selected_process.as_ref(),
        app.selected_group.as_ref(),
        &app.metrics_history,
    );
    f.render_widget(detail_widget, body[1]);
}

fn render_energy_view(f: &mut ratatui::Frame, app: &mut AppState, body_area: Rect) {
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(3), Constraint::Fill(2)])
        .split(body_area);

    // Sort by energy impact for this view
    let mut groups = app.process_collector.groups().to_vec();
    groups.sort_by(|a, b| {
        let a_impact = match a.energy_impact.as_str() {
            "HIGH" => 3,
            "MED" => 2,
            "LOW" => 1,
            _ => 0,
        };
        let b_impact = match b.energy_impact.as_str() {
            "HIGH" => 3,
            "MED" => 2,
            "LOW" => 1,
            _ => 0,
        };
        b_impact.cmp(&a_impact).then_with(|| {
            b.total_cpu
                .partial_cmp(&a.total_cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });

    let row_data = app.energy_tree.build_rows(&groups);
    let tree_widget = ProcessTreeWidget::new(
        &app.metrics_history,
        &app.energy_collector,
        &row_data,
        &mut app.energy_tree.table_state,
        app.interaction_pause_until,
    );
    f.render_widget(tree_widget, body[0]);

    // Right: Inspector
    let detail_widget = DetailPanelWidget::new(
        app.selected_process.as_ref(),
        app.selected_group.as_ref(),
        &app.metrics_history,
    );
    f.render_widget(detail_widget, body[1]);
}

fn render_reclaim_view(f: &mut ratatui::Frame, app: &mut AppState, body_area: Rect) {
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(3), Constraint::Fill(2)])
        .split(body_area);

    let candidates = app
        .metrics_history
        .get_reclaim_candidates(app.process_collector.groups());
    let reclaim_widget = ReclaimPanelWidget::new(&candidates, &mut app.reclaim_state);
    f.render_widget(reclaim_widget, body[0]);

    // Right: Inspector — show reclaim candidate details
    let detail_widget = DetailPanelWidget::from_reclaim(
        app.selected_reclaim_candidate.as_ref(),
        app.selected_reclaim_group.as_ref(),
        &app.metrics_history,
    );
    f.render_widget(detail_widget, body[1]);
}

// ─── Navigation Helpers ────────────────────────────────

fn handle_nav_up(app: &mut AppState, lines: usize) {
    match app.active_view {
        View::Processes => app.processes_tree.move_up(lines),
        View::Energy => app.energy_tree.move_up(lines),
        View::Reclaim => {
            let i = match app.reclaim_state.selected() {
                Some(i) => i.saturating_sub(lines),
                None => 0,
            };
            app.reclaim_state.select(Some(i));
        }
        _ => {}
    }
}

fn handle_nav_down(app: &mut AppState, lines: usize) {
    match app.active_view {
        View::Processes => {
            let max = app.processes_tree.row_keys.len();
            app.processes_tree.move_down(lines, max);
        }
        View::Energy => {
            let max = app.energy_tree.row_keys.len();
            app.energy_tree.move_down(lines, max);
        }
        View::Reclaim => {
            let max = app
                .metrics_history
                .get_reclaim_candidates(app.process_collector.groups())
                .len();
            let i = match app.reclaim_state.selected() {
                Some(i) => {
                    if i + lines >= max {
                        max.saturating_sub(1)
                    } else {
                        i + lines
                    }
                }
                None => 0,
            };
            app.reclaim_state.select(Some(i));
        }
        _ => {}
    }
}
