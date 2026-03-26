//! SVG rendering for database schema diagrams.
//!
//! This crate provides SVG rendering functionality for visualizing database schemas
//! as interactive diagrams.

use std::fmt::Write;

use relune_core::{EdgeKind, NodeKind};

pub mod edge;
pub mod escape;
pub mod geometry;
pub mod group;
pub mod legend;
pub mod node;

mod options;
mod theme;

pub use edge::{EdgeRenderOptions, render_edge};
pub use geometry::{Point, Rect, clamp, compute_column_y, compute_node_height, lerp};
pub use group::render_group;
pub use legend::render_legend;
pub use node::{ColumnInfo, NodeRenderOptions, render_node};
pub use options::SvgRenderOptions;
pub use theme::{Theme, ThemeColors, get_colors};

/// Renders a positioned graph to an SVG string with the given options.
///
/// Supports group rendering for visually grouping related tables.
#[must_use]
pub fn render_svg(graph: &relune_layout::PositionedGraph, options: SvgRenderOptions) -> String {
    let colors = get_colors(options.theme);
    let mut out = String::new();

    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{:.0}" height="{:.0}" viewBox="0 0 {:.0} {:.0}" fill="none">"#,
        graph.width, graph.height, graph.width, graph.height
    );
    out_push_defs(&mut out, &colors);
    let _ = write!(
        out,
        r#"<rect width="100%" height="100%" fill="{}"/>"#,
        colors.background
    );
    out.push_str(r#"<rect width="100%" height="100%" fill="url(#canvas-grid)" opacity="0.92"/>"#);

    // Render groups FIRST (behind nodes and edges)
    for group in &graph.groups {
        render_group(&mut out, group, &colors);
    }

    // Render edges with enhanced options
    let edge_options = EdgeRenderOptions {
        stroke_width: 2.0,
        show_tooltips: options.show_tooltips,
        ..EdgeRenderOptions::default()
    };
    for (index, edge) in graph.edges.iter().enumerate() {
        render_edge_internal(&mut out, edge, &colors, &edge_options, index);
    }

    // Render nodes
    for (index, node) in graph.nodes.iter().enumerate() {
        render_node_internal(&mut out, node, &colors, options.show_tooltips, index);
    }

    // Render legend if requested
    if options.show_legend {
        render_legend(&mut out, &colors, None, graph.width, graph.height);
    }

    out.push_str("</svg>");
    out
}

fn out_push_defs(out: &mut String, colors: &ThemeColors) {
    let (grid_base, grid_dot, shadow_color, hatch_color) = if is_light_theme(colors) {
        ("#f7f8fc", "#e8eaf0", "rgba(15, 23, 42, 0.12)", "#cbd5e1")
    } else {
        ("#0c0f1a", "#151928", "rgba(0, 0, 0, 0.52)", "#334155")
    };

    let _ = write!(
        out,
        r##"<defs>
<pattern id="canvas-grid" width="32" height="32" patternUnits="userSpaceOnUse">
<rect width="32" height="32" fill="{grid_base}"/>
<circle cx="2" cy="2" r="1.2" fill="{grid_dot}" fill-opacity="0.9"/>
</pattern>
<pattern id="type-filter-hatch" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="rotate(35)">
<rect width="8" height="8" fill="transparent"/>
<rect width="3" height="8" fill="{hatch_color}" fill-opacity="0.42"/>
</pattern>
<filter id="node-shadow" x="-20%" y="-20%" width="140%" height="150%">
<feDropShadow dx="0" dy="6" stdDeviation="8" flood-color="{shadow_color}"/>
</filter>
<filter id="edge-glow" x="-50%" y="-50%" width="200%" height="200%">
<feDropShadow dx="0" dy="0" stdDeviation="5" flood-color="#f59e0b"/>
</filter>
<marker id="arrow" markerWidth="10" markerHeight="10" refX="9" refY="3" orient="auto">
<path d="M0,0 L0,6 L9,3 z" fill="{}" />
</marker>
<marker id="cardinality-one" markerWidth="12" markerHeight="12" refX="10" refY="6" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
<path d="M7 1 L7 11" stroke="{}" stroke-width="1.8" stroke-linecap="round"/>
</marker>
<marker id="cardinality-many" markerWidth="12" markerHeight="12" refX="10" refY="6" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
<path d="M1 1 L10 6 M1 6 L10 6 M1 11 L10 6" stroke="{}" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
</marker>
<marker id="cardinality-zero-many" markerWidth="16" markerHeight="12" refX="14" refY="6" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
<circle cx="3.5" cy="6" r="2.2" fill="none" stroke="{}" stroke-width="1.4"/>
<path d="M6 1 L14 6 M6 6 L14 6 M6 11 L14 6" stroke="{}" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
</marker>
</defs>"##,
        colors.arrow_fill,
        colors.text_secondary,
        colors.text_secondary,
        colors.text_secondary,
        colors.text_secondary
    );
}

