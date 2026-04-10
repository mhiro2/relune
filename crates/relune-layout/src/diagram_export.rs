//! Text diagram formats for review tooling (Mermaid ER, D2, Graphviz DOT).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use crate::graph::{LayoutEdge, LayoutGraph};

fn node_index_by_id(graph: &LayoutGraph, table_id: &str) -> Option<usize> {
    graph.node_index.get(table_id).copied().or_else(|| {
        graph.nodes.iter().position(|node| {
            node.id == table_id || node.table_name == table_id || node.label == table_id
        })
    })
}

fn first_non_empty<'a>(values: &[&'a str]) -> Option<&'a str> {
    values
        .iter()
        .copied()
        .find(|value| !value.trim().is_empty())
}

fn node_display_name(node: &crate::graph::LayoutNode, index: usize) -> Cow<'_, str> {
    match first_non_empty(&[
        node.label.as_str(),
        node.table_name.as_str(),
        node.id.as_str(),
    ]) {
        Some(value) => Cow::Borrowed(value),
        None => Cow::Owned(format!("node_{}", index + 1)),
    }
}

fn node_export_id(node: &crate::graph::LayoutNode, index: usize) -> Cow<'_, str> {
    match first_non_empty(&[
        node.id.as_str(),
        node.table_name.as_str(),
        node.label.as_str(),
    ]) {
        Some(value) => Cow::Borrowed(value),
        None => Cow::Owned(format!("node_{}", index + 1)),
    }
}

fn edge_endpoint_name(
    graph: &LayoutGraph,
    raw_id: &str,
    names: &[String],
    fallback_prefix: &str,
) -> String {
    if raw_id.trim().is_empty() {
        return fallback_export_name(&[], fallback_prefix, names.len());
    }
    node_index_by_id(graph, raw_id).map_or_else(
        || fallback_export_name(&[raw_id], fallback_prefix, names.len()),
        |index| names[index].clone(),
    )
}

fn fallback_export_name(values: &[&str], prefix: &str, index: usize) -> String {
    first_non_empty(values).map_or_else(|| format!("{prefix}_{}", index + 1), ToOwned::to_owned)
}

fn mermaid_token_type(data_type: &str) -> String {
    let head = data_type
        .split_whitespace()
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown");
    head.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn mermaid_escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '[' | ']' | '{' | '}' | '(' | ')' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn mermaid_quote_entity(label: &str, fallback_prefix: &str, index: usize) -> String {
    let label = fallback_export_name(&[label], fallback_prefix, index);
    format!("\"{}\"", mermaid_escape_text(&label))
}

fn mermaid_column_name(name: &str, index: usize) -> String {
    let mut sanitized = String::with_capacity(name.len().max(8));
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
        }
    }
    if sanitized.is_empty() {
        sanitized = format!("column_{}", index + 1);
    } else if sanitized
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_digit())
    {
        sanitized.insert(0, '_');
    }
    sanitized
}

fn mermaid_column_names(node: &crate::graph::LayoutNode) -> Vec<String> {
    let mut used = HashSet::new();
    let mut next_suffix = HashMap::<String, usize>::new();

    node.columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let base = mermaid_column_name(&column.name, index);
            let mut candidate = base.clone();

            if used.contains(&candidate) {
                let suffix = next_suffix.entry(base.clone()).or_insert(1);
                loop {
                    *suffix += 1;
                    candidate = format!("{base}_{}", *suffix);
                    if !used.contains(&candidate) {
                        break;
                    }
                }
            }

            used.insert(candidate.clone());
            candidate
        })
        .collect()
}

fn mermaid_escape_edge_label(label: &str) -> String {
    mermaid_escape_text(label)
}

