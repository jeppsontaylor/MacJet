//! Parallel filesystem walk using `ignore` (WalkParallel).

use crate::disk::model::{
    is_transient_download_name, system_time_ms, DiskUiEvent, FileMeta, ScanPhase,
};
use crate::disk::names::normalize_filename;
use crossbeam_channel::Sender;
use ignore::WalkBuilder;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

const BATCH: usize = 800;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

struct ThreadBatch {
    batch: Vec<FileMeta>,
    db_tx: Sender<Vec<FileMeta>>,
}

impl ThreadBatch {
    fn new(db_tx: Sender<Vec<FileMeta>>) -> Self {
        Self {
            batch: Vec::with_capacity(BATCH),
            db_tx,
        }
    }

    fn push(&mut self, fm: FileMeta) {
        self.batch.push(fm);
        if self.batch.len() >= BATCH {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.batch.is_empty() {
            return;
        }
        let next = Vec::with_capacity(BATCH);
        let old = std::mem::replace(&mut self.batch, next);
        let _ = self.db_tx.send(old);
    }
}

impl Drop for ThreadBatch {
    fn drop(&mut self) {
        self.flush();
    }
}

pub fn walk_root_parallel(
    root: PathBuf,
    _epoch: i64,
    ui_tx: Sender<DiskUiEvent>,
    db_tx: Sender<Vec<FileMeta>>,
    cancel: &AtomicBool,
) {
    let root_str = root.to_string_lossy().into_owned();
    let start = Instant::now();
    let files = Arc::new(AtomicU64::new(0));
    let dirs = Arc::new(AtomicU64::new(0));
    let bytes = Arc::new(AtomicU64::new(0));

    let mut builder = WalkBuilder::new(&root);
    builder.hidden(false);
    builder.git_ignore(false);
    builder.ignore(false);
    builder.follow_links(false);
    builder.standard_filters(false);

    let walk = builder.build_parallel();
    walk.run(|| {
        let ui_tx = ui_tx.clone();
        let db_tx = db_tx.clone();
        let root_str = root_str.clone();
        let files_c = Arc::clone(&files);
        let dirs_c = Arc::clone(&dirs);
        let bytes_c = Arc::clone(&bytes);
        let mut tb = ThreadBatch::new(db_tx);
        let mut progress_counter: u32 = 0;

        Box::new(move |result| {
            if cancel.load(Ordering::Relaxed) {
                return ignore::WalkState::Quit;
            }

            let entry = match result {
                Ok(e) => e,
                Err(_) => return ignore::WalkState::Continue,
            };

            let path = entry.path();
            if path.to_string_lossy() == root_str {
                return ignore::WalkState::Continue;
            }

            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => return ignore::WalkState::Continue,
            };

            let is_dir = meta.is_dir();
            let (size, mtime_ms, inode, dev) = if is_dir {
                (0u64, None, None, None)
            } else {
                let sz = meta.len();
                let mt = meta.modified().ok().and_then(system_time_ms);
                #[cfg(unix)]
                let (ino, dv) = (Some(meta.ino()), Some(meta.dev()));
                #[cfg(not(unix))]
                let (ino, dv) = (None, None);
                (sz, mt, ino, dv)
            };

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            let mut flags = 0u32;
            if name.starts_with('.') {
                flags |= crate::disk::model::EntryFlags::IS_HIDDEN;
            }
            if meta.is_symlink() {
                flags |= crate::disk::model::EntryFlags::IS_SYMLINK;
            }
            if is_transient_download_name(&name) {
                flags |= crate::disk::model::EntryFlags::TRANSIENT_DOWNLOAD;
            }

            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();

            let norm = normalize_filename(std::path::Path::new(&name).as_os_str());

            let parent_path = path.parent().map(PathBuf::from);

            let fm = FileMeta {
                path: path.to_path_buf(),
                parent_path,
                name,
                is_dir,
                size_bytes: size,
                mtime_ms,
                ext,
                inode,
                dev: dev.map(|d| d as u64),
                flags,
                normalized_stem: norm.normalized_stem.clone(),
                copy_index: norm.copy_index.map(|i| i as i32),
            };

            if is_dir {
                dirs_c.fetch_add(1, Ordering::Relaxed);
            } else {
                files_c.fetch_add(1, Ordering::Relaxed);
                bytes_c.fetch_add(size, Ordering::Relaxed);
            }

            tb.push(fm);

            progress_counter = progress_counter.wrapping_add(1);
            if progress_counter % 4096 == 0 {
                let f = files_c.load(Ordering::Relaxed);
                let d = dirs_c.load(Ordering::Relaxed);
                let b = bytes_c.load(Ordering::Relaxed);
                let elapsed = start.elapsed().as_secs_f64().max(0.001);
                let fps = f as f64 / elapsed;
                let _ = ui_tx.send(DiskUiEvent::Progress {
                    phase: ScanPhase::Walking,
                    files: f,
                    dirs: d,
                    bytes_scanned: b,
                    message: format!("{fps:.0} files/s"),
                });
            }

            ignore::WalkState::Continue
        })
    });

    let f = files.load(Ordering::Relaxed);
    let d = dirs.load(Ordering::Relaxed);
    let b = bytes.load(Ordering::Relaxed);
    let elapsed = start.elapsed().as_secs_f64().max(0.001);
    let fps = f as f64 / elapsed;
    let _ = ui_tx.send(DiskUiEvent::Progress {
        phase: ScanPhase::Walking,
        files: f,
        dirs: d,
        bytes_scanned: b,
        message: format!("{fps:.0} files/s (final)"),
    });
}
