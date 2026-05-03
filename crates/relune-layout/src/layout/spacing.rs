//! Node sizing, text width estimation, bounds, and shared rendering helpers.

use relune_core::{LayoutDirection, NodeKind};
use unicode_width::UnicodeWidthChar;

use super::{
    ColumnFlags, ColumnRelationFlags, LayoutConfig, NodeSize, PositionedColumn, PositionedEdge,
    PositionedNode,
};

/// Node header font size used for width estimation.
const HEADER_FONT_SIZE: f32 = 13.0;
/// Node column font size used for width estimation.
pub(super) const COLUMN_FONT_SIZE: f32 = 11.5;
/// Lower bound factor applied to configured node width.
const MIN_NODE_WIDTH_FACTOR: f32 = 0.72;
/// Extra right-side space for the kind label ("TABLE"/"VIEW"/"ENUM") in the header.
const HEADER_KIND_LABEL_RESERVE: f32 = 48.0;

pub(super) fn build_positioned_node(
    node: &crate::graph::LayoutNode,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    show_columns: bool,
) -> PositionedNode {
    PositionedNode {
        id: node.id.clone(),
        label: node.label.clone(),
        kind: node.kind,
        columns: if show_columns {
            node.columns
                .iter()
                .map(|c| PositionedColumn {
                    name: c.name.clone(),
                    data_type: c.data_type.clone(),
                    flags: ColumnFlags {
                        nullable: c.nullable,
                        relation: ColumnRelationFlags {
                            is_primary_key: c.is_primary_key,
                            is_foreign_key: c.is_foreign_key,
                            is_indexed: c.is_indexed,
                        },
                    },
                })
                .collect()
        } else {
            Vec::new()
        },
        x,
        y,
        width,
        height,
        is_join_table_candidate: node.is_join_table_candidate,
        has_self_loop: node.has_self_loop,
        group_index: node.group_index,
    }
}

pub(super) fn measure_node_sizes(
    graph: &crate::graph::LayoutGraph,
    config: &LayoutConfig,
) -> Vec<NodeSize> {
    graph
        .nodes
        .iter()
        .map(|node| NodeSize {
            width: estimate_node_width(node, config),
            height: estimate_node_height(node, config),
        })
        .collect()
}

fn estimate_node_width(node: &crate::graph::LayoutNode, config: &LayoutConfig) -> f32 {
    let minimum_width = (config.node_width * MIN_NODE_WIDTH_FACTOR).max(160.0);
    let header_width = config
        .node_padding
        .mul_add(2.0, estimate_text_width(&node.label, HEADER_FONT_SIZE))
        + HEADER_KIND_LABEL_RESERVE;
    if !config.show_columns {
        return header_width.max(minimum_width).ceil();
    }

    let column_width = node
        .columns
        .iter()
        .map(|column| {
            let text = display_column_text(node.kind, &column.name, &column.data_type);
            let text_px = estimate_text_width(&text, COLUMN_FONT_SIZE);
            let icon_slots = usize::from(column.is_indexed)
                + usize::from(column.is_foreign_key)
                + usize::from(column.is_primary_key);
            #[allow(clippy::cast_precision_loss)] // Icon counts are tiny layout values.
            let badge_reserve = if icon_slots > 0 {
                (icon_slots as f32 - 1.0).mul_add(24.0, 28.0)
            } else {
                0.0
            };
            text_px + badge_reserve
        })
        .fold(0.0, f32::max);

    header_width
        .max(config.node_padding.mul_add(2.0, column_width) + 10.0)
        .max(minimum_width)
        .ceil()
}

#[allow(clippy::cast_precision_loss)] // Layout sizing is approximate and bounded for diagram rendering.
#[allow(clippy::suboptimal_flops)]
#[allow(clippy::missing_const_for_fn)] // This helper stays non-const to avoid over-constraining floating-point layout code.
pub(super) fn estimate_node_height(node: &crate::graph::LayoutNode, config: &LayoutConfig) -> f32 {
    if !config.show_columns {
        return config
            .node_padding
            .mul_add(2.0, config.header_height)
            .ceil();
    }
    config
        .node_padding
        .mul_add(
            2.0,
            (node.columns.len() as f32).mul_add(config.column_height, config.header_height),
        )
        .ceil()
}

pub(super) fn compute_graph_bounds(
    positioned_nodes: &[PositionedNode],
    config: &LayoutConfig,
) -> (f32, f32) {
    let max_x = positioned_nodes
        .iter()
        .map(|node| node.x + node.width)
        .fold(config.origin_x, f32::max);
    let max_y = positioned_nodes
        .iter()
        .map(|node| node.y + node.height)
        .fold(config.origin_y, f32::max);

    (max_x + config.origin_x, max_y + config.origin_y)
}

/// Flips node coordinates for `BottomToTop` / `RightToLeft`, matching hierarchical
/// `assign_coordinates` so reversed directions are visually mirrored.
pub(super) fn mirror_positioned_nodes_for_direction(
    positioned_nodes: &mut [PositionedNode],
    graph_bounds: (f32, f32),
    direction: LayoutDirection,
) {
    match direction {
        LayoutDirection::BottomToTop => {
            for node in positioned_nodes.iter_mut() {
                node.y = graph_bounds.1 - node.y - node.height;
            }
        }
        LayoutDirection::RightToLeft => {
            for node in positioned_nodes.iter_mut() {
                node.x = graph_bounds.0 - node.x - node.width;
            }
        }
        LayoutDirection::TopToBottom | LayoutDirection::LeftToRight => {}
    }
}

/// Expand graph bounds so that edge routes (especially self-loop curves and
/// their control points) are not clipped by the SVG viewport.
pub(super) fn expand_bounds_for_edges(
    width: f32,
    height: f32,
    edges: &[PositionedEdge],
) -> (f32, f32) {
    const MARKER_PAD: f32 = 24.0; // room for Crow's Foot markers
    let mut w = width;
    let mut h = height;
    for edge in edges {
        let r = &edge.route;
        for &x in &[r.x1, r.x2] {
            if x + MARKER_PAD > w {
                w = x + MARKER_PAD;
            }
        }
        for &y in &[r.y1, r.y2] {
            if y + MARKER_PAD > h {
                h = y + MARKER_PAD;
            }
        }
        for &(cx, cy) in &r.control_points {
            if cx + MARKER_PAD > w {
                w = cx + MARKER_PAD;
            }
            if cy + MARKER_PAD > h {
                h = cy + MARKER_PAD;
            }
        }
    }
    (w, h)
}

pub(super) fn display_column_text(kind: NodeKind, name: &str, data_type: &str) -> String {
    if kind == NodeKind::Enum {
        format!("• {name}")
    } else if data_type.is_empty() {
        name.to_string()
    } else {
        format!("{name}: {data_type}")
    }
}

pub(super) fn estimate_text_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|ch| {
            let width_factor = match ch {
                'A'..='Z' => 0.72,
                'a'..='z' | '0'..='9' => 0.62,
                '_' | '-' | '.' | ':' | ',' | '(' | ')' | '[' | ']' | ' ' => 0.38,
                _ if ch.is_ascii_punctuation() => 0.52,
                _ if ch.is_ascii() => 0.62,
                _ => match ch.width_cjk().or_else(|| ch.width()) {
                    Some(0) => 0.0,
                    Some(1) => 0.94,
                    Some(_) => 1.12,
                    None => 1.0,
                },
            };
            font_size * width_factor
        })
        .sum()
}
