//! Disk space view: background scan, SQLite index, treemap, dedupe, watch, trash.

mod hash;
mod model;
mod names;
mod scan;
pub mod store;
mod treemap;
pub mod ui;
mod watch;

pub use model::{DiskUiEvent, EntryFlags, EntryRow, FileMeta, NormalizedName, ScanPhase};
pub use treemap::{TreeMapState, TreeTile, DEFAULT_MAX_TREEMAP_TILES};

/// Phase of the duplicate “quick wins” full-screen reviewer (`d`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DupReviewPhase {
    ListGroups,
    PickKeeper,
}

/// Stateful UI for reviewing duplicate groups and picking a keeper file.
#[derive(Debug)]
pub struct DupReviewState {
    pub phase: DupReviewPhase,
    pub group_rows: Vec<store::DupGroupQuickRow>,
    pub groups_table: TableState,
    pub member_rows: Vec<store::DupGroupMember>,
    pub members_table: TableState,
    pub pick_group_id: i64,
}

use crossbeam_channel::{bounded, Sender};
use ratatui::layout::Rect;
use ratatui::widgets::TableState;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Default SQLite path for the disk index (same as TUI and MCP readers).
pub fn default_disk_index_path() -> PathBuf {
    directories::ProjectDirs::from("com", "hot", "MacJet")
        .map(|p| p.cache_dir().join("disk_index.sqlite"))
        .unwrap_or_else(|| std::env::temp_dir().join("macjet_disk_index.sqlite"))
}

