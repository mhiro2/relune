//! Focus extraction for subgraph views
//!
//! This module provides functionality to extract a focused subgraph
//! centered on a specific table with configurable depth.

use std::collections::{BTreeMap, BTreeSet};

use crate::graph::{LayoutGraph, LayoutGraphBuilder, LayoutNode};
use relune_core::{FocusSpec, Schema};
use thiserror::Error;

/// Error during focus extraction.
#[derive(Debug, Error)]
pub enum FocusError {
    /// The specified focus target table was not found in the graph.
    #[error("focus target table not found: {table}")]
    TargetNotFound {
        /// Requested focus table.
        table: String,
    },

    /// The graph referenced an edge endpoint that does not exist in the node index.
    #[error(
        "focus extraction graph invariant violated for edge {from} -> {to}: missing {missing_endpoint} node"
    )]
    GraphInvariant {
        /// Source node ID.
        from: String,
        /// Target node ID.
        to: String,
        /// Missing endpoint label.
        missing_endpoint: &'static str,
    },
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
            .ok_or_else(|| FocusError::TargetNotFound {
                table: focus.table.clone(),
            })?;

        // Find all nodes within the specified depth using BFS
        let included_indices = self.find_nodes_within_depth(graph, target_idx, focus.depth)?;

        // Build the filtered graph
        self.build_focused_graph(graph, &included_indices)
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
    ) -> Result<BTreeSet<usize>, FocusError> {
        let mut visited = BTreeSet::new();
        let mut current_level = BTreeSet::new();
        current_level.insert(target_idx);

        // Build adjacency map for both directions
        let mut adjacency: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        for edge in &graph.edges {
            let Some(&from_idx) = graph.node_index.get(&edge.from) else {
                return Err(FocusError::GraphInvariant {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    missing_endpoint: "source",
                });
            };
            let Some(&to_idx) = graph.node_index.get(&edge.to) else {
                return Err(FocusError::GraphInvariant {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    missing_endpoint: "target",
                });
            };
            adjacency.entry(from_idx).or_default().insert(to_idx);
            adjacency.entry(to_idx).or_default().insert(from_idx);
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

        Ok(visited)
    }

    /// Build a new graph containing only the included nodes and their edges.
    #[allow(clippy::unused_self)]
    fn build_focused_graph(
        &self,
        graph: &LayoutGraph,
        included_indices: &BTreeSet<usize>,
    ) -> Result<LayoutGraph, FocusError> {
        let nodes: Vec<LayoutNode> = included_indices
            .iter()
            .map(|&idx| {
                let mut node = graph.nodes[idx].clone();
                node.group_index = None; // Reset group indices
                node
            })
            .collect();

        // Filter edges to only those between included nodes
        let mut edges = Vec::new();
        for edge in &graph.edges {
            let Some(&from_idx) = graph.node_index.get(&edge.from) else {
                return Err(FocusError::GraphInvariant {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    missing_endpoint: "source",
                });
            };
            let Some(&to_idx) = graph.node_index.get(&edge.to) else {
                return Err(FocusError::GraphInvariant {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    missing_endpoint: "target",
                });
            };

            if included_indices.contains(&from_idx) && included_indices.contains(&to_idx) {
                edges.push(edge.clone());
            }
        }

        // Rebuild indices
        let mut node_index = BTreeMap::new();
        let mut reverse_index = BTreeMap::new();
        for (i, node) in nodes.iter().enumerate() {
            node_index.insert(node.id.clone(), i);
            reverse_index.insert(i, node.id.clone());
        }

        // Build new groups (empty for focused graphs by default)
        let groups = Vec::new();

        Ok(LayoutGraph {
            nodes,
            edges,
            groups,
            node_index,
            reverse_index,
        })
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

    #[test]
    fn test_build_focused_graph_reports_dangling_edges() {
        let extractor = FocusExtractor;
        let graph = LayoutGraph {
            nodes: vec![
                LayoutNode {
                    id: "users".to_string(),
                    label: "users".to_string(),
                    schema_name: None,
                    table_name: "users".to_string(),
                    kind: relune_core::NodeKind::Table,
                    columns: vec![],
                    inbound_count: 0,
                    outbound_count: 0,
                    has_self_loop: false,
                    is_join_table_candidate: false,
                    group_index: Some(1),
                },
                LayoutNode {
                    id: "posts".to_string(),
                    label: "posts".to_string(),
                    schema_name: None,
                    table_name: "posts".to_string(),
                    kind: relune_core::NodeKind::Table,
                    columns: vec![],
                    inbound_count: 0,
                    outbound_count: 0,
                    has_self_loop: false,
                    is_join_table_candidate: false,
                    group_index: Some(2),
                },
            ],
            edges: vec![
                crate::LayoutEdge {
                    from: "posts".to_string(),
                    to: "users".to_string(),
                    name: Some("fk_posts_users".to_string()),
                    from_columns: vec!["user_id".to_string()],
                    to_columns: vec!["id".to_string()],
                    kind: relune_core::EdgeKind::ForeignKey,
                    is_self_loop: false,
                    nullable: false,
                    target_cardinality: relune_core::layout::Cardinality::One,
                    is_collapsed_join: false,
                    collapsed_join_table: None,
                },
                crate::LayoutEdge {
                    from: "ghost".to_string(),
                    to: "users".to_string(),
                    name: Some("fk_ghost_users".to_string()),
                    from_columns: vec!["ghost_id".to_string()],
                    to_columns: vec!["id".to_string()],
                    kind: relune_core::EdgeKind::ForeignKey,
                    is_self_loop: false,
                    nullable: false,
                    target_cardinality: relune_core::layout::Cardinality::One,
                    is_collapsed_join: false,
                    collapsed_join_table: None,
                },
            ],
            groups: vec![],
            node_index: BTreeMap::from([
                ("users".to_string(), 0_usize),
                ("posts".to_string(), 1_usize),
            ]),
            reverse_index: BTreeMap::from([
                (0_usize, "users".to_string()),
                (1_usize, "posts".to_string()),
            ]),
        };

        let error = extractor
            .build_focused_graph(&graph, &BTreeSet::from([0_usize, 1_usize]))
            .expect_err("dangling edges should surface a graph invariant error");

        assert!(matches!(
            error,
            FocusError::GraphInvariant {
                from,
                to,
                missing_endpoint: "source",
            } if from == "ghost" && to == "users"
        ));
    }
}
