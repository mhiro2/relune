//! Node ordering within layers
//!
//! This module implements algorithms for ordering nodes within each layer
//! to minimize edge crossings and improve readability.

use std::collections::{BTreeMap, HashMap};

use crate::graph::LayoutGraph;
use crate::rank::RankAssignment;

/// Strategy for crossing reduction during node ordering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CrossingReductionStrategy {
    /// Use barycenter heuristic (average position of neighbors).
    #[default]
    Barycenter,
    /// Use median heuristic (median position of neighbors).
    Median,
    /// Use sifting algorithm (local optimization).
    Sifting,
    /// Try multiple strategies and pick the one with fewest crossings.
    Combined,
}

/// Order nodes within each layer to minimize edge crossings.
///
/// Returns a new rank assignment with nodes reordered within each layer.
/// The ordering is deterministic for consistent output.
#[must_use]
pub fn order_nodes_within_layers(graph: &LayoutGraph, ranks: &RankAssignment) -> Vec<Vec<usize>> {
    order_nodes_within_layers_with_strategy(graph, ranks, CrossingReductionStrategy::default())
}

/// Order nodes within each layer using a specific crossing reduction strategy.
///
/// Returns a new rank assignment with nodes reordered within each layer.
/// The ordering is deterministic for consistent output.
#[must_use]
pub fn order_nodes_within_layers_with_strategy(
    graph: &LayoutGraph,
    ranks: &RankAssignment,
    strategy: CrossingReductionStrategy,
) -> Vec<Vec<usize>> {
    if ranks.num_ranks == 0 {
        return Vec::new();
    }

    // Build edge information for crossing counting
    let edges_by_node = build_edges_by_node(graph);

    match strategy {
        CrossingReductionStrategy::Combined => {
            // Try all strategies and pick the best
            let strategies = [
                CrossingReductionStrategy::Barycenter,
                CrossingReductionStrategy::Median,
                CrossingReductionStrategy::Sifting,
            ];

            let mut best_ordering = Vec::new();
            let mut best_crossings = usize::MAX;

            for strat in strategies {
                let ordering = apply_strategy(graph, ranks, strat);
                let crossings = count_crossings(&ordering, &edges_by_node);

                if crossings < best_crossings {
                    best_crossings = crossings;
                    best_ordering = ordering;
                }
            }

            best_ordering
        }
        _ => apply_strategy(graph, ranks, strategy),
    }
}

/// Apply a single crossing reduction strategy.
fn apply_strategy(
    graph: &LayoutGraph,
    ranks: &RankAssignment,
    strategy: CrossingReductionStrategy,
) -> Vec<Vec<usize>> {
    let mut nodes_by_rank = ranks.nodes_by_rank.clone();

    // Build adjacency information for crossing reduction
    let adjacency = build_adjacency(graph);

    // Multiple passes for better results
    for _ in 0..3 {
        // Forward pass
        for rank_idx in 1..ranks.num_ranks {
            nodes_by_rank[rank_idx] = match strategy {
                CrossingReductionStrategy::Barycenter => order_by_barycenter(
                    &nodes_by_rank[rank_idx],
                    &nodes_by_rank[rank_idx - 1],
                    &adjacency,
                ),
                CrossingReductionStrategy::Median => order_by_median(
                    &nodes_by_rank[rank_idx],
                    &nodes_by_rank[rank_idx - 1],
                    &adjacency,
                ),
                CrossingReductionStrategy::Sifting => order_by_sifting(
                    &nodes_by_rank[rank_idx],
                    &nodes_by_rank[rank_idx - 1],
                    &adjacency,
                ),
                CrossingReductionStrategy::Combined => unreachable!(),
            };
        }

        // Backward pass
        for rank_idx in (0..ranks.num_ranks.saturating_sub(1)).rev() {
            nodes_by_rank[rank_idx] = match strategy {
                CrossingReductionStrategy::Barycenter => order_by_barycenter(
                    &nodes_by_rank[rank_idx],
                    &nodes_by_rank[rank_idx + 1],
                    &adjacency,
                ),
                CrossingReductionStrategy::Median => order_by_median(
                    &nodes_by_rank[rank_idx],
                    &nodes_by_rank[rank_idx + 1],
                    &adjacency,
                ),
                CrossingReductionStrategy::Sifting => order_by_sifting(
                    &nodes_by_rank[rank_idx],
                    &nodes_by_rank[rank_idx + 1],
                    &adjacency,
                ),
                CrossingReductionStrategy::Combined => unreachable!(),
            };
        }
    }

    // Apply final sifting pass for local optimization
    if strategy == CrossingReductionStrategy::Sifting {
        apply_global_sifting(&mut nodes_by_rank, &adjacency);
    }

    nodes_by_rank
}

