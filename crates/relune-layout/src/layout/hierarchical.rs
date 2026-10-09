//! Hierarchical (rank-based) coordinate assignment and component packing.
//!
//! Nodes are split into weakly connected components, and each component keeps
//! its own rank rows. Components (and group containers built from them) are
//! then packed into "shelves" along the secondary axis, wrapping to a new shelf
//! once a target extent derived from the total area is reached. This keeps
//! schemas with many unrelated tables (or no foreign keys at all) close to a
//! screen-friendly aspect ratio instead of one unbounded row.

use std::collections::{BTreeMap, HashMap};

use relune_core::LayoutDirection;

use crate::graph::LayoutGraph;

use super::spacing::{
    build_positioned_node, compute_graph_bounds, mirror_positioned_nodes_for_direction,
};
use super::{
    GROUP_PADDING, GROUP_TOP_PADDING, LayoutConfig, LayoutError, NodeSize, PositionedNode,
};

/// Minimum visible gap between two padded group containers.
const MIN_GROUP_CLEARANCE: f32 = 16.0;
/// Preferred on-screen width / height ratio of a packed layout.
const TARGET_SCREEN_ASPECT: f32 = 1.6;
/// Down/up sweep pairs used to align ranked rows with their neighbours.
const ALIGNMENT_SWEEPS: usize = 4;
/// Weight of a node without neighbours in the swept rows, which then mostly
/// keeps its place but still yields to anchored nodes.
const UNANCHORED_WEIGHT: f32 = 1e-3;

/// Positioned nodes plus the row structure that produced them.
#[derive(Debug, Clone)]
pub(super) struct HierarchicalPlacement {
    pub(super) nodes: Vec<PositionedNode>,
    pub(super) width: f32,
    pub(super) height: f32,
    /// Global row index of every node. Rows are disjoint bands along the
    /// primary axis, so routing can treat them as ranks.
    pub(super) node_rows: Vec<usize>,
}

/// How nodes are placed along the secondary axis inside their rank rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowAlignment {
    /// Centre nodes over their neighbours in other rows.
    Neighbors,
    /// Pack each row from the start; used as the force-directed seed, which
    /// relaxes the secondary axis on its own.
    Packed,
}

/// Assign coordinates to nodes based on their ranks and order.
pub(super) fn assign_coordinates(
    graph: &LayoutGraph,
    node_ranks: &[usize],
    ordered_nodes: &[Vec<usize>],
    config: &LayoutConfig,
    node_sizes: &[NodeSize],
    alignment: RowAlignment,
) -> Result<HierarchicalPlacement, LayoutError> {
    let axes = Axes::new(config);
    let packer = Packer {
        node_sizes,
        axes,
        target_ratio: axes.target_secondary_to_primary_ratio(),
        alignment,
    };
    let layout = packer.pack_graph(graph, node_ranks, ordered_nodes);

    let n = graph.nodes.len();
    let mut positioned_slots: Vec<Option<PositionedNode>> = vec![None; n];
    let mut node_rows = vec![0usize; n];
    let mut primary = axes.primary_origin;
    for (row_idx, row) in layout
        .rows
        .iter()
        .filter(|row| !row.cells.is_empty())
        .enumerate()
    {
        if row_idx > 0 {
            primary += row.extra_gap_before;
        }
        let mut row_extent = 0.0_f32;
        for &(node_idx, secondary) in &row.cells {
            let size = node_sizes[node_idx];
            let (x, y) = axes.to_xy(primary, axes.secondary_origin + secondary);
            positioned_slots[node_idx] = Some(build_positioned_node(
                &graph.nodes[node_idx],
                x,
                y,
                size.width,
                size.height,
                size.omitted_columns,
            ));
            node_rows[node_idx] = row_idx;
            row_extent = row_extent.max(axes.primary_extent(size));
        }
        primary += row_extent + axes.primary_gap;
    }

    // Every graph node must have been assigned a position above.
    let mut positioned_nodes = Vec::with_capacity(n);
    for (node_idx, slot) in positioned_slots.into_iter().enumerate() {
        let Some(node) = slot else {
            let node_id = graph
                .reverse_index
                .get(&node_idx)
                .cloned()
                .unwrap_or_else(|| format!("#{node_idx}"));
            return Err(LayoutError::MissingNodePosition { node_id });
        };
        positioned_nodes.push(node);
    }

    let graph_bounds = compute_graph_bounds(&positioned_nodes, config);
    mirror_positioned_nodes_for_direction(&mut positioned_nodes, graph_bounds, config.direction);

    let (width, height) = compute_graph_bounds(&positioned_nodes, config);
    Ok(HierarchicalPlacement {
        nodes: positioned_nodes,
        width,
        height,
        node_rows,
    })
}

