//! SVG node (table / view / enum) rendering.

use std::fmt::{self, Write};

use relune_core::NodeKind;
use relune_layout::metrics::{
    COLUMN_BADGE_HEIGHT, COLUMN_BADGE_WIDTH, COLUMN_NULLABLE_GAP, COLUMN_NULLABLE_MARKER,
    COLUMN_NULLABLE_SLOT_WIDTH, ColumnSlots, NODE_COLUMN_FONT_SIZE, NODE_COLUMN_HEIGHT,
    NODE_CORNER_RADIUS, NODE_DETAIL_FONT_SIZE, NODE_FIRST_ROW_TOP, NODE_HEADER_BASELINE,
    NODE_HEADER_FONT_SIZE, NODE_HEADER_HEIGHT, NODE_KIND_LABEL_RESERVE, NODE_ROW_BASELINE,
    NODE_TEXT_INSET,
};

use crate::escape::{escape_attribute, escape_text};
use crate::theme::ThemeColors;
use crate::{is_light_theme, overlay_severity_color, overlay_severity_label};

// ---------------------------------------------------------------------------
// Node style
// ---------------------------------------------------------------------------

pub(crate) struct NodeStyle {
    pub body_fill: &'static str,
    pub header_fill: &'static str,
    pub stroke: &'static str,
}

pub(crate) const fn node_kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Table => "table",
        NodeKind::View => "view",
        NodeKind::Enum => "enum",
    }
}

pub(crate) const fn node_kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Table => "table",
        NodeKind::View => "view",
        NodeKind::Enum => "enum",
    }
}

pub(crate) const fn node_style(kind: NodeKind, colors: &ThemeColors) -> NodeStyle {
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

pub(crate) const fn node_label_background(colors: &ThemeColors) -> &'static str {
    if is_light_theme(colors) {
        "#ffffff"
    } else {
        "#111827"
    }
}

// ---------------------------------------------------------------------------
// Column badges (PK / FK / IX)
// ---------------------------------------------------------------------------

/// Unified column-metadata badge renderer.
///
/// All indicators share the same rounded-rect + label form-factor so they are
/// instantly distinguishable at a glance regardless of density.
fn render_column_badge(
    out: &mut String,
    x: f32,
    y: f32,
    label: &str,
    bg: &str,
    fg: &str,
) -> fmt::Result {
    write!(
        out,
        r#"<rect class="col-badge" x="{x:.1}" y="{y:.1}" width="{COLUMN_BADGE_WIDTH}" height="{COLUMN_BADGE_HEIGHT}" rx="3.5" fill="{bg}" fill-opacity="0.18"/><text x="{:.1}" y="{:.1}" font-family="'JetBrains Mono', ui-monospace, monospace" font-size="8.5" font-weight="700" letter-spacing="0.04em" fill="{fg}">{label}</text>"#,
        x + 2.5,
        y + 9.5,
    )
}

pub(crate) fn render_pk_indicator(out: &mut String, x: f32, y: f32) -> fmt::Result {
    render_column_badge(out, x, y, "PK", "#fbbf24", "#fbbf24")
}

pub(crate) fn render_fk_indicator(out: &mut String, x: f32, y: f32) -> fmt::Result {
    render_column_badge(out, x, y, "FK", "#38bdf8", "#38bdf8")
}

pub(crate) fn render_idx_indicator(out: &mut String, x: f32, y: f32) -> fmt::Result {
    render_column_badge(out, x, y, "IX", "#f59e0b", "#f59e0b")
}

// ---------------------------------------------------------------------------
// Overlay severity helpers
// ---------------------------------------------------------------------------

pub(crate) fn render_severity_badge(
    out: &mut String,
    x: f32,
    y: f32,
    severity: relune_layout::OverlaySeverity,
    count: usize,
    colors: &ThemeColors,
) -> fmt::Result {
    let fill = overlay_severity_color(severity, colors);
    let text_fill = if is_light_theme(colors) {
        "#ffffff"
    } else {
        "#0c0f1a"
    };
    let label = count.to_string();
    let badge_width = if count >= 10 { 22.0 } else { 18.0 };
    let badge_x = x - badge_width / 2.0;
    write!(
        out,
        r#"<rect class="overlay-badge" x="{badge_x:.1}" y="{y:.1}" width="{badge_width:.1}" height="18" rx="9" fill="{fill}"/><text x="{:.1}" y="{:.1}" font-family="'Inter', system-ui, sans-serif" font-size="10" font-weight="700" text-anchor="middle" fill="{text_fill}">{label}</text>"#,
        badge_x + badge_width / 2.0,
        y + 13.0,
    )
}

