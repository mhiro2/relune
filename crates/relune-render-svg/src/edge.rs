//! Enhanced edge rendering with CSS classes and data attributes for interactivity.

use std::fmt::{self, Write};

use relune_core::layout::{EdgeRoute, RouteStyle};
use relune_layout::PositionedEdge;

use crate::theme::ThemeColors;

/// Builds the SVG `d` attribute for [`EdgeRoute`] from the layout engine.
#[must_use]
pub(crate) fn edge_route_svg_path_d(route: &EdgeRoute, curve_offset: f32) -> String {
    let points = route_backbone_points(route);

    match route.style {
        RouteStyle::Straight if route.control_points.is_empty() => {
            format!(
                "M {:.1} {:.1} L {:.1} {:.1}",
                route.x1, route.y1, route.x2, route.y2
            )
        }
        RouteStyle::Straight | RouteStyle::Orthogonal => polyline_path_d(&points),
        RouteStyle::Curved => rounded_backbone_path_d(&points, curve_offset),
    }
}

fn route_backbone_points(route: &EdgeRoute) -> Vec<(f32, f32)> {
    let mut points = Vec::with_capacity(route.control_points.len() + 2);
    points.push((route.x1, route.y1));
    points.extend(route.control_points.iter().copied());
    points.push((route.x2, route.y2));
    points
}

/// Approximates the visible route as a polyline after style-specific rendering.
#[must_use]
pub fn rendered_path_points(
    route: &EdgeRoute,
    curve_offset: f32,
    curve_samples: u32,
) -> Vec<(f32, f32)> {
    let points = route_backbone_points(route);
    match route.style {
        RouteStyle::Curved => rounded_backbone_points(&points, curve_offset, curve_samples),
        RouteStyle::Straight | RouteStyle::Orthogonal => points,
    }
}

fn polyline_path_d(points: &[(f32, f32)]) -> String {
    let Some(&(x, y)) = points.first() else {
        return String::new();
    };

    let mut d = format!("M {x:.1} {y:.1}");
    for &(px, py) in &points[1..] {
        write!(&mut d, " L {px:.1} {py:.1}").expect("writing SVG path to String must succeed");
    }
    d
}

fn rounded_backbone_path_d(points: &[(f32, f32)], curve_offset: f32) -> String {
    let Some(&(start_x, start_y)) = points.first() else {
        return String::new();
    };
    if points.len() < 3 {
        return format!(
            "M {start_x:.1} {start_y:.1} L {:.1} {:.1}",
            points[1].0, points[1].1
        );
    }

    let mut d = format!("M {start_x:.1} {start_y:.1}");
    let max_radius = curve_offset.clamp(0.0, 24.0);

    for window in points.windows(3) {
        let prev = window[0];
        let curr = window[1];
        let next = window[2];

        let incoming = segment_length(prev, curr);
        let outgoing = segment_length(curr, next);
        let radius = max_radius.min(incoming * 0.5).min(outgoing * 0.5);

        if radius <= 0.0 || is_collinear(prev, curr, next) {
            write!(&mut d, " L {:.1} {:.1}", curr.0, curr.1)
                .expect("writing SVG path to String must succeed");
            continue;
        }

        let before = move_toward(curr, prev, radius);
        let after = move_toward(curr, next, radius);
        write!(
            &mut d,
            " L {:.1} {:.1} Q {:.1} {:.1} {:.1} {:.1}",
            before.0, before.1, curr.0, curr.1, after.0, after.1
        )
        .expect("writing SVG path to String must succeed");
    }

    let end = points[points.len() - 1];
    write!(&mut d, " L {:.1} {:.1}", end.0, end.1)
        .expect("writing SVG path to String must succeed");
    d
}

fn rounded_backbone_points(
    points: &[(f32, f32)],
    curve_offset: f32,
    curve_samples: u32,
) -> Vec<(f32, f32)> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let sample_count = curve_samples.max(1);
    let mut rendered = vec![points[0]];
    let max_radius = curve_offset.clamp(0.0, 24.0);

    for window in points.windows(3) {
        let prev = window[0];
        let curr = window[1];
        let next = window[2];

        let incoming = segment_length(prev, curr);
        let outgoing = segment_length(curr, next);
        let radius = max_radius.min(incoming * 0.5).min(outgoing * 0.5);

        if radius <= 0.0 || is_collinear(prev, curr, next) {
            push_if_distinct(&mut rendered, curr);
            continue;
        }

        let before = move_toward(curr, prev, radius);
        let after = move_toward(curr, next, radius);
        push_if_distinct(&mut rendered, before);
        for index in 1..=sample_count {
            #[allow(clippy::cast_precision_loss)]
            let t = index as f32 / sample_count as f32;
            push_if_distinct(
                &mut rendered,
                quadratic_bezier_point(before, curr, after, t),
            );
        }
    }

    push_if_distinct(&mut rendered, points[points.len() - 1]);
    rendered
}

