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
/// Step size used when probing alternate label positions.
const LABEL_NUDGE_STEP: f32 = 12.0;
/// Minimum clearance preserved when rerouting around node obstacles.
const DETOUR_PADDING: f32 = 12.0;
/// Extra obstacle inflation used when deciding whether an edge is too close to a node.
const ROUTE_CLEARANCE: f32 = 14.0;

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
    max_offset: f32,
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
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let max_steps = (max_offset / LABEL_NUDGE_STEP).floor() as usize;
    #[allow(clippy::cast_precision_loss)]
    for step in 1..=max_steps {
        let offset = step as f32 * LABEL_NUDGE_STEP;
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
/// If any segment of the backbone polyline (start → control points → end)
/// passes through an obstacle, additional waypoints are inserted to route
/// around it.
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
    if obstacles.is_empty() {
        return route.clone();
    }

    let initial_points = route_points(route);
    if !route_intersects_obstacles(&initial_points, obstacles) {
        return route.clone();
    }

    let mut points = add_endpoint_anchors(&initial_points, source_side, target_side);

    // Iteratively detour until no segments intersect obstacles (max 4 passes).
    for _ in 0..4 {
        let mut detoured = Vec::with_capacity(points.len() * 2);
        detoured.push(points[0]);

        for i in 0..points.len() - 1 {
            let seg_start = points[i];
            let seg_end = points[i + 1];

            // Find the nearest intersecting obstacle along this segment.
            let nearest = obstacles
                .iter()
                .filter(|r| {
                    let clearance = inflate_rect(**r, ROUTE_CLEARANCE);
                    segment_intersects_rect(seg_start, seg_end, &clearance)
                })
                .min_by(|a, b| {
                    let da = (a.w.mul_add(0.5, a.x) - seg_start.0)
                        .hypot(a.h.mul_add(0.5, a.y) - seg_start.1);
                    let db = (b.w.mul_add(0.5, b.x) - seg_start.0)
                        .hypot(b.h.mul_add(0.5, b.y) - seg_start.1);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                });

            if let Some(obstacle) = nearest {
                let clearance = inflate_rect(*obstacle, ROUTE_CLEARANCE);
                let waypoints = compute_detour(seg_start, seg_end, &clearance);
                detoured.extend_from_slice(&waypoints);
            }
            detoured.push(seg_end);
        }

        points = detoured;

        if !route_intersects_obstacles(&points, obstacles) {
            break;
        }
    }

    // Clean up collinear and backtracking waypoints produced by the detour loop.
    points = simplify_orthogonal_path(&points);

    // Rebuild the route: first and last points are endpoints, middle are control points.
    let new_start = points[0];
    let new_end = points[points.len() - 1];
    let new_control_points: Vec<(f32, f32)> = points[1..points.len() - 1].to_vec();

    // Recompute label position at the midpoint of the new polyline.
    let label_position = polyline_midpoint(&points);

    EdgeRoute {
        x1: new_start.0,
        y1: new_start.1,
        x2: new_end.0,
        y2: new_end.1,
        control_points: new_control_points,
        style: route.style,
        label_position,
    }
}

/// Returns the full route polyline including endpoints.
#[must_use]
pub(crate) fn route_points(route: &EdgeRoute) -> Vec<(f32, f32)> {
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
            .map(|obstacle| inflate_rect(*obstacle, ROUTE_CLEARANCE))
            .any(|obstacle| segment_intersects_rect(segment[0], segment[1], &obstacle))
    })
}

