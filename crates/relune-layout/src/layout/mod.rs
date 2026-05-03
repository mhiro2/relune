//! Main layout engine
//!
//! This module provides the main layout algorithm that combines
//! ranking, ordering, and coordinate assignment to produce
//! a positioned graph suitable for rendering.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{Level, debug, info, span};

use relune_core::layout::{EdgeRoute, RouteStyle};
use relune_core::{
    EdgeKind, LayoutAlgorithm, LayoutCompactionSpec, LayoutDirection, LayoutSpec, NodeKind, Schema,
};

use crate::focus::FocusExtractor;
use crate::graph::{CollapsedJoinTable, LayoutGraph, LayoutGraphBuilder, LayoutRequest};
use crate::order::order_nodes_within_layers;
use crate::rank::{RankAssignmentStrategy, assign_ranks};

mod edge_routing;
mod force;
mod groups;
mod hierarchical;
mod routing_debug;
mod spacing;

use edge_routing::route_edges_with_diagnostics;
use force::apply_force_layout;
use groups::position_groups;
use hierarchical::assign_coordinates;
use spacing::{expand_bounds_for_edges, measure_node_sizes};

/// Layout mode alias shared with `relune-core`.
pub type LayoutMode = LayoutAlgorithm;

/// Default number of iterations for force-directed layout.
const fn default_force_iterations() -> usize {
    150
}

/// Horizontal padding around grouped nodes.
pub(super) const GROUP_PADDING: f32 = 20.0;
/// Extra top inset reserved for the rendered group label band.
pub(super) const GROUP_TOP_PADDING: f32 = 44.0;

#[derive(Debug, Clone, Copy)]
pub(super) struct NodeSize {
    pub(super) width: f32,
    pub(super) height: f32,
}

/// Configuration for layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    /// Origin X coordinate.
    pub origin_x: f32,
    /// Origin Y coordinate.
    pub origin_y: f32,
    /// Horizontal spacing between nodes.
    pub horizontal_spacing: f32,
    /// Vertical spacing between nodes.
    pub vertical_spacing: f32,
    /// Node width.
    pub node_width: f32,
    /// Height per column row.
    pub column_height: f32,
    /// Header height.
    pub header_height: f32,
    /// Padding inside nodes.
    pub node_padding: f32,
    /// Layout direction.
    pub direction: LayoutDirection,
    /// Edge rendering style.
    pub edge_style: RouteStyle,
    /// Whether to show column details in nodes.
    /// When false, only table names are displayed.
    pub show_columns: bool,
    /// Layout mode (hierarchical or force-directed).
    #[serde(default)]
    pub mode: LayoutMode,
    /// Number of iterations for force-directed layout.
    #[serde(default = "default_force_iterations")]
    pub force_iterations: usize,
    /// Automatic compaction settings for large schemas.
    #[serde(default)]
    pub compaction: LayoutCompactionSpec,
    /// When true, spacing is automatically adjusted based on graph density.
    #[serde(default)]
    pub auto_tune_spacing: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            origin_x: 56.0,
            origin_y: 56.0,
            horizontal_spacing: 320.0,
            vertical_spacing: 160.0,
            node_width: 260.0,
            column_height: 18.0,
            header_height: 32.0,
            node_padding: 8.0,
            direction: LayoutDirection::TopToBottom,
            edge_style: RouteStyle::Orthogonal,
            show_columns: true,
            mode: LayoutAlgorithm::default(),
            force_iterations: default_force_iterations(),
            compaction: LayoutCompactionSpec::default(),
            auto_tune_spacing: true,
        }
    }
}