fn segment_length(from: (f32, f32), to: (f32, f32)) -> f32 {
    (to.0 - from.0).hypot(to.1 - from.1)
}

fn move_toward(from: (f32, f32), to: (f32, f32), distance: f32) -> (f32, f32) {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let length = dx.hypot(dy);
    if length <= 0.001 {
        return from;
    }

    let ratio = distance / length;
    (dx.mul_add(ratio, from.0), dy.mul_add(ratio, from.1))
}

fn is_collinear(prev: (f32, f32), curr: (f32, f32), next: (f32, f32)) -> bool {
    let same_x = (prev.0 - curr.0).abs() < 0.5 && (curr.0 - next.0).abs() < 0.5;
    let same_y = (prev.1 - curr.1).abs() < 0.5 && (curr.1 - next.1).abs() < 0.5;
    same_x || same_y
}

fn quadratic_bezier_point(
    start: (f32, f32),
    control: (f32, f32),
    end: (f32, f32),
    t: f32,
) -> (f32, f32) {
    let one_minus_t = 1.0 - t;
    let start_weight = one_minus_t * one_minus_t;
    let control_weight = 2.0 * one_minus_t * t;
    let end_weight = t * t;

    (
        end_weight.mul_add(
            end.0,
            (start_weight * start.0) + (control_weight * control.0),
        ),
        end_weight.mul_add(
            end.1,
            (start_weight * start.1) + (control_weight * control.1),
        ),
    )
}

fn push_if_distinct(points: &mut Vec<(f32, f32)>, point: (f32, f32)) {
    let is_distinct = points
        .last()
        .is_none_or(|last| (last.0 - point.0).abs() > 0.001 || (last.1 - point.1).abs() > 0.001);
    if is_distinct {
        points.push(point);
    }
}

/// Options for edge rendering.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct EdgeRenderOptions {
    /// Stroke width for edges.
    pub stroke_width: f32,
    /// Whether to show edge labels.
    pub show_labels: bool,
    /// Whether to use dashed lines for edges.
    pub dashed: bool,
    /// Curve offset for bezier control points (0 = straight, higher = curvier).
    pub curve_offset: f32,
    /// Whether to show cardinality indicators (1, N, 0..1, etc.).
    pub show_cardinality: bool,
    /// Whether to show FK column names on edges.
    pub show_fk_columns: bool,
    /// Whether to show tooltips on hover.
    pub show_tooltips: bool,
}

impl Default for EdgeRenderOptions {
    fn default() -> Self {
        Self {
            stroke_width: 1.2,
            show_labels: true,
            dashed: false,
            curve_offset: 50.0,
            show_cardinality: true,
            show_fk_columns: true,
            show_tooltips: false,
        }
    }
}

