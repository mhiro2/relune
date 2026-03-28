//! Edge routing for layout
//!
//! This module provides edge routing algorithms for drawing
//! connections between nodes in the layout.

use relune_core::layout::{EdgeRoute, RouteStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttachmentSide {
    North,
    South,
    East,
    West,
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
    match style {
        RouteStyle::Straight => route_straight(x1, y1, w1, h1, x2, y2, w2, h2, lane_offset),
        RouteStyle::Orthogonal => route_orthogonal(x1, y1, w1, h1, x2, y2, w2, h2, lane_offset),
        RouteStyle::Curved => route_curved(x1, y1, w1, h1, x2, y2, w2, h2, lane_offset),
    }
}

#[allow(clippy::too_many_arguments)]
fn route_straight(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
    lane_offset: f32,
) -> EdgeRoute {
    let ((sx, sy), (tx, ty), source_side, target_side) =
        attachment_points(x1, y1, w1, h1, x2, y2, w2, h2);
    let (sx, sy) = offset_attachment_point((sx, sy), source_side, lane_offset);
    let (tx, ty) = offset_attachment_point((tx, ty), target_side, lane_offset);

    let label_position = (f32::midpoint(sx, tx), f32::midpoint(sy, ty));

    EdgeRoute {
        x1: sx,
        y1: sy,
        x2: tx,
        y2: ty,
        control_points: Vec::new(),
        style: RouteStyle::Straight,
        label_position,
    }
}

#[allow(clippy::too_many_arguments)]
fn route_orthogonal(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
    lane_offset: f32,
) -> EdgeRoute {
    let ((sx, sy), (tx, ty), source_side, target_side) =
        attachment_points(x1, y1, w1, h1, x2, y2, w2, h2);
    let (sx, sy) = offset_attachment_point((sx, sy), source_side, lane_offset);
    let (tx, ty) = offset_attachment_point((tx, ty), target_side, lane_offset);

    let (control_points, label_position) =
        orthogonal_control_points((sx, sy), (tx, ty), source_side, target_side);

    EdgeRoute {
        x1: sx,
        y1: sy,
        x2: tx,
        y2: ty,
        control_points,
        style: RouteStyle::Orthogonal,
        label_position,
    }
}

#[allow(clippy::too_many_arguments)]
fn route_curved(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
    lane_offset: f32,
) -> EdgeRoute {
    let ((sx, sy), (tx, ty), source_side, target_side) =
        attachment_points(x1, y1, w1, h1, x2, y2, w2, h2);
    let (sx, sy) = offset_attachment_point((sx, sy), source_side, lane_offset);
    let (tx, ty) = offset_attachment_point((tx, ty), target_side, lane_offset);

    let offset = if source_side.is_horizontal() && target_side.is_horizontal() {
        ((tx - sx).abs() * 0.3).max(24.0)
    } else if source_side.is_vertical() && target_side.is_vertical() {
        ((ty - sy).abs() * 0.3).max(24.0)
    } else {
        (((tx - sx).abs() + (ty - sy).abs()) * 0.2).max(28.0)
    };

    let cp1 = step_from_attachment((sx, sy), source_side, offset);
    let cp2 = step_from_attachment((tx, ty), target_side, offset);

    // Label at the midpoint of the cubic bezier curve (t = 0.5)
    let label_position = calculate_cubic_bezier_midpoint(sx, sy, cp1, cp2, tx, ty);

    EdgeRoute {
        x1: sx,
        y1: sy,
        x2: tx,
        y2: ty,
        control_points: vec![cp1, cp2],
        style: RouteStyle::Curved,
        label_position,
    }
}

#[allow(clippy::too_many_arguments)]
fn attachment_points(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
) -> ((f32, f32), (f32, f32), AttachmentSide, AttachmentSide) {
    let source_center_x = x1 + w1 / 2.0;
    let source_center_y = y1 + h1 / 2.0;
    let target_center_x = x2 + w2 / 2.0;
    let target_center_y = y2 + h2 / 2.0;
    let dx = target_center_x - source_center_x;
    let dy = target_center_y - source_center_y;

    if dx.abs() >= dy.abs() {
        if dx >= 0.0 {
            (
                (x1 + w1, source_center_y),
                (x2, target_center_y),
                AttachmentSide::East,
                AttachmentSide::West,
            )
        } else {
            (
                (x1, source_center_y),
                (x2 + w2, target_center_y),
                AttachmentSide::West,
                AttachmentSide::East,
            )
        }
    } else if dy >= 0.0 {
        (
            (source_center_x, y1 + h1),
            (target_center_x, y2),
            AttachmentSide::South,
            AttachmentSide::North,
        )
    } else {
        (
            (source_center_x, y1),
            (target_center_x, y2 + h2),
            AttachmentSide::North,
            AttachmentSide::South,
        )
    }
}

/// Calculate the point at t=0.5 on a cubic bezier curve.
fn calculate_cubic_bezier_midpoint(
    x0: f32,
    y0: f32,
    cp1: (f32, f32),
    cp2: (f32, f32),
    x1: f32,
    y1: f32,
) -> (f32, f32) {
    // Using De Casteljau's algorithm at t = 0.5
    let t = 0.5;
    let one_minus_t = 1.0 - t;

    // First level interpolation
    let q0x = one_minus_t * x0 + t * cp1.0;
    let q0y = one_minus_t * y0 + t * cp1.1;
    let q1x = one_minus_t * cp1.0 + t * cp2.0;
    let q1y = one_minus_t * cp1.1 + t * cp2.1;
    let q2x = one_minus_t * cp2.0 + t * x1;
    let q2y = one_minus_t * cp2.1 + t * y1;

    // Second level interpolation
    let r0x = one_minus_t * q0x + t * q1x;
    let r0y = one_minus_t * q0y + t * q1y;
    let r1x = one_minus_t * q1x + t * q2x;
    let r1y = one_minus_t * q1y + t * q2y;

    // Final point at t = 0.5
    let x = one_minus_t * r0x + t * r1x;
    let y = one_minus_t * r0y + t * r1y;

    (x, y)
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
    // Self-loops go around the node
    let loop_radius = 20.0 + radius_offset.max(0.0);

    match style {
        RouteStyle::Straight => {
            // Straight lines don't make sense for self-loops, use curved instead
            route_self_loop_curved(x, y, w, h, loop_radius)
        }
        RouteStyle::Orthogonal => route_self_loop_orthogonal(x, y, w, h, loop_radius),
        RouteStyle::Curved => route_self_loop_curved(x, y, w, h, loop_radius),
    }
}

fn route_self_loop_orthogonal(x: f32, y: f32, w: f32, h: f32, radius: f32) -> EdgeRoute {
    // Start from right edge, go up, right, down, back to right edge
    let sx = x + w;
    let sy = h.mul_add(0.25, y);
    let tx = x + w;
    let ty = h.mul_add(0.75, y);

    // Label position at the middle of the loop (right of the node)
    let label_position = (sx + radius, f32::midpoint(sy, ty));

    EdgeRoute {
        x1: sx,
        y1: sy,
        x2: tx,
        y2: ty,
        control_points: vec![(sx + radius, sy), (sx + radius, ty)],
        style: RouteStyle::Orthogonal,
        label_position,
    }
}

fn route_self_loop_curved(x: f32, y: f32, w: f32, h: f32, radius: f32) -> EdgeRoute {
    let sx = x + w;
    let sy = h.mul_add(0.25, y);
    let tx = x + w;
    let ty = h.mul_add(0.75, y);

    let cp1 = (radius.mul_add(1.5, sx), radius.mul_add(-0.5, sy));
    let cp2 = (radius.mul_add(1.5, sx), radius.mul_add(0.5, ty));

    // Label at the midpoint of the cubic bezier curve
    let label_position = calculate_cubic_bezier_midpoint(sx, sy, cp1, cp2, tx, ty);

    EdgeRoute {
        x1: sx,
        y1: sy,
        x2: tx,
        y2: ty,
        control_points: vec![cp1, cp2],
        style: RouteStyle::Curved,
        label_position,
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

fn step_from_attachment(point: (f32, f32), side: AttachmentSide, distance: f32) -> (f32, f32) {
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

    const fn is_vertical(self) -> bool {
        matches!(self, Self::North | Self::South)
    }
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

/// Nudge a label position so it does not overlap any of the given rectangles.
///
/// The label is shifted outward along the perpendicular to the edge direction
/// until it clears all obstacles by at least `margin` pixels.
#[must_use]
pub fn nudge_label(
    label: (f32, f32),
    edge_start: (f32, f32),
    edge_end: (f32, f32),
    obstacles: &[Rect],
    margin: f32,
) -> (f32, f32) {
    // Estimated label bounding box (half-sizes).
    let label_half_w = 40.0;
    let label_half_h = 10.0;

    let overlaps = |lx: f32, ly: f32| -> bool {
        obstacles.iter().any(|r| {
            lx + label_half_w + margin > r.x
                && lx - label_half_w - margin < r.x + r.w
                && ly + label_half_h + margin > r.y
                && ly - label_half_h - margin < r.y + r.h
        })
    };

    if !overlaps(label.0, label.1) {
        return label;
    }

    // Perpendicular direction to the edge vector.
    let dx = edge_end.0 - edge_start.0;
    let dy = edge_end.1 - edge_start.1;
    let len = dx.hypot(dy).max(1.0);
    // Normal: rotate 90 degrees.
    let nx = -dy / len;
    let ny = dx / len;

    // Try shifting in both perpendicular directions with increasing step.
    #[allow(clippy::cast_precision_loss)]
    for step in 1..=8_i32 {
        let offset = step as f32 * 12.0;
        let candidate_pos = (label.0 + nx * offset, label.1 + ny * offset);
        if !overlaps(candidate_pos.0, candidate_pos.1) {
            return candidate_pos;
        }
        let candidate_neg = (label.0 - nx * offset, label.1 - ny * offset);
        if !overlaps(candidate_neg.0, candidate_neg.1) {
            return candidate_neg;
        }
    }

    // Fallback: return original position.
    label
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
        assert!(route.control_points.is_empty());
        assert_eq!(route.x1, 100.0); // Right edge of source
        assert_eq!(route.x2, 200.0); // Left edge of target
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

        assert!(route.control_points.len() >= 2);
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
        assert!((route.y1 - 60.0).abs() < 0.001);
        assert!((route.x2 - 70.0).abs() < 0.001);
        assert!((route.y2 - 200.0).abs() < 0.001);
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
            RouteStyle::Straight,
        );

        // Label should be at midpoint
        let expected_x = f32::midpoint(100.0, 200.0);
        let expected_y = f32::midpoint(25.0, 125.0);
        assert!((route.label_position.0 - expected_x).abs() < 0.001);
        assert!((route.label_position.1 - expected_y).abs() < 0.001);
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

        // Label should be at the middle vertical segment
        let mid_x = 100.0 + (200.0 - 100.0) / 2.0;
        let mid_y = f32::midpoint(25.0, 125.0);
        assert!((route.label_position.0 - mid_x).abs() < 0.001);
        assert!((route.label_position.1 - mid_y).abs() < 0.001);
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

        // Label should be at the bezier curve midpoint (t=0.5)
        // For a cubic bezier, the point at t=0.5 is calculated via De Casteljau's algorithm
        // The exact position depends on control points, but it should be between start and end
        assert!(route.label_position.0 > route.x1);
        assert!(route.label_position.0 < route.x2);
    }

    #[test]
    fn test_label_position_self_loop_orthogonal() {
        let route = route_self_loop(0.0, 0.0, 100.0, 50.0, RouteStyle::Orthogonal);

        // Self-loop starts and ends on right edge of node at x=100
        // Label should be to the right of the node
        assert!(route.label_position.0 > 100.0);
        // Label y should be between the start and end points
        let sy = 50.0 * 0.25; // y + h * 0.25
        let ty = 50.0 * 0.75; // y + h * 0.75
        assert!(route.label_position.1 > sy);
        assert!(route.label_position.1 < ty);
    }

    #[test]
    fn test_label_position_self_loop_curved() {
        let route = route_self_loop(0.0, 0.0, 100.0, 50.0, RouteStyle::Curved);

        // Self-loop starts and ends on right edge of node at x=100
        // Label should be to the right of the node
        assert!(route.label_position.0 > 100.0);
        // Label y should be roughly at the vertical center of the node
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
    fn test_nudge_label_no_overlap_returns_original() {
        let label = (150.0, 75.0);
        let obstacles = vec![Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        }];
        let result = nudge_label(label, (100.0, 25.0), (200.0, 125.0), &obstacles, 4.0);
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
        let result = nudge_label(label, (0.0, 25.0), (100.0, 25.0), &obstacles, 4.0);
        // Should have moved away from the obstacle
        assert!((result.0 - label.0).abs() > 0.001 || (result.1 - label.1).abs() > 0.001);
    }

    #[test]
    fn test_nudge_label_empty_obstacles() {
        let label = (50.0, 25.0);
        let result = nudge_label(label, (0.0, 25.0), (100.0, 25.0), &[], 4.0);
        assert!((result.0 - label.0).abs() < 0.001);
        assert!((result.1 - label.1).abs() < 0.001);
    }

    #[test]
    fn test_cubic_bezier_midpoint() {
        // Test the bezier midpoint calculation
        // For a cubic bezier curve at t=0.5:
        // B(0.5) = 0.125*P0 + 0.375*P1 + 0.375*P2 + 0.125*P3
        // With P0=(0,0), P1=(33,0), P2=(66,0), P3=(100,0):
        // x = 0.125*0 + 0.375*33 + 0.375*66 + 0.125*100 = 49.625
        let (x, y) =
            calculate_cubic_bezier_midpoint(0.0, 0.0, (33.0, 0.0), (66.0, 0.0), 100.0, 0.0);
        assert!((x - 49.625).abs() < 0.001);
        assert!(y.abs() < 0.001);

        // Test with symmetric control points at exactly 1/3 and 2/3
        // This should give exactly 50.0
        let (x, y) = calculate_cubic_bezier_midpoint(
            0.0,
            0.0,
            (100.0 / 3.0, 0.0),
            (200.0 / 3.0, 0.0),
            100.0,
            0.0,
        );
        assert!((x - 50.0).abs() < 0.001);
        assert!(y.abs() < 0.001);

        // Test with a curve that goes upward
        // From (0, 0) to (100, 0) with symmetric control points creating an arc
        let (x, y) =
            calculate_cubic_bezier_midpoint(0.0, 0.0, (25.0, 50.0), (75.0, 50.0), 100.0, 0.0);
        assert!((x - 50.0).abs() < 0.001);
        assert!(y > 0.0); // The midpoint should be above the baseline
    }
}