impl LayoutConfig {
    /// Validates that numeric settings are finite and internally consistent.
    pub fn validate(&self) -> Result<(), LayoutConfigValidationError> {
        let mut issues = Vec::new();

        validate_finite("origin_x", self.origin_x, &mut issues);
        validate_finite("origin_y", self.origin_y, &mut issues);
        validate_positive("horizontal_spacing", self.horizontal_spacing, &mut issues);
        validate_positive("vertical_spacing", self.vertical_spacing, &mut issues);
        validate_positive("node_width", self.node_width, &mut issues);
        validate_positive("column_height", self.column_height, &mut issues);
        validate_positive("header_height", self.header_height, &mut issues);
        validate_non_negative("node_padding", self.node_padding, &mut issues);
        validate_positive(
            "compaction.min_horizontal_spacing",
            self.compaction.min_horizontal_spacing,
            &mut issues,
        );
        validate_positive(
            "compaction.min_vertical_spacing",
            self.compaction.min_vertical_spacing,
            &mut issues,
        );
        validate_positive(
            "compaction.min_node_width",
            self.compaction.min_node_width,
            &mut issues,
        );
        validate_non_negative(
            "compaction.min_node_padding",
            self.compaction.min_node_padding,
            &mut issues,
        );

        if self.force_iterations == 0 {
            issues.push("force_iterations must be greater than 0".to_string());
        }

        validate_upper_bound(
            "compaction.min_horizontal_spacing",
            self.compaction.min_horizontal_spacing,
            "horizontal_spacing",
            self.horizontal_spacing,
            &mut issues,
        );
        validate_upper_bound(
            "compaction.min_vertical_spacing",
            self.compaction.min_vertical_spacing,
            "vertical_spacing",
            self.vertical_spacing,
            &mut issues,
        );
        validate_upper_bound(
            "compaction.min_node_width",
            self.compaction.min_node_width,
            "node_width",
            self.node_width,
            &mut issues,
        );
        validate_upper_bound(
            "compaction.min_node_padding",
            self.compaction.min_node_padding,
            "node_padding",
            self.node_padding,
            &mut issues,
        );

        if issues.is_empty() {
            Ok(())
        } else {
            Err(LayoutConfigValidationError::new(&issues))
        }
    }
}

impl From<&LayoutSpec> for LayoutConfig {
    fn from(spec: &LayoutSpec) -> Self {
        Self {
            mode: spec.algorithm,
            edge_style: spec.edge_style,
            horizontal_spacing: spec.horizontal_spacing,
            vertical_spacing: spec.vertical_spacing,
            direction: spec.direction,
            force_iterations: spec.force_iterations,
            compaction: spec.compaction.clone(),
            auto_tune_spacing: spec.auto_tune_spacing,
            ..Default::default()
        }
    }
}

fn validate_finite(field: &'static str, value: f32, issues: &mut Vec<String>) {
    if !value.is_finite() {
        issues.push(format!("{field} must be finite, got {value}"));
    }
}

fn validate_positive(field: &'static str, value: f32, issues: &mut Vec<String>) {
    validate_finite(field, value, issues);
    if value <= 0.0 {
        issues.push(format!("{field} must be greater than 0, got {value}"));
    }
}

fn validate_non_negative(field: &'static str, value: f32, issues: &mut Vec<String>) {
    validate_finite(field, value, issues);
    if value < 0.0 {
        issues.push(format!("{field} must be at least 0, got {value}"));
    }
}

fn validate_upper_bound(
    minimum_field: &'static str,
    minimum_value: f32,
    base_field: &'static str,
    base_value: f32,
    issues: &mut Vec<String>,
) {
    if minimum_value > base_value {
        issues.push(format!(
            "{minimum_field} must be less than or equal to {base_field}, got {minimum_value} > {base_value}"
        ));
    }
}

/// Compacted layout configuration values computed based on graph size.
#[derive(Debug, Clone)]
pub struct CompactedConfig {
    /// Compacted horizontal spacing.
    pub horizontal_spacing: f32,
    /// Compacted vertical spacing.
    pub vertical_spacing: f32,
    /// Compacted node width.
    pub node_width: f32,
    /// Compacted padding inside nodes.
    pub node_padding: f32,
    /// Whether columns should be hidden.
    pub hide_columns: bool,
}

