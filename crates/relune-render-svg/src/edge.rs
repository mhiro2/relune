//! Edge path geometry and edge rendering options.

use std::fmt::Write;

use relune_core::layout::{EdgeRoute, RouteStyle};

/// Builds the SVG `d` attribute for [`EdgeRoute`] from the layout engine.
#[must_use]
pub(crate) fn edge_route_svg_path_d(route: &EdgeRoute, curve_offset: f32) -> String {
    let points = route_backbone_points(route);

    match route.style {
        RouteStyle::Straight if route.control_points.is_empty() => {
            format!(
                "M {:.1} {:.1} L {:.1} {:.1}",
                route.x1, route.y1, route.x2, route.y2
            )
        }
        RouteStyle::Straight | RouteStyle::Orthogonal => polyline_path_d(&points),
        RouteStyle::Curved => rounded_backbone_path_d(&points, curve_offset),
    }
}

fn route_backbone_points(route: &EdgeRoute) -> Vec<(f32, f32)> {
    let mut points = Vec::with_capacity(route.control_points.len() + 2);
    points.push((route.x1, route.y1));
    points.extend(route.control_points.iter().copied());
    points.push((route.x2, route.y2));
    points
}

/// Approximates the visible route as a polyline after style-specific rendering.
#[must_use]
pub fn rendered_path_points(
    route: &EdgeRoute,
    curve_offset: f32,
    curve_samples: u32,
) -> Vec<(f32, f32)> {
    let points = route_backbone_points(route);
    match route.style {
        RouteStyle::Curved => rounded_backbone_points(&points, curve_offset, curve_samples),
        RouteStyle::Straight | RouteStyle::Orthogonal => points,
    }
}

fn polyline_path_d(points: &[(f32, f32)]) -> String {
    let Some(&(x, y)) = points.first() else {
        return String::new();
    };

    let mut d = format!("M {x:.1} {y:.1}");
    for &(px, py) in &points[1..] {
        write!(&mut d, " L {px:.1} {py:.1}").expect("writing SVG path to String must succeed");
    }
    d
}

fn rounded_backbone_path_d(points: &[(f32, f32)], curve_offset: f32) -> String {
    let Some(&(start_x, start_y)) = points.first() else {
        return String::new();
    };
    if points.len() < 3 {
        return format!(
            "M {start_x:.1} {start_y:.1} L {:.1} {:.1}",
            points[1].0, points[1].1
        );
    }

    let mut d = format!("M {start_x:.1} {start_y:.1}");
    let max_radius = curve_offset.clamp(0.0, 24.0);

    for window in points.windows(3) {
        let prev = window[0];
        let curr = window[1];
        let next = window[2];

        let incoming = segment_length(prev, curr);
        let outgoing = segment_length(curr, next);
        let radius = max_radius.min(incoming * 0.5).min(outgoing * 0.5);

        if radius <= 0.0 || is_collinear(prev, curr, next) {
            write!(&mut d, " L {:.1} {:.1}", curr.0, curr.1)
                .expect("writing SVG path to String must succeed");
            continue;
        }

        let before = move_toward(curr, prev, radius);
        let after = move_toward(curr, next, radius);
        write!(
            &mut d,
            " L {:.1} {:.1} Q {:.1} {:.1} {:.1} {:.1}",
            before.0, before.1, curr.0, curr.1, after.0, after.1
        )
        .expect("writing SVG path to String must succeed");
    }

    let end = points[points.len() - 1];
    write!(&mut d, " L {:.1} {:.1}", end.0, end.1)
        .expect("writing SVG path to String must succeed");
    d
}

fn rounded_backbone_points(
    points: &[(f32, f32)],
    curve_offset: f32,
    curve_samples: u32,
) -> Vec<(f32, f32)> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let sample_count = curve_samples.max(1);
    let mut rendered = vec![points[0]];
    let max_radius = curve_offset.clamp(0.0, 24.0);

    for window in points.windows(3) {
        let prev = window[0];
        let curr = window[1];
        let next = window[2];

        let incoming = segment_length(prev, curr);
        let outgoing = segment_length(curr, next);
        let radius = max_radius.min(incoming * 0.5).min(outgoing * 0.5);

        if radius <= 0.0 || is_collinear(prev, curr, next) {
            push_if_distinct(&mut rendered, curr);
            continue;
        }

        let before = move_toward(curr, prev, radius);
        let after = move_toward(curr, next, radius);
        push_if_distinct(&mut rendered, before);
        for index in 1..=sample_count {
            #[allow(clippy::cast_precision_loss)]
            let t = index as f32 / sample_count as f32;
            push_if_distinct(
                &mut rendered,
                quadratic_bezier_point(before, curr, after, t),
            );
        }
    }

    push_if_distinct(&mut rendered, points[points.len() - 1]);
    rendered
}

