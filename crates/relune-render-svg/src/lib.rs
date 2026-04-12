//! SVG rendering for database schema diagrams.
//!
//! This crate provides SVG rendering functionality for visualizing database schemas
//! as interactive diagrams.

use std::fmt::{self, Write};

use relune_core::{EdgeKind, layout::RouteStyle};

pub mod edge;
mod error;
pub mod escape;
pub mod geometry;
pub mod group;
pub mod legend;
mod markers;
mod node;
mod options;
mod theme;

pub use edge::{EdgeRenderOptions, render_edge};
pub use error::SvgRenderError;
pub use geometry::{Point, Rect, clamp, compute_column_y, compute_node_height, lerp};
pub use group::{render_group, render_group_background, render_group_label};
pub use legend::render_legend;
pub use options::SvgRenderOptions;
pub use theme::{Theme, ThemeColors, get_colors};

/// Renders a positioned graph to an SVG string with the given options.
///
/// Supports group rendering for visually grouping related tables.
/// When an overlay is provided, annotations (lint warnings, diff status, etc.)
/// are attached to the corresponding nodes and edges.
pub fn render_svg(
    graph: &relune_layout::PositionedGraph,
    options: SvgRenderOptions,
) -> Result<String, SvgRenderError> {
    render_svg_with_overlay(graph, options, None)
}

/// Renders a positioned graph to an SVG string with an optional overlay.
///
/// This is the full-featured entry point. `render_svg` delegates here with
/// `overlay = None` for backwards compatibility.
pub fn render_svg_with_overlay(
    graph: &relune_layout::PositionedGraph,
    options: SvgRenderOptions,
    overlay: Option<&relune_layout::DiagramOverlay>,
) -> Result<String, SvgRenderError> {
    let colors = get_colors(options.theme);
    let mut out = String::with_capacity(estimate_svg_capacity(graph, options, overlay));

    write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{:.0}" height="{:.0}" viewBox="0 0 {:.0} {:.0}" fill="none">"#,
        graph.width, graph.height, graph.width, graph.height
    )?;
    out_push_defs(&mut out, &colors)?;
    write!(
        out,
        r#"<rect width="100%" height="100%" fill="{}"/>"#,
        colors.background
    )?;
    out.push_str(r#"<rect width="100%" height="100%" fill="url(#canvas-grid)" opacity="0.92"/>"#);

    // Render group shells behind nodes and edges.
    for group in &graph.groups {
        render_group_background(&mut out, group, &colors)?;
    }

    // Render edges with enhanced options
    let edge_options = EdgeRenderOptions {
        stroke_width: 2.0,
        show_tooltips: options.show_tooltips,
        ..EdgeRenderOptions::default()
    };
    for (index, edge) in graph.edges.iter().enumerate() {
        let edge_overlay = overlay.and_then(|o| o.edge(&edge.from, &edge.to));
        render_edge_internal(&mut out, edge, &colors, &edge_options, index, edge_overlay)?;
    }

    // Render nodes
    for (index, node) in graph.nodes.iter().enumerate() {
        let node_overlay = overlay.and_then(|o| o.node(&node.id));
        node::render_node_internal(
            &mut out,
            node,
            &colors,
            options.show_tooltips,
            index,
            node_overlay,
        )?;
    }

    // Render group labels after nodes so schema titles remain readable even
    // when force-directed groups pack tightly.
    for group in &graph.groups {
        render_group_label(&mut out, group, &colors)?;
    }

    // Render legend if requested
    if options.show_legend {
        render_legend(&mut out, &colors, None, graph.width, graph.height)?;
    }

    out.push_str("</svg>");
    Ok(out)
}

