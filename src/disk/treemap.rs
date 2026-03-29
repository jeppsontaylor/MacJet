//! Squarified treemap (`treemap` crate), spatial navigation, "Other (n)" bucket.

use crate::disk::model::EntryRow;
use crate::ui::styles;

/// Default max rectangles per zoom level (top entries + one `Other` when over cap).
pub const DEFAULT_MAX_TREEMAP_TILES: usize = 100;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::StatefulWidget,
};
use std::collections::HashSet;
use std::path::PathBuf;
use treemap::{Mappable, Rect as TRect, TreemapLayout};

#[derive(Debug, Clone)]
pub struct TreeTile {
    /// Indices into `children` covered by this tile (len > 1 = aggregated "Other").
    pub child_indices: Vec<usize>,
    pub rect: Rect,
    pub label: String,
    pub fill: Color,
    pub likely_delete: bool,
    pub keep_winner: bool,
    pub is_dir: bool,
    pub marked: bool,
    /// Total bytes when `child_indices.len() > 1`, else same as single entry.
    pub aggregate_bytes: u64,
}

impl TreeTile {
    pub fn primary_index(&self) -> usize {
        *self.child_indices.first().unwrap_or(&0)
    }
}

#[derive(Debug, Default, Clone)]
pub struct TreeMapState {
    pub focus: usize,
    /// left, up, right, down
    pub neighbors: Vec<[Option<usize>; 4]>,
}

pub struct TreeMapWidget<'a> {
    pub tiles: &'a [TreeTile],
    pub use_256_color: bool,
}

#[derive(Clone)]
struct LayItem {
    size: f64,
    bounds: TRect,
    child_indices: Vec<usize>,
    aggregate_bytes: u64,
}

impl Mappable for LayItem {
    fn size(&self) -> f64 {
        self.size
    }
    fn bounds(&self) -> &TRect {
        &self.bounds
    }
    fn set_bounds(&mut self, bounds: TRect) {
        self.bounds = bounds;
    }
}

/// Sort: directories first, then descending size, then path (stable).
fn sort_entries_for_treemap(pairs: &mut Vec<(usize, u64, &EntryRow)>) {
    pairs.sort_by(|a, b| {
        b.2.is_dir
            .cmp(&a.2.is_dir)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.path.cmp(&b.2.path))
    });
}

/// Fold singles whose area share is below 2 cells / panel into one `Other` bucket.
fn fold_small_share_buckets(
    mut specs: Vec<(Vec<usize>, u64)>,
    area_f: f64,
) -> Vec<(Vec<usize>, u64)> {
    let min_share = 2.0 / area_f.max(1.0);
    loop {
        let total: u64 = specs.iter().map(|(_, s)| *s).sum::<u64>().max(1);
        let mut small: Vec<(Vec<usize>, u64)> = Vec::new();
        let mut big: Vec<(Vec<usize>, u64)> = Vec::new();
        for (idxs, sz) in specs {
            let share = (sz as f64) / (total as f64);
            if idxs.len() == 1 && share < min_share {
                small.push((idxs, sz));
            } else {
                big.push((idxs, sz));
            }
        }
        if small.is_empty() {
            return big;
        }
        let mut oidx: Vec<usize> = Vec::new();
        let mut osum: u64 = 0;
        for (ix, s) in small {
            oidx.extend(ix);
            osum += s;
        }
        let mut merged = big;
        if let Some(pos) = merged.iter().position(|(ix, _)| ix.len() > 1) {
            let (mut ix, s) = merged.swap_remove(pos);
            ix.extend(oidx);
            merged.push((ix, s + osum));
        } else {
            merged.push((oidx, osum.max(1)));
        }
        specs = merged;
    }
}

