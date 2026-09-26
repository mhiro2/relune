//! Hierarchical (rank-based) coordinate assignment and component packing.
//!
//! Nodes are split into weakly connected components, and each component keeps
//! its own rank rows. Components (and group containers built from them) are
//! then packed into "shelves" along the secondary axis, wrapping to a new shelf
//! once a target extent derived from the total area is reached. This keeps
//! schemas with many unrelated tables (or no foreign keys at all) close to a
//! screen-friendly aspect ratio instead of one unbounded row.

use std::collections::BTreeMap;

use relune_core::LayoutDirection;

use crate::graph::LayoutGraph;

use super::spacing::{
    build_positioned_node, compute_graph_bounds, mirror_positioned_nodes_for_direction,
};
use super::{
    GROUP_PADDING, GROUP_TOP_PADDING, LayoutConfig, LayoutError, NodeSize, PositionedNode,
};

/// Preferred on-screen width / height ratio of a packed layout.
const TARGET_SCREEN_ASPECT: f32 = 1.6;

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

/// Assign coordinates to nodes based on their ranks and order.
pub(super) fn assign_coordinates(
    graph: &LayoutGraph,
    node_ranks: &[usize],
    ordered_nodes: &[Vec<usize>],
    config: &LayoutConfig,
    node_sizes: &[NodeSize],
) -> Result<HierarchicalPlacement, LayoutError> {
    let axes = Axes::new(config);
    let packer = Packer {
        node_sizes,
        axes,
        target_ratio: axes.target_secondary_to_primary_ratio(),
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
                config.show_columns,
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

    /// Gap between two group containers placed next to each other.
    fn group_gap(self) -> f32 {
        self.secondary_gap * 1.5
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
        for (from, to) in graph_edges(graph) {
            is_isolated[from] = false;
            is_isolated[to] = false;
        }

        let context = ClusterContext {
            graph,
            node_ranks,
            order_in_rank: &order_in_rank,
            is_isolated: &is_isolated,
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
                let mut lane = self.rows_block(ranked.into_iter().map(|mut row| {
                    row.sort_by_key(|&idx| context.order_in_rank[idx]);
                    row
                }));
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

/// Ranked rows and edge-less nodes of one lane, in that order.
type LaneNodes = (Vec<Vec<usize>>, Vec<usize>);

/// Shared per-node lookups used while building cluster blocks.
struct ClusterContext<'a> {
    graph: &'a LayoutGraph,
    node_ranks: &'a [usize],
    order_in_rank: &'a [usize],
    is_isolated: &'a [bool],
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
