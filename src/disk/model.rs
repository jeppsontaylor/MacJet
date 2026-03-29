//! Disk index model: entry rows, flags, scan phases, UI messages.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Bitfield for `entries.flags`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryFlags(pub u32);

impl EntryFlags {
    pub const IS_HIDDEN: u32 = 1 << 0;
    pub const IS_SYMLINK: u32 = 1 << 1;
    pub const TRANSIENT_DOWNLOAD: u32 = 1 << 2;
    pub const LIKELY_DELETE: u32 = 1 << 3;
    pub const KEEP_WINNER: u32 = 1 << 4;
    pub const HASH_DIRTY: u32 = 1 << 5;

    pub fn contains(self, bit: u32) -> bool {
        (self.0 & bit) != 0
    }
}

#[derive(Debug, Clone)]
pub struct FileMeta {
    pub path: PathBuf,
    pub parent_path: Option<PathBuf>,
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub mtime_ms: Option<i64>,
    pub ext: String,
    pub inode: Option<u64>,
    pub dev: Option<u64>,
    pub flags: u32,
    pub normalized_stem: String,
    pub copy_index: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct EntryRow {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub mtime_ms: Option<i64>,
    pub ext: String,
    pub flags: u32,
    pub full_hash: Option<[u8; 32]>,
    pub dup_group_id: Option<i64>,
    pub likely_delete: bool,
    pub keep_winner: bool,
}

impl EntryRow {
    pub fn path_buf(&self) -> PathBuf {
        PathBuf::from(&self.path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanPhase {
    Idle,
    Walking,
    Aggregating,
    Hashing,
    Ready,
    Watching,
    Error,
}

impl ScanPhase {
    pub fn label(self) -> &'static str {
        match self {
            ScanPhase::Idle => "idle",
            ScanPhase::Walking => "scanning",
            ScanPhase::Aggregating => "aggregating",
            ScanPhase::Hashing => "hashing dupes",
            ScanPhase::Ready => "ready",
            ScanPhase::Watching => "watching",
            ScanPhase::Error => "error",
        }
    }
}

/// Events from background workers → UI thread (bounded channel).
#[derive(Debug)]
pub enum DiskUiEvent {
    Progress {
        phase: ScanPhase,
        files: u64,
        dirs: u64,
        bytes_scanned: u64,
        message: String,
    },
    ScanComplete {
        files: u64,
        dirs: u64,
    },
    HashProgress {
        done: u64,
        total: u64,
    },
    DuplicatesReady {
        groups: u64,
        reclaimable_bytes: u64,
    },
    Error(String),
    /// Read-only DB connection should be reopened (path unchanged).
    IndexFlushed,
    /// Filesystem changed — user can press `R` to rescan.
    WatchSuggested,
    /// Append to disk activity log (ring buffer in UI).
    Log(String),
}

#[derive(Debug, Clone)]
pub struct NormalizedName {
    pub display_name: String,
    pub ext: String,
    pub normalized_stem: String,
    pub copy_index: Option<u32>,
    pub has_copy_suffix: bool,
}

pub fn system_time_ms(st: SystemTime) -> Option<i64> {
    st.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

pub fn is_transient_download_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".crdownload")
        || lower.ends_with(".part")
        || lower.ends_with(".download")
        || lower.ends_with(".tmp")
        || lower.ends_with(".partial")
}

pub fn parent_path_str(path: &Path) -> Option<String> {
    path.parent().map(|p| p.to_string_lossy().into_owned())
}