impl LayoutConfig {
    /// Compute compacted configuration values based on the number of nodes.
    ///
    /// When the node count exceeds `compaction.threshold`, this method returns
    /// reduced spacing and sizing values to create a more compact layout.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn compute_compacted_config(&self, node_count: usize) -> CompactedConfig {
        if self.compaction.threshold > 0 && node_count > self.compaction.threshold {
            // Calculate compaction factor based on how much we exceed the threshold.
            // The min_* floors below already prevent runaway shrinkage, so the
            // excess_ratio itself does not need to saturate.
            let excess_ratio = node_count as f32 / self.compaction.threshold as f32;
            let compaction_factor = 1.0 / excess_ratio;

            // Apply compaction with minimum bounds to maintain readability
            CompactedConfig {
                horizontal_spacing: (self.horizontal_spacing * compaction_factor)
                    .max(self.compaction.min_horizontal_spacing),
                vertical_spacing: (self.vertical_spacing * compaction_factor)
                    .max(self.compaction.min_vertical_spacing),
                node_width: (self.node_width * compaction_factor)
                    .max(self.compaction.min_node_width),
                node_padding: (self.node_padding * compaction_factor)
                    .max(self.compaction.min_node_padding),
                hide_columns: !self.show_columns
                    || (self.compaction.hide_columns_threshold_multiplier > 0
                        && node_count
                            > self
                                .compaction
                                .threshold
                                .saturating_mul(self.compaction.hide_columns_threshold_multiplier)),
            }
        } else {
            CompactedConfig {
                horizontal_spacing: self.horizontal_spacing,
                vertical_spacing: self.vertical_spacing,
                node_width: self.node_width,
                node_padding: self.node_padding,
                hide_columns: !self.show_columns,
            }
        }
    }

    /// Check if compact mode should be enabled based on node count.
    #[must_use]
    pub const fn should_compact(&self, node_count: usize) -> bool {
        self.compaction.threshold > 0 && node_count > self.compaction.threshold
    }

    /// Auto-tune spacing based on node count and edge density.
    ///
    /// This adjusts `horizontal_spacing` and `vertical_spacing` so that
    /// small schemas stay roomy, medium schemas stay balanced, and large /
    /// dense schemas compress proportionally without exceeding screen real-estate.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn auto_tuned(mut self, node_count: usize, edge_count: usize) -> Self {
        if node_count == 0 {
            return self;
        }

        // Density = edges per node.  A linear chain has density ~1, a
        // fully-connected graph approaches N-1.
        let density = edge_count as f32 / node_count as f32;

        // --- Node-count factor: reduce spacing as graph grows. ---
        let count_factor = match node_count {
            0..=6 => 1.0,
            7..=15 => 0.9,
            16..=30 => 0.8,
            31..=60 => 0.7,
            _ => 0.6,
        };

        // --- Density factor: denser graphs need more room for edges. ---
        // The clamp keeps the curve continuous at density=2.0 (the lower
        // bound of 1.0 holds until density>2.5, then climbs to a 1.2× cap).
        let density_factor = if density <= 1.0 {
            0.9
        } else {
            (density * 0.4).clamp(1.0, 1.2)
        };

        let combined = count_factor * density_factor;

        self.horizontal_spacing =
            (self.horizontal_spacing * combined).max(self.compaction.min_horizontal_spacing);
        self.vertical_spacing =
            (self.vertical_spacing * combined).max(self.compaction.min_vertical_spacing);

        self
    }
}

/// Validation error for an invalid layout configuration.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("invalid layout config: {message}")]
pub struct LayoutConfigValidationError {
    message: String,
}

impl LayoutConfigValidationError {
    fn new(issues: &[String]) -> Self {
        Self {
            message: issues.join("; "),
        }
    }
}

/// Error during layout.
#[derive(Debug, Error)]
pub enum LayoutError {
    /// Error occurred during focus extraction.
    #[error("focus extraction failed: {0}")]
    Focus(#[from] crate::focus::FocusError),

    /// Layout configuration failed validation.
    #[error(transparent)]
    InvalidConfig(#[from] LayoutConfigValidationError),

    /// Coordinate assignment left a node without a position.
    #[error("coordinate assignment did not produce a position for node {node_id}")]
    MissingNodePosition {
        /// Stable node identifier.
        node_id: String,
    },

    /// Edge routing could not find a required graph component.
    #[error("edge routing invariant violated for edge {from} -> {to}: {detail}")]
    RoutingInvariant {
        /// Source node identifier.
        from: String,
        /// Target node identifier.
        to: String,
        /// Human-readable detail.
        detail: &'static str,
    },
}

/// A positioned node ready for rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionedNode {
    /// Stable ID of the table.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Node kind.
    pub kind: NodeKind,
    /// Column information.
    pub columns: Vec<PositionedColumn>,
    /// X coordinate (top-left corner).
    pub x: f32,
    /// Y coordinate (top-left corner).
    pub y: f32,
    /// Node width.
    pub width: f32,
    /// Node height.
    pub height: f32,
    /// Whether this is a join table candidate.
    pub is_join_table_candidate: bool,
    /// Whether this node has a self-loop.
    pub has_self_loop: bool,
    /// Group index (if grouped).
    pub group_index: Option<usize>,
}

/// Shared flag set for rendered columns.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ColumnRelationFlags {
    /// Whether this column is part of the primary key.
    pub is_primary_key: bool,
    /// Whether this column participates in a foreign key.
    #[serde(default)]
    pub is_foreign_key: bool,
    /// Whether this column appears in an index.
    #[serde(default)]
    pub is_indexed: bool,
}

