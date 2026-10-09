//! Node sizing, text width estimation, bounds, and shared rendering helpers.

use relune_core::LayoutDirection;

use super::spatial::BBox;
use super::{
    ColumnFlags, ColumnRelationFlags, LayoutConfig, NodeSize, PositionedColumn, PositionedEdge,
    PositionedGroup, PositionedNode,
};
use crate::metrics::{
    ColumnSlots, GROUP_LABEL_FONT_SIZE, GROUP_LABEL_INSET, GROUP_LABEL_LETTER_SPACING,
    NODE_COLLAPSE_CONTROL_RESERVE, NODE_COLUMN_HEIGHT, NODE_FIRST_ROW_TOP, NODE_HEADER_FONT_SIZE,
    NODE_HEADER_HEIGHT, NODE_HEADER_NAME_OFFSET, NODE_KIND_LABEL_RESERVE, NODE_ROWS_BOTTOM_PADDING,
    NODE_TEXT_INSET, column_row_width, estimate_mono_text_width, estimate_text_width,
};
use crate::route::{LABEL_HALF_H, Rect, estimate_label_half_width, route_points};

/// Lower bound factor applied to configured node width.
const MIN_NODE_WIDTH_FACTOR: f32 = 0.72;
/// Header width kept for the kind label, with a little slack beyond the
/// renderer's table-name clip so names are not cut at the estimate's edge.
const HEADER_KIND_LABEL_RESERVE: f32 = NODE_KIND_LABEL_RESERVE + 4.0;

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
                    flags: column_flags(c),
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

const fn column_flags(column: &crate::graph::LayoutColumn) -> ColumnFlags {
    ColumnFlags {
        nullable: column.nullable,
        relation: ColumnRelationFlags {
            is_primary_key: column.is_primary_key,
            is_foreign_key: column.is_foreign_key,
            is_indexed: column.is_indexed,
        },
    }
}

fn estimate_node_width(node: &crate::graph::LayoutNode, config: &LayoutConfig) -> f32 {
    let minimum_width = (config.node_width * MIN_NODE_WIDTH_FACTOR).max(160.0);
    let header_width = NODE_TEXT_INSET.mul_add(
        2.0,
        estimate_mono_text_width(&node.label, NODE_HEADER_FONT_SIZE),
    ) + NODE_HEADER_NAME_OFFSET
        + HEADER_KIND_LABEL_RESERVE
        + NODE_COLLAPSE_CONTROL_RESERVE;
    if !config.show_columns {
        return header_width.max(minimum_width).ceil();
    }

    let flags: Vec<ColumnFlags> = node.columns.iter().map(column_flags).collect();
    let slots = ColumnSlots::of(&flags);
    let column_width = node
        .columns
        .iter()
        .map(|column| column_row_width(slots, &column.name, &column.data_type))
        .fold(0.0, f32::max);

    header_width
        .max(NODE_TEXT_INSET.mul_add(2.0, column_width))
        .max(minimum_width)
        .ceil()
}

#[allow(clippy::cast_precision_loss)] // Layout sizing is approximate and bounded for diagram rendering.
pub(super) fn estimate_node_height(node: &crate::graph::LayoutNode, config: &LayoutConfig) -> f32 {
    if !config.show_columns {
        return NODE_HEADER_HEIGHT;
    }
    (node.columns.len() as f32)
        .mul_add(
            NODE_COLUMN_HEIGHT,
            NODE_FIRST_ROW_TOP + NODE_ROWS_BOTTOM_PADDING,
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
        // Group labels are not clipped to their container.
        #[allow(clippy::cast_precision_loss)] // Label lengths are small.
        let label_width = (group.label.chars().count() as f32).mul_add(
            GROUP_LABEL_LETTER_SPACING,
            estimate_text_width(&group.label, GROUP_LABEL_FONT_SIZE),
        );
        bounds.include_rect(&Rect {
            x: group.x,
            y: group.y,
            w: GROUP_LABEL_INSET + label_width + CANVAS_LABEL_PAD,
            h: 0.0,
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