/// Node rendering for relune-layout `PositionedNode`.
#[allow(clippy::cast_precision_loss)] // Entry animation indices are presentation-only.
#[allow(clippy::too_many_lines)] // SVG node markup is easier to audit in one place.
fn render_node_internal(
    out: &mut String,
    node: &relune_layout::PositionedNode,
    colors: &ThemeColors,
    show_tooltips: bool,
    index: usize,
) {
    let kind = node_kind_name(node.kind);
    let node_style = node_style(node.kind, colors);
    let node_label = node_kind_label(node.kind);
    let _ = write!(
        out,
        r#"<g class="table-node node node-kind-{}" data-table-id="{}" data-id="{}" data-node-kind="{}" style="--enter-delay:{:.3}s">"#,
        kind,
        escape_attribute(&node.id),
        escape_attribute(&node.id),
        kind,
        index as f32 * 0.022
    );

    // Add tooltip if enabled
    if show_tooltips {
        let column_count = node.columns.len();
        let pk_count = node.columns.iter().filter(|c| c.is_primary_key).count();
        let mut tooltip_parts = vec![
            format!("{} {}", node.label, node_label),
            format!(
                "{} column{}",
                column_count,
                if column_count == 1 { "" } else { "s" }
            ),
        ];
        if pk_count > 0 {
            tooltip_parts.push(format!(
                "{} primary key{}",
                pk_count,
                if pk_count == 1 { "" } else { "s" }
            ));
        }
        let _ = write!(
            out,
            r"<title>{}</title>",
            escape_text(&tooltip_parts.join("\n"))
        );
    }

    let _ = write!(
        out,
        r#"<rect class="table-body" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="16" ry="16" fill="{}" stroke="{}" stroke-width="1.6" filter="url(#node-shadow)"/>"#,
        node.x, node.y, node.width, node.height, node_style.body_fill, node_style.stroke
    );
    let _ = write!(
        out,
        r#"<rect class="table-header" x="{:.1}" y="{:.1}" width="{:.1}" height="36" rx="16" ry="16" fill="{}"/>"#,
        node.x, node.y, node.width, node_style.header_fill
    );
    let _ = write!(
        out,
        r#"<rect class="table-header-underlay" x="{:.1}" y="{:.1}" width="{:.1}" height="18" fill="{}" fill-opacity="0.28"/>"#,
        node.x,
        node.y + 18.0,
        node.width,
        node_style.header_fill
    );
    let _ = write!(
        out,
        r#"<text class="table-name" x="{:.1}" y="{:.1}" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="14" font-weight="700" letter-spacing="0.02em" fill="{}">{}</text>"#,
        node.x + 12.0,
        node.y + 23.0,
        colors.text_primary,
        escape_text(&node.label)
    );
    let _ = write!(
        out,
        r#"<text class="table-kind" x="{:.1}" y="{:.1}" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="10" font-weight="600" text-anchor="end" letter-spacing="0.12em" fill="{}">{}</text>"#,
        node.x + node.width - 12.0,
        node.y + 23.0,
        colors.text_primary,
        escape_text(&kind.to_ascii_uppercase())
    );

    let mut line_y = node.y + 52.0;
    for (index, column) in node.columns.iter().enumerate() {
        if index > 0 {
            let separator_y = line_y - 12.0;
            let _ = write!(
                out,
                r#"<line class="column-separator" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-opacity="0.75" stroke-width="1"/>"#,
                node.x + 12.0,
                separator_y,
                node.x + node.width - 12.0,
                separator_y,
                node_style.stroke
            );
        }
        let text = if node.kind == NodeKind::Enum {
            format!("• {}", column.name)
        } else if column.data_type.is_empty() {
            column.name.clone()
        } else {
            format!("{}: {}", column.name, column.data_type)
        };
        let _ = write!(
            out,
            r#"<text class="column-name" x="{:.1}" y="{:.1}" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="12" fill="{}">{}</text>"#,
            node.x + 12.0,
            line_y,
            if column.nullable {
                colors.text_muted
            } else {
                colors.text_secondary
            },
            escape_text(&text)
        );
        if column.is_primary_key {
            let icon_x = node.x + node.width - 26.0;
            let icon_y = line_y - 8.5;
            let _ = write!(
                out,
                r##"<path class="pk-indicator" d="M{:.1} {:.1}a3.2 3.2 0 1 0 0.01 0M{:.1} {:.1}h7m-2.4 0v2.1m-2.2 -2.1v3.4" fill="none" stroke="#fbbf24" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>"##,
                icon_x,
                icon_y + 3.2,
                icon_x + 3.2,
                icon_y + 3.2
            );
        }
        line_y += 18.0;
    }
    let _ = write!(
        out,
        r#"<rect class="type-filter-overlay" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="16" ry="16" fill="url(#type-filter-hatch)" opacity="0"/>"#,
        node.x, node.y, node.width, node.height
    );
    out.push_str("</g>");
}

/// Edge rendering for relune-layout `PositionedEdge`.
#[allow(clippy::cast_precision_loss)] // Entry animation indices are presentation-only.
#[allow(clippy::suboptimal_flops)] // Render-time coordinate math favors readability here.
fn render_edge_internal(
    out: &mut String,
    edge: &relune_layout::PositionedEdge,
    colors: &ThemeColors,
    options: &EdgeRenderOptions,
    index: usize,
) {
    let kind = edge_kind_name(edge.kind);
    let edge_style = edge_style(edge.kind, colors);
    let path_d = crate::edge::edge_route_svg_path_d(&edge.route, options.curve_offset);
    let uses_crow_markers = edge.kind == EdgeKind::ForeignKey;

    let stroke_dasharray = if options.dashed {
        Some("5,3")
    } else {
        edge_style.dasharray
    };

    let _ = write!(
        out,
        r#"<g class="edge edge-kind-{}" data-from="{}" data-to="{}" data-edge-kind="{}" style="--enter-delay:{:.3}s">"#,
        kind,
        escape_attribute(&edge.from),
        escape_attribute(&edge.to),
        kind,
        index as f32 * 0.016 + 0.04
    );

    // Add tooltip if enabled
    if options.show_tooltips {
        let tooltip_text = generate_edge_tooltip(edge);
        let _ = write!(out, r"<title>{}</title>", escape_text(&tooltip_text));
    }

    // Render the path with CSS class and data attributes
    match stroke_dasharray {
        Some(stroke_dasharray) => {
            let _ = write!(
                out,
                r##"<path class="edge-glow-path" d="{}" stroke="#f59e0b" stroke-width="{:.1}" fill="none" stroke-linecap="round" stroke-linejoin="round" opacity="0" filter="url(#edge-glow)"/><path class="edge-path" d="{}" stroke="{}" stroke-width="{:.1}" fill="none" stroke-linecap="round" stroke-linejoin="round"{} stroke-dasharray="{}"/>"##,
                escape_attribute(&path_d),
                options.stroke_width + 2.0,
                escape_attribute(&path_d),
                edge_style.stroke,
                options.stroke_width,
                edge_marker_attributes(uses_crow_markers, edge.nullable),
                stroke_dasharray
            );
        }
        None => {
            let _ = write!(
                out,
                r##"<path class="edge-glow-path" d="{}" stroke="#f59e0b" stroke-width="{:.1}" fill="none" stroke-linecap="round" stroke-linejoin="round" opacity="0" filter="url(#edge-glow)"/><path class="edge-path" d="{}" stroke="{}" stroke-width="{:.1}" fill="none" stroke-linecap="round" stroke-linejoin="round"{} />"##,
                escape_attribute(&path_d),
                options.stroke_width + 2.0,
                escape_attribute(&path_d),
                edge_style.stroke,
                options.stroke_width,
                edge_marker_attributes(uses_crow_markers, edge.nullable)
            );
        }
    }

    // Render the label if enabled
    if options.show_labels && !edge.label.is_empty() {
        let label_x = edge.label_x;
        let label_y = edge.label_y;
        let label_width = estimate_label_width(&edge.label);
        let _ = write!(
            out,
            r#"<rect class="edge-label-pill" x="{:.1}" y="{:.1}" width="{:.1}" height="18" rx="9" ry="9" fill="{}" fill-opacity="0.92" stroke="{}" stroke-opacity="0.65"/>"#,
            label_width.mul_add(-0.5, label_x),
            label_y - 12.0,
            label_width,
            node_label_background(colors),
            edge_style.stroke
        );

        let _ = write!(
            out,
            r#"<text class="edge-label" x="{:.1}" y="{:.1}" font-family="'Inter', 'Segoe UI', system-ui, sans-serif" font-size="11" font-weight="600" text-anchor="middle" fill="{}">{}</text>"#,
            label_x,
            label_y,
            edge_style.label_fill.unwrap_or(colors.text_muted),
            escape_text(&edge.label)
        );
    }

    out.push_str("</g>");
}