/// Renders an edge with enhanced styling, CSS classes, and data attributes.
///
/// # Arguments
/// * `out` - The output string buffer
/// * `edge` - The positioned edge to render
/// * `theme` - The theme colors to use
/// * `options` - The rendering options
pub fn render_edge(
    out: &mut String,
    edge: &PositionedEdge,
    theme: &ThemeColors,
    options: &EdgeRenderOptions,
) -> fmt::Result {
    let path_d = edge_route_svg_path_d(&edge.route, options.curve_offset);

    // Build stroke dash array if dashed
    let stroke_dasharray = if options.dashed {
        r#" stroke-dasharray="5,3""#
    } else {
        ""
    };

    // Add tooltip if enabled
    if options.show_tooltips {
        let tooltip_text = generate_edge_tooltip(edge);
        write!(
            out,
            r#"<g class="edge-group"><title>{}</title>"#,
            escape_text(&tooltip_text)
        )?;
    }

    // Render the path with CSS class and data attributes
    write!(
        out,
        r#"<path class="edge-path" data-from="{}" data-to="{}" d="{}" stroke="{}" stroke-width="{:.1}" fill="none" marker-end="url(#arrow)"{}/>"#,
        escape_attribute(&edge.from),
        escape_attribute(&edge.to),
        escape_attribute(&path_d),
        theme.edge_stroke,
        options.stroke_width,
        stroke_dasharray
    )?;

    // Determine if we need to render any labels
    let has_label = options.show_labels && !edge.label.is_empty();
    let has_cardinality = options.show_cardinality;
    let has_fk_columns = options.show_fk_columns && !edge.from_columns.is_empty();

    if !has_label && !has_cardinality && !has_fk_columns {
        // Close the group tag if tooltips are enabled
        if options.show_tooltips {
            out.push_str("</g>");
        }
        return Ok(());
    }

    let going_right = edge.route.x2 > edge.route.x1;

    // Render cardinality indicators at endpoints
    if has_cardinality {
        render_cardinality_labels(out, edge, theme, going_right)?;
    }

    // Build the main label text
    let mut label_parts = Vec::new();
    if has_label {
        label_parts.push(edge.label.clone());
    }
    if has_fk_columns {
        label_parts.push(edge.from_columns.join(", "));
    }

    if !label_parts.is_empty() {
        let label_text = label_parts.join(": ");
        let label_x = edge.label_x;
        let label_y = edge.label_y;

        // Render label with background for better readability
        write!(
            out,
            r#"<text class="edge-label" x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="10" fill="{}" text-anchor="middle">{}</text>"#,
            label_x,
            label_y,
            theme.text_muted,
            escape_text(&label_text)
        )?;
    }

    // Close the group tag if tooltips are enabled
    if options.show_tooltips {
        out.push_str("</g>");
    }
    Ok(())
}

/// Renders cardinality labels at the endpoints of an edge.
fn render_cardinality_labels(
    out: &mut String,
    edge: &PositionedEdge,
    theme: &ThemeColors,
    going_right: bool,
) -> fmt::Result {
    // Source side (FK / child table): always "many" — each parent can be
    // referenced by multiple child rows.  The nullable flag determines
    // participation: optional (0..N) vs mandatory (1..N).
    let source_symbol = if edge.nullable { "0..N" } else { "1..N" };

    let target_symbol = edge.target_cardinality.symbol();

    // Source label position (near x1, y1)
    let (source_x, source_y) = if going_right {
        (edge.route.x1 - 5.0, edge.route.y1)
    } else {
        (edge.route.x1 + 5.0, edge.route.y1)
    };
    let source_anchor = if going_right { "end" } else { "start" };

    // Target label position (near x2, y2)
    let (target_x, target_y) = if going_right {
        (edge.route.x2 + 5.0, edge.route.y2)
    } else {
        (edge.route.x2 - 5.0, edge.route.y2)
    };
    let target_anchor = if going_right { "start" } else { "end" };

    // Render source cardinality
    write!(
        out,
        r#"<text class="cardinality-label" x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="9" font-weight="600" fill="{}" text-anchor="{}" dominant-baseline="middle">{}</text>"#,
        source_x, source_y, theme.text_secondary, source_anchor, source_symbol
    )?;

    // Render target cardinality
    write!(
        out,
        r#"<text class="cardinality-label" x="{:.1}" y="{:.1}" font-family="ui-sans-serif, system-ui" font-size="9" font-weight="600" fill="{}" text-anchor="{}" dominant-baseline="middle">{}</text>"#,
        target_x, target_y, theme.text_secondary, target_anchor, target_symbol
    )?;
    Ok(())
}

/// Generates tooltip text for an edge.
fn generate_edge_tooltip(edge: &PositionedEdge) -> String {
    let mut lines = Vec::new();

    // Add FK name if available
    if !edge.label.is_empty() {
        lines.push(format!("Foreign Key: {}", edge.label));
    }

    // Add column mapping
    if !edge.from_columns.is_empty() && !edge.to_columns.is_empty() {
        let from_cols = edge.from_columns.join(", ");
        let to_cols = edge.to_columns.join(", ");
        lines.push(format!(
            "{}.{} -> {}.{}",
            edge.from, from_cols, edge.to, to_cols
        ));
    } else {
        // Simple format without column details
        lines.push(format!("{} -> {}", edge.from, edge.to));
    }

    // Add nullability info
    if edge.nullable {
        lines.push("Nullable: Yes".to_string());
    } else {
        lines.push("Nullable: No".to_string());
    }

    lines.join("\n")
}

use crate::escape::{escape_attribute, escape_text};

#[cfg(test)]
mod tests {
    use super::*;
    use relune_core::EdgeKind;
    use relune_core::layout::{Cardinality, EdgeRoute, RouteStyle};

