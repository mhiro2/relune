//! Port assignment for routed edges.

use std::collections::BTreeMap;

use relune_core::LayoutDirection;

use crate::graph::LayoutGraph;
use crate::layout::{LayoutConfig, PositionedNode};
use crate::route::AttachmentSide;

/// Base gap between parallel self-loop edges.
const SELF_LOOP_SLOT_GAP: f32 = 18.0;
/// Base gap between ports that share the same node side.
const BASE_SLOT_GAP: f32 = 14.0;
/// Minimum slot gap used when many edges share a side.
const MIN_SLOT_GAP: f32 = 10.0;
/// Maximum slot gap.
const MAX_SLOT_GAP: f32 = 20.0;
/// Additional distance treated as a near-neighbor case on the primary flow axis.
const NEAR_NODE_PADDING: f32 = 24.0;

#[derive(Debug, Clone)]
pub(crate) enum EdgePortAssignment {
    Regular(RegularPortAssignment),
    SelfLoop(SelfLoopPortAssignment),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RegularPortAssignment {
    pub source_side: AttachmentSide,
    pub target_side: AttachmentSide,
    pub source_slot_offset: f32,
    pub source_slot_index: usize,
    pub source_slot_count: usize,
    pub target_slot_offset: f32,
    pub target_slot_index: usize,
    pub target_slot_count: usize,
    pub source_row_offset: f32,
    pub target_row_offset: f32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SelfLoopPortAssignment {
    pub radius_offset: f32,
}

#[derive(Debug, Clone)]
struct EndpointCandidate {
    edge_index: usize,
    is_source: bool,
    remote_order: f32,
    row_order: f32,
    remote_primary: f32,
    stable_key: String,
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub(crate) fn assign_edge_ports(
    graph: &LayoutGraph,
    positioned_nodes: &[PositionedNode],
    config: &LayoutConfig,
) -> Vec<Option<EdgePortAssignment>> {
    let node_by_id: BTreeMap<&str, &PositionedNode> = positioned_nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();

    let mut assignments = vec![None; graph.edges.len()];
    let mut endpoint_groups: BTreeMap<(String, AttachmentSide), Vec<EndpointCandidate>> =
        BTreeMap::new();
    let mut self_loop_counts = BTreeMap::new();

    for (edge_index, edge) in graph.edges.iter().enumerate() {
        if edge.is_self_loop {
            let slot_index = self_loop_counts
                .entry(edge.from.clone())
                .and_modify(|count| *count += 1usize)
                .or_insert(0usize);
            assignments[edge_index] = Some(EdgePortAssignment::SelfLoop(SelfLoopPortAssignment {
                radius_offset: self_loop_radius_offset(*slot_index),
            }));
            continue;
        }

        let Some(source_node) = node_by_id.get(edge.from.as_str()) else {
            continue;
        };
        let Some(target_node) = node_by_id.get(edge.to.as_str()) else {
            continue;
        };

        let (source_side, target_side) = choose_regular_sides(source_node, target_node, config);
        let source_row_offset =
            column_y_offset_from_center(source_node, &edge.from_columns, config);
        let target_row_offset = column_y_offset_from_center(target_node, &edge.to_columns, config);

        assignments[edge_index] = Some(EdgePortAssignment::Regular(RegularPortAssignment {
            source_side,
            target_side,
            source_slot_offset: 0.0,
            source_slot_index: 0,
            source_slot_count: 1,
            target_slot_offset: 0.0,
            target_slot_index: 0,
            target_slot_count: 1,
            source_row_offset,
            target_row_offset,
        }));

        let source_center = node_center(source_node);
        let target_center = node_center(target_node);
        let source_key = (edge.from.clone(), source_side);
        let target_key = (edge.to.clone(), target_side);
        let stable_key = stable_edge_key(edge.from.as_str(), edge.to.as_str(), edge_index);

        endpoint_groups
            .entry(source_key)
            .or_default()
            .push(EndpointCandidate {
                edge_index,
                is_source: true,
                remote_order: secondary_flow_coordinate(target_center, config.direction),
                row_order: source_row_offset,
                remote_primary: primary_flow_coordinate(target_center, config.direction),
                stable_key: stable_key.clone(),
            });
        endpoint_groups
            .entry(target_key)
            .or_default()
            .push(EndpointCandidate {
                edge_index,
                is_source: false,
                remote_order: secondary_flow_coordinate(source_center, config.direction),
                row_order: target_row_offset,
                remote_primary: primary_flow_coordinate(source_center, config.direction),
                stable_key,
            });
    }

    for ((node_id, side), candidates) in &mut endpoint_groups {
        candidates.sort_by(|left, right| {
            left.remote_order
                .total_cmp(&right.remote_order)
                .then_with(|| left.row_order.total_cmp(&right.row_order))
                .then_with(|| left.remote_primary.total_cmp(&right.remote_primary))
                .then_with(|| left.stable_key.cmp(&right.stable_key))
        });

        let slot_total = candidates.len();

        // For horizontal sides (East/West), `row_offset` is also added to the
        // port Y in `apply_endpoint_offsets`, so slot and row offsets are in
        // the same axis and may cancel each other out. We solve a constrained
        // isotonic regression over the row offsets, preserving the
        // remote-order monotonicity (to keep source-side fan-outs free of
        // self-crossings) while pulling each port toward its column row.
        //
        // For vertical sides (North/South) the column offset does not affect
        // the port X, so the naive centered distribution stays correct.
        if side.is_horizontal() && slot_total >= 2 {
            let max_offset = node_by_id
                .get(node_id.as_str())
                .map_or(f32::INFINITY, |node| (node.height / 2.0 - 4.0).max(0.0));
            let policy = centered_slot_gap(slot_total);
            #[allow(clippy::cast_precision_loss)]
            let span_cap = if max_offset.is_finite() {
                (2.0 * max_offset) / (slot_total - 1) as f32
            } else {
                f32::INFINITY
            };
            let effective_gap = policy.min(span_cap).max(0.0);

            let desired: Vec<f32> = candidates.iter().map(|c| c.row_order).collect();
            let assigned = pav_pack_with_min_gap(&desired, effective_gap, -max_offset, max_offset);

            for (slot_index, candidate) in candidates.iter().enumerate() {
                let effective_offset = assigned[slot_index];
                let Some(Some(EdgePortAssignment::Regular(assignment))) =
                    assignments.get_mut(candidate.edge_index)
                else {
                    continue;
                };
                // The renderer computes port_y = center + slot_offset + row_offset;
                // choose slot_offset so the sum equals the packed effective offset.
                let slot_offset = effective_offset - candidate.row_order;
                if candidate.is_source {
                    assignment.source_slot_offset = slot_offset;
                    assignment.source_slot_index = slot_index;
                    assignment.source_slot_count = slot_total;
                } else {
                    assignment.target_slot_offset = slot_offset;
                    assignment.target_slot_index = slot_index;
                    assignment.target_slot_count = slot_total;
                }
            }
        } else {
            for (slot_index, candidate) in candidates.iter().enumerate() {
                let slot_offset = centered_slot_offset(slot_index, slot_total);
                let Some(Some(EdgePortAssignment::Regular(assignment))) =
                    assignments.get_mut(candidate.edge_index)
                else {
                    continue;
                };
                if candidate.is_source {
                    assignment.source_slot_offset = slot_offset;
                    assignment.source_slot_index = slot_index;
                    assignment.source_slot_count = slot_total;
                } else {
                    assignment.target_slot_offset = slot_offset;
                    assignment.target_slot_index = slot_index;
                    assignment.target_slot_count = slot_total;
                }
            }
        }
    }

    assignments
}

#[must_use]
pub(crate) fn column_y_offset_from_center(
    node: &PositionedNode,
    edge_columns: &[String],
    config: &LayoutConfig,
) -> f32 {
    if edge_columns.is_empty() || node.columns.is_empty() {
        return 0.0;
    }
    let Some(col_index) = node
        .columns
        .iter()
        .position(|column| edge_columns.contains(&column.name))
    else {
        return 0.0;
    };
    let center_y = node.height / 2.0;
    #[allow(clippy::cast_precision_loss)]
    let column_y = (col_index as f32).mul_add(
        config.column_height,
        config.node_padding + config.header_height,
    ) + config.column_height / 2.0;
    let offset = column_y - center_y;
    let max_offset = (center_y - 4.0).max(0.0);
    offset.clamp(-max_offset, max_offset)
}

fn choose_regular_sides(
    source: &PositionedNode,
    target: &PositionedNode,
    config: &LayoutConfig,
) -> (AttachmentSide, AttachmentSide) {
    let source_center = node_center(source);
    let target_center = node_center(target);
    let dx = target_center.0 - source_center.0;
    let dy = target_center.1 - source_center.1;

    if should_use_cross_axis(source, target, dx, dy, config.direction) {
        return cross_axis_sides(dx, dy, config.direction);
    }

    if is_reverse_flow(dx, dy, config.direction) {
        return opposite_primary_sides(config.direction);
    }

    preferred_primary_sides(config.direction)
}

fn should_use_cross_axis(
    source: &PositionedNode,
    target: &PositionedNode,
    dx: f32,
    dy: f32,
    direction: LayoutDirection,
) -> bool {
    let (primary_delta, secondary_delta, primary_extent) = match direction {
        LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => {
            (dy.abs(), dx.abs(), source.height.max(target.height))
        }
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => {
            (dx.abs(), dy.abs(), source.width.max(target.width))
        }
    };

    let same_rank_threshold = primary_extent * 0.5;
    let near_threshold = primary_extent + NEAR_NODE_PADDING;

    primary_delta <= same_rank_threshold
        || (primary_delta <= near_threshold && secondary_delta > primary_delta)
}

fn is_reverse_flow(dx: f32, dy: f32, direction: LayoutDirection) -> bool {
    let preferred_sign = match direction {
        LayoutDirection::TopToBottom | LayoutDirection::LeftToRight => 1.0,
        LayoutDirection::BottomToTop | LayoutDirection::RightToLeft => -1.0,
    };
    let primary_delta = match direction {
        LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => dy,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => dx,
    };
    primary_delta * preferred_sign < 0.0
}

const fn preferred_primary_sides(direction: LayoutDirection) -> (AttachmentSide, AttachmentSide) {
    match direction {
        LayoutDirection::TopToBottom => (AttachmentSide::South, AttachmentSide::North),
        LayoutDirection::BottomToTop => (AttachmentSide::North, AttachmentSide::South),
        LayoutDirection::LeftToRight => (AttachmentSide::East, AttachmentSide::West),
        LayoutDirection::RightToLeft => (AttachmentSide::West, AttachmentSide::East),
    }
}

const fn opposite_primary_sides(direction: LayoutDirection) -> (AttachmentSide, AttachmentSide) {
    match direction {
        LayoutDirection::TopToBottom => (AttachmentSide::North, AttachmentSide::South),
        LayoutDirection::BottomToTop => (AttachmentSide::South, AttachmentSide::North),
        LayoutDirection::LeftToRight => (AttachmentSide::West, AttachmentSide::East),
        LayoutDirection::RightToLeft => (AttachmentSide::East, AttachmentSide::West),
    }
}

fn cross_axis_sides(
    dx: f32,
    dy: f32,
    direction: LayoutDirection,
) -> (AttachmentSide, AttachmentSide) {
    match direction {
        LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => {
            if dx >= 0.0 {
                (AttachmentSide::East, AttachmentSide::West)
            } else {
                (AttachmentSide::West, AttachmentSide::East)
            }
        }
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => {
            if dy >= 0.0 {
                (AttachmentSide::South, AttachmentSide::North)
            } else {
                (AttachmentSide::North, AttachmentSide::South)
            }
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn self_loop_radius_offset(slot_index: usize) -> f32 {
    slot_index as f32 * SELF_LOOP_SLOT_GAP
}

#[allow(clippy::cast_precision_loss)]
fn centered_slot_gap(slot_total: usize) -> f32 {
    if slot_total <= 2 {
        BASE_SLOT_GAP
    } else {
        (BASE_SLOT_GAP * 2.0 / (slot_total as f32).sqrt()).clamp(MIN_SLOT_GAP, MAX_SLOT_GAP)
    }
}

#[allow(clippy::cast_precision_loss)]
fn centered_slot_offset(slot_index: usize, slot_total: usize) -> f32 {
    let gap = centered_slot_gap(slot_total);
    let center = (slot_total.saturating_sub(1)) as f32 * 0.5;
    (slot_index as f32 - center) * gap
}

/// Constrained isotonic regression: pick values `assigned[i]` minimizing
/// `Σ (assigned[i] - desired[i])²` subject to
/// `assigned[i] >= assigned[i-1] + gap`, then shift the resulting block to
/// fit within `[min_bound, max_bound]`. A uniform shift preserves min-gap,
/// so the only way min-gap is violated in the output is when the required
/// span `(n − 1) * gap` exceeds `max_bound - min_bound`; callers are
/// expected to choose `gap` so that does not happen.
///
/// The transformation `z[i] = desired[i] - i * gap` reduces the problem to
/// unconstrained isotonic regression on `z`, solved by Pool Adjacent
/// Violators in O(n).
#[allow(clippy::cast_precision_loss)]
fn pav_pack_with_min_gap(desired: &[f32], gap: f32, min_bound: f32, max_bound: f32) -> Vec<f32> {
    let n = desired.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![desired[0].clamp(min_bound, max_bound)];
    }

    // PAV on z[i] = desired[i] - i * gap.
    let mut blocks: Vec<(f32, usize)> = Vec::with_capacity(n);
    for (i, &d) in desired.iter().enumerate() {
        let z = (i as f32).mul_add(-gap, d);
        let mut cur_mean = z;
        let mut cur_weight = 1usize;
        while let Some(&(prev_mean, prev_weight)) = blocks.last() {
            if prev_mean <= cur_mean {
                break;
            }
            blocks.pop();
            let total_weight = prev_weight + cur_weight;
            cur_mean = prev_mean.mul_add(prev_weight as f32, cur_mean * cur_weight as f32)
                / total_weight as f32;
            cur_weight = total_weight;
        }
        blocks.push((cur_mean, cur_weight));
    }

    let mut assigned = Vec::with_capacity(n);
    let mut idx = 0usize;
    for (mean, weight) in blocks {
        for _ in 0..weight {
            assigned.push((idx as f32).mul_add(gap, mean));
            idx += 1;
        }
    }

    // Uniform shift to fit bounds; preserves min-gap.
    let low_excess = min_bound - assigned[0];
    if low_excess > 0.0 {
        for a in &mut assigned {
            *a += low_excess;
        }
    }
    let high_excess = assigned[n - 1] - max_bound;
    if high_excess > 0.0 {
        for a in &mut assigned {
            *a -= high_excess;
        }
    }

    // Pathological fallback: required span exceeds available bounds. Clamp
    // individually (min-gap may be sacrificed, which is unavoidable here).
    for a in &mut assigned {
        *a = a.clamp(min_bound, max_bound);
    }

    assigned
}

const fn node_center(node: &PositionedNode) -> (f32, f32) {
    (node.x + node.width / 2.0, node.y + node.height / 2.0)
}

const fn primary_flow_coordinate(center: (f32, f32), direction: LayoutDirection) -> f32 {
    match direction {
        LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => center.1,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => center.0,
    }
}

const fn secondary_flow_coordinate(center: (f32, f32), direction: LayoutDirection) -> f32 {
    match direction {
        LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => center.0,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => center.1,
    }
}

fn stable_edge_key(from: &str, to: &str, edge_index: usize) -> String {
    format!("{from}\u{0}{to}\u{0}{edge_index}")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use relune_core::NodeKind;

    use super::{
        EdgePortAssignment, assign_edge_ports, centered_slot_offset, choose_regular_sides,
        column_y_offset_from_center,
    };
    use crate::graph::{LayoutEdge, LayoutGraph};
    use crate::layout::{
        ColumnFlags, ColumnRelationFlags, LayoutConfig, PositionedColumn, PositionedNode,
    };
    use crate::route::AttachmentSide;
    use relune_core::{EdgeKind, LayoutDirection, layout::Cardinality};

    fn node(id: &str, x: f32, y: f32) -> PositionedNode {
        PositionedNode {
            id: id.to_string(),
            label: id.to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x,
            y,
            width: 120.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        }
    }

    fn edge(from: &str, to: &str, from_columns: &[&str], to_columns: &[&str]) -> LayoutEdge {
        LayoutEdge {
            from: from.to_string(),
            to: to.to_string(),
            name: None,
            from_columns: from_columns
                .iter()
                .map(|column| (*column).to_string())
                .collect(),
            to_columns: to_columns
                .iter()
                .map(|column| (*column).to_string())
                .collect(),
            kind: EdgeKind::ForeignKey,
            is_self_loop: false,
            nullable: false,
            target_cardinality: Cardinality::One,
            is_collapsed_join: false,
            collapsed_join_table: None,
        }
    }

    #[test]
    fn test_choose_regular_sides_prefers_flow_axis_when_primary_gap_is_clear() {
        let source = node("source", 0.0, 0.0);
        let target = node("target", 260.0, 220.0);
        let config = LayoutConfig {
            direction: LayoutDirection::TopToBottom,
            ..Default::default()
        };

        let (source_side, target_side) = choose_regular_sides(&source, &target, &config);

        assert_eq!(source_side, AttachmentSide::South);
        assert_eq!(target_side, AttachmentSide::North);
    }

    #[test]
    fn test_choose_regular_sides_uses_cross_axis_for_same_rank_nodes() {
        let source = node("source", 0.0, 0.0);
        let target = node("target", 260.0, 10.0);
        let config = LayoutConfig {
            direction: LayoutDirection::TopToBottom,
            ..Default::default()
        };

        let (source_side, target_side) = choose_regular_sides(&source, &target, &config);

        assert_eq!(source_side, AttachmentSide::East);
        assert_eq!(target_side, AttachmentSide::West);
    }

    #[test]
    fn test_choose_regular_sides_uses_opposite_primary_sides_for_reverse_flow() {
        let source = node("source", 0.0, 240.0);
        let target = node("target", 40.0, 0.0);
        let config = LayoutConfig {
            direction: LayoutDirection::TopToBottom,
            ..Default::default()
        };

        let (source_side, target_side) = choose_regular_sides(&source, &target, &config);

        assert_eq!(source_side, AttachmentSide::North);
        assert_eq!(target_side, AttachmentSide::South);
    }

    #[test]
    fn test_assign_edge_ports_orders_slots_by_remote_position() {
        let graph = LayoutGraph {
            nodes: Vec::new(),
            edges: vec![
                edge("center", "left", &[], &[]),
                edge("center", "right", &[], &[]),
            ],
            groups: Vec::new(),
            node_index: BTreeMap::new(),
            reverse_index: BTreeMap::new(),
        };
        let positioned_nodes = vec![
            node("center", 200.0, 0.0),
            node("left", 0.0, 220.0),
            node("right", 440.0, 220.0),
        ];
        let config = LayoutConfig {
            direction: LayoutDirection::TopToBottom,
            ..Default::default()
        };

        let assignments = assign_edge_ports(&graph, &positioned_nodes, &config);

        let left_assignment = match assignments[0].as_ref().expect("assignment") {
            EdgePortAssignment::Regular(assignment) => assignment,
            EdgePortAssignment::SelfLoop(_) => panic!("expected regular assignment"),
        };
        let right_assignment = match assignments[1].as_ref().expect("assignment") {
            EdgePortAssignment::Regular(assignment) => assignment,
            EdgePortAssignment::SelfLoop(_) => panic!("expected regular assignment"),
        };

        assert!(left_assignment.source_slot_offset < right_assignment.source_slot_offset);
    }

    #[test]
    fn test_assign_edge_ports_keeps_parallel_slots_stable_across_runs() {
        let graph = LayoutGraph {
            nodes: Vec::new(),
            edges: vec![
                edge("posts", "users", &["author_id"], &["id"]),
                edge("posts", "users", &["reviewer_id"], &["id"]),
            ],
            groups: Vec::new(),
            node_index: BTreeMap::new(),
            reverse_index: BTreeMap::new(),
        };
        let positioned_nodes = vec![node("posts", 320.0, 0.0), node("users", 0.0, 220.0)];
        let config = LayoutConfig {
            direction: LayoutDirection::TopToBottom,
            ..Default::default()
        };

        let first = assign_edge_ports(&graph, &positioned_nodes, &config);
        let second = assign_edge_ports(&graph, &positioned_nodes, &config);

        assert_eq!(
            format!("{:?}", first.iter().collect::<Vec<_>>()),
            format!("{:?}", second.iter().collect::<Vec<_>>())
        );
    }

    #[test]
    fn test_column_y_offset_from_center_uses_matching_column_row() {
        let config = LayoutConfig::default();
        let node = PositionedNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            kind: NodeKind::Table,
            columns: vec![
                PositionedColumn {
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    flags: ColumnFlags {
                        nullable: false,
                        relation: ColumnRelationFlags {
                            is_primary_key: true,
                            is_foreign_key: false,
                            is_indexed: false,
                        },
                    },
                },
                PositionedColumn {
                    name: "author_id".to_string(),
                    data_type: "int".to_string(),
                    flags: ColumnFlags {
                        nullable: false,
                        relation: ColumnRelationFlags {
                            is_primary_key: false,
                            is_foreign_key: true,
                            is_indexed: true,
                        },
                    },
                },
            ],
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 84.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        };

        let offset = column_y_offset_from_center(&node, &["author_id".to_string()], &config);

        assert!(offset > 0.0);
    }

    #[test]
    fn test_centered_slot_offset_shrinks_for_large_side_fan_out() {
        let gap_two = (centered_slot_offset(1, 2) - centered_slot_offset(0, 2)).abs();
        let gap_six = (centered_slot_offset(1, 6) - centered_slot_offset(0, 6)).abs();

        assert!(gap_six < gap_two);
        assert!(gap_six >= 10.0);
    }

    fn node_with_fk_columns(
        id: &str,
        x: f32,
        y: f32,
        width: f32,
        columns: &[&str],
    ) -> PositionedNode {
        // Match LayoutConfig defaults: node_padding=8, header_height=32, column_height=18.
        #[allow(clippy::cast_precision_loss)]
        let height = (columns.len() as f32).mul_add(18.0, 16.0 + 32.0);
        PositionedNode {
            id: id.to_string(),
            label: id.to_string(),
            kind: NodeKind::Table,
            columns: columns
                .iter()
                .map(|name| PositionedColumn {
                    name: (*name).to_string(),
                    data_type: "int".to_string(),
                    flags: ColumnFlags {
                        nullable: false,
                        relation: ColumnRelationFlags {
                            is_primary_key: false,
                            is_foreign_key: true,
                            is_indexed: false,
                        },
                    },
                })
                .collect(),
            x,
            y,
            width,
            height,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        }
    }

    #[test]
    fn test_assign_edge_ports_horizontal_join_table_avoids_port_collapse() {
        // LTR + join table with 2 FKs whose column rows and target y positions
        // are in reverse order. Before the isotonic-regression fix this caused
        // the source-side ports to collapse within a few pixels. The packed
        // ports must both preserve the minimum spacing AND the remote_order
        // monotonicity (so source fan-outs still avoid self-crossings).
        let join = node_with_fk_columns(
            "product_categories",
            0.0,
            0.0,
            245.0,
            &["product_id", "category_id", "is_primary", "sort_order"],
        );
        let categories = node("categories", 500.0, 160.0);
        let products = node("products", 500.0, 600.0);
        let graph = LayoutGraph {
            nodes: Vec::new(),
            edges: vec![
                edge("product_categories", "products", &["product_id"], &["id"]),
                edge(
                    "product_categories",
                    "categories",
                    &["category_id"],
                    &["id"],
                ),
            ],
            groups: Vec::new(),
            node_index: BTreeMap::new(),
            reverse_index: BTreeMap::new(),
        };
        let positioned_nodes = vec![join, categories, products];
        let config = LayoutConfig {
            direction: LayoutDirection::LeftToRight,
            ..Default::default()
        };

        let assignments = assign_edge_ports(&graph, &positioned_nodes, &config);

        let to_products = match assignments[0].as_ref().expect("assignment") {
            EdgePortAssignment::Regular(assignment) => assignment,
            EdgePortAssignment::SelfLoop(_) => panic!("expected regular assignment"),
        };
        let to_categories = match assignments[1].as_ref().expect("assignment") {
            EdgePortAssignment::Regular(assignment) => assignment,
            EdgePortAssignment::SelfLoop(_) => panic!("expected regular assignment"),
        };

        assert_eq!(to_products.source_side, AttachmentSide::East);
        assert_eq!(to_categories.source_side, AttachmentSide::East);

        // The renderer computes port_y = center + slot_offset + row_offset on
        // horizontal sides, so this sum is the quantity callers actually see.
        let eff_products = to_products.source_slot_offset + to_products.source_row_offset;
        let eff_categories = to_categories.source_slot_offset + to_categories.source_row_offset;

        assert!(
            (eff_products - eff_categories).abs() >= 13.5,
            "source ports collapsed: categories={eff_categories}, products={eff_products}"
        );
        assert!(
            eff_categories < eff_products,
            "remote_order monotonicity violated: categories={eff_categories}, products={eff_products}"
        );
    }

    #[test]
    fn test_assign_edge_ports_vertical_side_uses_centered_distribution() {
        // TTB with targets well below the source: the source side resolves to
        // South (vertical). The horizontal-only packing branch must not touch
        // these slots — slot_offsets must remain the naive centered values,
        // independent of the column rows that each edge attaches to.
        let top = node_with_fk_columns("top", 0.0, 0.0, 240.0, &["a_id", "b_id", "extra"]);
        let left_target = node("left", -400.0, 600.0);
        let right_target = node("right", 400.0, 600.0);
        let graph = LayoutGraph {
            nodes: Vec::new(),
            edges: vec![
                edge("top", "left", &["a_id"], &["id"]),
                edge("top", "right", &["b_id"], &["id"]),
            ],
            groups: Vec::new(),
            node_index: BTreeMap::new(),
            reverse_index: BTreeMap::new(),
        };
        let positioned_nodes = vec![top, left_target, right_target];
        let config = LayoutConfig {
            direction: LayoutDirection::TopToBottom,
            ..Default::default()
        };

        let assignments = assign_edge_ports(&graph, &positioned_nodes, &config);

        let to_left = match assignments[0].as_ref().expect("assignment") {
            EdgePortAssignment::Regular(assignment) => assignment,
            EdgePortAssignment::SelfLoop(_) => panic!("expected regular assignment"),
        };
        let to_right = match assignments[1].as_ref().expect("assignment") {
            EdgePortAssignment::Regular(assignment) => assignment,
            EdgePortAssignment::SelfLoop(_) => panic!("expected regular assignment"),
        };

        assert_eq!(to_left.source_side, AttachmentSide::South);
        assert_eq!(to_right.source_side, AttachmentSide::South);

        // Non-zero row_offsets for the two FK columns confirm we're exercising
        // a case the horizontal branch would have touched.
        assert!(to_left.source_row_offset.abs() > f32::EPSILON);
        assert!(to_right.source_row_offset.abs() > f32::EPSILON);

        // On vertical sides the slot_offsets should match the centered helper
        // exactly (no row_offset baked in).
        assert!((to_left.source_slot_offset - centered_slot_offset(0, 2)).abs() < f32::EPSILON);
        assert!((to_right.source_slot_offset - centered_slot_offset(1, 2)).abs() < f32::EPSILON);
    }

    #[test]
    fn test_assign_edge_ports_ttb_cross_axis_uses_horizontal_packing() {
        // TTB direction but the two nodes are at the same rank, so
        // choose_regular_sides falls back to East/West. The horizontal-only
        // packing branch must still apply here — the direction enum alone
        // cannot be used to decide whether the new logic kicks in.
        let source = node_with_fk_columns("source", 0.0, 0.0, 240.0, &["x_id", "y_id", "z"]);
        let other = node_with_fk_columns("other", 400.0, 10.0, 240.0, &["x_id", "y_id", "z"]);
        let graph = LayoutGraph {
            nodes: Vec::new(),
            edges: vec![
                edge("source", "other", &["x_id"], &["y_id"]),
                edge("source", "other", &["y_id"], &["x_id"]),
            ],
            groups: Vec::new(),
            node_index: BTreeMap::new(),
            reverse_index: BTreeMap::new(),
        };
        let positioned_nodes = vec![source, other];
        let config = LayoutConfig {
            direction: LayoutDirection::TopToBottom,
            ..Default::default()
        };

        let assignments = assign_edge_ports(&graph, &positioned_nodes, &config);

        let e0 = match assignments[0].as_ref().expect("assignment") {
            EdgePortAssignment::Regular(assignment) => assignment,
            EdgePortAssignment::SelfLoop(_) => panic!("expected regular assignment"),
        };
        let e1 = match assignments[1].as_ref().expect("assignment") {
            EdgePortAssignment::Regular(assignment) => assignment,
            EdgePortAssignment::SelfLoop(_) => panic!("expected regular assignment"),
        };

        assert_eq!(e0.source_side, AttachmentSide::East);
        assert_eq!(e0.target_side, AttachmentSide::West);
        assert_eq!(e1.source_side, AttachmentSide::East);
        assert_eq!(e1.target_side, AttachmentSide::West);

        let eff_0_src = e0.source_slot_offset + e0.source_row_offset;
        let eff_1_src = e1.source_slot_offset + e1.source_row_offset;
        assert!(
            (eff_0_src - eff_1_src).abs() >= 13.5,
            "source ports collapsed: e0={eff_0_src}, e1={eff_1_src}"
        );

        let eff_0_tgt = e0.target_slot_offset + e0.target_row_offset;
        let eff_1_tgt = e1.target_slot_offset + e1.target_row_offset;
        assert!(
            (eff_0_tgt - eff_1_tgt).abs() >= 13.5,
            "target ports collapsed: e0={eff_0_tgt}, e1={eff_1_tgt}"
        );
    }

    #[test]
    fn test_assign_edge_ports_tracks_self_loops_separately() {
        let mut loop_edge = edge("users", "users", &[], &[]);
        loop_edge.is_self_loop = true;
        let graph = LayoutGraph {
            nodes: Vec::new(),
            edges: vec![loop_edge],
            groups: Vec::new(),
            node_index: BTreeMap::new(),
            reverse_index: BTreeMap::new(),
        };
        let positioned_nodes = vec![node("users", 0.0, 0.0)];

        let assignments = assign_edge_ports(&graph, &positioned_nodes, &LayoutConfig::default());

        match assignments[0].as_ref().expect("assignment") {
            EdgePortAssignment::SelfLoop(assignment) => {
                assert!(assignment.radius_offset.abs() < f32::EPSILON);
            }
            EdgePortAssignment::Regular(_) => panic!("expected self-loop assignment"),
        }
    }
}