/// Build adjacency information for each node.
fn build_adjacency(graph: &LayoutGraph) -> BTreeMap<usize, Vec<usize>> {
    let mut adjacency: BTreeMap<usize, Vec<usize>> = BTreeMap::new();

    for edge in &graph.edges {
        if edge.is_self_loop {
            continue;
        }
        if let (Some(&from_idx), Some(&to_idx)) = (
            graph.node_index.get(&edge.from),
            graph.node_index.get(&edge.to),
        ) {
            adjacency.entry(from_idx).or_default().push(to_idx);
            adjacency.entry(to_idx).or_default().push(from_idx);
        }
    }

    adjacency
}

/// Build edge information organized by node for crossing counting.
fn build_edges_by_node(graph: &LayoutGraph) -> BTreeMap<usize, Vec<(usize, usize)>> {
    let mut edges_by_node: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();

    for edge in &graph.edges {
        if edge.is_self_loop {
            continue;
        }
        if let (Some(&from_idx), Some(&to_idx)) = (
            graph.node_index.get(&edge.from),
            graph.node_index.get(&edge.to),
        ) {
            edges_by_node
                .entry(from_idx)
                .or_default()
                .push((from_idx, to_idx));
            edges_by_node
                .entry(to_idx)
                .or_default()
                .push((from_idx, to_idx));
        }
    }

    edges_by_node
}

/// Count the number of edge crossings in the current ordering.
fn count_crossings(
    nodes_by_rank: &[Vec<usize>],
    edges_by_node: &BTreeMap<usize, Vec<(usize, usize)>>,
) -> usize {
    let mut crossings = 0;

    // Build position map for all nodes
    let mut position: BTreeMap<usize, usize> = BTreeMap::new();
    for (rank_idx, rank_nodes) in nodes_by_rank.iter().enumerate() {
        for (pos, &node_idx) in rank_nodes.iter().enumerate() {
            position.insert(node_idx, (rank_idx << 16) | pos);
        }
    }

    // Collect all edges with their positions
    let mut all_edges: Vec<(usize, usize, usize, usize)> = Vec::new(); // (from_rank, from_pos, to_rank, to_pos)
    for edges in edges_by_node.values() {
        for &(from_idx, to_idx) in edges {
            if let (Some(&from_pos), Some(&to_pos)) =
                (position.get(&from_idx), position.get(&to_idx))
            {
                let from_rank = from_pos >> 16;
                let from_col = from_pos & 0xFFFF;
                let to_rank = to_pos >> 16;
                let to_col = to_pos & 0xFFFF;
                // Only count each edge once (from lower rank to higher rank)
                if from_rank < to_rank {
                    all_edges.push((from_rank, from_col, to_rank, to_col));
                }
            }
        }
    }

    // Count crossings between consecutive layers using merge-sort inversion count O(E log E)
    for rank_idx in 0..nodes_by_rank.len().saturating_sub(1) {
        let mut edges_in_layer: Vec<(usize, usize)> = all_edges
            .iter()
            .filter(|(fr, _, tr, _)| *fr == rank_idx && *tr == rank_idx + 1)
            .map(|(_, fp, _, tp)| (*fp, *tp))
            .collect();

        // Sort by source position, then count inversions in target positions
        edges_in_layer.sort_unstable_by_key(|&(fp, _)| fp);
        let targets: Vec<usize> = edges_in_layer.iter().map(|&(_, tp)| tp).collect();
        crossings += count_inversions(&targets);
    }

    crossings
}

/// Counts inversions using merge sort in O(n log n).
fn count_inversions(arr: &[usize]) -> usize {
    if arr.len() <= 1 {
        return 0;
    }
    let mut work = arr.to_vec();
    merge_sort_count(&mut work)
}

