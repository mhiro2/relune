//! SVG node (table / view / enum) rendering.

use std::fmt::{self, Write};

use relune_core::NodeKind;
use unicode_width::UnicodeWidthChar;

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
    pub separator: &'static str,
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
            separator: "#4a3415",
        },
        (NodeKind::Table, true) => NodeStyle {
            body_fill: "#fffaf0",
            header_fill: "#f59e0b",
            stroke: "#d97706",
            separator: "#fed7aa",
        },
        (NodeKind::View, false) => NodeStyle {
            body_fill: "#10232a",
            header_fill: "#0f766e",
            stroke: "#2dd4bf",
            separator: "#134e4a",
        },
        (NodeKind::View, true) => NodeStyle {
            body_fill: "#f0fdfa",
            header_fill: "#14b8a6",
            stroke: "#0f766e",
            separator: "#99f6e4",
        },
        (NodeKind::Enum, false) => NodeStyle {
            body_fill: "#241533",
            header_fill: "#7c3aed",
            stroke: "#c084fc",
            separator: "#4c1d95",
        },
        (NodeKind::Enum, true) => NodeStyle {
            body_fill: "#faf5ff",
            header_fill: "#a855f7",
            stroke: "#7e22ce",
            separator: "#e9d5ff",
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
// Label width estimation
// ---------------------------------------------------------------------------

pub(crate) fn estimate_label_width(text: &str) -> f32 {
    text.chars()
        .map(|ch| match ch.width_cjk().or_else(|| ch.width()) {
            Some(0) | None => 0.0,
            Some(1) => 6.4,
            Some(_) => 12.8,
        })
        .sum::<f32>()
        + 18.0
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
        r#"<rect class="col-badge" x="{x:.1}" y="{y:.1}" width="20" height="13" rx="3.5" fill="{bg}" fill-opacity="0.18"/><text x="{:.1}" y="{:.1}" font-family="'JetBrains Mono', ui-monospace, monospace" font-size="8.5" font-weight="700" letter-spacing="0.04em" fill="{fg}">{label}</text>"#,
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
// Column text width
// ---------------------------------------------------------------------------

pub(crate) fn column_text_width(
    node: &relune_layout::PositionedNode,
    column: &relune_layout::PositionedColumn,
) -> f32 {
    let icon_slots = usize::from(column.flags.relation.is_indexed)
        + usize::from(column.flags.relation.is_foreign_key)
        + usize::from(column.flags.relation.is_primary_key);
    if icon_slots == 0 {
        (node.width - 20.0).max(18.0)
    } else {
        // Badges start at node.width - 22, spaced 24px apart (left edge to left edge).
        // Reserve space for all badges plus a small gap before the leftmost one.
        #[allow(clippy::cast_precision_loss)] // Icon counts are tiny and only affect text clipping.
        let badge_area = (icon_slots as f32 - 1.0).mul_add(24.0, 28.0);
        (node.width - 10.0 - badge_area).max(18.0)
    }
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
        r#"<rect class="table-body" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="16" ry="16" fill="{}" stroke="{}" stroke-width="{}" filter="url(#node-shadow)"/>"#,
        node.x, node.y, node.width, node.height, node_style.body_fill, stroke_color, stroke_width
    )?;
    write!(
        out,
        r#"<rect class="table-header" x="{:.1}" y="{:.1}" width="{:.1}" height="32" rx="16" ry="16" fill="{}"/>"#,
        node.x, node.y, node.width, node_style.header_fill
    )?;
    // Gradient transition from header to body — eliminates the hard underlay band
    write!(
        out,
        r#"<defs><linearGradient id="header-fade-{index}" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="{}" stop-opacity="0.38"/><stop offset="100%" stop-color="{}" stop-opacity="0"/></linearGradient></defs><rect class="table-header-fade" x="{:.1}" y="{:.1}" width="{:.1}" height="16" fill="url(#header-fade-{index})"/>"#,
        node_style.header_fill,
        node_style.header_fill,
        node.x,
        node.y + 16.0,
        node.width
    )?;
    write!(
        out,
        r#"<clipPath id="node-{index}-header-clip"><rect x="{:.1}" y="{:.1}" width="{:.1}" height="16"/></clipPath><text class="table-name" x="{:.1}" y="{:.1}" clip-path="url(#node-{index}-header-clip)" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="13" font-weight="700" letter-spacing="0.02em" fill="{}">{}</text>"#,
        node.x + 10.0,
        node.y + 8.0,
        (node.width - 54.0).max(40.0),
        node.x + 10.0,
        node.y + 21.0,
        colors.text_primary,
        escape_text(&node.label)
    )?;
    write!(
        out,
        r#"<text class="table-kind" x="{:.1}" y="{:.1}" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="9" font-weight="600" text-anchor="end" letter-spacing="0.12em" fill="{}">{}</text>"#,
        node.x + node.width - 10.0,
        node.y + 21.0,
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

    let mut line_y = node.y + 46.0;
    for (column_index, column) in node.columns.iter().enumerate() {
        write!(
            out,
            r#"<g class="column-row" data-column-name="{}" data-nullable="{}">"#,
            escape_attribute(&column.name),
            column.flags.nullable
        )?;
        if index > 0 {
            let separator_y = line_y - 12.0;
            write!(
                out,
                r#"<line class="column-separator" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-opacity="0.38" stroke-width="1"/>"#,
                node.x + 10.0,
                separator_y,
                node.x + node.width - 10.0,
                separator_y,
                node_style.separator
            )?;
        }
        let font_style = if column.flags.nullable {
            r#" font-style="italic""#
        } else {
            ""
        };
        write!(
            out,
            r#"<clipPath id="node-{index}-column-{column_index}-clip"><rect x="{:.1}" y="{:.1}" width="{:.1}" height="16"/></clipPath><text class="column-name" x="{:.1}" y="{:.1}" clip-path="url(#node-{index}-column-{column_index}-clip)" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="11.5" fill="{}"{}>"#,
            node.x + 10.0,
            line_y - 12.5,
            column_text_width(node, column),
            node.x + 10.0,
            line_y,
            if column.flags.nullable {
                colors.text_muted
            } else {
                colors.text_secondary
            },
            font_style,
        )?;
        if node.kind == NodeKind::Enum {
            write!(out, "• {}", escape_text(&column.name))?;
        } else if column.data_type.is_empty() {
            write!(out, "{}", escape_text(&column.name))?;
        } else {
            write!(
                out,
                "{}: {}",
                escape_text(&column.name),
                escape_text(&column.data_type)
            )?;
        }
        out.push_str("</text>");

        let mut icon_x = node.x + node.width - 22.0;
        if column.flags.relation.is_indexed {
            render_idx_indicator(out, icon_x, line_y - 9.0)?;
            icon_x -= 24.0;
        }
        if column.flags.relation.is_foreign_key {
            render_fk_indicator(out, icon_x, line_y - 9.0)?;
            icon_x -= 24.0;
        }
        if column.flags.relation.is_primary_key {
            render_pk_indicator(out, icon_x, line_y - 8.5)?;
        }

        out.push_str("</g>");
        line_y += 18.0;
    }
    write!(
        out,
        r#"<rect class="type-filter-overlay" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="16" ry="16" fill="url(#type-filter-hatch)" opacity="0"/>"#,
        node.x, node.y, node.width, node.height
    )?;
    out.push_str("</g>");
    Ok(())
}
