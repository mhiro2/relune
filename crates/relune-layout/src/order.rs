//! Node ordering within layers
//!
//! Crossing reduction runs on a proper layered graph: every edge spanning
//! more than one rank is split into a chain of virtual nodes, one per
//! intermediate rank, so long edges take part in the ordering of every layer
//! they cross and their crossings are counted. Layer sweeps keep the best
//! ordering seen so far, and virtual nodes are removed from the result.

use std::collections::HashMap;

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

/// Maximum number of down/up sweep pairs per strategy.
const MAX_SWEEPS: usize = 12;
/// Consecutive sweeps without improvement after which the search stops.
const SWEEP_PATIENCE: usize = 3;
/// Virtual nodes allowed per real node, with a floor for small graphs.
const VIRTUAL_NODES_PER_NODE: usize = 16;
const MIN_VIRTUAL_NODE_BUDGET: usize = 4096;

fn virtual_node_budget(real_count: usize) -> usize {
    real_count
        .saturating_mul(VIRTUAL_NODES_PER_NODE)
        .max(MIN_VIRTUAL_NODE_BUDGET)
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

    let layered = LayeredGraph::new(graph, ranks);
    let (ordering, _) = layered.best_ordering(strategy);
    layered.without_virtual_nodes(ordering)
}

/// Layered graph in which every edge connects adjacent layers.
///
/// Node ids below `real_count` are graph nodes; the rest are virtual nodes
/// that stand in for a long edge on an intermediate layer.
struct LayeredGraph {
    real_count: usize,
    /// Initial ordering of every layer, virtual nodes appended.
    layers: Vec<Vec<usize>>,
    /// Neighbours of every node, all on an adjacent layer.
    adjacency: Vec<Vec<usize>>,
    /// Layer of every node.
    layer_of: Vec<usize>,
}

impl LayeredGraph {
    fn new(graph: &LayoutGraph, ranks: &RankAssignment) -> Self {
        let edges = graph
            .edges
            .iter()
            .filter(|edge| !edge.is_self_loop)
            .filter_map(|edge| {
                Some((
                    *graph.node_index.get(&edge.from)?,
                    *graph.node_index.get(&edge.to)?,
                ))
            });
        Self::from_parts(&ranks.nodes_by_rank, &ranks.node_rank, edges)
    }

    fn from_parts(
        nodes_by_rank: &[Vec<usize>],
        node_rank: &[usize],
        edges: impl IntoIterator<Item = (usize, usize)>,
    ) -> Self {
        let real_count = node_rank.len();
        let mut layers = nodes_by_rank.to_vec();
        let mut adjacency = vec![Vec::new(); real_count];
        let mut layer_of = node_rank.to_vec();

        // Edges inside one layer do not influence the ordering. Shorter edges
        // are split first so the virtual node budget covers as many edges as
        // possible; the stable sort keeps input order among equal spans.
        let mut spans: Vec<(usize, usize)> = edges
            .into_iter()
            .map(|(from, to)| {
                if node_rank[from] <= node_rank[to] {
                    (from, to)
                } else {
                    (to, from)
                }
            })
            .filter(|&(upper, lower)| node_rank[upper] != node_rank[lower])
            .collect();
        spans.sort_by_key(|&(upper, lower)| node_rank[lower] - node_rank[upper]);
        let mut virtual_budget = virtual_node_budget(real_count);

        for (upper, lower) in spans {
            let (start, end) = (layer_of[upper], layer_of[lower]);
            let needed = end - start - 1;
            if needed > virtual_budget {
                // Dense, deep graphs could need a cubic number of virtual
                // nodes; beyond the budget a long edge still links its
                // endpoints for the ordering keys but is not split.
                adjacency[upper].push(lower);
                adjacency[lower].push(upper);
                continue;
            }
            virtual_budget -= needed;
            let mut previous = upper;
            for (layer, layer_nodes) in layers.iter_mut().enumerate().take(end).skip(start + 1) {
                let virtual_node = adjacency.len();
                adjacency.push(Vec::new());
                layer_of.push(layer);
                layer_nodes.push(virtual_node);
                adjacency[previous].push(virtual_node);
                adjacency[virtual_node].push(previous);
                previous = virtual_node;
            }
            adjacency[previous].push(lower);
            adjacency[lower].push(previous);
        }

        Self {
            real_count,
            layers,
            adjacency,
            layer_of,
        }
    }

