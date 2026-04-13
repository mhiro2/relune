//! Inline Crow's Foot markers for FK edges where SVG `<marker>` placement fails.
//!
//! SVG `<marker>` elements are oriented along the tangent at the path endpoint.
//! Once the visible path is smoothed, composite markers (circle + crow's foot)
//! can appear disconnected.  Short orthogonal polyline legs can also be shorter
//! than the marker's refX footprint, so glyphs appear to float mid-edge.
//! These helpers draw the same symbols as regular SVG elements positioned along
//! the rendered backbone instead.

use std::fmt::{self, Write};

use relune_core::layout::{Cardinality, EdgeRoute};

// ---------------------------------------------------------------------------
// Sampling helpers
// ---------------------------------------------------------------------------

/// Sample the route at an approximate arc-length `dist` from the **start**.
/// Returns `(point, unit_tangent)`.
fn sample_route_from_start(route: &EdgeRoute, dist: f32) -> ((f32, f32), (f32, f32)) {
    let p0 = (route.x1, route.y1);
    let next = route
        .control_points
        .first()
        .copied()
        .unwrap_or((route.x2, route.y2));
    sample_line(p0, next, dist)
}

/// Sample the route at an approximate arc-length `dist` from the **end**.
/// The returned tangent points *toward* the endpoint.
fn sample_route_from_end(route: &EdgeRoute, dist: f32) -> ((f32, f32), (f32, f32)) {
    let p3 = (route.x2, route.y2);
    let prev = route
        .control_points
        .last()
        .copied()
        .unwrap_or((route.x1, route.y1));
    let (pt, tang) = sample_line(p3, prev, dist);
    // Flip tangent so it points toward the endpoint.
    (pt, (-tang.0, -tang.1))
}

fn perp_vec(v: (f32, f32)) -> (f32, f32) {
    (-v.1, v.0)
}

/// Sample a point along a straight line at `dist` pixels from `from`.
fn sample_line(from: (f32, f32), to: (f32, f32), dist: f32) -> ((f32, f32), (f32, f32)) {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let len = dx.hypot(dy);
    let unit = if len > 0.001 {
        (dx / len, dy / len)
    } else {
        (1.0, 0.0)
    };
    let t = if len > 0.001 { dist / len } else { 0.0 };
    let t = t.clamp(0.0, 1.0);
    ((dx.mul_add(t, from.0), dy.mul_add(t, from.1)), unit)
}

const fn offset_point(point: (f32, f32), direction: (f32, f32), distance: f32) -> (f32, f32) {
    (
        direction.0.mul_add(distance, point.0),
        direction.1.mul_add(distance, point.1),
    )
}

// Marker geometry constants (matching the SVG `<marker>` definitions).
const CROW_PRONG_LEN: f32 = 14.0;
const CROW_SPREAD: f32 = 7.0;
const BAR_HALF: f32 = 7.0;
const CIRCLE_RADIUS: f32 = 3.4;
/// Distance from endpoint to the secondary symbol in compound markers.
const COMPOUND_SECONDARY: f32 = 19.0;

// ---------------------------------------------------------------------------
// Marker rendering
// ---------------------------------------------------------------------------

