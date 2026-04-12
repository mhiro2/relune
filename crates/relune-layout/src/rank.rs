//! Rank/layer assignment for hierarchical layout
//!
//! This module implements algorithms for assigning nodes to layers (ranks)
//! in a hierarchical graph layout.

use std::collections::VecDeque;

use crate::graph::LayoutGraph;
use tracing::warn;

/// Strategy for rank assignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RankAssignmentStrategy {
    /// Simple topological sort based ranking.
    #[default]
    Topological,
    /// Longest path ranking (minimizes height).
    LongestPath,
}

/// Result of rank assignment.
#[derive(Debug, Clone)]
pub struct RankAssignment {
    /// Map from node index to rank (layer).
    pub node_rank: Vec<usize>,
    /// Total number of ranks.
    pub num_ranks: usize,
    /// Nodes grouped by rank.
    pub nodes_by_rank: Vec<Vec<usize>>,
}

/// Assign ranks to nodes in the graph.
///
/// This uses a longest-path algorithm to assign nodes to layers,
/// ensuring that edges generally point downward (or in the layout direction).
#[must_use]
pub fn assign_ranks(graph: &LayoutGraph, strategy: RankAssignmentStrategy) -> RankAssignment {
    warn_if_cycles(graph);

    match strategy {
        RankAssignmentStrategy::Topological => assign_ranks_topological(graph),
        RankAssignmentStrategy::LongestPath => assign_ranks_longest_path(graph),
    }
}

fn warn_if_cycles(graph: &LayoutGraph) {
    let cycle_nodes = detect_cycle_nodes(graph);
    if cycle_nodes.is_empty() {
        return;
    }

    warn!(
        count = cycle_nodes.len(),
        nodes = ?cycle_nodes,
        "Cycle detected; rank assignment will keep remaining nodes in a fallback order"
    );
}

fn detect_cycle_nodes(graph: &LayoutGraph) -> Vec<String> {
    let n = graph.nodes.len();
    let mut in_degree = vec![0usize; n];
    let mut adjacency = vec![Vec::new(); n];

    for edge in &graph.edges {
        if edge.is_self_loop {
            continue;
        }
        if let (Some(&from_idx), Some(&to_idx)) = (
            graph.node_index.get(&edge.from),
            graph.node_index.get(&edge.to),
        ) {
            adjacency[from_idx].push(to_idx);
            in_degree[to_idx] += 1;
        }
    }

    let mut queue = VecDeque::new();
    for (idx, &degree) in in_degree.iter().enumerate() {
        if degree == 0 {
            queue.push_back(idx);
        }
    }

    let mut processed = 0usize;
    while let Some(idx) = queue.pop_front() {
        processed += 1;
        for &neighbor in &adjacency[idx] {
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                queue.push_back(neighbor);
            }
        }
    }

    if processed == n {
        return Vec::new();
    }

    (0..n)
        .filter(|&idx| in_degree[idx] > 0)
        .map(|idx| graph.nodes[idx].id.clone())
        .collect()
}

fn assign_ranks_topological(graph: &LayoutGraph) -> RankAssignment {
    assign_ranks_via_components(graph)
}

fn assign_ranks_longest_path(graph: &LayoutGraph) -> RankAssignment {
    let n = graph.nodes.len();
    if n == 0 {
        return RankAssignment {
            node_rank: Vec::new(),
            num_ranks: 0,
            nodes_by_rank: Vec::new(),
        };
    }

    assign_ranks_via_components(graph)
}

#[derive(Debug)]
struct StronglyConnectedComponents {
    component_of: Vec<usize>,
    components: Vec<Vec<usize>>,
}

fn assign_ranks_via_components(graph: &LayoutGraph) -> RankAssignment {
    let n = graph.nodes.len();
    if n == 0 {
        return RankAssignment {
            node_rank: Vec::new(),
            num_ranks: 0,
            nodes_by_rank: Vec::new(),
        };
    }

    let scc = strongly_connected_components(graph);
    let component_count = scc.components.len();
    let mut adjacency = vec![Vec::new(); component_count];
    let mut in_degree = vec![0usize; component_count];

    for edge in &graph.edges {
        if edge.is_self_loop {
            continue;
        }
        if let (Some(&from_idx), Some(&to_idx)) = (
            graph.node_index.get(&edge.from),
            graph.node_index.get(&edge.to),
        ) {
            let from_component = scc.component_of[from_idx];
            let to_component = scc.component_of[to_idx];
            if from_component != to_component && !adjacency[to_component].contains(&from_component)
            {
                adjacency[to_component].push(from_component);
                in_degree[from_component] += 1;
            }
        }
    }

    let component_height: Vec<usize> = scc
        .components
        .iter()
        .map(|component| component.len().max(1))
        .collect();
    let mut base_rank = vec![0usize; component_count];
    let mut queue = VecDeque::new();

    for (component_idx, &degree) in in_degree.iter().enumerate() {
        if degree == 0 {
            queue.push_back(component_idx);
        }
    }

    while let Some(component_idx) = queue.pop_front() {
        let next_rank = base_rank[component_idx] + component_height[component_idx];
        for &neighbor in &adjacency[component_idx] {
            base_rank[neighbor] = base_rank[neighbor].max(next_rank);
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                queue.push_back(neighbor);
            }
        }
    }

    let mut node_rank = vec![0usize; n];
    for (component_idx, component_nodes) in scc.components.iter().enumerate() {
        let mut ordered_nodes = component_nodes.clone();
        ordered_nodes.sort_unstable();
        for (offset, node_idx) in ordered_nodes.into_iter().enumerate() {
            node_rank[node_idx] = base_rank[component_idx] + offset;
        }
    }

    build_rank_assignment(graph, node_rank)
}

