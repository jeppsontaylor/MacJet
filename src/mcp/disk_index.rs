//! Read-only disk index access for MCP (same SQLite as TUI).

use crate::disk::default_disk_index_path;
use crate::disk::store;
use crate::mcp::snapshot::{wrap, McpSnapshot};
use serde_json::json;
use std::path::{Path, PathBuf};

fn open_disk_db(path: Option<PathBuf>) -> Result<rusqlite::Connection, String> {
    let p = path.unwrap_or_else(default_disk_index_path);
    if !p.exists() {
        return Err(format!(
            "disk index not found at {} — open MacJet Disk tab once to build the index",
            p.display()
        ));
    }
    store::open_read(&p).map_err(|e| e.to_string())
}

pub fn json_disk_summary(snap: &McpSnapshot, db_path: Option<PathBuf>) -> String {
    match open_disk_db(db_path) {
        Ok(conn) => {
            let root = store::get_meta(&conn, store::META_INDEXED_ROOT)
                .unwrap_or(None)
                .unwrap_or_default();
            let epoch = store::get_meta(&conn, store::META_LAST_COMPLETED_EPOCH)
                .unwrap_or(None)
                .unwrap_or_default();
            let (files, dirs) = store::count_entries(&conn).unwrap_or((0, 0));
            let reclaim = store::reclaimable_bytes(&conn).unwrap_or(0);
            let total = store::total_file_bytes(&conn).unwrap_or(0);
            let dup_groups = store::count_dup_groups(&conn).unwrap_or(0);
            let data = json!({
                "indexed_root": root,
                "last_completed_epoch": epoch,
                "file_count": files,
                "dir_count": dirs,
                "total_file_bytes": total,
                "reclaimable_bytes": reclaim,
                "dup_group_count": dup_groups,
                "staleness": "Index reflects last completed MacJet disk scan; run TUI Disk tab or rescan for freshness.",
            });
            serde_json::to_string(&wrap(snap, data)).unwrap_or_default()
        }
        Err(e) => serde_json::to_string(&wrap(
            snap,
            json!({ "error": e, "hint": "Build index from MacJet Disk view (key 6)." }),
        ))
        .unwrap_or_default(),
    }
}

pub fn json_disk_duplicates(
    snap: &McpSnapshot,
    db_path: Option<PathBuf>,
    limit: u64,
    min_size: u64,
    only_reclaim: bool,
) -> String {
    match open_disk_db(db_path) {
        Ok(conn) => match store::mcp_duplicate_rows(&conn, limit, min_size, only_reclaim) {
            Ok(rows) => serde_json::to_string(&wrap(snap, json!({ "duplicates": rows })))
                .unwrap_or_default(),
            Err(e) => serde_json::to_string(&wrap(snap, json!({ "error": e.to_string() })))
                .unwrap_or_default(),
        },
        Err(e) => serde_json::to_string(&wrap(snap, json!({ "error": e }))).unwrap_or_default(),
    }
}

pub fn json_disk_directory(snap: &McpSnapshot, db_path: Option<PathBuf>, path: &str) -> String {
    match open_disk_db(db_path) {
        Ok(conn) => {
            let parent = path.trim_end_matches('/');
            match store::query_children(&conn, parent) {
                Ok(rows) => {
                    let v: Vec<_> = rows
                        .into_iter()
                        .map(|r| {
                            json!({
                                "path": r.path,
                                "name": r.name,
                                "is_dir": r.is_dir,
                                "size_bytes": r.size_bytes,
                            })
                        })
                        .collect();
                    serde_json::to_string(&wrap(snap, json!({ "children": v }))).unwrap_or_default()
                }
                Err(e) => serde_json::to_string(&wrap(snap, json!({ "error": e.to_string() })))
                    .unwrap_or_default(),
            }
        }
        Err(e) => serde_json::to_string(&wrap(snap, json!({ "error": e }))).unwrap_or_default(),
    }
}

pub fn json_suggest_disk_cleanup(snap: &McpSnapshot, db_path: Option<PathBuf>) -> String {
    match open_disk_db(db_path) {
        Ok(conn) => {
            let rows = store::mcp_duplicate_rows(&conn, 200, 1024, true).unwrap_or_default();
            let safe: u64 = rows
                .iter()
                .filter(|r| r.likely_delete)
                .map(|r| r.size_bytes)
                .sum();
            let data = json!({
                "safe_reclaim_bytes": safe,
                "likely_delete_paths": rows.iter().take(50).map(|r| &r.path).collect::<Vec<_>>(),
                "review": "Paths not auto-flagged may still be duplicates; confirm in TUI before deleting.",
                "danger": "Do not delete arbitrary paths; prefer LIKELY_DELETE flagged copies after review.",
            });
            serde_json::to_string(&wrap(snap, data)).unwrap_or_default()
        }
        Err(e) => serde_json::to_string(&wrap(snap, json!({ "error": e }))).unwrap_or_default(),
    }
}

/// Returns Ok(paths_trashed) after moving to Trash and updating the index.
pub fn trash_paths_mcp(paths: &[String], db_path: Option<PathBuf>) -> Result<usize, String> {
    if paths.is_empty() {
        return Err("paths array is empty".into());
    }
    let dbp = db_path.unwrap_or_else(default_disk_index_path);
    let root_meta = if dbp.exists() {
        let conn = store::open_read(&dbp).map_err(|e| e.to_string())?;
        store::get_meta(&conn, store::META_INDEXED_ROOT)
            .map_err(|e| e.to_string())?
            .unwrap_or_default()
    } else {
        String::new()
    };
    if !root_meta.is_empty() {
        let root_pb = PathBuf::from(&root_meta);
        for p in paths {
            let pb = Path::new(p);
            if !pb.starts_with(&root_pb) {
                return Err(format!("path outside indexed root {}: {}", root_meta, p));
            }
        }
    }
    for p in paths {
        trash::delete(Path::new(p)).map_err(|e| e.to_string())?;
    }
    if dbp.exists() {
        if let Ok(conn) = store::open_write(&dbp) {
            let _ = store::remove_paths(&conn, paths);
        }
    }
    Ok(paths.len())
}