/// Generates tooltip text for a relune-layout `PositionedEdge`.
fn generate_edge_tooltip(edge: &relune_layout::PositionedEdge) -> String {
    let mut lines = Vec::new();

    let edge_kind = match edge.kind {
        EdgeKind::ForeignKey => "Foreign Key",
        EdgeKind::EnumReference => "Enum Reference",
        EdgeKind::ViewDependency => "View Dependency",
    };

    if edge.label.is_empty() {
        lines.push(edge_kind.to_string());
    } else {
        lines.push(format!("{edge_kind}: {}", edge.label));
    }

    if !edge.from_columns.is_empty() && !edge.to_columns.is_empty() {
        let from_cols = edge.from_columns.join(", ");
        let to_cols = edge.to_columns.join(", ");
        lines.push(format!(
            "{}.{} -> {}.{}",
            edge.from, from_cols, edge.to, to_cols
        ));
    } else {
        lines.push(format!("{} -> {}", edge.from, edge.to));
    }

    if edge.kind != EdgeKind::ViewDependency {
        if edge.nullable {
            lines.push("Nullable: Yes".to_string());
        } else {
            lines.push("Nullable: No".to_string());
        }
    }

    lines.join("\n")
}

struct NodeStyle {
    body_fill: &'static str,
    header_fill: &'static str,
    stroke: &'static str,
}

struct EdgeStyle {
    stroke: &'static str,
    dasharray: Option<&'static str>,
    label_fill: Option<&'static str>,
}

fn is_light_theme(colors: &ThemeColors) -> bool {
    colors.background == "#ffffff"
}

const fn node_kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Table => "table",
        NodeKind::View => "view",
        NodeKind::Enum => "enum",
    }
}

const fn node_kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Table => "table",
        NodeKind::View => "view",
        NodeKind::Enum => "enum",
    }
}

fn node_style(kind: NodeKind, colors: &ThemeColors) -> NodeStyle {
    match (kind, is_light_theme(colors)) {
        (NodeKind::Table, false) => NodeStyle {
            body_fill: "#151926",
            header_fill: "#8b5e1a",
            stroke: "#fbbf24",
        },
        (NodeKind::Table, true) => NodeStyle {
            body_fill: "#fffaf0",
            header_fill: "#f59e0b",
            stroke: "#d97706",
        },
        (NodeKind::View, false) => NodeStyle {
            body_fill: "#10232a",
            header_fill: "#0f766e",
            stroke: "#2dd4bf",
        },
        (NodeKind::View, true) => NodeStyle {
            body_fill: "#f0fdfa",
            header_fill: "#14b8a6",
            stroke: "#0f766e",
        },
        (NodeKind::Enum, false) => NodeStyle {
            body_fill: "#241533",
            header_fill: "#7c3aed",
            stroke: "#c084fc",
        },
        (NodeKind::Enum, true) => NodeStyle {
            body_fill: "#faf5ff",
            header_fill: "#a855f7",
            stroke: "#7e22ce",
        },
    }
}

const fn edge_kind_name(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::ForeignKey => "foreign-key",
        EdgeKind::EnumReference => "enum-reference",
        EdgeKind::ViewDependency => "view-dependency",
    }
}

fn edge_style(kind: EdgeKind, colors: &ThemeColors) -> EdgeStyle {
    match (kind, is_light_theme(colors)) {
        (EdgeKind::ForeignKey, _) => EdgeStyle {
            stroke: if is_light_theme(colors) {
                "#64748b"
            } else {
                "#475569"
            },
            dasharray: None,
            label_fill: None,
        },
        (EdgeKind::EnumReference, false) => EdgeStyle {
            stroke: "#f59e0b",
            dasharray: Some("6,4"),
            label_fill: Some("#fbbf24"),
        },
        (EdgeKind::EnumReference, true) => EdgeStyle {
            stroke: "#d97706",
            dasharray: Some("6,4"),
            label_fill: Some("#b45309"),
        },
        (EdgeKind::ViewDependency, false) => EdgeStyle {
            stroke: "#2dd4bf",
            dasharray: Some("4,4"),
            label_fill: Some("#5eead4"),
        },
        (EdgeKind::ViewDependency, true) => EdgeStyle {
            stroke: "#0f766e",
            dasharray: Some("4,4"),
            label_fill: Some("#115e59"),
        },
    }
}

const fn edge_marker_attributes(uses_crow_markers: bool, nullable: bool) -> &'static str {
    if uses_crow_markers {
        if nullable {
            r#" marker-start="url(#cardinality-zero-many)" marker-end="url(#cardinality-one)""#
        } else {
            r#" marker-start="url(#cardinality-many)" marker-end="url(#cardinality-one)""#
        }
    } else {
        r#" marker-end="url(#arrow)""#
    }
}

fn estimate_label_width(text: &str) -> f32 {
    text.chars()
        .map(|ch| if ch.is_ascii() { 6.4 } else { 10.0 })
        .sum::<f32>()
        + 18.0
}