#[allow(clippy::items_after_statements)] // Keep Tarjan helpers scoped to SCC construction.
fn strongly_connected_components(graph: &LayoutGraph) -> StronglyConnectedComponents {
    let n = graph.nodes.len();
    let mut adjacency = vec![Vec::new(); n];
    for edge in &graph.edges {
        if edge.is_self_loop {
            continue;
        }
        if let (Some(&from_idx), Some(&to_idx)) = (
            graph.node_index.get(&edge.from),
            graph.node_index.get(&edge.to),
        ) {
            adjacency[from_idx].push(to_idx);
        }
    }

    struct TarjanState {
        index: usize,
        indices: Vec<Option<usize>>,
        lowlinks: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        component_of: Vec<usize>,
        components: Vec<Vec<usize>>,
    }

    fn strong_connect(idx: usize, adjacency: &[Vec<usize>], state: &mut TarjanState) {
        let current_index = state.index;
        state.indices[idx] = Some(current_index);
        state.lowlinks[idx] = current_index;
        state.index += 1;
        state.stack.push(idx);
        state.on_stack[idx] = true;

        for &neighbor in &adjacency[idx] {
            if state.indices[neighbor].is_none() {
                strong_connect(neighbor, adjacency, state);
                state.lowlinks[idx] = state.lowlinks[idx].min(state.lowlinks[neighbor]);
            } else if state.on_stack[neighbor] {
                state.lowlinks[idx] =
                    state.lowlinks[idx].min(state.indices[neighbor].unwrap_or_default());
            }
        }

        if state.lowlinks[idx] == current_index {
            let component_idx = state.components.len();
            let mut component = Vec::new();
            while let Some(node_idx) = state.stack.pop() {
                state.on_stack[node_idx] = false;
                state.component_of[node_idx] = component_idx;
                component.push(node_idx);
                if node_idx == idx {
                    break;
                }
            }
            state.components.push(component);
        }
    }

    let mut state = TarjanState {
        index: 0,
        indices: vec![None; n],
        lowlinks: vec![0; n],
        stack: Vec::new(),
        on_stack: vec![false; n],
        component_of: vec![0; n],
        components: Vec::new(),
    };

    for idx in 0..n {
        if state.indices[idx].is_none() {
            strong_connect(idx, &adjacency, &mut state);
        }
    }

    StronglyConnectedComponents {
        component_of: state.component_of,
        components: state.components,
    }
}

fn build_rank_assignment(_graph: &LayoutGraph, node_rank: Vec<usize>) -> RankAssignment {
    let num_ranks = node_rank.iter().copied().max().map_or(0, |r| r + 1);

    let mut nodes_by_rank = vec![Vec::new(); num_ranks];
    for (idx, &rank) in node_rank.iter().enumerate() {
        nodes_by_rank[rank].push(idx);
    }

    RankAssignment {
        node_rank,
        num_ranks,
        nodes_by_rank,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::LayoutGraphBuilder;
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
                        from_columns: vec!["b_id".to_string()],
                        to_schema: None,
                        to_table: "b".to_string(),
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

    #[test]
    fn test_assign_ranks_topological() {
        let schema = make_test_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph, RankAssignmentStrategy::Topological);

        // Build a map from node ID to rank for easier testing
        let node_ranks: std::collections::BTreeMap<&str, usize> = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(idx, node)| (node.id.as_str(), ranks.node_rank[idx]))
            .collect();

        // The FK chain is: c -> b -> a (FK edges point from child to referenced parent)
        // Parent tables get lower ranks so they appear first in the layout direction.
        // So a has rank 0, b has rank 1, c has rank 2
        let a_rank = *node_ranks.get("a").unwrap();
        let b_rank = *node_ranks.get("b").unwrap();
        let c_rank = *node_ranks.get("c").unwrap();

        assert_eq!(a_rank, 0); // a is the root parent
        assert_eq!(b_rank, 1); // b references a
        assert_eq!(c_rank, 2); // c references b
        assert_eq!(ranks.num_ranks, 3);
    }

    #[test]
    fn test_assign_ranks_longest_path() {
        let schema = make_test_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph, RankAssignmentStrategy::LongestPath);

        assert_eq!(ranks.num_ranks, 3);
    }

    #[test]
    fn test_detect_cycle_nodes() {
        let schema = Schema {
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
                    foreign_keys: vec![ForeignKey {
                        name: None,
                        from_columns: vec!["b_id".to_string()],
                        to_schema: None,
                        to_table: "b".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    }],
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
        };

        let graph = LayoutGraphBuilder::new().build(&schema);
        assert_eq!(
            detect_cycle_nodes(&graph),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn test_assign_ranks_spreads_cycle_nodes_across_layers() {
        let schema = Schema {
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
                    foreign_keys: vec![ForeignKey {
                        name: None,
                        from_columns: vec!["b_id".to_string()],
                        to_schema: None,
                        to_table: "b".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    }],
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
                        from_columns: vec!["c_id".to_string()],
                        to_schema: None,
                        to_table: "c".to_string(),
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
            ],
            views: vec![],
            enums: vec![],
        };

        let graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&graph, RankAssignmentStrategy::LongestPath);
        let mut cycle_ranks = ranks.node_rank;
        cycle_ranks.sort_unstable();

        assert_eq!(cycle_ranks, vec![0, 1, 2]);
    }
}
