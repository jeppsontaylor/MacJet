//! Staged BLAKE3 hashing for duplicate detection (parallel full-hash, inode collapse, gated flags).

use crate::disk::names::normalize_filename;
use crate::disk::store::{self, DuplicateApplyGroup, EntryRowDb, FilePathMeta};
use anyhow::Result;
use blake3::Hasher;
use rayon::prelude::*;
use std::cmp::min;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const PREFIX_SUFFIX: usize = 64 * 1024;
const SMALL_FILE_MAX: u64 = 128 * 1024;

pub fn sample_hash_file(path: &Path, size: u64) -> std::io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hasher = Hasher::new();

    let prefix_len = min(PREFIX_SUFFIX as u64, size) as usize;
    let mut prefix = vec![0u8; prefix_len];
    file.read_exact(&mut prefix)?;
    hasher.update(&prefix);

    if size > PREFIX_SUFFIX as u64 {
        let suffix_len = min(PREFIX_SUFFIX as u64, size) as usize;
        file.seek(SeekFrom::End(-(suffix_len as i64)))?;
        let mut suffix = vec![0u8; suffix_len];
        file.read_exact(&mut suffix)?;
        hasher.update(&suffix);
    }

    hasher.update(&size.to_le_bytes());
    Ok(*hasher.finalize().as_bytes())
}

pub fn full_hash_file(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hasher = Hasher::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(*hasher.finalize().as_bytes())
}

/// Group paths by (dev, inode); each group shares one content hash. Returns representative paths and expansion map.
fn inode_representatives(metas: &[FilePathMeta]) -> (Vec<String>, HashMap<String, Vec<String>>) {
    let mut by_id: HashMap<(i64, i64), Vec<String>> = HashMap::new();
    let mut singles: Vec<String> = Vec::new();

    for m in metas {
        if let (Some(ino), Some(dev)) = (m.inode, m.dev) {
            by_id.entry((dev, ino)).or_default().push(m.path.clone());
        } else {
            singles.push(m.path.clone());
        }
    }

    let mut rep_to_all: HashMap<String, Vec<String>> = HashMap::new();
    let mut reps = Vec::new();

    for mut paths in by_id.into_values() {
        paths.sort();
        let rep = paths[0].clone();
        rep_to_all.insert(rep.clone(), paths.clone());
        reps.push(rep);
    }

    for p in singles {
        rep_to_all.insert(p.clone(), vec![p.clone()]);
        reps.push(p);
    }

    reps.sort();
    (reps, rep_to_all)
}

fn parallel_full_hashes(paths: &[String]) -> Vec<(String, std::io::Result<[u8; 32]>)> {
    paths
        .par_iter()
        .map(|p| (p.clone(), full_hash_file(Path::new(p))))
        .collect()
}

fn parallel_sample_hashes(paths: &[String], size: u64) -> Vec<(String, std::io::Result<[u8; 32]>)> {
    paths
        .par_iter()
        .map(|p| (p.clone(), sample_hash_file(Path::new(p), size)))
        .collect()
}

/// Gate `LIKELY_DELETE`: same normalized stem as keeper and (copy suffix on loser OR longer name).
fn build_likely_delete_flags(
    paths: &[String],
    keep_idx: usize,
    stem_map: &HashMap<String, (String, String)>,
) -> Vec<bool> {
    let keeper_path = &paths[keep_idx];
    let keeper_name = Path::new(keeper_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let keeper_norm = normalize_filename(Path::new(keeper_name).as_os_str());
    let keeper_stem = stem_map
        .get(keeper_path)
        .map(|(s, _)| s.as_str())
        .unwrap_or(keeper_norm.normalized_stem.as_str());

    paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i == keep_idx {
                return false;
            }
            let name = Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            let n = normalize_filename(Path::new(name).as_os_str());
            let stem = stem_map
                .get(p)
                .map(|(s, _)| s.as_str())
                .unwrap_or(n.normalized_stem.as_str());
            if stem != keeper_stem {
                return false;
            }
            n.has_copy_suffix || name.len() > keeper_name.len()
        })
        .collect()
}

