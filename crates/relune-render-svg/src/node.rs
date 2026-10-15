//! SVG node (table / view / enum) rendering.

use std::fmt::{self, Write};

use relune_core::ChangeKind;
use relune_core::NodeKind;
use relune_layout::metrics::{
    CHANGE_MARKER_CENTER, COLUMN_BADGE_HEIGHT, COLUMN_BADGE_WIDTH, COLUMN_NULLABLE_GAP,
    COLUMN_NULLABLE_MARKER, COLUMN_NULLABLE_SLOT_WIDTH, ColumnSlots, NODE_COLUMN_FONT_SIZE,
    NODE_COLUMN_HEIGHT, NODE_CORNER_RADIUS, NODE_DETAIL_FONT_SIZE, NODE_FIRST_ROW_TOP,
    NODE_HEADER_BASELINE, NODE_HEADER_FONT_SIZE, NODE_HEADER_HEIGHT, NODE_HEADER_NAME_OFFSET,
    NODE_KIND_LABEL_RESERVE, NODE_KIND_MARK_SIZE, NODE_ROW_BASELINE, NODE_TEXT_INSET,
    TYPE_CHANGE_SEPARATOR, omitted_columns_label, shows_omitted_columns_row,
};
use relune_layout::{NodeOverlay, PositionedColumn};

use crate::diff::{
    REMOVED_DASHARRAY, change_class, change_color, change_marker, render_change_marker,
    render_change_tint, render_risk_label, risk_class,
};
use crate::escape::{escape_attribute, escape_text};
use crate::is_light_theme;
use crate::theme::{BadgeColor, ThemeColors};

// ---------------------------------------------------------------------------
// Node style
// ---------------------------------------------------------------------------

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

/// Color of the small kind mark in a node header, the only place a card
/// carries its kind's hue.
pub(crate) const fn node_kind_accent(kind: NodeKind, colors: &ThemeColors) -> &'static str {
    match (kind, is_light_theme(colors)) {
        (NodeKind::Table, false) => "#fb923c",
        (NodeKind::Table, true) => "#ea580c",
        (NodeKind::View, false) => "#2dd4bf",
        (NodeKind::View, true) => "#0d9488",
        (NodeKind::Enum, false) => "#c084fc",
        (NodeKind::Enum, true) => "#9333ea",
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

/// Column badge kinds, ordered by how much a reader relies on them.
#[derive(Clone, Copy)]
pub(crate) enum ColumnBadge {
    PrimaryKey,
    ForeignKey,
    Index,
}

impl ColumnBadge {
    const fn label(self) -> &'static str {
        match self {
            Self::PrimaryKey => "PK",
            Self::ForeignKey => "FK",
            Self::Index => "IX",
        }
    }

    const fn colors(self, colors: &ThemeColors) -> BadgeColor {
        match self {
            Self::PrimaryKey => colors.badges.primary_key,
            Self::ForeignKey => colors.badges.foreign_key,
            Self::Index => colors.badges.index,
        }
    }
}

/// Draws one PK / FK / IX badge with its top-left corner at (`x`, `y`).
pub(crate) fn render_column_badge(
    out: &mut String,
    x: f32,
    y: f32,
    badge: ColumnBadge,
    colors: &ThemeColors,
) -> fmt::Result {
    let BadgeColor {
        fill,
        fill_opacity_percent,
        text,
    } = badge.colors(colors);
    let fill_opacity = f32::from(fill_opacity_percent) / 100.0;
    write!(
        out,
        r#"<rect class="col-badge" x="{x:.1}" y="{y:.1}" width="{COLUMN_BADGE_WIDTH}" height="{COLUMN_BADGE_HEIGHT}" rx="3" fill="{fill}" fill-opacity="{fill_opacity}"/><text x="{:.1}" y="{:.1}" text-anchor="middle" font-family="'JetBrains Mono', ui-monospace, monospace" font-size="8.5" font-weight="700" fill="{text}">{}</text>"#,
        x + COLUMN_BADGE_WIDTH / 2.0,
        y + 9.5,
        badge.label(),
    )
}

// ---------------------------------------------------------------------------
// Column rows
// ---------------------------------------------------------------------------

