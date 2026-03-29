//! SQLite WAL store for disk index — single writer, concurrent readers.

use crate::disk::model::FileMeta;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

pub const SCHEMA_VERSION: i32 = 2;

pub const META_INDEXED_ROOT: &str = "indexed_root";
pub const META_LAST_COMPLETED_EPOCH: &str = "last_completed_epoch";

/// Normalize a path for `entries.parent_path` equality (matches `Walk` / `Path::parent()` output).
/// Root `/` stays `/` so it does not become empty after trimming.
pub fn path_key_for_parent_query(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    if s.as_ref() == "/" {
        return "/".to_string();
    }
    s.trim_end_matches('/').to_string()
}

/// Normalize a stored path string the same way as [`path_key_for_parent_query`].
pub fn normalize_path_key_str(s: &str) -> String {
    if s == "/" {
        return "/".to_string();
    }
    s.trim_end_matches('/').to_string()
}

pub fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    let v: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()?;
    Ok(v)
}

pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

/// Remove index rows not touched during this scan epoch (deleted/moved files).
pub fn delete_stale_entries(conn: &Connection, epoch: i64) -> Result<u64> {
    let n = conn.execute("DELETE FROM entries WHERE scan_epoch < ?1", [epoch])?;
    Ok(n as u64)
}

pub fn apply_pragma(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA temp_store = MEMORY;
        PRAGMA foreign_keys = ON;
        ",
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS duplicate_groups (
            id INTEGER PRIMARY KEY,
            full_hash BLOB NOT NULL,
            size_bytes INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_dup_hash ON duplicate_groups(full_hash);

        CREATE TABLE IF NOT EXISTS entries (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            parent_path TEXT,
            name TEXT NOT NULL,
            is_dir INTEGER NOT NULL,
            size_bytes INTEGER NOT NULL,
            mtime_ms INTEGER,
            ext TEXT,
            inode INTEGER,
            dev INTEGER,
            quick_hash BLOB,
            full_hash BLOB,
            normalized_stem TEXT,
            copy_index INTEGER,
            dup_group_id INTEGER,
            flags INTEGER NOT NULL DEFAULT 0,
            scan_epoch INTEGER NOT NULL,
            FOREIGN KEY (dup_group_id) REFERENCES duplicate_groups(id)
        );

        CREATE INDEX IF NOT EXISTS idx_entries_parent ON entries(parent_path);
        CREATE INDEX IF NOT EXISTS idx_entries_size ON entries(size_bytes) WHERE is_dir = 0;
        CREATE INDEX IF NOT EXISTS idx_entries_full_hash ON entries(full_hash)
            WHERE full_hash IS NOT NULL AND is_dir = 0;
        ",
    )?;

    let ver: i32 = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);

    if ver < SCHEMA_VERSION {
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
    }

    Ok(())
}

pub fn open_write(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("open sqlite {}", path.display()))?;
    apply_pragma(&conn)?;
    init_schema(&conn)?;
    Ok(conn)
}

pub fn open_read(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open sqlite read-only {}", path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(conn)
}

pub fn clear_entries(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM entries", [])?;
    conn.execute("DELETE FROM duplicate_groups", [])?;
    Ok(())
}

