//! Graph representation of a database schema.

use std::collections::{BTreeMap, HashMap, HashSet};

use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{Enum, Schema, Table, TableId};

/// The kind of node in the schema graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    /// A database table.
    Table,
    /// A database view.
    View,
    /// An enum type.
    Enum,
}

/// The kind of edge in the schema graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// A foreign key relationship between tables.
    ForeignKey,
    /// A column references an enum type.
    EnumReference,
    /// A view depends on a table or another view.
    ViewDependency,
}

/// A node in the schema graph representing a table, view, or enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Node identifier.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Column descriptions (empty for enums).
    pub columns: Vec<String>,
    /// Kind of this node.
    pub kind: NodeKind,
}

/// An edge in the schema graph representing a relationship.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Source node identifier.
    pub from: String,
    /// Target node identifier.
    pub to: String,
    /// Display label for the relationship.
    pub label: String,
    /// Whether the foreign key columns are nullable (only for FK edges).
    pub nullable: bool,
    /// The FK column names on the source table.
    pub from_columns: Vec<String>,
    /// The referenced column names on the target table.
    pub to_columns: Vec<String>,
    /// Kind of this edge.
    pub kind: EdgeKind,
}

/// A directed graph representing table relationships in a schema.
#[derive(Debug)]
pub struct SchemaGraph {
    /// The underlying petgraph structure.
    pub graph: DiGraph<GraphNode, GraphEdge>,
}

/// Errors that can occur when building a schema graph.
#[derive(Debug, Error)]
pub enum GraphBuildError {
    /// A foreign key references a table that doesn't exist.
    #[error("foreign key references unknown table: {0}")]
    UnknownTable(String),
    /// An enum reference is ambiguous across schemas.
    #[error("ambiguous enum reference for table '{table}': '{data_type}'")]
    AmbiguousEnumReference {
        /// Table where the ambiguity was encountered.
        table: String,
        /// The ambiguous enum type name.
        data_type: String,
    },
}

impl SchemaGraph {
    /// Builds a schema graph from a Schema.
    pub fn from_schema(schema: &Schema) -> Result<Self, GraphBuildError> {
        let mut graph = DiGraph::<GraphNode, GraphEdge>::new();
        let mut ids = BTreeMap::new();

        let name_index = Self::build_name_index(schema);
        let enum_index = Self::add_enum_nodes(&mut graph, schema);
        Self::add_table_nodes(&mut graph, &mut ids, schema);
        Self::add_table_edges(&mut graph, &ids, &name_index, &enum_index, schema)?;
        Self::add_view_nodes(&mut graph, &ids, schema);

        Ok(Self { graph })
    }

    fn build_name_index(schema: &Schema) -> HashMap<String, TableId> {
        let mut index = HashMap::with_capacity(schema.tables.len() * 2);
        for table in &schema.tables {
            index.insert(table.name.to_lowercase(), table.id);
            index.insert(table.stable_id.to_lowercase(), table.id);
        }
        index
    }

    fn add_enum_nodes(graph: &mut DiGraph<GraphNode, GraphEdge>, schema: &Schema) -> EnumIndex {
        let mut enum_index = EnumIndex::default();
        for enum_type in &schema.enums {
            let idx = graph.add_node(GraphNode {
                id: enum_type.id.clone(),
                label: enum_type.qualified_name(),
                columns: enum_type.values.iter().map(|v| format!("• {v}")).collect(),
                kind: NodeKind::Enum,
            });
            enum_index.insert(enum_type, idx);
        }
        enum_index
    }

    fn add_table_nodes(
        graph: &mut DiGraph<GraphNode, GraphEdge>,
        ids: &mut BTreeMap<TableId, NodeIndex>,
        schema: &Schema,
    ) {
        for table in &schema.tables {
            let idx = graph.add_node(GraphNode {
                id: table.stable_id.clone(),
                label: table.qualified_name(),
                columns: table
                    .columns
                    .iter()
                    .map(|column| {
                        let pk = if column.is_primary_key { " PK" } else { "" };
                        let nullable = if column.nullable { "?" } else { "" };
                        format!("{}: {}{}{}", column.name, column.data_type, nullable, pk)
                    })
                    .collect(),
                kind: NodeKind::Table,
            });
            ids.insert(table.id, idx);
        }
    }