fn inflate_rect(rect: Rect, padding: f32) -> Rect {
    Rect {
        x: rect.x - padding,
        y: rect.y - padding,
        w: padding.mul_add(2.0, rect.w),
        h: padding.mul_add(2.0, rect.h),
    }
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
///
/// Uses the Liang-Barsky algorithm for exact analytical intersection.
fn segment_intersects_rect(a: (f32, f32), b: (f32, f32), r: &Rect) -> bool {
    let margin = 2.0;
    let rx = r.x + margin;
    let ry = r.y + margin;
    let rw = 2.0f32.mul_add(-margin, r.w).max(0.0);
    let rh = 2.0f32.mul_add(-margin, r.h).max(0.0);

    // Quick AABB test: if the segment's bounding box doesn't overlap the rect, skip.
    let seg_min_x = a.0.min(b.0);
    let seg_max_x = a.0.max(b.0);
    let seg_min_y = a.1.min(b.1);
    let seg_max_y = a.1.max(b.1);
    if seg_max_x < rx || seg_min_x > rx + rw || seg_max_y < ry || seg_min_y > ry + rh {
        return false;
    }

    // Liang-Barsky: parameterize segment as P(t) = a + t*(b-a), t in [0,1].
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;

    let mut t_enter: f32 = 0.0;
    let mut t_leave: f32 = 1.0;

    // Clip against each of the 4 rectangle edges.
    // p = -dx, q = a.x - rx  (left edge)
    // p =  dx, q = rx+rw - a.x (right edge)
    // p = -dy, q = a.y - ry  (top edge)
    // p =  dy, q = ry+rh - a.y (bottom edge)
    let clips: [(f32, f32); 4] = [
        (-dx, a.0 - rx),
        (dx, rx + rw - a.0),
        (-dy, a.1 - ry),
        (dy, ry + rh - a.1),
    ];

    for &(p, q) in &clips {
        if p.abs() < 1e-9 {
            // Segment is parallel to this edge.
            if q < 0.0 {
                return false; // Outside and parallel — no intersection.
            }
            // Otherwise inside this slab — continue checking other edges.
        } else {
            let t = q / p;
            if p < 0.0 {
                // Entering the rectangle from this edge.
                if t > t_enter {
                    t_enter = t;
                }
            } else {
                // Leaving the rectangle from this edge.
                if t < t_leave {
                    t_leave = t;
                }
            }
            if t_enter > t_leave {
                return false;
            }
        }
    }

    true
}

/// Compute detour waypoints around a single obstacle rectangle.
///
/// Picks the side of the obstacle that requires the smallest deviation
/// from the segment direction and routes orthogonally around it.
#[allow(clippy::suboptimal_flops)]
fn compute_detour(start: (f32, f32), end: (f32, f32), obstacle: &Rect) -> Vec<(f32, f32)> {
    let cx = obstacle.x + obstacle.w * 0.5;
    let cy = obstacle.y + obstacle.h * 0.5;
    let mid_x = (start.0 + end.0) * 0.5;
    let mid_y = (start.1 + end.1) * 0.5;
    let is_vertical = (start.0 - end.0).abs() < 0.5;
    let is_horizontal = (start.1 - end.1).abs() < 0.5;

    if is_vertical {
        let detour_x = if start.0 < cx {
            obstacle.x - DETOUR_PADDING
        } else {
            obstacle.x + obstacle.w + DETOUR_PADDING
        };
        return vec![(detour_x, start.1), (detour_x, end.1)];
    }

    if is_horizontal {
        let detour_y = if start.1 < cy {
            obstacle.y - DETOUR_PADDING
        } else {
            obstacle.y + obstacle.h + DETOUR_PADDING
        };
        return vec![(start.0, detour_y), (end.0, detour_y)];
    }

    // Decide whether to go around the top/bottom or left/right of the obstacle.
    let dx = (mid_x - cx).abs();
    let dy = (mid_y - cy).abs();

    if dx > dy {
        // Horizontal deviation is larger — route above or below.
        let detour_y = if mid_y < cy {
            obstacle.y - DETOUR_PADDING
        } else {
            obstacle.y + obstacle.h + DETOUR_PADDING
        };
        vec![(start.0, detour_y), (end.0, detour_y)]
    } else {
        // Vertical deviation is larger — route left or right.
        let detour_x = if mid_x < cx {
            obstacle.x - DETOUR_PADDING
        } else {
            obstacle.x + obstacle.w + DETOUR_PADDING
        };
        vec![(detour_x, start.1), (detour_x, end.1)]
    }
}

/// Approximate a route with small axis-aligned obstacle samples.
///
/// This is primarily used during label placement so labels can avoid other
/// edge paths without needing exact curve/segment intersection tests.
#[must_use]
pub(crate) fn sample_route_obstacles(route: &EdgeRoute, half_size: f32, spacing: f32) -> Vec<Rect> {
    let safe_spacing = spacing.max(1.0);
    let route_length = approximate_route_length(route).max(safe_spacing);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let sample_count = (route_length / safe_spacing).ceil() as usize;
    let sample_count = sample_count.max(1);

    let mut obstacles = Vec::with_capacity(sample_count + 1);
    #[allow(clippy::cast_precision_loss)]
    for index in 0..=sample_count {
        let t = index as f32 / sample_count as f32;
        let (x, y) = point_along_route(route, t);
        obstacles.push(Rect {
            x: x - half_size,
            y: y - half_size,
            w: half_size * 2.0,
            h: half_size * 2.0,
        });
    }
    obstacles
}

/// Returns the total polyline length of the routed edge.
#[must_use]
pub(crate) fn approximate_route_length(route: &EdgeRoute) -> f32 {
    route_points(route)
        .windows(2)
        .map(|segment| {
            let dx = segment[1].0 - segment[0].0;
            let dy = segment[1].1 - segment[0].1;
            dx.hypot(dy)
        })
        .sum()
}

/// Simplify an orthogonal path by removing collinear and backtracking points.
///
/// Pass 1: remove intermediate points that are collinear with their neighbors
/// (all three share the same X or same Y coordinate).
/// Pass 2: remove backtracking — when three consecutive points lie on the same
/// axis and the direction reverses, the middle point is a stub. Repeat until
/// stable.
fn simplify_orthogonal_path(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    // Pass 1: remove collinear intermediate points.
    let mut result: Vec<(f32, f32)> = Vec::with_capacity(points.len());
    result.push(points[0]);
    for i in 1..points.len() - 1 {
        let prev = *result.last().unwrap();
        let curr = points[i];
        let next = points[i + 1];
        let same_x = (prev.0 - curr.0).abs() < 0.5 && (curr.0 - next.0).abs() < 0.5;
        let same_y = (prev.1 - curr.1).abs() < 0.5 && (curr.1 - next.1).abs() < 0.5;
        if same_x || same_y {
            continue;
        }
        result.push(curr);
    }
    result.push(*points.last().unwrap());

    // Pass 2: remove backtracking points (iterate until stable).
    loop {
        let prev_len = result.len();
        let mut cleaned: Vec<(f32, f32)> = Vec::with_capacity(result.len());
        cleaned.push(result[0]);

        let mut i = 1;
        while i < result.len() - 1 {
            let a = *cleaned.last().unwrap();
            let b = result[i];
            let c = result[i + 1];

            let on_x = (a.0 - b.0).abs() < 0.5 && (b.0 - c.0).abs() < 0.5;
            let on_y = (a.1 - b.1).abs() < 0.5 && (b.1 - c.1).abs() < 0.5;

            if on_x && (b.1 - a.1) * (c.1 - b.1) < 0.0 {
                // Vertical backtrack — skip b.
                i += 1;
                continue;
            }
            if on_y && (b.0 - a.0) * (c.0 - b.0) < 0.0 {
                // Horizontal backtrack — skip b.
                i += 1;
                continue;
            }

            cleaned.push(b);
            i += 1;
        }
        if i < result.len() {
            cleaned.push(*result.last().unwrap());
        }

        result = cleaned;
        if result.len() == prev_len {
            break;
        }
    }

    result
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

/// Compute a point at parameter `t` (0..1) along an edge route.
///
/// `t` is interpreted as a fraction of the backbone's total arc length.
#[must_use]
pub fn point_along_route(route: &EdgeRoute, t: f32) -> (f32, f32) {
    point_along_polyline(&route_points(route), t)
}

/// Compute a point at fraction `t` (0..1) of total arc length along a polyline.
fn point_along_polyline(points: &[(f32, f32)], t: f32) -> (f32, f32) {
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

    let target = total_length * t;
    let mut accumulated = 0.0;

    for w in points.windows(2) {
        let dx = w[1].0 - w[0].0;
        let dy = w[1].1 - w[0].1;
        let seg_len = dx.hypot(dy);
        if accumulated + seg_len >= target && seg_len > 0.0 {
            let frac = (target - accumulated) / seg_len;
            return (dx.mul_add(frac, w[0].0), dy.mul_add(frac, w[0].1));
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
        assert!(
            result
                .control_points
                .iter()
                .any(|&(_, y)| y <= obstacles[0].y - DETOUR_PADDING),
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
        let points = vec![(0.0, 0.0), (0.0, 50.0), (0.0, 100.0)];
        let result = simplify_orthogonal_path(&points);
        assert_eq!(result, vec![(0.0, 0.0), (0.0, 100.0)]);
    }

    #[test]
    fn test_simplify_removes_backtracking() {
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
        let points = vec![(0.0, 0.0), (0.0, 50.0), (100.0, 50.0)];
        let result = simplify_orthogonal_path(&points);
        assert_eq!(result, points);
    }

    #[test]
    fn test_simplify_full_bug_path() {
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