/// Upsert scan rows; sets `HASH_DIRTY` when a file is new or size/mtime/inode/dev changed.
pub fn insert_batch(conn: &mut Connection, epoch: i64, batch: &[FileMeta]) -> Result<()> {
    use crate::disk::model::EntryFlags;
    let dup_bits = EntryFlags::LIKELY_DELETE | EntryFlags::KEEP_WINNER;
    let dirty = EntryFlags::HASH_DIRTY;

    let tx = conn.transaction()?;

    {
        let mut sel = tx.prepare_cached(
            "SELECT size_bytes, mtime_ms, inode, dev, flags FROM entries WHERE path = ?1",
        )?;
        let mut ins = tx.prepare_cached(
            "INSERT INTO entries (path, parent_path, name, is_dir, size_bytes, mtime_ms, ext, inode, dev, normalized_stem, copy_index, flags, scan_epoch)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )?;
        let mut upd = tx.prepare_cached(
            "UPDATE entries SET parent_path = ?1, name = ?2, is_dir = ?3, size_bytes = ?4, mtime_ms = ?5,
         ext = ?6, inode = ?7, dev = ?8, normalized_stem = ?9, copy_index = ?10, flags = ?11, scan_epoch = ?12
         WHERE path = ?13",
        )?;

        for m in batch {
            let path = m.path.to_string_lossy().into_owned();
            let parent = m
                .parent_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned());

            let old = sel
                .query_row([&path], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                        r.get::<_, i32>(4)?,
                    ))
                })
                .optional()?;

            let (flags_out, is_insert) = match old {
                None => {
                    let base = m.flags;
                    let f = if m.is_dir { base } else { base | dirty };
                    (f, true)
                }
                Some((sz, mt, ino, dev, old_flags)) => {
                    let ino_m = m.inode.map(|i| i as i64);
                    let dev_m = m.dev.map(|d| d as i64);
                    let changed = sz != m.size_bytes as i64
                        || mt != m.mtime_ms
                        || ino != ino_m
                        || dev != dev_m;
                    let base = m.flags & !dup_bits;
                    let preserved_dup = if changed {
                        0u32
                    } else {
                        old_flags as u32 & dup_bits
                    };
                    let f = if m.is_dir {
                        base | preserved_dup
                    } else if changed {
                        base | preserved_dup | dirty
                    } else {
                        (base | preserved_dup) & !dirty
                    };
                    (f, false)
                }
            };

            if is_insert {
                ins.execute(params![
                    &path,
                    parent,
                    &m.name,
                    if m.is_dir { 1i32 } else { 0i32 },
                    m.size_bytes as i64,
                    m.mtime_ms,
                    &m.ext,
                    m.inode.map(|i| i as i64),
                    m.dev.map(|d| d as i64),
                    &m.normalized_stem,
                    m.copy_index,
                    flags_out as i32,
                    epoch,
                ])?;
            } else {
                upd.execute(params![
                    parent,
                    &m.name,
                    if m.is_dir { 1i32 } else { 0i32 },
                    m.size_bytes as i64,
                    m.mtime_ms,
                    &m.ext,
                    m.inode.map(|i| i as i64),
                    m.dev.map(|d| d as i64),
                    &m.normalized_stem,
                    m.copy_index,
                    flags_out as i32,
                    epoch,
                    &path,
                ])?;
            }
        }
    }

    tx.commit()?;
    Ok(())
}

/// Propagate file sizes up directory tree (dirs may have been inserted with size 0).
pub fn aggregate_directory_sizes(conn: &Connection) -> Result<()> {
    aggregate_directory_sizes_with_progress(conn, |_| {})
}

