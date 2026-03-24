//! Text diagram formats for review tooling (Mermaid ER, D2, Graphviz DOT).

use std::borrow::Cow;
use std::fmt::Write;

use crate::graph::{LayoutEdge, LayoutGraph};

fn node_label_by_id<'a>(graph: &'a LayoutGraph, table_id: &str) -> Cow<'a, str> {
    match graph
        .nodes
        .iter()
        .find(|n| n.id == table_id || n.table_name == table_id || n.label == table_id)
    {
        Some(n) => Cow::Borrowed(n.label.as_str()),
        None => Cow::Owned(table_id.to_string()),
    }
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

fn mermaid_quote_entity(label: &str) -> String {
    if label.chars().all(|c| c.is_alphanumeric() || c == '_') {
        label.to_string()
    } else {
        format!("\"{}\"", label.replace('"', "'"))
    }
}

fn mermaid_escape_edge_label(label: &str) -> String {
    label.replace('"', "'")
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
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Renders a [`LayoutGraph`] as a Mermaid `erDiagram` document (no markdown fence).
#[must_use]
pub fn layout_graph_to_mermaid(graph: &LayoutGraph) -> String {
    let mut out = String::from("erDiagram\n");

    for node in &graph.nodes {
        let name = mermaid_quote_entity(node.label.as_str());
        let _ = writeln!(&mut out, "  {name} {{");
        for col in &node.columns {
            let t = mermaid_token_type(&col.data_type);
            let _ = write!(&mut out, "    {t} {}", col.name);
            if col.is_primary_key {
                out.push_str(" PK");
            }
            out.push('\n');
        }
        let _ = writeln!(&mut out, "  }}");
    }

    for edge in &graph.edges {
        let parent = node_label_by_id(graph, &edge.to);
        let child = node_label_by_id(graph, &edge.from);
        let p = mermaid_quote_entity(parent.as_ref());
        let c = mermaid_quote_entity(child.as_ref());
        let label = mermaid_escape_edge_label(&format_edge_label(edge));
        // One referenced row, zero or more referencing rows (`||--o{`).
        let _ = writeln!(&mut out, "  {p} ||--o{{ {c} : \"{label}\"");
    }

    out
}

/// Renders a [`LayoutGraph`] as D2 source (connections and optional table labels).
#[must_use]
pub fn layout_graph_to_d2(graph: &LayoutGraph) -> String {
    let mut out = String::new();
    for node in &graph.nodes {
        let id = d2_escape(&node.label);
        let lbl = d2_escape(&node.label);
        let _ = writeln!(&mut out, "\"{id}\": {{ label: \"{lbl}\" }}");
    }
    for edge in &graph.edges {
        let from = d2_escape(node_label_by_id(graph, &edge.from).as_ref());
        let to = d2_escape(node_label_by_id(graph, &edge.to).as_ref());
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

    for node in &graph.nodes {
        let id = dot_escape(&node.id);
        let label = dot_escape(&node.label);
        let _ = writeln!(&mut out, "  \"{id}\" [label=\"{label}\"];");
    }
    for edge in &graph.edges {
        let from = dot_escape(&edge.from);
        let to = dot_escape(&edge.to);
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
                    },
                    LayoutColumn {
                        name: "user_id".into(),
                        data_type: "INT".into(),
                        nullable: true,
                        is_primary_key: false,
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
        assert!(s.contains("users {"));
        assert!(s.contains("posts {"));
        assert!(s.contains("||--o{"));
        assert!(s.contains("posts_user_fk"));
    }

    #[test]
    fn d2_contains_arrow() {
        let s = layout_graph_to_d2(&tiny_graph());
        assert!(s.contains("\"posts\" -> \"users\""));
    }

    #[test]
    fn dot_is_digraph() {
        let s = layout_graph_to_dot(&tiny_graph());
        assert!(s.starts_with("digraph erd"));
        assert!(s.contains("\"p1\" -> \"u1\""));
    }
}
