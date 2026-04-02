//! Edge routing for layout
//!
//! This module provides edge routing algorithms for drawing
//! connections between nodes in the layout.

mod geometry;
mod obstacle;

pub use geometry::point_along_route;
pub(crate) use geometry::{
    approximate_route_length, rebuild_route_from_points, route_points, sample_route_obstacles,
};
pub use obstacle::{detour_around_obstacles, nudge_label};

use geometry::{polyline_midpoint, simplify_orthogonal_path};
use relune_core::layout::{EdgeRoute, RouteStyle};

/// Outward offset (in pixels) applied to attachment points so that edge
/// endpoints and markers sit slightly outside the node border rather than
/// overlapping it.
const BORDER_OUTSET: f32 = 2.0;

/// Default half-width of an edge label bounding box (in pixels).
/// Used as a fallback when no label text is available.
pub const LABEL_HALF_W: f32 = 40.0;
/// Estimated half-height of an edge label bounding box (in pixels).
pub const LABEL_HALF_H: f32 = 10.0;

/// Estimate the half-width of a label bounding box from its text content.
///
/// Mirrors the character-width heuristic used by the SVG renderer
/// (`estimate_label_width` in relune-render-svg) so that the layout engine's
/// obstacle rectangles match the actual rendered label size.
#[must_use]
pub fn estimate_label_half_width(text: &str) -> f32 {
    let char_width: f32 = text
        .chars()
        .map(|ch| if ch.is_ascii() { 6.4 } else { 10.0 })
        .sum();
    // The SVG renderer adds 18px padding; half of the total width is the half-extent.
    (char_width + 18.0) * 0.5
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AttachmentSide {
    North,
    South,
    East,
    West,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ChannelAxis {
    X,
    Y,
}

/// A rectangle representing an obstacle on the canvas (typically a node).
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    /// Left edge X coordinate.
    pub x: f32,
    /// Top edge Y coordinate.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

/// Route an edge between two points.
///
/// This function calculates the path for an edge given the positions
/// and dimensions of the source and target nodes.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn route_edge(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
    style: RouteStyle,
) -> EdgeRoute {
    route_edge_with_offset(x1, y1, w1, h1, x2, y2, w2, h2, style, 0.0)
}

#[must_use]
#[allow(clippy::too_many_arguments)]
pub(crate) fn route_edge_with_offset(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
    style: RouteStyle,
    lane_offset: f32,
) -> EdgeRoute {
    let (source_side, target_side) = attachment_sides(x1, y1, w1, h1, x2, y2, w2, h2);
    route_edge_with_assigned_ports(
        x1,
        y1,
        w1,
        h1,
        x2,
        y2,
        w2,
        h2,
        style,
        source_side,
        target_side,
        lane_offset,
        lane_offset,
        0.0,
        0.0,
    )
}

/// Route an edge with separate per-endpoint lane offsets and column Y offsets.
///
/// `source_col_offset` / `target_col_offset` are Y deltas from node center
/// that align the attachment point with a specific column row.  They are only
/// applied for East/West (horizontal) attachments; for North/South they are
/// ignored because column alignment is not meaningful in that direction.
#[must_use]
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn route_edge_column_aware(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
    style: RouteStyle,
    source_lane_offset: f32,
    target_lane_offset: f32,
    source_col_offset: f32,
    target_col_offset: f32,
) -> EdgeRoute {
    let (source_side, target_side) = attachment_sides(x1, y1, w1, h1, x2, y2, w2, h2);
    route_edge_with_assigned_ports(
        x1,
        y1,
        w1,
        h1,
        x2,
        y2,
        w2,
        h2,
        style,
        source_side,
        target_side,
        source_lane_offset,
        target_lane_offset,
        source_col_offset,
        target_col_offset,
    )
}

#[must_use]
#[allow(clippy::too_many_arguments)]
pub(crate) fn route_edge_with_assigned_ports(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
    style: RouteStyle,
    source_side: AttachmentSide,
    target_side: AttachmentSide,
    source_lane_offset: f32,
    target_lane_offset: f32,
    source_col_offset: f32,
    target_col_offset: f32,
) -> EdgeRoute {
    let source = apply_endpoint_offsets(
        attachment_point_for_side(x1, y1, w1, h1, source_side),
        source_side,
        source_lane_offset,
        source_col_offset,
    );
    let target = apply_endpoint_offsets(
        attachment_point_for_side(x2, y2, w2, h2, target_side),
        target_side,
        target_lane_offset,
        target_col_offset,
    );
    build_backbone_route(source, target, source_side, target_side, style)
}

#[must_use]
#[allow(clippy::too_many_arguments)]
#[cfg_attr(not(test), allow(dead_code))] // The simple channel helper remains covered by route unit tests.
pub(crate) fn route_edge_with_simple_channel(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
    style: RouteStyle,
    source_side: AttachmentSide,
    target_side: AttachmentSide,
    source_lane_offset: f32,
    target_lane_offset: f32,
    source_col_offset: f32,
    target_col_offset: f32,
    channel_axis: ChannelAxis,
    channel_coordinate: f32,
) -> EdgeRoute {
    let source = apply_endpoint_offsets(
        attachment_point_for_side(x1, y1, w1, h1, source_side),
        source_side,
        source_lane_offset,
        source_col_offset,
    );
    let target = apply_endpoint_offsets(
        attachment_point_for_side(x2, y2, w2, h2, target_side),
        target_side,
        target_lane_offset,
        target_col_offset,
    );
    build_simple_channel_route(source, target, style, channel_axis, channel_coordinate)
}

/// Apply lane + column offsets to an attachment point.
///
/// Lane offset is always applied perpendicular to the attachment side.
/// Column offset is additionally applied to Y for East/West attachments.
fn apply_endpoint_offsets(
    point: (f32, f32),
    side: AttachmentSide,
    lane_offset: f32,
    col_offset: f32,
) -> (f32, f32) {
    let p = offset_attachment_point(point, side, lane_offset);
    if side.is_horizontal() {
        (p.0, p.1 + col_offset)
    } else {
        p
    }
}

#[allow(clippy::too_many_arguments)]
fn build_backbone_route(
    source: (f32, f32),
    target: (f32, f32),
    source_side: AttachmentSide,
    target_side: AttachmentSide,
    style: RouteStyle,
) -> EdgeRoute {
    let (control_points, _) = orthogonal_control_points(source, target, source_side, target_side);

    let mut points = Vec::with_capacity(control_points.len() + 2);
    points.push(source);
    points.extend(control_points);
    points.push(target);
    let points = simplify_orthogonal_path(&points);

    EdgeRoute {
        x1: points[0].0,
        y1: points[0].1,
        x2: points[points.len() - 1].0,
        y2: points[points.len() - 1].1,
        control_points: points[1..points.len() - 1].to_vec(),
        style,
        label_position: polyline_midpoint(&points),
    }
}

#[cfg_attr(not(test), allow(dead_code))] // Only exercised through the test-only helper above.
fn build_simple_channel_route(
    source: (f32, f32),
    target: (f32, f32),
    style: RouteStyle,
    channel_axis: ChannelAxis,
    channel_coordinate: f32,
) -> EdgeRoute {
    let (source_channel_turn, target_channel_turn, label_position) = match channel_axis {
        ChannelAxis::X => (
            (channel_coordinate, source.1),
            (channel_coordinate, target.1),
            (channel_coordinate, f32::midpoint(source.1, target.1)),
        ),
        ChannelAxis::Y => (
            (source.0, channel_coordinate),
            (target.0, channel_coordinate),
            (f32::midpoint(source.0, target.0), channel_coordinate),
        ),
    };

    let points =
        simplify_orthogonal_path(&[source, source_channel_turn, target_channel_turn, target]);
    let label_position = if points.len() >= 4 {
        label_position
    } else {
        polyline_midpoint(&points)
    };

    EdgeRoute {
        x1: points[0].0,
        y1: points[0].1,
        x2: points[points.len() - 1].0,
        y2: points[points.len() - 1].1,
        control_points: points[1..points.len() - 1].to_vec(),
        style,
        label_position,
    }
}

#[must_use]
#[allow(clippy::too_many_arguments)]
pub(crate) fn attachment_sides(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
) -> (AttachmentSide, AttachmentSide) {
    let source_center_x = x1 + w1 / 2.0;
    let source_center_y = y1 + h1 / 2.0;
    let target_center_x = x2 + w2 / 2.0;
    let target_center_y = y2 + h2 / 2.0;
    let dx = target_center_x - source_center_x;
    let dy = target_center_y - source_center_y;

    if dx.abs() >= dy.abs() {
        if dx >= 0.0 {
            (AttachmentSide::East, AttachmentSide::West)
        } else {
            (AttachmentSide::West, AttachmentSide::East)
        }
    } else if dy >= 0.0 {
        (AttachmentSide::South, AttachmentSide::North)
    } else {
        (AttachmentSide::North, AttachmentSide::South)
    }
}

fn attachment_point_for_side(x: f32, y: f32, w: f32, h: f32, side: AttachmentSide) -> (f32, f32) {
    let center_x = x + w / 2.0;
    let center_y = y + h / 2.0;
    match side {
        AttachmentSide::North => (center_x, y - BORDER_OUTSET),
        AttachmentSide::South => (center_x, y + h + BORDER_OUTSET),
        AttachmentSide::East => (x + w + BORDER_OUTSET, center_y),
        AttachmentSide::West => (x - BORDER_OUTSET, center_y),
    }
}

/// Route a self-loop edge.
#[must_use]
pub fn route_self_loop(x: f32, y: f32, w: f32, h: f32, style: RouteStyle) -> EdgeRoute {
    route_self_loop_with_offset(x, y, w, h, style, 0.0)
}

#[must_use]
pub(crate) fn route_self_loop_with_offset(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    style: RouteStyle,
    radius_offset: f32,
) -> EdgeRoute {
    // Self-loops go around the node.
    // Use a generous base radius so that Crow's Foot SVG markers (up to 20 px
    // wide) stay visually attached to the curved path instead of diverging
    // along the tangent.
    let loop_radius = 36.0 + radius_offset.max(0.0);

    let sx = x + w;
    let sy = h.mul_add(0.25, y);
    let tx = x + w;
    let ty = h.mul_add(0.75, y);
    let points = [
        (sx, sy),
        (sx + loop_radius, sy),
        (sx + loop_radius, ty),
        (tx, ty),
    ];

    EdgeRoute {
        x1: sx,
        y1: sy,
        x2: tx,
        y2: ty,
        control_points: points[1..points.len() - 1].to_vec(),
        style,
        label_position: polyline_midpoint(&points),
    }
}

fn offset_attachment_point(
    point: (f32, f32),
    side: AttachmentSide,
    lane_offset: f32,
) -> (f32, f32) {
    match side {
        AttachmentSide::East | AttachmentSide::West => (point.0, point.1 + lane_offset),
        AttachmentSide::North | AttachmentSide::South => (point.0 + lane_offset, point.1),
    }
}

pub(crate) fn step_from_attachment(
    point: (f32, f32),
    side: AttachmentSide,
    distance: f32,
) -> (f32, f32) {
    match side {
        AttachmentSide::East => (point.0 + distance, point.1),
        AttachmentSide::West => (point.0 - distance, point.1),
        AttachmentSide::North => (point.0, point.1 - distance),
        AttachmentSide::South => (point.0, point.1 + distance),
    }
}

fn orthogonal_control_points(
    source: (f32, f32),
    target: (f32, f32),
    source_side: AttachmentSide,
    target_side: AttachmentSide,
) -> (Vec<(f32, f32)>, (f32, f32)) {
    let (sx, sy) = source;
    let (tx, ty) = target;

    if matches!(
        (source_side, target_side),
        (AttachmentSide::East, AttachmentSide::West) | (AttachmentSide::West, AttachmentSide::East)
    ) {
        let mid_x = f32::midpoint(sx, tx);
        return (
            vec![(mid_x, sy), (mid_x, ty)],
            (mid_x, f32::midpoint(sy, ty)),
        );
    }

    if matches!(
        (source_side, target_side),
        (AttachmentSide::North, AttachmentSide::South)
            | (AttachmentSide::South, AttachmentSide::North)
    ) {
        let mid_y = f32::midpoint(sy, ty);
        return (
            vec![(sx, mid_y), (tx, mid_y)],
            (f32::midpoint(sx, tx), mid_y),
        );
    }

    let bend_offset = 28.0;
    let source_bend = step_from_attachment(source, source_side, bend_offset);
    let target_bend = step_from_attachment(target, target_side, bend_offset);
    let elbow = if source_side.is_horizontal() {
        (source_bend.0, target_bend.1)
    } else {
        (target_bend.0, source_bend.1)
    };

    (vec![source_bend, elbow, target_bend], elbow)
}

impl AttachmentSide {
    const fn is_horizontal(self) -> bool {
        matches!(self, Self::East | Self::West)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_route_straight() {
        let route = route_edge(
            0.0,
            0.0,
            100.0,
            50.0,
            200.0,
            100.0,
            100.0,
            50.0,
            RouteStyle::Straight,
        );

        assert_eq!(route.style, RouteStyle::Straight);
        assert_eq!(route.control_points.len(), 2);
        assert_eq!(route.x1, 100.0 + BORDER_OUTSET); // Right edge of source + outset
        assert_eq!(route.x2, 200.0 - BORDER_OUTSET); // Left edge of target - outset
    }

    #[test]
    fn test_route_orthogonal() {
        let route = route_edge(
            0.0,
            0.0,
            100.0,
            50.0,
            200.0,
            100.0,
            100.0,
            50.0,
            RouteStyle::Orthogonal,
        );

        assert_eq!(route.style, RouteStyle::Orthogonal);
        assert_eq!(route.control_points.len(), 2);
    }

    #[test]
    fn test_route_curved() {
        let route = route_edge(
            0.0,
            0.0,
            100.0,
            50.0,
            200.0,
            100.0,
            100.0,
            50.0,
            RouteStyle::Curved,
        );

        assert_eq!(route.style, RouteStyle::Curved);
        assert_eq!(route.control_points.len(), 2);
    }

    #[test]
    fn test_route_self_loop() {
        let route = route_self_loop(0.0, 0.0, 100.0, 50.0, RouteStyle::Curved);

        assert_eq!(route.control_points.len(), 2);
    }

    #[test]
    fn test_route_straight_with_lane_offset_shifts_horizontal_attachment() {
        let route = route_edge_with_offset(
            0.0,
            0.0,
            100.0,
            50.0,
            200.0,
            0.0,
            100.0,
            50.0,
            RouteStyle::Straight,
            12.0,
        );

        assert!((route.y1 - 37.0).abs() < 0.001);
        assert!((route.y2 - 37.0).abs() < 0.001);
    }

    #[test]
    fn test_orthogonal_control_points_support_mixed_attachment_sides() {
        let (control_points, label_position) = orthogonal_control_points(
            (100.0, 40.0),
            (160.0, 120.0),
            AttachmentSide::East,
            AttachmentSide::North,
        );

        assert_eq!(control_points.len(), 3);
        assert_eq!(label_position, (128.0, 92.0));
    }

    #[test]
    fn test_route_straight_uses_vertical_attachments_for_stacked_nodes() {
        let route = route_edge(
            0.0,
            0.0,
            120.0,
            60.0,
            10.0,
            200.0,
            120.0,
            60.0,
            RouteStyle::Straight,
        );

        assert!((route.x1 - 60.0).abs() < 0.001);
        assert!((route.y1 - (60.0 + BORDER_OUTSET)).abs() < 0.001); // bottom + outset
        assert!((route.x2 - 70.0).abs() < 0.001);
        assert!((route.y2 - (200.0 - BORDER_OUTSET)).abs() < 0.001); // top - outset
    }

    #[test]
    fn test_route_orthogonal_uses_horizontal_middle_segment_for_stacked_nodes() {
        let route = route_edge(
            0.0,
            0.0,
            120.0,
            60.0,
            10.0,
            200.0,
            120.0,
            60.0,
            RouteStyle::Orthogonal,
        );

        assert_eq!(route.control_points.len(), 2);
        assert!((route.control_points[0].0 - 60.0).abs() < 0.001);
        assert!((route.control_points[0].1 - 130.0).abs() < 0.001);
        assert!((route.control_points[1].0 - 70.0).abs() < 0.001);
        assert!((route.control_points[1].1 - 130.0).abs() < 0.001);
    }

    #[test]
    fn test_label_position_straight() {
        let route = route_edge(
            0.0,
            0.0,
            100.0,
            50.0,
            200.0,
            100.0,
            100.0,
            50.0,
            RouteStyle::Straight,
        );

        assert_eq!(route.label_position, (150.0, 75.0));
    }

    #[test]
    fn test_label_position_orthogonal() {
        // Source at (0, 0) with size (100, 50) -> edge starts at (100, 25)
        // Target at (200, 100) with size (100, 50) -> edge ends at (200, 125)
        let route = route_edge(
            0.0,
            0.0,
            100.0,
            50.0,
            200.0,
            100.0,
            100.0,
            50.0,
            RouteStyle::Orthogonal,
        );

        assert_eq!(route.label_position, (150.0, 75.0));
    }

    #[test]
    fn test_label_position_curved() {
        // Source at (0, 0) with size (100, 50) -> edge starts at (100, 25)
        // Target at (200, 100) with size (100, 50) -> edge ends at (200, 125)
        let route = route_edge(
            0.0,
            0.0,
            100.0,
            50.0,
            200.0,
            100.0,
            100.0,
            50.0,
            RouteStyle::Curved,
        );

        assert_eq!(route.label_position, (150.0, 75.0));
    }

    #[test]
    fn test_label_position_self_loop_orthogonal() {
        let route = route_self_loop(0.0, 0.0, 100.0, 50.0, RouteStyle::Orthogonal);

        assert!(route.label_position.0 > 100.0);
        let sy = 50.0 * 0.25; // y + h * 0.25
        let ty = 50.0 * 0.75; // y + h * 0.75
        assert!(route.label_position.1 > sy);
        assert!(route.label_position.1 < ty);
    }

    #[test]
    fn test_label_position_self_loop_curved() {
        let route = route_self_loop(0.0, 0.0, 100.0, 50.0, RouteStyle::Curved);

        assert!(route.label_position.0 > 100.0);
        let sy = 50.0 * 0.25; // y + h * 0.25
        let ty = 50.0 * 0.75; // y + h * 0.75
        assert!(route.label_position.1 > sy);
        assert!(route.label_position.1 < ty);
    }

    #[test]
    fn test_self_loop_offset_pushes_loop_farther_out() {
        let base = route_self_loop_with_offset(0.0, 0.0, 100.0, 50.0, RouteStyle::Orthogonal, 0.0);
        let offset =
            route_self_loop_with_offset(0.0, 0.0, 100.0, 50.0, RouteStyle::Orthogonal, 18.0);

        assert!(offset.control_points[0].0 > base.control_points[0].0);
        assert!(offset.label_position.0 > base.label_position.0);
    }

    #[test]
    fn test_detour_no_obstacles_returns_original() {
        let route = route_edge(
            0.0,
            0.0,
            100.0,
            50.0,
            300.0,
            0.0,
            100.0,
            50.0,
            RouteStyle::Straight,
        );
        let result = detour_around_obstacles(&route, &[]);
        assert_eq!(result.control_points.len(), route.control_points.len());
    }

    #[test]
    fn test_detour_adds_waypoints_when_edge_crosses_node() {
        // Edge goes straight through an obstacle in the middle.
        let route = EdgeRoute {
            x1: 0.0,
            y1: 100.0,
            x2: 400.0,
            y2: 100.0,
            control_points: Vec::new(),
            style: RouteStyle::Straight,
            label_position: (200.0, 100.0),
        };
        let obstacles = vec![Rect {
            x: 150.0,
            y: 60.0,
            w: 100.0,
            h: 80.0,
        }];
        let result = detour_around_obstacles(&route, &obstacles);
        assert!(
            result.control_points.len() > route.control_points.len(),
            "should add detour waypoints"
        );
        assert_eq!(result.style, RouteStyle::Straight);
    }

    #[test]
    fn test_detour_preserves_horizontal_endpoint_approach() {
        let route = EdgeRoute {
            x1: 1080.4,
            y1: 123.0,
            x2: 373.0,
            y2: 775.0,
            control_points: Vec::new(),
            style: RouteStyle::Straight,
            label_position: (726.7, 449.0),
        };
        let obstacles = vec![Rect {
            x: 630.2,
            y: 382.0,
            w: 315.0,
            h: 156.0,
        }];

        let result = obstacle::detour_around_obstacles_with_endpoints(
            &route,
            &obstacles,
            Some(AttachmentSide::West),
            Some(AttachmentSide::East),
        );

        let first = result
            .control_points
            .first()
            .expect("detour should add points");
        let last = result
            .control_points
            .last()
            .expect("detour should add points");

        assert!((first.1 - route.y1).abs() < 0.001);
        assert!(first.0 < route.x1);
        assert!((last.1 - route.y2).abs() < 0.001);
        assert!(last.0 > route.x2);
        assert!(
            result
                .control_points
                .iter()
                .any(|&(_, y)| y <= obstacles[0].y - 12.0),
            "detour should keep a visible gap from the obstacle"
        );
    }

    #[test]
    fn test_detour_curved_route_with_obstacle_keeps_render_style() {
        let route = route_edge(
            0.0,
            0.0,
            100.0,
            50.0,
            300.0,
            0.0,
            100.0,
            50.0,
            RouteStyle::Curved,
        );
        // Place an obstacle directly in the curve's path.
        let obstacles = vec![Rect {
            x: 150.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
        }];
        let result = detour_around_obstacles(&route, &obstacles);
        assert_eq!(result.style, RouteStyle::Curved);
        assert!(result.control_points.len() > route.control_points.len());
    }

    #[test]
    fn test_detour_curved_route_without_obstacle_stays_curved() {
        let route = route_edge(
            0.0,
            0.0,
            100.0,
            50.0,
            300.0,
            0.0,
            100.0,
            50.0,
            RouteStyle::Curved,
        );
        // Obstacle far away — should not affect the curve.
        let obstacles = vec![Rect {
            x: 500.0,
            y: 500.0,
            w: 50.0,
            h: 50.0,
        }];
        let result = detour_around_obstacles(&route, &obstacles);
        assert_eq!(result.style, RouteStyle::Curved);
        assert_eq!(result.control_points.len(), route.control_points.len());
    }

    #[test]
    fn test_simplify_removes_collinear_points() {
        use geometry::simplify_orthogonal_path;
        let points = vec![(0.0, 0.0), (0.0, 50.0), (0.0, 100.0)];
        let result = simplify_orthogonal_path(&points);
        assert_eq!(result, vec![(0.0, 0.0), (0.0, 100.0)]);
    }

    #[test]
    fn test_simplify_removes_backtracking() {
        use geometry::simplify_orthogonal_path;
        // The exact bug: path goes up then reverses down.
        let points = vec![
            (377.0, 206.0),
            (377.0, 188.0),
            (377.0, 152.0),
            (377.0, 431.0),
        ];
        let result = simplify_orthogonal_path(&points);
        assert_eq!(result, vec![(377.0, 206.0), (377.0, 431.0)]);
    }

    #[test]
    fn test_simplify_preserves_right_angle_bends() {
        use geometry::simplify_orthogonal_path;
        let points = vec![(0.0, 0.0), (0.0, 50.0), (100.0, 50.0)];
        let result = simplify_orthogonal_path(&points);
        assert_eq!(result, points);
    }

    #[test]
    fn test_simplify_full_bug_path() {
        use geometry::simplify_orthogonal_path;
        let points = vec![
            (2767.0, 105.0),
            (2739.0, 105.0),
            (2739.0, 152.0),
            (2739.0, 188.0),
            (2739.0, 206.0),
            (377.0, 206.0),
            (377.0, 188.0),
            (377.0, 152.0),
            (377.0, 431.0),
            (349.0, 431.0),
        ];
        let result = simplify_orthogonal_path(&points);
        assert_eq!(
            result,
            vec![
                (2767.0, 105.0),
                (2739.0, 105.0),
                (2739.0, 206.0),
                (377.0, 206.0),
                (377.0, 431.0),
                (349.0, 431.0),
            ]
        );
    }

    #[test]
    fn test_polyline_midpoint_two_points() {
        use geometry::polyline_midpoint;
        let mid = polyline_midpoint(&[(0.0, 0.0), (100.0, 0.0)]);
        assert!((mid.0 - 50.0).abs() < 0.001);
        assert!(mid.1.abs() < 0.001);
    }

    #[test]
    fn test_polyline_midpoint_three_points() {
        use geometry::polyline_midpoint;
        let mid = polyline_midpoint(&[(0.0, 0.0), (50.0, 0.0), (50.0, 50.0)]);
        // Total length = 50 + 50 = 100, midpoint at arc-length 50 = (50, 0)
        assert!((mid.0 - 50.0).abs() < 0.001);
        assert!(mid.1.abs() < 0.001);
    }

    #[test]
    fn test_nudge_label_no_overlap_returns_original() {
        let label = (150.0, 75.0);
        let obstacles = vec![Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        }];
        let result = nudge_label(
            label,
            (100.0, 25.0),
            (200.0, 125.0),
            &obstacles,
            4.0,
            LABEL_HALF_W,
            96.0,
        );
        assert!((result.0 - label.0).abs() < 0.001);
        assert!((result.1 - label.1).abs() < 0.001);
    }

    #[test]
    fn test_nudge_label_overlapping_is_moved_away() {
        // Label sits right on top of a node
        let label = (50.0, 25.0);
        let obstacles = vec![Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        }];
        let result = nudge_label(
            label,
            (0.0, 25.0),
            (100.0, 25.0),
            &obstacles,
            4.0,
            LABEL_HALF_W,
            96.0,
        );
        // Should have moved away from the obstacle
        assert!((result.0 - label.0).abs() > 0.001 || (result.1 - label.1).abs() > 0.001);
    }

    #[test]
    fn test_nudge_label_empty_obstacles() {
        let label = (50.0, 25.0);
        let result = nudge_label(
            label,
            (0.0, 25.0),
            (100.0, 25.0),
            &[],
            4.0,
            LABEL_HALF_W,
            96.0,
        );
        assert!((result.0 - label.0).abs() < 0.001);
        assert!((result.1 - label.1).abs() < 0.001);
    }

    #[test]
    fn test_nudge_label_respects_max_offset() {
        let label = (50.0, 25.0);
        let obstacles = vec![Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        }];

        let limited = nudge_label(
            label,
            (0.0, 25.0),
            (100.0, 25.0),
            &obstacles,
            4.0,
            LABEL_HALF_W,
            40.0,
        );
        assert_eq!(limited, label);

        let expanded = nudge_label(
            label,
            (0.0, 25.0),
            (100.0, 25.0),
            &obstacles,
            4.0,
            LABEL_HALF_W,
            48.0,
        );
        assert_ne!(expanded, label);
    }

    #[test]
    fn test_route_geometries_match_across_render_styles() {
        let straight = route_edge(
            0.0,
            0.0,
            100.0,
            50.0,
            200.0,
            100.0,
            100.0,
            50.0,
            RouteStyle::Straight,
        );
        let orthogonal = route_edge(
            0.0,
            0.0,
            100.0,
            50.0,
            200.0,
            100.0,
            100.0,
            50.0,
            RouteStyle::Orthogonal,
        );
        let curved = route_edge(
            0.0,
            0.0,
            100.0,
            50.0,
            200.0,
            100.0,
            100.0,
            50.0,
            RouteStyle::Curved,
        );

        assert_eq!(straight.control_points, orthogonal.control_points);
        assert_eq!(orthogonal.control_points, curved.control_points);
        assert_eq!(straight.label_position, orthogonal.label_position);
        assert_eq!(orthogonal.label_position, curved.label_position);
    }

    #[test]
    fn test_route_column_aware_shifts_y_for_horizontal_attachment() {
        // Two nodes side by side (dx > dy → East/West attachment).
        let route = route_edge_column_aware(
            0.0,
            0.0,
            100.0,
            100.0, // source
            200.0,
            0.0,
            100.0,
            100.0, // target
            RouteStyle::Straight,
            0.0,  // source lane
            0.0,  // target lane
            10.0, // source col offset (Y shift)
            -5.0, // target col offset (Y shift)
        );

        // Without column offset, center_y would be 50.0.
        // y1 = 50 + 10 = 60, y2 = 50 + (-5) = 45.
        assert!(
            (route.y1 - 60.0).abs() < 0.01,
            "source Y should be shifted by col offset"
        );
        assert!(
            (route.y2 - 45.0).abs() < 0.01,
            "target Y should be shifted by col offset"
        );
    }

    #[test]
    fn test_route_column_aware_ignores_col_offset_for_vertical_attachment() {
        // Two nodes stacked (dy > dx → North/South attachment).
        let route = route_edge_column_aware(
            50.0,
            0.0,
            100.0,
            50.0, // source
            50.0,
            200.0,
            100.0,
            50.0, // target
            RouteStyle::Straight,
            0.0,  // source lane
            0.0,  // target lane
            20.0, // source col offset — should be IGNORED for N/S
            20.0, // target col offset — should be IGNORED for N/S
        );

        // For South attachment, x = center_x = 100; y = 50 + BORDER_OUTSET = 52.
        // Column offset should not be applied.
        assert!(
            (route.x1 - 100.0).abs() < 0.01,
            "source X should be center, got {}",
            route.x1
        );
        assert!(
            (route.y1 - (50.0 + BORDER_OUTSET)).abs() < 0.01,
            "source Y should be bottom + outset, got {}",
            route.y1
        );
    }

    #[test]
    fn test_simple_channel_route_places_label_on_channel_midpoint() {
        let route = route_edge_with_simple_channel(
            0.0,
            0.0,
            100.0,
            60.0,
            200.0,
            200.0,
            100.0,
            60.0,
            RouteStyle::Orthogonal,
            AttachmentSide::South,
            AttachmentSide::North,
            0.0,
            0.0,
            0.0,
            0.0,
            ChannelAxis::Y,
            130.0,
        );

        assert_eq!(route.control_points, vec![(50.0, 130.0), (250.0, 130.0)]);
        assert_eq!(route.label_position, (150.0, 130.0));
    }
}