/// Horizontal positions shared by every column row of one node.
struct RowGeometry {
    /// Left edge of the card.
    card_x: f32,
    /// Width of the card.
    card_width: f32,
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
            card_x: node.x,
            card_width: node.width,
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
    column: &PositionedColumn,
    change: Option<ChangeKind>,
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
        r#"<g class="column-row{}{}" data-column-name="{}" data-nullable="{}">"#,
        if change.is_some() { " " } else { "" },
        change.map_or("", change_class),
        escape_attribute(&column.name),
        column.flags.nullable
    )?;
    if let Some(change) = change {
        // The tint stays inside the border; the marker sits in the text inset.
        render_change_tint(
            out,
            (
                geometry.card_x + 1.0,
                row_top,
                geometry.card_width - 2.0,
                NODE_COLUMN_HEIGHT,
            ),
            None,
            change,
            colors,
        )?;
        render_change_marker(
            out,
            geometry.card_x + CHANGE_MARKER_CENTER,
            baseline,
            NODE_DETAIL_FONT_SIZE,
            change,
            colors,
        )?;
    }
    if relation.is_primary_key {
        render_column_badge(
            out,
            geometry.key_x,
            badge_y,
            ColumnBadge::PrimaryKey,
            colors,
        )?;
    }
    if relation.is_foreign_key {
        render_column_badge(
            out,
            geometry.key_x + geometry.slots.foreign_key_offset(),
            badge_y,
            ColumnBadge::ForeignKey,
            colors,
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
            r#"<text class="column-type" x="{:.1}" y="{baseline:.1}" clip-path="url(#{clip_id})" text-anchor="end" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="{NODE_DETAIL_FONT_SIZE}" fill="{}">"#,
            geometry.type_end_x, colors.text_muted,
        )?;
        // A changed type reads `previous → current`, with the current type
        // in the stronger column-name color.
        match &column.previous_data_type {
            Some(previous) => write!(
                out,
                r#"<tspan class="column-type-previous">{}</tspan>{TYPE_CHANGE_SEPARATOR}<tspan class="column-type-current" fill="{}">{}</tspan>"#,
                escape_text(previous),
                colors.text_secondary,
                escape_text(&column.data_type)
            )?,
            None => out.push_str(&escape_text(&column.data_type)),
        }
        out.push_str("</text>");
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
        render_column_badge(out, geometry.index_x, badge_y, ColumnBadge::Index, colors)?;
    }
    out.push_str("</g>");
    Ok(())
}