    fn add_table_edges(
        graph: &mut DiGraph<GraphNode, GraphEdge>,
        ids: &BTreeMap<TableId, NodeIndex>,
        name_index: &HashMap<String, TableId>,
        enum_index: &EnumIndex,
        schema: &Schema,
    ) -> Result<(), GraphBuildError> {
        for table in &schema.tables {
            let from = *ids
                .get(&table.id)
                .ok_or_else(|| GraphBuildError::UnknownTable(table.stable_id.clone()))?;

            Self::add_fk_edges(graph, from, table, ids, name_index)?;
            Self::add_enum_ref_edges(graph, from, table, enum_index)?;
        }
        Ok(())
    }

    fn add_fk_edges(
        graph: &mut DiGraph<GraphNode, GraphEdge>,
        from: NodeIndex,
        table: &Table,
        ids: &BTreeMap<TableId, NodeIndex>,
        name_index: &HashMap<String, TableId>,
    ) -> Result<(), GraphBuildError> {
        for fk in &table.foreign_keys {
            let fk_target = fk.to_table.to_lowercase();
            let to = name_index
                .get(&fk_target)
                .and_then(|table_id| ids.get(table_id).copied())
                .ok_or_else(|| GraphBuildError::UnknownTable(fk.to_table.clone()))?;

            let fk_nullable = fk.from_columns.iter().all(|col_name| {
                table
                    .columns
                    .iter()
                    .find(|c| &c.name == col_name)
                    .is_some_and(|c| c.nullable)
            });

            graph.add_edge(
                from,
                to,
                GraphEdge {
                    from: table.stable_id.clone(),
                    to: fk.to_table.clone(),
                    label: fk.name.clone().unwrap_or_else(|| {
                        if fk.from_columns.is_empty() {
                            "fk".to_string()
                        } else {
                            fk.from_columns.join(",")
                        }
                    }),
                    nullable: fk_nullable,
                    from_columns: fk.from_columns.clone(),
                    to_columns: fk.to_columns.clone(),
                    kind: EdgeKind::ForeignKey,
                },
            );
        }
        Ok(())
    }

    fn add_enum_ref_edges(
        graph: &mut DiGraph<GraphNode, GraphEdge>,
        from: NodeIndex,
        table: &Table,
        enum_index: &EnumIndex,
    ) -> Result<(), GraphBuildError> {
        for column in &table.columns {
            if let Some(enum_idx) = enum_index.resolve(table, &column.data_type)? {
                graph.add_edge(
                    from,
                    enum_idx,
                    GraphEdge {
                        from: table.stable_id.clone(),
                        to: column.data_type.clone(),
                        label: format!("{} ({})", column.name, column.data_type),
                        nullable: column.nullable,
                        from_columns: vec![column.name.clone()],
                        to_columns: vec![],
                        kind: EdgeKind::EnumReference,
                    },
                );
            }
        }
        Ok(())
    }

    fn add_view_nodes(
        graph: &mut DiGraph<GraphNode, GraphEdge>,
        ids: &BTreeMap<TableId, NodeIndex>,
        schema: &Schema,
    ) {
        let table_names: HashSet<String> = schema
            .tables
            .iter()
            .flat_map(|t| vec![t.name.to_lowercase(), t.stable_id.to_lowercase()])
            .collect();

        for view in &schema.views {
            let view_idx = graph.add_node(GraphNode {
                id: view.id.clone(),
                label: view.qualified_name(),
                columns: view
                    .columns
                    .iter()
                    .map(|column| {
                        let nullable = if column.nullable { "?" } else { "" };
                        format!("{}: {}{}", column.name, column.data_type, nullable)
                    })
                    .collect(),
                kind: NodeKind::View,
            });

            if let Some(ref definition) = view.definition {
                let def_lower = definition.to_lowercase();
                for table in &schema.tables {
                    let tname = table.name.to_lowercase();
                    if table_names.contains(&tname)
                        && def_lower.contains(&tname)
                        && let Some(&table_idx) = ids.get(&table.id)
                    {
                        graph.add_edge(
                            table_idx,
                            view_idx,
                            GraphEdge {
                                from: table.stable_id.clone(),
                                to: view.id.clone(),
                                label: "view dep".to_string(),
                                nullable: false,
                                from_columns: vec![],
                                to_columns: vec![],
                                kind: EdgeKind::ViewDependency,
                            },
                        );
                    }
                }
            }
        }
    }
}

