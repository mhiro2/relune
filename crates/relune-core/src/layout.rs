//! Layout type definitions for positioned schema nodes.

use serde::{Deserialize, Serialize};

/// Cardinality representation for relationship endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cardinality {
    /// Exactly one (required, non-nullable FK)
    One,
    /// Zero or one (optional, nullable FK)
    ZeroOrOne,
    /// Many (the "many" side of a relationship)
    Many,
}

impl Cardinality {
    /// Returns the symbol representation for display.
    #[must_use]
    pub const fn symbol(&self) -> &'static str {
        match self {
            Self::One => "1",
            Self::ZeroOrOne => "0..1",
            Self::Many => "N",
        }
    }
}

/// Visual routing style for edges in a positioned graph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RouteStyle {
    /// Single straight segment between attachment points.
    #[default]
    Straight,
    /// Polyline with axis-aligned segments; bend coordinates live in [`EdgeRoute::control_points`].
    Orthogonal,
    /// Cubic Bézier; exactly two control points in [`EdgeRoute::control_points`] (P1, P2).
    Curved,
}

/// Attachment points and geometry for one routed edge (shared by layout and renderers).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeRoute {
    /// Start X (first attachment point).
    pub x1: f32,
    /// Start Y (first attachment point).
    pub y1: f32,
    /// End X (second attachment point).
    pub x2: f32,
    /// End Y (second attachment point).
    pub y2: f32,
    /// For [`RouteStyle::Orthogonal`], intermediate polyline vertices between the endpoints.
    /// For [`RouteStyle::Curved`], cubic Bézier control points (two points).
    pub control_points: Vec<(f32, f32)>,
    /// Routing style used to interpret `control_points`.
    pub style: RouteStyle,
    /// Suggested position for the edge label.
    pub label_position: (f32, f32),
}
