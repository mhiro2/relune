//! HTML renderer for Relune ERD diagrams.
//!
//! This crate provides a self-contained HTML document wrapper around SVG output,
//! with pan/zoom interaction and embedded graph metadata for future features.

mod components;
mod css;
mod error;
mod html;
mod metadata;
mod options;

pub use error::HtmlRenderError;
pub use options::{HtmlRenderOptions, Theme};
pub use relune_render_svg::SvgArtifact;

use relune_layout::LayoutGraph;

/// Render a self-contained HTML document with embedded SVG and metadata.
///
/// # Arguments
///
/// * `graph` - The layout graph containing node/edge/group information
/// * `svg` - Pre-rendered SVG produced by [`relune_render_svg`]
/// * `options` - HTML rendering options
///
/// # Returns
///
/// A complete, self-contained HTML document string.
pub fn render_html(
    graph: &LayoutGraph,
    svg: &SvgArtifact,
    options: &HtmlRenderOptions,
) -> Result<String, HtmlRenderError> {
    render_html_with_overlay(graph, svg, options, None)
}

/// Render a self-contained HTML document with an optional overlay.
///
/// When an overlay is provided, its annotations are embedded in the metadata
/// so that client-side scripts can display lint warnings, diff status, etc.
pub fn render_html_with_overlay(
    graph: &LayoutGraph,
    svg: &SvgArtifact,
    options: &HtmlRenderOptions,
    overlay: Option<&relune_layout::DiagramOverlay>,
) -> Result<String, HtmlRenderError> {
    let metadata = metadata::build_metadata_with_overlay(graph, overlay);
    let metadata_json = serde_json::to_string(&metadata)?;
    let escaped_metadata = html::escape_json_for_script(&metadata_json);

    let html_document = html::build_html_document(svg.as_str(), &escaped_metadata, options);

    Ok(html_document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use relune_core::{EdgeKind, NodeKind};
    use relune_layout::graph::LayoutColumn;
    use relune_layout::{LayoutEdge, LayoutGraph, LayoutNode};

    fn create_test_graph() -> LayoutGraph {
        LayoutGraph {
            nodes: vec![
                LayoutNode {
                    id: "users".to_string(),
                    label: "users".to_string(),
                    schema_name: None,
                    table_name: "users".to_string(),
                    kind: NodeKind::Table,
                    columns: vec![
                        LayoutColumn {
                            name: "id".to_string(),
                            data_type: "integer".to_string(),
                            nullable: false,
                            is_primary_key: true,
                            is_foreign_key: false,
                            is_indexed: false,
                            change: None,
                        },
                        LayoutColumn {
                            name: "name".to_string(),
                            data_type: "varchar".to_string(),
                            nullable: true,
                            is_primary_key: false,
                            is_foreign_key: false,
                            is_indexed: false,
                            change: None,
                        },
                    ],
                    inbound_count: 1,
                    outbound_count: 0,
                    has_self_loop: false,
                    is_join_table_candidate: false,
                    group_index: None,
                },
                LayoutNode {
                    id: "posts".to_string(),
                    label: "posts".to_string(),
                    schema_name: None,
                    table_name: "posts".to_string(),
                    kind: NodeKind::Table,
                    columns: vec![
                        LayoutColumn {
                            name: "id".to_string(),
                            data_type: "integer".to_string(),
                            nullable: false,
                            is_primary_key: true,
                            is_foreign_key: false,
                            is_indexed: false,
                            change: None,
                        },
                        LayoutColumn {
                            name: "user_id".to_string(),
                            data_type: "integer".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            is_foreign_key: true,
                            is_indexed: true,
                            change: None,
                        },
                        LayoutColumn {
                            name: "title".to_string(),
                            data_type: "varchar".to_string(),
                            nullable: true,
                            is_primary_key: false,
                            is_foreign_key: false,
                            is_indexed: false,
                            change: None,
                        },
                    ],
                    inbound_count: 0,
                    outbound_count: 1,
                    has_self_loop: false,
                    is_join_table_candidate: false,
                    group_index: None,
                },
            ],
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
            groups: vec![],
            node_index: std::collections::BTreeMap::new(),
            reverse_index: std::collections::BTreeMap::new(),
        }
    }

    fn create_test_svg() -> SvgArtifact {
        SvgArtifact::from_trusted(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 432 400">
  <g class="node" data-id="users"><rect x="56" y="56" width="260" height="94"/></g>
  <g class="node" data-id="posts"><rect x="56" y="230" width="260" height="112"/></g>
  <g class="edge"><line x1="316" y1="286" x2="56" y2="103"/></g>
</svg>"#
                .to_string(),
        )
    }

    #[test]
    fn test_render_html_basic() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions::default();

        let result = render_html(&graph, &svg, &options).unwrap();

        assert!(result.contains("<!DOCTYPE html>"));
        assert!(result.contains("<html"));
        assert!(result.contains("</html>"));
        assert!(result.contains("<svg"));
        assert!(result.contains("data-relune-metadata"));
    }

    #[test]
    fn test_render_html_contains_svg() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions::default();

        let result = render_html(&graph, &svg, &options).unwrap();

        assert!(result.contains(r#"<svg xmlns="http://www.w3.org/2000/svg""#));
        assert!(result.contains(r#"data-id="users""#));
        assert!(result.contains(r#"data-id="posts""#));
    }

    /// Returns the byte range of the metadata JSON inside its script element.
    fn metadata_span(html: &str) -> std::ops::Range<usize> {
        let open = r#"<script type="application/json" id="relune-metadata" data-relune-metadata>"#;
        let start = html.find(open).expect("metadata script tag") + open.len();
        let end = start
            + html[start..]
                .find("</script>")
                .expect("metadata script end tag");
        start..end
    }

    /// Extracts and parses the metadata JSON the viewer reads on load.
    fn embedded_metadata(html: &str) -> serde_json::Value {
        serde_json::from_str(&html[metadata_span(html)]).expect("metadata must be valid JSON")
    }

    #[test]
    fn test_render_html_embeds_parseable_metadata() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions::default();

        let result = render_html(&graph, &svg, &options).unwrap();
        let metadata = embedded_metadata(&result);

        let table_ids: Vec<&str> = metadata["tables"]
            .as_array()
            .expect("tables array")
            .iter()
            .filter_map(|table| table["id"].as_str())
            .collect();
        assert_eq!(table_ids, vec!["users", "posts"]);
        assert_eq!(metadata["tables"][1]["columns"][1]["name"], "user_id");

        let edges = metadata["edges"].as_array().expect("edges array");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["from"], "posts");
        assert_eq!(edges[0]["to"], "users");
        assert_eq!(edges[0]["from_columns"], serde_json::json!(["user_id"]));
        assert_eq!(metadata["groups"], serde_json::json!([]));
    }

    #[test]
    fn test_render_html_keeps_hostile_names_inert_end_to_end() {
        let payload = "</script><script>alert('xss')</script>";
        let mut graph = create_test_graph();
        graph.nodes[0].id = payload.to_string();
        graph.nodes[0].label = payload.to_string();
        graph.nodes[0].table_name = payload.to_string();
        graph.nodes[0].columns[1].name = payload.to_string();
        graph.edges[0].to = payload.to_string();
        for (index, node) in graph.nodes.iter().enumerate() {
            graph.node_index.insert(node.id.clone(), index);
            graph.reverse_index.insert(index, node.id.clone());
        }

        // Lay out and render the SVG from the hostile graph instead of
        // handing the HTML renderer a pre-escaped SVG.
        let positioned = relune_layout::build_layout_from_graph_with_config(
            &graph,
            &relune_layout::LayoutConfig::default(),
        )
        .expect("layout");
        let svg = relune_render_svg::render_svg(
            &positioned,
            relune_render_svg::SvgRenderOptions {
                show_tooltips: true,
                ..Default::default()
            },
        )
        .expect("svg render");
        let options = HtmlRenderOptions {
            title: Some(payload.to_string()),
            ..Default::default()
        };

        let result = render_html(&graph, &svg, &options).unwrap();

        assert!(!result.contains(payload));
        // Outside the metadata JSON, markup only carries entity-escaped names
        // (title, heading, SVG text, and tooltips).
        let span = metadata_span(&result);
        let markup = format!("{}{}", &result[..span.start], &result[span.end..]);
        assert!(!markup.contains("<script>alert"));
        assert!(markup.contains("<title>&lt;/script&gt;&lt;script&gt;alert(&#39;xss&#39;)"));
        assert!(markup.contains("<h1>&lt;/script&gt;&lt;script&gt;alert(&#39;xss&#39;)"));
        // Inside it, nothing can close the script element or open a comment.
        let json = result[span].to_ascii_lowercase();
        assert!(!json.contains("</script"));
        assert!(!json.contains("<!--"));

        // The metadata survives escaping and decodes back to the raw names.
        let metadata = embedded_metadata(&result);
        assert_eq!(metadata["tables"][0]["id"], payload);
        assert_eq!(metadata["tables"][0]["columns"][1]["name"], payload);
        assert_eq!(metadata["edges"][0]["to"], payload);
    }

    #[test]
    fn test_render_html_custom_title() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions {
            title: Some("My Schema ERD".to_string()),
            ..Default::default()
        };

        let result = render_html(&graph, &svg, &options).unwrap();

        assert!(result.contains("<title>My Schema ERD</title>"));
        assert!(result.contains("<h1>My Schema ERD</h1>"));
    }

    #[test]
    fn test_render_html_dark_theme() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions {
            theme: Theme::Dark,
            ..Default::default()
        };

        let result = render_html(&graph, &svg, &options).unwrap();

        assert!(result.contains("--bg-color: #0c0f1a"));
        assert!(result.contains("--text-color: #e2e8f0"));
    }

    #[test]
    fn test_render_html_light_theme() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions {
            theme: Theme::Light,
            ..Default::default()
        };

        let result = render_html(&graph, &svg, &options).unwrap();

        assert!(result.contains("--bg-color: #f7f8fc"));
        assert!(result.contains("--text-color: #1e293b"));
    }

    #[test]
    fn test_render_html_self_contained() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions::default();

        let result = render_html(&graph, &svg, &options).unwrap();

        // Should not reference external HTTP resources
        assert!(result.contains("<link"));
        assert!(!result.contains("href=\"http"));
        assert!(!result.contains("src=\"http"));
    }

    #[test]
    fn test_render_html_preserves_view_and_enum_metadata() {
        let graph = LayoutGraph {
            nodes: vec![
                LayoutNode {
                    id: "active_users".to_string(),
                    label: "active_users".to_string(),
                    schema_name: None,
                    table_name: "active_users".to_string(),
                    kind: NodeKind::View,
                    columns: vec![LayoutColumn {
                        name: "id".to_string(),
                        data_type: "integer".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        is_foreign_key: false,
                        is_indexed: false,
                        change: None,
                    }],
                    inbound_count: 1,
                    outbound_count: 0,
                    has_self_loop: false,
                    is_join_table_candidate: false,
                    group_index: None,
                },
                LayoutNode {
                    id: "status".to_string(),
                    label: "status".to_string(),
                    schema_name: None,
                    table_name: "status".to_string(),
                    kind: NodeKind::Enum,
                    columns: vec![LayoutColumn {
                        name: "active".to_string(),
                        data_type: String::new(),
                        nullable: false,
                        is_primary_key: false,
                        is_foreign_key: false,
                        is_indexed: false,
                        change: None,
                    }],
                    inbound_count: 1,
                    outbound_count: 0,
                    has_self_loop: false,
                    is_join_table_candidate: false,
                    group_index: None,
                },
            ],
            edges: vec![LayoutEdge {
                from: "users".to_string(),
                to: "active_users".to_string(),
                name: Some("view dep".to_string()),
                from_columns: vec![],
                to_columns: vec![],
                kind: EdgeKind::ViewDependency,
                is_self_loop: false,
                nullable: false,
                target_cardinality: relune_core::layout::Cardinality::One,
                is_collapsed_join: false,
                collapsed_join_table: None,
            }],
            groups: vec![],
            node_index: std::collections::BTreeMap::new(),
            reverse_index: std::collections::BTreeMap::new(),
        };
        let svg = SvgArtifact::from_trusted(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 432 400">
  <g class="table-node node node-kind-view" data-table-id="active_users" data-id="active_users" data-node-kind="view"></g>
  <g class="table-node node node-kind-enum" data-table-id="status" data-id="status" data-node-kind="enum"></g>
</svg>"#
                .to_string(),
        );

        let result = render_html(&graph, &svg, &HtmlRenderOptions::default()).unwrap();

        assert!(result.contains(r#""kind":"view""#));
        assert!(result.contains(r#""kind":"enum""#));
        assert!(result.contains(r#""kind":"view_dependency""#));
        assert!(result.contains(r#"data-node-kind="view""#));
        assert!(result.contains(r#"data-node-kind="enum""#));
    }
}