// ---------------------------------------------------------------------------
// Column rows
// ---------------------------------------------------------------------------

/// Horizontal positions shared by every column row of one node.
struct RowGeometry {
    /// Left edge of the key badges.
    key_x: f32,
    /// Left edge of the column name.
    name_x: f32,
    /// Right edge of the right-aligned type.
    type_end_x: f32,
    /// Left edge of the IX badge.
    index_x: f32,
    slots: ColumnSlots,
}

impl RowGeometry {
    fn new(node: &relune_layout::PositionedNode) -> Self {
        let slots = ColumnSlots::of(node.columns.iter().map(|column| &column.flags));
        let key_x = node.x + NODE_TEXT_INSET;
        let content_end = node.x + node.width - NODE_TEXT_INSET;
        let index_x = content_end - COLUMN_BADGE_WIDTH;
        let nullable_reserve = if slots.nullable {
            COLUMN_NULLABLE_SLOT_WIDTH
        } else {
            0.0
        };
        let index_reserve = slots.trailing_reserve() - nullable_reserve;
        Self {
            key_x,
            name_x: key_x + slots.key_gutter(),
            type_end_x: content_end - index_reserve - nullable_reserve,
            index_x,
            slots,
        }
    }
}

fn render_column_row(
    out: &mut String,
    column: &relune_layout::PositionedColumn,
    geometry: &RowGeometry,
    row_top: f32,
    clip_id: &str,
    colors: &ThemeColors,
) -> fmt::Result {
    let baseline = row_top + NODE_ROW_BASELINE;
    let badge_y = row_top + (NODE_COLUMN_HEIGHT - COLUMN_BADGE_HEIGHT) / 2.0;
    let relation = &column.flags.relation;

    write!(
        out,
        r#"<g class="column-row" data-column-name="{}" data-nullable="{}">"#,
        escape_attribute(&column.name),
        column.flags.nullable
    )?;
    if relation.is_primary_key {
        render_pk_indicator(out, geometry.key_x, badge_y)?;
    }
    if relation.is_foreign_key {
        render_fk_indicator(
            out,
            geometry.key_x + geometry.slots.foreign_key_offset(),
            badge_y,
        )?;
    }
    write!(
        out,
        r#"<text class="column-name" x="{:.1}" y="{baseline:.1}" clip-path="url(#{clip_id})" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="{NODE_COLUMN_FONT_SIZE}" fill="{}">{}</text>"#,
        geometry.name_x,
        colors.text_secondary,
        escape_text(&column.name)
    )?;
    if !column.data_type.is_empty() {
        write!(
            out,
            r#"<text class="column-type" x="{:.1}" y="{baseline:.1}" clip-path="url(#{clip_id})" text-anchor="end" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="{NODE_DETAIL_FONT_SIZE}" fill="{}">{}</text>"#,
            geometry.type_end_x,
            colors.text_muted,
            escape_text(&column.data_type)
        )?;
    }
    if column.flags.nullable {
        write!(
            out,
            r#"<text class="column-nullable" x="{:.1}" y="{baseline:.1}" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="{NODE_DETAIL_FONT_SIZE}" fill="{}">{COLUMN_NULLABLE_MARKER}</text>"#,
            geometry.type_end_x + COLUMN_NULLABLE_GAP,
            colors.text_muted,
        )?;
    }
    if relation.is_indexed {
        render_idx_indicator(out, geometry.index_x, badge_y)?;
    }
    out.push_str("</g>");
    Ok(())
}

// ---------------------------------------------------------------------------
// Node rendering
// ---------------------------------------------------------------------------

