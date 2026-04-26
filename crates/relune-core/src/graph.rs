//! Graph representation of a database schema.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::ControlFlow;

use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use sqlparser::ast::{ObjectName, ObjectNamePart, Query, Visit, Visitor};
use sqlparser::dialect::{Dialect, GenericDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;
use thiserror::Error;
use tracing::debug;

use crate::model::{
    Enum, ForeignKeyTargetResolution, Schema, Table, TableId, View, normalize_identifier,
    resolve_table_reference,
};

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
    /// A foreign key reference is ambiguous across schemas.
    #[error("ambiguous foreign key reference for table '{table}': '{reference}'")]
    AmbiguousTableReference {
        /// Table where the ambiguity was encountered.
        table: String,
        /// The ambiguous table reference.
        reference: String,
    },
}

/// A normalized relation reference extracted from SQL.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SqlRelation {
    /// Referenced schema name, if qualified in SQL.
    pub schema_name: Option<String>,
    /// Referenced relation name.
    pub name: String,
}

impl SqlRelation {
    /// Returns `true` if this SQL relation reference resolves to `table`.
    ///
    /// Schema-qualified references require the schema to match; bare
    /// references match either the table name or its stable id.
    #[must_use]
    pub fn matches_table(&self, table: &Table) -> bool {
        let table_name = table.name.to_lowercase();
        let stable_id = table.stable_id.to_lowercase();

        match self.schema_name.as_deref() {
            Some(reference_schema) => table.schema_name.as_deref().is_some_and(|table_schema| {
                table_schema.to_lowercase() == reference_schema && table_name == self.name
            }),
            None => self.name == table_name || self.name == stable_id,
        }
    }

    /// Returns `true` if this SQL relation reference resolves to `view`.
    ///
    /// Schema-qualified references require the schema to match; bare
    /// references match the view name, id, or qualified name.
    #[must_use]
    pub fn matches_view(&self, view: &View) -> bool {
        let view_name = view.name.to_lowercase();
        let view_id = view.id.to_lowercase();
        let view_label = view.qualified_name().to_lowercase();

        match self.schema_name.as_deref() {
            Some(reference_schema) => view.schema_name.as_deref().is_some_and(|view_schema| {
                view_schema.to_lowercase() == reference_schema && view_name == self.name
            }),
            None => self.name == view_name || self.name == view_id || self.name == view_label,
        }
    }
}

#[derive(Debug, Default)]
struct RelationCollector {
    cte_scopes: Vec<HashSet<String>>,
    references: HashSet<SqlRelation>,
}

impl Visitor for RelationCollector {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        let cte_names = query.with.as_ref().map_or_else(HashSet::new, |with| {
            with.cte_tables
                .iter()
                .map(|cte| normalize_identifier(&cte.alias.name.value))
                .collect()
        });
        self.cte_scopes.push(cte_names);
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<Self::Break> {
        let _ = self.cte_scopes.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<Self::Break> {
        if let Some(reference) = object_name_to_relation(relation)
            && !self.is_cte_reference(&reference)
        {
            self.references.insert(reference);
        }
        ControlFlow::Continue(())
    }
}

impl RelationCollector {
    fn is_cte_reference(&self, reference: &SqlRelation) -> bool {
        reference.schema_name.is_none()
            && self
                .cte_scopes
                .iter()
                .rev()
                .any(|scope| scope.contains(&reference.name))
    }
}

fn object_name_to_relation(name: &ObjectName) -> Option<SqlRelation> {
    let parts: Vec<String> = name
        .0
        .iter()
        .filter_map(object_name_identifier)
        .map(normalize_identifier)
        .collect();
    match parts.as_slice() {
        [name] => Some(SqlRelation {
            schema_name: None,
            name: name.clone(),
        }),
        [.., schema_name, name] => Some(SqlRelation {
            schema_name: Some(schema_name.clone()),
            name: name.clone(),
        }),
        [] => None,
    }
}

const fn object_name_identifier(part: &ObjectNamePart) -> Option<&str> {
    match part {
        ObjectNamePart::Identifier(ident) => Some(ident.value.as_str()),
        ObjectNamePart::Function(_) => None,
    }
}

/// Collects normalized table/view references from a SQL fragment.
///
/// The result excludes CTE aliases so callers can reason about actual
/// relation dependencies without comment or alias false positives.
#[must_use]
pub fn collect_sql_relations(definition: &str) -> HashSet<SqlRelation> {
    let generic = GenericDialect {};
    let postgres = PostgreSqlDialect {};
    let mysql = MySqlDialect {};
    let sqlite = SQLiteDialect {};
    let dialects: [&dyn Dialect; 4] = [&generic, &postgres, &mysql, &sqlite];

    for dialect in dialects {
        let Ok(statements) = Parser::parse_sql(dialect, definition) else {
            continue;
        };
        let mut collector = RelationCollector::default();
        let _ = statements.visit(&mut collector);
        return collector.references;
    }

    debug!(
        "collect_sql_relations: no dialect could parse the definition; view dependencies may be missing"
    );
    HashSet::new()
}

impl SchemaGraph {
    /// Builds a schema graph from a Schema.
    pub fn from_schema(schema: &Schema) -> Result<Self, GraphBuildError> {
        let mut graph = DiGraph::<GraphNode, GraphEdge>::new();
        let mut ids = BTreeMap::new();

        let enum_index = Self::add_enum_nodes(&mut graph, schema);
        Self::add_table_nodes(&mut graph, &mut ids, schema);
        Self::add_table_edges(&mut graph, &ids, &enum_index, schema)?;
        Self::add_view_nodes(&mut graph, &ids, schema);

        Ok(Self { graph })
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
        enum_index: &EnumIndex,
        schema: &Schema,
    ) -> Result<(), GraphBuildError> {
        for table in &schema.tables {
            let from = *ids
                .get(&table.id)
                .ok_or_else(|| GraphBuildError::UnknownTable(table.stable_id.clone()))?;

            Self::add_fk_edges(graph, from, table, ids, schema)?;
            Self::add_enum_ref_edges(graph, from, table, enum_index)?;
        }
        Ok(())
    }