/// Renders the row counting the columns a card leaves out.
fn render_omitted_columns_row(
    out: &mut String,
    omitted: usize,
    geometry: &RowGeometry,
    row_top: f32,
    colors: &ThemeColors,
) -> fmt::Result {
    write!(
        out,
        r#"<text class="omitted-columns" x="{:.1}" y="{:.1}" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="{NODE_DETAIL_FONT_SIZE}" fill="{}">{}</text>"#,
        geometry.name_x,
        row_top + NODE_ROW_BASELINE,
        colors.text_muted,
        escape_text(&omitted_columns_label(omitted))
    )
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
    overlay: Option<&NodeOverlay>,
) -> fmt::Result {
    let kind = node_kind_name(node.kind);
    let node_label = node_kind_label(node.kind);
    let change = overlay.and_then(NodeOverlay::change_kind);
    let top_risk = overlay.and_then(NodeOverlay::top_risk);

    let mut state_classes = String::new();
    if let Some(change) = change {
        state_classes.push(' ');
        state_classes.push_str(change_class(change));
    }
    if let Some((severity, _)) = top_risk {
        state_classes.push(' ');
        state_classes.push_str(risk_class(severity));
    }

    write!(
        out,
        r#"<g class="table-node node node-kind-{}{}" data-table-id="{}" data-id="{}" data-node-kind="{}" style="--enter-delay:{:.3}s">"#,
        kind,
        state_classes,
        escape_attribute(&node.id),
        escape_attribute(&node.id),
        kind,
        index as f32 * 0.022
    )?;

    // Add tooltip if enabled (with overlay annotations appended)
    if show_tooltips {
        let column_count = node.columns.len() + node.omitted_columns;
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
        if let Some(node_change) = overlay.and_then(|o| o.change.as_ref()) {
            tooltip_parts.push(String::new());
            tooltip_parts.push(format!(
                "{} {}",
                change_marker(node_change.kind),
                node_change.summary
            ));
            tooltip_parts.extend(
                node_change
                    .details
                    .iter()
                    .map(|detail| format!("  {detail}")),
            );
        }
        if let Some(node_overlay) = overlay
            && !node_overlay.annotations.is_empty()
        {
            tooltip_parts.push(String::new());
            for annotation in &node_overlay.annotations {
                tooltip_parts.push(format!("[{}] {}", annotation.severity, annotation.message));
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

    // A changed card is outlined in its change kind's color; a removed one
    // also gets a dashed outline.
    let (stroke_color, stroke_width) = match change {
        Some(change) => (change_color(change, colors), "1.5"),
        None => (colors.node_stroke, "1"),
    };
    let stroke_dash = if change == Some(ChangeKind::Removed) {
        format!(r#" stroke-dasharray="{REMOVED_DASHARRAY}""#)
    } else {
        String::new()
    };

    write!(
        out,
        r#"<rect class="table-body" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{NODE_CORNER_RADIUS}" ry="{NODE_CORNER_RADIUS}" fill="{}" stroke="{stroke_color}" stroke-width="{stroke_width}"{stroke_dash}/>"#,
        node.x, node.y, node.width, node.height, colors.node_fill
    )?;
    // The header tint is clipped to the card's rounded shape, inset so it
    // never covers the border.
    write!(
        out,
        r#"<clipPath id="node-{index}-card-clip"><rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{:.1}"/></clipPath><rect class="table-header" x="{:.1}" y="{:.1}" width="{:.1}" height="{NODE_HEADER_HEIGHT}" clip-path="url(#node-{index}-card-clip)" fill="{}"/>"#,
        node.x + 0.5,
        node.y + 0.5,
        node.width - 1.0,
        node.height - 1.0,
        NODE_CORNER_RADIUS - 0.5,
        node.x,
        node.y,
        node.width,
        colors.header_fill
    )?;
    if !node.columns.is_empty() {
        write!(
            out,
            r#"<line class="table-header-divider" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-opacity="0.6" stroke-width="1"/>"#,
            node.x,
            node.y + NODE_HEADER_HEIGHT,
            node.x + node.width,
            node.y + NODE_HEADER_HEIGHT,
            colors.node_stroke
        )?;
    }
    // A card added or removed as a whole is tinted throughout; a modified
    // card tints only its changed rows.
    if let Some(change @ (ChangeKind::Added | ChangeKind::Removed)) = change {
        let card_clip = format!("node-{index}-card-clip");
        render_change_tint(
            out,
            (node.x, node.y, node.width, node.height),
            Some(&card_clip),
            change,
            colors,
        )?;
    }
    if let Some(change) = change {
        render_change_marker(
            out,
            node.x + CHANGE_MARKER_CENTER,
            node.y + NODE_HEADER_BASELINE,
            NODE_COLUMN_FONT_SIZE,
            change,
            colors,
        )?;
    }
    write!(
        out,
        r#"<rect class="table-kind-mark" x="{:.1}" y="{:.1}" width="{NODE_KIND_MARK_SIZE}" height="{NODE_KIND_MARK_SIZE}" rx="2" fill="{}"/>"#,
        node.x + NODE_TEXT_INSET,
        node.y + (NODE_HEADER_HEIGHT - NODE_KIND_MARK_SIZE) / 2.0,
        node_kind_accent(node.kind, colors)
    )?;
    let name_x = node.x + NODE_TEXT_INSET + NODE_HEADER_NAME_OFFSET;
    write!(
        out,
        r#"<clipPath id="node-{index}-header-clip"><rect x="{name_x:.1}" y="{:.1}" width="{:.1}" height="{NODE_HEADER_HEIGHT}"/></clipPath><text class="table-name" x="{name_x:.1}" y="{:.1}" clip-path="url(#node-{index}-header-clip)" font-family="'JetBrains Mono', 'Fira Code', ui-monospace, monospace" font-size="{NODE_HEADER_FONT_SIZE}" font-weight="700" fill="{}">{}</text>"#,
        node.y,
        (node.x + node.width - NODE_KIND_LABEL_RESERVE - name_x).max(40.0),
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

    // The risk label names the highest review severity and how many
    // findings carry it, apart from the change kind's colors.
    if let Some((severity, count)) = top_risk {
        render_risk_label(out, node.x + node.width, node.y, severity, count, colors)?;
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
            let column_change = overlay
                .and_then(|o| o.column_changes.get(&column.name))
                .map(|column_change| column_change.kind);
            render_column_row(
                out,
                column,
                column_change,
                &geometry,
                row_top,
                &clip_id,
                colors,
            )?;
            row_top += NODE_COLUMN_HEIGHT;
        }
        if shows_omitted_columns_row(node.columns.len(), node.omitted_columns) {
            render_omitted_columns_row(out, node.omitted_columns, &geometry, row_top, colors)?;
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