fn segment_length(from: (f32, f32), to: (f32, f32)) -> f32 {
    (to.0 - from.0).hypot(to.1 - from.1)
}

fn move_toward(from: (f32, f32), to: (f32, f32), distance: f32) -> (f32, f32) {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let length = dx.hypot(dy);
    if length <= 0.001 {
        return from;
    }

    let ratio = distance / length;
    (dx.mul_add(ratio, from.0), dy.mul_add(ratio, from.1))
}

fn is_collinear(prev: (f32, f32), curr: (f32, f32), next: (f32, f32)) -> bool {
    let same_x = (prev.0 - curr.0).abs() < 0.5 && (curr.0 - next.0).abs() < 0.5;
    let same_y = (prev.1 - curr.1).abs() < 0.5 && (curr.1 - next.1).abs() < 0.5;
    same_x || same_y
}

fn quadratic_bezier_point(
    start: (f32, f32),
    control: (f32, f32),
    end: (f32, f32),
    t: f32,
) -> (f32, f32) {
    let one_minus_t = 1.0 - t;
    let start_weight = one_minus_t * one_minus_t;
    let control_weight = 2.0 * one_minus_t * t;
    let end_weight = t * t;

    (
        end_weight.mul_add(
            end.0,
            (start_weight * start.0) + (control_weight * control.0),
        ),
        end_weight.mul_add(
            end.1,
            (start_weight * start.1) + (control_weight * control.1),
        ),
    )
}

fn push_if_distinct(points: &mut Vec<(f32, f32)>, point: (f32, f32)) {
    let is_distinct = points
        .last()
        .is_none_or(|last| (last.0 - point.0).abs() > 0.001 || (last.1 - point.1).abs() > 0.001);
    if is_distinct {
        points.push(point);
    }
}

/// Options for edge rendering.
#[derive(Debug, Clone)]
pub struct EdgeRenderOptions {
    /// Stroke width for edges.
    pub stroke_width: f32,
    /// Whether to show edge labels.
    pub show_labels: bool,
    /// Curve offset for bezier control points (0 = straight, higher = curvier).
    pub curve_offset: f32,
    /// Whether to show tooltips on hover.
    pub show_tooltips: bool,
}

impl Default for EdgeRenderOptions {
    fn default() -> Self {
        Self {
            stroke_width: 1.2,
            show_labels: true,
            curve_offset: 50.0,
            show_tooltips: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orthogonal_route_renders_polyline_path() {
        let path = edge_route_svg_path_d(
            &EdgeRoute {
                x1: 100.0,
                y1: 25.0,
                x2: 200.0,
                y2: 125.0,
                control_points: vec![(150.0, 25.0), (150.0, 125.0)],
                style: RouteStyle::Orthogonal,
                label_position: (150.0, 75.0),
            },
            50.0,
        );

        assert_eq!(
            path,
            "M 100.0 25.0 L 150.0 25.0 L 150.0 125.0 L 200.0 125.0"
        );
    }

    #[test]
    fn test_straight_style_keeps_backbone_when_bends_are_required() {
        let path = edge_route_svg_path_d(
            &EdgeRoute {
                x1: 100.0,
                y1: 25.0,
                x2: 200.0,
                y2: 125.0,
                control_points: vec![(150.0, 25.0), (150.0, 125.0)],
                style: RouteStyle::Straight,
                label_position: (150.0, 75.0),
            },
            50.0,
        );

        assert!(path.contains("L 150.0 25.0"));
        assert!(path.contains("L 150.0 125.0"));
    }

    #[test]
    fn test_curved_style_rounds_backbone_corners() {
        let path = edge_route_svg_path_d(
            &EdgeRoute {
                x1: 100.0,
                y1: 25.0,
                x2: 200.0,
                y2: 125.0,
                control_points: vec![(150.0, 25.0), (150.0, 125.0)],
                style: RouteStyle::Curved,
                label_position: (150.0, 75.0),
            },
            50.0,
        );

        assert!(path.contains("Q 150.0 25.0"));
        assert!(path.contains("Q 150.0 125.0"));
    }

    #[test]
    fn test_rendered_path_points_follow_curved_corner_geometry() {
        let points = rendered_path_points(
            &EdgeRoute {
                x1: 100.0,
                y1: 25.0,
                x2: 200.0,
                y2: 125.0,
                control_points: vec![(150.0, 25.0), (150.0, 125.0)],
                style: RouteStyle::Curved,
                label_position: (150.0, 75.0),
            },
            50.0,
            8,
        );

        assert_eq!(points.first().copied(), Some((100.0, 25.0)));
        assert_eq!(points.last().copied(), Some((200.0, 125.0)));
        assert!(points.iter().any(|point| point.1 > 25.0 && point.0 < 150.0));
        assert!(
            points
                .iter()
                .any(|point| point.0 > 150.0 && point.1 < 125.0)
        );
    }
}
