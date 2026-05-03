//! Hierarchical (rank-based) coordinate assignment and swimlane planning.

use std::collections::BTreeMap;

use relune_core::LayoutDirection;

use crate::graph::LayoutGraph;

use super::spacing::{
    build_positioned_node, compute_graph_bounds, mirror_positioned_nodes_for_direction,
};
use super::{LayoutConfig, LayoutError, NodeSize, PositionedNode};

/// Assign coordinates to nodes based on their ranks and order.
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::suboptimal_flops)]
pub(super) fn assign_coordinates(
    graph: &LayoutGraph,
    ordered_nodes: &[Vec<usize>],
    config: &LayoutConfig,
    node_sizes: &[NodeSize],
) -> Result<(Vec<PositionedNode>, f32, f32), LayoutError> {
    let n = graph.nodes.len();
    // Index by graph node index so that positioned_nodes[node_idx] correctly
    // addresses the corresponding node (needed by resolve_rank_collisions).
    let mut positioned_slots: Vec<Option<PositionedNode>> = vec![None; n];

    let is_horizontal = matches!(
        config.direction,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft
    );
    let rank_primary_offsets = compute_rank_primary_offsets(ordered_nodes, node_sizes, config);

    // Group-aware "swimlane" placement: each group occupies a contiguous range
    // on the secondary axis across all ranks, so group bounding boxes never
    // overlap. When the graph has no groups this collapses to the original
    // single-lane behaviour.
    let swimlanes = compute_swimlanes(graph, ordered_nodes, node_sizes, config, is_horizontal);

    // Reorder nodes within each rank so members of the same lane are contiguous,
    // while preserving the existing crossing-minimised order within each lane.
    let ordered_nodes_owned: Vec<Vec<usize>> = ordered_nodes
        .iter()
        .map(|rank_nodes| {
            let mut sorted: Vec<usize> = rank_nodes.clone();
            sorted.sort_by_key(|&node_idx| {
                swimlanes.lane_order_index_for_node(graph.nodes[node_idx].group_index)
            });
            sorted
        })
        .collect();
    let ordered_nodes: &[Vec<usize>] = &ordered_nodes_owned;

    for (rank_idx, rank_nodes) in ordered_nodes.iter().enumerate() {
        // Per-rank cursor inside each lane.
        let mut lane_cursor: BTreeMap<Option<usize>, f32> = BTreeMap::new();

        for &node_idx in rank_nodes {
            let node = &graph.nodes[node_idx];
            let node_size = node_sizes[node_idx];
            let primary = rank_primary_offsets[rank_idx];

            let lane_start = swimlanes.lane_start(node.group_index);
            let cursor = lane_cursor.entry(node.group_index).or_insert(lane_start);
            let secondary = *cursor;
            let advance = if is_horizontal {
                node_size.height + config.vertical_spacing
            } else {
                node_size.width + config.horizontal_spacing
            };
            *cursor = secondary + advance;

            let (node_x, node_y) = if is_horizontal {
                (primary, secondary)
            } else {
                (secondary, primary)
            };

            positioned_slots[node_idx] = Some(build_positioned_node(
                node,
                node_x,
                node_y,
                node_size.width,
                node_size.height,
                config.show_columns,
            ));
        }
    }

    // Every graph node must have been assigned a position above.
    let mut positioned_nodes = Vec::with_capacity(positioned_slots.len());
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

    resolve_rank_collisions(
        &mut positioned_nodes,
        ordered_nodes,
        graph,
        config,
        &swimlanes,
        is_horizontal,
    );
    let graph_bounds = compute_graph_bounds(&positioned_nodes, config);
    mirror_positioned_nodes_for_direction(&mut positioned_nodes, graph_bounds, config.direction);

    let (width, height) = compute_graph_bounds(&positioned_nodes, config);
    Ok((positioned_nodes, width, height))
}

fn compute_rank_primary_offsets(
    ordered_nodes: &[Vec<usize>],
    node_sizes: &[NodeSize],
    config: &LayoutConfig,
) -> Vec<f32> {
    let is_horizontal = matches!(
        config.direction,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft
    );
    let mut offsets = Vec::with_capacity(ordered_nodes.len());
    let mut primary = if is_horizontal {
        config.origin_x
    } else {
        config.origin_y
    };
    let gap = if is_horizontal {
        config.horizontal_spacing
    } else {
        config.vertical_spacing
    };

    for rank_nodes in ordered_nodes {
        offsets.push(primary);
        let extent = rank_nodes
            .iter()
            .map(|&node_idx| {
                if is_horizontal {
                    node_sizes[node_idx].width
                } else {
                    node_sizes[node_idx].height
                }
            })
            .fold(0.0, f32::max);
        primary += extent + gap;
    }

    offsets
}