fn estimate_svg_capacity(
    graph: &relune_layout::PositionedGraph,
    options: SvgRenderOptions,
    overlay: Option<&relune_layout::DiagramOverlay>,
) -> usize {
    let base = 4_096usize;
    let group_bytes: usize = graph
        .groups
        .iter()
        .map(|group| 256 + group.id.len() * 4 + group.label.len() * 6)
        .sum();
    let edge_bytes: usize = graph
        .edges
        .iter()
        .map(|edge| {
            let route_bytes = edge.route.control_points.len() * 48;
            let label_bytes = edge.label.len() * 12;
            let endpoint_bytes = (edge.from.len() + edge.to.len()) * 6;
            let column_bytes = edge
                .from_columns
                .iter()
                .chain(&edge.to_columns)
                .map(|column| 24 + column.len() * 10)
                .sum::<usize>();
            let tooltip_bytes = if options.show_tooltips { 256 } else { 0 };
            let overlay_bytes = overlay
                .and_then(|item| item.edge(&edge.from, &edge.to))
                .map_or(0, estimate_edge_overlay_bytes);
            768 + route_bytes
                + label_bytes
                + endpoint_bytes
                + column_bytes
                + tooltip_bytes
                + overlay_bytes
        })
        .sum();
    let node_bytes: usize = graph
        .nodes
        .iter()
        .map(|node| {
            let label_bytes = node.id.len() * 6 + node.label.len() * 12;
            let column_bytes = node
                .columns
                .iter()
                .map(|column| 320 + (column.name.len() + column.data_type.len()) * 12)
                .sum::<usize>();
            let tooltip_bytes = if options.show_tooltips {
                192 + node.columns.len() * 48
            } else {
                0
            };
            let overlay_bytes = overlay
                .and_then(|item| item.node(&node.id))
                .map_or(0, estimate_node_overlay_bytes);
            1_024 + label_bytes + column_bytes + tooltip_bytes + overlay_bytes
        })
        .sum();
    let legend_bytes = if options.show_legend { 1_024 } else { 0 };

    base + group_bytes + edge_bytes + node_bytes + legend_bytes
}

fn estimate_node_overlay_bytes(overlay: &relune_layout::NodeOverlay) -> usize {
    estimate_annotations_bytes(&overlay.annotations)
}

fn estimate_edge_overlay_bytes(overlay: &relune_layout::EdgeOverlay) -> usize {
    estimate_annotations_bytes(&overlay.annotations)
}

fn estimate_annotations_bytes(annotations: &[relune_layout::Annotation]) -> usize {
    annotations
        .iter()
        .map(|annotation| {
            let hint_bytes = annotation.hint.as_ref().map_or(0, |hint| hint.len() * 10);
            let rule_id_bytes = annotation
                .rule_id
                .as_ref()
                .map_or(0, |rule_id| rule_id.len() * 6);
            192 + annotation.message.len() * 12 + hint_bytes + rule_id_bytes
        })
        .sum()
}

fn out_push_defs(out: &mut String, colors: &ThemeColors) -> fmt::Result {
    let (shadow_dy, shadow_blur, hatch_color) = if is_light_theme(colors) {
        ("2", "5", "#cbd5e1")
    } else {
        ("4", "8", "#334155")
    };

    write!(
        out,
        r"<defs>
<style>
.edge-glow-path,
.edge-particles {{ opacity: 0; pointer-events: none; transition: opacity 0.18s ease; }}
.edge-particle {{ fill: {glow_particle}; }}
.edge:hover .edge-glow-path,
.edge:hover .edge-particles {{ opacity: 0.92; }}
.edge:hover .edge-path,
.edge:hover .crow-inline {{ stroke: {glow_color}; }}
.node:hover .table-body {{ stroke-width: 2.1px; }}
.group-box,
.group-band,
.group-divider,
.group-label {{ pointer-events: none; }}
</style>",
        glow_color = colors.glow_color,
        glow_particle = colors.glow_particle,
    )?;
    write!(
        out,
        r#"
<pattern id="canvas-grid" width="32" height="32" patternUnits="userSpaceOnUse">
<rect width="32" height="32" fill="{}"/>
<circle cx="2" cy="2" r="1.2" fill="{}" fill-opacity="0.9"/>
<circle cx="18" cy="18" r="0.8" fill="{}" fill-opacity="0.54"/>
</pattern>
<pattern id="type-filter-hatch" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="rotate(35)">
<rect width="8" height="8" fill="transparent"/>
<rect width="3" height="8" fill="{hatch_color}" fill-opacity="0.42"/>
</pattern>
<filter id="node-shadow" x="-20%" y="-20%" width="140%" height="150%">
<feDropShadow dx="0" dy="{shadow_dy}" stdDeviation="{shadow_blur}" flood-color="{}"/>
</filter>
<filter id="group-shadow" x="-20%" y="-20%" width="140%" height="160%">
<feDropShadow dx="0" dy="8" stdDeviation="12" flood-color="{}" flood-opacity="0.16"/>
</filter>
<filter id="edge-glow" x="-50%" y="-50%" width="200%" height="200%">
<feDropShadow dx="0" dy="0" stdDeviation="5" flood-color="{}"/>
</filter>
<marker id="arrow" markerWidth="10" markerHeight="10" refX="9" refY="5" orient="auto" markerUnits="userSpaceOnUse">
<path d="M1,1 L9,5 L1,9" fill="none" stroke="context-stroke" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" shape-rendering="geometricPrecision"/>
</marker>
<marker id="cardinality-one" markerWidth="18" markerHeight="18" refX="16" refY="9" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
<path d="M10 2 L10 16" stroke="context-stroke" stroke-width="1.5" stroke-linecap="round" shape-rendering="geometricPrecision"/>
</marker>
<marker id="cardinality-many" markerWidth="18" markerHeight="18" refX="16" refY="9" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
<path d="M2 2 L16 9 M2 9 L16 9 M2 16 L16 9" stroke="context-stroke" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" shape-rendering="geometricPrecision"/>
</marker>
<marker id="cardinality-zero-many" markerWidth="26" markerHeight="18" refX="23" refY="9" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
<circle cx="4" cy="9" r="3.4" fill="none" stroke="context-stroke" stroke-width="1.2" shape-rendering="geometricPrecision"/>
<path d="M10 2 L23 9 M10 9 L23 9 M10 16 L23 9" stroke="context-stroke" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" shape-rendering="geometricPrecision"/>
</marker>
<marker id="cardinality-zero-one" markerWidth="26" markerHeight="18" refX="23" refY="9" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
<circle cx="4" cy="9" r="3.4" fill="none" stroke="context-stroke" stroke-width="1.2" shape-rendering="geometricPrecision"/>
<path d="M18 2 L18 16" stroke="context-stroke" stroke-width="1.5" stroke-linecap="round" shape-rendering="geometricPrecision"/>
</marker>
<marker id="cardinality-one-many" markerWidth="26" markerHeight="18" refX="23" refY="9" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
<path d="M2 2 L2 16" stroke="context-stroke" stroke-width="1.5" stroke-linecap="round" shape-rendering="geometricPrecision"/>
<path d="M10 2 L23 9 M10 9 L23 9 M10 16 L23 9" stroke="context-stroke" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" shape-rendering="geometricPrecision"/>
</marker>
</defs>"#,
        colors.canvas_base,
        colors.canvas_dot,
        colors.canvas_dot,
        colors.node_shadow,
        colors.node_shadow,
        colors.glow_color
    )?;
    Ok(())
}