    fn render_edge_ok(
        out: &mut String,
        edge: &PositionedEdge,
        colors: &ThemeColors,
        options: &EdgeRenderOptions,
    ) {
        render_edge(out, edge, colors, options).expect("edge rendering should succeed in tests");
    }

    fn create_test_theme() -> ThemeColors {
        ThemeColors {
            background: "#0c0f1a",
            canvas_base: "#0c0f1a",
            canvas_dot: "#151928",
            foreground: "#e2e8f0",
            node_fill: "#111827",
            node_stroke: "#334155",
            header_fill: "#1e293b",
            text_primary: "#e2e8f0",
            text_secondary: "#cbd5e1",
            text_muted: "#94a3b8",
            edge_stroke: "#64748b",
            arrow_fill: "#64748b",
            node_shadow: "rgba(0, 0, 0, 0.5)",
            group_fill: "#0f172acc",
            group_band_fill: "#172036",
            group_stroke: "#334155",
            glow_color: "#f59e0b",
            glow_particle: "#fbbf24",
            is_light: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn make_edge(
        from: &str,
        to: &str,
        label: &str,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        nullable: bool,
        from_columns: Vec<String>,
        to_columns: Vec<String>,
    ) -> PositionedEdge {
        PositionedEdge {
            from: from.to_string(),
            to: to.to_string(),
            label: label.to_string(),
            kind: EdgeKind::ForeignKey,
            route: EdgeRoute {
                x1,
                y1,
                x2,
                y2,
                control_points: vec![],
                style: RouteStyle::Straight,
                label_position: (f32::midpoint(x1, x2), f32::midpoint(y1, y2)),
            },
            is_self_loop: false,
            nullable,
            target_cardinality: Cardinality::One,
            from_columns,
            to_columns,
            is_collapsed_join: false,
            collapsed_join_table: None,
            label_x: f32::midpoint(x1, x2),
            label_y: f32::midpoint(y1, y2),
            routing_debug: None,
        }
    }

    #[test]
    fn test_render_edge_produces_valid_svg() {
        let edge = make_edge(
            "users",
            "posts",
            "user_id",
            100.0,
            50.0,
            300.0,
            150.0,
            false,
            vec!["user_id".to_string()],
            vec!["id".to_string()],
        );

        let colors = create_test_theme();
        let options = EdgeRenderOptions::default();

        let mut out = String::new();
        render_edge_ok(&mut out, &edge, &colors, &options);

        assert!(out.contains("class=\"edge-path\""));
        assert!(out.contains("data-from=\"users\""));
        assert!(out.contains("data-to=\"posts\""));
        assert!(out.contains("class=\"edge-label\""));
        assert!(out.contains("marker-end=\"url(#arrow)\""));
    }

    #[test]
    fn test_render_edge_dashed() {
        let edge = make_edge("a", "b", "", 0.0, 0.0, 100.0, 100.0, false, vec![], vec![]);

        let colors = create_test_theme();
        let options = EdgeRenderOptions {
            dashed: true,
            show_cardinality: false,
            ..Default::default()
        };

        let mut out = String::new();
        render_edge_ok(&mut out, &edge, &colors, &options);

        assert!(out.contains("stroke-dasharray=\"5,3\""));
    }

    #[test]
    fn test_render_edge_no_label() {
        let edge = make_edge(
            "a",
            "b",
            "test_label",
            0.0,
            0.0,
            100.0,
            100.0,
            false,
            vec![],
            vec![],
        );

        let colors = create_test_theme();
        let options = EdgeRenderOptions {
            show_labels: false,
            show_cardinality: false,
            show_fk_columns: false,
            ..Default::default()
        };

        let mut out = String::new();
        render_edge_ok(&mut out, &edge, &colors, &options);

        assert!(!out.contains("edge-label"));
        assert!(!out.contains("test_label"));
    }

    #[test]
    fn test_render_cardinality_non_nullable() {
        let edge = make_edge(
            "posts",
            "users",
            "user_id",
            100.0,
            100.0,
            300.0,
            100.0,
            false,
            vec!["user_id".to_string()],
            vec!["id".to_string()],
        );

        let colors = create_test_theme();
        let options = EdgeRenderOptions {
            show_cardinality: true,
            show_labels: false,
            show_fk_columns: false,
            ..Default::default()
        };

        let mut out = String::new();
        render_edge_ok(&mut out, &edge, &colors, &options);

        // Non-nullable FK should show "1..N" at source (mandatory many)
        assert!(out.contains(">1..N</text>"));
        // Target unique reference should show "1"
        assert!(out.contains(">1</text>"));
    }

    #[test]
    fn test_render_cardinality_nullable() {
        let edge = make_edge(
            "posts",
            "users",
            "reviewer_id",
            100.0,
            100.0,
            300.0,
            100.0,
            true,
            vec!["reviewer_id".to_string()],
            vec!["id".to_string()],
        );

        let colors = create_test_theme();
        let options = EdgeRenderOptions {
            show_cardinality: true,
            show_labels: false,
            show_fk_columns: false,
            ..Default::default()
        };

        let mut out = String::new();
        render_edge_ok(&mut out, &edge, &colors, &options);

        // Nullable FK should show "0..N" at source (optional many)
        assert!(out.contains(">0..N</text>"));
        // Target unique reference should still show "1"
        assert!(out.contains(">1</text>"));
    }

    #[test]
    fn test_render_cardinality_many_target() {
        let mut edge = make_edge(
            "audit_entries",
            "users",
            "actor_email",
            100.0,
            100.0,
            300.0,
            100.0,
            false,
            vec!["actor_email".to_string()],
            vec!["email".to_string()],
        );
        edge.target_cardinality = Cardinality::Many;

        let colors = create_test_theme();
        let options = EdgeRenderOptions {
            show_cardinality: true,
            show_labels: false,
            show_fk_columns: false,
            ..Default::default()
        };

        let mut out = String::new();
        render_edge_ok(&mut out, &edge, &colors, &options);

        assert!(out.contains(">N</text>"));
    }

    #[test]
    fn test_render_fk_columns() {
        let edge = make_edge(
            "order_items",
            "products",
            "fk_product",
            100.0,
            100.0,
            300.0,
            100.0,
            false,
            vec!["product_id".to_string()],
            vec!["id".to_string()],
        );

        let colors = create_test_theme();
        let options = EdgeRenderOptions {
            show_cardinality: false,
            show_labels: true,
            show_fk_columns: true,
            ..Default::default()
        };

        let mut out = String::new();
        render_edge_ok(&mut out, &edge, &colors, &options);

        // Should show both label and FK column
        assert!(out.contains("fk_product: product_id"));
    }

    #[test]
    fn test_render_multiple_fk_columns() {
        let edge = make_edge(
            "order_items",
            "composite_pk",
            "fk_composite",
            100.0,
            100.0,
            300.0,
            100.0,
            false,
            vec!["tenant_id".to_string(), "order_id".to_string()],
            vec!["tenant_id".to_string(), "id".to_string()],
        );

        let colors = create_test_theme();
        let options = EdgeRenderOptions {
            show_cardinality: false,
            show_labels: false,
            show_fk_columns: true,
            ..Default::default()
        };

        let mut out = String::new();
        render_edge_ok(&mut out, &edge, &colors, &options);

        // Should show both FK columns
        assert!(out.contains("tenant_id, order_id"));
    }

    #[test]
    fn test_cardinality_disabled() {
        let edge = make_edge(
            "posts",
            "users",
            "user_id",
            100.0,
            100.0,
            300.0,
            100.0,
            true,
            vec!["user_id".to_string()],
            vec!["id".to_string()],
        );

        let colors = create_test_theme();
        let options = EdgeRenderOptions {
            show_cardinality: false,
            show_labels: false,
            show_fk_columns: false,
            ..Default::default()
        };

        let mut out = String::new();
        render_edge_ok(&mut out, &edge, &colors, &options);

        // Should not contain cardinality class
        assert!(!out.contains("cardinality-label"));
    }

    #[test]
    fn test_label_positioning() {
        let edge = make_edge(
            "a",
            "b",
            "test",
            0.0,
            0.0,
            200.0,
            100.0,
            false,
            vec![],
            vec![],
        );

        let colors = create_test_theme();
        let options = EdgeRenderOptions {
            show_cardinality: false,
            show_labels: true,
            show_fk_columns: false,
            ..Default::default()
        };

        let mut out = String::new();
        render_edge_ok(&mut out, &edge, &colors, &options);

        // Label uses layout-computed `label_y` (straight route: segment midpoint).
        assert!(out.contains("y=\"50.0\""));
        assert!(out.contains("text-anchor=\"middle\""));
    }

    #[test]
    fn test_orthogonal_route_renders_polyline_path() {
        let edge = PositionedEdge {
            from: "a".into(),
            to: "b".into(),
            label: String::new(),
            kind: EdgeKind::ForeignKey,
            route: EdgeRoute {
                x1: 100.0,
                y1: 25.0,
                x2: 200.0,
                y2: 125.0,
                control_points: vec![(150.0, 25.0), (150.0, 125.0)],
                style: RouteStyle::Orthogonal,
                label_position: (150.0, 75.0),
            },
            is_self_loop: false,
            nullable: false,
            target_cardinality: Cardinality::One,
            from_columns: vec![],
            to_columns: vec![],
            is_collapsed_join: false,
            collapsed_join_table: None,
            label_x: 150.0,
            label_y: 75.0,
            routing_debug: None,
        };

        let colors = create_test_theme();
        let options = EdgeRenderOptions {
            show_cardinality: false,
            show_labels: false,
            show_fk_columns: false,
            ..Default::default()
        };

        let mut out = String::new();
        render_edge_ok(&mut out, &edge, &colors, &options);

        assert!(out.contains("M 100.0 25.0"));
        assert!(out.contains("L 150.0 25.0"));
        assert!(out.contains("L 150.0 125.0"));
        assert!(out.contains("L 200.0 125.0"));
    }

    #[test]
    fn test_straight_style_keeps_backbone_when_bends_are_required() {
        let path = edge_route_svg_path_d(
            &EdgeRoute {
                x1: 100.0,
                y1: 25.0,
                x2: 200.0,
                y2: 125.0,
                control_points: vec![(150.0, 25.0), (150.0, 125.0)],
                style: RouteStyle::Straight,
                label_position: (150.0, 75.0),
            },
            50.0,
        );

        assert!(path.contains("L 150.0 25.0"));
        assert!(path.contains("L 150.0 125.0"));
    }

    #[test]
    fn test_curved_style_rounds_backbone_corners() {
        let path = edge_route_svg_path_d(
            &EdgeRoute {
                x1: 100.0,
                y1: 25.0,
                x2: 200.0,
                y2: 125.0,
                control_points: vec![(150.0, 25.0), (150.0, 125.0)],
                style: RouteStyle::Curved,
                label_position: (150.0, 75.0),
            },
            50.0,
        );

        assert!(path.contains("Q 150.0 25.0"));
        assert!(path.contains("Q 150.0 125.0"));
    }

    #[test]
    fn test_rendered_path_points_follow_curved_corner_geometry() {
        let points = rendered_path_points(
            &EdgeRoute {
                x1: 100.0,
                y1: 25.0,
                x2: 200.0,
                y2: 125.0,
                control_points: vec![(150.0, 25.0), (150.0, 125.0)],
                style: RouteStyle::Curved,
                label_position: (150.0, 75.0),
            },
            50.0,
            8,
        );

        assert_eq!(points.first().copied(), Some((100.0, 25.0)));
        assert_eq!(points.last().copied(), Some((200.0, 125.0)));
        assert!(points.iter().any(|point| point.1 > 25.0 && point.0 < 150.0));
        assert!(
            points
                .iter()
                .any(|point| point.0 > 150.0 && point.1 < 125.0)
        );
    }

    #[test]
    fn test_cardinality_symbol() {
        assert_eq!(Cardinality::One.symbol(), "1");
        assert_eq!(Cardinality::ZeroOrOne.symbol(), "0..1");
        assert_eq!(Cardinality::Many.symbol(), "N");
    }

    #[test]
    fn test_tooltip_generation() {
        let edge = make_edge(
            "orders",
            "customers",
            "fk_customer",
            0.0,
            0.0,
            100.0,
            100.0,
            false,
            vec!["customer_id".to_string()],
            vec!["id".to_string()],
        );

        let tooltip = generate_edge_tooltip(&edge);

        assert!(tooltip.contains("Foreign Key: fk_customer"));
        assert!(tooltip.contains("orders.customer_id -> customers.id"));
        assert!(tooltip.contains("Nullable: No"));
    }

    #[test]
    fn test_tooltip_nullable() {
        let edge = make_edge(
            "posts",
            "users",
            "reviewer_fk",
            0.0,
            0.0,
            100.0,
            100.0,
            true,
            vec!["reviewer_id".to_string()],
            vec!["id".to_string()],
        );

        let tooltip = generate_edge_tooltip(&edge);

        assert!(tooltip.contains("Nullable: Yes"));
    }
}