    /// Returns the best ordering for `strategy` (trying every single strategy
    /// for [`CrossingReductionStrategy::Combined`]) and its crossing count.
    fn best_ordering(&self, strategy: CrossingReductionStrategy) -> (Vec<Vec<usize>>, usize) {
        if strategy != CrossingReductionStrategy::Combined {
            return self.reduce_crossings(strategy);
        }
        // `min_by_key` keeps the first strategy among equally good results.
        [
            CrossingReductionStrategy::Barycenter,
            CrossingReductionStrategy::Median,
            CrossingReductionStrategy::Sifting,
        ]
        .into_iter()
        .map(|single| self.reduce_crossings(single))
        .min_by_key(|(_, crossings)| *crossings)
        .unwrap_or_else(|| (self.layers.clone(), 0))
    }

    /// Runs alternating down/up sweeps and returns the ordering with the
    /// fewest crossings seen, together with that crossing count.
    fn reduce_crossings(&self, strategy: CrossingReductionStrategy) -> (Vec<Vec<usize>>, usize) {
        let mut layers = self.layers.clone();
        let mut best = layers.clone();
        let mut best_crossings = self.count_crossings(&best);
        let mut stale_sweeps = 0;

        for _ in 0..MAX_SWEEPS {
            if best_crossings == 0 {
                break;
            }
            let mut improved = false;
            for downward in [true, false] {
                if downward {
                    for layer in 1..layers.len() {
                        layers[layer] =
                            self.reorder_layer(strategy, &layers[layer], &layers[layer - 1]);
                    }
                } else {
                    for layer in (0..layers.len().saturating_sub(1)).rev() {
                        layers[layer] =
                            self.reorder_layer(strategy, &layers[layer], &layers[layer + 1]);
                    }
                }
                // Check after each direction: an upward sweep can undo what
                // the downward sweep gained.
                let crossings = self.count_crossings(&layers);
                if crossings < best_crossings {
                    best.clone_from(&layers);
                    best_crossings = crossings;
                    improved = true;
                }
            }
            if improved {
                stale_sweeps = 0;
            } else {
                stale_sweeps += 1;
                if stale_sweeps >= SWEEP_PATIENCE {
                    break;
                }
            }
        }

        if strategy == CrossingReductionStrategy::Sifting && best_crossings > 0 {
            let mut sifted = best.clone();
            apply_global_sifting(&mut sifted, &self.adjacency);
            let crossings = self.count_crossings(&sifted);
            if crossings < best_crossings {
                best = sifted;
                best_crossings = crossings;
            }
        }

        (best, best_crossings)
    }

    fn reorder_layer(
        &self,
        strategy: CrossingReductionStrategy,
        layer_nodes: &[usize],
        adjacent_layer: &[usize],
    ) -> Vec<usize> {
        match strategy {
            CrossingReductionStrategy::Barycenter | CrossingReductionStrategy::Combined => {
                order_by_key(layer_nodes, adjacent_layer, &self.adjacency, barycenter)
            }
            CrossingReductionStrategy::Median => {
                order_by_key(layer_nodes, adjacent_layer, &self.adjacency, median)
            }
            CrossingReductionStrategy::Sifting => {
                order_by_sifting(layer_nodes, adjacent_layer, &self.adjacency)
            }
        }
    }

