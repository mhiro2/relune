use serde::{Deserialize, Serialize};

use crate::RouteStyle;

/// Specifies which tables to include or exclude from processing.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilterSpec {
    /// Tables to include (glob patterns). If empty, all tables are included.
    pub include: Vec<String>,
    /// Tables to exclude (glob patterns).
    pub exclude: Vec<String>,
}

/// Maximum allowed focus depth to prevent graph explosion.
pub const MAX_FOCUS_DEPTH: u32 = 10;

/// Specifies a focus table and how many levels of related tables to include.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FocusSpec {
    /// The table to focus on (qualified name or ID).
    pub table: String,
    /// Number of levels of related tables to include (clamped to [`MAX_FOCUS_DEPTH`]).
    #[serde(default = "default_focus_depth", deserialize_with = "clamp_depth")]
    pub depth: u32,
}

const fn default_focus_depth() -> u32 {
    1
}

fn clamp_depth<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = u32::deserialize(deserializer)?;
    if value > MAX_FOCUS_DEPTH {
        tracing::warn!(
            requested = value,
            max = MAX_FOCUS_DEPTH,
            "focus depth exceeds maximum, clamping to {MAX_FOCUS_DEPTH}",
        );
    }
    Ok(value.min(MAX_FOCUS_DEPTH))
}

impl FocusSpec {
    /// Creates a new `FocusSpec`, clamping `depth` to [`MAX_FOCUS_DEPTH`].
    #[must_use]
    pub fn new(table: impl Into<String>, depth: u32) -> Self {
        if depth > MAX_FOCUS_DEPTH {
            tracing::warn!(
                requested = depth,
                max = MAX_FOCUS_DEPTH,
                "focus depth exceeds maximum, clamping to {MAX_FOCUS_DEPTH}",
            );
        }
        Self {
            table: table.into(),
            depth: depth.min(MAX_FOCUS_DEPTH),
        }
    }
}

impl Default for FocusSpec {
    fn default() -> Self {
        Self {
            table: String::new(),
            depth: 1,
        }
    }
}

/// Specifies how to group tables in the output.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupingSpec {
    /// Grouping strategy.
    pub strategy: GroupingStrategy,
}

/// Strategy for grouping tables in the output.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GroupingStrategy {
    /// No grouping.
    #[default]
    None,
    /// Group by schema name.
    BySchema,
    /// Group by name prefix.
    ByPrefix,
}

/// Specifies layout configuration hints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutSpec {
    /// Layout algorithm.
    #[serde(default)]
    pub algorithm: LayoutAlgorithm,
    /// Layout direction.
    #[serde(default)]
    pub direction: LayoutDirection,
    /// Edge routing style.
    #[serde(default)]
    pub edge_style: RouteStyle,
    /// Horizontal spacing hint.
    #[serde(default = "default_horizontal_spacing")]
    pub horizontal_spacing: f32,
    /// Vertical spacing hint.
    #[serde(default = "default_vertical_spacing")]
    pub vertical_spacing: f32,
    /// Iteration count for force-directed layout.
    #[serde(default = "default_force_iterations")]
    pub force_iterations: usize,
}

/// Layout algorithm for positioning nodes.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutAlgorithm {
    /// Hierarchical layered layout.
    #[default]
    Hierarchical,
    /// Force-directed layout.
    ForceDirected,
}

const fn default_horizontal_spacing() -> f32 {
    320.0
}
const fn default_vertical_spacing() -> f32 {
    80.0
}
const fn default_force_iterations() -> usize {
    150
}

/// Direction for layout flow.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LayoutDirection {
    /// Top to bottom.
    #[default]
    TopToBottom,
    /// Left to right.
    LeftToRight,
    /// Right to left.
    RightToLeft,
    /// Bottom to top.
    BottomToTop,
}

impl Default for LayoutSpec {
    fn default() -> Self {
        Self {
            algorithm: LayoutAlgorithm::default(),
            direction: LayoutDirection::default(),
            edge_style: RouteStyle::default(),
            horizontal_spacing: default_horizontal_spacing(),
            vertical_spacing: default_vertical_spacing(),
            force_iterations: default_force_iterations(),
        }
    }
}