fn node_label_background(colors: &ThemeColors) -> &'static str {
    if is_light_theme(colors) {
        "#ffffff"
    } else {
        "#111827"
    }
}

use escape::{escape_attribute, escape_text};

#[cfg(test)]
mod tests {
    use super::*;
    use relune_core::{EdgeKind, NodeKind};
    use relune_layout::{
        EdgeRoute, PositionedColumn, PositionedEdge, PositionedGroup, PositionedNode, RouteStyle,
    };

    /// Helper function to create a test `PositionedGraph` with empty state
    fn empty_graph() -> relune_layout::PositionedGraph {
        relune_layout::PositionedGraph {
            nodes: vec![],
            edges: vec![],
            groups: vec![],
            width: 800.0,
            height: 600.0,
        }
    }

    /// Helper function to create a test `PositionedGraph` with a single node
    fn single_node_graph() -> relune_layout::PositionedGraph {
        relune_layout::PositionedGraph {
            nodes: vec![PositionedNode {
                id: "users".to_string(),
                label: "users".to_string(),
                kind: NodeKind::Table,
                columns: vec![
                    PositionedColumn {
                        name: "id".to_string(),
                        data_type: "uuid PK".to_string(),
                        nullable: false,
                        is_primary_key: true,
                    },
                    PositionedColumn {
                        name: "name".to_string(),
                        data_type: "text".to_string(),
                        nullable: false,
                        is_primary_key: false,
                    },
                ],
                x: 56.0,
                y: 56.0,
                width: 260.0,
                height: 94.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            }],
            edges: vec![],
            groups: vec![],
            width: 432.0,
            height: 206.0,
        }
    }

    /// Helper function to create a test `PositionedGraph` with multiple nodes and edges
    fn multi_node_graph() -> relune_layout::PositionedGraph {
        relune_layout::PositionedGraph {
            nodes: vec![
                PositionedNode {
                    id: "users".to_string(),
                    label: "users".to_string(),
                    kind: NodeKind::Table,
                    columns: vec![
                        PositionedColumn {
                            name: "id".to_string(),
                            data_type: "uuid PK".to_string(),
                            nullable: false,
                            is_primary_key: true,
                        },
                        PositionedColumn {
                            name: "name".to_string(),
                            data_type: "text".to_string(),
                            nullable: false,
                            is_primary_key: false,
                        },
                    ],
                    x: 56.0,
                    y: 56.0,
                    width: 260.0,
                    height: 94.0,
                    is_join_table_candidate: false,
                    has_self_loop: false,
                    group_index: None,
                },
                PositionedNode {
                    id: "posts".to_string(),
                    label: "posts".to_string(),
                    kind: NodeKind::Table,
                    columns: vec![
                        PositionedColumn {
                            name: "id".to_string(),
                            data_type: "uuid PK".to_string(),
                            nullable: false,
                            is_primary_key: true,
                        },
                        PositionedColumn {
                            name: "user_id".to_string(),
                            data_type: "uuid".to_string(),
                            nullable: false,
                            is_primary_key: false,
                        },
                        PositionedColumn {
                            name: "title".to_string(),
                            data_type: "text".to_string(),
                            nullable: false,
                            is_primary_key: false,
                        },
                    ],
                    x: 56.0,
                    y: 230.0,
                    width: 260.0,
                    height: 112.0,
                    is_join_table_candidate: false,
                    has_self_loop: false,
                    group_index: None,
                },
            ],
            edges: vec![PositionedEdge {
                from: "posts".to_string(),
                to: "users".to_string(),
                label: "user_id".to_string(),
                kind: EdgeKind::ForeignKey,
                route: EdgeRoute {
                    x1: 316.0,
                    y1: 286.0,
                    x2: 56.0,
                    y2: 103.0,
                    control_points: vec![],
                    style: RouteStyle::Straight,
                    label_position: (186.0, 194.5),
                },
                is_self_loop: false,
                nullable: false,
                from_columns: vec!["user_id".to_string()],
                to_columns: vec!["id".to_string()],
                is_collapsed_join: false,
                collapsed_join_table: None,
                label_x: 186.0,
                label_y: 194.5,
            }],
            groups: vec![],
            width: 432.0,
            height: 398.0,
        }
    }