/// Like [`aggregate_directory_sizes`], but invokes `on_pass` with 1-based pass count after each SQL pass that updates rows.
pub fn aggregate_directory_sizes_with_progress<F>(conn: &Connection, mut on_pass: F) -> Result<()>
where
    F: FnMut(u32),
{
    // Multiple passes until stable (deepest dirs first eventually propagate to root).
    let mut pass: u32 = 0;
    loop {
        let n = conn.execute(
            "UPDATE entries AS d
             SET size_bytes = (
               SELECT COALESCE(SUM(c.size_bytes), 0) FROM entries c WHERE c.parent_path = d.path
             )
             WHERE d.is_dir = 1
               AND size_bytes != (
                 SELECT COALESCE(SUM(c.size_bytes), 0) FROM entries c WHERE c.parent_path = d.path
               )",
            [],
        )?;
        if n == 0 {
            break;
        }
        pass = pass.saturating_add(1);
        on_pass(pass);
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct EntryRowDb {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub mtime_ms: Option<i64>,
    pub ext: String,
    pub flags: u32,
    pub full_hash: Option<Vec<u8>>,
    pub dup_group_id: Option<i64>,
}

pub fn query_children(conn: &Connection, parent_path: &str) -> Result<Vec<EntryRowDb>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, path, name, is_dir, size_bytes, mtime_ms, ext, flags, full_hash, dup_group_id
         FROM entries
         WHERE parent_path IS NOT NULL AND parent_path = ?1
         ORDER BY is_dir DESC, size_bytes DESC, name ASC",
    )?;

    let rows = stmt
        .query_map([parent_path], |r| {
            let hash: Option<Vec<u8>> = r.get(8)?;
            Ok(EntryRowDb {
                id: r.get(0)?,
                path: r.get(1)?,
                name: r.get(2)?,
                is_dir: r.get::<_, i32>(3)? != 0,
                size_bytes: r.get::<_, i64>(4)? as u64,
                mtime_ms: r.get(5)?,
                ext: r.get(6)?,
                flags: r.get::<_, i32>(7)? as u32,
                full_hash: hash,
                dup_group_id: r.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn query_root_children(conn: &Connection, root: &str) -> Result<Vec<EntryRowDb>> {
    let root_norm = normalize_path_key_str(root);
    query_children(conn, &root_norm)
}

/// Files only, same size, size > 0 — hash dedupe candidates.
pub fn query_size_collision_candidates(conn: &Connection) -> Result<Vec<(u64, Vec<String>)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT size_bytes, path FROM entries
         WHERE is_dir = 0 AND size_bytes > 0
         ORDER BY size_bytes, path",
    )?;

    let mut map: std::collections::BTreeMap<u64, Vec<String>> = std::collections::BTreeMap::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let size: i64 = row.get(0)?;
        let path: String = row.get(1)?;
        map.entry(size as u64).or_default().push(path);
    }

    Ok(map
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .collect())
}

#[derive(Debug, Clone)]
pub struct FilePathMeta {
    pub path: String,
    pub inode: Option<i64>,
    pub dev: Option<i64>,
}

/// Size-collision candidates with inode/dev for hard-link collapse.
pub fn query_size_collision_with_meta(conn: &Connection) -> Result<Vec<(u64, Vec<FilePathMeta>)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT size_bytes, path, inode, dev FROM entries
         WHERE is_dir = 0 AND size_bytes > 0
         ORDER BY size_bytes, path",
    )?;

    let mut map: std::collections::BTreeMap<u64, Vec<FilePathMeta>> =
        std::collections::BTreeMap::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let size: i64 = row.get(0)?;
        let path: String = row.get(1)?;
        let inode: Option<i64> = row.get(2)?;
        let dev: Option<i64> = row.get(3)?;
        map.entry(size as u64)
            .or_default()
            .push(FilePathMeta { path, inode, dev });
    }

    Ok(map
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .collect())
}

pub fn fetch_normalized_stems(
    conn: &Connection,
    paths: &[String],
) -> Result<std::collections::HashMap<String, (String, String)>> {
    let mut out = std::collections::HashMap::new();
    if paths.is_empty() {
        return Ok(out);
    }
    let mut stmt =
        conn.prepare_cached("SELECT path, normalized_stem, ext FROM entries WHERE path = ?1")?;
    for p in paths {
        if let Ok((stem, ext)) = stmt.query_row([p.as_str()], |r| {
            Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        }) {
            out.insert(p.clone(), (stem, ext));
        }
    }
    Ok(out)
}

pub fn update_full_hash(conn: &Connection, path: &str, hash: &[u8; 32]) -> Result<()> {
    use crate::disk::model::EntryFlags;
    let d = EntryFlags::HASH_DIRTY as i64;
    conn.execute(
        "UPDATE entries SET full_hash = ?1, quick_hash = ?1, flags = (CAST(flags AS INTEGER) & ~(?3)) WHERE path = ?2",
        params![hash.as_slice(), path, d],
    )?;
    Ok(())
}

pub fn clear_dup_metadata(conn: &Connection) -> Result<()> {
    use crate::disk::model::EntryFlags;
    conn.execute("UPDATE entries SET dup_group_id = NULL", [])?;
    let mask = !(EntryFlags::LIKELY_DELETE | EntryFlags::KEEP_WINNER);
    conn.execute("UPDATE entries SET flags = flags & ?1", [mask as i64])?;
    conn.execute("DELETE FROM duplicate_groups", [])?;
    Ok(())
}