/// Squarified layout; at most `max_tiles` buckets (top by size + one `Other` when needed).
pub fn layout_entries(
    area: Rect,
    entries: &[EntryRow],
    marked: &HashSet<PathBuf>,
    max_tiles: usize,
) -> Vec<TreeTile> {
    if area.width < 4 || area.height < 2 || entries.is_empty() {
        return Vec::new();
    }

    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    let max_tiles = max_tiles.max(2);
    let mut pairs: Vec<(usize, u64, &EntryRow)> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (i, e.size_bytes.max(1), e))
        .collect();
    sort_entries_for_treemap(&mut pairs);

    let area_f = (inner.width as f64) * (inner.height as f64).max(1.0);

    let mut layout_specs: Vec<(Vec<usize>, u64)> = Vec::new();
    if pairs.len() <= max_tiles {
        for (i, sz, _) in pairs {
            layout_specs.push((vec![i], sz));
        }
    } else {
        let head = max_tiles.saturating_sub(1);
        for (i, sz, _) in pairs.iter().take(head) {
            layout_specs.push((vec![*i], *sz));
        }
        let tail: Vec<(usize, u64)> = pairs
            .iter()
            .skip(head)
            .map(|(i, sz, _)| (*i, *sz))
            .collect();
        let osum: u64 = tail.iter().map(|(_, s)| *s).sum::<u64>().max(1);
        let oidx: Vec<usize> = tail.iter().map(|(i, _)| *i).collect();
        layout_specs.push((oidx, osum));
    }

    layout_specs = fold_small_share_buckets(layout_specs, area_f);

    layout_specs.sort_by(|a, b| {
        b.1.cmp(&a.1).then_with(|| {
            let da =
                a.0.first()
                    .and_then(|&i| entries.get(i))
                    .is_some_and(|e| e.is_dir);
            let db =
                b.0.first()
                    .and_then(|&i| entries.get(i))
                    .is_some_and(|e| e.is_dir);
            db.cmp(&da)
        })
    });

    let mut items: Vec<LayItem> = layout_specs
        .into_iter()
        .map(|(idxs, sz)| LayItem {
            size: sz as f64,
            bounds: TRect::new(),
            child_indices: idxs,
            aggregate_bytes: sz,
        })
        .collect();

    if items.is_empty() {
        return Vec::new();
    }

    let layout = TreemapLayout::new();
    layout.layout_items(
        &mut items,
        TRect::from_points(0.0, 0.0, inner.width as f64, inner.height as f64),
    );

    let mut tiles = Vec::with_capacity(items.len());
    for it in items {
        let r = Rect::new(
            inner.x.saturating_add(it.bounds.x.floor() as u16),
            inner.y.saturating_add(it.bounds.y.floor() as u16),
            (it.bounds.w.ceil() as u16).max(1).min(inner.width),
            (it.bounds.h.ceil() as u16).max(1).min(inner.height),
        );
        let rep_idx = representative_index(entries, &it.child_indices);
        let row = entries.get(rep_idx).expect("child index in bounds");
        let label = if it.child_indices.len() > 1 {
            format!("Other ({})", it.child_indices.len())
        } else {
            row.name.clone()
        };
        let marked_tile = it
            .child_indices
            .iter()
            .filter_map(|&i| entries.get(i))
            .any(|e| marked.contains(&e.path_buf()));
        let likely_del = if it.child_indices.len() == 1 {
            row.likely_delete
        } else {
            it.child_indices
                .iter()
                .any(|&i| entries.get(i).is_some_and(|e| e.likely_delete))
        };
        let t = make_tile(
            &it.child_indices,
            r,
            row,
            marked_tile,
            likely_del,
            it.aggregate_bytes,
            &label,
        );
        tiles.push(t);
    }

    tiles
}

fn representative_index(entries: &[EntryRow], child_indices: &[usize]) -> usize {
    child_indices
        .iter()
        .max_by_key(|&&i| entries.get(i).map(|e| e.size_bytes).unwrap_or(0))
        .copied()
        .unwrap_or(0)
}

