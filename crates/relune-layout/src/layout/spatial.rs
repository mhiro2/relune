//! Uniform-grid spatial index shared by edge routing and force-directed layout.

use std::collections::HashMap;
use std::ops::RangeInclusive;

use crate::route::Rect;

/// Axis-aligned bounding box used to drive the spatial index.
#[derive(Debug, Clone, Copy)]
pub(super) struct BBox {
    pub(super) min_x: f32,
    pub(super) min_y: f32,
    pub(super) max_x: f32,
    pub(super) max_y: f32,
}

impl BBox {
    pub(super) const EMPTY: Self = Self {
        min_x: f32::MAX,
        min_y: f32::MAX,
        max_x: f32::MIN,
        max_y: f32::MIN,
    };

    pub(super) fn from_points(points: &[(f32, f32)]) -> Self {
        let mut bbox = Self::EMPTY;
        for &(x, y) in points {
            bbox.min_x = bbox.min_x.min(x);
            bbox.min_y = bbox.min_y.min(y);
            bbox.max_x = bbox.max_x.max(x);
            bbox.max_y = bbox.max_y.max(y);
        }
        bbox
    }

    pub(super) const fn from_rect(rect: &Rect) -> Self {
        Self {
            min_x: rect.x,
            min_y: rect.y,
            max_x: rect.x + rect.w,
            max_y: rect.y + rect.h,
        }
    }

    pub(super) fn include_rect(&mut self, rect: &Rect) {
        self.min_x = self.min_x.min(rect.x);
        self.min_y = self.min_y.min(rect.y);
        self.max_x = self.max_x.max(rect.x + rect.w);
        self.max_y = self.max_y.max(rect.y + rect.h);
    }

    pub(super) fn expanded(self, margin: f32) -> Self {
        Self {
            min_x: self.min_x - margin,
            min_y: self.min_y - margin,
            max_x: self.max_x + margin,
            max_y: self.max_y + margin,
        }
    }

    /// Whether `rect` touches or overlaps this box.
    pub(super) fn intersects_rect(&self, rect: &Rect) -> bool {
        rect.x <= self.max_x
            && rect.x + rect.w >= self.min_x
            && rect.y <= self.max_y
            && rect.y + rect.h >= self.min_y
    }

    const fn is_empty(&self) -> bool {
        self.min_x > self.max_x || self.min_y > self.max_y
    }
}

/// Uniform-grid spatial index mapping cells to the indices of items whose
/// bounding box covers them.
///
/// An item is registered in every cell its box touches, so a single oversized
/// item never forces a coarser cell size on everything else, and two boxes
/// that intersect always share at least one cell.
#[derive(Debug)]
pub(super) struct SpatialGrid {
    inv_cell: f32,
    cells: HashMap<(i32, i32), Vec<usize>>,
}

impl SpatialGrid {
    pub(super) fn new(cell_size: f32) -> Self {
        Self {
            inv_cell: 1.0 / cell_size.max(1.0),
            cells: HashMap::new(),
        }
    }

    #[allow(clippy::cast_possible_truncation)] // Layout coordinates stay well within i32 cell indices.
    fn cell_range(&self, bbox: &BBox) -> (RangeInclusive<i32>, RangeInclusive<i32>) {
        let cell = |value: f32| (value * self.inv_cell).floor() as i32;
        (
            cell(bbox.min_x)..=cell(bbox.max_x),
            cell(bbox.min_y)..=cell(bbox.max_y),
        )
    }

    pub(super) fn clear(&mut self) {
        self.cells.clear();
    }

    pub(super) fn insert(&mut self, index: usize, bbox: &BBox) {
        if bbox.is_empty() {
            return;
        }
        let (x_cells, y_cells) = self.cell_range(bbox);
        for x in x_cells {
            for y in y_cells.clone() {
                self.cells.entry((x, y)).or_default().push(index);
            }
        }
    }

    /// Appends the indices of all items whose footprint may intersect `bbox`
    /// to `indices`, without sorting or de-duplicating.
    fn collect(&self, bbox: &BBox, indices: &mut Vec<usize>) {
        if bbox.is_empty() {
            return;
        }
        let (x_cells, y_cells) = self.cell_range(bbox);
        for x in x_cells {
            for y in y_cells.clone() {
                if let Some(items) = self.cells.get(&(x, y)) {
                    indices.extend_from_slice(items);
                }
            }
        }
    }

    /// Returns the indices of all items whose footprint may intersect `bbox`,
    /// sorted ascending and de-duplicated so callers can preserve the original
    /// obstacle iteration order (and therefore identical floating-point results).
    pub(super) fn query_sorted(&self, bbox: &BBox) -> Vec<usize> {
        self.query_many_sorted(std::slice::from_ref(bbox))
    }

    /// Like [`Self::query_sorted`], for the union of several boxes.
    pub(super) fn query_many_sorted(&self, bboxes: &[BBox]) -> Vec<usize> {
        let mut indices = Vec::new();
        for bbox in bboxes {
            self.collect(bbox, &mut indices);
        }
        indices.sort_unstable();
        indices.dedup();
        indices
    }

    /// Returns every pair of items that share at least one cell, as
    /// `(smaller, larger)` index pairs sorted ascending without duplicates.
    pub(super) fn cell_pairs(&self) -> Vec<(usize, usize)> {
        let mut pairs = Vec::new();
        for items in self.cells.values() {
            for (offset, &left) in items.iter().enumerate() {
                for &right in &items[offset + 1..] {
                    if left != right {
                        pairs.push((left.min(right), left.max(right)));
                    }
                }
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        pairs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn oversized_item_only_pairs_with_items_it_touches() {
        let mut grid = SpatialGrid::new(100.0);
        grid.insert(0, &BBox::from_rect(&rect(0.0, 0.0, 1000.0, 50.0)));
        grid.insert(1, &BBox::from_rect(&rect(900.0, 10.0, 40.0, 40.0)));
        grid.insert(2, &BBox::from_rect(&rect(0.0, 500.0, 40.0, 40.0)));

        assert_eq!(grid.cell_pairs(), vec![(0, 1)]);
    }

    #[test]
    fn query_many_sorted_merges_and_deduplicates() {
        let mut grid = SpatialGrid::new(50.0);
        for (index, x) in [0.0, 200.0, 400.0].into_iter().enumerate() {
            grid.insert(index, &BBox::from_rect(&rect(x, 0.0, 20.0, 20.0)));
        }

        let hits = grid.query_many_sorted(&[
            BBox::from_rect(&rect(390.0, 0.0, 10.0, 10.0)),
            BBox::from_rect(&rect(0.0, 0.0, 10.0, 10.0)),
            BBox::from_rect(&rect(5.0, 5.0, 1.0, 1.0)),
        ]);

        assert_eq!(hits, vec![0, 2]);
    }
}
