//! Obstacle avoidance for edge routing.
//!
//! Label nudging, edge detour around node rectangles, and segment–rectangle
//! intersection tests.

use relune_core::layout::EdgeRoute;

use super::geometry::{polyline_midpoint, route_points, simplify_orthogonal_path};
use super::{AttachmentSide, Rect, step_from_attachment};

/// Step size used when probing alternate label positions.
const LABEL_NUDGE_STEP: f32 = 12.0;
/// Minimum clearance preserved when rerouting around node obstacles.
const DETOUR_PADDING: f32 = 12.0;
/// Extra obstacle inflation used when deciding whether an edge is too close to a node.
const ROUTE_CLEARANCE: f32 = 14.0;
/// Short outward segment length used to preserve endpoint approach direction
/// when obstacle detours insert extra bends into a route.
const ENDPOINT_ANCHOR_DISTANCE: f32 = 28.0;

/// Nudge a label position so it does not overlap any of the given rectangles.
///
/// The label is shifted outward along the perpendicular to the edge direction
/// until it clears all obstacles by at least `margin` pixels.
///
/// `label_half_w` controls the estimated half-width of the label bounding box.
/// Pass [`super::estimate_label_half_width`] of the label text for an accurate estimate,
/// or [`super::LABEL_HALF_W`] as a default.
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
    let label_half_h = super::LABEL_HALF_H;
    let dx = edge_end.0 - edge_start.0;
    let dy = edge_end.1 - edge_start.1;
    let len = dx.hypot(dy).max(1.0);
    let normal = (-dy / len, dx / len);
    let relevant_obstacles = relevant_label_obstacles(
        label,
        obstacles,
        margin,
        label_half_w,
        label_half_h,
        max_offset,
        normal,
    );
    if relevant_obstacles.is_empty() {
        return label;
    }

    let overlaps = |lx: f32, ly: f32| -> bool {
        relevant_obstacles.iter().any(|r| {
            lx + label_half_w + margin > r.x
                && lx - label_half_w - margin < r.x + r.w
                && ly + label_half_h + margin > r.y
                && ly - label_half_h - margin < r.y + r.h
        })
    };

    if !overlaps(label.0, label.1) {
        return label;
    }

    // Normal: rotate 90 degrees.
    let (nx, ny) = normal;

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

fn relevant_label_obstacles(
    label: (f32, f32),
    obstacles: &[Rect],
    margin: f32,
    label_half_w: f32,
    label_half_h: f32,
    max_offset: f32,
    normal: (f32, f32),
) -> Vec<Rect> {
    let offset_x = normal.0 * max_offset;
    let offset_y = normal.1 * max_offset;
    let extent_x = label_half_w + margin;
    let extent_y = label_half_h + margin;
    let sweep_min_x = (label.0 - offset_x).min(label.0 + offset_x) - extent_x;
    let sweep_max_x = (label.0 - offset_x).max(label.0 + offset_x) + extent_x;
    let sweep_min_y = (label.1 - offset_y).min(label.1 + offset_y) - extent_y;
    let sweep_max_y = (label.1 - offset_y).max(label.1 + offset_y) + extent_y;

    obstacles
        .iter()
        .copied()
        .filter(|obstacle| {
            obstacle.x < sweep_max_x
                && obstacle.x + obstacle.w > sweep_min_x
                && obstacle.y < sweep_max_y
                && obstacle.y + obstacle.h > sweep_min_y
        })
        .collect()
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

    let clearance_obstacles = inflate_obstacles(obstacles, ROUTE_CLEARANCE);
    let initial_points = route_points(route);
    if !route_intersects_obstacles(&initial_points, &clearance_obstacles) {
        return route.clone();
    }

    let mut points = add_endpoint_anchors(&initial_points, source_side, target_side);

    // Iteratively detour until no segments intersect obstacles (max 4 passes).
    for _ in 0..4 {
        let mut detoured = Vec::with_capacity(points.len() * 2);
        detoured.push(points[0]);
        let mut changed = false;

        for i in 0..points.len() - 1 {
            let seg_start = points[i];
            let seg_end = points[i + 1];

            // Find the nearest intersecting obstacle along this segment.
            let nearest = nearest_intersecting_obstacle(seg_start, seg_end, &clearance_obstacles);

            if let Some(obstacle) = nearest {
                let waypoints = compute_detour(seg_start, seg_end, obstacle);
                detoured.extend_from_slice(&waypoints);
                changed = true;
            }
            detoured.push(seg_end);
        }

        if !changed {
            break;
        }

        points = detoured;

        if !route_intersects_obstacles(&points, &clearance_obstacles) {
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

fn route_intersects_obstacles(points: &[(f32, f32)], obstacles: &[Rect]) -> bool {
    points.windows(2).any(|segment| {
        obstacles
            .iter()
            .any(|obstacle| segment_intersects_rect(segment[0], segment[1], obstacle))
    })
}

fn inflate_obstacles(obstacles: &[Rect], padding: f32) -> Vec<Rect> {
    obstacles
        .iter()
        .copied()
        .map(|obstacle| inflate_rect(obstacle, padding))
        .collect()
}

fn inflate_rect(rect: Rect, padding: f32) -> Rect {
    Rect {
        x: rect.x - padding,
        y: rect.y - padding,
        w: padding.mul_add(2.0, rect.w),
        h: padding.mul_add(2.0, rect.h),
    }
}

fn nearest_intersecting_obstacle(
    seg_start: (f32, f32),
    seg_end: (f32, f32),
    obstacles: &[Rect],
) -> Option<&Rect> {
    obstacles
        .iter()
        .filter(|obstacle| segment_intersects_rect(seg_start, seg_end, obstacle))
        .min_by(|a, b| {
            let da =
                (a.w.mul_add(0.5, a.x) - seg_start.0).hypot(a.h.mul_add(0.5, a.y) - seg_start.1);
            let db =
                (b.w.mul_add(0.5, b.x) - seg_start.0).hypot(b.h.mul_add(0.5, b.y) - seg_start.1);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
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