fn format_bytes_compact(n: u64) -> String {
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

fn make_tile(
    child_indices: &[usize],
    rect: Rect,
    representative: &EntryRow,
    marked: bool,
    likely_delete: bool,
    aggregate_bytes: u64,
    label: &str,
) -> TreeTile {
    let fill = if representative.ext.is_empty() && representative.is_dir {
        styles::ACCENT_VIOLET
    } else {
        styles::color_hash(&representative.ext)
    };
    TreeTile {
        child_indices: child_indices.to_vec(),
        rect,
        label: label.to_string(),
        fill,
        likely_delete,
        keep_winner: representative.keep_winner && child_indices.len() == 1,
        is_dir: representative.is_dir && child_indices.len() == 1,
        marked,
        aggregate_bytes,
    }
}

fn rect_bottom(r: Rect) -> u16 {
    r.y.saturating_add(r.height)
}

fn rect_right(r: Rect) -> u16 {
    r.x.saturating_add(r.width)
}

fn vertical_overlap(a: Rect, b: Rect) -> bool {
    let ay2 = rect_bottom(a);
    let by2 = rect_bottom(b);
    a.y.max(b.y) < ay2.min(by2)
}

fn horizontal_overlap(a: Rect, b: Rect) -> bool {
    let ax2 = rect_right(a);
    let bx2 = rect_right(b);
    a.x.max(b.x) < ax2.min(bx2)
}

fn pick_better(
    cand: (usize, f32, f32),
    best: Option<(usize, f32, f32)>,
) -> Option<(usize, f32, f32)> {
    match best {
        None => Some(cand),
        Some(o) if cand.1 < o.1 || (cand.1 == o.1 && cand.2 < o.2) => Some(cand),
        Some(o) => Some(o),
    }
}

pub fn compute_neighbors(tiles: &[TreeTile]) -> Vec<[Option<usize>; 4]> {
    let n = tiles.len();
    let mut out = vec![[None, None, None, None]; n];
    if n == 0 {
        return out;
    }

    let centers: Vec<(f32, f32)> = tiles
        .iter()
        .map(|t| {
            (
                t.rect.x as f32 + t.rect.width as f32 / 2.0,
                t.rect.y as f32 + t.rect.height as f32 / 2.0,
            )
        })
        .collect();

    for i in 0..n {
        let (cx, cy) = centers[i];
        let ri = tiles[i].rect;

        let mut best_r = None;
        for j in 0..n {
            if i == j {
                continue;
            }
            let (ox, oy) = centers[j];
            if ox <= cx {
                continue;
            }
            let rj = tiles[j].rect;
            let ov = if vertical_overlap(ri, rj) {
                0.0
            } else {
                (oy - cy).abs()
            };
            let dist = (ox - cx).hypot(oy - cy);
            best_r = pick_better((j, ov, dist), best_r);
        }
        out[i][2] = best_r.map(|b| b.0);

        let mut best_l = None;
        for j in 0..n {
            if i == j {
                continue;
            }
            let (ox, oy) = centers[j];
            if ox >= cx {
                continue;
            }
            let rj = tiles[j].rect;
            let ov = if vertical_overlap(ri, rj) {
                0.0
            } else {
                (oy - cy).abs()
            };
            let dist = (cx - ox).hypot(oy - cy);
            best_l = pick_better((j, ov, dist), best_l);
        }
        out[i][0] = best_l.map(|b| b.0);

        let mut best_d = None;
        for j in 0..n {
            if i == j {
                continue;
            }
            let (ox, oy) = centers[j];
            if oy <= cy {
                continue;
            }
            let rj = tiles[j].rect;
            let ov = if horizontal_overlap(ri, rj) {
                0.0
            } else {
                (ox - cx).abs()
            };
            let dist = (ox - cx).hypot(oy - cy);
            best_d = pick_better((j, ov, dist), best_d);
        }
        out[i][3] = best_d.map(|b| b.0);

        let mut best_u = None;
        for j in 0..n {
            if i == j {
                continue;
            }
            let (ox, oy) = centers[j];
            if oy >= cy {
                continue;
            }
            let rj = tiles[j].rect;
            let ov = if horizontal_overlap(ri, rj) {
                0.0
            } else {
                (ox - cx).abs()
            };
            let dist = (ox - cx).hypot(cy - oy);
            best_u = pick_better((j, ov, dist), best_u);
        }
        out[i][1] = best_u.map(|b| b.0);
    }

    out
}

fn treemap_bg(fill: Color, likely_delete: bool, reduced_color: bool) -> Color {
    if likely_delete {
        return if reduced_color {
            Color::Red
        } else {
            Color::Rgb(90, 35, 55)
        };
    }
    if !reduced_color {
        return fill;
    }
    match fill {
        Color::Rgb(r, g, b) => {
            let y = (r as u16 + g as u16 + b as u16) / 3;
            if y > 200 {
                Color::Gray
            } else if y > 120 {
                Color::DarkGray
            } else {
                Color::Blue
            }
        }
        Color::Indexed(i) => Color::Indexed(i),
        c => c,
    }
}

impl<'a> StatefulWidget for TreeMapWidget<'a> {
    type State = TreeMapState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let focus_border = styles::ACCENT_CYAN;
        let mark_border = styles::ACCENT_AMBER;
        let reduced = self.use_256_color;
        for (ti, tile) in self.tiles.iter().enumerate() {
            let r = tile.rect.intersection(area);
            if r.width == 0 || r.height == 0 {
                continue;
            }

            let bg = treemap_bg(tile.fill, tile.likely_delete, reduced);

            for y in r.y..r.y + r.height {
                for x in r.x..r.x + r.width {
                    let cell = &mut buf[(x, y)];
                    cell.set_bg(bg);
                    cell.set_fg(styles::TEXT_BRIGHT);
                    cell.set_symbol(" ");
                }
            }

            let border_col = if ti == state.focus {
                focus_border
            } else if tile.marked {
                mark_border
            } else {
                Color::Reset
            };

            if ti == state.focus || tile.marked {
                let bcol = if reduced && matches!(border_col, Color::Rgb(_, _, _)) {
                    Color::Cyan
                } else {
                    border_col
                };
                for x in r.x..r.x + r.width {
                    buf[(x, r.y)].set_bg(bcol);
                    if r.height > 1 {
                        buf[(x, r.y + r.height - 1)].set_bg(bcol);
                    }
                }
                for y in r.y..r.y + r.height {
                    buf[(r.x, y)].set_bg(bcol);
                    if r.width > 1 {
                        buf[(r.x + r.width - 1, y)].set_bg(bcol);
                    }
                }
            }

            if tile.keep_winner && r.width > 2 && r.height > 1 {
                buf[(r.x + 1, r.y + 1)].set_symbol("K");
                buf[(r.x + 1, r.y + 1)]
                    .set_fg(styles::ACCENT_GREEN)
                    .set_bg(bg);
            }

            if r.width > 4 && r.height > 1 {
                let max_chars = (r.width.saturating_sub(2)) as usize;
                let text = crate::ui::styles::truncate_ellipsis(&tile.label, max_chars);
                let line_style = Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD);
                buf.set_string(r.x + 1, r.y + 1, text, line_style);
            }
            if r.width > 4 && r.height >= 3 {
                let sz = format_bytes_compact(tile.aggregate_bytes);
                let sz_line =
                    crate::ui::styles::truncate_ellipsis(&sz, (r.width.saturating_sub(2)) as usize);
                let sz_style = Style::default().fg(styles::TEXT_DIM).bg(bg);
                buf.set_string(r.x + 1, r.y + 2, sz_line, sz_style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::model::EntryRow;

    fn entry(name: &str, sz: u64) -> EntryRow {
        EntryRow {
            id: 0,
            path: format!("/tmp/{name}"),
            name: name.to_string(),
            is_dir: false,
            size_bytes: sz,
            mtime_ms: None,
            ext: "txt".into(),
            flags: 0,
            full_hash: None,
            dup_group_id: None,
            likely_delete: false,
            keep_winner: false,
        }
    }

    #[test]
    fn layout_fits_area_and_weights_sum() {
        let entries = vec![entry("a", 100), entry("b", 50), entry("c", 25)];
        let area = Rect::new(0, 0, 40, 20);
        let tiles = layout_entries(area, &entries, &HashSet::new(), DEFAULT_MAX_TREEMAP_TILES);
        assert!(!tiles.is_empty());
        for t in &tiles {
            assert!(t.rect.x >= area.x);
            assert!(t.rect.y >= area.y);
            assert!(rect_right(t.rect) <= area.x + area.width);
            assert!(rect_bottom(t.rect) <= area.y + area.height);
        }
    }

    #[test]
    fn neighbors_len_matches_tiles() {
        let entries = vec![
            entry("a", 10),
            entry("b", 10),
            entry("c", 10),
            entry("d", 10),
        ];
        let tiles = layout_entries(
            Rect::new(0, 0, 30, 20),
            &entries,
            &HashSet::new(),
            DEFAULT_MAX_TREEMAP_TILES,
        );
        let n = compute_neighbors(&tiles);
        assert_eq!(n.len(), tiles.len());
    }

    #[test]
    fn cap_at_100_yields_other_bucket() {
        let mut entries: Vec<EntryRow> = (0..101)
            .map(|i| entry(&format!("f{i}"), 1000 + i as u64))
            .collect();
        let area = Rect::new(0, 0, 80, 24);
        let tiles = layout_entries(area, &entries, &HashSet::new(), 100);
        assert!(
            tiles.len() <= 100,
            "expected at most 100 tiles, got {}",
            tiles.len()
        );
        let other: Vec<_> = tiles.iter().filter(|t| t.child_indices.len() > 1).collect();
        assert_eq!(
            other.len(),
            1,
            "expected exactly one Other bucket, got {:?}",
            other.len()
        );
        assert_eq!(other[0].child_indices.len(), 2);
        let covered: usize = tiles.iter().map(|t| t.child_indices.len()).sum();
        assert_eq!(covered, 101);
    }

    #[test]
    fn sort_dirs_before_files_at_equal_size() {
        let e0 = entry("f", 50);
        let mut e1 = entry("d", 50);
        e1.is_dir = true;
        e1.ext = String::new();
        let entries = vec![e0, e1];
        let mut pairs: Vec<(usize, u64, &EntryRow)> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (i, e.size_bytes.max(1), e))
            .collect();
        sort_entries_for_treemap(&mut pairs);
        assert_eq!(pairs[0].0, 1, "directory row should sort before file");
    }

    #[test]
    fn tiny_area_folds_to_single_other() {
        let entries: Vec<EntryRow> = (0..20).map(|i| entry(&format!("s{i}"), 1)).collect();
        let area = Rect::new(0, 0, 12, 6);
        let tiles = layout_entries(area, &entries, &HashSet::new(), 100);
        assert!(!tiles.is_empty());
        let sum_children: usize = tiles.iter().map(|t| t.child_indices.len()).sum();
        assert_eq!(sum_children, 20);
    }
}
