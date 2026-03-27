/// MacJet — Drill-Down Screens
///
/// Full-screen overlays for tailing standard output of system utilities
/// like `sample`, `fs_usage`, `nettop`, and `sc_usage`.
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};
use std::collections::VecDeque;
use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

pub struct DrillScreenState {
    pub title: String,
    pub lines: VecDeque<String>,
    pub status: String,
    child_process: Option<Child>,
    stdout_task: Option<JoinHandle<()>>,
    stderr_task: Option<JoinHandle<()>>,
    pub is_active: bool,
}

impl DrillScreenState {
    pub fn new(title: String) -> Self {
        Self {
            title,
            lines: VecDeque::with_capacity(50),
            status: "Starting...".to_string(),
            child_process: None,
            stdout_task: None,
            stderr_task: None,
            is_active: false,
        }
    }

    pub fn start_command(&mut self, cmd: &str, args: &[&str], require_sudo: bool) {
        self.is_active = true;
        self.status = "Running command...".to_string();
        self.lines.clear();

        let mut command = if require_sudo {
            let mut c = Command::new("sudo");
            c.arg(cmd);
            for a in args {
                c.arg(a);
            }
            c
        } else {
            let mut c = Command::new(cmd);
            for a in args {
                c.arg(a);
            }
            c
        };

        command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        if let Ok(mut child) = command.spawn() {
            let _stdout = child.stdout.take().unwrap();
            let _stderr = child.stderr.take().unwrap();

            self.child_process = Some(child);

            // Channel wouldn't easily be integrated without an injected sender,
            // so we will just let Tokio update an Arc<Mutex> in a real app,
            // but for this UI struct, we can't easily push to `self` from Tokio directly.
            // In ratatui loops, the App usually holds a receiver.
            // For now, in Phase 5 skeleton, we simulate or store the fact the tasks run.
            // The AppState will poll a ringbuffer.
            self.lines
                .push_back(format!("$ {} {}", cmd, args.join(" ")));
            self.status = "Streaming (stub until cross-thread bus constructed)".to_string();
        } else {
            self.status = "Failed to start command".to_string();
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child_process.take() {
            let _ = child.start_kill();
        }
        if let Some(task) = self.stdout_task.take() {
            task.abort();
        }
        if let Some(task) = self.stderr_task.take() {
            task.abort();
        }
        self.is_active = false;
    }
}

pub struct DrillScreenWidget<'a> {
    pub state: &'a DrillScreenState,
}

impl<'a> Widget for DrillScreenWidget<'a> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        // Full screen clear overlay
        Clear.render(area, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(48, 54, 61)))
            .style(Style::default().bg(Color::Rgb(13, 17, 23)));

        let inner = block.inner(area);
        block.render(area, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(5),
                Constraint::Length(1),
            ])
            .split(inner);

        // Title
        let title_line = Line::from(vec![
            Span::styled("  ", Style::default().bg(Color::Rgb(22, 27, 34))),
            Span::styled(
                format!("{}  (ESC to close) ", self.state.title),
                Style::default()
                    .fg(Color::Rgb(88, 166, 255))
                    .bg(Color::Rgb(22, 27, 34))
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        Paragraph::new(title_line)
            .style(Style::default().bg(Color::Rgb(22, 27, 34)))
            .render(chunks[0], buf);

        // Output lines
        let output_text: Vec<Line> = self
            .state
            .lines
            .iter()
            .map(|s| {
                Line::from(Span::styled(
                    s,
                    Style::default().fg(Color::Rgb(230, 237, 243)),
                ))
            })
            .collect();
        let p = Paragraph::new(output_text);
        p.render(chunks[1], buf);

        // Status
        let status_line = Line::from(vec![Span::styled(
            format!("  {}  ", self.state.status),
            Style::default()
                .fg(Color::Rgb(139, 148, 158))
                .bg(Color::Rgb(22, 27, 34)),
        )]);
        Paragraph::new(status_line)
            .style(Style::default().bg(Color::Rgb(22, 27, 34)))
            .render(chunks[2], buf);
    }
}