/// Shared flag set for rendered columns.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ColumnFlags {
    /// Whether the column can be null.
    pub nullable: bool,
    /// Relationship and index markers.
    #[serde(flatten)]
    pub relation: ColumnRelationFlags,
}

/// Column information for positioned nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionedColumn {
    /// Column name.
    pub name: String,
    /// Column data type.
    pub data_type: String,
    /// Boolean render flags flattened for stable serialized output.
    #[serde(flatten)]
    pub flags: ColumnFlags,
}

/// A positioned edge ready for rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionedEdge {
    /// Source table ID.
    pub from: String,
    /// Target table ID.
    pub to: String,
    /// Edge label (FK name or columns).
    pub label: String,
    /// Edge kind.
    pub kind: EdgeKind,
    /// Route information.
    pub route: EdgeRoute,
    /// Whether this is a self-loop.
    pub is_self_loop: bool,
    /// Whether the FK columns are nullable.
    pub nullable: bool,
    /// Cardinality at the target endpoint.
    pub target_cardinality: relune_core::layout::Cardinality,
    /// The FK column names on the source table.
    pub from_columns: Vec<String>,
    /// The referenced column names on the target table.
    pub to_columns: Vec<String>,
    /// Whether this edge represents a collapsed join table (many-to-many relationship).
    #[serde(default)]
    pub is_collapsed_join: bool,
    /// If this is a collapsed join edge, contains information about the join table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapsed_join_table: Option<CollapsedJoinTable>,
    /// X coordinate for the edge label.
    pub label_x: f32,
    /// Y coordinate for the edge label.
    pub label_y: f32,
    /// Optional routing metadata exposed by `layout-json` for debugging and comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_debug: Option<PositionedEdgeRoutingDebug>,
}

/// A fully positioned graph ready for rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionedGraph {
    /// Positioned nodes.
    pub nodes: Vec<PositionedNode>,
    /// Positioned edges.
    pub edges: Vec<PositionedEdge>,
    /// Group information.
    pub groups: Vec<PositionedGroup>,
    /// Total width of the graph.
    pub width: f32,
    /// Total height of the graph.
    pub height: f32,
    /// Optional graph-level routing diagnostics exposed by `layout-json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_debug: Option<PositionedGraphRoutingDebug>,
}

/// Debug metadata for one routed edge in `layout-json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionedEdgeRoutingDebug {
    /// Chosen source-side attachment policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_side: Option<String>,
    /// Chosen target-side attachment policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_side: Option<String>,
    /// Zero-based source-side slot index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_slot_index: Option<usize>,
    /// Total slot count on the source side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_slot_count: Option<usize>,
    /// Zero-based target-side slot index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_slot_index: Option<usize>,
    /// Total slot count on the target side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_slot_count: Option<usize>,
    /// Column-aware row offset applied on the source side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_row_offset: Option<f32>,
    /// Column-aware row offset applied on the target side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_row_offset: Option<f32>,
    /// Channel axis chosen by obstacle-aware routing when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_axis: Option<String>,
    /// Channel coordinate chosen by obstacle-aware routing when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_coordinate: Option<f32>,
    /// Whether this edge contributed to the non-self-loop detour activation count.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub detour_activation_counted: bool,
    /// Self-loop radius offset when the edge is routed as a loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_loop_radius_offset: Option<f32>,
}

/// Graph-level routing diagnostics emitted with `layout-json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionedGraphRoutingDebug {
    /// Number of non-self-loop edges whose final backbone still intersects padded obstacles.
    pub non_self_loop_detour_activations: usize,
    /// Number of edges that fell back to simple backbone routing because no obstacle-aware
    /// channel candidate satisfied hard constraints.
    pub channel_fallback_activations: usize,
}

/// A positioned group for rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionedGroup {
    /// Group identifier.
    pub id: String,
    /// Group label.
    pub label: String,
    /// X coordinate (top-left corner).
    pub x: f32,
    /// Y coordinate (top-left corner).
    pub y: f32,
    /// Group width.
    pub width: f32,
    /// Group height.
    pub height: f32,
}

/// Build a positioned layout from a schema with default configuration.
pub fn build_layout(schema: &Schema) -> Result<PositionedGraph, LayoutError> {
    build_layout_with_config(schema, &LayoutRequest::default(), &LayoutConfig::default())
}