pub fn run_hash_and_dedupe(
    writer: &mut rusqlite::Connection,
    ui_tx: &crossbeam_channel::Sender<crate::disk::model::DiskUiEvent>,
    cold_dup_pass: bool,
) -> Result<()> {
    use crate::disk::model::DiskUiEvent;

    if cold_dup_pass {
        let _ = store::clear_dup_metadata(writer)?;
    } else {
        let n = store::clear_orphan_dup_metadata(writer)?;
        if n > 0 {
            let _ = ui_tx.send(DiskUiEvent::Log(format!(
                "Cleared dup metadata on {n} orphan-sized file(s)"
            )));
        }
    }

    let candidates = store::query_size_collision_with_meta(writer)?;
    let total_groups: u64 = candidates.len() as u64;
    let mut done_groups: u64 = 0;
    let mut skipped_clean_buckets: u64 = 0;

    let mut duplicate_apply: Vec<DuplicateApplyGroup> = Vec::new();

    for (size, metas) in candidates {
        if metas.len() < 2 {
            continue;
        }

        let paths: Vec<String> = metas.iter().map(|m| m.path.clone()).collect();
        if !cold_dup_pass && !store::bucket_has_hash_dirty(writer, &paths)? {
            skipped_clean_buckets += 1;
            continue;
        }
        if !cold_dup_pass {
            store::clear_dup_flags_for_bucket_paths(writer, &paths)?;
        }

        let (reps, rep_to_all) = inode_representatives(&metas);

        if size <= SMALL_FILE_MAX {
            let results = parallel_full_hashes(&reps);
            let mut full_map: HashMap<[u8; 32], Vec<String>> = HashMap::new();
            for (rep, res) in results {
                if let Ok(h) = res {
                    let expanded = rep_to_all
                        .get(&rep)
                        .cloned()
                        .unwrap_or_else(|| vec![rep.clone()]);
                    full_map.entry(h).or_default().extend(expanded);
                }
            }
            for (hash, mut dup_paths) in full_map {
                dup_paths.sort();
                dup_paths.dedup();
                if dup_paths.len() < 2 {
                    if let Some(p) = dup_paths.first() {
                        let _ = store::update_full_hash(writer, p, &hash);
                    }
                    continue;
                }
                push_dup_group(writer, &mut duplicate_apply, &dup_paths, hash, size)?;
            }
        } else {
            let sample_results = parallel_sample_hashes(&reps, size);
            let mut sample_map: HashMap<[u8; 32], Vec<String>> = HashMap::new();
            for (rep, res) in sample_results {
                if let Ok(h) = res {
                    sample_map.entry(h).or_default().push(rep);
                }
            }

            for (_sample, rep_group) in sample_map {
                if rep_group.len() < 2 {
                    continue;
                }
                let results = parallel_full_hashes(&rep_group);
                let mut full_map: HashMap<[u8; 32], Vec<String>> = HashMap::new();
                for (rep, res) in results {
                    if let Ok(h) = res {
                        let expanded = rep_to_all
                            .get(&rep)
                            .cloned()
                            .unwrap_or_else(|| vec![rep.clone()]);
                        full_map.entry(h).or_default().extend(expanded);
                    }
                }

                for (hash, mut dup_paths) in full_map {
                    dup_paths.sort();
                    dup_paths.dedup();
                    if dup_paths.len() < 2 {
                        if let Some(p) = dup_paths.first() {
                            let _ = store::update_full_hash(writer, p, &hash);
                        }
                        continue;
                    }
                    push_dup_group(writer, &mut duplicate_apply, &dup_paths, hash, size)?;
                }
            }
        }

        done_groups += 1;
        if done_groups % 4 == 0 {
            let _ = ui_tx.send(DiskUiEvent::HashProgress {
                done: done_groups,
                total: total_groups.max(1),
            });
        }
    }

    store::apply_duplicate_results(writer, &duplicate_apply)?;

    if !cold_dup_pass {
        let cleared = store::clear_standalone_hash_dirty(writer)?;
        if cleared > 0 {
            let _ = ui_tx.send(DiskUiEvent::Log(format!(
                "Cleared hash-dirty on {cleared} non-collision file(s)"
            )));
        }
        if skipped_clean_buckets > 0 {
            let _ = ui_tx.send(DiskUiEvent::Log(format!(
                "Duplicate scan: skipped {skipped_clean_buckets} unchanged size bucket(s)"
            )));
        }
    }

    let reclaim = store::reclaimable_bytes(writer)?;
    let _ = ui_tx.send(DiskUiEvent::DuplicatesReady {
        groups: duplicate_apply.len() as u64,
        reclaimable_bytes: reclaim,
    });

    Ok(())
}