fn format_edge_label(edge: &LayoutEdge) -> String {
    let mut out = String::new();
    if edge.is_collapsed_join
        && let Some(j) = &edge.collapsed_join_table
    {
        out.push_str("via ");
        out.push_str(&j.table_label);
        out.push_str(": ");
    }
    if let Some(name) = edge.name.as_deref() {
        out.push_str(name);
        if !edge.from_columns.is_empty() {
            out.push(' ');
            out.push('(');
            out.push_str(&edge.from_columns.join(", "));
            out.push_str(" -> ");
            out.push_str(&edge.to_columns.join(", "));
            out.push(')');
        }
    } else if edge.from_columns.is_empty() && edge.to_columns.is_empty() {
        out.push_str("fk");
    } else {
        out.push_str(&edge.from_columns.join(", "));
        out.push_str(" → ");
        out.push_str(&edge.to_columns.join(", "));
    }
    out
}

fn dot_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out
}

fn d2_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '<' | '>' | '{' | '}' | '|' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Renders a [`LayoutGraph`] as a Mermaid `erDiagram` document (no markdown fence).
#[must_use]
pub fn layout_graph_to_mermaid(graph: &LayoutGraph) -> String {
    let mut out = String::from("erDiagram\n");
    let node_names: Vec<String> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            mermaid_quote_entity(node_display_name(node, index).as_ref(), "node", index)
        })
        .collect();

    for (index, node) in graph.nodes.iter().enumerate() {
        let name = &node_names[index];
        let column_names = mermaid_column_names(node);
        let _ = writeln!(&mut out, "  {name} {{");
        for (column_index, col) in node.columns.iter().enumerate() {
            let t = mermaid_token_type(&col.data_type);
            let column_name = &column_names[column_index];
            let _ = write!(&mut out, "    {t} {column_name}");
            if col.is_primary_key {
                out.push_str(" PK");
            }
            out.push('\n');
        }
        let _ = writeln!(&mut out, "  }}");
    }

    for edge in &graph.edges {
        let parent = edge_endpoint_name(graph, &edge.to, &node_names, "parent");
        let child = edge_endpoint_name(graph, &edge.from, &node_names, "child");
        let label = mermaid_escape_edge_label(&format_edge_label(edge));
        // One referenced row, zero or more referencing rows (`||--o{`).
        let _ = writeln!(&mut out, "  {parent} ||--o{{ {child} : \"{label}\"");
    }

    out
}

/// Renders a [`LayoutGraph`] as D2 source (connections and optional table labels).
#[must_use]
pub fn layout_graph_to_d2(graph: &LayoutGraph) -> String {
    let mut out = String::new();
    let node_ids: Vec<String> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            fallback_export_name(&[node_export_id(node, index).as_ref()], "node", index)
        })
        .collect();
    let node_labels: Vec<String> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            fallback_export_name(&[node_display_name(node, index).as_ref()], "node", index)
        })
        .collect();
    for index in 0..graph.nodes.len() {
        let id = d2_escape(&node_ids[index]);
        let lbl = d2_escape(&node_labels[index]);
        let _ = writeln!(&mut out, "\"{id}\": {{ label: \"{lbl}\" }}");
    }
    for edge in &graph.edges {
        let from = d2_escape(&edge_endpoint_name(graph, &edge.from, &node_ids, "from"));
        let to = d2_escape(&edge_endpoint_name(graph, &edge.to, &node_ids, "to"));
        let lbl = d2_escape(&format_edge_label(edge));
        let _ = writeln!(&mut out, "\"{from}\" -> \"{to}\": \"{lbl}\"");
    }
    out
}