/// Edge rendering for relune-layout `PositionedEdge`.
#[allow(clippy::cast_precision_loss)] // Entry animation indices are presentation-only.
#[allow(clippy::suboptimal_flops)] // Render-time coordinate math favors readability here.
#[allow(clippy::too_many_lines)] // SVG edge markup with overlay integration is clearer in one block.
fn render_edge_internal(
    out: &mut String,
    edge: &relune_layout::PositionedEdge,
    colors: &ThemeColors,
    options: &EdgeRenderOptions,
    index: usize,
    overlay: Option<&relune_layout::EdgeOverlay>,
) -> fmt::Result {
    let kind = edge_kind_name(edge.kind);
    let edge_style = edge_style(edge.kind, colors);
    let path_d = crate::edge::edge_route_svg_path_d(&edge.route, options.curve_offset);
    let uses_crow_markers = edge.kind == EdgeKind::ForeignKey;
    // Curved FK edges use inline markers drawn against the rendered backbone,
    // because SVG <marker> elements follow the endpoint tangent and can look
    // detached once the visible path is smoothed.
    let use_inline_markers =
        uses_crow_markers && (edge.is_self_loop || edge.route.style == RouteStyle::Curved);
    let max_severity = overlay.and_then(relune_layout::EdgeOverlay::max_severity);

    let stroke_dasharray = if options.dashed {
        Some("5,3")
    } else {
        edge_style.dasharray
    };

    // Add overlay severity CSS class if present
    let severity_class = match max_severity {
        Some(relune_layout::OverlaySeverity::Error) => " overlay-error",
        Some(relune_layout::OverlaySeverity::Warning) => " overlay-warning",
        Some(relune_layout::OverlaySeverity::Info) => " overlay-info",
        Some(relune_layout::OverlaySeverity::Hint) => " overlay-hint",
        None => "",
    };

    write!(
        out,
        r#"<g class="edge edge-kind-{}{}" data-from="{}" data-to="{}" data-edge-kind="{}" style="--enter-delay:{:.3}s">"#,
        kind,
        severity_class,
        escape_attribute(&edge.from),
        escape_attribute(&edge.to),
        kind,
        index as f32 * 0.016 + 0.04
    )?;

    // Add tooltip if enabled (with overlay annotations appended)
    if options.show_tooltips {
        let mut tooltip_text = generate_edge_tooltip(edge);
        if let Some(edge_overlay) = overlay
            && !edge_overlay.annotations.is_empty()
        {
            tooltip_text.push('\n');
            for annotation in &edge_overlay.annotations {
                let severity_label = overlay_severity_label(annotation.severity);
                write!(tooltip_text, "\n[{severity_label}] {}", annotation.message)?;
                if let Some(ref hint) = annotation.hint {
                    write!(tooltip_text, "\n  → {hint}")?;
                }
            }
        }
        write!(out, r"<title>{}</title>", escape_text(&tooltip_text))?;
    }

    // Override stroke color when overlay severity is present
    let effective_stroke = match max_severity {
        Some(severity) => overlay_severity_color(severity, colors),
        None => edge_style.stroke,
    };

    // Render the path with CSS class and data attributes
    let glow = colors.glow_color;
    let marker_attrs = if use_inline_markers {
        "" // Inline markers are drawn separately below.
    } else {
        edge_marker_attributes(uses_crow_markers, edge.nullable, edge.target_cardinality)
    };
    match stroke_dasharray {
        Some(stroke_dasharray) => {
            write!(
                out,
                r#"<path class="edge-glow-path" d="{}" stroke="{glow}" stroke-width="{:.1}" fill="none" stroke-linecap="round" stroke-linejoin="round" opacity="0" filter="url(#edge-glow)"/><path id="edge-path-{}" class="edge-path" d="{}" stroke="{}" stroke-width="{:.1}" fill="none" stroke-linecap="round" stroke-linejoin="round" shape-rendering="geometricPrecision"{} stroke-dasharray="{}" pathLength="100"/>"#,
                escape_attribute(&path_d),
                options.stroke_width + 2.0,
                index,
                escape_attribute(&path_d),
                effective_stroke,
                options.stroke_width,
                marker_attrs,
                stroke_dasharray
            )?;
        }
        None => {
            write!(
                out,
                r#"<path class="edge-glow-path" d="{}" stroke="{glow}" stroke-width="{:.1}" fill="none" stroke-linecap="round" stroke-linejoin="round" opacity="0" filter="url(#edge-glow)"/><path id="edge-path-{}" class="edge-path" d="{}" stroke="{}" stroke-width="{:.1}" fill="none" stroke-linecap="round" stroke-linejoin="round" shape-rendering="geometricPrecision"{} pathLength="100" />"#,
                escape_attribute(&path_d),
                options.stroke_width + 2.0,
                index,
                escape_attribute(&path_d),
                effective_stroke,
                options.stroke_width,
                marker_attrs,
            )?;
        }
    }

    if edge.kind == EdgeKind::ForeignKey {
        out.push_str(r#"<g class="edge-particles" opacity="0">"#);
        write!(
            out,
            r##"<circle class="edge-particle" r="2.4"><animateMotion dur="2.6s" repeatCount="indefinite" rotate="auto"><mpath href="#edge-path-{index}"/></animateMotion></circle><circle class="edge-particle" r="1.8" opacity="0.72"><animateMotion dur="2.6s" begin="-1.3s" repeatCount="indefinite" rotate="auto"><mpath href="#edge-path-{index}"/></animateMotion></circle>"##
        )?;
        out.push_str("</g>");
    }

    // For curved FK edges, draw Crow's Foot symbols as inline SVG
    // elements positioned along the actual curve path.
    if use_inline_markers {
        markers::render_inline_crow_markers(
            out,
            &edge.route,
            edge.nullable,
            edge.target_cardinality,
            effective_stroke,
        )?;
    }

    // Render the label if enabled
    if options.show_labels && !edge.label.is_empty() {
        let label_x = edge.label_x;
        let label_y = edge.label_y;
        let label_width = estimate_label_width(&edge.label);
        write!(
            out,
            r#"<rect class="edge-label-pill" x="{:.1}" y="{:.1}" width="{:.1}" height="18" rx="9" ry="9" fill="{}" fill-opacity="0.92" stroke="{}" stroke-opacity="0.65"/>"#,
            label_width.mul_add(-0.5, label_x),
            label_y - 12.0,
            label_width,
            node_label_background(colors),
            edge_style.stroke
        )?;

        write!(
            out,
            r#"<text class="edge-label" x="{:.1}" y="{:.1}" font-family="'Inter', 'Segoe UI', system-ui, sans-serif" font-size="11" font-weight="600" text-anchor="middle" fill="{}">{}</text>"#,
            label_x,
            label_y,
            edge_style.label_fill.unwrap_or(colors.text_muted),
            escape_text(&edge.label)
        )?;
    }

    out.push_str("</g>");
    Ok(())
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

struct EdgeStyle {
    stroke: &'static str,
    dasharray: Option<&'static str>,
    label_fill: Option<&'static str>,
}

pub(crate) fn is_light_theme(colors: &ThemeColors) -> bool {
    colors.background == "#f7f8fc"
}

/// Returns a display-friendly label for an overlay severity level.
pub(crate) const fn overlay_severity_label(
    severity: relune_layout::OverlaySeverity,
) -> &'static str {
    match severity {
        relune_layout::OverlaySeverity::Error => "error",
        relune_layout::OverlaySeverity::Warning => "warning",
        relune_layout::OverlaySeverity::Info => "info",
        relune_layout::OverlaySeverity::Hint => "hint",
    }
}

/// Returns the stroke/fill color for an overlay severity, themed for light/dark.
pub(crate) fn overlay_severity_color(
    severity: relune_layout::OverlaySeverity,
    colors: &ThemeColors,
) -> &'static str {
    let light = is_light_theme(colors);
    match (severity, light) {
        (relune_layout::OverlaySeverity::Error, false) => "#f87171",
        (relune_layout::OverlaySeverity::Error, true) => "#dc2626",
        (relune_layout::OverlaySeverity::Warning, false) => "#fbbf24",
        (relune_layout::OverlaySeverity::Warning, true) => "#d97706",
        (relune_layout::OverlaySeverity::Info, false) => "#38bdf8",
        (relune_layout::OverlaySeverity::Info, true) => "#0284c7",
        (relune_layout::OverlaySeverity::Hint, false) => "#94a3b8",
        (relune_layout::OverlaySeverity::Hint, true) => "#64748b",
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

use escape::{escape_attribute, escape_text};
use markers::edge_marker_attributes;
use node::{estimate_label_width, node_label_background};

#[cfg(test)]
mod tests {
    use super::*;
    use relune_core::layout::Cardinality;
    use relune_core::{EdgeKind, NodeKind};
    use relune_layout::{
        ColumnFlags, ColumnRelationFlags, EdgeRoute, PositionedColumn, PositionedEdge,
        PositionedGroup, PositionedNode, RouteStyle,
    };

    fn render_svg(graph: &relune_layout::PositionedGraph, options: SvgRenderOptions) -> String {
        super::render_svg(graph, options).expect("svg rendering should succeed in tests")
    }

    fn render_svg_with_overlay(
        graph: &relune_layout::PositionedGraph,
        options: SvgRenderOptions,
        overlay: Option<&relune_layout::DiagramOverlay>,
    ) -> String {
        super::render_svg_with_overlay(graph, options, overlay)
            .expect("svg overlay rendering should succeed in tests")
    }

    fn relation_flags(
        is_primary_key: bool,
        is_foreign_key: bool,
        is_indexed: bool,
    ) -> ColumnRelationFlags {
        ColumnRelationFlags {
            is_primary_key,
            is_foreign_key,
            is_indexed,
        }
    }

    fn column(
        name: &str,
        data_type: &str,
        nullable: bool,
        relation: ColumnRelationFlags,
    ) -> PositionedColumn {
        PositionedColumn {
            name: name.to_string(),
            data_type: data_type.to_string(),
            flags: ColumnFlags { nullable, relation },
        }
    }

    /// Helper function to create a test `PositionedGraph` with empty state
    fn empty_graph() -> relune_layout::PositionedGraph {
        relune_layout::PositionedGraph {
            nodes: vec![],
            edges: vec![],
            groups: vec![],
            width: 800.0,
            height: 600.0,
            routing_debug: None,
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
                    column("id", "uuid PK", false, relation_flags(true, false, false)),
                    column("name", "text", false, relation_flags(false, false, false)),
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
            routing_debug: None,
        }
    }

    /// Helper function to create a test `PositionedGraph` with multiple nodes and edges
    #[allow(clippy::too_many_lines)] // Test fixture keeps the end-to-end render assertions readable.
    fn multi_node_graph() -> relune_layout::PositionedGraph {
        relune_layout::PositionedGraph {
            nodes: vec![
                PositionedNode {
                    id: "users".to_string(),
                    label: "users".to_string(),
                    kind: NodeKind::Table,
                    columns: vec![
                        column("id", "uuid PK", false, relation_flags(true, false, false)),
                        column("name", "text", false, relation_flags(false, false, false)),
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
                        column("id", "uuid PK", false, relation_flags(true, false, false)),
                        column("user_id", "uuid", false, relation_flags(false, true, true)),
                        column("title", "text", false, relation_flags(false, false, false)),
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
                target_cardinality: Cardinality::One,
                from_columns: vec!["user_id".to_string()],
                to_columns: vec!["id".to_string()],
                is_collapsed_join: false,
                collapsed_join_table: None,
                label_x: 186.0,
                label_y: 194.5,
                routing_debug: None,
            }],
            groups: vec![],
            width: 432.0,
            height: 398.0,
            routing_debug: None,
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
        assert!(svg.contains("node-0-header-clip"));
        assert!(svg.contains("node-0-column-0-clip"));
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
                    columns: vec![column(
                        "status",
                        "status",
                        false,
                        relation_flags(false, false, false),
                    )],
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
                    columns: vec![column(
                        "id",
                        "int",
                        false,
                        relation_flags(false, false, false),
                    )],
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
                        column("active", "", false, relation_flags(false, false, false)),
                        column("inactive", "", false, relation_flags(false, false, false)),
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
                    target_cardinality: Cardinality::One,
                    from_columns: vec![],
                    to_columns: vec![],
                    is_collapsed_join: false,
                    collapsed_join_table: None,
                    label_x: 356.0,
                    label_y: 94.0,
                    routing_debug: None,
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
                    target_cardinality: Cardinality::One,
                    from_columns: vec!["status".to_string()],
                    to_columns: vec![],
                    is_collapsed_join: false,
                    collapsed_join_table: None,
                    label_x: 356.0,
                    label_y: 187.5,
                    routing_debug: None,
                },
            ],
            groups: vec![],
            width: 712.0,
            height: 366.0,
            routing_debug: None,
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
                columns: vec![column(
                    "col \"name\"",
                    "text",
                    false,
                    relation_flags(false, false, false),
                )],
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
                target_cardinality: Cardinality::One,
                from_columns: vec!["test_id".to_string()],
                to_columns: vec!["id".to_string()],
                is_collapsed_join: false,
                collapsed_join_table: None,
                label_x: 358.0,
                label_y: 100.0,
                routing_debug: None,
            }],
            groups: vec![],
            width: 500.0,
            height: 200.0,
            routing_debug: None,
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
                columns: vec![column(
                    payload,
                    "text",
                    false,
                    relation_flags(false, false, false),
                )],
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
            routing_debug: None,
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
        assert!(svg.contains("#0c0f1a"));
        // Dark theme node fill
        assert!(svg.contains("#151926"));
        assert!(svg.contains("edge-particles"));
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
        assert!(svg.contains("#f7f8fc"));
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
            routing_debug: None,
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
        assert!(svg.contains("LEGEND"));
        assert!(svg.contains("Primary key"));
        assert!(svg.contains("Foreign key"));
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
                target_cardinality: Cardinality::One,
                from_columns: vec!["user_id".to_string()],
                to_columns: vec!["id".to_string()],
                is_collapsed_join: false,
                collapsed_join_table: None,
                label_x: 200.0,
                label_y: 100.0,
                routing_debug: None,
            }],
            groups: vec![],
            width: 400.0,
            height: 200.0,
            routing_debug: None,
        };

        let svg = render_svg(&graph, SvgRenderOptions::default());

        // Should contain edge CSS classes and data attributes
        assert!(svg.contains("class=\"edge-path\""));
        assert!(svg.contains("data-from=\"users\""));
        assert!(svg.contains("data-to=\"posts\""));
        assert!(svg.contains("class=\"edge-label\""));
    }

    /// Helper to build a single-edge graph for marker tests.
    fn marker_test_graph(
        nullable: bool,
        target_cardinality: Cardinality,
    ) -> relune_layout::PositionedGraph {
        relune_layout::PositionedGraph {
            nodes: vec![],
            edges: vec![PositionedEdge {
                from: "child".to_string(),
                to: "parent".to_string(),
                label: "fk".to_string(),
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
                nullable,
                target_cardinality,
                from_columns: vec!["fk_col".to_string()],
                to_columns: vec!["id".to_string()],
                is_collapsed_join: false,
                collapsed_join_table: None,
                label_x: 200.0,
                label_y: 100.0,
                routing_debug: None,
            }],
            groups: vec![],
            width: 400.0,
            height: 200.0,
            routing_debug: None,
        }
    }

    fn self_loop_marker_test_graph(
        nullable: bool,
        target_cardinality: Cardinality,
    ) -> relune_layout::PositionedGraph {
        relune_layout::PositionedGraph {
            nodes: vec![],
            edges: vec![PositionedEdge {
                from: "users".to_string(),
                to: "users".to_string(),
                label: "manager_id".to_string(),
                kind: EdgeKind::ForeignKey,
                route: EdgeRoute {
                    x1: 100.0,
                    y1: 60.0,
                    x2: 100.0,
                    y2: 140.0,
                    control_points: vec![(170.0, 60.0), (170.0, 140.0)],
                    style: RouteStyle::Curved,
                    label_position: (170.0, 100.0),
                },
                is_self_loop: true,
                nullable,
                target_cardinality,
                from_columns: vec!["manager_id".to_string()],
                to_columns: vec!["id".to_string()],
                is_collapsed_join: false,
                collapsed_join_table: None,
                label_x: 170.0,
                label_y: 100.0,
                routing_debug: None,
            }],
            groups: vec![],
            width: 260.0,
            height: 220.0,
            routing_debug: None,
        }
    }

    #[test]
    fn test_marker_required_fk_one_target() {
        let svg = render_svg(
            &marker_test_graph(false, Cardinality::One),
            SvgRenderOptions::default(),
        );
        assert!(svg.contains("marker-start=\"url(#cardinality-one-many)\""));
        assert!(svg.contains("marker-end=\"url(#cardinality-one)\""));
    }

    #[test]
    fn test_marker_required_fk_many_target() {
        let svg = render_svg(
            &marker_test_graph(false, Cardinality::Many),
            SvgRenderOptions::default(),
        );
        assert!(svg.contains("marker-start=\"url(#cardinality-one-many)\""));
        assert!(svg.contains("marker-end=\"url(#cardinality-many)\""));
    }

    #[test]
    fn test_marker_required_fk_zero_or_one_target() {
        let svg = render_svg(
            &marker_test_graph(false, Cardinality::ZeroOrOne),
            SvgRenderOptions::default(),
        );
        assert!(svg.contains("marker-start=\"url(#cardinality-one-many)\""));
        assert!(svg.contains("marker-end=\"url(#cardinality-zero-one)\""));
    }

    #[test]
    fn test_marker_nullable_fk_one_target() {
        let svg = render_svg(
            &marker_test_graph(true, Cardinality::One),
            SvgRenderOptions::default(),
        );
        assert!(svg.contains("marker-start=\"url(#cardinality-zero-many)\""));
        assert!(svg.contains("marker-end=\"url(#cardinality-one)\""));
    }

    #[test]
    fn test_marker_nullable_fk_many_target() {
        let svg = render_svg(
            &marker_test_graph(true, Cardinality::Many),
            SvgRenderOptions::default(),
        );
        assert!(svg.contains("marker-start=\"url(#cardinality-zero-many)\""));
        assert!(svg.contains("marker-end=\"url(#cardinality-many)\""));
    }

    #[test]
    fn test_marker_nullable_fk_zero_or_one_target() {
        let svg = render_svg(
            &marker_test_graph(true, Cardinality::ZeroOrOne),
            SvgRenderOptions::default(),
        );
        assert!(svg.contains("marker-start=\"url(#cardinality-zero-many)\""));
        assert!(svg.contains("marker-end=\"url(#cardinality-zero-one)\""));
    }

    #[test]
    fn test_marker_defs_inherit_parent_edge_stroke() {
        let svg = render_svg(
            &marker_test_graph(false, Cardinality::Many),
            SvgRenderOptions::default(),
        );
        assert!(svg.contains(r#"stroke="context-stroke""#));
        assert!(svg.contains(r#"shape-rendering="geometricPrecision""#));
    }

    #[test]
    fn test_self_loop_inline_markers_use_edge_stroke() {
        let svg = render_svg(
            &self_loop_marker_test_graph(false, Cardinality::Many),
            SvgRenderOptions::default(),
        );
        assert!(!svg.contains("marker-start=\"url(#cardinality-one-many)\""));
        assert!(svg.contains(
            r##"stroke="#475569" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none" shape-rendering="geometricPrecision""##,
        ));
        assert!(!svg.contains(
            r##"stroke="#cbd5e1" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none" shape-rendering="geometricPrecision""##,
        ));
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
                        column("id", "uuid", false, relation_flags(true, false, false)),
                        column("name", "text", false, relation_flags(false, false, false)),
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
                        column("id", "uuid", false, relation_flags(true, false, false)),
                        column("user_id", "uuid", false, relation_flags(false, true, true)),
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
                target_cardinality: Cardinality::One,
                from_columns: vec!["user_id".to_string()],
                to_columns: vec!["id".to_string()],
                is_collapsed_join: false,
                collapsed_join_table: None,
                label_x: 186.0,
                label_y: 190.0,
                routing_debug: None,
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
            routing_debug: None,
        }
    }

    /// Helper function to create a test layout `PositionedGraph` without groups
    fn layout_graph_without_groups() -> relune_layout::PositionedGraph {
        relune_layout::PositionedGraph {
            nodes: vec![PositionedNode {
                id: "users".to_string(),
                label: "users".to_string(),
                kind: NodeKind::Table,
                columns: vec![column(
                    "id",
                    "uuid",
                    false,
                    relation_flags(true, false, false),
                )],
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
            routing_debug: None,
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
        // Should contain the refreshed group accent band
        assert!(svg.contains("class=\"group-band\""));
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
    fn test_render_svg_group_labels_after_nodes() {
        let graph = layout_graph_with_groups();
        let svg = render_svg(&graph, SvgRenderOptions::default());

        let node_pos = svg
            .find("data-table-id=\"users\"")
            .expect("Node should exist");
        let label_pos = svg
            .rfind("class=\"group-label\"")
            .expect("Group label should exist");

        assert!(
            node_pos < label_pos,
            "Group labels should be rendered after nodes"
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
        assert!(svg.contains("#0c0f1a"));
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
        assert!(svg.contains("#f7f8fc"));
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
                columns: vec![column(
                    "col \"name\"",
                    "text",
                    false,
                    relation_flags(false, false, false),
                )],
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
            routing_debug: None,
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

    #[test]
    fn test_render_svg_with_node_overlay_adds_severity_class() {
        let graph = single_node_graph();
        let mut overlay = relune_layout::DiagramOverlay::new();
        overlay.add_node_annotation(
            "users",
            relune_layout::Annotation {
                severity: relune_layout::OverlaySeverity::Warning,
                message: "No primary key".to_string(),
                hint: Some("Add a PK column".to_string()),
                rule_id: Some("no-primary-key".to_string()),
            },
        );

        let options = SvgRenderOptions {
            show_tooltips: true,
            ..Default::default()
        };
        let svg = render_svg_with_overlay(&graph, options, Some(&overlay));

        assert!(svg.contains("overlay-warning"));
        assert!(svg.contains("overlay-badge"));
        assert!(svg.contains("[warning] No primary key"));
        assert!(svg.contains("Add a PK column"));
    }

    #[test]
    fn test_render_svg_with_edge_overlay_changes_stroke() {
        let graph = multi_node_graph();
        let mut overlay = relune_layout::DiagramOverlay::new();
        overlay.add_edge_annotation(
            "posts",
            "users",
            relune_layout::Annotation {
                severity: relune_layout::OverlaySeverity::Warning,
                message: "Missing index on FK".to_string(),
                hint: None,
                rule_id: Some("missing-foreign-key-index".to_string()),
            },
        );

        let options = SvgRenderOptions {
            show_tooltips: true,
            ..Default::default()
        };
        let svg = render_svg_with_overlay(&graph, options, Some(&overlay));

        // Edge should have overlay-warning class
        assert!(svg.contains("edge-kind-foreign-key overlay-warning"));
        // Tooltip should include annotation
        assert!(svg.contains("[warning] Missing index on FK"));
    }

    #[test]
    fn test_svg_capacity_estimate_covers_overlay_render() {
        let graph = multi_node_graph();
        let mut overlay = relune_layout::DiagramOverlay::new();
        overlay.add_node_annotation(
            "users",
            relune_layout::Annotation {
                severity: relune_layout::OverlaySeverity::Warning,
                message: "Very long warning message for capacity estimation".to_string(),
                hint: Some("Helpful remediation guidance for the warning".to_string()),
                rule_id: Some("warn-capacity".to_string()),
            },
        );
        overlay.add_edge_annotation(
            "posts",
            "users",
            relune_layout::Annotation {
                severity: relune_layout::OverlaySeverity::Error,
                message: "Another long edge warning for tooltip expansion".to_string(),
                hint: Some("Add the missing index".to_string()),
                rule_id: Some("edge-capacity".to_string()),
            },
        );

        let options = SvgRenderOptions {
            show_legend: true,
            show_tooltips: true,
            ..Default::default()
        };
        let estimated = estimate_svg_capacity(&graph, options, Some(&overlay));
        let svg = render_svg_with_overlay(&graph, options, Some(&overlay));

        assert!(estimated >= svg.len());
    }

    #[test]
    fn test_estimate_label_width_cjk_wider_than_ascii() {
        let ascii_width = estimate_label_width("abcd");
        let cjk_width = estimate_label_width("漢字表示");
        // CJK characters (full-width) should produce a wider estimate
        assert!(cjk_width > ascii_width);
    }

    #[test]
    fn test_estimate_label_width_mixed_script() {
        let ascii_only = estimate_label_width("users");
        let mixed = estimate_label_width("users_テーブル");
        assert!(mixed > ascii_only);
    }

    #[test]
    fn test_render_svg_without_overlay_unchanged() {
        let graph = single_node_graph();
        let options = SvgRenderOptions::default();

        let svg_no_overlay = render_svg_with_overlay(&graph, options, None);
        let svg_empty_overlay =
            render_svg_with_overlay(&graph, options, Some(&relune_layout::DiagramOverlay::new()));

        // Both should produce identical output (no overlay classes/badges)
        assert!(!svg_no_overlay.contains("overlay-"));
        assert!(!svg_empty_overlay.contains("overlay-"));
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
            .and_then(|positioned_graph| {
                render_svg(&positioned_graph, SvgRenderOptions::default()).ok()
            });
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
