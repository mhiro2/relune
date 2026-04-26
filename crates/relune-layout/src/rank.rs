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

/// Iterative Tarjan's SCC algorithm.
///
/// Uses an explicit work stack instead of recursion so that large or
/// long-chain graphs (especially under WASM's ~1 MB stack) cannot
/// overflow.
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

    let mut index: usize = 0;
    let mut indices: Vec<Option<usize>> = vec![None; n];
    let mut lowlinks: Vec<usize> = vec![0; n];
    let mut scc_stack: Vec<usize> = Vec::new();
    let mut on_stack: Vec<bool> = vec![false; n];
    let mut component_of: Vec<usize> = vec![0; n];
    let mut components: Vec<Vec<usize>> = Vec::new();

    // Each frame: (node, neighbor_cursor). When we first visit a node
    // we set cursor = 0 and push it onto scc_stack. On resume we pick
    // up where we left off in its neighbor list.
    let mut work: Vec<(usize, usize)> = Vec::new();

    for root in 0..n {
        if indices[root].is_some() {
            continue;
        }

        work.push((root, 0));

        while let Some((node, cursor)) = work.last_mut() {
            let node = *node;

            if *cursor == 0 {
                // First visit — initialise Tarjan state for this node.
                indices[node] = Some(index);
                lowlinks[node] = index;
                index += 1;
                scc_stack.push(node);
                on_stack[node] = true;
            }

            let neighbors = &adjacency[node];
            let mut descended = false;

            while *cursor < neighbors.len() {
                let neighbor = neighbors[*cursor];
                if indices[neighbor].is_none() {
                    // Descend into unvisited neighbor (simulates recursion).
                    *cursor += 1;
                    work.push((neighbor, 0));
                    descended = true;
                    break;
                }
                if on_stack[neighbor] {
                    lowlinks[node] = lowlinks[node].min(indices[neighbor].unwrap_or_default());
                }
                *cursor += 1;
            }
            if descended {
                continue;
            }

            // All neighbors processed — equivalent of the post-recursion
            // lowlink propagation and SCC extraction.
            if lowlinks[node] == indices[node].unwrap_or_default() {
                let comp_idx = components.len();
                let mut component = Vec::new();
                while let Some(top) = scc_stack.pop() {
                    on_stack[top] = false;
                    component_of[top] = comp_idx;
                    component.push(top);
                    if top == node {
                        break;
                    }
                }
                components.push(component);
            }

            work.pop();

            // Propagate lowlink to parent frame.
            if let Some((parent, _)) = work.last() {
                lowlinks[*parent] = lowlinks[*parent].min(lowlinks[node]);
            }
        }
    }

    StronglyConnectedComponents {
        component_of,
        components,
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
                    primary_key_name: None,
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
                    primary_key_name: None,
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
                    primary_key_name: None,
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
                    primary_key_name: None,
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
                    primary_key_name: None,
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
                    primary_key_name: None,
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
                    primary_key_name: None,
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
                    primary_key_name: None,
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
