//! Focus extraction for subgraph views
//!
//! This module provides functionality to extract a focused subgraph
//! centered on a specific table with configurable depth.

use std::collections::{BTreeMap, BTreeSet};

use relune_core::{FocusSpec, Schema};
use thiserror::Error;

use crate::graph::{LayoutEdge, LayoutGraph, LayoutGraphBuilder, LayoutNode};

/// Error during focus extraction.
#[derive(Debug, Error)]
pub enum FocusError {
    /// The specified focus target table was not found in the graph.
    #[error("focus target table not found: {0}")]
    TargetNotFound(String),
}

/// Extractor for focused subgraphs.
#[derive(Debug, Default)]
pub struct FocusExtractor;

impl FocusExtractor {
    /// Extract a focused subgraph from the given graph.
    ///
    /// Returns a new graph containing only the nodes within the specified
    /// depth from the focus target, along with edges between those nodes.
    pub fn extract(
        &self,
        graph: &LayoutGraph,
        focus: &FocusSpec,
    ) -> Result<LayoutGraph, FocusError> {
        // Find the target node
        let target_idx = graph
            .node_index
            .get(&focus.table)
            .copied()
            .or_else(|| {
                // Try matching by table name or qualified name
                graph
                    .nodes
                    .iter()
                    .position(|n| n.table_name == focus.table || n.label == focus.table)
            })
            .ok_or_else(|| FocusError::TargetNotFound(focus.table.clone()))?;

        // Find all nodes within the specified depth using BFS
        let included_indices = self.find_nodes_within_depth(graph, target_idx, focus.depth);

        // Build the filtered graph
        Ok(self.build_focused_graph(graph, &included_indices))
    }

    /// Extract a focused subgraph from a schema directly.
    pub fn extract_from_schema(
        &self,
        schema: &Schema,
        focus: &FocusSpec,
    ) -> Result<LayoutGraph, FocusError> {
        let full_graph = LayoutGraphBuilder::new().build(schema);
        self.extract(&full_graph, focus)
    }

    /// Find all node indices within the specified depth from the target.
    #[allow(clippy::unused_self)]
    fn find_nodes_within_depth(
        &self,
        graph: &LayoutGraph,
        target_idx: usize,
        depth: u32,
    ) -> BTreeSet<usize> {
        let mut visited = BTreeSet::new();
        let mut current_level = BTreeSet::new();
        current_level.insert(target_idx);

        // Build adjacency map for both directions
        let mut adjacency: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        for edge in &graph.edges {
            if let (Some(&from_idx), Some(&to_idx)) = (
                graph.node_index.get(&edge.from),
                graph.node_index.get(&edge.to),
            ) {
                adjacency.entry(from_idx).or_default().insert(to_idx);
                adjacency.entry(to_idx).or_default().insert(from_idx);
            }
        }

        // BFS for the specified depth
        for _ in 0..=depth {
            visited.extend(current_level.iter());
            let mut next_level = BTreeSet::new();

            for &idx in &current_level {
                if let Some(neighbors) = adjacency.get(&idx) {
                    for &neighbor in neighbors {
                        if !visited.contains(&neighbor) {
                            next_level.insert(neighbor);
                        }
                    }
                }
            }

            current_level = next_level;
            if current_level.is_empty() {
                break;
            }
        }

        visited
    }

    /// Build a new graph containing only the included nodes and their edges.
    #[allow(clippy::unused_self)]
    fn build_focused_graph(
        &self,
        graph: &LayoutGraph,
        included_indices: &BTreeSet<usize>,
    ) -> LayoutGraph {
        let nodes: Vec<LayoutNode> = included_indices
            .iter()
            .map(|&idx| {
                let mut node = graph.nodes[idx].clone();
                node.group_index = None; // Reset group indices
                node
            })
            .collect();

        // Filter edges to only those between included nodes
        let edges: Vec<LayoutEdge> = graph
            .edges
            .iter()
            .filter(|edge| {
                let from_included = graph
                    .node_index
                    .get(&edge.from)
                    .is_some_and(|&idx| included_indices.contains(&idx));
                let to_included = graph
                    .node_index
                    .get(&edge.to)
                    .is_some_and(|&idx| included_indices.contains(&idx));
                from_included && to_included
            })
            .cloned()
            .collect();

        // Rebuild indices
        let mut node_index = BTreeMap::new();
        let mut reverse_index = BTreeMap::new();
        for (i, node) in nodes.iter().enumerate() {
            node_index.insert(node.id.clone(), i);
            reverse_index.insert(i, node.id.clone());
        }

        // Build new groups (empty for focused graphs by default)
        let groups = Vec::new();

        LayoutGraph {
            nodes,
            edges,
            groups,
            node_index,
            reverse_index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_schema() -> Schema {
        use relune_core::{Column, ColumnId, ForeignKey, ReferentialAction, Table, TableId};

        Schema {
            tables: vec![
                Table {
                    id: TableId(1),
                    stable_id: "users".to_string(),
                    schema_name: None,
                    name: "users".to_string(),
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
                    stable_id: "posts".to_string(),
                    schema_name: None,
                    name: "posts".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(2),
                            name: "id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: true,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(3),
                            name: "user_id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![ForeignKey {
                        name: Some("fk_posts_user".to_string()),
                        from_columns: vec!["user_id".to_string()],
                        to_schema: None,
                        to_table: "users".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    }],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(3),
                    stable_id: "comments".to_string(),
                    schema_name: None,
                    name: "comments".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(4),
                            name: "id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: true,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(5),
                            name: "post_id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![ForeignKey {
                        name: Some("fk_comments_post".to_string()),
                        from_columns: vec!["post_id".to_string()],
                        to_schema: None,
                        to_table: "posts".to_string(),
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
    fn test_focus_extraction_depth_0() {
        let schema = make_test_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let extractor = FocusExtractor;

        let focus = FocusSpec {
            table: "posts".to_string(),
            depth: 0,
        };

        let focused = extractor.extract(&graph, &focus).unwrap();
        assert_eq!(focused.nodes.len(), 1);
        assert_eq!(focused.nodes[0].id, "posts");
    }

    #[test]
    fn test_focus_extraction_depth_1() {
        let schema = make_test_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let extractor = FocusExtractor;

        let focus = FocusSpec {
            table: "posts".to_string(),
            depth: 1,
        };

        let focused = extractor.extract(&graph, &focus).unwrap();
        // depth 1 from posts includes: posts, users (outgoing FK), comments (incoming FK)
        assert_eq!(focused.nodes.len(), 3);
        let node_ids: std::collections::BTreeSet<_> =
            focused.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(node_ids.contains("posts"));
        assert!(node_ids.contains("users"));
        assert!(node_ids.contains("comments"));
    }

    #[test]
    fn test_focus_extraction_target_not_found() {
        let schema = make_test_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let extractor = FocusExtractor;

        let focus = FocusSpec {
            table: "nonexistent".to_string(),
            depth: 1,
        };

        let result = extractor.extract(&graph, &focus);
        assert!(result.is_err());
    }
}
