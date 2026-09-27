//! Node sizing, text width estimation, bounds, and shared rendering helpers.

use relune_core::{LayoutDirection, NodeKind};
use unicode_width::UnicodeWidthChar;

use super::spatial::BBox;
use super::{
    ColumnFlags, ColumnRelationFlags, LayoutConfig, NodeSize, PositionedColumn, PositionedEdge,
    PositionedGroup, PositionedNode,
};
use crate::route::{LABEL_HALF_H, Rect, estimate_label_half_width, route_points};

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

/// Room kept around route points for Crow's Foot markers.
const CANVAS_MARKER_PAD: f32 = 24.0;
/// Room kept around edge labels.
const CANVAS_LABEL_PAD: f32 = 4.0;

/// Fits the canvas to everything that is drawn: nodes, groups, edge routes
/// (bypass lanes, self-loops and markers) and edge labels.
///
/// Routing may place bypass lanes, self-loops or labels left of or above the
/// node area; the whole drawing is then shifted so its top-left content stays
/// at a non-negative coordinate, since renderers use a `0 0 width height`
/// viewport. Returns the resulting canvas size.
pub(super) fn fit_canvas_to_content(
    width: f32,
    height: f32,
    nodes: &mut [PositionedNode],
    edges: &mut [PositionedEdge],
    groups: &mut [PositionedGroup],
) -> (f32, f32) {
    let mut bounds = BBox::from_points(&[(0.0, 0.0), (width, height)]);
    for node in nodes.iter() {
        bounds.include_rect(&Rect {
            x: node.x,
            y: node.y,
            w: node.width,
            h: node.height,
        });
    }
    for group in groups.iter() {
        bounds.include_rect(&Rect {
            x: group.x,
            y: group.y,
            w: group.width,
            h: group.height,
        });
    }
    for edge in edges.iter() {
        for point in route_points(&edge.route) {
            bounds.include_rect(&Rect {
                x: point.0 - CANVAS_MARKER_PAD,
                y: point.1 - CANVAS_MARKER_PAD,
                w: CANVAS_MARKER_PAD * 2.0,
                h: CANVAS_MARKER_PAD * 2.0,
            });
        }
        let half_w = estimate_label_half_width(&edge.label) + CANVAS_LABEL_PAD;
        let half_h = LABEL_HALF_H + CANVAS_LABEL_PAD;
        bounds.include_rect(&Rect {
            x: edge.label_x - half_w,
            y: edge.label_y - half_h,
            w: half_w * 2.0,
            h: half_h * 2.0,
        });
    }

    // Left/top overflow shifts the drawing; the existing origin margin on the
    // right/bottom is kept by measuring from the unshifted bounds.
    let dx = (-bounds.min_x).max(0.0);
    let dy = (-bounds.min_y).max(0.0);
    if dx > 0.0 || dy > 0.0 {
        translate_drawing(nodes, edges, groups, dx, dy);
    }
    (bounds.max_x + dx, bounds.max_y + dy)
}

fn translate_drawing(
    nodes: &mut [PositionedNode],
    edges: &mut [PositionedEdge],
    groups: &mut [PositionedGroup],
    dx: f32,
    dy: f32,
) {
    let shift = |point: &mut (f32, f32)| {
        point.0 += dx;
        point.1 += dy;
    };
    for node in nodes {
        node.x += dx;
        node.y += dy;
    }
    for group in groups {
        group.x += dx;
        group.y += dy;
    }
    for edge in edges {
        let route = &mut edge.route;
        route.x1 += dx;
        route.y1 += dy;
        route.x2 += dx;
        route.y2 += dy;
        route.control_points.iter_mut().for_each(shift);
        shift(&mut route.label_position);
        edge.label_x += dx;
        edge.label_y += dy;
        if let Some(debug) = edge.routing_debug.as_mut()
            && let Some(coordinate) = debug.channel_coordinate.as_mut()
        {
            match debug.channel_axis.as_deref() {
                Some("x") => *coordinate += dx,
                Some("y") => *coordinate += dy,
                _ => {}
            }
        }
    }
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
