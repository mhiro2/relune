//! HTML renderer for Relune ERD diagrams.
//!
//! This crate provides a self-contained HTML document wrapper around SVG output,
//! with pan/zoom interaction and embedded graph metadata for future features.

mod error;
mod html;
mod metadata;
mod options;

pub use error::HtmlRenderError;
pub use options::{HtmlRenderOptions, Theme};

use relune_layout::LayoutGraph;

/// Render a self-contained HTML document with embedded SVG and metadata.
///
/// # Arguments
///
/// * `graph` - The layout graph containing node/edge/group information
/// * `svg` - Pre-rendered SVG content to embed
/// * `options` - HTML rendering options
///
/// # Returns
///
/// A complete, self-contained HTML document string.
pub fn render_html(
    graph: &LayoutGraph,
    svg: &str,
    options: &HtmlRenderOptions,
) -> Result<String, HtmlRenderError> {
    let metadata = metadata::build_metadata(graph);
    let metadata_json = serde_json::to_string(&metadata)?;
    let escaped_metadata = html::escape_json_for_script(&metadata_json);

    let html_document = html::build_html_document(svg, &escaped_metadata, options);

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
                        },
                        LayoutColumn {
                            name: "name".to_string(),
                            data_type: "varchar".to_string(),
                            nullable: true,
                            is_primary_key: false,
                            is_foreign_key: false,
                            is_indexed: false,
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
                        },
                        LayoutColumn {
                            name: "user_id".to_string(),
                            data_type: "integer".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            is_foreign_key: true,
                            is_indexed: true,
                        },
                        LayoutColumn {
                            name: "title".to_string(),
                            data_type: "varchar".to_string(),
                            nullable: true,
                            is_primary_key: false,
                            is_foreign_key: false,
                            is_indexed: false,
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

    fn create_test_svg() -> &'static str {
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 432 400">
  <g class="node" data-id="users"><rect x="56" y="56" width="260" height="94"/></g>
  <g class="node" data-id="posts"><rect x="56" y="230" width="260" height="112"/></g>
  <g class="edge"><line x1="316" y1="286" x2="56" y2="103"/></g>
</svg>"#
    }

    #[test]
    fn test_render_html_basic() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions::default();

        let result = render_html(&graph, svg, &options).unwrap();

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

        let result = render_html(&graph, svg, &options).unwrap();

        assert!(result.contains(r#"<svg xmlns="http://www.w3.org/2000/svg""#));
        assert!(result.contains(r#"data-id="users""#));
        assert!(result.contains(r#"data-id="posts""#));
    }

    #[test]
    fn test_render_html_contains_metadata() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions::default();

        let result = render_html(&graph, svg, &options).unwrap();

        assert!(result.contains(r#"id="relune-metadata""#));
        assert!(result.contains(r#""tables""#));
        assert!(result.contains(r#""edges""#));
        assert!(result.contains(r#""users""#));
        assert!(result.contains(r#""posts""#));
    }

    #[test]
    fn test_render_html_custom_title() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions {
            title: Some("My Schema ERD".to_string()),
            ..Default::default()
        };

        let result = render_html(&graph, svg, &options).unwrap();

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

        let result = render_html(&graph, svg, &options).unwrap();

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

        let result = render_html(&graph, svg, &options).unwrap();

        assert!(result.contains("--bg-color: #f7f8fc"));
        assert!(result.contains("--text-color: #1e293b"));
    }

    #[test]
    fn test_render_html_contains_pan_zoom_script() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions::default();

        let result = render_html(&graph, svg, &options).unwrap();

        assert!(result.contains("updateTransform"));
        assert!(result.contains("addEventListener"));
    }

    #[test]
    fn test_render_html_self_contained() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions::default();

        let result = render_html(&graph, svg, &options).unwrap();

        // Should not reference external HTTP resources
        assert!(result.contains("<link"));
        assert!(!result.contains("href=\"http"));
        assert!(!result.contains("src=\"http"));
    }

    #[test]
    fn test_metadata_structure() {
        let graph = create_test_graph();
        let svg = create_test_svg();
        let options = HtmlRenderOptions::default();

        let result = render_html(&graph, svg, &options).unwrap();

        // Check metadata contains expected structure
        assert!(result.contains(r#""id":"users""#));
        assert!(result.contains(r#""id":"posts""#));
        assert!(result.contains(r#""from":"posts""#));
        assert!(result.contains(r#""to":"users""#));
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
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 432 400">
  <g class="table-node node node-kind-view" data-table-id="active_users" data-id="active_users" data-node-kind="view"></g>
  <g class="table-node node node-kind-enum" data-table-id="status" data-id="status" data-node-kind="enum"></g>
</svg>"#;

        let result = render_html(&graph, svg, &HtmlRenderOptions::default()).unwrap();

        assert!(result.contains(r#""kind":"view""#));
        assert!(result.contains(r#""kind":"enum""#));
        assert!(result.contains(r#""kind":"view_dependency""#));
        assert!(result.contains(r#"data-node-kind="view""#));
        assert!(result.contains(r#"data-node-kind="enum""#));
    }
}