    #[test]
    fn test_render_svg_empty_graph() {
        let graph = empty_graph();
        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Should contain valid SVG structure
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.contains("viewBox=\"0 0 800 600\""));
    }

    #[test]
    fn test_render_svg_single_node() {
        let graph = single_node_graph();
        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Should contain the node label
        assert!(svg.contains(">users<"));
        // Should contain the columns (now in "name: type" format from PositionedColumn)
        assert!(svg.contains("id: uuid PK"));
        assert!(svg.contains("name: text"));
        // Should contain valid SVG structure
        assert!(svg.contains("xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.contains("<rect"));
        assert!(svg.contains("<text"));
    }

    #[test]
    fn test_render_svg_multiple_nodes_and_edges() {
        let graph = multi_node_graph();
        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Should contain both node labels
        assert!(svg.contains(">users<"));
        assert!(svg.contains(">posts<"));
        // Should contain edge label
        assert!(svg.contains("user_id"));
        // Should contain edge path
        assert!(svg.contains("<path"));
        assert!(svg.contains("marker-end=\"url(#cardinality-one)\""));
        // Should contain arrow marker definition
        assert!(svg.contains("<marker id=\"arrow\""));
    }

    #[test]
    #[allow(clippy::too_many_lines)] // Builds a compact end-to-end fixture for mixed node kinds.
    fn test_render_svg_view_and_enum_nodes() {
        let graph = relune_layout::PositionedGraph {
            nodes: vec![
                PositionedNode {
                    id: "users".to_string(),
                    label: "users".to_string(),
                    kind: NodeKind::Table,
                    columns: vec![PositionedColumn {
                        name: "status".to_string(),
                        data_type: "status".to_string(),
                        nullable: false,
                        is_primary_key: false,
                    }],
                    x: 56.0,
                    y: 56.0,
                    width: 260.0,
                    height: 76.0,
                    is_join_table_candidate: false,
                    has_self_loop: false,
                    group_index: None,
                },
                PositionedNode {
                    id: "active_users".to_string(),
                    label: "active_users".to_string(),
                    kind: NodeKind::View,
                    columns: vec![PositionedColumn {
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: false,
                    }],
                    x: 396.0,
                    y: 56.0,
                    width: 260.0,
                    height: 76.0,
                    is_join_table_candidate: false,
                    has_self_loop: false,
                    group_index: None,
                },
                PositionedNode {
                    id: "status".to_string(),
                    label: "status".to_string(),
                    kind: NodeKind::Enum,
                    columns: vec![
                        PositionedColumn {
                            name: "active".to_string(),
                            data_type: String::new(),
                            nullable: false,
                            is_primary_key: false,
                        },
                        PositionedColumn {
                            name: "inactive".to_string(),
                            data_type: String::new(),
                            nullable: false,
                            is_primary_key: false,
                        },
                    ],
                    x: 396.0,
                    y: 216.0,
                    width: 260.0,
                    height: 94.0,
                    is_join_table_candidate: false,
                    has_self_loop: false,
                    group_index: None,
                },
            ],
            edges: vec![
                PositionedEdge {
                    from: "users".to_string(),
                    to: "active_users".to_string(),
                    label: "view dep".to_string(),
                    kind: EdgeKind::ViewDependency,
                    route: EdgeRoute {
                        x1: 316.0,
                        y1: 94.0,
                        x2: 396.0,
                        y2: 94.0,
                        control_points: vec![],
                        style: RouteStyle::Straight,
                        label_position: (356.0, 94.0),
                    },
                    is_self_loop: false,
                    nullable: false,
                    from_columns: vec![],
                    to_columns: vec![],
                    is_collapsed_join: false,
                    collapsed_join_table: None,
                    label_x: 356.0,
                    label_y: 94.0,
                },
                PositionedEdge {
                    from: "users".to_string(),
                    to: "status".to_string(),
                    label: "status (status)".to_string(),
                    kind: EdgeKind::EnumReference,
                    route: EdgeRoute {
                        x1: 316.0,
                        y1: 112.0,
                        x2: 396.0,
                        y2: 263.0,
                        control_points: vec![],
                        style: RouteStyle::Straight,
                        label_position: (356.0, 187.5),
                    },
                    is_self_loop: false,
                    nullable: false,
                    from_columns: vec!["status".to_string()],
                    to_columns: vec![],
                    is_collapsed_join: false,
                    collapsed_join_table: None,
                    label_x: 356.0,
                    label_y: 187.5,
                },
            ],
            groups: vec![],
            width: 712.0,
            height: 366.0,
        };

        let svg = render_svg(&graph, SvgRenderOptions::default());

        assert!(svg.contains("node-kind-view"));
        assert!(svg.contains("node-kind-enum"));
        assert!(svg.contains("edge-kind-view-dependency"));
        assert!(svg.contains("edge-kind-enum-reference"));
        assert!(svg.contains("stroke-dasharray=\"4,4\""));
        assert!(svg.contains("stroke-dasharray=\"6,4\""));
        assert!(svg.contains("• active"));
        assert!(svg.contains("data-node-kind=\"view\""));
        assert!(svg.contains("data-node-kind=\"enum\""));
    }

    #[test]
    fn test_render_svg_contains_valid_svg_structure() {
        let graph = single_node_graph();
        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Check for valid SVG root element with xmlns
        assert!(svg.contains("xmlns=\"http://www.w3.org/2000/svg\""));
        // Check for viewBox attribute
        assert!(svg.contains("viewBox="));
        // Check for width and height
        assert!(svg.contains("width=\""));
        assert!(svg.contains("height=\""));
        // Check for defs with arrow marker
        assert!(svg.contains("<defs>"));
        assert!(svg.contains("</defs>"));
        // Check for background rect
        assert!(svg.contains("<rect width=\"100%\" height=\"100%\""));
    }

    #[test]
    fn test_escape_text_special_characters() {
        // Test ampersand
        assert_eq!(escape_text("a & b"), "a &amp; b");
        // Test less than
        assert_eq!(escape_text("a < b"), "a &lt; b");
        // Test greater than
        assert_eq!(escape_text("a > b"), "a &gt; b");
        // Test double quote
        assert_eq!(escape_text("a \"b\" c"), "a &quot;b&quot; c");
        // Test single quote
        assert_eq!(escape_text("a 'b' c"), "a &#39;b&#39; c");
        // Test combined
        assert_eq!(
            escape_text("<script>alert('xss')</script>"),
            "&lt;script&gt;alert(&#39;xss&#39;)&lt;/script&gt;"
        );
    }

    #[test]
    fn test_render_svg_escapes_special_characters_in_labels() {
        let graph = relune_layout::PositionedGraph {
            nodes: vec![PositionedNode {
                id: "test".to_string(),
                label: "Test & <Label>".to_string(),
                kind: NodeKind::Table,
                columns: vec![PositionedColumn {
                    name: "col \"name\"".to_string(),
                    data_type: "text".to_string(),
                    nullable: false,
                    is_primary_key: false,
                }],
                x: 56.0,
                y: 56.0,
                width: 260.0,
                height: 94.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            }],
            edges: vec![PositionedEdge {
                from: "a".to_string(),
                to: "b".to_string(),
                label: "FK 'test'".to_string(),
                kind: EdgeKind::ForeignKey,
                route: EdgeRoute {
                    x1: 316.0,
                    y1: 100.0,
                    x2: 400.0,
                    y2: 100.0,
                    control_points: vec![],
                    style: RouteStyle::Straight,
                    label_position: (358.0, 100.0),
                },
                is_self_loop: false,
                nullable: false,
                from_columns: vec!["test_id".to_string()],
                to_columns: vec!["id".to_string()],
                is_collapsed_join: false,
                collapsed_join_table: None,
                label_x: 358.0,
                label_y: 100.0,
            }],
            groups: vec![],
            width: 500.0,
            height: 200.0,
        };

        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Escaped characters should appear in the output
        assert!(svg.contains("&amp;"));
        assert!(svg.contains("&lt;"));
        assert!(svg.contains("&gt;"));
        assert!(svg.contains("&quot;"));
        assert!(svg.contains("&#39;"));
        // Raw special characters should not appear (except in SVG syntax)
        assert!(!svg.contains("Test & <Label>"));
        assert!(!svg.contains("col \"name\""));
        assert!(!svg.contains("FK 'test'"));
    }

    #[test]
    fn test_render_svg_escapes_xss_payload_in_table_name() {
        let payload = "<script>alert('xss')</script>";
        let escaped = "&lt;script&gt;alert(&#39;xss&#39;)&lt;/script&gt;";
        let graph = relune_layout::PositionedGraph {
            nodes: vec![PositionedNode {
                id: payload.to_string(),
                label: payload.to_string(),
                kind: NodeKind::Table,
                columns: vec![PositionedColumn {
                    name: payload.to_string(),
                    data_type: "text".to_string(),
                    nullable: false,
                    is_primary_key: false,
                }],
                x: 56.0,
                y: 56.0,
                width: 280.0,
                height: 94.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            }],
            edges: vec![],
            groups: vec![],
            width: 400.0,
            height: 180.0,
        };

        let svg = render_svg(
            &graph,
            SvgRenderOptions {
                show_tooltips: true,
                ..Default::default()
            },
        );

        assert!(svg.contains(escaped));
        assert!(svg.contains(&format!(r#"data-table-id="{escaped}""#)));
        assert!(svg.contains(&format!(r"<title>{escaped} table")));
        assert!(!svg.contains(payload));
    }

    #[test]
    fn test_render_svg_with_dark_theme() {
        let graph = single_node_graph();
        let options = SvgRenderOptions {
            theme: Theme::Dark,
            ..Default::default()
        };
        let svg = render_svg(&graph, options);

        // Dark theme background
        assert!(svg.contains("#0f172a"));
        // Dark theme node fill
        assert!(svg.contains("#151926"));
    }

    #[test]
    fn test_render_svg_with_light_theme() {
        let graph = single_node_graph();
        let options = SvgRenderOptions {
            theme: Theme::Light,
            ..Default::default()
        };
        let svg = render_svg(&graph, options);

        // Light theme background
        assert!(svg.contains("#ffffff"));
        // Light theme node fill
        assert!(svg.contains("#fffaf0"));
    }

    #[test]
    fn test_render_svg_deterministic() {
        let graph = multi_node_graph();

        // Generate SVG multiple times
        let svg1 = render_svg(&graph, SvgRenderOptions::default());
        let svg2 = render_svg(&graph, SvgRenderOptions::default());
        let svg3 = render_svg(&graph, SvgRenderOptions::default());

        // All outputs should be identical
        assert_eq!(svg1, svg2);
        assert_eq!(svg2, svg3);
    }

    #[test]
    fn test_render_svg_with_empty_columns() {
        let graph = relune_layout::PositionedGraph {
            nodes: vec![PositionedNode {
                id: "empty".to_string(),
                label: "EmptyTable".to_string(),
                kind: NodeKind::Table,
                columns: vec![],
                x: 56.0,
                y: 56.0,
                width: 260.0,
                height: 58.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            }],
            edges: vec![],
            groups: vec![],
            width: 432.0,
            height: 170.0,
        };

        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Should render without errors
        assert!(svg.contains(">EmptyTable<"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn test_render_svg_with_legend() {
        let graph = empty_graph();
        let options = SvgRenderOptions {
            show_legend: true,
            ..Default::default()
        };
        let svg = render_svg(&graph, options);

        // Should contain legend elements
        assert!(svg.contains("class=\"legend\""));
        assert!(svg.contains("class=\"legend-background\""));
        assert!(svg.contains("Legend"));
        assert!(svg.contains("Primary Key"));
        assert!(svg.contains("Foreign Key"));
        assert!(svg.contains("Indexed"));
        assert!(svg.contains("nullable"));
    }

    #[test]
    fn test_render_edge_with_css_classes() {
        let graph = relune_layout::PositionedGraph {
            nodes: vec![],
            edges: vec![PositionedEdge {
                from: "users".to_string(),
                to: "posts".to_string(),
                label: "user_id".to_string(),
                kind: EdgeKind::ForeignKey,
                route: EdgeRoute {
                    x1: 100.0,
                    y1: 50.0,
                    x2: 300.0,
                    y2: 150.0,
                    control_points: vec![],
                    style: RouteStyle::Straight,
                    label_position: (200.0, 100.0),
                },
                is_self_loop: false,
                nullable: false,
                from_columns: vec!["user_id".to_string()],
                to_columns: vec!["id".to_string()],
                is_collapsed_join: false,
                collapsed_join_table: None,
                label_x: 200.0,
                label_y: 100.0,
            }],
            groups: vec![],
            width: 400.0,
            height: 200.0,
        };

        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Should contain edge CSS classes and data attributes
        assert!(svg.contains("class=\"edge-path\""));
        assert!(svg.contains("data-from=\"users\""));
        assert!(svg.contains("data-to=\"posts\""));
        assert!(svg.contains("class=\"edge-label\""));
    }

    #[test]
    fn test_render_svg_with_tooltips_on_nodes() {
        let graph = single_node_graph();
        let options = SvgRenderOptions {
            show_tooltips: true,
            ..Default::default()
        };
        let svg = render_svg(&graph, options);

        // Should contain title elements for tooltips
        assert!(svg.contains("<title>"));
        assert!(svg.contains("users table"));
        assert!(svg.contains("2 columns"));
    }

    #[test]
    fn test_render_svg_with_tooltips_on_edges() {
        let graph = multi_node_graph();
        let options = SvgRenderOptions {
            show_tooltips: true,
            ..Default::default()
        };
        let svg = render_svg(&graph, options);

        // Should contain title elements for edge tooltips
        assert!(svg.contains("<title>"));
        // Edge tooltip should show FK relationship
        assert!(svg.contains("Foreign Key: user_id"));
        // Edge tooltip should show column mapping (note: > is escaped as &gt; in XML/SVG)
        assert!(svg.contains("posts.user_id -&gt; users.id"));
    }

    #[test]
    fn test_render_svg_without_tooltips() {
        let graph = multi_node_graph();
        let options = SvgRenderOptions {
            show_tooltips: false,
            ..Default::default()
        };
        let svg = render_svg(&graph, options);

        // Should NOT contain title elements for tooltips
        assert!(!svg.contains("<title>users table"));
        assert!(!svg.contains("<title>Foreign Key"));
    }

    // === Tests for render_svg with groups ===

    /// Helper function to create a test layout `PositionedGraph` with groups
    fn layout_graph_with_groups() -> relune_layout::PositionedGraph {
        relune_layout::PositionedGraph {
            nodes: vec![
                PositionedNode {
                    id: "users".to_string(),
                    label: "users".to_string(),
                    kind: NodeKind::Table,
                    columns: vec![
                        PositionedColumn {
                            name: "id".to_string(),
                            data_type: "uuid".to_string(),
                            nullable: false,
                            is_primary_key: true,
                        },
                        PositionedColumn {
                            name: "name".to_string(),
                            data_type: "text".to_string(),
                            nullable: false,
                            is_primary_key: false,
                        },
                    ],
                    x: 56.0,
                    y: 56.0,
                    width: 260.0,
                    height: 94.0,
                    is_join_table_candidate: false,
                    has_self_loop: false,
                    group_index: Some(0),
                },
                PositionedNode {
                    id: "posts".to_string(),
                    label: "posts".to_string(),
                    kind: NodeKind::Table,
                    columns: vec![
                        PositionedColumn {
                            name: "id".to_string(),
                            data_type: "uuid".to_string(),
                            nullable: false,
                            is_primary_key: true,
                        },
                        PositionedColumn {
                            name: "user_id".to_string(),
                            data_type: "uuid".to_string(),
                            nullable: false,
                            is_primary_key: false,
                        },
                    ],
                    x: 56.0,
                    y: 230.0,
                    width: 260.0,
                    height: 94.0,
                    is_join_table_candidate: false,
                    has_self_loop: false,
                    group_index: Some(0),
                },
            ],
            edges: vec![PositionedEdge {
                from: "posts".to_string(),
                to: "users".to_string(),
                label: "user_id".to_string(),
                kind: EdgeKind::ForeignKey,
                route: EdgeRoute {
                    x1: 316.0,
                    y1: 277.0,
                    x2: 56.0,
                    y2: 103.0,
                    control_points: vec![],
                    style: RouteStyle::Straight,
                    label_position: (186.0, 190.0),
                },
                is_self_loop: false,
                nullable: false,
                from_columns: vec!["user_id".to_string()],
                to_columns: vec!["id".to_string()],
                is_collapsed_join: false,
                collapsed_join_table: None,
                label_x: 186.0,
                label_y: 190.0,
            }],
            groups: vec![PositionedGroup {
                id: "user_domain".to_string(),
                label: "User Domain".to_string(),
                x: 36.0,
                y: 36.0,
                width: 300.0,
                height: 308.0,
            }],
            width: 432.0,
            height: 398.0,
        }
    }

    /// Helper function to create a test layout `PositionedGraph` without groups
    fn layout_graph_without_groups() -> relune_layout::PositionedGraph {
        relune_layout::PositionedGraph {
            nodes: vec![PositionedNode {
                id: "users".to_string(),
                label: "users".to_string(),
                kind: NodeKind::Table,
                columns: vec![PositionedColumn {
                    name: "id".to_string(),
                    data_type: "uuid".to_string(),
                    nullable: false,
                    is_primary_key: true,
                }],
                x: 56.0,
                y: 56.0,
                width: 260.0,
                height: 76.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            }],
            edges: vec![],
            groups: vec![],
            width: 432.0,
            height: 188.0,
        }
    }

    #[test]
    fn test_render_svg_basic() {
        let graph = layout_graph_without_groups();
        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Should contain valid SVG structure
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("xmlns=\"http://www.w3.org/2000/svg\""));
        // Should contain the node
        assert!(svg.contains(">users<"));
        assert!(svg.contains("data-table-id=\"users\""));
    }

    #[test]
    fn test_render_svg_with_groups() {
        let graph = layout_graph_with_groups();
        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Should contain group elements
        assert!(svg.contains("class=\"group-box\""));
        assert!(svg.contains("data-group-id=\"user_domain\""));
        assert!(svg.contains("class=\"group-label\""));
        assert!(svg.contains(">User Domain<"));
        // Should contain dashed stroke for group
        assert!(svg.contains("stroke-dasharray=\"10,5\""));
        // Should contain semi-transparent fill
        assert!(svg.contains("fill-opacity=\"0.48\""));
    }

    #[test]
    fn test_render_svg_groups_behind_nodes() {
        let graph = layout_graph_with_groups();
        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Find positions of group and node elements
        let group_pos = svg.find("class=\"group-box\"").expect("Group should exist");
        let node_pos = svg
            .find("data-table-id=\"users\"")
            .expect("Node should exist");

        // Groups should appear before nodes (rendered first, behind nodes)
        assert!(
            group_pos < node_pos,
            "Groups should be rendered before nodes"
        );
    }

    #[test]
    fn test_render_svg_with_edges() {
        let graph = layout_graph_with_groups();
        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Should contain edge elements
        assert!(svg.contains("class=\"edge-path\""));
        assert!(svg.contains("data-from=\"posts\""));
        assert!(svg.contains("data-to=\"users\""));
        assert!(svg.contains("class=\"edge-label\""));
        assert!(svg.contains(">user_id<"));
    }

    #[test]
    fn test_render_svg_deterministic_with_groups() {
        let graph = layout_graph_with_groups();

        let svg1 = render_svg(&graph, SvgRenderOptions::default());
        let svg2 = render_svg(&graph, SvgRenderOptions::default());
        let svg3 = render_svg(&graph, SvgRenderOptions::default());

        // All outputs should be identical
        assert_eq!(svg1, svg2);
        assert_eq!(svg2, svg3);
    }

    #[test]
    fn test_render_svg_with_dark_theme_groups() {
        let graph = layout_graph_with_groups();
        let options = SvgRenderOptions {
            theme: Theme::Dark,
            ..Default::default()
        };
        let svg = render_svg(&graph, options);

        // Dark theme background
        assert!(svg.contains("#0f172a"));
        // Dark theme node fill
        assert!(svg.contains("#151926"));
    }

    #[test]
    fn test_render_svg_with_light_theme_groups() {
        let graph = layout_graph_with_groups();
        let options = SvgRenderOptions {
            theme: Theme::Light,
            ..Default::default()
        };
        let svg = render_svg(&graph, options);

        // Light theme background
        assert!(svg.contains("#ffffff"));
        // Light theme node fill
        assert!(svg.contains("#fffaf0"));
    }

    #[test]
    fn test_render_svg_rounded_corners() {
        let graph = layout_graph_with_groups();
        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Group box should have rounded corners
        assert!(svg.contains("rx=\"16\""));
        assert!(svg.contains("ry=\"16\""));
    }

    #[test]
    fn test_render_svg_escapes_special_characters_in_groups() {
        let graph = relune_layout::PositionedGraph {
            nodes: vec![PositionedNode {
                id: "test & table".to_string(),
                label: "Test & <Table>".to_string(),
                kind: NodeKind::Table,
                columns: vec![PositionedColumn {
                    name: "col \"name\"".to_string(),
                    data_type: "text".to_string(),
                    nullable: false,
                    is_primary_key: false,
                }],
                x: 56.0,
                y: 56.0,
                width: 260.0,
                height: 76.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            }],
            edges: vec![],
            groups: vec![PositionedGroup {
                id: "group & 1".to_string(),
                label: "Group & <Test>".to_string(),
                x: 36.0,
                y: 36.0,
                width: 300.0,
                height: 100.0,
            }],
            width: 400.0,
            height: 200.0,
        };

        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Escaped characters should appear in the output
        assert!(svg.contains("&amp;"));
        assert!(svg.contains("&lt;"));
        assert!(svg.contains("&gt;"));
        assert!(svg.contains("&quot;"));
        // Raw special characters should not appear (except in SVG syntax)
        assert!(!svg.contains("Test & <Table>"));
        assert!(!svg.contains("Group & <Test>"));
    }
}

// === Snapshot tests for fixtures ===

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use relune_layout::build_layout;
    use relune_parser_sql::{ParseOutput, parse_sql_to_schema_with_diagnostics};

    /// Helper to read fixture file content
    fn read_fixture(name: &str) -> &'static str {
        // Use include_str! to embed fixture files at compile time
        match name {
            "simple_blog.sql" => include_str!("../../../fixtures/sql/simple_blog.sql"),
            "ecommerce.sql" => include_str!("../../../fixtures/sql/ecommerce.sql"),
            "multi_schema.sql" => include_str!("../../../fixtures/sql/multi_schema.sql"),
            "broken_input.sql" => include_str!("../../../fixtures/sql/broken_input.sql"),
            "cyclic_fk.sql" => include_str!("../../../fixtures/sql/cyclic_fk.sql"),
            "join_heavy.sql" => include_str!("../../../fixtures/sql/join_heavy.sql"),
            _ => panic!("Unknown fixture: {name}"),
        }
    }

    /// Parse SQL once, then render SVG when schema and layout succeed.
    fn parse_and_render_fixture_svg(sql: &str) -> (ParseOutput, Option<String>) {
        let parse_output = parse_sql_to_schema_with_diagnostics(sql);
        let svg = parse_output
            .schema
            .as_ref()
            .and_then(|schema| build_layout(schema).ok())
            .map(|positioned_graph| render_svg(&positioned_graph, SvgRenderOptions::default()));
        (parse_output, svg)
    }

    /// Helper to process SQL fixture and generate SVG
    fn process_fixture_to_svg(sql: &str) -> Option<String> {
        parse_and_render_fixture_svg(sql).1
    }

    #[test]
    fn test_snapshot_simple_blog() {
        let sql = read_fixture("simple_blog.sql");
        let svg = process_fixture_to_svg(sql).expect("Failed to process simple_blog.sql");

        insta::with_settings!({
            snapshot_path => "snapshots",
            prepend_module_to_snapshot => false,
        }, {
            insta::assert_snapshot!("simple_blog", svg);
        });
    }

    #[test]
    fn test_snapshot_ecommerce() {
        let sql = read_fixture("ecommerce.sql");
        let svg = process_fixture_to_svg(sql).expect("Failed to process ecommerce.sql");

        insta::with_settings!({
            snapshot_path => "snapshots",
            prepend_module_to_snapshot => false,
        }, {
            insta::assert_snapshot!("ecommerce", svg);
        });
    }

    #[test]
    fn test_snapshot_multi_schema() {
        let sql = read_fixture("multi_schema.sql");
        let svg = process_fixture_to_svg(sql).expect("Failed to process multi_schema.sql");

        insta::with_settings!({
            snapshot_path => "snapshots",
            prepend_module_to_snapshot => false,
        }, {
            insta::assert_snapshot!("multi_schema", svg);
        });
    }

    #[test]
    fn test_snapshot_broken_input() {
        let sql = read_fixture("broken_input.sql");
        let (parse_output, svg) = parse_and_render_fixture_svg(sql);

        let snapshot_data = serde_json::json!({
            "svg": svg,
            "diagnostics": parse_output.diagnostics.iter().map(|d| serde_json::json!({
                "severity": format!("{}", d.severity),
                "code": d.code.full_code(),
                "message": d.message,
            })).collect::<Vec<_>>(),
        });

        insta::with_settings!({
            snapshot_path => "snapshots",
            prepend_module_to_snapshot => false,
        }, {
            insta::assert_json_snapshot!("broken_input", snapshot_data);
        });
    }

    #[test]
    fn test_snapshot_cyclic_fk() {
        let sql = read_fixture("cyclic_fk.sql");
        let svg = process_fixture_to_svg(sql).expect("Failed to process cyclic_fk.sql");

        insta::with_settings!({
            snapshot_path => "snapshots",
            prepend_module_to_snapshot => false,
        }, {
            insta::assert_snapshot!("cyclic_fk", svg);
        });
    }

    #[test]
    fn test_snapshot_join_heavy() {
        let sql = read_fixture("join_heavy.sql");
        let svg = process_fixture_to_svg(sql).expect("Failed to process join_heavy.sql");

        insta::with_settings!({
            snapshot_path => "snapshots",
            prepend_module_to_snapshot => false,
        }, {
            insta::assert_snapshot!("join_heavy", svg);
        });
    }
}