/// Axis mapping between the rank flow (primary) and the in-rank (secondary) axis.
#[derive(Debug, Clone, Copy)]
struct Axes {
    is_horizontal: bool,
    primary_origin: f32,
    secondary_origin: f32,
    /// Gap between consecutive rows.
    primary_gap: f32,
    /// Gap between neighbouring nodes or components inside a row.
    secondary_gap: f32,
}

impl Axes {
    const fn new(config: &LayoutConfig) -> Self {
        let is_horizontal = matches!(
            config.direction,
            LayoutDirection::LeftToRight | LayoutDirection::RightToLeft
        );
        if is_horizontal {
            Self {
                is_horizontal,
                primary_origin: config.origin_x,
                secondary_origin: config.origin_y,
                primary_gap: config.horizontal_spacing,
                secondary_gap: config.vertical_spacing,
            }
        } else {
            Self {
                is_horizontal,
                primary_origin: config.origin_y,
                secondary_origin: config.origin_x,
                primary_gap: config.vertical_spacing,
                secondary_gap: config.horizontal_spacing,
            }
        }
    }

    const fn primary_extent(self, size: NodeSize) -> f32 {
        if self.is_horizontal {
            size.width
        } else {
            size.height
        }
    }

    const fn secondary_extent(self, size: NodeSize) -> f32 {
        if self.is_horizontal {
            size.height
        } else {
            size.width
        }
    }

    const fn to_xy(self, primary: f32, secondary: f32) -> (f32, f32) {
        if self.is_horizontal {
            (primary, secondary)
        } else {
            (secondary, primary)
        }
    }

    const fn target_secondary_to_primary_ratio(self) -> f32 {
        if self.is_horizontal {
            TARGET_SCREEN_ASPECT.recip()
        } else {
            TARGET_SCREEN_ASPECT
        }
    }

    /// Gap between two group containers placed next to each other. The floor
    /// keeps the padded container boxes apart even with tiny node spacing.
    fn group_gap(self) -> f32 {
        let padding = if self.is_horizontal {
            GROUP_TOP_PADDING + GROUP_PADDING
        } else {
            2.0 * GROUP_PADDING
        };
        (self.secondary_gap * 1.5).max(padding + MIN_GROUP_CLEARANCE)
    }

    /// Extra primary-axis gap between shelves that hold group containers, so
    /// the bottom padding of one container and the label band of the next fit.
    const fn group_shelf_extra_gap() -> f32 {
        GROUP_TOP_PADDING + GROUP_PADDING
    }
}

/// A rectangular arrangement of nodes in rows, positioned relative to its own
/// secondary-axis origin.
#[derive(Debug, Clone, Default)]
struct Block {
    rows: Vec<BlockRow>,
    /// Extent along the secondary axis.
    secondary_extent: f32,
    /// Whether this block renders as a group container.
    is_group: bool,
}

#[derive(Debug, Clone, Default)]
struct BlockRow {
    /// Additional primary-axis gap inserted before this row.
    extra_gap_before: f32,
    /// `(node index, secondary offset)` pairs in placement order.
    cells: Vec<(usize, f32)>,
}

struct Packer<'a> {
    node_sizes: &'a [NodeSize],
    axes: Axes,
    target_ratio: f32,
    alignment: RowAlignment,
}

