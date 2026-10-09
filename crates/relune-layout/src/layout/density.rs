//! Picks the columns each card lists at a [`CardDensity`].

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use relune_core::CardDensity;

use crate::graph::{LayoutColumn, LayoutGraph};

/// A graph whose nodes keep only the columns their cards list.
pub(super) struct DensityView<'a> {
    /// The graph with unlisted columns removed.
    pub(super) graph: Cow<'a, LayoutGraph>,
    /// Number of columns left out of each node's card, by node index.
    pub(super) omitted_columns: Vec<usize>,
}

/// Removes the columns a card does not list at `density`.
///
/// `Keys` lists primary key and foreign key columns and every column a
/// relationship names at that end, so each line still meets a listed row.
pub(super) fn apply_density(graph: &LayoutGraph, density: CardDensity) -> DensityView<'_> {
    match density {
        CardDensity::Full => DensityView {
            graph: Cow::Borrowed(graph),
            omitted_columns: vec![0; graph.nodes.len()],
        },
        CardDensity::Overview => filter_columns(graph, |_, _| false),
        CardDensity::Keys => {
            let mut referenced: HashMap<&str, HashSet<&str>> = HashMap::new();
            for edge in &graph.edges {
                for (node_id, columns) in [
                    (&edge.from, &edge.from_columns),
                    (&edge.to, &edge.to_columns),
                ] {
                    referenced
                        .entry(node_id.as_str())
                        .or_default()
                        .extend(columns.iter().map(String::as_str));
                }
            }
            filter_columns(graph, |node_id, column| {
                column.is_primary_key
                    || column.is_foreign_key
                    || referenced
                        .get(node_id)
                        .is_some_and(|names| names.contains(column.name.as_str()))
            })
        }
    }
}

fn filter_columns(
    graph: &LayoutGraph,
    keep: impl Fn(&str, &LayoutColumn) -> bool,
) -> DensityView<'_> {
    let mut filtered = graph.clone();
    let omitted_columns = filtered
        .nodes
        .iter_mut()
        .map(|node| {
            let total = node.columns.len();
            let node_id = node.id.as_str();
            node.columns.retain(|column| keep(node_id, column));
            total - node.columns.len()
        })
        .collect();
    DensityView {
        graph: Cow::Owned(filtered),
        omitted_columns,
    }
}

#[cfg(test)]
mod tests {
    use relune_core::{Column, ColumnId, ForeignKey, Schema, Table, TableId};

    use super::*;
    use crate::graph::LayoutGraphBuilder;

    fn column(id: u64, name: &str, primary_key: bool) -> Column {
        Column {
            id: ColumnId(id),
            name: name.to_string(),
            data_type: "int".to_string(),
            nullable: false,
            is_primary_key: primary_key,
            comment: None,
            enum_values: None,
            semantics: relune_core::ColumnSemantics::default(),
        }
    }

    fn table(id: u64, name: &str, columns: Vec<Column>, foreign_keys: Vec<ForeignKey>) -> Table {
        Table {
            id: TableId(id),
            stable_id: name.to_string(),
            schema_name: None,
            name: name.to_string(),
            columns,
            foreign_keys,
            indexes: vec![],
            primary_key_name: None,
            check_constraints: Vec::new(),
            comment: None,
        }
    }

    fn schema() -> Schema {
        Schema {
            tables: vec![
                table(
                    1,
                    "users",
                    vec![
                        column(1, "id", true),
                        column(2, "email", false),
                        column(3, "name", false),
                    ],
                    vec![],
                ),
                table(
                    2,
                    "posts",
                    vec![
                        column(4, "id", true),
                        column(5, "author_email", false),
                        column(6, "body", false),
                    ],
                    vec![ForeignKey {
                        name: None,
                        from_columns: vec!["author_email".to_string()],
                        to_schema: None,
                        to_table: "users".to_string(),
                        to_columns: vec!["email".to_string()],
                        on_delete: relune_core::ReferentialAction::NoAction,
                        on_update: relune_core::ReferentialAction::NoAction,
                    }],
                ),
            ],
            views: vec![],
            enums: vec![],
        }
    }

    fn listed<'a>(view: &'a DensityView<'_>, node_id: &str) -> Vec<&'a str> {
        view.graph
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .expect("node exists")
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect()
    }

    #[test]
    fn full_density_borrows_the_graph() {
        let graph = LayoutGraphBuilder::new().build(&schema());
        let view = apply_density(&graph, CardDensity::Full);

        assert!(matches!(view.graph, Cow::Borrowed(_)));
        assert_eq!(view.omitted_columns, vec![0, 0]);
    }

    #[test]
    fn keys_density_lists_keys_and_referenced_columns() {
        let graph = LayoutGraphBuilder::new().build(&schema());
        let view = apply_density(&graph, CardDensity::Keys);

        assert_eq!(listed(&view, "users"), ["id", "email"]);
        assert_eq!(listed(&view, "posts"), ["id", "author_email"]);
        assert_eq!(view.omitted_columns, vec![1, 1]);
    }

    #[test]
    fn overview_density_lists_no_columns() {
        let graph = LayoutGraphBuilder::new().build(&schema());
        let view = apply_density(&graph, CardDensity::Overview);

        assert!(view.graph.nodes.iter().all(|node| node.columns.is_empty()));
        assert_eq!(view.omitted_columns, vec![3, 3]);
        assert_eq!(graph.nodes[0].columns.len(), 3, "input graph is untouched");
    }
}