    /// Counts crossings between every pair of adjacent layers.
    fn count_crossings(&self, layers: &[Vec<usize>]) -> usize {
        let mut position = vec![0usize; self.adjacency.len()];
        for layer in layers {
            for (pos, &node) in layer.iter().enumerate() {
                position[node] = pos;
            }
        }

        layers
            .iter()
            .enumerate()
            .take(layers.len().saturating_sub(1))
            .map(|(layer_idx, layer)| {
                let mut edges: Vec<(usize, usize)> = layer
                    .iter()
                    .flat_map(|&node| {
                        self.adjacency[node]
                            .iter()
                            .filter(|&&neighbor| self.layer_of[neighbor] == layer_idx + 1)
                            .map(|&neighbor| (position[node], position[neighbor]))
                            .collect::<Vec<_>>()
                    })
                    .collect();
                // Edges sharing a source never cross each other, so sort
                // targets too before counting inversions.
                edges.sort_unstable();
                let targets: Vec<usize> = edges.into_iter().map(|(_, target)| target).collect();
                count_inversions(&targets)
            })
            .sum()
    }

    fn without_virtual_nodes(&self, layers: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
        layers
            .into_iter()
            .map(|layer| {
                layer
                    .into_iter()
                    .filter(|&node| node < self.real_count)
                    .collect()
            })
            .collect()
    }
}

/// Counts inversions using merge sort in O(n log n).
fn count_inversions(arr: &[usize]) -> usize {
    if arr.len() <= 1 {
        return 0;
    }
    let mut work = arr.to_vec();
    let mut scratch = vec![0; arr.len()];
    merge_sort_count(&mut work, &mut scratch)
}

fn merge_sort_count(arr: &mut [usize], scratch: &mut [usize]) -> usize {
    let n = arr.len();
    if n <= 1 {
        return 0;
    }
    let mid = n / 2;
    let mut count = 0;
    count += merge_sort_count(&mut arr[..mid], &mut scratch[..mid]);
    count += merge_sort_count(&mut arr[mid..], &mut scratch[mid..]);

    // Merge step counting inversions.
    let (mut i, mut j, mut k) = (0, mid, 0);

    while i < mid && j < n {
        if arr[i] <= arr[j] {
            scratch[k] = arr[i];
            i += 1;
        } else {
            scratch[k] = arr[j];
            count += mid - i; // All remaining left elements form inversions.
            j += 1;
        }
        k += 1;
    }

    while i < mid {
        scratch[k] = arr[i];
        i += 1;
        k += 1;
    }

    while j < n {
        scratch[k] = arr[j];
        j += 1;
        k += 1;
    }

    arr.copy_from_slice(&scratch[..n]);
    count
}

#[allow(clippy::cast_precision_loss)] // Layer positions are small layout values.
fn barycenter(positions: &mut [usize]) -> f64 {
    positions.iter().sum::<usize>() as f64 / positions.len() as f64
}

#[allow(clippy::cast_precision_loss)] // Layer positions are small layout values.
fn median(positions: &mut [usize]) -> f64 {
    positions.sort_unstable();
    let middle = positions.len() / 2;
    if positions.len() % 2 == 1 {
        positions[middle] as f64
    } else {
        (positions[middle - 1] + positions[middle]) as f64 / 2.0
    }
}