impl Packer<'_> {
    fn pack_graph(
        &self,
        graph: &LayoutGraph,
        node_ranks: &[usize],
        ordered_nodes: &[Vec<usize>],
    ) -> Block {
        let n = graph.nodes.len();
        let mut order_in_rank = vec![0usize; n];
        for rank_nodes in ordered_nodes {
            for (position, &node_idx) in rank_nodes.iter().enumerate() {
                order_in_rank[node_idx] = position;
            }
        }
        let mut is_isolated = vec![true; n];
        let mut neighbors = vec![Vec::new(); n];
        for (from, to) in graph_edges(graph) {
            is_isolated[from] = false;
            is_isolated[to] = false;
            neighbors[from].push(to);
            neighbors[to].push(from);
        }
        for list in &mut neighbors {
            list.sort_unstable();
            list.dedup();
        }

        let context = ClusterContext {
            graph,
            node_ranks,
            order_in_rank: &order_in_rank,
            is_isolated: &is_isolated,
            neighbors: &neighbors,
        };
        let mut clusters: Vec<(usize, Block)> = clusters(graph)
            .into_iter()
            .map(|cluster| (cluster.len(), self.cluster_block(&context, &cluster)))
            .collect();
        // Largest clusters first; the stable sort keeps discovery order for ties.
        clusters.sort_by_key(|(size, _)| std::cmp::Reverse(*size));
        self.pack_shelves(clusters.into_iter().map(|(_, block)| block).collect(), 0.0)
    }

    /// Lays out one cluster as side-by-side lanes (one per group, ungrouped
    /// nodes last) that share the cluster's rank rows, so edges between groups
    /// keep flowing along the primary axis. Nodes without any edge are packed
    /// into a grid below their lane's ranked rows.
    fn cluster_block(&self, context: &ClusterContext<'_>, cluster: &[usize]) -> Block {
        let mut ranks: Vec<usize> = cluster
            .iter()
            .filter(|&&idx| !context.is_isolated[idx])
            .map(|&idx| context.node_ranks[idx])
            .collect();
        ranks.sort_unstable();
        ranks.dedup();

        // Lane key sorts groups by index and the ungrouped lane last.
        let mut lanes: BTreeMap<(bool, usize), LaneNodes> = BTreeMap::new();
        for &node_idx in cluster {
            let group_index = context.graph.nodes[node_idx].group_index;
            let key = (group_index.is_none(), group_index.unwrap_or(0));
            let (ranked, isolated) = lanes
                .entry(key)
                .or_insert_with(|| (vec![Vec::new(); ranks.len()], Vec::new()));
            if context.is_isolated[node_idx] {
                isolated.push(node_idx);
            } else {
                let row = ranks
                    .binary_search(&context.node_ranks[node_idx])
                    .expect("rank collected from the same cluster");
                ranked[row].push(node_idx);
            }
        }

        let lanes: Vec<Block> = lanes
            .into_iter()
            .map(|((ungrouped, _), (ranked, isolated))| {
                let ranked: Vec<Vec<usize>> = ranked
                    .into_iter()
                    .map(|mut row| {
                        row.sort_by_key(|&idx| context.order_in_rank[idx]);
                        row
                    })
                    .collect();
                let mut lane = match self.alignment {
                    RowAlignment::Neighbors => self.aligned_rows_block(&ranked, context.neighbors),
                    RowAlignment::Packed => self.rows_block(ranked),
                };
                let grid = self.pack_shelves(
                    isolated
                        .into_iter()
                        .map(|idx| self.rows_block(std::iter::once(vec![idx])))
                        .collect(),
                    lane.secondary_extent,
                );
                lane.secondary_extent = lane.secondary_extent.max(grid.secondary_extent);
                lane.rows.extend(grid.rows);
                lane.is_group = !ungrouped;
                lane
            })
            .collect();

        let is_group = lanes.iter().any(|lane| lane.is_group);
        Block {
            is_group,
            ..self.pack_shelves(lanes, f32::INFINITY)
        }
    }

    /// Places ranked rows so nodes line up with their neighbours in other
    /// rows while keeping each row's order and minimum gaps.
    ///
    /// Starting from left-packed rows, alternating down and up sweeps move
    /// every row towards the median centre of each node's neighbours in the
    /// rows already swept. Each row is solved exactly as a weighted isotonic
    /// regression (pool adjacent violators), so rows never overlap or reorder.
    #[allow(clippy::too_many_lines)] // The sweep closure and final normalization read best together.
    fn aligned_rows_block(&self, rows: &[Vec<usize>], neighbors: &[Vec<usize>]) -> Block {
        let extent = |node: usize| self.axes.secondary_extent(self.node_sizes[node]);
        let mut row_of: HashMap<usize, usize> = HashMap::new();
        let mut center: HashMap<usize, f32> = HashMap::new();
        let mut lane_extent = 0.0_f32;
        for (row_idx, row) in rows.iter().enumerate() {
            let mut cursor = 0.0_f32;
            for &node in row {
                row_of.insert(node, row_idx);
                center.insert(node, cursor + extent(node) / 2.0);
                cursor += extent(node) + self.axes.secondary_gap;
            }
            lane_extent = lane_extent.max(cursor - self.axes.secondary_gap);
        }

        let align_row = |row_idx: usize, toward_upper: bool, center: &mut HashMap<usize, f32>| {
            let row = &rows[row_idx];
            let (targets, weights): (Vec<f32>, Vec<f32>) = row
                .iter()
                .map(|&node| {
                    let mut anchors: Vec<f32> = neighbors[node]
                        .iter()
                        .filter(|neighbor| {
                            row_of.get(neighbor).is_some_and(|&other_row| {
                                if toward_upper {
                                    other_row < row_idx
                                } else {
                                    other_row > row_idx
                                }
                            })
                        })
                        .map(|neighbor| center[neighbor])
                        .collect();
                    if anchors.is_empty() {
                        (center[&node], UNANCHORED_WEIGHT)
                    } else {
                        #[allow(clippy::cast_precision_loss)] // Neighbour counts are small.
                        let weight = anchors.len() as f32;
                        (median_value(&mut anchors), weight)
                    }
                })
                .unzip();
            // Offsets turn "centres at least a gap apart" into "shifted
            // centres non-decreasing", which isotonic regression solves.
            let mut offsets = Vec::with_capacity(row.len());
            let mut offset = 0.0_f32;
            for (position, &node) in row.iter().enumerate() {
                if position > 0 {
                    offset += f32::midpoint(extent(row[position - 1]), extent(node))
                        + self.axes.secondary_gap;
                }
                offsets.push(offset);
            }
            let shifted: Vec<f32> = targets
                .iter()
                .zip(&offsets)
                .map(|(target, offset)| target - offset)
                .collect();
            // Keep the row inside the widest packed row, so alignment never
            // widens the lane. Clamping preserves the non-decreasing fit.
            let (Some(&first), Some(&last), Some(&last_offset)) =
                (row.first(), row.last(), offsets.last())
            else {
                return;
            };
            let lower = extent(first) / 2.0;
            let upper = (lane_extent - extent(last) / 2.0 - last_offset).max(lower);
            for ((&node, fitted), offset) in row
                .iter()
                .zip(isotonic_regression(&shifted, &weights))
                .zip(offsets)
            {
                center.insert(node, fitted.clamp(lower, upper) + offset);
            }
        };

        for _ in 0..ALIGNMENT_SWEEPS {
            for row_idx in 1..rows.len() {
                align_row(row_idx, true, &mut center);
            }
            for row_idx in (0..rows.len().saturating_sub(1)).rev() {
                align_row(row_idx, false, &mut center);
            }
        }

        let left = |node: usize| center[&node] - extent(node) / 2.0;
        let min_left = rows
            .iter()
            .flatten()
            .map(|&node| left(node))
            .fold(f32::INFINITY, f32::min);
        let mut secondary_extent = 0.0_f32;
        let rows = rows
            .iter()
            .map(|row| BlockRow {
                extra_gap_before: 0.0,
                cells: row
                    .iter()
                    .map(|&node| {
                        let offset = left(node) - min_left;
                        secondary_extent = secondary_extent.max(offset + extent(node));
                        (node, offset)
                    })
                    .collect(),
            })
            .collect();

        Block {
            rows,
            secondary_extent,
            is_group: false,
        }
    }

    /// Places each row's nodes one after another along the secondary axis.
    fn rows_block(&self, rows: impl IntoIterator<Item = Vec<usize>>) -> Block {
        let mut secondary_extent = 0.0_f32;
        let rows = rows
            .into_iter()
            .map(|row_nodes| {
                let mut cursor = 0.0_f32;
                let cells = row_nodes
                    .into_iter()
                    .map(|node_idx| {
                        let offset = cursor;
                        cursor += self.axes.secondary_extent(self.node_sizes[node_idx])
                            + self.axes.secondary_gap;
                        (node_idx, offset)
                    })
                    .collect::<Vec<_>>();
                if !cells.is_empty() {
                    secondary_extent = secondary_extent.max(cursor - self.axes.secondary_gap);
                }
                BlockRow {
                    extra_gap_before: 0.0,
                    cells,
                }
            })
            .collect();

        Block {
            rows,
            secondary_extent,
            is_group: false,
        }
    }

    /// Packs blocks left-to-right into shelves stacked along the primary axis.
    ///
    /// The shelf extent targets a secondary/primary ratio matching
    /// [`TARGET_SCREEN_ASPECT`] but never drops below the widest block or
    /// `min_extent`, so a single cluster keeps its original placement and an
    /// infinite `min_extent` lines every block up in one shelf.
    fn pack_shelves(&self, blocks: Vec<Block>, min_extent: f32) -> Block {
        if blocks.len() <= 1 {
            return blocks.into_iter().next().unwrap_or_default();
        }

        let widest = blocks
            .iter()
            .map(|block| block.secondary_extent)
            .fold(0.0_f32, f32::max);
        let area: f32 = blocks
            .iter()
            .map(|block| {
                (block.secondary_extent + self.axes.secondary_gap)
                    * (self.primary_extent(block) + self.axes.primary_gap)
            })
            .sum();
        let target = widest
            .max(min_extent)
            .max((area * self.target_ratio).sqrt());

        let mut shelves: Vec<Vec<(f32, Block)>> = Vec::new();
        let mut cursor = 0.0_f32;
        let mut previous_is_group = false;
        for block in blocks {
            let gap = self.block_gap(previous_is_group, block.is_group);
            match shelves.last_mut() {
                Some(current) if cursor + gap + block.secondary_extent <= target => {
                    cursor += gap;
                    previous_is_group = block.is_group;
                    current.push((cursor, block));
                }
                _ => {
                    previous_is_group = block.is_group;
                    shelves.push(vec![(0.0, block)]);
                }
            }
            let (offset, block) = shelves
                .last()
                .and_then(|shelf| shelf.last())
                .expect("block was just pushed");
            cursor = offset + block.secondary_extent;
        }

        let mut packed = Block::default();
        let mut previous_shelf_has_group = false;
        for (shelf_idx, shelf) in shelves.into_iter().enumerate() {
            let shelf_has_group = shelf.iter().any(|(_, block)| block.is_group);
            let shelf_extra_gap = if shelf_idx > 0 && (shelf_has_group || previous_shelf_has_group)
            {
                Axes::group_shelf_extra_gap()
            } else {
                0.0
            };
            previous_shelf_has_group = shelf_has_group;
            let row_count = shelf
                .iter()
                .map(|(_, block)| block.rows.len())
                .max()
                .unwrap_or(0);
            let first_row = packed.rows.len();
            packed
                .rows
                .resize_with(first_row + row_count, BlockRow::default);
            for (offset, block) in shelf {
                packed.secondary_extent =
                    packed.secondary_extent.max(offset + block.secondary_extent);
                for (local_idx, row) in block.rows.into_iter().enumerate() {
                    let target_row = &mut packed.rows[first_row + local_idx];
                    target_row.extra_gap_before =
                        target_row.extra_gap_before.max(row.extra_gap_before);
                    target_row.cells.extend(
                        row.cells
                            .into_iter()
                            .map(|(node_idx, secondary)| (node_idx, offset + secondary)),
                    );
                }
            }
            if let Some(row) = packed.rows.get_mut(first_row) {
                row.extra_gap_before = row.extra_gap_before.max(shelf_extra_gap);
            }
        }
        packed
    }

    fn block_gap(&self, left_is_group: bool, right_is_group: bool) -> f32 {
        if left_is_group || right_is_group {
            self.axes.group_gap()
        } else {
            self.axes.secondary_gap
        }
    }

    /// Primary-axis extent of a block, including gaps between its rows.
    fn primary_extent(&self, block: &Block) -> f32 {
        let rows: f32 = block
            .rows
            .iter()
            .map(|row| {
                row.extra_gap_before
                    + row
                        .cells
                        .iter()
                        .map(|&(node_idx, _)| self.axes.primary_extent(self.node_sizes[node_idx]))
                        .fold(0.0_f32, f32::max)
            })
            .sum();
        #[allow(clippy::cast_precision_loss)] // Row counts are small layout values.
        let gaps = block.rows.len().saturating_sub(1) as f32 * self.axes.primary_gap;
        rows + gaps
    }
}