/// Renders a [`LayoutGraph`] as a Graphviz `digraph` in DOT syntax.
#[must_use]
pub fn layout_graph_to_dot(graph: &LayoutGraph) -> String {
    let mut out = String::from("digraph erd {\n");
    out.push_str("  graph [rankdir=BT];\n");
    out.push_str("  node [shape=box, fontname=\"Helvetica\"];\n");
    out.push_str("  edge [fontname=\"Helvetica\"];\n");
    let node_ids: Vec<String> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            fallback_export_name(&[node_export_id(node, index).as_ref()], "node", index)
        })
        .collect();
    let node_labels: Vec<String> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            fallback_export_name(&[node_display_name(node, index).as_ref()], "node", index)
        })
        .collect();

    for index in 0..graph.nodes.len() {
        let id = dot_escape(&node_ids[index]);
        let label = dot_escape(&node_labels[index]);
        let _ = writeln!(&mut out, "  \"{id}\" [label=\"{label}\"];");
    }
    for edge in &graph.edges {
        let from = dot_escape(&edge_endpoint_name(graph, &edge.from, &node_ids, "from"));
        let to = dot_escape(&edge_endpoint_name(graph, &edge.to, &node_ids, "to"));
        let el = dot_escape(&format_edge_label(edge));
        let _ = writeln!(&mut out, "  \"{from}\" -> \"{to}\" [label=\"{el}\"];");
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{LayoutColumn, LayoutEdge, LayoutNode};
    use relune_core::{EdgeKind, NodeKind};
    use std::collections::BTreeMap;

    fn tiny_graph() -> LayoutGraph {
        let nodes = vec![
            LayoutNode {
                id: "u1".into(),
                label: "users".into(),
                schema_name: None,
                table_name: "users".into(),
                kind: NodeKind::Table,
                columns: vec![LayoutColumn {
                    name: "id".into(),
                    data_type: "INT".into(),
                    nullable: false,
                    is_primary_key: true,
                    is_foreign_key: false,
                    is_indexed: false,
                }],
                inbound_count: 0,
                outbound_count: 1,
                has_self_loop: false,
                is_join_table_candidate: false,
                group_index: None,
            },
            LayoutNode {
                id: "p1".into(),
                label: "posts".into(),
                schema_name: None,
                table_name: "posts".into(),
                kind: NodeKind::Table,
                columns: vec![
                    LayoutColumn {
                        name: "id".into(),
                        data_type: "INT".into(),
                        nullable: false,
                        is_primary_key: true,
                        is_foreign_key: false,
                        is_indexed: false,
                    },
                    LayoutColumn {
                        name: "user_id".into(),
                        data_type: "INT".into(),
                        nullable: true,
                        is_primary_key: false,
                        is_foreign_key: true,
                        is_indexed: true,
                    },
                ],
                inbound_count: 1,
                outbound_count: 0,
                has_self_loop: false,
                is_join_table_candidate: false,
                group_index: None,
            },
        ];
        let edges = vec![LayoutEdge {
            from: "p1".into(),
            to: "u1".into(),
            name: Some("posts_user_fk".into()),
            from_columns: vec!["user_id".into()],
            to_columns: vec!["id".into()],
            kind: EdgeKind::ForeignKey,
            is_self_loop: false,
            nullable: true,
            target_cardinality: relune_core::layout::Cardinality::One,
            is_collapsed_join: false,
            collapsed_join_table: None,
        }];
        let mut node_index = BTreeMap::new();
        let mut reverse_index = BTreeMap::new();
        for (i, n) in nodes.iter().enumerate() {
            node_index.insert(n.id.clone(), i);
            reverse_index.insert(i, n.id.clone());
        }
        LayoutGraph {
            nodes,
            edges,
            groups: vec![],
            node_index,
            reverse_index,
        }
    }

    #[test]
    fn mermaid_contains_er_diagram_and_relationship() {
        let s = layout_graph_to_mermaid(&tiny_graph());
        assert!(s.starts_with("erDiagram\n"));
        assert!(s.contains("\"users\" {"));
        assert!(s.contains("\"posts\" {"));
        assert!(s.contains("||--o{"));
        assert!(s.contains("posts_user_fk"));
    }

    #[test]
    fn d2_contains_arrow() {
        let s = layout_graph_to_d2(&tiny_graph());
        assert!(s.contains("\"p1\" -> \"u1\""));
    }

    #[test]
    fn dot_is_digraph() {
        let s = layout_graph_to_dot(&tiny_graph());
        assert!(s.starts_with("digraph erd"));
        assert!(s.contains("\"p1\" -> \"u1\""));
    }

    #[test]
    fn exports_escape_control_characters_and_brackets() {
        let mut graph = tiny_graph();
        graph.nodes[0].label = "users\n[prod]".into();
        graph.nodes[0].columns[0].name = "user id".into();
        graph.edges[0].name = Some("fk[\n]".into());

        let mermaid = layout_graph_to_mermaid(&graph);
        assert!(mermaid.contains("\"users\\n\\[prod\\]\" {"));
        assert!(mermaid.contains("INT user_id PK"));
        assert!(mermaid.contains(": \"fk\\[\\n\\] \\(user_id -> id\\)\""));

        let d2 = layout_graph_to_d2(&graph);
        assert!(d2.contains("\"users\\n[prod]\""));
        assert!(d2.contains("\"fk[\\n] (user_id -\\> id)\""));

        let dot = layout_graph_to_dot(&graph);
        assert!(dot.contains("[label=\"users\\n[prod]\"]"));
        assert!(dot.contains("[label=\"fk[\\n] (user_id -> id)\"]"));
    }

    #[test]
    fn exports_fallback_names_for_empty_nodes() {
        let mut graph = tiny_graph();
        graph.nodes[0].id.clear();
        graph.nodes[0].label.clear();
        graph.nodes[0].table_name.clear();

        let mermaid = layout_graph_to_mermaid(&graph);
        assert!(mermaid.contains("\"node_1\" {"));

        let d2 = layout_graph_to_d2(&graph);
        assert!(d2.contains("\"node_1\": { label: \"node_1\" }"));

        let dot = layout_graph_to_dot(&graph);
        assert!(dot.contains("\"node_1\" [label=\"node_1\"]"));
    }

    #[test]
    fn d2_escape_handles_special_d2_characters() {
        assert_eq!(d2_escape("<div>"), "\\<div\\>");
        assert_eq!(d2_escape("{block}"), "\\{block\\}");
        assert_eq!(d2_escape("a|b"), "a\\|b");
        assert_eq!(d2_escape("a<b>c{d}e|f"), "a\\<b\\>c\\{d\\}e\\|f");
        assert_eq!(d2_escape("plain"), "plain");
        assert_eq!(d2_escape("line\nnewline"), "line\\nnewline");
    }

    #[test]
    fn d2_export_escapes_special_characters_in_labels() {
        let mut graph = tiny_graph();
        graph.nodes[0].label = "users<admin>".into();
        graph.edges[0].name = Some("fk{main}|ref".into());

        let d2 = layout_graph_to_d2(&graph);
        assert!(d2.contains("\\<admin\\>"));
        assert!(d2.contains("\\{main\\}\\|ref"));
    }

    #[test]
    fn mermaid_columns_remain_unique_after_sanitizing_names() {
        let mut graph = tiny_graph();
        graph.nodes[0].columns = vec![
            LayoutColumn {
                name: "user id".into(),
                data_type: "INT".into(),
                nullable: false,
                is_primary_key: false,
                is_foreign_key: false,
                is_indexed: false,
            },
            LayoutColumn {
                name: "user-id".into(),
                data_type: "INT".into(),
                nullable: false,
                is_primary_key: false,
                is_foreign_key: false,
                is_indexed: false,
            },
            LayoutColumn {
                name: "user_id".into(),
                data_type: "INT".into(),
                nullable: false,
                is_primary_key: false,
                is_foreign_key: false,
                is_indexed: false,
            },
        ];

        let mermaid = layout_graph_to_mermaid(&graph);
        assert!(mermaid.contains("INT user_id\n"));
        assert!(mermaid.contains("INT user_id_2\n"));
        assert!(mermaid.contains("INT user_id_3\n"));
    }
}