fn merge_sort_count(arr: &mut [usize]) -> usize {
    let n = arr.len();
    if n <= 1 {
        return 0;
    }
    let mid = n / 2;
    let mut count = 0;
    count += merge_sort_count(&mut arr[..mid]);
    count += merge_sort_count(&mut arr[mid..]);

    // Merge step counting inversions
    let left = arr[..mid].to_vec();
    let right = arr[mid..].to_vec();
    let (mut i, mut j, mut k) = (0, 0, 0);

    while i < left.len() && j < right.len() {
        if left[i] <= right[j] {
            arr[k] = left[i];
            i += 1;
        } else {
            arr[k] = right[j];
            count += left.len() - i; // All remaining left elements form inversions
            j += 1;
        }
        k += 1;
    }
    while i < left.len() {
        arr[k] = left[i];
        i += 1;
        k += 1;
    }
    while j < right.len() {
        arr[k] = right[j];
        j += 1;
        k += 1;
    }
    count
}

/// Order nodes in a layer using the barycenter heuristic.
fn order_by_barycenter(
    layer_nodes: &[usize],
    adjacent_layer: &[usize],
    adjacency: &BTreeMap<usize, Vec<usize>>,
) -> Vec<usize> {
    // Create position map for adjacent layer
    let position: BTreeMap<usize, usize> = adjacent_layer
        .iter()
        .enumerate()
        .map(|(pos, &idx)| (idx, pos))
        .collect();

    // Calculate barycenter for each node in current layer
    #[allow(clippy::cast_precision_loss)]
    #[allow(clippy::map_unwrap_or)]
    let mut nodes_with_barycenter: Vec<(usize, f64)> = layer_nodes
        .iter()
        .map(|&node_idx| {
            let neighbors = adjacency.get(&node_idx).map(Vec::as_slice).unwrap_or(&[]);
            let positions: Vec<usize> = neighbors
                .iter()
                .filter_map(|&n| position.get(&n).copied())
                .collect();

            let barycenter = if positions.is_empty() {
                // No connections - use node index as tie-breaker for determinism
                node_idx as f64
            } else {
                positions.iter().sum::<usize>() as f64 / positions.len() as f64
            };

            (node_idx, barycenter)
        })
        .collect();

    // Sort by barycenter, then by node index for determinism
    nodes_with_barycenter.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    nodes_with_barycenter
        .into_iter()
        .map(|(idx, _)| idx)
        .collect()
}

/// Order nodes in a layer using the median heuristic.
#[allow(clippy::map_unwrap_or)]
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::stable_sort_primitive)]
fn order_by_median(
    layer_nodes: &[usize],
    adjacent_layer: &[usize],
    adjacency: &BTreeMap<usize, Vec<usize>>,
) -> Vec<usize> {
    // Create position map for adjacent layer
    let position: BTreeMap<usize, usize> = adjacent_layer
        .iter()
        .enumerate()
        .map(|(pos, &idx)| (idx, pos))
        .collect();

    // Calculate median for each node in current layer
    #[allow(clippy::map_unwrap_or)]
    #[allow(clippy::cast_precision_loss)]
    #[allow(clippy::stable_sort_primitive)]
    let mut nodes_with_median: Vec<(usize, f64)> = layer_nodes
        .iter()
        .map(|&node_idx| {
            let neighbors = adjacency.get(&node_idx).map(Vec::as_slice).unwrap_or(&[]);
            let mut positions: Vec<usize> = neighbors
                .iter()
                .filter_map(|&n| position.get(&n).copied())
                .collect();

            let median = if positions.is_empty() {
                // No connections - use node index as tie-breaker for determinism
                node_idx as f64
            } else {
                positions.sort();
                let len = positions.len();
                if len % 2 == 1 {
                    positions[len / 2] as f64
                } else {
                    // Average of two middle elements
                    (positions[len / 2 - 1] + positions[len / 2]) as f64 / 2.0
                }
            };

            (node_idx, median)
        })
        .collect();

    // Sort by median, then by node index for determinism
    nodes_with_median.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    nodes_with_median.into_iter().map(|(idx, _)| idx).collect()
}

/// Threshold for sifting: above this, fall back to median heuristic only.
const SIFTING_NODE_LIMIT: usize = 100;

