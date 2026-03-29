//! Graph metadata for embedding in HTML output.
//!
//! This metadata is embedded as JSON in the HTML document for future features
//! like search, filtering, and highlighting.

use relune_core::{EdgeKind, NodeKind};
use relune_layout::{DiagramOverlay, LayoutGraph, overlay::edge_key};
use serde::{Deserialize, Serialize};

/// Metadata about the graph for client-side features.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMetadata {
    /// Table/node information.
    pub tables: Vec<TableMetadata>,
    /// Edge/relation information.
    pub edges: Vec<EdgeMetadata>,
    /// Group information.
    pub groups: Vec<GroupMetadata>,
}

/// Metadata about a single table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableMetadata {
    /// Unique identifier for the table.
    pub id: String,
    /// Display label (may include schema prefix).
    pub label: String,
    /// Schema name (if any).
    pub schema_name: Option<String>,
    /// Table name.
    pub table_name: String,
    /// Node kind.
    pub kind: NodeKind,
    /// Column information.
    pub columns: Vec<ColumnMetadata>,
    /// Number of inbound foreign keys.
    pub inbound_count: usize,
    /// Number of outbound foreign keys.
    pub outbound_count: usize,
    /// Whether this is likely a join table.
    pub is_join_table_candidate: bool,
    /// Overlay lint/health annotations (empty when no overlay provided).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<IssueMetadata>,
}

/// Metadata about a column.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ColumnMetadata {
    /// Column name.
    pub name: String,
    /// Data type.
    pub data_type: String,
    /// Whether the column is nullable.
    pub nullable: bool,
    /// Whether the column is a primary key.
    pub is_primary_key: bool,
    /// Whether the column participates in a foreign key.
    #[serde(default)]
    pub is_foreign_key: bool,
    /// Whether the column appears in an index.
    #[serde(default)]
    pub is_indexed: bool,
}

/// Metadata about a single edge/relation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeMetadata {
    /// Source table ID.
    pub from: String,
    /// Target table ID.
    pub to: String,
    /// Foreign key name (if any).
    pub name: Option<String>,
    /// Source columns.
    pub from_columns: Vec<String>,
    /// Target columns.
    pub to_columns: Vec<String>,
    /// Edge kind.
    pub kind: EdgeKind,
    /// Overlay lint/health annotations (empty when no overlay provided).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<IssueMetadata>,
}

/// A single lint/health issue for client-side display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueMetadata {
    /// Severity level.
    pub severity: String,
    /// Short description.
    pub message: String,
    /// Optional resolution hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Optional rule identifier (e.g. `"no-primary-key"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
}

/// Metadata about a group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMetadata {
    /// Group identifier.
    pub id: String,
    /// Group label.
    pub label: String,
    /// IDs of tables in this group.
    pub table_ids: Vec<String>,
}

/// Build metadata from a layout graph with optional overlay annotations.
pub fn build_metadata_with_overlay(
    graph: &LayoutGraph,
    overlay: Option<&DiagramOverlay>,
) -> GraphMetadata {
    let tables: Vec<TableMetadata> = graph
        .nodes
        .iter()
        .map(|node| {
            let issues = overlay
                .and_then(|o| o.node(&node.id))
                .map(|no| annotations_to_issues(&no.annotations))
                .unwrap_or_default();
            TableMetadata {
                id: node.id.clone(),
                label: node.label.clone(),
                schema_name: node.schema_name.clone(),
                table_name: node.table_name.clone(),
                kind: node.kind,
                columns: node
                    .columns
                    .iter()
                    .map(|c| ColumnMetadata {
                        name: c.name.clone(),
                        data_type: c.data_type.clone(),
                        nullable: c.nullable,
                        is_primary_key: c.is_primary_key,
                        is_foreign_key: c.is_foreign_key,
                        is_indexed: c.is_indexed,
                    })
                    .collect(),
                inbound_count: node.inbound_count,
                outbound_count: node.outbound_count,
                is_join_table_candidate: node.is_join_table_candidate,
                issues,
            }
        })
        .collect();

    let edges: Vec<EdgeMetadata> = graph
        .edges
        .iter()
        .map(|edge| {
            let issues = overlay
                .and_then(|o| o.edges.get(&edge_key(&edge.from, &edge.to)))
                .map(|eo| annotations_to_issues(&eo.annotations))
                .unwrap_or_default();
            EdgeMetadata {
                from: edge.from.clone(),
                to: edge.to.clone(),
                name: edge.name.clone(),
                from_columns: edge.from_columns.clone(),
                to_columns: edge.to_columns.clone(),
                kind: edge.kind,
                issues,
            }
        })
        .collect();

    let groups: Vec<GroupMetadata> = graph
        .groups
        .iter()
        .map(|group| GroupMetadata {
            id: group.id.clone(),
            label: group.label.clone(),
            table_ids: group
                .node_indices
                .iter()
                .filter_map(|&idx| graph.nodes.get(idx).map(|n| n.id.clone()))
                .collect(),
        })
        .collect();

    GraphMetadata {
        tables,
        edges,
        groups,
    }
}