/// Render a single positioned node as SVG markup.
#[allow(clippy::cast_precision_loss)] // Entry animation indices are presentation-only.
#[allow(clippy::too_many_lines)] // SVG node markup with overlay integration is clearer in one block.
pub(crate) fn render_node_internal(
    out: &mut String,
    node: &relune_layout::PositionedNode,
    colors: &ThemeColors,
    show_tooltips: bool,
    index: usize,
    overlay: Option<&relune_layout::NodeOverlay>,
) -> fmt::Result {
    let kind = node_kind_name(node.kind);
    let node_style = node_style(node.kind, colors);
    let node_label = node_kind_label(node.kind);
    let max_severity = overlay.and_then(relune_layout::NodeOverlay::max_severity);

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
        r#"<g class="table-node node node-kind-{}{}" data-table-id="{}" data-id="{}" data-node-kind="{}" style="--enter-delay:{:.3}s">"#,
        kind,
        severity_class,
        escape_attribute(&node.id),
        escape_attribute(&node.id),
        kind,
        index as f32 * 0.022
    )?;

    // Add tooltip if enabled (with overlay annotations appended)
    if show_tooltips {
        let column_count = node.columns.len();
        let pk_count = node
            .columns
            .iter()
            .filter(|c| c.flags.relation.is_primary_key)
            .count();
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
        if let Some(node_overlay) = overlay
            && !node_overlay.annotations.is_empty()
        {
            tooltip_parts.push(String::new());
            for annotation in &node_overlay.annotations {
                let severity_label = overlay_severity_label(annotation.severity);
                tooltip_parts.push(format!("[{}] {}", severity_label, annotation.message));
                if let Some(ref hint) = annotation.hint {
                    tooltip_parts.push(format!("  → {hint}"));
                }
            }
        }
        write!(
            out,
            r"<title>{}</title>",
            escape_text(&tooltip_parts.join("\n"))
        )?;
    }

    // Node border: override stroke color when overlay severity is present
    let (stroke_color, stroke_width) = match max_severity {
        Some(severity) => (overlay_severity_color(severity, colors), "2.4"),
        None => (node_style.stroke, "1.6"),
    };

    write!(
        out,
        r#"<rect class="table-body" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{NODE_CORNER_RADIUS}" ry="{NODE_CORNER_RADIUS}" fill="{}" stroke="{}" stroke-width="{}" filter="url(#node-shadow)"/>"#,
        node.x, node.y, node.width, node.height, node_style.body_fill, stroke_color, stroke_width
    )?;
    write!(
        out,
        r#"<rect class="table-header" x="{:.1}" y="{:.1}" width="{:.1}" height="{NODE_HEADER_HEIGHT}" rx="{NODE_CORNER_RADIUS}" ry="{NODE_CORNER_RADIUS}" fill="{}"/>"#,
        node.x, node.y, node.width, node_style.header_fill
    )?;
    // Gradient transition from header to body — eliminates the hard underlay band
    write!(
        out,
        r#"<rect class="table-header-fade" x="{:.1}" y="{:.1}" width="{:.1}" height="16" fill="url(#header-fade-{kind})"/>"#,
        node.x,
        node.y + 16.0,
        node.width
    )?;
    write!(
        out,
        r#"<clipPath id="node-{index}-header-clip"><rect x="{:.1}" y="{:.1}" width="{:.1}" height="{NODE_HEADER_HEIGHT}"/></clipPath><text class="table-name" x="{:.1}" y="{:.1}" clip-path="url(#node-{index}-header-clip)" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="{NODE_HEADER_FONT_SIZE}" font-weight="700" fill="{}">{}</text>"#,
        node.x + NODE_TEXT_INSET,
        node.y,
        (node.width - NODE_TEXT_INSET - NODE_KIND_LABEL_RESERVE).max(40.0),
        node.x + NODE_TEXT_INSET,
        node.y + NODE_HEADER_BASELINE,
        colors.text_primary,
        escape_text(&node.label)
    )?;
    write!(
        out,
        r#"<text class="table-kind" x="{:.1}" y="{:.1}" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="9" font-weight="600" text-anchor="end" letter-spacing="0.12em" fill="{}">{}</text>"#,
        node.x + node.width - NODE_TEXT_INSET,
        node.y + NODE_HEADER_BASELINE,
        colors.text_muted,
        escape_text(&kind.to_ascii_uppercase())
    )?;

    // Render severity badge when overlay has annotations
    if let Some(severity) = max_severity {
        let annotation_count = overlay.map_or(0, |o| o.annotations.len());
        render_severity_badge(
            out,
            node.x + node.width - 12.0,
            node.y - 6.0,
            severity,
            annotation_count,
            colors,
        )?;
    }

    if !node.columns.is_empty() {
        let geometry = RowGeometry::new(node);
        // Names and types share one clip so neither can spill past the
        // node's trailing marks, whatever the font actually measures.
        let clip_id = format!("node-{index}-columns-clip");
        write!(
            out,
            r#"<clipPath id="{clip_id}"><rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}"/></clipPath>"#,
            geometry.name_x,
            node.y,
            (geometry.type_end_x - geometry.name_x).max(0.0),
            node.height
        )?;
        let mut row_top = node.y + NODE_FIRST_ROW_TOP;
        for column in &node.columns {
            render_column_row(out, column, &geometry, row_top, &clip_id, colors)?;
            row_top += NODE_COLUMN_HEIGHT;
        }
    }
    write!(
        out,
        r#"<rect class="type-filter-overlay" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{NODE_CORNER_RADIUS}" ry="{NODE_CORNER_RADIUS}" fill="url(#type-filter-hatch)" opacity="0"/>"#,
        node.x, node.y, node.width, node.height
    )?;
    out.push_str("</g>");
    Ok(())
}