/// Order nodes in a layer using the sifting algorithm.
///
/// For layers with more than [`SIFTING_NODE_LIMIT`] nodes, the sifting phase
/// is skipped and the input ordering (typically from median heuristic) is
/// returned directly to avoid O(V^2) degradation.
#[allow(clippy::map_unwrap_or)]
fn order_by_sifting(
    layer_nodes: &[usize],
    adjacent_layer: &[usize],
    adjacency: &BTreeMap<usize, Vec<usize>>,
) -> Vec<usize> {
    if layer_nodes.len() <= 1 || layer_nodes.len() > SIFTING_NODE_LIMIT {
        return layer_nodes.to_vec();
    }

    let adj_position: HashMap<usize, usize> = adjacent_layer
        .iter()
        .enumerate()
        .map(|(pos, &idx)| (idx, pos))
        .collect();

    let mut ordering = layer_nodes.to_vec();
    if count_layer_crossings(&ordering, &adj_position, adjacency) == 0 {
        return ordering;
    }

    for i in 0..ordering.len() {
        let node = ordering[i];
        let mut reduced_ordering = ordering.clone();
        reduced_ordering.remove(i);

        let node_targets = collect_target_positions(node, adjacency, &adj_position);
        let other_edges = collect_layer_edges(&reduced_ordering, adjacency, &adj_position);

        let mut best_pos = i;
        let mut best_crossings = usize::MAX;

        for try_pos in 0..=reduced_ordering.len() {
            let crossings = count_inserted_node_crossings(&node_targets, &other_edges, try_pos);

            if crossings < best_crossings || (crossings == best_crossings && try_pos < best_pos) {
                best_crossings = crossings;
                best_pos = try_pos;
            }
        }

        if best_pos != i {
            ordering.remove(i);
            ordering.insert(best_pos, node);
        }
    }

    ordering
}

/// Count crossings for a single layer relative to its adjacent layer.
#[allow(clippy::map_unwrap_or)]
fn count_layer_crossings(
    layer_nodes: &[usize],
    adj_position: &HashMap<usize, usize>,
    adjacency: &BTreeMap<usize, Vec<usize>>,
) -> usize {
    // Build edge list with positions
    let edges: Vec<(usize, usize)> = layer_nodes
        .iter()
        .enumerate()
        .flat_map(|(pos, &node_idx)| {
            let neighbors = adjacency.get(&node_idx).map(Vec::as_slice).unwrap_or(&[]);
            neighbors
                .iter()
                .filter_map(|&n| adj_position.get(&n).map(|&adj_pos| (pos, adj_pos)))
                .collect::<Vec<_>>()
        })
        .collect();

    // Count inversions in the edge list (crossings)
    let mut crossings = 0;
    for i in 0..edges.len() {
        for j in (i + 1)..edges.len() {
            let (from1, to1) = edges[i];
            let (from2, to2) = edges[j];
            // Crossing occurs when edges cross
            if (from1 < from2 && to1 > to2) || (from1 > from2 && to1 < to2) {
                crossings += 1;
            }
        }
    }

    crossings
}

/// Apply global sifting across all layers.
#[allow(clippy::assigning_clones)]
fn apply_global_sifting(nodes_by_rank: &mut [Vec<usize>], adjacency: &BTreeMap<usize, Vec<usize>>) {
    if nodes_by_rank.is_empty() {
        return;
    }

    for _ in 0..2 {
        for rank_idx in 0..nodes_by_rank.len() {
            let mut ordering = nodes_by_rank[rank_idx].clone();
            let prev_positions = rank_idx
                .checked_sub(1)
                .map(|prev| build_position_index(&nodes_by_rank[prev]));
            let next_positions = (rank_idx + 1 < nodes_by_rank.len())
                .then(|| build_position_index(&nodes_by_rank[rank_idx + 1]));

            for i in 0..ordering.len() {
                let node = ordering[i];
                let mut reduced_ordering = ordering.clone();
                reduced_ordering.remove(i);

                let node_prev_targets = prev_positions.as_ref().map_or_else(Vec::new, |pos| {
                    collect_target_positions(node, adjacency, pos)
                });
                let node_next_targets = next_positions.as_ref().map_or_else(Vec::new, |pos| {
                    collect_target_positions(node, adjacency, pos)
                });
                let prev_edges = prev_positions.as_ref().map_or_else(Vec::new, |pos| {
                    collect_layer_edges(&reduced_ordering, adjacency, pos)
                });
                let next_edges = next_positions.as_ref().map_or_else(Vec::new, |pos| {
                    collect_layer_edges(&reduced_ordering, adjacency, pos)
                });

                let mut best_pos = i;
                let mut best_crossings = usize::MAX;

                for try_pos in 0..=reduced_ordering.len() {
                    let crossings =
                        count_inserted_node_crossings(&node_prev_targets, &prev_edges, try_pos)
                            + count_inserted_node_crossings(
                                &node_next_targets,
                                &next_edges,
                                try_pos,
                            );

                    if crossings < best_crossings
                        || (crossings == best_crossings && try_pos < best_pos)
                    {
                        best_crossings = crossings;
                        best_pos = try_pos;
                    }
                }

                if best_pos != i {
                    ordering.remove(i);
                    ordering.insert(best_pos, node);
                }
            }

            nodes_by_rank[rank_idx] = ordering;
        }
    }
}