    fn add_fk_edges(
        graph: &mut DiGraph<GraphNode, GraphEdge>,
        from: NodeIndex,
        table: &Table,
        ids: &BTreeMap<TableId, NodeIndex>,
        schema: &Schema,
    ) -> Result<(), GraphBuildError> {
        for fk in &table.foreign_keys {
            let target_table = match resolve_table_reference(
                schema,
                Some(table),
                fk.to_schema.as_deref(),
                &fk.to_table,
            ) {
                ForeignKeyTargetResolution::Found(target_table) => target_table,
                ForeignKeyTargetResolution::Missing => {
                    return Err(GraphBuildError::UnknownTable(fk.to_table.clone()));
                }
                ForeignKeyTargetResolution::Ambiguous => {
                    return Err(GraphBuildError::AmbiguousTableReference {
                        table: table.qualified_name(),
                        reference: fk.to_table.clone(),
                    });
                }
            };
            let to = ids
                .get(&target_table.id)
                .copied()
                .ok_or_else(|| GraphBuildError::UnknownTable(fk.to_table.clone()))?;

            let fk_nullable = fk.from_columns.iter().any(|col_name| {
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
                let references = collect_sql_relations(definition);
                for table in &schema.tables {
                    if table_names.contains(&table.name.to_lowercase())
                        && references
                            .iter()
                            .any(|reference| reference.matches_table(table))
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
            primary_key_name: None,
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
    fn foreign_key_edges_are_nullable_when_any_source_column_is_nullable() {
        let schema = Schema {
            tables: vec![
                make_table(
                    1,
                    "users",
                    None,
                    vec![make_column("id", "integer", false, true)],
                    vec![],
                ),
                make_table(
                    2,
                    "sessions",
                    None,
                    vec![
                        make_column("id", "integer", false, true),
                        make_column("tenant_id", "integer", true, false),
                        make_column("user_id", "integer", false, false),
                    ],
                    vec![make_foreign_key(
                        "users",
                        &["tenant_id", "user_id"],
                        &["id", "id"],
                    )],
                ),
            ],
            views: vec![],
            enums: vec![],
        };

        let graph = SchemaGraph::from_schema(&schema).expect("schema graph should build");
        let edge = graph
            .graph
            .edge_references()
            .find(|edge| edge.weight().kind == EdgeKind::ForeignKey)
            .expect("foreign key edge");

        assert!(edge.weight().nullable);
    }

    #[test]
    fn from_schema_prefers_same_schema_target_for_unqualified_fk() {
        let schema = Schema {
            tables: vec![
                make_table(
                    1,
                    "users",
                    Some("public"),
                    vec![make_column("id", "integer", false, true)],
                    vec![],
                ),
                make_table(
                    2,
                    "users",
                    Some("audit"),
                    vec![make_column("id", "integer", false, true)],
                    vec![],
                ),
                make_table(
                    3,
                    "sessions",
                    Some("audit"),
                    vec![
                        make_column("id", "integer", false, true),
                        make_column("user_id", "integer", false, false),
                    ],
                    vec![make_foreign_key("users", &["user_id"], &["id"])],
                ),
            ],
            views: vec![],
            enums: vec![],
        };

        let graph = SchemaGraph::from_schema(&schema).expect("schema graph should build");
        let edge = graph
            .graph
            .edge_references()
            .find(|edge| edge.weight().kind == EdgeKind::ForeignKey)
            .expect("foreign key edge");

        assert_eq!(graph.graph[edge.target()].label, "audit.users");
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

    #[test]
    fn view_dependency_does_not_link_prefix_table_name_in_longer_identifier() {
        let schema = Schema {
            tables: vec![
                make_table(
                    1,
                    "user",
                    None,
                    vec![make_column("id", "integer", false, true)],
                    vec![],
                ),
                make_table(
                    2,
                    "users",
                    None,
                    vec![make_column("id", "integer", false, true)],
                    vec![],
                ),
            ],
            views: vec![make_view("active_users", None, "select * from users")],
            enums: vec![],
        };

        let graph = SchemaGraph::from_schema(&schema).expect("schema graph should build");
        let user = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "user")
            .expect("user node");
        let users = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "users")
            .expect("users node");
        let view = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "active_users")
            .expect("view node");

        let deps: Vec<_> = graph
            .graph
            .edge_references()
            .filter(|edge| edge.target() == view && edge.weight().kind == EdgeKind::ViewDependency)
            .map(|edge| edge.source())
            .collect();

        assert_eq!(deps, vec![users]);
        assert!(!deps.contains(&user));
    }

    #[test]
    fn view_dependency_ignores_cte_names_that_shadow_tables() {
        let schema = Schema {
            tables: vec![make_table(
                1,
                "users",
                None,
                vec![make_column("id", "integer", false, true)],
                vec![],
            )],
            views: vec![make_view(
                "active_users",
                None,
                "with users as (select 1 as id) select * from users",
            )],
            enums: vec![],
        };

        let graph = SchemaGraph::from_schema(&schema).expect("schema graph should build");
        let view = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "active_users")
            .expect("view node");

        assert!(!graph.graph.edge_references().any(|edge| {
            edge.target() == view && edge.weight().kind == EdgeKind::ViewDependency
        }));
    }

    #[test]
    fn view_dependency_ignores_comment_text() {
        let schema = Schema {
            tables: vec![make_table(
                1,
                "users",
                None,
                vec![make_column("id", "integer", false, true)],
                vec![],
            )],
            views: vec![make_view(
                "active_users",
                None,
                "select 1 /* users */ as id",
            )],
            enums: vec![],
        };

        let graph = SchemaGraph::from_schema(&schema).expect("schema graph should build");
        let view = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "active_users")
            .expect("view node");

        assert!(!graph.graph.edge_references().any(|edge| {
            edge.target() == view && edge.weight().kind == EdgeKind::ViewDependency
        }));
    }

    #[test]
    fn view_dependency_resolves_schema_qualified_relations() {
        let schema = Schema {
            tables: vec![
                make_table(
                    1,
                    "users",
                    Some("public"),
                    vec![make_column("id", "integer", false, true)],
                    vec![],
                ),
                make_table(
                    2,
                    "users",
                    Some("analytics"),
                    vec![make_column("id", "integer", false, true)],
                    vec![],
                ),
            ],
            views: vec![make_view(
                "active_users",
                None,
                "select * from public.users",
            )],
            enums: vec![],
        };

        let graph = SchemaGraph::from_schema(&schema).expect("schema graph should build");
        let public_users = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "public.users")
            .expect("public.users node");
        let analytics_users = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "analytics.users")
            .expect("analytics.users node");
        let view = graph
            .graph
            .node_indices()
            .find(|&idx| graph.graph[idx].label == "active_users")
            .expect("view node");

        let deps: Vec<_> = graph
            .graph
            .edge_references()
            .filter(|edge| edge.target() == view && edge.weight().kind == EdgeKind::ViewDependency)
            .map(|edge| edge.source())
            .collect();

        assert_eq!(deps, vec![public_users]);
        assert!(!deps.contains(&analytics_users));
    }
}
