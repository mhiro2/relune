//! Graph representation of a database schema.

use std::collections::{BTreeMap, HashMap, HashSet};

use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{Schema, Table, TableId};

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

    fn add_enum_nodes(
        graph: &mut DiGraph<GraphNode, GraphEdge>,
        schema: &Schema,
    ) -> HashMap<String, NodeIndex> {
        let mut enum_index = HashMap::new();
        for enum_type in &schema.enums {
            let idx = graph.add_node(GraphNode {
                id: enum_type.id.clone(),
                label: enum_type.qualified_name(),
                columns: enum_type.values.iter().map(|v| format!("• {v}")).collect(),
                kind: NodeKind::Enum,
            });
            enum_index.insert(enum_type.name.to_lowercase(), idx);
            if let Some(ref schema_name) = enum_type.schema_name {
                let qualified = format!(
                    "{}.{}",
                    schema_name.to_lowercase(),
                    enum_type.name.to_lowercase()
                );
                enum_index.insert(qualified, idx);
            }
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
        enum_index: &HashMap<String, NodeIndex>,
        schema: &Schema,
    ) -> Result<(), GraphBuildError> {
        for table in &schema.tables {
            let from = *ids
                .get(&table.id)
                .ok_or_else(|| GraphBuildError::UnknownTable(table.stable_id.clone()))?;

            Self::add_fk_edges(graph, from, table, ids, name_index)?;
            Self::add_enum_ref_edges(graph, from, table, enum_index);
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
        enum_index: &HashMap<String, NodeIndex>,
    ) {
        for column in &table.columns {
            let col_type_lower = column.data_type.to_lowercase();
            if let Some(&enum_idx) = enum_index.get(&col_type_lower) {
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