fn build_position_index(nodes: &[usize]) -> HashMap<usize, usize> {
    nodes
        .iter()
        .enumerate()
        .map(|(pos, &idx)| (idx, pos))
        .collect()
}

fn collect_target_positions(
    node_idx: usize,
    adjacency: &BTreeMap<usize, Vec<usize>>,
    target_positions: &HashMap<usize, usize>,
) -> Vec<usize> {
    adjacency
        .get(&node_idx)
        .map_or_else(|| &[] as &[usize], Vec::as_slice)
        .iter()
        .filter_map(|&neighbor| target_positions.get(&neighbor).copied())
        .collect()
}

fn collect_layer_edges(
    ordering: &[usize],
    adjacency: &BTreeMap<usize, Vec<usize>>,
    adjacent_positions: &HashMap<usize, usize>,
) -> Vec<(usize, usize)> {
    ordering
        .iter()
        .enumerate()
        .flat_map(|(src_pos, &node_idx)| {
            adjacency
                .get(&node_idx)
                .map_or_else(|| &[] as &[usize], Vec::as_slice)
                .iter()
                .filter_map(move |&neighbor| {
                    adjacent_positions
                        .get(&neighbor)
                        .copied()
                        .map(|dst_pos| (src_pos, dst_pos))
                })
        })
        .collect()
}

