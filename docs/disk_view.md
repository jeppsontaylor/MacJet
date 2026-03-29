# Disk space view

## Overview

Press **`6`** in the TUI (or use **`Tab`** to cycle) to open the Disk view. It indexes a configurable root (default: **Downloads**) into a **SQLite WAL** database, shows a **squarified treemap** (wide terminals) or a **size-sorted table**, and supports **move to Trash** (with confirmation) via the `trash` crate.

**Treemap layout:** Direct children of the current folder are shown as rectangles whose **area is proportional to size** (minimum footprint **2×1** cells per spec). At most **100** rectangles are shown per zoom level: the largest items get their own tiles; the rest are merged into a single **`Other (n)`** tile. Press **`Enter`** on **`Other`** to open a **scrollable table** of every aggregated item (sorted by size). **`Backspace`**, **`Esc`**, or **`←`** closes that list; **`Enter`** on a row opens a subdirectory or returns to the treemap with that item’s tile focused (files). Sort order for layout is **directories first**, then **descending size**, then path.

## Indexing and warm reload

The index file lives at the path returned by `macjet::disk::default_disk_index_path()` (typically under the MacJet application cache directory: `disk_index.sqlite`).

- **Cold start** (first run, or scan root changed vs `indexed_root` meta): the database is cleared and fully rebuilt.
- **Warm start** (same root as last completed scan): existing rows are **not** bulk-deleted; the walk **upserts** metadata and removes stale paths (`scan_epoch`).
- **`HASH_DIRTY`**: each file row is marked dirty when the path is **new** or **`size_bytes` / `mtime_ms` / `inode` / `dev`** change. On warm runs, **duplicate detection skips entire size-collision buckets** where no member is dirty, avoiding redundant BLAKE3 work.
- **Orphans**: if a duplicate “partner” disappears (only one file left at that size), dup metadata is cleared automatically.

After a warm scan with no file changes, check the Disk log strip for lines like **`Duplicate scan: skipped N unchanged size bucket(s)`**.

## Duplicate “quick wins” (inspector + `d`)

After duplicate detection runs, the **Inspector** lists up to **8** duplicate **groups** (not individual files) sorted by **reclaimable bytes** first (`SUM(size_bytes)` of rows already flagged **`LIKELY_DELETE` / DUPE?**), then by total size in the group. Each line shows reclaim estimate, member count, and a preview name.

Press **`d`** to open a **full-screen modal**:

1. **Group list:** arrow keys, **Enter** opens the selected group.
2. **Pick keeper:** choose which path to **keep**; **Enter** saves to SQLite — that path gets **`KEEP_WINNER`**, every **other file in the same `dup_group_id`** gets **`LIKELY_DELETE`**. This uses the existing index (**no re-hash**).
3. **`Esc`** or **`Backspace`** closes the modal, or returns from pick-keeper to the group list.

**Trash:** saving a keeper does **not** delete files. Mark dupes with **`Space`** or rely on **`LIKELY_DELETE`** in the treemap, then **`t`** to open the existing Trash confirmation modal.

## MCP (headless)

The MCP server reads the **same** SQLite file (optional `db_path` on tools). Index may be stale until a TUI Disk scan has completed at least once.

**Resources (examples)**

- `macjet://disk/summary`
- `macjet://disk/duplicates?limit=50&only_reclaimable=true`
- `macjet://disk/directory?path=/path/to/dir`

**Tools**

- `get_disk_summary`, `list_disk_duplicates`, `suggest_disk_cleanup`
- `trash_disk_paths` (disabled when `MACJET_MCP_READONLY=1`; audited to `~/.macjet/mcp_audit.jsonl`)

See [mcp.md](mcp.md) for the full tool list.

## Demo asset

The README may reference [`assets/macjet_disk_demo.gif`](../assets/macjet_disk_demo.gif). Record the **real** TUI (treemap, **`Other`** drill, inspector) and overwrite that file so marketing matches the app:

1. Size the terminal similarly to [`assets/macjet_demo.gif`](../assets/macjet_demo.gif) (if present).
2. Run `macjet`, press **`6`**, drill into a folder, focus **`Other`** and press **`Enter`** to show the list, then **`Backspace`** or **`←`** to return.
3. Export a looping GIF (e.g. [asciinema](https://asciinema.org/) + `agg`, `ttyrec` + conversion, or screen recording → `ffmpeg`) and save as `assets/macjet_disk_demo.gif`.

## Disk keybindings (summary)

Context-specific hints also appear in the footer on the Disk tab. Common keys:

| Key | Action |
|-----|--------|
| `6` | Disk view |
| Arrows | Treemap spatial nav; in **Other** list, move selection |
| `Enter` | Directory: drill in · **Other** tile: open member list · in list: open dir or return to treemap |
| `Backspace` | Close **Other** list, or parent directory |
| `Esc` / `←` | Close **Other** list (also `Esc` clears global filter when not in **Other**) |
| `Space` | Mark / unmark file for Trash |
| `t` | Confirm Trash modal |
| `R` | Rescan |
| `/` | Search (filter indexed children) |
| `o` | Reveal in Finder |
| `d` | Duplicate quick wins modal (pick keeper per group) |