/// Render Crow's Foot cardinality symbols for a curved FK edge as inline SVG
/// elements positioned along the actual curve path.
#[allow(clippy::too_many_lines)]
pub(crate) fn render_inline_crow_markers(
    out: &mut String,
    route: &EdgeRoute,
    nullable: bool,
    target_cardinality: Cardinality,
    marker_color: &str,
) -> fmt::Result {
    let start = (route.x1, route.y1);
    let end = (route.x2, route.y2);

    // --- Start side (source / child): crow's foot + circle|bar ---
    {
        // Crow's foot prongs: base along the curve at CROW_PRONG_LEN, tip at start.
        let (base, tang) = sample_route_from_start(route, CROW_PRONG_LEN);
        let prp = perp_vec(tang);
        let upper = offset_point(base, prp, CROW_SPREAD);
        let lower = offset_point(base, prp, -CROW_SPREAD);
        write!(
            out,
            r#"<path class="crow-inline" d="M{:.1} {:.1} L{:.1} {:.1} M{:.1} {:.1} L{:.1} {:.1} M{:.1} {:.1} L{:.1} {:.1}" stroke="{}" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none" shape-rendering="geometricPrecision"/>"#,
            upper.0,
            upper.1,
            start.0,
            start.1,
            base.0,
            base.1,
            start.0,
            start.1,
            lower.0,
            lower.1,
            start.0,
            start.1,
            marker_color,
        )?;

        // Secondary symbol behind the crow's foot.
        if nullable {
            // Circle (zero indicator).
            let (c, _) = sample_route_from_start(route, COMPOUND_SECONDARY);
            write!(
                out,
                r#"<circle class="crow-inline" cx="{:.1}" cy="{:.1}" r="{CIRCLE_RADIUS}" fill="none" stroke="{}" stroke-width="1.2" shape-rendering="geometricPrecision"/>"#,
                c.0, c.1, marker_color,
            )?;
        } else {
            // Bar (mandatory indicator).
            let (c, t) = sample_route_from_start(route, COMPOUND_SECONDARY);
            let prp = perp_vec(t);
            let upper = offset_point(c, prp, BAR_HALF);
            let lower = offset_point(c, prp, -BAR_HALF);
            write!(
                out,
                r#"<path class="crow-inline" d="M{:.1} {:.1} L{:.1} {:.1}" stroke="{}" stroke-width="1.5" stroke-linecap="round" fill="none" shape-rendering="geometricPrecision"/>"#,
                upper.0, upper.1, lower.0, lower.1, marker_color,
            )?;
        }
    }

    // --- End side (target / parent) ---
    match target_cardinality {
        Cardinality::One => {
            // Single bar.
            let (c, t) = sample_route_from_end(route, 4.0);
            let prp = perp_vec(t);
            let upper = offset_point(c, prp, BAR_HALF);
            let lower = offset_point(c, prp, -BAR_HALF);
            write!(
                out,
                r#"<path class="crow-inline" d="M{:.1} {:.1} L{:.1} {:.1}" stroke="{}" stroke-width="1.5" stroke-linecap="round" fill="none" shape-rendering="geometricPrecision"/>"#,
                upper.0, upper.1, lower.0, lower.1, marker_color,
            )?;
        }
        Cardinality::Many => {
            // Crow's foot converging at endpoint.
            let (base, tang) = sample_route_from_end(route, CROW_PRONG_LEN);
            let prp = perp_vec(tang);
            let upper = offset_point(base, prp, CROW_SPREAD);
            let lower = offset_point(base, prp, -CROW_SPREAD);
            write!(
                out,
                r#"<path class="crow-inline" d="M{:.1} {:.1} L{:.1} {:.1} M{:.1} {:.1} L{:.1} {:.1} M{:.1} {:.1} L{:.1} {:.1}" stroke="{}" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none" shape-rendering="geometricPrecision"/>"#,
                upper.0,
                upper.1,
                end.0,
                end.1,
                base.0,
                base.1,
                end.0,
                end.1,
                lower.0,
                lower.1,
                end.0,
                end.1,
                marker_color,
            )?;
        }
        Cardinality::ZeroOrOne => {
            // Bar near endpoint.
            let (b, bt) = sample_route_from_end(route, 4.0);
            let bp = perp_vec(bt);
            let upper = offset_point(b, bp, BAR_HALF);
            let lower = offset_point(b, bp, -BAR_HALF);
            write!(
                out,
                r#"<path class="crow-inline" d="M{:.1} {:.1} L{:.1} {:.1}" stroke="{}" stroke-width="1.5" stroke-linecap="round" fill="none" shape-rendering="geometricPrecision"/>"#,
                upper.0, upper.1, lower.0, lower.1, marker_color,
            )?;
            // Circle further back.
            let (c, _) = sample_route_from_end(route, COMPOUND_SECONDARY);
            write!(
                out,
                r#"<circle class="crow-inline" cx="{:.1}" cy="{:.1}" r="{CIRCLE_RADIUS}" fill="none" stroke="{}" stroke-width="1.2" shape-rendering="geometricPrecision"/>"#,
                c.0, c.1, marker_color,
            )?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Marker attribute selection
// ---------------------------------------------------------------------------

/// Choose Crow's Foot SVG markers for a FK edge.
///
/// ## Semantics of each end (Crow's Foot / IE notation)
///
/// **marker-start (source / child side — the table that owns the FK column):**
///   - Crow's foot (three-pronged fork) = "many": each parent can be
///     referenced by multiple child rows.
///   - Bar prefix  (`one-many`)  = mandatory participation: the FK column is
///     NOT NULL, so every child row *must* reference a parent.
///   - Circle prefix (`zero-many`) = optional participation: the FK column is
///     nullable, so a child row *may* have no parent.
///
/// **marker-end (target / parent side — the referenced table):**
///   - `one`       = the referenced columns form a unique / PK constraint,
///     so each FK value resolves to exactly one parent row.
///   - `zero-one`  = unique but the relationship can be absent (`ZeroOrOne`).
///   - `many`      = the referenced columns are *not* unique, so multiple
///     parent rows could match (rare in practice).
pub(crate) const fn edge_marker_attributes(
    uses_crow_markers: bool,
    nullable: bool,
    target_cardinality: Cardinality,
) -> &'static str {
    if uses_crow_markers {
        match (nullable, target_cardinality) {
            // Nullable FK (optional participation): circle + crow's foot on source side.
            (true, Cardinality::Many) => {
                r#" marker-start="url(#cardinality-zero-many)" marker-end="url(#cardinality-many)""#
            }
            (true, Cardinality::One) => {
                r#" marker-start="url(#cardinality-zero-many)" marker-end="url(#cardinality-one)""#
            }
            (true, Cardinality::ZeroOrOne) => {
                r#" marker-start="url(#cardinality-zero-many)" marker-end="url(#cardinality-zero-one)""#
            }
            // Required FK (mandatory participation): bar + crow's foot on source side.
            (false, Cardinality::Many) => {
                r#" marker-start="url(#cardinality-one-many)" marker-end="url(#cardinality-many)""#
            }
            (false, Cardinality::One) => {
                r#" marker-start="url(#cardinality-one-many)" marker-end="url(#cardinality-one)""#
            }
            (false, Cardinality::ZeroOrOne) => {
                r#" marker-start="url(#cardinality-one-many)" marker-end="url(#cardinality-zero-one)""#
            }
        }
    } else {
        r#" marker-end="url(#arrow)""#
    }
}