/// Median of `values`, averaging the two middle values for even counts.
fn median_value(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        f32::midpoint(values[middle - 1], values[middle])
    } else {
        values[middle]
    }
}

/// Weighted least-squares fit of a non-decreasing sequence to `targets`
/// (pool adjacent violators). `weights` must be positive.
fn isotonic_regression(targets: &[f32], weights: &[f32]) -> Vec<f32> {
    // (weighted sum, total weight, member count) of each pooled block.
    let mut blocks: Vec<(f32, f32, usize)> = Vec::with_capacity(targets.len());
    for (&target, &weight) in targets.iter().zip(weights) {
        blocks.push((target * weight, weight, 1));
        while let [.., (left_sum, left_weight, _), (right_sum, right_weight, _)] = blocks[..]
            && left_sum / left_weight > right_sum / right_weight
        {
            let (sum, weight, count) = blocks.pop().expect("two blocks present");
            let last = blocks.last_mut().expect("two blocks present");
            last.0 += sum;
            last.1 += weight;
            last.2 += count;
        }
    }
    blocks
        .into_iter()
        .flat_map(|(sum, weight, count)| std::iter::repeat_n(sum / weight, count))
        .collect()
}

/// Ranked rows and edge-less nodes of one lane, in that order.
type LaneNodes = (Vec<Vec<usize>>, Vec<usize>);