/// Canonical filesystem path for the index root so walker `parent_path` strings match `current_dir`
/// (e.g. macOS `/private/Users/...` vs `/Users/...`).
pub fn resolve_disk_index_root(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

const DISK_LOG_MAX: usize = 80;

/// Coordinates DB writer thread, parallel walk, hashing, and optional watch.
pub struct DiskViewModel {
    pub root: PathBuf,
    pub db_path: PathBuf,
    pub read_conn: Option<rusqlite::Connection>,
    ui_rx: crossbeam_channel::Receiver<DiskUiEvent>,
    ui_tx: Sender<DiskUiEvent>,
    cancel: Arc<AtomicBool>,
    watch_cancel: Arc<AtomicBool>,

    pub phase: ScanPhase,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub status_msg: String,
    pub children: Vec<EntryRow>,
    pub current_dir: PathBuf,
    pub list_state: ratatui::widgets::TableState,
    pub tree_state: TreeMapState,
    pub tiles: Vec<TreeTile>,
    /// Updated each frame by `ui::render_disk_view`.
    pub layout_treemap: bool,
    pub marked: HashSet<PathBuf>,
    pub confirm_delete: bool,
    pub last_trashed: Vec<String>,
    pub hash_done: u64,
    pub hash_total: u64,
    pub dup_groups: u64,
    pub reclaim_bytes: u64,

    pipeline_running: bool,
    started_once: bool,

    pub log_lines: VecDeque<String>,
    /// Fingerprint of `children` last used for treemap layout (skip re-layout if unchanged).
    pub layout_children_sig: u64,
    pub indexed_bytes_total: u64,
    pub disk_search_filter: String,
    pub confirm_scroll: usize,

    /// Skip treemap re-layout when (w,h,children_sig,marked_sig) unchanged.
    pub last_treemap_key: Option<(u16, u16, u64, u64)>,
    /// Last treemap panel rect (terminal coords) for mouse hit-testing.
    pub last_treemap_screen_rect: Option<Rect>,
    /// Rolling hash completion rate (hashes / second) for sparkline.
    pub throughput_samples: VecDeque<f64>,
    last_hash_sample_at: Option<Instant>,
    last_hash_done_sample: u64,
    last_periodic_reconcile: Option<Instant>,
    /// Throttle `total_file_bytes` + `refresh_children` while the scan pipeline runs.
    last_partial_db_refresh: Option<Instant>,
    /// Set each frame from terminal color capability (Disk treemap).
    pub reduced_color: bool,
    pub search_bar_active: bool,
    pub search_buffer: String,

    /// When set, main pane shows a flat list of `children` indices (sorted by size) inside the focused `Other` tile.
    pub other_drill: Option<Vec<usize>>,
    pub other_table_state: TableState,

    /// Inspector: top duplicate groups by reclaimable bytes (refreshed from index).
    pub dup_quick_wins: Vec<store::DupGroupQuickRow>,
    dup_quick_wins_fp: Option<(u64, u64)>,
    /// Full-screen duplicate reviewer (`d`).
    pub dup_review: Option<DupReviewState>,
}

impl DiskViewModel {
    pub fn new() -> Self {
        let raw_root = dirs::download_dir().unwrap_or_else(|| std::env::temp_dir());
        let root = resolve_disk_index_root(raw_root);
        let db_path = default_disk_index_path();

        let (ui_tx, ui_rx) = bounded::<DiskUiEvent>(256);

        Self {
            root,
            db_path,
            read_conn: None,
            ui_rx,
            ui_tx,
            cancel: Arc::new(AtomicBool::new(false)),
            watch_cancel: Arc::new(AtomicBool::new(false)),
            phase: ScanPhase::Idle,
            files: 0,
            dirs: 0,
            bytes: 0,
            status_msg: String::new(),
            children: Vec::new(),
            current_dir: PathBuf::new(),
            list_state: ratatui::widgets::TableState::default(),
            tree_state: TreeMapState::default(),
            tiles: Vec::new(),
            layout_treemap: true,
            marked: HashSet::new(),
            confirm_delete: false,
            last_trashed: Vec::new(),
            hash_done: 0,
            hash_total: 0,
            dup_groups: 0,
            reclaim_bytes: 0,
            pipeline_running: false,
            started_once: false,

            log_lines: VecDeque::with_capacity(DISK_LOG_MAX),
            layout_children_sig: 0,
            indexed_bytes_total: 0,
            disk_search_filter: String::new(),
            confirm_scroll: 0,

            last_treemap_key: None,
            last_treemap_screen_rect: None,
            throughput_samples: VecDeque::with_capacity(48),
            last_hash_sample_at: None,
            last_hash_done_sample: 0,
            last_periodic_reconcile: None,
            last_partial_db_refresh: None,
            reduced_color: false,
            search_bar_active: false,
            search_buffer: String::new(),
            other_drill: None,
            other_table_state: TableState::default(),
            dup_quick_wins: Vec::new(),
            dup_quick_wins_fp: None,
            dup_review: None,
        }
    }

    fn push_log(&mut self, line: impl Into<String>) {
        let s = line.into();
        if self.log_lines.len() >= DISK_LOG_MAX {
            self.log_lines.pop_front();
        }
        self.log_lines.push_back(s);
    }

    pub fn init_paths(&mut self) {
        let prev_root = self.root.clone();
        self.root = resolve_disk_index_root(self.root.clone());
        if self.current_dir.as_os_str().is_empty() {
            self.current_dir = self.root.clone();
        } else if self.current_dir == prev_root {
            self.current_dir = self.root.clone();
        }
        if let Some(parent) = self.db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    /// Call when the Disk tab is shown (first time starts background pipeline).
    pub fn ensure_started(&mut self) {
        self.init_paths();
        if !self.started_once {
            self.started_once = true;
            self.start_pipeline();
        }
    }

    /// Full rescan (when idle/ready). Press `R`.
    pub fn request_rescan(&mut self) {
        if self.pipeline_running {
            self.status_msg = "Scan already running…".to_string();
            return;
        }
        self.watch_cancel.store(true, Ordering::SeqCst);
        self.start_pipeline();
    }

    fn start_pipeline(&mut self) {
        self.init_paths();
        self.pipeline_running = true;
        self.phase = ScanPhase::Walking;
        self.status_msg.clear();
        self.read_conn = None;
        self.indexed_bytes_total = 0;
        self.last_partial_db_refresh = None;
        self.cancel.store(false, Ordering::SeqCst);
        self.watch_cancel.store(false, Ordering::SeqCst);
        self.throughput_samples.clear();
        self.last_hash_sample_at = None;
        self.last_hash_done_sample = 0;
        self.last_treemap_key = None;
        self.bytes = 0;

        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let (db_tx, db_rx) = bounded::<Vec<FileMeta>>(64);
        let root = self.root.clone();
        let db_path = self.db_path.clone();
        let ui_tx = self.ui_tx.clone();
        let ui_tx_walk = self.ui_tx.clone();
        let cancel = Arc::clone(&self.cancel);
        let watch_cancel = Arc::clone(&self.watch_cancel);

        let root_key = root.to_string_lossy().into_owned();

        std::thread::spawn(move || {
            let mut conn = match store::open_write(&db_path) {
                Ok(c) => c,
                Err(e) => {
                    let _ = ui_tx.send(DiskUiEvent::Error(e.to_string()));
                    return;
                }
            };

            let warm = store::get_meta(&conn, store::META_INDEXED_ROOT)
                .ok()
                .flatten()
                .map(|v| v == root_key)
                .unwrap_or(false);

            if !warm {
                if let Err(e) = store::clear_entries(&conn) {
                    let _ = ui_tx.send(DiskUiEvent::Error(e.to_string()));
                    return;
                }
                let _ = ui_tx.send(DiskUiEvent::Log(
                    "Full index rebuild (new root or first run)".into(),
                ));
            } else {
                let _ = ui_tx.send(DiskUiEvent::Log("Incremental scan (warm index)".into()));
            }

            while let Ok(batch) = db_rx.recv() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = store::insert_batch(&mut conn, epoch, &batch) {
                    let _ = ui_tx.send(DiskUiEvent::Error(e.to_string()));
                    break;
                }
            }

            if cancel.load(Ordering::Relaxed) {
                let _ = ui_tx.send(DiskUiEvent::Error("Scan cancelled".to_string()));
                return;
            }

            match store::delete_stale_entries(&conn, epoch) {
                Ok(n) if n > 0 => {
                    let _ = ui_tx.send(DiskUiEvent::Log(format!("Removed {n} stale index rows")));
                }
                _ => {}
            }

            let (fc_agg, dc_agg) = store::count_entries(&conn).unwrap_or((0, 0));
            let bytes_agg = store::total_file_bytes(&conn).unwrap_or(0);
            let _ = ui_tx.send(DiskUiEvent::Progress {
                phase: ScanPhase::Aggregating,
                files: fc_agg,
                dirs: dc_agg,
                bytes_scanned: bytes_agg,
                message: "aggregating directory sizes (SQL)…".to_string(),
            });

            let ui_tx_agg = ui_tx.clone();
            let mut last_heartbeat = std::time::Instant::now();
            if let Err(e) = store::aggregate_directory_sizes_with_progress(&conn, |pass| {
                let now = std::time::Instant::now();
                let due = pass % 25 == 0
                    || now.duration_since(last_heartbeat) >= std::time::Duration::from_secs(2);
                if due {
                    last_heartbeat = now;
                    let _ = ui_tx_agg.send(DiskUiEvent::Progress {
                        phase: ScanPhase::Aggregating,
                        files: fc_agg,
                        dirs: dc_agg,
                        bytes_scanned: bytes_agg,
                        message: format!("aggregating directory sizes — SQL pass {pass}"),
                    });
                }
            }) {
                let _ = ui_tx.send(DiskUiEvent::Error(e.to_string()));
                return;
            }

            let (fc, dc) = store::count_entries(&conn).unwrap_or((0, 0));
            let _ = ui_tx.send(DiskUiEvent::ScanComplete {
                files: fc,
                dirs: dc,
            });
            let _ = ui_tx.send(DiskUiEvent::Progress {
                phase: ScanPhase::Hashing,
                files: fc,
                dirs: dc,
                bytes_scanned: 0,
                message: "hashing duplicates".to_string(),
            });

            if let Err(e) = hash::run_hash_and_dedupe(&mut conn, &ui_tx, !warm) {
                let _ = ui_tx.send(DiskUiEvent::Error(e.to_string()));
            }

            let _ = store::set_meta(&conn, store::META_INDEXED_ROOT, &root_key);
            let _ = store::set_meta(&conn, store::META_LAST_COMPLETED_EPOCH, &epoch.to_string());

            let reclaim = store::reclaimable_bytes(&conn).unwrap_or(0);
            let _ = ui_tx.send(DiskUiEvent::Progress {
                phase: ScanPhase::Ready,
                files: fc,
                dirs: dc,
                bytes_scanned: 0,
                message: format!("reclaimable {}", reclaim),
            });

            drop(conn);
            let _ = ui_tx.send(DiskUiEvent::IndexFlushed);

            let _ = watch::spawn_watcher(root, watch_cancel, ui_tx.clone());
        });

        let cancel_w = Arc::clone(&self.cancel);
        let root_w = self.root.clone();
        std::thread::spawn(move || {
            scan::walk_root_parallel(root_w, epoch, ui_tx_walk, db_tx, &cancel_w);
        });
    }

    /// Drain worker messages (call each UI tick / frame).
    pub fn poll_events(&mut self) {
        while let Ok(ev) = self.ui_rx.try_recv() {
            match ev {
                DiskUiEvent::Progress {
                    phase,
                    files,
                    dirs,
                    bytes_scanned,
                    message,
                } => {
                    self.phase = phase;
                    self.files = files;
                    self.dirs = dirs;
                    if bytes_scanned > 0 {
                        self.bytes = bytes_scanned;
                    }
                    if !message.is_empty() {
                        self.status_msg = message;
                    }
                    self.last_partial_db_refresh = None;
                }
                DiskUiEvent::ScanComplete { files, dirs } => {
                    self.files = files;
                    self.dirs = dirs;
                    self.last_partial_db_refresh = None;
                }
                DiskUiEvent::HashProgress { done, total } => {
                    let now = Instant::now();
                    if let Some(prev) = self.last_hash_sample_at {
                        let dt = now.duration_since(prev).as_secs_f64();
                        if dt >= 0.2 && done > self.last_hash_done_sample {
                            let rate = (done - self.last_hash_done_sample) as f64 / dt;
                            if self.throughput_samples.len() >= 48 {
                                self.throughput_samples.pop_front();
                            }
                            self.throughput_samples.push_back(rate);
                            self.last_hash_done_sample = done;
                            self.last_hash_sample_at = Some(now);
                        }
                    } else {
                        self.last_hash_sample_at = Some(now);
                        self.last_hash_done_sample = done;
                    }
                    self.hash_done = done;
                    self.hash_total = total;
                }
                DiskUiEvent::DuplicatesReady {
                    groups,
                    reclaimable_bytes,
                } => {
                    self.dup_groups = groups;
                    self.reclaim_bytes = reclaimable_bytes;
                    self.dup_quick_wins_fp = None;
                }
                DiskUiEvent::Error(s) => {
                    self.phase = ScanPhase::Error;
                    self.status_msg = s;
                    self.pipeline_running = false;
                }
                DiskUiEvent::IndexFlushed => {
                    self.read_conn = store::open_read(&self.db_path).ok();
                    self.phase = ScanPhase::Watching;
                    self.pipeline_running = false;
                    self.last_periodic_reconcile = None;
                    self.last_partial_db_refresh = None;
                    self.dup_quick_wins_fp = None;
                    if let Some(ref c) = self.read_conn {
                        self.indexed_bytes_total = store::total_file_bytes(c).unwrap_or(0);
                    }
                    self.refresh_children();
                }
                DiskUiEvent::WatchSuggested => {
                    self.status_msg = "FS changed — press R to rescan".to_string();
                    self.push_log("Filesystem change detected (debounced)");
                }
                DiskUiEvent::Log(s) => {
                    self.push_log(s);
                }
            }
        }

        self.try_open_read_during_pipeline();
        self.maybe_refresh_partial_index_throttled();

        if self.phase == ScanPhase::Watching {
            let now = Instant::now();
            if self.last_periodic_reconcile.is_none() {
                self.last_periodic_reconcile = Some(now);
            } else if self
                .last_periodic_reconcile
                .is_some_and(|t| t.elapsed() >= std::time::Duration::from_secs(300))
            {
                self.last_periodic_reconcile = Some(now);
                if let Some(ref c) = self.read_conn {
                    if let Ok((f, d)) = store::count_entries(c) {
                        self.push_log(format!("Periodic index check: {f} files, {d} dirs"));
                    }
                }
            }
        }
    }

    /// True while walk / aggregate / hash pipeline is running (before `IndexFlushed`).
    pub fn pipeline_active(&self) -> bool {
        self.pipeline_running
    }

    fn try_open_read_during_pipeline(&mut self) {
        if !self.pipeline_running {
            return;
        }
        if self.read_conn.is_some() {
            return;
        }
        if !self.db_path.exists() {
            return;
        }
        if let Ok(c) = store::open_read(&self.db_path) {
            self.read_conn = Some(c);
            self.last_partial_db_refresh = None;
        }
    }

    fn maybe_refresh_partial_index_throttled(&mut self) {
        if !self.pipeline_running {
            return;
        }
        if self.read_conn.is_none() {
            return;
        }
        const MIN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(450);
        let now = Instant::now();
        if let Some(t) = self.last_partial_db_refresh {
            if now.duration_since(t) < MIN_INTERVAL {
                return;
            }
        }
        self.last_partial_db_refresh = Some(now);
        if let Some(ref c) = self.read_conn {
            self.indexed_bytes_total = store::total_file_bytes(c).unwrap_or(0);
        }
        self.refresh_children();
    }

    pub fn refresh_children(&mut self) {
        let Some(conn) = self.read_conn.as_ref() else {
            return;
        };
        let parent = store::path_key_for_parent_query(&self.current_dir);
        match store::query_children(conn, &parent) {
            Ok(rows) => {
                let mut rows = hash::rowdb_to_rows(rows);
                if !self.disk_search_filter.is_empty() {
                    let q = self.disk_search_filter.to_lowercase();
                    rows.retain(|e| {
                        e.name.to_lowercase().contains(&q) || e.path.to_lowercase().contains(&q)
                    });
                }
                self.layout_children_sig = children_signature(&rows);
                self.children = rows;
                self.other_drill = None;
                self.other_table_state = TableState::default();
                if self.list_state.selected().is_none() && !self.children.is_empty() {
                    self.list_state.select(Some(0));
                }
                if self.tree_state.focus >= self.children.len() {
                    self.tree_state.focus = self.children.len().saturating_sub(1);
                }
            }
            Err(e) => self.status_msg = format!("query: {e}"),
        }
    }

    pub fn close_other_drill(&mut self) {
        self.other_drill = None;
        self.other_table_state = TableState::default();
    }

    pub fn open_other_drill_from_focus(&mut self) {
        let Some(tile) = self.selected_tile() else {
            return;
        };
        if tile.child_indices.len() <= 1 {
            return;
        }
        let mut idxs: Vec<usize> = tile.child_indices.clone();
        idxs.sort_by(|&a, &b| {
            let sa = self.children.get(a).map(|e| e.size_bytes).unwrap_or(0);
            let sb = self.children.get(b).map(|e| e.size_bytes).unwrap_or(0);
            sb.cmp(&sa)
                .then_with(|| self.children[a].path.cmp(&self.children[b].path))
        });
        self.other_drill = Some(idxs);
        self.other_table_state.select(Some(0));
        self.last_treemap_key = None;
    }

    fn focus_treemap_on_child_index(&mut self, child_idx: usize) {
        for (ti, t) in self.tiles.iter().enumerate() {
            if t.child_indices.contains(&child_idx) {
                self.tree_state.focus = ti;
                return;
            }
        }
    }

    fn drill_other_list_enter(&mut self) {
        let Some(ref indices) = self.other_drill else {
            return;
        };
        let sel = self.other_table_state.selected().unwrap_or(0);
        let Some(&idx) = indices.get(sel) else {
            return;
        };
        let Some(e) = self.children.get(idx) else {
            return;
        };
        if e.is_dir {
            self.current_dir = e.path_buf();
            self.close_other_drill();
            self.list_state.select(Some(0));
            self.tree_state.focus = 0;
            self.refresh_children();
            self.last_treemap_key = None;
        } else {
            self.close_other_drill();
            self.focus_treemap_on_child_index(idx);
        }
    }

    pub fn other_list_nav_up(&mut self, n: usize) {
        let Some(ref ix) = self.other_drill else {
            return;
        };
        let max = ix.len();
        if max == 0 {
            return;
        }
        let i = self
            .other_table_state
            .selected()
            .unwrap_or(0)
            .saturating_sub(n);
        self.other_table_state.select(Some(i));
    }

    pub fn other_list_nav_down(&mut self, n: usize) {
        let Some(ref ix) = self.other_drill else {
            return;
        };
        let max = ix.len();
        if max == 0 {
            return;
        }
        let i = (self.other_table_state.selected().unwrap_or(0) + n).min(max - 1);
        self.other_table_state.select(Some(i));
    }

    pub fn set_disk_search(&mut self, filter: String) {
        self.disk_search_filter = filter;
        self.refresh_children();
    }

    pub fn selected_tile(&self) -> Option<&TreeTile> {
        if self.other_drill.is_some() {
            return None;
        }
        if !self.layout_treemap || self.tiles.is_empty() {
            return None;
        }
        let ti = self
            .tree_state
            .focus
            .min(self.tiles.len().saturating_sub(1));
        self.tiles.get(ti)
    }

    pub fn selected_entry(&self) -> Option<&EntryRow> {
        if let Some(ref ix) = self.other_drill {
            let sel = self.other_table_state.selected()?;
            let ci = *ix.get(sel)?;
            return self.children.get(ci);
        }
        if self.layout_treemap && !self.tiles.is_empty() {
            let tile = self.selected_tile()?;
            if tile.child_indices.len() > 1 {
                return None;
            }
            return self.children.get(tile.primary_index());
        }
        self.list_state
            .selected()
            .and_then(|i| self.children.get(i))
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_entry().map(|e| e.path_buf())
    }

    pub fn toggle_mark(&mut self) {
        if let Some(t) = self.selected_tile() {
            if t.child_indices.len() > 1 {
                return;
            }
        }
        let Some(p) = self.selected_path() else {
            return;
        };
        if p == self.root {
            return;
        }
        if let Some(e) = self.children.iter().find(|e| e.path_buf() == p) {
            if e.is_dir {
                return;
            }
        } else {
            return;
        }
        if self.marked.contains(&p) {
            self.marked.remove(&p);
        } else {
            self.marked.insert(p);
        }
    }

    pub fn reveal_selected_in_finder(&self) -> Result<(), String> {
        let Some(p) = self.selected_path() else {
            return Err("nothing selected".into());
        };
        std::process::Command::new("open")
            .args(["-R", p.to_str().ok_or("path not UTF-8")?])
            .status()
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn drill_into(&mut self) {
        if self.other_drill.is_some() {
            self.drill_other_list_enter();
            return;
        }
        if let Some(t) = self.selected_tile() {
            if t.child_indices.len() > 1 {
                self.open_other_drill_from_focus();
                return;
            }
        }
        let Some(p) = self.selected_path() else {
            return;
        };
        if !self.children.iter().any(|e| e.path_buf() == p && e.is_dir) {
            return;
        }
        self.current_dir = p;
        self.list_state.select(Some(0));
        self.tree_state.focus = 0;
        self.refresh_children();
    }

    pub fn drill_up(&mut self) {
        if self.other_drill.is_some() {
            self.close_other_drill();
            return;
        }
        if self.current_dir == self.root {
            return;
        }
        let Some(p) = self.current_dir.parent() else {
            self.current_dir = self.root.clone();
            self.list_state.select(Some(0));
            self.tree_state.focus = 0;
            self.refresh_children();
            return;
        };
        if p.starts_with(&self.root) || p == self.root.as_path() {
            self.current_dir = p.to_path_buf();
        } else {
            self.current_dir = self.root.clone();
        }
        self.list_state.select(Some(0));
        self.tree_state.focus = 0;
        self.refresh_children();
    }

    /// Move marked files to Trash and remove from index.
    pub fn trash_marked(&mut self) -> Result<usize, String> {
        let paths: Vec<String> = self
            .marked
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if paths.is_empty() {
            return Err("nothing marked".to_string());
        }
        for p in &paths {
            trash::delete(std::path::Path::new(p)).map_err(|e| e.to_string())?;
        }
        if let Ok(conn) = store::open_write(&self.db_path) {
            let _ = store::remove_paths(&conn, &paths);
        }
        self.last_trashed = paths.clone();
        self.marked.clear();
        self.confirm_delete = false;
        self.read_conn = store::open_read(&self.db_path).ok();
        self.dup_quick_wins_fp = None;
        self.refresh_children();
        Ok(paths.len())
    }

    /// Refresh inspector quick wins when `(reclaim_bytes, dup_groups)` changes.
    pub fn try_refresh_dup_quick_wins(&mut self) {
        let fp = (self.reclaim_bytes, self.dup_groups);
        if self.dup_quick_wins_fp == Some(fp) {
            return;
        }
        let Some(ref c) = self.read_conn else {
            self.dup_quick_wins.clear();
            self.dup_quick_wins_fp = None;
            return;
        };
        if !matches!(self.phase, ScanPhase::Watching | ScanPhase::Ready) {
            return;
        }
        match store::list_dup_groups_quick(c, 8) {
            Ok(v) => {
                self.dup_quick_wins = v;
                self.dup_quick_wins_fp = Some(fp);
            }
            Err(_) => {}
        }
    }

    pub fn open_dup_review(&mut self) {
        if self.pipeline_running {
            self.status_msg = "Wait for scan to finish".to_string();
            return;
        }
        let Some(ref c) = self.read_conn else {
            self.status_msg = "Index not ready".to_string();
            return;
        };
        let rows = match store::list_dup_groups_quick(c, 50) {
            Ok(r) => r,
            Err(e) => {
                self.status_msg = format!("dup list: {e}");
                return;
            }
        };
        if rows.is_empty() {
            self.status_msg = "No duplicate groups in index".to_string();
            return;
        }
        let mut groups_table = TableState::default();
        groups_table.select(Some(0));
        self.dup_review = Some(DupReviewState {
            phase: DupReviewPhase::ListGroups,
            group_rows: rows,
            groups_table,
            member_rows: Vec::new(),
            members_table: TableState::default(),
            pick_group_id: 0,
        });
    }

    pub fn close_dup_review(&mut self) {
        self.dup_review = None;
    }

    /// Esc: close modal, or back from pick-keeper to group list.
    pub fn dup_review_escape(&mut self) {
        let Some(st) = self.dup_review.as_mut() else {
            return;
        };
        if st.phase == DupReviewPhase::PickKeeper {
            st.phase = DupReviewPhase::ListGroups;
            st.member_rows.clear();
            st.members_table = TableState::default();
            st.pick_group_id = 0;
        } else {
            self.close_dup_review();
        }
    }

    /// Enter: open group detail, or apply keeper and close.
    pub fn dup_review_on_enter(&mut self) -> Result<(), String> {
        let Some(st) = self.dup_review.as_mut() else {
            return Ok(());
        };
        match st.phase {
            DupReviewPhase::ListGroups => {
                let i = st.groups_table.selected().unwrap_or(0);
                let row = st.group_rows.get(i).ok_or_else(|| "no group".to_string())?;
                let c = self.read_conn.as_ref().ok_or_else(|| "no db".to_string())?;
                st.member_rows = store::load_dup_group_members(c, row.dup_group_id)
                    .map_err(|e| e.to_string())?;
                st.pick_group_id = row.dup_group_id;
                let sel = st
                    .member_rows
                    .iter()
                    .position(|m| m.keep_winner)
                    .unwrap_or(0);
                st.members_table.select(Some(sel));
                st.phase = DupReviewPhase::PickKeeper;
            }
            DupReviewPhase::PickKeeper => {
                let gid = st.pick_group_id;
                let sel = st
                    .members_table
                    .selected()
                    .ok_or_else(|| "no selection".to_string())?;
                let path = st
                    .member_rows
                    .get(sel)
                    .ok_or_else(|| "bad row".to_string())?
                    .path
                    .clone();
                let mut w = store::open_write(&self.db_path).map_err(|e| e.to_string())?;
                store::apply_dup_group_keeper(&mut w, gid, &path).map_err(|e| e.to_string())?;
                drop(w);
                self.read_conn = store::open_read(&self.db_path).ok();
                if let Some(ref c) = self.read_conn {
                    self.reclaim_bytes = store::reclaimable_bytes(c).unwrap_or(0);
                    self.dup_groups = store::count_dup_groups(c).unwrap_or(0);
                }
                self.dup_quick_wins_fp = None;
                self.try_refresh_dup_quick_wins();
                self.refresh_children();
                self.last_treemap_key = None;
                self.close_dup_review();
                self.status_msg =
                    "Keeper saved — others marked for Trash; press t to confirm Trash".to_string();
            }
        }
        Ok(())
    }

    pub fn dup_review_nav_up(&mut self, n: usize) {
        let Some(st) = self.dup_review.as_mut() else {
            return;
        };
        match st.phase {
            DupReviewPhase::ListGroups => {
                let max = st.group_rows.len();
                if max == 0 {
                    return;
                }
                let i = st.groups_table.selected().unwrap_or(0).saturating_sub(n);
                st.groups_table.select(Some(i));
            }
            DupReviewPhase::PickKeeper => {
                let max = st.member_rows.len();
                if max == 0 {
                    return;
                }
                let i = st.members_table.selected().unwrap_or(0).saturating_sub(n);
                st.members_table.select(Some(i));
            }
        }
    }

    pub fn dup_review_nav_down(&mut self, n: usize) {
        let Some(st) = self.dup_review.as_mut() else {
            return;
        };
        match st.phase {
            DupReviewPhase::ListGroups => {
                let max = st.group_rows.len();
                if max == 0 {
                    return;
                }
                let i = (st.groups_table.selected().unwrap_or(0) + n).min(max - 1);
                st.groups_table.select(Some(i));
            }
            DupReviewPhase::PickKeeper => {
                let max = st.member_rows.len();
                if max == 0 {
                    return;
                }
                let i = (st.members_table.selected().unwrap_or(0) + n).min(max - 1);
                st.members_table.select(Some(i));
            }
        }
    }

    pub fn dup_review_home(&mut self) {
        let Some(st) = self.dup_review.as_mut() else {
            return;
        };
        match st.phase {
            DupReviewPhase::ListGroups => {
                if !st.group_rows.is_empty() {
                    st.groups_table.select(Some(0));
                }
            }
            DupReviewPhase::PickKeeper => {
                if !st.member_rows.is_empty() {
                    st.members_table.select(Some(0));
                }
            }
        }
    }

    pub fn dup_review_end(&mut self) {
        let Some(st) = self.dup_review.as_mut() else {
            return;
        };
        match st.phase {
            DupReviewPhase::ListGroups => {
                let n = st.group_rows.len();
                if n > 0 {
                    st.groups_table.select(Some(n - 1));
                }
            }
            DupReviewPhase::PickKeeper => {
                let n = st.member_rows.len();
                if n > 0 {
                    st.members_table.select(Some(n - 1));
                }
            }
        }
    }

    pub fn treemap_nav(&mut self, dir: u8) {
        if self.tiles.is_empty() {
            return;
        }
        if let Some(nei) = self
            .tree_state
            .neighbors
            .get(self.tree_state.focus)
            .and_then(|n| n.get(dir as usize).copied().flatten())
        {
            if nei < self.tiles.len() {
                self.tree_state.focus = nei;
            }
        }
    }

    pub fn list_nav_up(&mut self, n: usize) {
        let max = self.children.len();
        if max == 0 {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0).saturating_sub(n);
        self.list_state.select(Some(i));
    }

    pub fn list_nav_down(&mut self, n: usize) {
        let max = self.children.len();
        if max == 0 {
            return;
        }
        let i = self
            .list_state
            .selected()
            .unwrap_or(0)
            .saturating_add(n)
            .min(max.saturating_sub(1));
        self.list_state.select(Some(i));
    }
}

impl Default for DiskViewModel {
    fn default() -> Self {
        Self::new()
    }
}

fn children_signature(rows: &[crate::disk::model::EntryRow]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    for r in rows {
        r.path.hash(&mut h);
        r.size_bytes.hash(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod path_tests {
    use super::resolve_disk_index_root;
    use std::path::PathBuf;

    #[test]
    fn resolve_disk_index_root_canonicalizes_existing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let raw: PathBuf = dir.path().to_path_buf();
        let resolved = resolve_disk_index_root(raw.clone());
        assert!(resolved.is_absolute());
        assert_eq!(
            std::fs::canonicalize(&raw).unwrap(),
            resolved,
            "resolve_disk_index_root should match std::fs::canonicalize for an existing directory"
        );
    }

    #[test]
    fn resolve_disk_index_root_falls_back_when_missing() {
        let missing = PathBuf::from("/nonexistent/macjet_disk_root_test_abc123");
        let resolved = resolve_disk_index_root(missing.clone());
        assert_eq!(resolved, missing);
    }
}