/// Build a positioned layout from a schema with custom configuration.
pub fn build_layout_with_config(
    schema: &Schema,
    request: &LayoutRequest,
    config: &LayoutConfig,
) -> Result<PositionedGraph, LayoutError> {
    let span = span!(Level::INFO, "build_layout");
    let _enter = span.enter();

    info!("Building layout for {} tables", schema.tables.len());

    let mut graph = LayoutGraphBuilder::new()
        .filter(request.filter.clone())
        .grouping(request.grouping)
        .collapse_join_tables(request.collapse_join_tables)
        .build(schema);

    debug!(
        "Built graph with {} nodes and {} edges",
        graph.nodes.len(),
        graph.edges.len()
    );

    if let Some(ref focus) = request.focus {
        let extractor = FocusExtractor;
        graph = extractor.extract(&graph, focus)?;
        debug!("Applied focus, resulting in {} nodes", graph.nodes.len());
    }

    build_layout_from_graph_with_config(&graph, config)
}

/// Build a positioned layout from a precomputed graph.
pub fn build_layout_from_graph_with_config(
    graph: &LayoutGraph,
    config: &LayoutConfig,
) -> Result<PositionedGraph, LayoutError> {
    config.validate()?;

    // Step 2a: Auto-tune spacing based on graph density before compaction.
    let tuned_config = if config.auto_tune_spacing {
        config
            .clone()
            .auto_tuned(graph.nodes.len(), graph.edges.len())
    } else {
        config.clone()
    };

    // Step 2b: Compute compacted config based on graph size and apply if needed
    let compacted = tuned_config.compute_compacted_config(graph.nodes.len());
    let effective_config = if tuned_config.should_compact(graph.nodes.len()) {
        info!(
            "Large schema detected ({} nodes > {} threshold), applying compact mode",
            graph.nodes.len(),
            tuned_config.compaction.threshold
        );
        LayoutConfig {
            horizontal_spacing: compacted.horizontal_spacing,
            vertical_spacing: compacted.vertical_spacing,
            node_width: compacted.node_width,
            node_padding: compacted.node_padding,
            show_columns: !compacted.hide_columns,
            ..tuned_config
        }
    } else {
        tuned_config
    };

    // Step 3: Assign coordinates based on layout mode
    let node_sizes = measure_node_sizes(graph, &effective_config);
    let (positioned_nodes, width, height, node_ranks) = match effective_config.mode {
        LayoutAlgorithm::Hierarchical => {
            // Hierarchical layout: assign ranks and order
            let ranks = assign_ranks(graph, RankAssignmentStrategy::LongestPath);
            debug!("Assigned {} ranks", ranks.num_ranks);
            let ordered_nodes = order_nodes_within_layers(graph, &ranks);
            let node_ranks = ranks.node_rank;
            let (positioned_nodes, width, height) =
                assign_coordinates(graph, &ordered_nodes, &effective_config, &node_sizes)?;
            (positioned_nodes, width, height, Some(node_ranks))
        }
        LayoutAlgorithm::ForceDirected => {
            let ranks = assign_ranks(graph, RankAssignmentStrategy::LongestPath);
            debug!(
                "Assigned {} ranks for force-directed directional guidance",
                ranks.num_ranks
            );
            let ordered_nodes = order_nodes_within_layers(graph, &ranks);
            let node_ranks = ranks.node_rank;
            let (positioned_nodes, width, height) =
                apply_force_layout(graph, &effective_config, &node_sizes, &ordered_nodes)?;
            (positioned_nodes, width, height, Some(node_ranks))
        }
    };

    // Step 4: Route edges
    let (positioned_edges, routing_diagnostics) = route_edges_with_diagnostics(
        graph,
        &positioned_nodes,
        &effective_config,
        node_ranks.as_deref(),
    )?;

    // Step 5: Position groups
    let positioned_groups = position_groups(&graph.groups, &positioned_nodes);

    // Expand canvas bounds so self-loop curves are not clipped.
    let (width, height) = expand_bounds_for_edges(width, height, &positioned_edges);

    info!("Layout complete: {}x{} pixels", width, height);

    Ok(PositionedGraph {
        nodes: positioned_nodes,
        edges: positioned_edges,
        groups: positioned_groups,
        width,
        height,
        routing_debug: Some(PositionedGraphRoutingDebug {
            non_self_loop_detour_activations: routing_diagnostics.non_self_loop_detour_activations,
            channel_fallback_activations: routing_diagnostics.channel_fallback_activations,
        }),
    })
}

#[cfg(test)]
mod tests;