/// Shared per-node lookups used while building cluster blocks.
struct ClusterContext<'a> {
    graph: &'a LayoutGraph,
    node_ranks: &'a [usize],
    order_in_rank: &'a [usize],
    is_isolated: &'a [bool],
    /// Deduplicated neighbours of every node across non-self-loop edges.
    neighbors: &'a [Vec<usize>],
}

/// Resolved `(from, to)` node indices of every non-self-loop edge.
fn graph_edges(graph: &LayoutGraph) -> impl Iterator<Item = (usize, usize)> + '_ {
    graph
        .edges
        .iter()
        .filter(|edge| !edge.is_self_loop)
        .filter_map(|edge| {
            let from = *graph.node_index.get(&edge.from)?;
            let to = *graph.node_index.get(&edge.to)?;
            (from != to).then_some((from, to))
        })
}

/// Splits nodes into clusters: weakly connected components where members of
/// the same group are also treated as connected, so a group container never
/// spans two clusters. Clusters are returned in order of their smallest node
/// index, with members sorted ascending.
fn clusters(graph: &LayoutGraph) -> Vec<Vec<usize>> {
    fn find(parent: &mut [usize], mut node: usize) -> usize {
        while parent[node] != node {
            parent[node] = parent[parent[node]];
            node = parent[node];
        }
        node
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let (root_a, root_b) = (find(parent, a), find(parent, b));
        if root_a != root_b {
            // Attach to the smaller root so cluster order stays index-based.
            parent[root_a.max(root_b)] = root_a.min(root_b);
        }
    }

    let n = graph.nodes.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for (from, to) in graph_edges(graph) {
        union(&mut parent, from, to);
    }
    for group in &graph.groups {
        if let Some((&first, rest)) = group.node_indices.split_first() {
            for &member in rest {
                if first < n && member < n {
                    union(&mut parent, first, member);
                }
            }
        }
    }

    let mut cluster_of_root: Vec<Option<usize>> = vec![None; n];
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    for node_idx in 0..n {
        let root = find(&mut parent, node_idx);
        let cluster_idx = *cluster_of_root[root].get_or_insert_with(|| {
            clusters.push(Vec::new());
            clusters.len() - 1
        });
        clusters[cluster_idx].push(node_idx);
    }
    clusters
}

#[cfg(test)]
mod tests {
    use super::isotonic_regression;

    #[test]
    fn isotonic_regression_pools_violating_neighbours() {
        let fitted = isotonic_regression(&[1.0, 3.0, 2.0, 4.0], &[1.0, 1.0, 1.0, 1.0]);
        assert_eq!(fitted, vec![1.0, 2.5, 2.5, 4.0]);
    }

    #[test]
    fn isotonic_regression_respects_weights() {
        let fitted = isotonic_regression(&[10.0, 0.0], &[3.0, 1.0]);
        assert_eq!(fitted, vec![7.5, 7.5]);
    }
}
