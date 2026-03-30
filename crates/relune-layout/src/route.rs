//! Edge routing for layout
//!
//! This module provides edge routing algorithms for drawing
//! connections between nodes in the layout.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentSide {
    North,
    South,
    East,
    West,
}

/// Short outward segment length used to preserve endpoint approach direction
/// when obstacle detours insert extra bends into a route.
const ENDPOINT_ANCHOR_DISTANCE: f32 = 28.0;

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

/// Route an edge with separate per-endpoint lane offsets and column Y offsets.
///
/// `source_col_offset` / `target_col_offset` are Y deltas from node center
/// that align the attachment point with a specific column row.  They are only
/// applied for East/West (horizontal) attachments; for North/South they are
/// ignored because column alignment is not meaningful in that direction.
#[must_use]
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
    match style {
        RouteStyle::Straight => route_straight_col(
            x1,
            y1,
            w1,
            h1,
            x2,
            y2,
            w2,
            h2,
            source_lane_offset,
            target_lane_offset,
            source_col_offset,
            target_col_offset,
        ),
        RouteStyle::Orthogonal => route_orthogonal_col(
            x1,
            y1,
            w1,
            h1,
            x2,
            y2,
            w2,
            h2,
            source_lane_offset,
            target_lane_offset,
            source_col_offset,
            target_col_offset,
        ),
        RouteStyle::Curved => route_curved_col(
            x1,
            y1,
            w1,
            h1,
            x2,
            y2,
            w2,
            h2,
            source_lane_offset,
            target_lane_offset,
            source_col_offset,
            target_col_offset,
        ),
    }
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
fn route_straight_col(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
    src_lane: f32,
    tgt_lane: f32,
    src_col: f32,
    tgt_col: f32,
) -> EdgeRoute {
    let ((sx, sy), (tx, ty), ss, ts) = attachment_points(x1, y1, w1, h1, x2, y2, w2, h2);
    let (sx, sy) = apply_endpoint_offsets((sx, sy), ss, src_lane, src_col);
    let (tx, ty) = apply_endpoint_offsets((tx, ty), ts, tgt_lane, tgt_col);

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
fn route_orthogonal_col(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
    src_lane: f32,
    tgt_lane: f32,
    src_col: f32,
    tgt_col: f32,
) -> EdgeRoute {
    let ((sx, sy), (tx, ty), ss, ts) = attachment_points(x1, y1, w1, h1, x2, y2, w2, h2);
    let (sx, sy) = apply_endpoint_offsets((sx, sy), ss, src_lane, src_col);
    let (tx, ty) = apply_endpoint_offsets((tx, ty), ts, tgt_lane, tgt_col);

    let (control_points, label_position) = orthogonal_control_points((sx, sy), (tx, ty), ss, ts);

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
fn route_curved_col(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
    src_lane: f32,
    tgt_lane: f32,
    src_col: f32,
    tgt_col: f32,
) -> EdgeRoute {
    let ((sx, sy), (tx, ty), ss, ts) = attachment_points(x1, y1, w1, h1, x2, y2, w2, h2);
    let (sx, sy) = apply_endpoint_offsets((sx, sy), ss, src_lane, src_col);
    let (tx, ty) = apply_endpoint_offsets((tx, ty), ts, tgt_lane, tgt_col);

    let offset = if ss.is_horizontal() && ts.is_horizontal() {
        ((tx - sx).abs() * 0.3).max(24.0)
    } else if ss.is_vertical() && ts.is_vertical() {
        ((ty - sy).abs() * 0.3).max(24.0)
    } else {
        (((tx - sx).abs() + (ty - sy).abs()) * 0.2).max(28.0)
    };

    let cp1 = step_from_attachment((sx, sy), ss, offset);
    let cp2 = step_from_attachment((tx, ty), ts, offset);

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
                (x1 + w1 + BORDER_OUTSET, source_center_y),
                (x2 - BORDER_OUTSET, target_center_y),
                AttachmentSide::East,
                AttachmentSide::West,
            )
        } else {
            (
                (x1 - BORDER_OUTSET, source_center_y),
                (x2 + w2 + BORDER_OUTSET, target_center_y),
                AttachmentSide::West,
                AttachmentSide::East,
            )
        }
    } else if dy >= 0.0 {
        (
            (source_center_x, y1 + h1 + BORDER_OUTSET),
            (target_center_x, y2 - BORDER_OUTSET),
            AttachmentSide::South,
            AttachmentSide::North,
        )
    } else {
        (
            (source_center_x, y1 - BORDER_OUTSET),
            (target_center_x, y2 + h2 + BORDER_OUTSET),
            AttachmentSide::North,
            AttachmentSide::South,
        )
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
    let (_, _, source_side, target_side) = attachment_points(x1, y1, w1, h1, x2, y2, w2, h2);
    (source_side, target_side)
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
    // Self-loops go around the node.
    // Use a generous base radius so that Crow's Foot SVG markers (up to 20 px
    // wide) stay visually attached to the curved path instead of diverging
    // along the tangent.
    let loop_radius = 36.0 + radius_offset.max(0.0);

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
///
/// `label_half_w` controls the estimated half-width of the label bounding box.
/// Pass [`estimate_label_half_width`] of the label text for an accurate estimate,
/// or [`LABEL_HALF_W`] as a default.
#[must_use]
pub fn nudge_label(
    label: (f32, f32),
    edge_start: (f32, f32),
    edge_end: (f32, f32),
    obstacles: &[Rect],
    margin: f32,
    label_half_w: f32,
) -> (f32, f32) {
    let label_half_h = LABEL_HALF_H;

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

/// Detour an edge route around obstacle rectangles.
///
/// If any segment of the polyline (start → control points → end) passes through
/// an obstacle, additional waypoints are inserted to route around it.  Only
/// `Orthogonal` and `Straight` styles are adjusted; curved routes are left as-is
/// because modifying Bézier control points would distort the curve shape.
#[must_use]
pub fn detour_around_obstacles(route: &EdgeRoute, obstacles: &[Rect]) -> EdgeRoute {
    detour_around_obstacles_with_endpoints(route, obstacles, None, None)
}

#[must_use]
pub(crate) fn detour_around_obstacles_with_endpoints(
    route: &EdgeRoute,
    obstacles: &[Rect],
    source_side: Option<AttachmentSide>,
    target_side: Option<AttachmentSide>,
) -> EdgeRoute {
    if obstacles.is_empty() || route.style == RouteStyle::Curved {
        return route.clone();
    }

    let points = route_points(route);
    if !route_intersects_obstacles(&points, obstacles) {
        return route.clone();
    }

    let points = add_endpoint_anchors(&points, source_side, target_side);

    let mut detoured = Vec::with_capacity(points.len() * 2);
    detoured.push(points[0]);

    for i in 0..points.len() - 1 {
        let seg_start = points[i];
        let seg_end = points[i + 1];

        if let Some(obstacle) = obstacles
            .iter()
            .find(|r| segment_intersects_rect(seg_start, seg_end, r))
        {
            // Route around the obstacle by going to the nearest side.
            let waypoints = compute_detour(seg_start, seg_end, obstacle);
            detoured.extend_from_slice(&waypoints);
        }
        detoured.push(seg_end);
    }

    // Rebuild the route: first and last points are endpoints, middle are control points.
    let new_start = detoured[0];
    let new_end = detoured[detoured.len() - 1];
    let new_control_points: Vec<(f32, f32)> = detoured[1..detoured.len() - 1].to_vec();

    // Recompute label position at the midpoint of the new polyline.
    let label_position = polyline_midpoint(&detoured);

    let style = if route.style == RouteStyle::Straight && !new_control_points.is_empty() {
        RouteStyle::Orthogonal
    } else {
        route.style
    };

    EdgeRoute {
        x1: new_start.0,
        y1: new_start.1,
        x2: new_end.0,
        y2: new_end.1,
        control_points: new_control_points,
        style,
        label_position,
    }
}

fn route_points(route: &EdgeRoute) -> Vec<(f32, f32)> {
    let mut points: Vec<(f32, f32)> = Vec::with_capacity(route.control_points.len() + 2);
    points.push((route.x1, route.y1));
    points.extend_from_slice(&route.control_points);
    points.push((route.x2, route.y2));
    points
}

fn route_intersects_obstacles(points: &[(f32, f32)], obstacles: &[Rect]) -> bool {
    points.windows(2).any(|segment| {
        obstacles
            .iter()
            .any(|obstacle| segment_intersects_rect(segment[0], segment[1], obstacle))
    })
}

fn add_endpoint_anchors(
    points: &[(f32, f32)],
    source_side: Option<AttachmentSide>,
    target_side: Option<AttachmentSide>,
) -> Vec<(f32, f32)> {
    let Some((&start, rest)) = points.split_first() else {
        return Vec::new();
    };
    let Some((&end, middle)) = rest.split_last() else {
        return vec![start];
    };

    let mut anchored = Vec::with_capacity(points.len() + 2);
    anchored.push(start);

    if let Some(side) = source_side {
        anchored.push(step_from_attachment(start, side, ENDPOINT_ANCHOR_DISTANCE));
    }

    anchored.extend_from_slice(middle);

    if let Some(side) = target_side {
        anchored.push(step_from_attachment(end, side, ENDPOINT_ANCHOR_DISTANCE));
    }

    anchored.push(end);
    anchored
}

/// Check whether a line segment from `a` to `b` passes through the interior
/// of a rectangle (with a small inset margin to ignore edges that merely
/// touch the boundary).
#[allow(clippy::suboptimal_flops)]
fn segment_intersects_rect(a: (f32, f32), b: (f32, f32), r: &Rect) -> bool {
    let margin = 2.0;
    let rx = r.x + margin;
    let ry = r.y + margin;
    let rw = (r.w - 2.0 * margin).max(0.0);
    let rh = (r.h - 2.0 * margin).max(0.0);

    // Quick AABB test: if the segment's bounding box doesn't overlap the rect, skip.
    let seg_min_x = a.0.min(b.0);
    let seg_max_x = a.0.max(b.0);
    let seg_min_y = a.1.min(b.1);
    let seg_max_y = a.1.max(b.1);
    if seg_max_x < rx || seg_min_x > rx + rw || seg_max_y < ry || seg_min_y > ry + rh {
        return false;
    }

    // Check if the midpoint of the segment lies inside the rect (fast common case).
    let mx = (a.0 + b.0) * 0.5;
    let my = (a.1 + b.1) * 0.5;
    if mx > rx && mx < rx + rw && my > ry && my < ry + rh {
        return true;
    }

    // Cohen–Sutherland style: test a few sample points along the segment.
    for &t in &[0.25, 0.5, 0.75] {
        let px = a.0 + (b.0 - a.0) * t;
        let py = a.1 + (b.1 - a.1) * t;
        if px > rx && px < rx + rw && py > ry && py < ry + rh {
            return true;
        }
    }

    false
}

/// Compute detour waypoints around a single obstacle rectangle.
///
/// Picks the side of the obstacle that requires the smallest deviation
/// from the segment direction and routes orthogonally around it.
#[allow(clippy::suboptimal_flops)]
fn compute_detour(start: (f32, f32), end: (f32, f32), obstacle: &Rect) -> Vec<(f32, f32)> {
    let padding = 12.0;
    let cx = obstacle.x + obstacle.w * 0.5;
    let cy = obstacle.y + obstacle.h * 0.5;
    let mid_x = (start.0 + end.0) * 0.5;
    let mid_y = (start.1 + end.1) * 0.5;

    // Decide whether to go around the top/bottom or left/right of the obstacle.
    let dx = (mid_x - cx).abs();
    let dy = (mid_y - cy).abs();

    if dx > dy {
        // Horizontal deviation is larger — route above or below.
        let detour_y = if mid_y < cy {
            obstacle.y - padding
        } else {
            obstacle.y + obstacle.h + padding
        };
        vec![(start.0, detour_y), (end.0, detour_y)]
    } else {
        // Vertical deviation is larger — route left or right.
        let detour_x = if mid_x < cx {
            obstacle.x - padding
        } else {
            obstacle.x + obstacle.w + padding
        };
        vec![(detour_x, start.1), (detour_x, end.1)]
    }
}

/// Find the midpoint of a polyline (by arc-length).
#[allow(clippy::suboptimal_flops)]
fn polyline_midpoint(points: &[(f32, f32)]) -> (f32, f32) {
    if points.len() < 2 {
        return points.first().copied().unwrap_or((0.0, 0.0));
    }

    let total_length: f32 = points
        .windows(2)
        .map(|w| {
            let dx = w[1].0 - w[0].0;
            let dy = w[1].1 - w[0].1;
            dx.hypot(dy)
        })
        .sum();

    let half = total_length * 0.5;
    let mut accumulated = 0.0;

    for w in points.windows(2) {
        let dx = w[1].0 - w[0].0;
        let dy = w[1].1 - w[0].1;
        let seg_len = dx.hypot(dy);
        if accumulated + seg_len >= half && seg_len > 0.0 {
            let t = (half - accumulated) / seg_len;
            return (w[0].0 + dx * t, w[0].1 + dy * t);
        }
        accumulated += seg_len;
    }

    *points.last().unwrap()
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
        // Upgraded to orthogonal since waypoints were inserted.
        assert_eq!(result.style, RouteStyle::Orthogonal);
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

        let result = detour_around_obstacles_with_endpoints(
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
    }

    #[test]
    fn test_detour_curved_routes_unchanged() {
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
        let obstacles = vec![Rect {
            x: 150.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
        }];
        let result = detour_around_obstacles(&route, &obstacles);
        assert_eq!(result.control_points.len(), route.control_points.len());
    }

    #[test]
    fn test_polyline_midpoint_two_points() {
        let mid = polyline_midpoint(&[(0.0, 0.0), (100.0, 0.0)]);
        assert!((mid.0 - 50.0).abs() < 0.001);
        assert!(mid.1.abs() < 0.001);
    }

    #[test]
    fn test_polyline_midpoint_three_points() {
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
        );
        // Should have moved away from the obstacle
        assert!((result.0 - label.0).abs() > 0.001 || (result.1 - label.1).abs() > 0.001);
    }

    #[test]
    fn test_nudge_label_empty_obstacles() {
        let label = (50.0, 25.0);
        let result = nudge_label(label, (0.0, 25.0), (100.0, 25.0), &[], 4.0, LABEL_HALF_W);
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
}
