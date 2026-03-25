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
    /// Network simplex (better quality, slower).
    NetworkSimplex,
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
        RankAssignmentStrategy::NetworkSimplex => {
            // Fall back to longest path for now
            assign_ranks_longest_path(graph)
        }
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
    let n = graph.nodes.len();
    if n == 0 {
        return RankAssignment {
            node_rank: Vec::new(),
            num_ranks: 0,
            nodes_by_rank: Vec::new(),
        };
    }

    // Build adjacency list and in-degree count
    let mut in_degree = vec![0usize; n];
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); n];

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

    // Kahn's algorithm with level tracking
    let mut node_rank = vec![0usize; n];
    let mut queue: VecDeque<usize> = VecDeque::new();

    // Start with nodes that have no incoming edges
    for (idx, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(idx);
        }
    }

    let mut processed = 0;
    while let Some(idx) = queue.pop_front() {
        processed += 1;
        for &neighbor in &adjacency[idx] {
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                node_rank[neighbor] = node_rank[idx] + 1;
                queue.push_back(neighbor);
            }
        }
    }

    // Handle cycles - assign remaining nodes to appropriate ranks
    if processed < n {
        for idx in 0..n {
            if node_rank[idx] == 0 && in_degree[idx] > 0 {
                node_rank[idx] = 0;
            }
        }
    }

    build_rank_assignment(graph, node_rank)
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

    // Build reverse adjacency (incoming edges)
    let mut incoming: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n]; // (from_idx, edge_idx)

    for (edge_idx, edge) in graph.edges.iter().enumerate() {
        if edge.is_self_loop {
            continue;
        }
        if let (Some(&from_idx), Some(&to_idx)) = (
            graph.node_index.get(&edge.from),
            graph.node_index.get(&edge.to),
        ) {
            incoming[to_idx].push((from_idx, edge_idx));
        }
    }

    // Find nodes with no incoming edges (sources)
    #[allow(clippy::collection_is_never_read)]
    let mut sources = Vec::new();
    for (idx, inc) in incoming.iter().enumerate().take(n) {
        if inc.is_empty() {
            sources.push(idx);
        }
    }

    // Longest path from sources
    let mut node_rank = vec![0usize; n];
    let mut visited = vec![false; n];

    #[allow(clippy::items_after_statements)]
    fn dfs(
        idx: usize,
        incoming: &[Vec<(usize, usize)>],
        node_rank: &mut [usize],
        visited: &mut [bool],
    ) {
        if visited[idx] {
            return;
        }
        visited[idx] = true;

        let mut max_pred_rank = 0;
        for &(pred_idx, _) in &incoming[idx] {
            dfs(pred_idx, incoming, node_rank, visited);
            max_pred_rank = max_pred_rank.max(node_rank[pred_idx]);
        }

        if !incoming[idx].is_empty() {
            node_rank[idx] = max_pred_rank + 1;
        }
    }

    // Process all nodes
    for idx in 0..n {
        dfs(idx, &incoming, &mut node_rank, &mut visited);
    }

    // Handle isolated nodes and self-loops
    for idx in 0..n {
        if !visited[idx] {
            node_rank[idx] = 0;
        }
    }

    build_rank_assignment(graph, node_rank)
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

        // The FK chain is: c -> b -> a (FK edges point from source to referenced table)
        // Topological sort gives rank 0 to nodes with no incoming edges
        // So c has rank 0, b has rank 1, a has rank 2
        let a_rank = *node_ranks.get("a").unwrap();
        let b_rank = *node_ranks.get("b").unwrap();
        let c_rank = *node_ranks.get("c").unwrap();

        assert_eq!(c_rank, 0); // c has no incoming FK edges
        assert_eq!(b_rank, 1); // b is referenced by c
        assert_eq!(a_rank, 2); // a is referenced by b
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
}
