//! Edge routing for layout
//!
//! This module provides edge routing algorithms for drawing
//! connections between nodes in the layout.

use relune_core::layout::{EdgeRoute, RouteStyle};

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
    match style {
        RouteStyle::Straight => route_straight(x1, y1, w1, h1, x2, y2, w2, h2),
        RouteStyle::Orthogonal => route_orthogonal(x1, y1, w1, h1, x2, y2, w2, h2),
        RouteStyle::Curved => route_curved(x1, y1, w1, h1, x2, y2, w2, h2),
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
    _w2: f32,
    h2: f32,
) -> EdgeRoute {
    // Start from right edge of source, end at left edge of target
    let sx = x1 + w1;
    let sy = y1 + h1 / 2.0;
    let tx = x2;
    let ty = y2 + h2 / 2.0;

    // Label at midpoint of straight line
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
) -> EdgeRoute {
    // Determine if target is to the right or left
    let target_is_right = x2 > x1 + w1;

    let (sx, sy, tx, ty, mid_x) = if target_is_right {
        // Target is to the right
        let sx = x1 + w1;
        let sy = y1 + h1 / 2.0;
        let tx = x2;
        let ty = y2 + h2 / 2.0;
        let mid_x = sx + (tx - sx) / 2.0;
        (sx, sy, tx, ty, mid_x)
    } else {
        // Target is to the left (or overlapping)
        let sx = x1;
        let sy = y1 + h1 / 2.0;
        let tx = x2 + w2;
        let ty = y2 + h2 / 2.0;
        let mid_x = tx + (sx - tx) / 2.0;
        (sx, sy, tx, ty, mid_x)
    };

    // Label at the middle of the vertical segment (the corner point)
    let label_position = (mid_x, f32::midpoint(sy, ty));

    EdgeRoute {
        x1: sx,
        y1: sy,
        x2: tx,
        y2: ty,
        control_points: vec![(mid_x, sy), (mid_x, ty)],
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
    _w2: f32,
    h2: f32,
) -> EdgeRoute {
    let sx = x1 + w1;
    let sy = y1 + h1 / 2.0;
    let tx = x2;
    let ty = y2 + h2 / 2.0;

    // Calculate control points for a smooth curve
    let dx = (tx - sx).abs();
    let offset = dx * 0.3;

    let cp1 = (sx + offset, sy);
    let cp2 = (tx - offset, ty);

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
    // Self-loops go around the node
    let loop_radius = 20.0;

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