fn push_dup_group(
    conn: &mut rusqlite::Connection,
    duplicate_apply: &mut Vec<DuplicateApplyGroup>,
    dup_paths: &[String],
    hash: [u8; 32],
    size: u64,
) -> Result<()> {
    let keep_idx = rank_keep_index(dup_paths);
    for p in dup_paths {
        let _ = store::update_full_hash(conn, p, &hash);
    }
    let stem_map = store::fetch_normalized_stems(conn, dup_paths)?;
    let likely_delete = build_likely_delete_flags(dup_paths, keep_idx, &stem_map);
    duplicate_apply.push(DuplicateApplyGroup {
        paths: dup_paths.to_vec(),
        hash,
        size_bytes: size,
        keep_index: keep_idx,
        likely_delete,
    });
    Ok(())
}

fn rank_keep_index(paths: &[String]) -> usize {
    let mut best_i = 0usize;
    let mut best_key = (2u32, u32::MAX); // lower is better; first path wins ties

    for (i, p) in paths.iter().enumerate() {
        let name = Path::new(p)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let norm = normalize_filename(Path::new(name).as_os_str());
        let copy_penalty = if norm.has_copy_suffix { 1u32 } else { 0u32 };
        let name_len = name.chars().count() as u32;
        let key = (copy_penalty, name_len);
        if key < best_key {
            best_key = key;
            best_i = i;
        }
    }
    best_i
}

pub fn rowdb_to_rows(rows: Vec<EntryRowDb>) -> Vec<crate::disk::model::EntryRow> {
    use crate::disk::model::{EntryFlags, EntryRow};
    rows.into_iter()
        .map(|r| {
            let fh = r.full_hash.as_ref().and_then(|v| {
                if v.len() == 32 {
                    let mut a = [0u8; 32];
                    a.copy_from_slice(v);
                    Some(a)
                } else {
                    None
                }
            });
            EntryRow {
                id: r.id,
                path: r.path,
                name: r.name,
                is_dir: r.is_dir,
                size_bytes: r.size_bytes,
                mtime_ms: r.mtime_ms,
                ext: r.ext,
                flags: r.flags,
                full_hash: fh,
                dup_group_id: r.dup_group_id,
                likely_delete: EntryFlags(r.flags).contains(EntryFlags::LIKELY_DELETE),
                keep_winner: EntryFlags(r.flags).contains(EntryFlags::KEEP_WINNER),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn full_hash_idempotent_small_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.bin");
        let mut f = File::create(&p).unwrap();
        f.write_all(b"hello world content").unwrap();
        drop(f);
        let a = full_hash_file(&p).unwrap();
        let b = full_hash_file(&p).unwrap();
        assert_eq!(a, b);
        // Sample path mixes prefix/suffix + little-endian size; full hash is raw BLAKE3 of file.
        let sz = std::fs::metadata(&p).unwrap().len();
        let s = sample_hash_file(&p, sz).unwrap();
        assert_ne!(s, a);
    }

    #[test]
    fn inode_representatives_merge() {
        let metas = vec![
            FilePathMeta {
                path: "/a/x".into(),
                inode: Some(42),
                dev: Some(9),
            },
            FilePathMeta {
                path: "/a/y".into(),
                inode: Some(42),
                dev: Some(9),
            },
            FilePathMeta {
                path: "/a/z".into(),
                inode: None,
                dev: None,
            },
        ];
        let (reps, m) = inode_representatives(&metas);
        assert_eq!(reps.len(), 2);
        let mut all: HashSet<_> = reps.iter().cloned().collect();
        assert!(all.contains("/a/x") || all.contains("/a/y"));
        assert!(all.contains("/a/z"));
        let key = if m.contains_key("/a/x") {
            "/a/x"
        } else {
            "/a/y"
        };
        assert_eq!(m.get(key).map(|v| v.len()), Some(2));
    }
}