fn resolve_rank_collisions(
    positioned_nodes: &mut [PositionedNode],
    ordered_nodes: &[Vec<usize>],
    graph: &LayoutGraph,
    config: &LayoutConfig,
    swimlanes: &Swimlanes,
    is_horizontal: bool,
) {
    let spacing = if is_horizontal {
        config.vertical_spacing
    } else {
        config.horizontal_spacing
    };
    let group_gap = swimlanes.group_gap;

    for rank_nodes in ordered_nodes {
        let mut previous_end: Option<f32> = None;
        let mut previous_group: Option<Option<usize>> = None;
        for &node_idx in rank_nodes {
            let current_group = graph.nodes[node_idx].group_index;
            let node = &mut positioned_nodes[node_idx];
            let coordinate = if is_horizontal {
                &mut node.y
            } else {
                &mut node.x
            };
            let extent = if is_horizontal {
                node.height
            } else {
                node.width
            };

            if let Some(end) = previous_end {
                let crossing_group_boundary =
                    matches!(previous_group, Some(prev) if prev != current_group);
                let gap = if crossing_group_boundary {
                    spacing.max(group_gap)
                } else {
                    spacing
                };
                let required = end + gap;
                if *coordinate < required {
                    *coordinate = required;
                }
            }

            previous_end = Some(*coordinate + extent);
            previous_group = Some(current_group);
        }
    }
}

/// Per-group "swimlane" placement plan along the secondary axis.
///
/// Groups are assigned disjoint ranges along the secondary axis (Y for
/// horizontal layouts, X for vertical layouts) so that group bounding boxes
/// never overlap. Ungrouped nodes are placed in a sentinel lane that always
/// sorts last.
#[derive(Debug, Clone)]
struct Swimlanes {
    /// Canonical lane order. Each entry identifies a `group_index` (`Some(idx)`)
    /// or the ungrouped sentinel (`None`).
    order: Vec<Option<usize>>,
    /// Position on the secondary axis at which the lane begins.
    starts: BTreeMap<Option<usize>, f32>,
    /// Extra spacing inserted between adjacent lanes.
    group_gap: f32,
}

impl Swimlanes {
    fn lane_start(&self, group_index: Option<usize>) -> f32 {
        self.starts.get(&group_index).copied().unwrap_or(0.0)
    }

    /// Sort key used to make nodes from the same lane contiguous within a rank.
    /// Ungrouped nodes go to the back so they form a single trailing lane.
    fn lane_order_index_for_node(&self, group_index: Option<usize>) -> usize {
        self.order
            .iter()
            .position(|g| *g == group_index)
            .unwrap_or(self.order.len())
    }
}

#[allow(clippy::cast_precision_loss)]
fn compute_swimlanes(
    graph: &LayoutGraph,
    ordered_nodes: &[Vec<usize>],
    node_sizes: &[NodeSize],
    config: &LayoutConfig,
    is_horizontal: bool,
) -> Swimlanes {
    let secondary_origin = if is_horizontal {
        config.origin_y
    } else {
        config.origin_x
    };
    let spacing = if is_horizontal {
        config.vertical_spacing
    } else {
        config.horizontal_spacing
    };
    let group_gap = spacing * 1.5;

    // No groups: collapse to a single lane that begins at the secondary origin.
    // The lane carries the ungrouped sentinel key.
    if graph.groups.is_empty() {
        let mut starts = BTreeMap::new();
        starts.insert(None, secondary_origin);
        return Swimlanes {
            order: vec![None],
            starts,
            group_gap,
        };
    }

    // Canonical lane order: groups in their build order, then the ungrouped
    // sentinel last.
    let mut order: Vec<Option<usize>> = (0..graph.groups.len()).map(Some).collect();
    order.push(None);

    // Per-rank secondary span for each lane.
    // Span = sum of node extents + spacing * (count - 1) for nodes of that lane in that rank.
    let mut lane_extent: BTreeMap<Option<usize>, f32> = BTreeMap::new();
    for rank_nodes in ordered_nodes {
        let mut per_rank: BTreeMap<Option<usize>, (f32, usize)> = BTreeMap::new();
        for &node_idx in rank_nodes {
            let group_index = graph.nodes[node_idx].group_index;
            let extent = if is_horizontal {
                node_sizes[node_idx].height
            } else {
                node_sizes[node_idx].width
            };
            let entry = per_rank.entry(group_index).or_insert((0.0, 0));
            entry.0 += extent;
            entry.1 += 1;
        }
        for (group_index, (sum, count)) in per_rank {
            let span = if count == 0 {
                0.0
            } else {
                spacing.mul_add((count - 1) as f32, sum)
            };
            let slot = lane_extent.entry(group_index).or_insert(0.0);
            if span > *slot {
                *slot = span;
            }
        }
    }

    // Lane starts: walk canonical order, accumulating extents and gaps.
    let mut starts = BTreeMap::new();
    let mut cursor = secondary_origin;
    let mut emitted_any = false;
    for lane in &order {
        let extent = lane_extent.get(lane).copied().unwrap_or(0.0);
        if extent <= 0.0 {
            // No nodes in this lane: still record a start so lookups don't
            // panic, but do not advance the cursor.
            starts.insert(*lane, cursor);
            continue;
        }
        if emitted_any {
            cursor += group_gap;
        }
        starts.insert(*lane, cursor);
        cursor += extent;
        emitted_any = true;
    }

    Swimlanes {
        order,
        starts,
        group_gap,
    }
}