#[derive(Debug, Default)]
struct EnumIndex {
    exact: HashMap<String, NodeIndex>,
    by_name: HashMap<String, Vec<(Option<String>, NodeIndex)>>,
}

impl EnumIndex {
    fn insert(&mut self, enum_type: &Enum, node_index: NodeIndex) {
        let name = enum_type.name.to_lowercase();
        let schema_name = enum_type
            .schema_name
            .as_ref()
            .map(|schema| schema.to_lowercase());
        self.by_name
            .entry(name.clone())
            .or_default()
            .push((schema_name.clone(), node_index));

        if let Some(schema_name) = schema_name {
            self.exact
                .insert(format!("{schema_name}.{name}"), node_index);
        }
    }

    fn resolve(
        &self,
        table: &Table,
        data_type: &str,
    ) -> Result<Option<NodeIndex>, GraphBuildError> {
        let data_type = data_type.to_lowercase();

        if data_type.contains('.') {
            return Ok(self.exact.get(&data_type).copied());
        }

        let Some(candidates) = self.by_name.get(&data_type) else {
            return Ok(None);
        };

        if candidates.len() == 1 {
            return Ok(Some(candidates[0].1));
        }

        if let Some(table_schema) = table.schema_name.as_deref().map(str::to_lowercase) {
            let matching_candidates: Vec<&(Option<String>, NodeIndex)> = candidates
                .iter()
                .filter(|(schema_name, _)| schema_name.as_deref() == Some(table_schema.as_str()))
                .collect();
            if let [(_, node_index)] = matching_candidates.as_slice() {
                return Ok(Some(*node_index));
            }
        }

        Err(GraphBuildError::AmbiguousEnumReference {
            table: table.qualified_name(),
            data_type,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::model::{Column, Enum, ForeignKey, ReferentialAction, Schema, View};
    use petgraph::visit::EdgeRef;

    fn make_column(name: &str, data_type: &str, nullable: bool, is_primary_key: bool) -> Column {
        Column {
            id: crate::model::ColumnId(0),
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable,
            is_primary_key,
            comment: None,
        }
    }

    fn make_foreign_key(to_table: &str, from_columns: &[&str], to_columns: &[&str]) -> ForeignKey {
        ForeignKey {
            name: None,
            from_columns: from_columns
                .iter()
                .map(|column| (*column).to_string())
                .collect(),
            to_schema: None,
            to_table: to_table.to_string(),
            to_columns: to_columns
                .iter()
                .map(|column| (*column).to_string())
                .collect(),
            on_delete: ReferentialAction::NoAction,
            on_update: ReferentialAction::NoAction,
        }
    }

    fn make_table(
        id: u64,
        name: &str,
        schema_name: Option<&str>,
        columns: Vec<Column>,
        foreign_keys: Vec<ForeignKey>,
    ) -> crate::model::Table {
        let qualified =
            schema_name.map_or_else(|| name.to_string(), |schema| format!("{schema}.{name}"));

        crate::model::Table {
            id: crate::model::TableId(id),
            stable_id: qualified,
            schema_name: schema_name.map(str::to_string),
            name: name.to_string(),
            columns,
            foreign_keys,
            indexes: Vec::new(),
            comment: None,
        }
    }

    fn make_enum(name: &str, schema_name: Option<&str>, values: &[&str]) -> Enum {
        let qualified =
            schema_name.map_or_else(|| name.to_string(), |schema| format!("{schema}.{name}"));

        Enum {
            id: qualified,
            schema_name: schema_name.map(str::to_string),
            name: name.to_string(),
            values: values.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    fn make_view(name: &str, schema_name: Option<&str>, definition: &str) -> View {
        let qualified =
            schema_name.map_or_else(|| name.to_string(), |schema| format!("{schema}.{name}"));

        View {
            id: qualified,
            schema_name: schema_name.map(str::to_string),
            name: name.to_string(),
            columns: vec![make_column("id", "integer", false, true)],
            definition: Some(definition.to_string()),
        }
    }

    #[test]
    fn from_schema_builds_fk_enum_and_view_edges() {
        let schema = Schema {
            tables: vec![
                make_table(
                    1,
                    "users",
                    None,
                    vec![
                        make_column("id", "integer", false, true),
                        make_column("status", "status", false, false),
                    ],
                    vec![],
                ),
                make_table(
                    2,
                    "posts",
                    None,
                    vec![
                        make_column("id", "integer", false, true),
                        make_column("user_id", "integer", false, false),
                    ],
                    vec![make_foreign_key("users", &["user_id"], &["id"])],
                ),
            ],
            views: vec![make_view("active_users", None, "select id from users")],
            enums: vec![make_enum("status", None, &["active", "inactive"])],
        };

        let graph = SchemaGraph::from_schema(&schema).expect("schema graph should build");

        assert_eq!(graph.graph.node_count(), 4);
        assert_eq!(graph.graph.edge_count(), 3);

        let users = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "users")
            .expect("users node");
        let posts = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "posts")
            .expect("posts node");
        let active_users = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "active_users")
            .expect("view node");
        let status = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "status")
            .expect("enum node");

        let edges: Vec<_> = graph
            .graph
            .edge_references()
            .map(|edge| (edge.source(), edge.target(), edge.weight().clone()))
            .collect();

        assert!(edges.iter().any(|(source, target, edge)| {
            *source == posts
                && *target == users
                && edge.kind == EdgeKind::ForeignKey
                && edge.from_columns == vec!["user_id".to_string()]
                && edge.to_columns == vec!["id".to_string()]
                && edge.label == "user_id"
        }));

        assert!(edges.iter().any(|(source, target, edge)| {
            *source == users
                && *target == status
                && edge.kind == EdgeKind::EnumReference
                && edge.from_columns == vec!["status".to_string()]
                && edge.to_columns.is_empty()
                && edge.label == "status (status)"
        }));

        assert!(edges.iter().any(|(source, target, edge)| {
            *source == users
                && *target == active_users
                && edge.kind == EdgeKind::ViewDependency
                && edge.label == "view dep"
        }));
    }

    #[test]
    fn from_schema_resolves_same_named_enums_by_schema() {
        let schema = Schema {
            tables: vec![
                make_table(
                    1,
                    "accounts",
                    Some("public"),
                    vec![
                        make_column("id", "integer", false, true),
                        make_column("status", "status", false, false),
                    ],
                    vec![],
                ),
                make_table(
                    2,
                    "sessions",
                    Some("auth"),
                    vec![
                        make_column("id", "integer", false, true),
                        make_column("status", "status", false, false),
                    ],
                    vec![],
                ),
            ],
            views: vec![],
            enums: vec![
                make_enum("status", Some("public"), &["active", "inactive"]),
                make_enum("status", Some("auth"), &["open", "closed"]),
            ],
        };

        let graph = SchemaGraph::from_schema(&schema).expect("schema graph should build");

        let public_accounts = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "public.accounts")
            .expect("public.accounts node");
        let auth_sessions = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "auth.sessions")
            .expect("auth.sessions node");
        let public_status = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "public.status")
            .expect("public.status node");
        let auth_status = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "auth.status")
            .expect("auth.status node");

        let edges: Vec<_> = graph.graph.edge_references().collect();

        assert!(edges.iter().any(|edge| {
            edge.source() == public_accounts
                && edge.target() == public_status
                && edge.weight().kind == EdgeKind::EnumReference
        }));

        assert!(edges.iter().any(|edge| {
            edge.source() == auth_sessions
                && edge.target() == auth_status
                && edge.weight().kind == EdgeKind::EnumReference
        }));

        assert_eq!(
            edges
                .iter()
                .filter(|edge| edge.weight().kind == EdgeKind::EnumReference)
                .count(),
            2
        );
    }

    #[test]
    fn from_schema_errors_on_ambiguous_unqualified_enum_reference() {
        let schema = Schema {
            tables: vec![make_table(
                1,
                "accounts",
                None,
                vec![
                    make_column("id", "integer", false, true),
                    make_column("status", "status", false, false),
                ],
                vec![],
            )],
            views: vec![],
            enums: vec![
                make_enum("status", Some("public"), &["active", "inactive"]),
                make_enum("status", Some("auth"), &["open", "closed"]),
            ],
        };

        let err = SchemaGraph::from_schema(&schema).expect_err("ambiguous enum should fail");
        assert!(matches!(
            err,
            GraphBuildError::AmbiguousEnumReference { ref data_type, .. } if data_type == "status"
        ));
    }
}