fn count_inserted_node_crossings(
    node_targets: &[usize],
    other_edges: &[(usize, usize)],
    insert_pos: usize,
) -> usize {
    let mut crossings = 0;

    for &(base_src, other_target) in other_edges {
        let other_src = if base_src >= insert_pos {
            base_src + 1
        } else {
            base_src
        };

        for &node_target in node_targets {
            if (insert_pos < other_src && node_target > other_target)
                || (insert_pos > other_src && node_target < other_target)
            {
                crossings += 1;
            }
        }
    }

    crossings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::LayoutGraphBuilder;
    use crate::rank::{RankAssignmentStrategy, assign_ranks};
    use relune_core::{Column, ColumnId, ForeignKey, ReferentialAction, Schema, Table, TableId};

    fn make_test_schema() -> Schema {
        Schema {
            tables: vec![
                Table {
                    id: TableId(1),
                    stable_id: "a".to_string(),
                    schema_name: None,
                    name: "a".to_string(),
                    columns: vec![Column {
                        id: ColumnId(1),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(2),
                    stable_id: "b".to_string(),
                    schema_name: None,
                    name: "b".to_string(),
                    columns: vec![Column {
                        id: ColumnId(2),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![ForeignKey {
                        name: None,
                        from_columns: vec!["a_id".to_string()],
                        to_schema: None,
                        to_table: "a".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    }],
                    indexes: vec![],
                    comment: None,
                },
            ],
            views: vec![],
            enums: vec![],
        }
    }

    #[allow(clippy::too_many_lines)]
    fn make_complex_schema() -> Schema {
        // Create a more complex schema with multiple tables for crossing tests
        // Layout:
        //   a (rank 2)    <- referenced by b and c
        //   b (rank 1)    <- references a, referenced by d and e
        //   c (rank 1)    <- references a, referenced by d and e
        //   d (rank 0)    <- references b and c
        //   e (rank 0)    <- references b and c
        Schema {
            tables: vec![
                Table {
                    id: TableId(1),
                    stable_id: "a".to_string(),
                    schema_name: None,
                    name: "a".to_string(),
                    columns: vec![Column {
                        id: ColumnId(1),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(2),
                    stable_id: "b".to_string(),
                    schema_name: None,
                    name: "b".to_string(),
                    columns: vec![Column {
                        id: ColumnId(2),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![ForeignKey {
                        name: None,
                        from_columns: vec!["a_id".to_string()],
                        to_schema: None,
                        to_table: "a".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    }],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(3),
                    stable_id: "c".to_string(),
                    schema_name: None,
                    name: "c".to_string(),
                    columns: vec![Column {
                        id: ColumnId(3),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![ForeignKey {
                        name: None,
                        from_columns: vec!["a_id".to_string()],
                        to_schema: None,
                        to_table: "a".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    }],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(4),
                    stable_id: "d".to_string(),
                    schema_name: None,
                    name: "d".to_string(),
                    columns: vec![Column {
                        id: ColumnId(4),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![
                        ForeignKey {
                            name: None,
                            from_columns: vec!["b_id".to_string()],
                            to_schema: None,
                            to_table: "b".to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        },
                        ForeignKey {
                            name: None,
                            from_columns: vec!["c_id".to_string()],
                            to_schema: None,
                            to_table: "c".to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        },
                    ],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(5),
                    stable_id: "e".to_string(),
                    schema_name: None,
                    name: "e".to_string(),
                    columns: vec![Column {
                        id: ColumnId(5),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![
                        ForeignKey {
                            name: None,
                            from_columns: vec!["b_id".to_string()],
                            to_schema: None,
                            to_table: "b".to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        },
                        ForeignKey {
                            name: None,
                            from_columns: vec!["c_id".to_string()],
                            to_schema: None,
                            to_table: "c".to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        },
                    ],
                    indexes: vec![],
                    comment: None,
                },
            ],
            views: vec![],
            enums: vec![],
        }
    }

    #[test]
    fn test_order_nodes_deterministic() {
        let schema = make_test_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph, RankAssignmentStrategy::Topological);

        let ordered1 = order_nodes_within_layers(&graph, &ranks);
        let ordered2 = order_nodes_within_layers(&graph, &ranks);

        // Ordering should be deterministic
        assert_eq!(ordered1, ordered2);
    }

    #[test]
    fn test_order_nodes_deterministic_with_strategy() {
        let schema = make_test_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph, RankAssignmentStrategy::Topological);

        for strategy in [
            CrossingReductionStrategy::Barycenter,
            CrossingReductionStrategy::Median,
            CrossingReductionStrategy::Sifting,
            CrossingReductionStrategy::Combined,
        ] {
            let ordered1 = order_nodes_within_layers_with_strategy(&graph, &ranks, strategy);
            let ordered2 = order_nodes_within_layers_with_strategy(&graph, &ranks, strategy);

            // Ordering should be deterministic for all strategies
            assert_eq!(
                ordered1, ordered2,
                "Strategy {strategy:?} should be deterministic",
            );
        }
    }

    #[test]
    fn test_barycenter_ordering() {
        let schema = make_test_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph, RankAssignmentStrategy::Topological);

        let ordered = order_nodes_within_layers_with_strategy(
            &graph,
            &ranks,
            CrossingReductionStrategy::Barycenter,
        );

        // Verify all nodes are present
        let total_nodes: usize = ordered.iter().map(Vec::len).sum();
        assert_eq!(total_nodes, graph.nodes.len());
    }

    #[test]
    fn test_median_ordering() {
        let schema = make_test_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph, RankAssignmentStrategy::Topological);

        let ordered = order_nodes_within_layers_with_strategy(
            &graph,
            &ranks,
            CrossingReductionStrategy::Median,
        );

        // Verify all nodes are present
        let total_nodes: usize = ordered.iter().map(Vec::len).sum();
        assert_eq!(total_nodes, graph.nodes.len());
    }

    #[test]
    fn test_sifting_ordering() {
        let schema = make_test_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph, RankAssignmentStrategy::Topological);

        let ordered = order_nodes_within_layers_with_strategy(
            &graph,
            &ranks,
            CrossingReductionStrategy::Sifting,
        );

        // Verify all nodes are present
        let total_nodes: usize = ordered.iter().map(Vec::len).sum();
        assert_eq!(total_nodes, graph.nodes.len());
    }

    #[test]
    fn test_combined_strategy_picks_best() {
        let schema = make_complex_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph, RankAssignmentStrategy::Topological);

        let edges_by_node = build_edges_by_node(&graph);

        // Get results from individual strategies
        let barycenter = order_nodes_within_layers_with_strategy(
            &graph,
            &ranks,
            CrossingReductionStrategy::Barycenter,
        );
        let median = order_nodes_within_layers_with_strategy(
            &graph,
            &ranks,
            CrossingReductionStrategy::Median,
        );
        let sifting = order_nodes_within_layers_with_strategy(
            &graph,
            &ranks,
            CrossingReductionStrategy::Sifting,
        );
        let combined = order_nodes_within_layers_with_strategy(
            &graph,
            &ranks,
            CrossingReductionStrategy::Combined,
        );

        let barycenter_crossings = count_crossings(&barycenter, &edges_by_node);
        let median_crossings = count_crossings(&median, &edges_by_node);
        let sifting_crossings = count_crossings(&sifting, &edges_by_node);
        let combined_crossings = count_crossings(&combined, &edges_by_node);

        let min_crossings = barycenter_crossings
            .min(median_crossings)
            .min(sifting_crossings);

        // Combined should not be worse than the best individual strategy
        assert!(
            combined_crossings <= min_crossings,
            "Combined crossings ({combined_crossings}) should be <= min of individual strategies ({min_crossings})",
        );
    }

    #[test]
    fn test_crossing_count() {
        // Test the crossing count function directly
        // Simple case: two nodes in each of two layers
        let nodes_by_rank = vec![
            vec![0, 1], // Layer 0
            vec![2, 3], // Layer 1
        ];

        // Build edges_by_node manually
        let mut edges_by_node: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
        // Edge from 0 to 2
        edges_by_node.entry(0).or_default().push((0, 2));
        edges_by_node.entry(2).or_default().push((0, 2));
        // Edge from 1 to 3
        edges_by_node.entry(1).or_default().push((1, 3));
        edges_by_node.entry(3).or_default().push((1, 3));

        // No crossings: 0->2, 1->3 (parallel edges)
        let crossings = count_crossings(&nodes_by_rank, &edges_by_node);
        assert_eq!(crossings, 0, "Parallel edges should have no crossings");

        // Swap layer 1 nodes to create crossing
        let nodes_crossing = vec![
            vec![0, 1], // Layer 0
            vec![3, 2], // Layer 1 (swapped)
        ];

        // Now 0->2 crosses 1->3 because:
        // - Node 0 is at position 0 in layer 0, connects to node 2 at position 1 in layer 1
        // - Node 1 is at position 1 in layer 0, connects to node 3 at position 0 in layer 1
        // This creates a crossing
        let crossings = count_crossings(&nodes_crossing, &edges_by_node);
        assert!(crossings > 0, "Crossed edges should have crossings");
    }

    #[test]
    fn test_improved_ordering_reduces_crossings() {
        let schema = make_complex_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph, RankAssignmentStrategy::Topological);

        let edges_by_node = build_edges_by_node(&graph);

        // Get ordering with default strategy
        let ordered = order_nodes_within_layers(&graph, &ranks);
        let crossings = count_crossings(&ordered, &edges_by_node);

        // Combined strategy should be at least as good
        let combined = order_nodes_within_layers_with_strategy(
            &graph,
            &ranks,
            CrossingReductionStrategy::Combined,
        );
        let combined_crossings = count_crossings(&combined, &edges_by_node);

        assert!(
            combined_crossings <= crossings,
            "Combined strategy crossings ({combined_crossings}) should be <= default crossings ({crossings})",
        );
    }

    #[test]
    fn test_all_strategies_produce_valid_ordering() {
        let schema = make_complex_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph, RankAssignmentStrategy::Topological);

        let original_nodes: Vec<std::collections::BTreeSet<usize>> = ranks
            .nodes_by_rank
            .iter()
            .map(|layer| layer.iter().copied().collect())
            .collect();

        for strategy in [
            CrossingReductionStrategy::Barycenter,
            CrossingReductionStrategy::Median,
            CrossingReductionStrategy::Sifting,
            CrossingReductionStrategy::Combined,
        ] {
            let ordered = order_nodes_within_layers_with_strategy(&graph, &ranks, strategy);

            // Verify each layer contains exactly the same nodes
            assert_eq!(
                ordered.len(),
                original_nodes.len(),
                "Strategy {strategy:?}: layer count mismatch",
            );

            for (rank_idx, (ordered_layer, original_layer)) in
                ordered.iter().zip(original_nodes.iter()).enumerate()
            {
                let ordered_set: std::collections::BTreeSet<usize> =
                    ordered_layer.iter().copied().collect();
                assert_eq!(
                    &ordered_set, original_layer,
                    "Strategy {strategy:?}: nodes in rank {rank_idx} don't match original",
                );
            }
        }
    }
}
