//! Picks the columns each card lists at a [`CardDensity`].

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use relune_core::{CardDensity, normalize_identifier};

use super::OmittedColumns;
use crate::graph::{LayoutColumn, LayoutGraph};

/// A graph whose nodes keep only the columns their cards list.
pub(super) struct DensityView<'a> {
    /// The graph with unlisted columns removed.
    pub(super) graph: Cow<'a, LayoutGraph>,
    /// Columns left out of each node's card, by node index.
    pub(super) omitted: Vec<Option<OmittedColumns>>,
}

/// Removes the columns a card does not list at `density`.
///
/// `Keys` lists primary key and foreign key columns and every column a
/// relationship names at that end, so each line still meets a listed row.
pub(super) fn apply_density(graph: &LayoutGraph, density: CardDensity) -> DensityView<'_> {
    match density {
        CardDensity::Full => DensityView {
            graph: Cow::Borrowed(graph),
            omitted: vec![None; graph.nodes.len()],
        },
        CardDensity::Overview => filter_columns(graph, false, |_, _| false),
        CardDensity::Keys => {
            // Relationships may name columns in a different case than the
            // table declares them, so names are compared normalized.
            let mut referenced: HashMap<&str, HashSet<String>> = HashMap::new();
            for edge in &graph.edges {
                for (node_id, columns) in [
                    (&edge.from, &edge.from_columns),
                    (&edge.to, &edge.to_columns),
                ] {
                    referenced
                        .entry(node_id.as_str())
                        .or_default()
                        .extend(columns.iter().map(|name| normalize_identifier(name)));
                }
            }
            // A keys card that lists nothing (a view without keys) still
            // counts its columns, so it never reads as an overview card.
            filter_columns(graph, true, |node_id, column| {
                column.is_primary_key
                    || column.is_foreign_key
                    || referenced
                        .get(node_id)
                        .is_some_and(|names| names.contains(&normalize_identifier(&column.name)))
            })
        }
    }
}

fn filter_columns(
    graph: &LayoutGraph,
    counted_row: bool,
    keep: impl Fn(&str, &LayoutColumn) -> bool,
) -> DensityView<'_> {
    let mut filtered = graph.clone();
    let omitted = filtered
        .nodes
        .iter_mut()
        .map(|node| {
            let node_id = node.id.as_str();
            let (listed, left_out): (Vec<_>, Vec<_>) = std::mem::take(&mut node.columns)
                .into_iter()
                .partition(|column| keep(node_id, column));
            node.columns = listed;
            (!left_out.is_empty()).then(|| OmittedColumns {
                count: left_out.len(),
                primary_keys: left_out
                    .iter()
                    .filter(|column| column.is_primary_key)
                    .count(),
                counted_row,
            })
        })
        .collect();
    DensityView {
        graph: Cow::Owned(filtered),
        omitted,
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
        assert_eq!(view.omitted, vec![None, None]);
    }

    #[test]
    fn keys_density_lists_keys_and_referenced_columns() {
        let graph = LayoutGraphBuilder::new().build(&schema());
        let view = apply_density(&graph, CardDensity::Keys);

        assert_eq!(listed(&view, "users"), ["id", "email"]);
        assert_eq!(listed(&view, "posts"), ["id", "author_email"]);
        let counted = Some(OmittedColumns {
            count: 1,
            primary_keys: 0,
            counted_row: true,
        });
        assert_eq!(view.omitted, vec![counted, counted]);
    }

    #[test]
    fn keys_density_matches_relationship_columns_regardless_of_case() {
        let mut schema = schema();
        schema.tables[0].columns[1].name = "Email".to_string();
        schema.tables[1].columns[1].name = "Author_Email".to_string();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let view = apply_density(&graph, CardDensity::Keys);

        assert_eq!(listed(&view, "users"), ["id", "Email"]);
        assert_eq!(listed(&view, "posts"), ["id", "Author_Email"]);
    }

    #[test]
    fn keys_density_counts_the_columns_of_a_card_that_lists_none() {
        let mut schema = schema();
        schema.views.push(relune_core::View {
            id: "recent_posts".to_string(),
            schema_name: None,
            name: "recent_posts".to_string(),
            columns: vec![column(7, "title", false), column(8, "published_at", false)],
            definition: None,
        });
        let graph = LayoutGraphBuilder::new().build(&schema);
        let view = apply_density(&graph, CardDensity::Keys);

        assert!(listed(&view, "recent_posts").is_empty());
        let index = view
            .graph
            .nodes
            .iter()
            .position(|node| node.id == "recent_posts")
            .expect("view node");
        assert_eq!(
            view.omitted[index],
            Some(OmittedColumns {
                count: 2,
                primary_keys: 0,
                counted_row: true,
            })
        );
    }

    #[test]
    fn overview_density_lists_no_columns() {
        let graph = LayoutGraphBuilder::new().build(&schema());
        let view = apply_density(&graph, CardDensity::Overview);

        assert!(view.graph.nodes.iter().all(|node| node.columns.is_empty()));
        let header_only = Some(OmittedColumns {
            count: 3,
            primary_keys: 1,
            counted_row: false,
        });
        assert_eq!(view.omitted, vec![header_only, header_only]);
        assert_eq!(graph.nodes[0].columns.len(), 3, "input graph is untouched");
    }
}