fn annotations_to_issues(annotations: &[relune_layout::overlay::Annotation]) -> Vec<IssueMetadata> {
    annotations
        .iter()
        .map(|a| {
            let severity = match a.severity {
                relune_layout::OverlaySeverity::Error => "error",
                relune_layout::OverlaySeverity::Warning => "warning",
                relune_layout::OverlaySeverity::Info => "info",
                relune_layout::OverlaySeverity::Hint => "hint",
            };
            IssueMetadata {
                severity: severity.to_string(),
                message: a.message.clone(),
                hint: a.hint.clone(),
                rule_id: a.rule_id.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use relune_core::{EdgeKind, NodeKind};
    use relune_layout::graph::LayoutColumn;
    use relune_layout::{LayoutEdge, LayoutGraph, LayoutGroup, LayoutNode};

    fn create_test_graph() -> LayoutGraph {
        LayoutGraph {
            nodes: vec![LayoutNode {
                id: "users".to_string(),
                label: "public.users".to_string(),
                schema_name: Some("public".to_string()),
                table_name: "users".to_string(),
                kind: NodeKind::Table,
                columns: vec![LayoutColumn {
                    name: "id".to_string(),
                    data_type: "integer".to_string(),
                    nullable: false,
                    is_primary_key: true,
                    is_foreign_key: false,
                    is_indexed: false,
                }],
                inbound_count: 1,
                outbound_count: 0,
                has_self_loop: false,
                is_join_table_candidate: false,
                group_index: Some(0),
            }],
            edges: vec![LayoutEdge {
                from: "posts".to_string(),
                to: "users".to_string(),
                name: Some("fk_posts_user".to_string()),
                from_columns: vec!["user_id".to_string()],
                to_columns: vec!["id".to_string()],
                kind: EdgeKind::ForeignKey,
                is_self_loop: false,
                nullable: false,
                target_cardinality: relune_core::layout::Cardinality::One,
                is_collapsed_join: false,
                collapsed_join_table: None,
            }],
            groups: vec![LayoutGroup {
                id: "schema_0".to_string(),
                label: "public".to_string(),
                node_indices: vec![0],
            }],
            node_index: std::collections::BTreeMap::new(),
            reverse_index: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn test_build_metadata() {
        let graph = create_test_graph();
        let metadata = build_metadata_with_overlay(&graph, None);

        assert_eq!(metadata.tables.len(), 1);
        assert_eq!(metadata.edges.len(), 1);
        assert_eq!(metadata.groups.len(), 1);
    }

    #[test]
    fn test_table_metadata() {
        let graph = create_test_graph();
        let metadata = build_metadata_with_overlay(&graph, None);

        let table = &metadata.tables[0];
        assert_eq!(table.id, "users");
        assert_eq!(table.label, "public.users");
        assert_eq!(table.schema_name, Some("public".to_string()));
        assert_eq!(table.table_name, "users");
        assert_eq!(table.kind, NodeKind::Table);
        assert_eq!(table.columns.len(), 1);
        assert_eq!(table.inbound_count, 1);
        assert_eq!(table.outbound_count, 0);
        assert!(!table.is_join_table_candidate);
    }

    #[test]
    fn test_column_metadata() {
        let graph = create_test_graph();
        let metadata = build_metadata_with_overlay(&graph, None);

        let column = &metadata.tables[0].columns[0];
        assert_eq!(column.name, "id");
        assert_eq!(column.data_type, "integer");
        assert!(!column.nullable);
        assert!(column.is_primary_key);
        assert!(!column.is_foreign_key);
        assert!(!column.is_indexed);
    }

    #[test]
    fn test_column_metadata_preserves_layout_flags() {
        let graph = LayoutGraph {
            nodes: vec![LayoutNode {
                id: "posts".to_string(),
                label: "posts".to_string(),
                schema_name: None,
                table_name: "posts".to_string(),
                kind: NodeKind::Table,
                columns: vec![LayoutColumn {
                    name: "user_id".to_string(),
                    data_type: "integer".to_string(),
                    nullable: false,
                    is_primary_key: false,
                    is_foreign_key: true,
                    is_indexed: true,
                }],
                inbound_count: 0,
                outbound_count: 1,
                has_self_loop: false,
                is_join_table_candidate: false,
                group_index: None,
            }],
            edges: vec![],
            groups: vec![],
            node_index: std::collections::BTreeMap::new(),
            reverse_index: std::collections::BTreeMap::new(),
        };

        let metadata = build_metadata_with_overlay(&graph, None);
        let column = &metadata.tables[0].columns[0];

        assert!(column.is_foreign_key);
        assert!(column.is_indexed);
    }

    #[test]
    fn test_edge_metadata() {
        let graph = create_test_graph();
        let metadata = build_metadata_with_overlay(&graph, None);

        let edge = &metadata.edges[0];
        assert_eq!(edge.from, "posts");
        assert_eq!(edge.to, "users");
        assert_eq!(edge.name, Some("fk_posts_user".to_string()));
        assert_eq!(edge.from_columns, vec!["user_id"]);
        assert_eq!(edge.to_columns, vec!["id"]);
        assert_eq!(edge.kind, EdgeKind::ForeignKey);
    }

    #[test]
    fn test_group_metadata() {
        let graph = create_test_graph();
        let metadata = build_metadata_with_overlay(&graph, None);

        let group = &metadata.groups[0];
        assert_eq!(group.id, "schema_0");
        assert_eq!(group.label, "public");
        assert_eq!(group.table_ids, vec!["users"]);
    }

    #[test]
    fn test_metadata_serialization() {
        let graph = create_test_graph();
        let metadata = build_metadata_with_overlay(&graph, None);

        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains(r#""id":"users""#));
        assert!(json.contains(r#""from":"posts""#));

        // Round-trip
        let deserialized: GraphMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tables.len(), 1);
        assert_eq!(deserialized.edges.len(), 1);
    }

    #[test]
    fn test_metadata_with_overlay_embeds_node_issues() {
        let graph = create_test_graph();
        let mut overlay = DiagramOverlay::new();
        overlay.add_node_annotation(
            "users",
            relune_layout::Annotation {
                severity: relune_layout::OverlaySeverity::Warning,
                message: "No primary key".to_string(),
                hint: Some("Add a PK column".to_string()),
                rule_id: Some("no-primary-key".to_string()),
            },
        );

        let metadata = build_metadata_with_overlay(&graph, Some(&overlay));

        let table = &metadata.tables[0];
        assert_eq!(table.issues.len(), 1);
        assert_eq!(table.issues[0].severity, "warning");
        assert_eq!(table.issues[0].message, "No primary key");
        assert_eq!(table.issues[0].hint.as_deref(), Some("Add a PK column"));
        assert_eq!(table.issues[0].rule_id.as_deref(), Some("no-primary-key"));
    }

    #[test]
    fn test_metadata_with_overlay_embeds_edge_issues() {
        let graph = create_test_graph();
        let mut overlay = DiagramOverlay::new();
        overlay.add_edge_annotation(
            "posts",
            "users",
            relune_layout::Annotation {
                severity: relune_layout::OverlaySeverity::Info,
                message: "Missing index on FK".to_string(),
                hint: None,
                rule_id: None,
            },
        );

        let metadata = build_metadata_with_overlay(&graph, Some(&overlay));

        let edge = &metadata.edges[0];
        assert_eq!(edge.issues.len(), 1);
        assert_eq!(edge.issues[0].severity, "info");
        assert_eq!(edge.issues[0].message, "Missing index on FK");
    }

    #[test]
    fn test_metadata_without_overlay_has_no_issues() {
        let graph = create_test_graph();
        let metadata = build_metadata_with_overlay(&graph, None);

        assert!(metadata.tables[0].issues.is_empty());
        assert!(metadata.edges[0].issues.is_empty());
    }

    #[test]
    fn test_metadata_issues_omitted_in_json_when_empty() {
        let graph = create_test_graph();
        let metadata = build_metadata_with_overlay(&graph, None);

        let json = serde_json::to_string(&metadata).unwrap();
        // "issues" key should not appear when empty (skip_serializing_if)
        assert!(!json.contains("\"issues\""));
    }
}
