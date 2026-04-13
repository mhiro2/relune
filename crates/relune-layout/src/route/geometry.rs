//! Geometry helpers for edge routing.
//!
//! Path simplification, polyline midpoint computation, arc-length sampling,
//! and route-to-obstacle conversion utilities.

use relune_core::layout::{EdgeRoute, RouteStyle};

use super::Rect;

/// Returns the full route polyline including endpoints.
#[must_use]
pub(crate) fn route_points(route: &EdgeRoute) -> Vec<(f32, f32)> {
    let mut points: Vec<(f32, f32)> = Vec::with_capacity(route.control_points.len() + 2);
    points.push((route.x1, route.y1));
    points.extend_from_slice(&route.control_points);
    points.push((route.x2, route.y2));
    points
}

#[must_use]
pub(crate) fn rebuild_route_from_points(points: &[(f32, f32)], style: RouteStyle) -> EdgeRoute {
    let points = simplify_orthogonal_path(points);
    let start = points.first().copied().unwrap_or((0.0, 0.0));
    let end = points.last().copied().unwrap_or(start);

    match style {
        RouteStyle::Straight => EdgeRoute {
            x1: start.0,
            y1: start.1,
            x2: end.0,
            y2: end.1,
            control_points: Vec::new(),
            style,
            label_position: (f32::midpoint(start.0, end.0), f32::midpoint(start.1, end.1)),
        },
        RouteStyle::Orthogonal | RouteStyle::Curved => EdgeRoute {
            x1: start.0,
            y1: start.1,
            x2: end.0,
            y2: end.1,
            control_points: points[1..points.len().saturating_sub(1)].to_vec(),
            style,
            label_position: polyline_midpoint(&points),
        },
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

/// Shortest polyline segment length along the routed backbone (endpoints included).
///
/// Renderers use this to decide when SVG `<marker>` Crow's Foot glyphs would extend
/// past elbows on short orthogonal legs.
#[must_use]
pub fn shortest_backbone_segment_length(route: &EdgeRoute) -> f32 {
    let points = route_points(route);
    if points.len() < 2 {
        return 0.0;
    }
    points
        .windows(2)
        .map(|segment| {
            let dx = segment[1].0 - segment[0].0;
            let dy = segment[1].1 - segment[0].1;
            dx.hypot(dy)
        })
        .fold(f32::INFINITY, f32::min)
}

/// Simplify an orthogonal path by removing collinear and backtracking points.
///
/// Pass 1: remove intermediate points that are collinear with their neighbors
/// (all three share the same X or same Y coordinate).
/// Pass 2: remove backtracking — when three consecutive points lie on the same
/// axis and the direction reverses, the middle point is a stub. Repeat until
/// stable.
pub(super) fn simplify_orthogonal_path(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
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
pub(super) fn polyline_midpoint(points: &[(f32, f32)]) -> (f32, f32) {
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
mod shortest_segment_tests {
    use super::{EdgeRoute, shortest_backbone_segment_length};
    use relune_core::layout::RouteStyle;

    #[test]
    fn shortest_backbone_segment_length_min_leg() {
        let route = EdgeRoute {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 0.0,
            control_points: vec![(50.0, 40.0)],
            style: RouteStyle::Orthogonal,
            label_position: (0.0, 0.0),
        };
        let leg = 50.0_f32.hypot(40.0);
        assert!((shortest_backbone_segment_length(&route) - leg).abs() < 0.01);
    }
}