/// Any path in the set still marked `HASH_DIRTY` (needs duplicate re-check).
pub fn bucket_has_hash_dirty(conn: &Connection, paths: &[String]) -> Result<bool> {
    use crate::disk::model::EntryFlags;
    if paths.is_empty() {
        return Ok(false);
    }
    let mut stmt = conn.prepare_cached(
        "SELECT 1 FROM entries WHERE path = ?1 AND (CAST(flags AS INTEGER) & ?2) != 0 LIMIT 1",
    )?;
    let bit = EntryFlags::HASH_DIRTY as i64;
    for p in paths {
        let y: Option<i32> = stmt.query_row(params![p, bit], |_| Ok(1)).optional()?;
        if y.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Clear duplicate group + dup flags for paths in a size bucket before rehashing (warm path).
pub fn clear_dup_flags_for_bucket_paths(conn: &Connection, paths: &[String]) -> Result<()> {
    use crate::disk::model::EntryFlags;
    let dup = (EntryFlags::LIKELY_DELETE | EntryFlags::KEEP_WINNER) as i64;
    let mut stmt = conn.prepare_cached(
        "UPDATE entries SET dup_group_id = NULL, flags = (CAST(flags AS INTEGER) & ~(?2)) WHERE path = ?1",
    )?;
    for p in paths {
        stmt.execute(params![p, dup])?;
    }
    Ok(())
}

/// Remove dup metadata when only one file remains at that size (partner deleted/moved).
pub fn clear_orphan_dup_metadata(conn: &Connection) -> Result<u64> {
    use crate::disk::model::EntryFlags;
    let dup = (EntryFlags::LIKELY_DELETE | EntryFlags::KEEP_WINNER) as i64;
    let n = conn.execute(
        "UPDATE entries SET dup_group_id = NULL,
         flags = (CAST(flags AS INTEGER) & ~(?1))
         WHERE is_dir = 0
           AND dup_group_id IS NOT NULL
           AND (SELECT COUNT(*) FROM entries e2 WHERE e2.is_dir = 0 AND e2.size_bytes = entries.size_bytes) < 2",
        [dup],
    )?;
    Ok(n as u64)
}

/// Clear `HASH_DIRTY` for files that are alone at their size (no duplicate candidate bucket).
pub fn clear_standalone_hash_dirty(conn: &Connection) -> Result<u64> {
    use crate::disk::model::EntryFlags;
    let d = EntryFlags::HASH_DIRTY as i64;
    let n = conn.execute(
        "UPDATE entries SET flags = (CAST(flags AS INTEGER) & ~(?1))
         WHERE is_dir = 0
           AND (CAST(flags AS INTEGER) & ?1) != 0
           AND (SELECT COUNT(*) FROM entries e2 WHERE e2.is_dir = 0 AND e2.size_bytes = entries.size_bytes AND e2.size_bytes > 0) < 2",
        [d],
    )?;
    Ok(n as u64)
}

/// One duplicate content group: all paths share `full_hash`. `likely_delete[i]` only for non-keepers.
pub struct DuplicateApplyGroup {
    pub paths: Vec<String>,
    pub hash: [u8; 32],
    pub size_bytes: u64,
    pub keep_index: usize,
    /// Same length as paths; only consulted when index != keep_index.
    pub likely_delete: Vec<bool>,
}

/// Apply duplicate groups: insert groups, update entries flags and dup_group_id.
pub fn apply_duplicate_results(
    conn: &mut Connection,
    groups: &[DuplicateApplyGroup],
) -> Result<()> {
    let tx = conn.transaction()?;
    for g in groups {
        if g.paths.len() < 2 {
            continue;
        }
        tx.execute(
            "INSERT INTO duplicate_groups (full_hash, size_bytes) VALUES (?1, ?2)",
            params![g.hash.as_slice(), g.size_bytes as i64],
        )?;
        let gid: i64 = tx.last_insert_rowid();

        use crate::disk::model::EntryFlags;
        let mask = !(EntryFlags::LIKELY_DELETE | EntryFlags::KEEP_WINNER);
        for (i, p) in g.paths.iter().enumerate() {
            let keep = i == g.keep_index;
            let flags_on = if keep {
                EntryFlags::KEEP_WINNER
            } else if g.likely_delete.get(i).copied().unwrap_or(false) {
                EntryFlags::LIKELY_DELETE
            } else {
                0
            };
            tx.execute(
                "UPDATE entries SET dup_group_id = ?1,
                 flags = (flags & ?2) | ?3,
                 full_hash = ?4
                 WHERE path = ?5",
                params![gid, mask as i32, flags_on as i32, g.hash.as_slice(), p],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn reclaimable_bytes(conn: &Connection) -> Result<u64> {
    use crate::disk::model::EntryFlags;
    let n: i64 = conn.query_row(
        "SELECT COALESCE(SUM(size_bytes), 0) FROM entries WHERE CAST(flags AS INTEGER) & ?1 != 0 AND is_dir = 0",
        [EntryFlags::LIKELY_DELETE as i64],
        |r| r.get(0),
    )?;
    Ok(n as u64)
}

pub fn total_file_bytes(conn: &Connection) -> Result<u64> {
    let n: i64 = conn.query_row(
        "SELECT COALESCE(SUM(size_bytes), 0) FROM entries WHERE is_dir = 0",
        [],
        |r| r.get(0),
    )?;
    Ok(n as u64)
}

pub fn count_entries(conn: &Connection) -> Result<(u64, u64)> {
    let files: i64 = conn.query_row("SELECT COUNT(*) FROM entries WHERE is_dir = 0", [], |r| {
        r.get(0)
    })?;
    let dirs: i64 = conn.query_row("SELECT COUNT(*) FROM entries WHERE is_dir = 1", [], |r| {
        r.get(0)
    })?;
    Ok((files as u64, dirs as u64))
}

pub fn remove_paths(conn: &Connection, paths: &[String]) -> Result<()> {
    let mut stmt = conn.prepare_cached("DELETE FROM entries WHERE path = ?1")?;
    for p in paths {
        stmt.execute([p])?;
    }
    Ok(())
}

/// Distinct duplicate groups (for MCP / summary).
pub fn count_dup_groups(conn: &Connection) -> Result<u64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT dup_group_id) FROM entries WHERE dup_group_id IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    Ok(n as u64)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct McpDupRow {
    pub path: String,
    pub size_bytes: u64,
    pub likely_delete: bool,
    pub dup_group_id: i64,
}

/// Paginated duplicate file rows for MCP tools.
pub fn mcp_duplicate_rows(
    conn: &Connection,
    limit: u64,
    min_size: u64,
    only_reclaim: bool,
) -> Result<Vec<McpDupRow>> {
    use crate::disk::model::EntryFlags;
    let lim = limit.clamp(1, 500) as i64;
    let min = min_size as i64;
    let ld = EntryFlags::LIKELY_DELETE as i64;
    let mut rows = Vec::new();
    if only_reclaim {
        let mut stmt = conn.prepare_cached(
            "SELECT path, size_bytes, flags, dup_group_id FROM entries
             WHERE is_dir = 0 AND dup_group_id IS NOT NULL
             AND (CAST(flags AS INTEGER) & ?1) != 0 AND size_bytes >= ?2
             ORDER BY size_bytes DESC LIMIT ?3",
        )?;
        let iter = stmt.query_map(params![ld, min, lim], |r| {
            let flags: i32 = r.get(2)?;
            Ok(McpDupRow {
                path: r.get(0)?,
                size_bytes: r.get::<_, i64>(1)? as u64,
                likely_delete: (flags as u32) & EntryFlags::LIKELY_DELETE != 0,
                dup_group_id: r.get(3)?,
            })
        })?;
        for x in iter {
            rows.push(x?);
        }
    } else {
        let mut stmt = conn.prepare_cached(
            "SELECT path, size_bytes, flags, dup_group_id FROM entries
             WHERE is_dir = 0 AND dup_group_id IS NOT NULL AND size_bytes >= ?1
             ORDER BY size_bytes DESC LIMIT ?2",
        )?;
        let iter = stmt.query_map(params![min, lim], |r| {
            let flags: i32 = r.get(2)?;
            Ok(McpDupRow {
                path: r.get(0)?,
                size_bytes: r.get::<_, i64>(1)? as u64,
                likely_delete: (flags as u32) & EntryFlags::LIKELY_DELETE != 0,
                dup_group_id: r.get(3)?,
            })
        })?;
        for x in iter {
            rows.push(x?);
        }
    }
    Ok(rows)
}

/// One duplicate group summary for Disk inspector “quick wins”.
#[derive(Debug, Clone)]
pub struct DupGroupQuickRow {
    pub dup_group_id: i64,
    pub reclaim_bytes: u64,
    pub member_count: u64,
    pub total_bytes: u64,
    pub preview_name: String,
}

/// Top duplicate groups by `SUM(LIKELY_DELETE sizes)` then total size.
pub fn list_dup_groups_quick(conn: &Connection, limit: u64) -> Result<Vec<DupGroupQuickRow>> {
    use crate::disk::model::EntryFlags;
    let ld = EntryFlags::LIKELY_DELETE as i64;
    let kw = EntryFlags::KEEP_WINNER as i64;
    let lim = limit.clamp(1, 200) as i64;
    let mut stmt = conn.prepare_cached(
        "SELECT g.dup_group_id, g.reclaim_sum, g.member_count, g.total_bytes,
                COALESCE(
                  (SELECT k.name FROM entries k
                   WHERE k.dup_group_id = g.dup_group_id
                     AND (CAST(k.flags AS INTEGER) & ?2) != 0
                   ORDER BY k.path LIMIT 1),
                  (SELECT k2.name FROM entries k2
                   WHERE k2.dup_group_id = g.dup_group_id
                   ORDER BY k2.path LIMIT 1)
                ) AS preview_name
         FROM (
           SELECT dup_group_id,
                  SUM(CASE WHEN (CAST(flags AS INTEGER) & ?1) != 0
                      THEN CAST(size_bytes AS INTEGER) ELSE 0 END) AS reclaim_sum,
                  COUNT(*) AS member_count,
                  SUM(size_bytes) AS total_bytes
           FROM entries
           WHERE is_dir = 0 AND dup_group_id IS NOT NULL
           GROUP BY dup_group_id
           HAVING COUNT(*) >= 2
         ) AS g
         ORDER BY g.reclaim_sum DESC, g.total_bytes DESC
         LIMIT ?3",
    )?;
    let iter = stmt.query_map(params![ld, kw, lim], |r| {
        let preview: Option<String> = r.get(4)?;
        Ok(DupGroupQuickRow {
            dup_group_id: r.get(0)?,
            reclaim_bytes: r.get::<_, i64>(1)? as u64,
            member_count: r.get::<_, i64>(2)? as u64,
            total_bytes: r.get::<_, i64>(3)? as u64,
            preview_name: preview.unwrap_or_default(),
        })
    })?;
    let mut out = Vec::new();
    for x in iter {
        out.push(x?);
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct DupGroupMember {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub likely_delete: bool,
    pub keep_winner: bool,
}

pub fn load_dup_group_members(conn: &Connection, dup_group_id: i64) -> Result<Vec<DupGroupMember>> {
    use crate::disk::model::EntryFlags;
    let mut stmt = conn.prepare_cached(
        "SELECT path, name, size_bytes, flags FROM entries
         WHERE dup_group_id = ?1 AND is_dir = 0
         ORDER BY path ASC",
    )?;
    let iter = stmt.query_map([dup_group_id], |r| {
        let flags: i32 = r.get(3)?;
        let f = EntryFlags(flags as u32);
        Ok(DupGroupMember {
            path: r.get(0)?,
            name: r.get(1)?,
            size_bytes: r.get::<_, i64>(2)? as u64,
            likely_delete: f.contains(EntryFlags::LIKELY_DELETE),
            keep_winner: f.contains(EntryFlags::KEEP_WINNER),
        })
    })?;
    let mut out = Vec::new();
    for x in iter {
        out.push(x?);
    }
    Ok(out)
}

/// User picked `keeper_path` as the only keeper; all other files in the group get `LIKELY_DELETE`.
pub fn apply_dup_group_keeper(
    conn: &mut Connection,
    dup_group_id: i64,
    keeper_path: &str,
) -> Result<()> {
    use crate::disk::model::EntryFlags;
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entries WHERE dup_group_id = ?1 AND path = ?2 AND is_dir = 0",
        params![dup_group_id, keeper_path],
        |r| r.get(0),
    )?;
    anyhow::ensure!(n == 1, "keeper path not in duplicate group");
    let tx = conn.transaction()?;
    let mask = !(EntryFlags::LIKELY_DELETE | EntryFlags::KEEP_WINNER);
    let mask_i = mask as i32;
    let ld = EntryFlags::LIKELY_DELETE as i32;
    let kw = EntryFlags::KEEP_WINNER as i32;
    tx.execute(
        "UPDATE entries SET flags = (CAST(flags AS INTEGER) & ?1) | ?2
         WHERE dup_group_id = ?3 AND path = ?4 AND is_dir = 0",
        params![mask_i, kw, dup_group_id, keeper_path],
    )?;
    tx.execute(
        "UPDATE entries SET flags = (CAST(flags AS INTEGER) & ?1) | ?2
         WHERE dup_group_id = ?3 AND path != ?4 AND is_dir = 0",
        params![mask_i, ld, dup_group_id, keeper_path],
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::model::FileMeta;
    use std::path::{Path, PathBuf};

    #[test]
    fn query_children_matches_canonical_parent_path_key() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join("f.txt"), b"x").unwrap();
        let db = dir.path().join("canonical.sqlite");
        let mut conn = open_write(&db).unwrap();
        let key = path_key_for_parent_query(&root);
        let batch = vec![FileMeta {
            path: root.join("f.txt"),
            parent_path: Some(root.clone()),
            name: "f.txt".into(),
            is_dir: false,
            size_bytes: 1,
            mtime_ms: Some(1),
            ext: "txt".into(),
            inode: None,
            dev: None,
            flags: 0,
            normalized_stem: "f".into(),
            copy_index: None,
        }];
        insert_batch(&mut conn, 1, &batch).unwrap();
        let kids = query_children(&conn, &key).unwrap();
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].name, "f.txt");

        // If the temp path differs from its canonical form (e.g. macOS /tmp vs /private/tmp),
        // querying with the non-canonical parent key must not match DB parent_path strings.
        let alias_key = path_key_for_parent_query(dir.path());
        if alias_key != key {
            let alias_kids = query_children(&conn, &alias_key).unwrap();
            assert!(
                alias_kids.is_empty(),
                "non-canonical parent key should not match rows keyed by canonical parent_path"
            );
        }
    }

    #[test]
    fn path_key_trims_trailing_slash_keeps_root() {
        assert_eq!(
            path_key_for_parent_query(Path::new("/Users/x/Downloads/")),
            "/Users/x/Downloads"
        );
        assert_eq!(path_key_for_parent_query(Path::new("/")), "/");
        assert_eq!(normalize_path_key_str("/foo/bar/"), "/foo/bar");
        assert_eq!(normalize_path_key_str("/"), "/");
    }

    #[test]
    fn insert_batch_and_query_children() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.sqlite");
        let mut conn = open_write(&db).unwrap();
        let parent = "/tmp/root";
        let batch = vec![
            FileMeta {
                path: PathBuf::from("/tmp/root/a.txt"),
                parent_path: Some(PathBuf::from(parent)),
                name: "a.txt".into(),
                is_dir: false,
                size_bytes: 10,
                mtime_ms: Some(1),
                ext: "txt".into(),
                inode: None,
                dev: None,
                flags: 0,
                normalized_stem: "a".into(),
                copy_index: None,
            },
            FileMeta {
                path: PathBuf::from("/tmp/root/sub"),
                parent_path: Some(PathBuf::from(parent)),
                name: "sub".into(),
                is_dir: true,
                size_bytes: 0,
                mtime_ms: None,
                ext: String::new(),
                inode: None,
                dev: None,
                flags: 0,
                normalized_stem: "sub".into(),
                copy_index: None,
            },
        ];
        insert_batch(&mut conn, 1, &batch).unwrap();
        let kids = query_children(&conn, parent).unwrap();
        assert_eq!(kids.len(), 2);
        aggregate_directory_sizes(&conn).unwrap();
    }

    #[test]
    fn insert_batch_sets_then_clears_hash_dirty_when_unchanged() {
        use crate::disk::model::EntryFlags;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("dirty.sqlite");
        let mut conn = open_write(&db).unwrap();
        let parent = "/tmp/p";
        let p = PathBuf::from("/tmp/p/f.txt");
        let mut f = |sz, mt| FileMeta {
            path: p.clone(),
            parent_path: Some(PathBuf::from(parent)),
            name: "f.txt".into(),
            is_dir: false,
            size_bytes: sz,
            mtime_ms: Some(mt),
            ext: "txt".into(),
            inode: Some(99),
            dev: Some(1),
            flags: 0,
            normalized_stem: "f".into(),
            copy_index: None,
        };
        insert_batch(&mut conn, 1, &[f(5, 100)]).unwrap();
        let flags1: i32 = conn
            .query_row(
                "SELECT flags FROM entries WHERE path = ?",
                [&p.to_string_lossy()],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(
            flags1 as u32 & EntryFlags::HASH_DIRTY,
            0,
            "new file should be hash-dirty"
        );

        insert_batch(&mut conn, 2, &[f(5, 100)]).unwrap();
        let flags2: i32 = conn
            .query_row(
                "SELECT flags FROM entries WHERE path = ?",
                [&p.to_string_lossy()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            flags2 as u32 & EntryFlags::HASH_DIRTY,
            0,
            "unchanged metadata clears hash-dirty"
        );

        insert_batch(&mut conn, 3, &[f(6, 100)]).unwrap();
        let flags3: i32 = conn
            .query_row(
                "SELECT flags FROM entries WHERE path = ?",
                [&p.to_string_lossy()],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(
            flags3 as u32 & EntryFlags::HASH_DIRTY,
            0,
            "size change sets dirty"
        );
    }

    #[test]
    fn apply_dup_group_keeper_sets_one_keep_rest_likely_delete() {
        use crate::disk::model::EntryFlags;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("keeper.sqlite");
        let mut conn = open_write(&db).unwrap();
        let h = [7u8; 32];
        conn.execute(
            "INSERT INTO duplicate_groups (full_hash, size_bytes) VALUES (?1, 100)",
            params![&h[..]],
        )
        .unwrap();
        let gid = conn.last_insert_rowid();
        let ld = EntryFlags::LIKELY_DELETE as i32;
        let kw = EntryFlags::KEEP_WINNER as i32;
        for (path, flags) in [("/d/a.txt", kw), ("/d/b.txt", ld), ("/d/c.txt", ld)] {
            conn.execute(
                "INSERT INTO entries (path, parent_path, name, is_dir, size_bytes, mtime_ms, ext,
                 inode, dev, normalized_stem, copy_index, flags, scan_epoch, dup_group_id)
                 VALUES (?1, '/d', 'x', 0, 100, 0, 'txt', NULL, NULL, 'x', NULL, ?2, 1, ?3)",
                params![path, flags, gid],
            )
            .unwrap();
        }
        apply_dup_group_keeper(&mut conn, gid, "/d/c.txt").unwrap();
        let fa: i32 = conn
            .query_row(
                "SELECT flags FROM entries WHERE path = '/d/a.txt'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let fb: i32 = conn
            .query_row(
                "SELECT flags FROM entries WHERE path = '/d/b.txt'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let fc: i32 = conn
            .query_row(
                "SELECT flags FROM entries WHERE path = '/d/c.txt'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(EntryFlags(fa as u32).contains(EntryFlags::LIKELY_DELETE));
        assert!(EntryFlags(fb as u32).contains(EntryFlags::LIKELY_DELETE));
        assert!(EntryFlags(fc as u32).contains(EntryFlags::KEEP_WINNER));
        assert!(!EntryFlags(fc as u32).contains(EntryFlags::LIKELY_DELETE));
    }

    #[test]
    fn list_dup_groups_quick_orders_by_reclaim_sum() {
        use crate::disk::model::EntryFlags;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("quick.sqlite");
        let mut conn = open_write(&db).unwrap();
        let h = [1u8; 32];
        conn.execute(
            "INSERT INTO duplicate_groups (full_hash, size_bytes) VALUES (?1, 10)",
            params![&h[..]],
        )
        .unwrap();
        let g1 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO duplicate_groups (full_hash, size_bytes) VALUES (?1, 20)",
            params![&h[..]],
        )
        .unwrap();
        let g2 = conn.last_insert_rowid();
        let ld = EntryFlags::LIKELY_DELETE as i32;
        let ins = |path: &str, sz: i64, gid: i64, flags: i32| {
            conn.execute(
                "INSERT INTO entries (path, parent_path, name, is_dir, size_bytes, mtime_ms, ext,
                 inode, dev, normalized_stem, copy_index, flags, scan_epoch, dup_group_id)
                 VALUES (?1, '/p', 'f', 0, ?2, 0, 'txt', NULL, NULL, 'f', NULL, ?3, 1, ?4)",
                params![path, sz, flags, gid],
            )
            .unwrap();
        };
        ins("/p/small_a", 10, g1, 0);
        ins("/p/small_b", 10, g1, 0);
        ins("/p/big_a", 100, g2, ld);
        ins("/p/big_b", 100, g2, ld);
        let rows = list_dup_groups_quick(&conn, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].dup_group_id, g2);
        assert_eq!(rows[0].reclaim_bytes, 200);
        assert_eq!(rows[1].dup_group_id, g1);
        assert_eq!(rows[1].reclaim_bytes, 0);
    }
}