/// Reorders a layer by a key computed from neighbour positions in the
/// adjacent layer.
///
/// Nodes without neighbours there keep their current slot, and the remaining
/// nodes fill the other slots in key order (ties keep their current order).
fn order_by_key(
    layer_nodes: &[usize],
    adjacent_layer: &[usize],
    adjacency: &[Vec<usize>],
    key: fn(&mut [usize]) -> f64,
) -> Vec<usize> {
    let position = build_position_index(adjacent_layer);
    let keys: Vec<Option<f64>> = layer_nodes
        .iter()
        .map(|&node| {
            let mut positions = collect_target_positions(node, adjacency, &position);
            (!positions.is_empty()).then(|| key(&mut positions))
        })
        .collect();

    let mut keyed: Vec<(f64, usize)> = keys
        .iter()
        .zip(layer_nodes)
        .filter_map(|(key, &node)| key.map(|key| (key, node)))
        .collect();
    // Stable sort keeps the current order for equal keys.
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut sorted = keyed.into_iter().map(|(_, node)| node);

    layer_nodes
        .iter()
        .zip(&keys)
        .filter_map(|(&node, key)| {
            if key.is_some() {
                sorted.next()
            } else {
                Some(node)
            }
        })
        .collect()
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
    adjacency: &[Vec<usize>],
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

    let mut reduced_ordering = Vec::with_capacity(ordering.len().saturating_sub(1));
    for i in 0..ordering.len() {
        let node = ordering[i];
        fill_reduced_ordering(&ordering, i, &mut reduced_ordering);

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
    adjacency: &[Vec<usize>],
) -> usize {
    let mut edges = collect_layer_edges(layer_nodes, adjacency, adj_position);
    edges.sort_unstable();
    let targets: Vec<usize> = edges.into_iter().map(|(_, dst_pos)| dst_pos).collect();
    count_inversions(&targets)
}

/// Apply global sifting across all layers.
#[allow(clippy::assigning_clones)]
fn apply_global_sifting(nodes_by_rank: &mut [Vec<usize>], adjacency: &[Vec<usize>]) {
    if nodes_by_rank.is_empty() {
        return;
    }

    for _ in 0..2 {
        for rank_idx in 0..nodes_by_rank.len() {
            let layer_len = nodes_by_rank[rank_idx].len();
            if layer_len <= 1 || layer_len > SIFTING_NODE_LIMIT {
                continue;
            }

            let mut ordering = nodes_by_rank[rank_idx].clone();
            let prev_positions = rank_idx
                .checked_sub(1)
                .map(|prev| build_position_index(&nodes_by_rank[prev]));
            let next_positions = (rank_idx + 1 < nodes_by_rank.len())
                .then(|| build_position_index(&nodes_by_rank[rank_idx + 1]));
            let mut reduced_ordering = Vec::with_capacity(ordering.len().saturating_sub(1));

            for i in 0..ordering.len() {
                let node = ordering[i];
                fill_reduced_ordering(&ordering, i, &mut reduced_ordering);

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

fn fill_reduced_ordering(ordering: &[usize], skip_idx: usize, reduced_ordering: &mut Vec<usize>) {
    reduced_ordering.clear();
    reduced_ordering.extend_from_slice(&ordering[..skip_idx]);
    reduced_ordering.extend_from_slice(&ordering[skip_idx + 1..]);
}

fn collect_target_positions(
    node_idx: usize,
    adjacency: &[Vec<usize>],
    target_positions: &HashMap<usize, usize>,
) -> Vec<usize> {
    adjacency[node_idx]
        .iter()
        .filter_map(|&neighbor| target_positions.get(&neighbor).copied())
        .collect()
}

fn collect_layer_edges(
    ordering: &[usize],
    adjacency: &[Vec<usize>],
    adjacent_positions: &HashMap<usize, usize>,
) -> Vec<(usize, usize)> {
    ordering
        .iter()
        .enumerate()
        .flat_map(|(src_pos, &node_idx)| {
            adjacency[node_idx].iter().filter_map(move |&neighbor| {
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
    use crate::rank::assign_ranks;
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
                        enum_values: None,
                        semantics: relune_core::ColumnSemantics::default(),
                    }],
                    foreign_keys: vec![],
                    indexes: vec![],
                    primary_key_name: None,
                    check_constraints: Vec::new(),
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
                        enum_values: None,
                        semantics: relune_core::ColumnSemantics::default(),
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
                    primary_key_name: None,
                    check_constraints: Vec::new(),
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
                        enum_values: None,
                        semantics: relune_core::ColumnSemantics::default(),
                    }],
                    foreign_keys: vec![],
                    indexes: vec![],
                    primary_key_name: None,
                    check_constraints: Vec::new(),
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
                        enum_values: None,
                        semantics: relune_core::ColumnSemantics::default(),
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
                    primary_key_name: None,
                    check_constraints: Vec::new(),
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
                        enum_values: None,
                        semantics: relune_core::ColumnSemantics::default(),
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
                    primary_key_name: None,
                    check_constraints: Vec::new(),
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
                        enum_values: None,
                        semantics: relune_core::ColumnSemantics::default(),
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
                    primary_key_name: None,
                    check_constraints: Vec::new(),
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
                        enum_values: None,
                        semantics: relune_core::ColumnSemantics::default(),
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
                    primary_key_name: None,
                    check_constraints: Vec::new(),
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
        let ranks = assign_ranks(&graph);

        let ordered1 = order_nodes_within_layers(&graph, &ranks);
        let ordered2 = order_nodes_within_layers(&graph, &ranks);

        // Ordering should be deterministic
        assert_eq!(ordered1, ordered2);
    }

    #[test]
    fn test_order_nodes_deterministic_with_strategy() {
        let schema = make_test_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph);

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
        let ranks = assign_ranks(&graph);

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
        let ranks = assign_ranks(&graph);

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
        let ranks = assign_ranks(&graph);

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
        let ranks = assign_ranks(&graph);
        let layered = LayeredGraph::new(&graph, &ranks);

        let min_crossings = [
            CrossingReductionStrategy::Barycenter,
            CrossingReductionStrategy::Median,
            CrossingReductionStrategy::Sifting,
        ]
        .into_iter()
        .map(|strategy| layered.reduce_crossings(strategy).1)
        .min()
        .unwrap();

        // Combined should not be worse than the best individual strategy
        let combined = layered.best_ordering(CrossingReductionStrategy::Combined).1;
        assert_eq!(combined, min_crossings);
        let combined_ordering = order_nodes_within_layers_with_strategy(
            &graph,
            &ranks,
            CrossingReductionStrategy::Combined,
        );
        assert_eq!(
            combined_ordering.iter().map(Vec::len).sum::<usize>(),
            graph.nodes.len()
        );
    }

    #[test]
    fn test_crossing_count() {
        // Two nodes in each of two layers: 0 -> 2 and 1 -> 3.
        let layered =
            LayeredGraph::from_parts(&[vec![0, 1], vec![2, 3]], &[0, 0, 1, 1], [(0, 2), (1, 3)]);
        assert_eq!(layered.count_crossings(&[vec![0, 1], vec![2, 3]]), 0);
        assert_eq!(layered.count_crossings(&[vec![0, 1], vec![3, 2]]), 1);
    }

    #[test]
    fn test_crossing_count_ignores_edges_sharing_a_source() {
        let layered =
            LayeredGraph::from_parts(&[vec![0], vec![1, 2]], &[0, 1, 1], [(0, 1), (0, 2)]);
        assert_eq!(layered.count_crossings(&[vec![0], vec![2, 1]]), 0);
    }

    #[test]
    fn test_long_edges_are_split_into_virtual_nodes() {
        // 0 (layer 0) -> 3 (layer 3) spans two intermediate layers.
        let layered = LayeredGraph::from_parts(
            &[vec![0], vec![1], vec![2], vec![3]],
            &[0, 1, 2, 3],
            [(0, 3), (0, 1), (1, 2)],
        );
        assert_eq!(layered.adjacency.len(), 6);
        assert_eq!(layered.layers[1], vec![1, 4]);
        assert_eq!(layered.layers[2], vec![2, 5]);
        assert_eq!(layered.adjacency[3], vec![5]);
        assert_eq!(
            layered.without_virtual_nodes(layered.layers.clone()),
            vec![vec![0], vec![1], vec![2], vec![3]]
        );
    }

    #[test]
    fn test_long_edge_crossings_are_counted() {
        // Layer 0: [a=0, b=1], layer 1: [c=2], layer 2: [d=3, e=4].
        // a -> e spans layer 1 through a virtual node; b -> c -> d is short.
        let layered = LayeredGraph::from_parts(
            &[vec![0, 1], vec![2], vec![3, 4]],
            &[0, 0, 1, 2, 2],
            [(0, 4), (1, 2), (2, 3)],
        );
        let virtual_node = 5;
        // Virtual node right of c while a is left of b: the long edge crosses b -> c.
        assert_eq!(
            layered.count_crossings(&[vec![0, 1], vec![2, virtual_node], vec![3, 4]]),
            1
        );
        let (best, crossings) = layered.reduce_crossings(CrossingReductionStrategy::Barycenter);
        assert_eq!(crossings, 0);
        assert_eq!(layered.count_crossings(&best), 0);
    }

    #[test]
    fn test_reduce_crossings_keeps_improvement_from_a_downward_sweep() {
        // The downward sweep reaches one crossing and the following upward
        // sweep returns to two; the better ordering must be kept.
        let layered = LayeredGraph::from_parts(
            &[vec![0, 1, 2], vec![3, 4, 5], vec![6, 7, 8]],
            &[0, 0, 0, 1, 1, 1, 2, 2, 2],
            [(0, 3), (0, 5), (1, 4), (2, 4), (3, 6), (4, 6), (5, 7)],
        );
        assert_eq!(layered.count_crossings(&layered.layers), 2);
        let (best, crossings) = layered.reduce_crossings(CrossingReductionStrategy::Barycenter);
        assert!(crossings <= 1);
        assert_eq!(layered.count_crossings(&best), crossings);
    }

    #[test]
    fn test_virtual_nodes_stay_within_budget_for_dense_deep_graphs() {
        // A transitively closed chain would need C(n, 3) virtual nodes.
        let count = 120;
        let node_rank: Vec<usize> = (0..count).collect();
        let nodes_by_rank: Vec<Vec<usize>> = (0..count).map(|node| vec![node]).collect();
        let edges =
            (0..count).flat_map(|upper| (upper + 1..count).map(move |lower| (upper, lower)));
        let layered = LayeredGraph::from_parts(&nodes_by_rank, &node_rank, edges);

        assert!(layered.adjacency.len() - count <= virtual_node_budget(count));
        let (best, _) = layered.reduce_crossings(CrossingReductionStrategy::Barycenter);
        assert_eq!(layered.without_virtual_nodes(best), nodes_by_rank);
    }

    #[test]
    fn test_nodes_without_neighbors_keep_their_slot() {
        // Node 9 has no neighbour in the adjacent layer and stays first while
        // the connected nodes swap to follow their neighbours.
        let mut adjacency = vec![Vec::new(); 10];
        adjacency[1] = vec![5];
        adjacency[2] = vec![4];
        let ordered = order_by_key(&[9, 1, 2], &[4, 5], &adjacency, barycenter);
        assert_eq!(ordered, vec![9, 2, 1]);
    }

    #[test]
    fn test_reduce_crossings_never_worsens_the_initial_ordering() {
        for seed in 0..20_usize {
            let layer_count = 5;
            let per_layer = 6;
            let node_rank: Vec<usize> = (0..layer_count * per_layer)
                .map(|node| node / per_layer)
                .collect();
            let nodes_by_rank: Vec<Vec<usize>> = (0..layer_count)
                .map(|layer| (layer * per_layer..(layer + 1) * per_layer).collect())
                .collect();
            let edges: Vec<(usize, usize)> = (0..40)
                .map(|step| {
                    let from = (seed * 7 + step * 13) % node_rank.len();
                    let to = (seed * 11 + step * 5 + 3) % node_rank.len();
                    (from, to)
                })
                .filter(|(from, to)| node_rank[*from] != node_rank[*to])
                .collect();
            let layered = LayeredGraph::from_parts(&nodes_by_rank, &node_rank, edges);
            let initial = layered.count_crossings(&layered.layers);
            for strategy in [
                CrossingReductionStrategy::Barycenter,
                CrossingReductionStrategy::Median,
                CrossingReductionStrategy::Sifting,
            ] {
                let (best, crossings) = layered.reduce_crossings(strategy);
                assert!(crossings <= initial, "seed {seed} {strategy:?}");
                assert_eq!(layered.count_crossings(&best), crossings);
            }
        }
    }

    #[test]
    fn test_all_strategies_produce_valid_ordering() {
        let schema = make_complex_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph);

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
