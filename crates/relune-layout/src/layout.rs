//! Main layout engine
//!
//! This module provides the main layout algorithm that combines
//! ranking, ordering, and coordinate assignment to produce
//! a positioned graph suitable for rendering.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{Level, debug, info, span, warn};
use unicode_width::UnicodeWidthChar;

use relune_core::{
    EdgeKind, LayoutAlgorithm, LayoutCompactionSpec, LayoutDirection, LayoutSpec, NodeKind, Schema,
};

use crate::channel::{
    ChannelCandidateClass, ChannelCandidateScore, ChannelCostWeights,
    compare_channel_candidate_scores,
};
use crate::focus::FocusExtractor;
use crate::graph::{CollapsedJoinTable, LayoutGraph, LayoutGraphBuilder, LayoutRequest};
use crate::order::order_nodes_within_layers;
use crate::port::{EdgePortAssignment, RegularPortAssignment, assign_edge_ports};
use crate::rank::{RankAssignmentStrategy, assign_ranks};
use crate::route::{
    AttachmentSide, ChannelAxis, LABEL_HALF_H, Rect, approximate_route_length,
    detour_around_obstacles, estimate_label_half_width, nudge_label, point_along_route,
    rebuild_route_from_points, route_edge_with_assigned_ports, route_points,
    route_self_loop_with_offset, sample_route_obstacles, step_from_attachment,
};
use relune_core::layout::{EdgeRoute, RouteStyle};

/// Layout mode alias shared with `relune-core`.
pub type LayoutMode = LayoutAlgorithm;

/// Default number of iterations for force-directed layout.
const fn default_force_iterations() -> usize {
    150
}

/// Node header font size used for width estimation.
const HEADER_FONT_SIZE: f32 = 13.0;
/// Node column font size used for width estimation.
const COLUMN_FONT_SIZE: f32 = 11.5;
/// Lower bound factor applied to configured node width.
const MIN_NODE_WIDTH_FACTOR: f32 = 0.72;
/// Extra right-side space for the kind label ("TABLE"/"VIEW"/"ENUM") in the header.
const HEADER_KIND_LABEL_RESERVE: f32 = 48.0;
/// Clearance target used while scoring obstacle-aware channel candidates.
const ROUTE_CLEARANCE_TARGET: f32 = 14.0;
/// Maximum gap between nearby channel candidates that may share one visual bundle.
const BUNDLE_CHANNEL_TOLERANCE: f32 = 36.0;
/// Distance used to preserve endpoint approach direction while entering a bypass channel.
const ROUTE_STUB_DISTANCE: f32 = 28.0;
/// Margin added when probing side corridors outside the endpoint nodes.
const BYPASS_CHANNEL_MARGIN: f32 = 24.0;
/// Additional offsets explored for bypass corridors after the first outer lane.
const BYPASS_CHANNEL_OFFSETS: &[f32] = &[0.0, 48.0, 96.0, 144.0];

#[derive(Debug, Clone, Copy)]
struct NodeSize {
    width: f32,
    height: f32,
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
            edge_style: RouteStyle::Straight,
            show_columns: true,
            mode: LayoutAlgorithm::default(),
            force_iterations: default_force_iterations(),
            compaction: LayoutCompactionSpec::default(),
            auto_tune_spacing: true,
        }
    }
}

impl LayoutConfig {
    /// Validates that spacing and dimension values are positive.
    /// Replaces non-positive values with defaults.
    #[must_use]
    pub fn validated(mut self) -> Self {
        let defaults = Self::default();
        if self.horizontal_spacing <= 0.0 {
            self.horizontal_spacing = defaults.horizontal_spacing;
        }
        if self.vertical_spacing <= 0.0 {
            self.vertical_spacing = defaults.vertical_spacing;
        }
        if self.node_width <= 0.0 {
            self.node_width = defaults.node_width;
        }
        if self.column_height <= 0.0 {
            self.column_height = defaults.column_height;
        }
        if self.header_height <= 0.0 {
            self.header_height = defaults.header_height;
        }
        if self.node_padding < 0.0 {
            self.node_padding = defaults.node_padding;
        }
        if self.compaction.min_horizontal_spacing <= 0.0 {
            self.compaction.min_horizontal_spacing = defaults.compaction.min_horizontal_spacing;
        }
        if self.compaction.min_vertical_spacing <= 0.0 {
            self.compaction.min_vertical_spacing = defaults.compaction.min_vertical_spacing;
        }
        if self.compaction.min_node_width <= 0.0 {
            self.compaction.min_node_width = defaults.compaction.min_node_width;
        }
        if self.compaction.min_node_padding < 0.0 {
            self.compaction.min_node_padding = defaults.compaction.min_node_padding;
        }
        self
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
        .validated()
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
            // Calculate compaction factor based on how much we exceed the threshold
            let excess_ratio = (node_count as f32 / self.compaction.threshold as f32).min(2.0);
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
        let density_factor = if density <= 1.0 {
            // Sparse: tighten a bit.
            0.9
        } else if density <= 2.0 {
            1.0
        } else {
            // Dense: widen to avoid congestion, cap at 1.2×.
            (density * 0.4).min(1.2)
        };

        let combined = count_factor * density_factor;

        self.horizontal_spacing =
            (self.horizontal_spacing * combined).max(self.compaction.min_horizontal_spacing);
        self.vertical_spacing =
            (self.vertical_spacing * combined).max(self.compaction.min_vertical_spacing);

        self
    }
}

/// Error during layout.
#[derive(Debug, Error)]
pub enum LayoutError {
    /// Error occurred during focus extraction.
    #[error("focus extraction failed: {0}")]
    FocusError(#[from] crate::focus::FocusError),
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

/// Column information for positioned nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct PositionedColumn {
    /// Column name.
    pub name: String,
    /// Column data type.
    pub data_type: String,
    /// Whether the column can be null.
    pub nullable: bool,
    /// Whether this column is part of the primary key.
    pub is_primary_key: bool,
    /// Whether this column participates in a foreign key.
    #[serde(default)]
    pub is_foreign_key: bool,
    /// Whether this column appears in an index.
    #[serde(default)]
    pub is_indexed: bool,
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

    // Step 2c: If compact mode hides columns, strip them from a temporary graph copy.
    let compacted_graph = compacted.hide_columns.then(|| {
        let mut compacted_graph = graph.clone();
        for node in &mut compacted_graph.nodes {
            node.columns.clear();
        }
        compacted_graph
    });
    let graph = compacted_graph.as_ref().unwrap_or(graph);

    // Step 3: Assign coordinates based on layout mode
    let node_sizes = measure_node_sizes(graph, &effective_config);
    let mut node_ranks = None;
    let (positioned_nodes, width, height) = match effective_config.mode {
        LayoutAlgorithm::Hierarchical => {
            // Hierarchical layout: assign ranks and order
            let ranks = assign_ranks(graph, RankAssignmentStrategy::LongestPath);
            debug!("Assigned {} ranks", ranks.num_ranks);
            let ordered_nodes = order_nodes_within_layers(graph, &ranks);
            node_ranks = Some(ranks.node_rank);
            assign_coordinates(graph, &ordered_nodes, &effective_config, &node_sizes)
        }
        LayoutAlgorithm::ForceDirected => {
            // Force-directed layout
            apply_force_layout(graph, &effective_config, &node_sizes)
        }
    };

    // Step 4: Route edges
    let (positioned_edges, routing_diagnostics) = route_edges_with_diagnostics(
        graph,
        &positioned_nodes,
        &effective_config,
        node_ranks.as_deref(),
    );

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
        }),
    })
}

/// Assign coordinates to nodes based on their ranks and order.
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::suboptimal_flops)]
fn assign_coordinates(
    graph: &LayoutGraph,
    ordered_nodes: &[Vec<usize>],
    config: &LayoutConfig,
    node_sizes: &[NodeSize],
) -> (Vec<PositionedNode>, f32, f32) {
    let n = graph.nodes.len();
    // Index by graph node index so that positioned_nodes[node_idx] correctly
    // addresses the corresponding node (needed by resolve_rank_collisions).
    let mut positioned_slots: Vec<Option<PositionedNode>> = vec![None; n];

    let is_horizontal = matches!(
        config.direction,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft
    );
    let rank_primary_offsets = compute_rank_primary_offsets(ordered_nodes, node_sizes, config);

    for (rank_idx, rank_nodes) in ordered_nodes.iter().enumerate() {
        let mut secondary_offset = if is_horizontal {
            config.origin_y
        } else {
            config.origin_x
        };

        for &node_idx in rank_nodes {
            let node = &graph.nodes[node_idx];
            let node_size = node_sizes[node_idx];
            let primary = rank_primary_offsets[rank_idx];
            let secondary = secondary_offset;

            let (node_x, node_y) = if is_horizontal {
                // Horizontal: ranks flow along X, nodes stack along Y
                secondary_offset += node_size.height + config.vertical_spacing;
                (primary, secondary)
            } else {
                // Vertical: ranks flow along Y, nodes stack along X
                secondary_offset += node_size.width + config.horizontal_spacing;
                (secondary, primary)
            };

            positioned_slots[node_idx] = Some(build_positioned_node(
                node,
                node_x,
                node_y,
                node_size.width,
                node_size.height,
            ));
        }
    }

    // Every graph node must have been assigned a position above.
    let mut positioned_nodes: Vec<PositionedNode> =
        positioned_slots.into_iter().map(Option::unwrap).collect();

    resolve_rank_collisions(&mut positioned_nodes, ordered_nodes, config, is_horizontal);
    let graph_bounds = compute_graph_bounds(&positioned_nodes, config);

    // Flip coordinates for reversed directions
    match config.direction {
        LayoutDirection::BottomToTop => {
            for node in &mut positioned_nodes {
                node.y = graph_bounds.1 - node.y - node.height;
            }
        }
        LayoutDirection::RightToLeft => {
            for node in &mut positioned_nodes {
                node.x = graph_bounds.0 - node.x - node.width;
            }
        }
        LayoutDirection::TopToBottom | LayoutDirection::LeftToRight => {}
    }

    let (width, height) = compute_graph_bounds(&positioned_nodes, config);
    (positioned_nodes, width, height)
}

/// Node count threshold above which the spatial grid is used for repulsion.
const SPATIAL_GRID_THRESHOLD: usize = 64;

/// Apply a single repulsion pair force between nodes `i` and `j`.
#[allow(
    clippy::cast_precision_loss,
    clippy::too_many_arguments,
    clippy::suboptimal_flops
)]
fn apply_repulsion_pair(
    i: usize,
    j: usize,
    positions: &[(f32, f32)],
    node_sizes: &[NodeSize],
    config: &LayoutConfig,
    repulsion_strength: f32,
    min_distance: f32,
    forces: &mut [(f32, f32)],
) {
    let dx = positions[i].0 - positions[j].0;
    let dy = positions[i].1 - positions[j].1;
    let min_gap = node_pair_spacing(node_sizes[i], node_sizes[j], config);
    let dist_sq = dx * dx + dy * dy + min_distance + min_gap * min_gap * 0.25;
    let dist = dist_sq.sqrt();

    let force = repulsion_strength / dist_sq;
    let fx = force * dx / dist;
    let fy = force * dy / dist;

    forces[i].0 += fx;
    forces[i].1 += fy;
    forces[j].0 -= fx;
    forces[j].1 -= fy;
}

/// Compute repulsive forces using a uniform spatial grid.
///
/// Nodes are binned into grid cells. Repulsion is only computed between nodes
/// in the same cell or in adjacent cells, giving O(V) amortised cost when
/// the graph is spread out (each cell contains O(1) nodes on average).
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
fn compute_repulsion_with_grid(
    positions: &[(f32, f32)],
    node_sizes: &[NodeSize],
    config: &LayoutConfig,
    repulsion_strength: f32,
    min_distance: f32,
    forces: &mut [(f32, f32)],
) {
    use std::collections::HashMap;

    let n = positions.len();
    if n == 0 {
        return;
    }

    // Choose cell size based on the effective interaction range.
    // Repulsion falls off as 1/d^2, so beyond a few multiples of the
    // typical node spacing the force is negligible.
    let max_span = node_sizes
        .iter()
        .map(|s| s.width.max(s.height))
        .fold(0.0_f32, f32::max);
    let cell_size = (config.horizontal_spacing + max_span).max(1.0);
    let inv_cell = 1.0 / cell_size;

    // Build grid: map (cell_x, cell_y) → list of node indices
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (idx, &(px, py)) in positions.iter().enumerate() {
        let cx = (px * inv_cell).floor() as i32;
        let cy = (py * inv_cell).floor() as i32;
        grid.entry((cx, cy)).or_default().push(idx);
    }

    // For each cell, compute repulsion within the cell and with 4 neighbours
    // (right, below, below-right, below-left) to avoid double-counting.
    let neighbour_offsets: [(i32, i32); 4] = [(1, 0), (0, 1), (1, 1), (-1, 1)];

    for (&(cx, cy), cell_nodes) in &grid {
        // Intra-cell pairs
        for (a, &i) in cell_nodes.iter().enumerate() {
            for &j in &cell_nodes[a + 1..] {
                apply_repulsion_pair(
                    i,
                    j,
                    positions,
                    node_sizes,
                    config,
                    repulsion_strength,
                    min_distance,
                    forces,
                );
            }
        }

        // Cross-cell pairs with 4 neighbours
        for &(dx, dy) in &neighbour_offsets {
            if let Some(neighbour_nodes) = grid.get(&(cx + dx, cy + dy)) {
                for &i in cell_nodes {
                    for &j in neighbour_nodes {
                        apply_repulsion_pair(
                            i,
                            j,
                            positions,
                            node_sizes,
                            config,
                            repulsion_strength,
                            min_distance,
                            forces,
                        );
                    }
                }
            }
        }
    }
}

/// Apply force-directed layout algorithm.
///
/// This is a simple "force-lite" implementation that uses:
/// - Repulsive forces between nearby nodes (spatial grid for large graphs)
/// - Attractive forces along edges
/// - Centering gravity to prevent drift
/// - Damping to stabilize
#[allow(clippy::too_many_lines)]
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::suboptimal_flops)]
#[allow(clippy::imprecise_flops)]
fn apply_force_layout(
    graph: &LayoutGraph,
    config: &LayoutConfig,
    node_sizes: &[NodeSize],
) -> (Vec<PositionedNode>, f32, f32) {
    let n = graph.nodes.len();
    if n == 0 {
        return (Vec::new(), config.origin_x * 2.0, config.origin_y * 2.0);
    }

    // Force parameters
    let repulsion_strength = 5000.0;
    let attraction_strength = 0.05;
    let gravity_strength = 0.1;
    let damping = 0.9;
    let min_distance = 1.0;

    // Calculate ideal spacing based on config
    let max_node_span = node_sizes
        .iter()
        .map(|size| size.width.max(size.height))
        .fold(0.0, f32::max);
    let ideal_spacing = config.horizontal_spacing.max(config.vertical_spacing) + max_node_span;
    let initial_radius = ideal_spacing * (n as f32).sqrt() * 0.5;

    // Initialize positions in a circle layout (deterministic)
    let mut positions: Vec<(f32, f32)> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let angle = 2.0 * std::f32::consts::PI * i as f32 / n as f32;
            let x = config.origin_x + initial_radius * angle.cos();
            let y = config.origin_y + initial_radius * angle.sin();
            (x, y)
        })
        .collect();

    // Initialize velocities
    let mut velocities: Vec<(f32, f32)> = vec![(0.0, 0.0); n];

    // Build edge list for quick lookup
    let edges: Vec<(usize, usize)> = graph
        .edges
        .iter()
        .filter_map(|edge| {
            let from_idx = graph.node_index.get(edge.from.as_str())?;
            let to_idx = graph.node_index.get(edge.to.as_str())?;
            Some((*from_idx, *to_idx))
        })
        .collect();

    // For large graphs, cap iterations to limit O(V^2 * iterations) cost
    let effective_iterations = if n > 100 {
        config.force_iterations.min(50)
    } else {
        config.force_iterations
    };

    // Run simulation
    for _ in 0..effective_iterations {
        // Calculate forces
        let mut forces: Vec<(f32, f32)> = vec![(0.0, 0.0); n];

        // Repulsive forces between nearby nodes using spatial grid for O(V) amortised cost.
        // For small graphs, fall back to the exact O(V^2) pairwise computation.
        if n > SPATIAL_GRID_THRESHOLD {
            compute_repulsion_with_grid(
                &positions,
                node_sizes,
                config,
                repulsion_strength,
                min_distance,
                &mut forces,
            );
        } else {
            for i in 0..n {
                for j in (i + 1)..n {
                    apply_repulsion_pair(
                        i,
                        j,
                        &positions,
                        node_sizes,
                        config,
                        repulsion_strength,
                        min_distance,
                        &mut forces,
                    );
                }
            }
        }

        // Attractive forces along edges
        for &(from_idx, to_idx) in &edges {
            let dx = positions[to_idx].0 - positions[from_idx].0;
            let dy = positions[to_idx].1 - positions[from_idx].1;
            let dist = (dx * dx + dy * dy).sqrt().max(min_distance);
            let target_distance =
                node_pair_spacing(node_sizes[from_idx], node_sizes[to_idx], config);

            // Attractive force: F = k * d
            let force = attraction_strength * (dist - target_distance);
            let fx = force * dx / dist;
            let fy = force * dy / dist;

            forces[from_idx].0 += fx;
            forces[from_idx].1 += fy;
            forces[to_idx].0 -= fx;
            forces[to_idx].1 -= fy;
        }

        // Centering gravity
        let center_x = config.origin_x + initial_radius;
        let center_y = config.origin_y + initial_radius;
        for i in 0..n {
            let dx = center_x - positions[i].0;
            let dy = center_y - positions[i].1;
            forces[i].0 += gravity_strength * dx;
            forces[i].1 += gravity_strength * dy;
        }

        // Update velocities and positions
        for i in 0..n {
            velocities[i].0 = (velocities[i].0 + forces[i].0) * damping;
            velocities[i].1 = (velocities[i].1 + forces[i].1) * damping;

            positions[i].0 += velocities[i].0;
            positions[i].1 += velocities[i].1;
        }
    }

    // Calculate bounding box and shift to positive coordinates
    let min_x = positions.iter().map(|p| p.0).fold(f32::MAX, f32::min);
    let min_y = positions.iter().map(|p| p.1).fold(f32::MAX, f32::min);

    // Shift positions to start from origin
    for pos in &mut positions {
        pos.0 = pos.0 - min_x + config.origin_x;
        pos.1 = pos.1 - min_y + config.origin_y;
    }
    // Build positioned nodes
    let mut max_x = config.origin_x;
    let mut max_y = config.origin_y;

    let positioned_nodes: Vec<PositionedNode> = graph
        .nodes
        .iter()
        .zip(positions.iter().zip(node_sizes.iter()))
        .map(|(node, (&(x, y), size))| {
            max_x = max_x.max(x + size.width);
            max_y = max_y.max(y + size.height);

            build_positioned_node(node, x, y, size.width, size.height)
        })
        .collect();

    let width = max_x + config.origin_x;
    let height = max_y + config.origin_y;

    (positioned_nodes, width, height)
}

fn build_positioned_node(
    node: &crate::graph::LayoutNode,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> PositionedNode {
    PositionedNode {
        id: node.id.clone(),
        label: node.label.clone(),
        kind: node.kind,
        columns: node
            .columns
            .iter()
            .map(|c| PositionedColumn {
                name: c.name.clone(),
                data_type: c.data_type.clone(),
                nullable: c.nullable,
                is_primary_key: c.is_primary_key,
                is_foreign_key: c.is_foreign_key,
                is_indexed: c.is_indexed,
            })
            .collect(),
        x,
        y,
        width,
        height,
        is_join_table_candidate: node.is_join_table_candidate,
        has_self_loop: node.has_self_loop,
        group_index: node.group_index,
    }
}

fn measure_node_sizes(graph: &LayoutGraph, config: &LayoutConfig) -> Vec<NodeSize> {
    graph
        .nodes
        .iter()
        .map(|node| NodeSize {
            width: estimate_node_width(node, config),
            height: estimate_node_height(node, config),
        })
        .collect()
}

fn estimate_node_width(node: &crate::graph::LayoutNode, config: &LayoutConfig) -> f32 {
    let minimum_width = (config.node_width * MIN_NODE_WIDTH_FACTOR).max(160.0);
    let header_width = config
        .node_padding
        .mul_add(2.0, estimate_text_width(&node.label, HEADER_FONT_SIZE))
        + HEADER_KIND_LABEL_RESERVE;

    let column_width = node
        .columns
        .iter()
        .map(|column| {
            let text = display_column_text(node.kind, &column.name, &column.data_type);
            let text_px = estimate_text_width(&text, COLUMN_FONT_SIZE);
            let icon_slots = usize::from(column.is_indexed)
                + usize::from(column.is_foreign_key)
                + usize::from(column.is_primary_key);
            #[allow(clippy::cast_precision_loss)] // Icon counts are tiny layout values.
            let badge_reserve = if icon_slots > 0 {
                (icon_slots as f32 - 1.0).mul_add(24.0, 28.0)
            } else {
                0.0
            };
            text_px + badge_reserve
        })
        .fold(0.0, f32::max);

    header_width
        .max(config.node_padding.mul_add(2.0, column_width) + 10.0)
        .max(minimum_width)
        .ceil()
}

#[allow(clippy::cast_precision_loss)] // Layout sizing is approximate and bounded for diagram rendering.
#[allow(clippy::suboptimal_flops)]
#[allow(clippy::missing_const_for_fn)] // This helper stays non-const to avoid over-constraining floating-point layout code.
fn estimate_node_height(node: &crate::graph::LayoutNode, config: &LayoutConfig) -> f32 {
    config
        .node_padding
        .mul_add(
            2.0,
            (node.columns.len() as f32).mul_add(config.column_height, config.header_height),
        )
        .ceil()
}

fn compute_rank_primary_offsets(
    ordered_nodes: &[Vec<usize>],
    node_sizes: &[NodeSize],
    config: &LayoutConfig,
) -> Vec<f32> {
    let is_horizontal = matches!(
        config.direction,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft
    );
    let mut offsets = Vec::with_capacity(ordered_nodes.len());
    let mut primary = if is_horizontal {
        config.origin_x
    } else {
        config.origin_y
    };
    let gap = if is_horizontal {
        config.horizontal_spacing
    } else {
        config.vertical_spacing
    };

    for rank_nodes in ordered_nodes {
        offsets.push(primary);
        let extent = rank_nodes
            .iter()
            .map(|&node_idx| {
                if is_horizontal {
                    node_sizes[node_idx].width
                } else {
                    node_sizes[node_idx].height
                }
            })
            .fold(0.0, f32::max);
        primary += extent + gap;
    }

    offsets
}

fn resolve_rank_collisions(
    positioned_nodes: &mut [PositionedNode],
    ordered_nodes: &[Vec<usize>],
    config: &LayoutConfig,
    is_horizontal: bool,
) {
    let spacing = if is_horizontal {
        config.vertical_spacing
    } else {
        config.horizontal_spacing
    };

    for rank_nodes in ordered_nodes {
        let mut previous_end = None;
        for &node_idx in rank_nodes {
            let node = &mut positioned_nodes[node_idx];
            let coordinate = if is_horizontal {
                &mut node.y
            } else {
                &mut node.x
            };
            let extent = if is_horizontal {
                node.height
            } else {
                node.width
            };

            if let Some(end) = previous_end {
                let required = end + spacing;
                if *coordinate < required {
                    *coordinate = required;
                }
            }

            previous_end = Some(*coordinate + extent);
        }
    }
}

fn compute_graph_bounds(positioned_nodes: &[PositionedNode], config: &LayoutConfig) -> (f32, f32) {
    let max_x = positioned_nodes
        .iter()
        .map(|node| node.x + node.width)
        .fold(config.origin_x, f32::max);
    let max_y = positioned_nodes
        .iter()
        .map(|node| node.y + node.height)
        .fold(config.origin_y, f32::max);

    (max_x + config.origin_x, max_y + config.origin_y)
}

/// Expand graph bounds so that edge routes (especially self-loop curves and
/// their control points) are not clipped by the SVG viewport.
fn expand_bounds_for_edges(width: f32, height: f32, edges: &[PositionedEdge]) -> (f32, f32) {
    const MARKER_PAD: f32 = 24.0; // room for Crow's Foot markers
    let mut w = width;
    let mut h = height;
    for edge in edges {
        let r = &edge.route;
        for &x in &[r.x1, r.x2] {
            if x + MARKER_PAD > w {
                w = x + MARKER_PAD;
            }
        }
        for &y in &[r.y1, r.y2] {
            if y + MARKER_PAD > h {
                h = y + MARKER_PAD;
            }
        }
        for &(cx, cy) in &r.control_points {
            if cx + MARKER_PAD > w {
                w = cx + MARKER_PAD;
            }
            if cy + MARKER_PAD > h {
                h = cy + MARKER_PAD;
            }
        }
    }
    (w, h)
}

fn node_pair_spacing(left: NodeSize, right: NodeSize, config: &LayoutConfig) -> f32 {
    let left_radius = left.width.max(left.height) * 0.5;
    let right_radius = right.width.max(right.height) * 0.5;
    config.node_padding.mul_add(2.0, left_radius + right_radius)
}

fn display_column_text(kind: NodeKind, name: &str, data_type: &str) -> String {
    if kind == NodeKind::Enum {
        format!("• {name}")
    } else if data_type.is_empty() {
        name.to_string()
    } else {
        format!("{name}: {data_type}")
    }
}

fn estimate_text_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|ch| {
            let width_factor = match ch {
                'A'..='Z' => 0.72,
                'a'..='z' | '0'..='9' => 0.62,
                '_' | '-' | '.' | ':' | ',' | '(' | ')' | '[' | ']' | ' ' => 0.38,
                _ if ch.is_ascii_punctuation() => 0.52,
                _ if ch.is_ascii() => 0.62,
                _ => match ch.width_cjk().or_else(|| ch.width()) {
                    Some(0) => 0.0,
                    Some(1) => 0.94,
                    Some(_) => 1.12,
                    None => 1.0,
                },
            };
            font_size * width_factor
        })
        .sum()
}

/// Route all edges in the graph.
#[allow(clippy::too_many_lines)]
#[cfg_attr(not(test), allow(dead_code))] // Test helpers exercise the wrapper directly.
fn route_edges(
    graph: &LayoutGraph,
    positioned_nodes: &[PositionedNode],
    config: &LayoutConfig,
    node_ranks: Option<&[usize]>,
) -> Vec<PositionedEdge> {
    route_edges_with_diagnostics(graph, positioned_nodes, config, node_ranks).0
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct RoutingDiagnostics {
    non_self_loop_detour_activations: usize,
}

#[derive(Debug, Clone, Copy)]
struct BundleRouteMetadata {
    axis: ChannelAxis,
    coordinate: f32,
    source_side: AttachmentSide,
    target_side: AttachmentSide,
}

#[derive(Debug, Clone)]
struct RoutedEdgeDraft {
    from: String,
    to: String,
    label: String,
    kind: EdgeKind,
    route: EdgeRoute,
    is_self_loop: bool,
    nullable: bool,
    target_cardinality: relune_core::layout::Cardinality,
    from_columns: Vec<String>,
    to_columns: Vec<String>,
    is_collapsed_join: bool,
    collapsed_join_table: Option<CollapsedJoinTable>,
    bundle_metadata: Option<BundleRouteMetadata>,
    routing_debug: PositionedEdgeRoutingDebug,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BundleGroupKey {
    from: String,
    to: String,
    axis: ChannelAxis,
}

#[derive(Debug, Clone, Copy)]
struct BundleClusterStats {
    shared_channel: f32,
    source_bundle_axis: f32,
    target_bundle_axis: f32,
    anchor_distance: f32,
}

#[allow(clippy::too_many_lines)]
fn route_edges_with_diagnostics(
    graph: &LayoutGraph,
    positioned_nodes: &[PositionedNode],
    config: &LayoutConfig,
    node_ranks: Option<&[usize]>,
) -> (Vec<PositionedEdge>, RoutingDiagnostics) {
    let node_positions: BTreeMap<&str, (f32, f32, f32, f32)> = positioned_nodes
        .iter()
        .map(|node| (node.id.as_str(), (node.x, node.y, node.width, node.height)))
        .collect();
    let port_assignments = assign_edge_ports(graph, positioned_nodes, config);
    let rank_bounds = node_ranks.map(|ranks| rank_axis_bounds(positioned_nodes, ranks, config));
    let edge_counts = edge_lane_counts(graph);
    let lane_indices = edge_lane_indices(graph);
    let routing_order = stable_edge_routing_order(graph, node_ranks);
    let mut channel_usage = BTreeMap::new();
    let mut diagnostics = RoutingDiagnostics::default();

    let mut routed_edges = vec![None; graph.edges.len()];

    for &edge_index in &routing_order {
        let edge = &graph.edges[edge_index];
        let from_pos = node_positions.get(edge.from.as_str());
        let to_pos = node_positions.get(edge.to.as_str());

        let mut bundle_metadata = None;
        let mut routing_debug = PositionedEdgeRoutingDebug {
            source_side: None,
            target_side: None,
            source_slot_index: None,
            source_slot_count: None,
            target_slot_index: None,
            target_slot_count: None,
            source_row_offset: None,
            target_row_offset: None,
            channel_axis: None,
            channel_coordinate: None,
            detour_activation_counted: false,
            self_loop_radius_offset: None,
        };

        let Some(port_assignment) = port_assignments.get(edge_index).and_then(Option::as_ref)
        else {
            continue;
        };

        let detour_obstacles: Vec<Rect> = positioned_nodes
            .iter()
            .filter(|node| node.id != edge.from && node.id != edge.to)
            .map(|node| Rect {
                x: node.x,
                y: node.y,
                w: node.width,
                h: node.height,
            })
            .collect();

        let route = if let EdgePortAssignment::SelfLoop(assignment) = port_assignment {
            routing_debug.self_loop_radius_offset = Some(assignment.radius_offset);
            if let Some(&(x, y, w, h)) = from_pos {
                route_self_loop_with_offset(x, y, w, h, config.edge_style, assignment.radius_offset)
            } else {
                continue;
            }
        } else if let (
            EdgePortAssignment::Regular(assignment),
            Some(&(x1, y1, w1, h1)),
            Some(&(x2, y2, w2, h2)),
        ) = (port_assignment, from_pos, to_pos)
        {
            routing_debug = build_regular_edge_debug(*assignment);

            if let Some((candidate, source_rank, target_rank)) = node_ranks.and_then(|ranks| {
                let source_rank = node_rank_for_edge_endpoint(graph, ranks, edge.from.as_str())?;
                let target_rank = node_rank_for_edge_endpoint(graph, ranks, edge.to.as_str())?;
                obstacle_aware_channel_for_edge(
                    graph,
                    edge,
                    ranks,
                    rank_bounds.as_deref(),
                    config.direction,
                    (x1, y1, w1, h1),
                    (x2, y2, w2, h2),
                    assignment,
                    &detour_obstacles,
                    &channel_usage,
                    config.edge_style,
                )
                .map(|candidate| (candidate, source_rank, target_rank))
            }) {
                record_channel_usage(&mut channel_usage, candidate.axis, candidate.coordinate);
                bundle_metadata = Some(BundleRouteMetadata {
                    axis: candidate.axis,
                    coordinate: candidate.coordinate,
                    source_side: assignment.source_side,
                    target_side: assignment.target_side,
                });
                routing_debug.channel_axis = Some(channel_axis_name(candidate.axis).to_string());
                routing_debug.channel_coordinate = Some(candidate.coordinate);
                route_edge_with_candidate_channel(
                    x1,
                    y1,
                    w1,
                    h1,
                    x2,
                    y2,
                    w2,
                    h2,
                    config.edge_style,
                    assignment,
                    candidate,
                    config.direction,
                    source_rank,
                    target_rank,
                    rank_bounds.as_deref(),
                )
            } else {
                route_edge_with_assigned_ports(
                    x1,
                    y1,
                    w1,
                    h1,
                    x2,
                    y2,
                    w2,
                    h2,
                    config.edge_style,
                    assignment.source_side,
                    assignment.target_side,
                    assignment.source_slot_offset,
                    assignment.target_slot_offset,
                    assignment.source_row_offset,
                    assignment.target_row_offset,
                )
            }
        } else {
            continue;
        };

        let route = match edge.is_self_loop {
            true => {
                bundle_metadata = None;
                detour_around_obstacles(&route, &detour_obstacles)
            }
            false if route_needs_detour(&route, &detour_obstacles) => {
                bundle_metadata = None;
                diagnostics.non_self_loop_detour_activations += 1;
                routing_debug.detour_activation_counted = true;
                debug!(
                    edge_from = edge.from,
                    edge_to = edge.to,
                    "Obstacle-aware channel still intersects padded obstacle corridor"
                );
                route
            }
            false => route,
        };

        let label = edge.name.clone().unwrap_or_else(|| {
            if edge.from_columns.is_empty() {
                "fk".to_string()
            } else {
                edge.from_columns.join(",")
            }
        });

        routed_edges[edge_index] = Some(RoutedEdgeDraft {
            from: edge.from.clone(),
            to: edge.to.clone(),
            label,
            kind: edge.kind,
            route,
            is_self_loop: edge.is_self_loop,
            nullable: edge.nullable,
            target_cardinality: edge.target_cardinality,
            from_columns: edge.from_columns.clone(),
            to_columns: edge.to_columns.clone(),
            is_collapsed_join: edge.is_collapsed_join,
            collapsed_join_table: edge.collapsed_join_table.clone(),
            bundle_metadata,
            routing_debug,
        });
    }

    apply_parallel_edge_bundling(&mut routed_edges, positioned_nodes, graph);

    let mut edges = vec![None; graph.edges.len()];
    let mut placed_labels: Vec<Rect> = Vec::new();

    for &edge_index in &routing_order {
        let Some(edge) = routed_edges[edge_index].as_ref() else {
            continue;
        };
        let lane_key = canonical_edge_pair(&edge.from, &edge.to);
        let lane_index = lane_indices[edge_index];
        let lane_total = edge_counts.get(&lane_key).copied().unwrap_or(1);
        let from_pos = node_positions.get(edge.from.as_str());
        let to_pos = node_positions.get(edge.to.as_str());

        let mut label_obstacles: Vec<Rect> = positioned_nodes
            .iter()
            .filter(|node| node.id != edge.from && node.id != edge.to)
            .map(|node| Rect {
                x: node.x,
                y: node.y,
                w: node.width,
                h: node.height,
            })
            .collect();
        label_obstacles.extend_from_slice(&placed_labels);
        if let Some(&(x, y, w, h)) = from_pos {
            label_obstacles.push(Rect { x, y, w, h });
        }
        if !edge.is_self_loop
            && let Some(&(x, y, w, h)) = to_pos
        {
            label_obstacles.push(Rect { x, y, w, h });
        }

        let lhw = estimate_label_half_width(&edge.label);
        let label_pos = if lane_total > 1 && !edge.is_self_loop {
            let t = parallel_label_parameter(&edge.from, &edge.to, lane_index, lane_total);
            point_along_route(&edge.route, t)
        } else {
            edge.route.label_position
        };

        let preferred_t = if lane_total > 1 && !edge.is_self_loop {
            parallel_label_parameter(&edge.from, &edge.to, lane_index, lane_total)
        } else {
            estimate_route_parameter(&edge.route, label_pos)
        };
        let (label_x, label_y) =
            place_label_on_route(&edge.route, preferred_t, &label_obstacles, 4.0, lhw);
        placed_labels.push(label_rect(label_x, label_y, lhw));

        edges[edge_index] = Some(PositionedEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
            label: edge.label.clone(),
            kind: edge.kind,
            route: edge.route.clone(),
            is_self_loop: edge.is_self_loop,
            nullable: edge.nullable,
            target_cardinality: edge.target_cardinality,
            from_columns: edge.from_columns.clone(),
            to_columns: edge.to_columns.clone(),
            is_collapsed_join: edge.is_collapsed_join,
            collapsed_join_table: edge.collapsed_join_table.clone(),
            label_x,
            label_y,
            routing_debug: Some(edge.routing_debug.clone()),
        });
    }

    let mut edges: Vec<_> = edges.into_iter().flatten().collect();
    resolve_edge_label_collisions(&mut edges, positioned_nodes);
    (edges, diagnostics)
}

fn build_regular_edge_debug(assignment: RegularPortAssignment) -> PositionedEdgeRoutingDebug {
    PositionedEdgeRoutingDebug {
        source_side: Some(attachment_side_name(assignment.source_side).to_string()),
        target_side: Some(attachment_side_name(assignment.target_side).to_string()),
        source_slot_index: Some(assignment.source_slot_index),
        source_slot_count: Some(assignment.source_slot_count),
        target_slot_index: Some(assignment.target_slot_index),
        target_slot_count: Some(assignment.target_slot_count),
        source_row_offset: Some(assignment.source_row_offset),
        target_row_offset: Some(assignment.target_row_offset),
        channel_axis: None,
        channel_coordinate: None,
        detour_activation_counted: false,
        self_loop_radius_offset: None,
    }
}

const fn attachment_side_name(side: AttachmentSide) -> &'static str {
    match side {
        AttachmentSide::North => "north",
        AttachmentSide::South => "south",
        AttachmentSide::East => "east",
        AttachmentSide::West => "west",
    }
}

const fn channel_axis_name(axis: ChannelAxis) -> &'static str {
    match axis {
        ChannelAxis::X => "x",
        ChannelAxis::Y => "y",
    }
}

fn apply_parallel_edge_bundling(
    routed_edges: &mut [Option<RoutedEdgeDraft>],
    positioned_nodes: &[PositionedNode],
    graph: &LayoutGraph,
) {
    let mut groups: BTreeMap<BundleGroupKey, Vec<usize>> = BTreeMap::new();

    for (edge_index, edge) in routed_edges.iter().enumerate() {
        let Some(edge) = edge.as_ref() else {
            continue;
        };
        let Some(bundle_metadata) = edge.bundle_metadata else {
            continue;
        };
        if edge.is_self_loop {
            continue;
        }

        groups
            .entry(BundleGroupKey {
                from: edge.from.clone(),
                to: edge.to.clone(),
                axis: bundle_metadata.axis,
            })
            .or_default()
            .push(edge_index);
    }

    #[allow(clippy::cast_precision_loss)]
    let density = if graph.nodes.is_empty() {
        0.0
    } else {
        graph.edges.len() as f32 / graph.nodes.len() as f32
    };
    let channel_tolerance = bundle_channel_tolerance(density);

    for edge_indices in groups.values_mut() {
        edge_indices.sort_by(|left, right| {
            bundle_coordinate(routed_edges, *left)
                .total_cmp(&bundle_coordinate(routed_edges, *right))
                .then_with(|| left.cmp(right))
        });

        let mut cluster_start = 0usize;
        while cluster_start < edge_indices.len() {
            let mut cluster_end = cluster_start + 1;
            while cluster_end < edge_indices.len()
                && (bundle_coordinate(routed_edges, edge_indices[cluster_end])
                    - bundle_coordinate(routed_edges, edge_indices[cluster_end - 1]))
                .abs()
                    <= channel_tolerance
            {
                cluster_end += 1;
            }

            if cluster_end - cluster_start >= 2 {
                let cluster = &edge_indices[cluster_start..cluster_end];
                let anchor_distance = bundle_anchor_distance(density, cluster.len());
                let stats = bundle_cluster_stats(routed_edges, cluster, anchor_distance);
                for &edge_index in cluster {
                    let Some(edge) = routed_edges[edge_index].as_mut() else {
                        continue;
                    };
                    let Some(bundle_metadata) = edge.bundle_metadata else {
                        continue;
                    };
                    let candidate = build_bundled_route(&edge.route, bundle_metadata, stats);
                    if bundled_route_is_valid(&candidate, edge, bundle_metadata, positioned_nodes) {
                        edge.route = candidate;
                    }
                }
            }

            cluster_start = cluster_end;
        }
    }
}

fn bundle_coordinate(routed_edges: &[Option<RoutedEdgeDraft>], edge_index: usize) -> f32 {
    routed_edges[edge_index]
        .as_ref()
        .and_then(|edge| edge.bundle_metadata.map(|metadata| metadata.coordinate))
        .unwrap_or(0.0)
}

fn bundle_channel_tolerance(density: f32) -> f32 {
    if density >= 1.5 {
        BUNDLE_CHANNEL_TOLERANCE + 12.0
    } else {
        BUNDLE_CHANNEL_TOLERANCE
    }
}

fn bundle_anchor_distance(density: f32, cluster_size: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let extra = cluster_size.saturating_sub(2) as f32;
    let base = if density >= 1.5 { 14.0 } else { 18.0 };
    base + (extra * 4.0).min(12.0)
}

fn bundle_cluster_stats(
    routed_edges: &[Option<RoutedEdgeDraft>],
    cluster: &[usize],
    anchor_distance: f32,
) -> BundleClusterStats {
    let mut channel_coordinates = Vec::with_capacity(cluster.len());
    let mut source_axes = Vec::with_capacity(cluster.len());
    let mut target_axes = Vec::with_capacity(cluster.len());

    for &edge_index in cluster {
        let edge = routed_edges[edge_index]
            .as_ref()
            .expect("bundle cluster should only reference routed edges");
        let metadata = edge
            .bundle_metadata
            .expect("bundle cluster should only reference bundle-eligible edges");
        let source_anchor = step_from_attachment(
            (edge.route.x1, edge.route.y1),
            metadata.source_side,
            anchor_distance,
        );
        let target_anchor = step_from_attachment(
            (edge.route.x2, edge.route.y2),
            metadata.target_side,
            anchor_distance,
        );
        channel_coordinates.push(metadata.coordinate);
        match metadata.axis {
            ChannelAxis::X => {
                source_axes.push(source_anchor.1);
                target_axes.push(target_anchor.1);
            }
            ChannelAxis::Y => {
                source_axes.push(source_anchor.0);
                target_axes.push(target_anchor.0);
            }
        }
    }

    BundleClusterStats {
        shared_channel: median_coordinate(&mut channel_coordinates),
        source_bundle_axis: mean_coordinate(&source_axes),
        target_bundle_axis: mean_coordinate(&target_axes),
        anchor_distance,
    }
}

fn build_bundled_route(
    route: &EdgeRoute,
    metadata: BundleRouteMetadata,
    stats: BundleClusterStats,
) -> EdgeRoute {
    let source = (route.x1, route.y1);
    let target = (route.x2, route.y2);
    let source_anchor = step_from_attachment(source, metadata.source_side, stats.anchor_distance);
    let target_anchor = step_from_attachment(target, metadata.target_side, stats.anchor_distance);

    let points = match metadata.axis {
        ChannelAxis::X => vec![
            source,
            source_anchor,
            (source_anchor.0, stats.source_bundle_axis),
            (stats.shared_channel, stats.source_bundle_axis),
            (stats.shared_channel, stats.target_bundle_axis),
            (target_anchor.0, stats.target_bundle_axis),
            target_anchor,
            target,
        ],
        ChannelAxis::Y => vec![
            source,
            source_anchor,
            (stats.source_bundle_axis, source_anchor.1),
            (stats.source_bundle_axis, stats.shared_channel),
            (stats.target_bundle_axis, stats.shared_channel),
            (stats.target_bundle_axis, target_anchor.1),
            target_anchor,
            target,
        ],
    };

    rebuild_route_from_points(&points, route.style)
}

fn bundled_route_is_valid(
    route: &EdgeRoute,
    edge: &RoutedEdgeDraft,
    metadata: BundleRouteMetadata,
    positioned_nodes: &[PositionedNode],
) -> bool {
    if endpoint_side_violations(route, metadata.source_side, metadata.target_side) > 0 {
        return false;
    }

    let obstacles = positioned_nodes
        .iter()
        .filter(|node| node.id != edge.from && node.id != edge.to)
        .map(|node| Rect {
            x: node.x,
            y: node.y,
            w: node.width,
            h: node.height,
        })
        .collect::<Vec<_>>();

    route_obstacle_hit_count(route, &obstacles, 0.0) == 0
}

#[allow(clippy::cast_precision_loss)] // Bundle clusters stay tiny and only affect visual interpolation.
fn mean_coordinate(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }

    values.iter().sum::<f32>() / values.len() as f32
}

fn median_coordinate(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }

    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        f32::midpoint(values[middle - 1], values[middle])
    } else {
        values[middle]
    }
}

#[derive(Debug, Clone, Copy)]
struct RankAxisBounds {
    min: f32,
    max: f32,
}

fn rank_axis_bounds(
    positioned_nodes: &[PositionedNode],
    node_ranks: &[usize],
    config: &LayoutConfig,
) -> Vec<RankAxisBounds> {
    let rank_count = node_ranks
        .iter()
        .copied()
        .max()
        .map_or(0usize, |rank| rank + 1);
    let mut bounds = vec![
        RankAxisBounds {
            min: f32::INFINITY,
            max: f32::NEG_INFINITY,
        };
        rank_count
    ];
    let use_x_axis = matches!(
        config.direction,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft
    );

    for (node, &rank) in positioned_nodes.iter().zip(node_ranks) {
        let (min, max) = if use_x_axis {
            (node.x, node.x + node.width)
        } else {
            (node.y, node.y + node.height)
        };
        bounds[rank].min = bounds[rank].min.min(min);
        bounds[rank].max = bounds[rank].max.max(max);
    }

    bounds
}

fn inter_rank_channel(
    source_rank: usize,
    target_rank: usize,
    rank_bounds: &[RankAxisBounds],
) -> Option<f32> {
    let source = *rank_bounds.get(source_rank)?;
    let target = *rank_bounds.get(target_rank)?;
    if source.min <= target.min {
        Some(f32::midpoint(source.max, target.min))
    } else {
        Some(f32::midpoint(source.min, target.max))
    }
}

fn same_rank_x_channel(
    source_rect: (f32, f32, f32, f32),
    target_rect: (f32, f32, f32, f32),
) -> f32 {
    let source_center = source_rect.0 + source_rect.2 / 2.0;
    let target_center = target_rect.0 + target_rect.2 / 2.0;
    if source_center <= target_center {
        f32::midpoint(source_rect.0 + source_rect.2, target_rect.0)
    } else {
        f32::midpoint(source_rect.0, target_rect.0 + target_rect.2)
    }
}

fn same_rank_y_channel(
    source_rect: (f32, f32, f32, f32),
    target_rect: (f32, f32, f32, f32),
) -> f32 {
    let source_center = source_rect.1 + source_rect.3 / 2.0;
    let target_center = target_rect.1 + target_rect.3 / 2.0;
    if source_center <= target_center {
        f32::midpoint(source_rect.1 + source_rect.3, target_rect.1)
    } else {
        f32::midpoint(source_rect.1, target_rect.1 + target_rect.3)
    }
}

#[derive(Debug, Clone, Copy)]
struct ChannelSearchPlan {
    axis: ChannelAxis,
    baseline: f32,
    class: ChannelCandidateClass,
}

#[derive(Debug, Clone, Copy)]
struct ObstacleAwareChannelCandidate {
    axis: ChannelAxis,
    coordinate: f32,
    baseline: f32,
    stable_order: u32,
}

#[allow(clippy::too_many_arguments)]
fn obstacle_aware_channel_for_edge(
    graph: &LayoutGraph,
    edge: &crate::graph::LayoutEdge,
    node_ranks: &[usize],
    rank_bounds: Option<&[RankAxisBounds]>,
    direction: LayoutDirection,
    source_rect: (f32, f32, f32, f32),
    target_rect: (f32, f32, f32, f32),
    assignment: &RegularPortAssignment,
    obstacles: &[Rect],
    channel_usage: &BTreeMap<(ChannelAxis, i32), u32>,
    style: RouteStyle,
) -> Option<ObstacleAwareChannelCandidate> {
    let source_rank = node_rank_for_edge_endpoint(graph, node_ranks, edge.from.as_str())?;
    let target_rank = node_rank_for_edge_endpoint(graph, node_ranks, edge.to.as_str())?;
    let search_plan = channel_search_plan(
        source_rank,
        target_rank,
        rank_bounds?,
        direction,
        source_rect,
        target_rect,
    )?;
    let weights = ChannelCostWeights::default();
    let mut best_candidate = None;
    let mut best_score = None;
    let mut candidates = channel_candidates(search_plan, source_rank, target_rank, rank_bounds?);
    if search_plan.class != ChannelCandidateClass::SameRank {
        let start_order = u32::try_from(candidates.len()).expect("candidate count should fit u32");
        candidates.extend(bypass_channel_candidates(
            direction,
            source_rect,
            target_rect,
            start_order,
        ));
    }

    for candidate in candidates {
        let route = route_edge_with_candidate_channel(
            source_rect.0,
            source_rect.1,
            source_rect.2,
            source_rect.3,
            target_rect.0,
            target_rect.1,
            target_rect.2,
            target_rect.3,
            style,
            assignment,
            candidate,
            direction,
            source_rank,
            target_rank,
            rank_bounds,
        );
        let score = score_channel_candidate(
            &route,
            obstacles,
            assignment.source_side,
            assignment.target_side,
            candidate,
            channel_usage,
        );
        if score.hard_constraint_violations != 0 {
            continue;
        }
        let is_better = best_score
            .is_none_or(|best| compare_channel_candidate_scores(score, best, weights).is_lt());
        if is_better {
            best_candidate = Some(candidate);
            best_score = Some(score);
        }
    }

    best_candidate
}

fn channel_search_plan(
    source_rank: usize,
    target_rank: usize,
    rank_bounds: &[RankAxisBounds],
    direction: LayoutDirection,
    source_rect: (f32, f32, f32, f32),
    target_rect: (f32, f32, f32, f32),
) -> Option<ChannelSearchPlan> {
    let same_rank = source_rank == target_rank;
    let baseline = match direction {
        LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => {
            if same_rank {
                same_rank_x_channel(source_rect, target_rect)
            } else {
                inter_rank_channel(source_rank, target_rank, rank_bounds)?
            }
        }
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => {
            if same_rank {
                same_rank_y_channel(source_rect, target_rect)
            } else {
                inter_rank_channel(source_rank, target_rank, rank_bounds)?
            }
        }
    };
    let axis = match direction {
        LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => {
            if same_rank {
                ChannelAxis::X
            } else {
                ChannelAxis::Y
            }
        }
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => {
            if same_rank {
                ChannelAxis::Y
            } else {
                ChannelAxis::X
            }
        }
    };
    let class = if same_rank {
        ChannelCandidateClass::SameRank
    } else if source_rank > target_rank {
        ChannelCandidateClass::ReverseEdge
    } else {
        ChannelCandidateClass::InterRank
    };

    Some(ChannelSearchPlan {
        axis,
        baseline,
        class,
    })
}

fn channel_candidates(
    plan: ChannelSearchPlan,
    source_rank: usize,
    target_rank: usize,
    rank_bounds: &[RankAxisBounds],
) -> Vec<ObstacleAwareChannelCandidate> {
    let mut candidates = Vec::with_capacity(plan.class.search_offsets().len());

    for (stable_order, offset) in plan.class.search_offsets().iter().copied().enumerate() {
        let coordinate = plan.baseline + offset;
        if plan.class == ChannelCandidateClass::InterRank
            && !inter_rank_candidate_within_gap(coordinate, source_rank, target_rank, rank_bounds)
        {
            continue;
        }

        #[allow(clippy::cast_possible_truncation)]
        candidates.push(ObstacleAwareChannelCandidate {
            axis: plan.axis,
            coordinate,
            baseline: plan.baseline,
            stable_order: stable_order as u32,
        });
    }

    if candidates.is_empty() {
        candidates.push(ObstacleAwareChannelCandidate {
            axis: plan.axis,
            coordinate: plan.baseline,
            baseline: plan.baseline,
            stable_order: 0,
        });
    }

    candidates
}

fn bypass_channel_candidates(
    direction: LayoutDirection,
    source_rect: (f32, f32, f32, f32),
    target_rect: (f32, f32, f32, f32),
    start_order: u32,
) -> Vec<ObstacleAwareChannelCandidate> {
    let mut candidates = Vec::with_capacity(BYPASS_CHANNEL_OFFSETS.len() * 2);

    match direction {
        LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => {
            let right_baseline = (source_rect.0 + source_rect.2).max(target_rect.0 + target_rect.2)
                + BYPASS_CHANNEL_MARGIN;
            let left_baseline = source_rect.0.min(target_rect.0) - BYPASS_CHANNEL_MARGIN;
            append_bypass_candidates(
                &mut candidates,
                ChannelAxis::X,
                right_baseline,
                left_baseline,
                start_order,
            );
        }
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => {
            let bottom_baseline = (source_rect.1 + source_rect.3)
                .max(target_rect.1 + target_rect.3)
                + BYPASS_CHANNEL_MARGIN;
            let top_baseline = source_rect.1.min(target_rect.1) - BYPASS_CHANNEL_MARGIN;
            append_bypass_candidates(
                &mut candidates,
                ChannelAxis::Y,
                bottom_baseline,
                top_baseline,
                start_order,
            );
        }
    }

    candidates
}

fn append_bypass_candidates(
    candidates: &mut Vec<ObstacleAwareChannelCandidate>,
    axis: ChannelAxis,
    positive_baseline: f32,
    negative_baseline: f32,
    start_order: u32,
) {
    for (offset_index, offset) in BYPASS_CHANNEL_OFFSETS.iter().copied().enumerate() {
        let stable_order =
            start_order + u32::try_from(offset_index.saturating_mul(2)).expect("offset index");
        candidates.push(ObstacleAwareChannelCandidate {
            axis,
            coordinate: positive_baseline + offset,
            baseline: positive_baseline,
            stable_order,
        });
        candidates.push(ObstacleAwareChannelCandidate {
            axis,
            coordinate: negative_baseline - offset,
            baseline: negative_baseline,
            stable_order: stable_order + 1,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn route_edge_with_candidate_channel(
    x1: f32,
    y1: f32,
    w1: f32,
    h1: f32,
    x2: f32,
    y2: f32,
    w2: f32,
    h2: f32,
    style: RouteStyle,
    assignment: &RegularPortAssignment,
    candidate: ObstacleAwareChannelCandidate,
    direction: LayoutDirection,
    source_rank: usize,
    target_rank: usize,
    rank_bounds: Option<&[RankAxisBounds]>,
) -> EdgeRoute {
    let seed_route = route_edge_with_assigned_ports(
        x1,
        y1,
        w1,
        h1,
        x2,
        y2,
        w2,
        h2,
        style,
        assignment.source_side,
        assignment.target_side,
        assignment.source_slot_offset,
        assignment.target_slot_offset,
        assignment.source_row_offset,
        assignment.target_row_offset,
    );
    let source = (seed_route.x1, seed_route.y1);
    let target = (seed_route.x2, seed_route.y2);
    let (source_anchor, target_anchor) = candidate_channel_anchors(
        source,
        target,
        assignment.source_side,
        assignment.target_side,
        candidate,
        direction,
        source_rank,
        target_rank,
        rank_bounds,
    );

    let points = match candidate.axis {
        ChannelAxis::X => vec![
            source,
            source_anchor,
            (candidate.coordinate, source_anchor.1),
            (candidate.coordinate, target_anchor.1),
            target_anchor,
            target,
        ],
        ChannelAxis::Y => vec![
            source,
            source_anchor,
            (source_anchor.0, candidate.coordinate),
            (target_anchor.0, candidate.coordinate),
            target_anchor,
            target,
        ],
    };

    rebuild_route_from_points(&points, style)
}

#[allow(clippy::too_many_arguments)]
fn candidate_channel_anchors(
    source: (f32, f32),
    target: (f32, f32),
    source_side: AttachmentSide,
    target_side: AttachmentSide,
    candidate: ObstacleAwareChannelCandidate,
    direction: LayoutDirection,
    source_rank: usize,
    target_rank: usize,
    rank_bounds: Option<&[RankAxisBounds]>,
) -> ((f32, f32), (f32, f32)) {
    match (direction, candidate.axis, rank_bounds) {
        (
            LayoutDirection::TopToBottom | LayoutDirection::BottomToTop,
            ChannelAxis::X,
            Some(bounds),
        ) if source_rank != target_rank => {
            let Some(source_bounds) = bounds.get(source_rank).copied() else {
                return (
                    step_from_attachment(source, source_side, ROUTE_STUB_DISTANCE),
                    step_from_attachment(target, target_side, ROUTE_STUB_DISTANCE),
                );
            };
            let Some(target_bounds) = bounds.get(target_rank).copied() else {
                return (
                    step_from_attachment(source, source_side, ROUTE_STUB_DISTANCE),
                    step_from_attachment(target, target_side, ROUTE_STUB_DISTANCE),
                );
            };
            if source_rank < target_rank {
                (
                    (source.0, source_bounds.max + ROUTE_STUB_DISTANCE),
                    (target.0, target_bounds.min - ROUTE_STUB_DISTANCE),
                )
            } else {
                (
                    (source.0, source_bounds.min - ROUTE_STUB_DISTANCE),
                    (target.0, target_bounds.max + ROUTE_STUB_DISTANCE),
                )
            }
        }
        (
            LayoutDirection::LeftToRight | LayoutDirection::RightToLeft,
            ChannelAxis::Y,
            Some(bounds),
        ) if source_rank != target_rank => {
            let Some(source_bounds) = bounds.get(source_rank).copied() else {
                return (
                    step_from_attachment(source, source_side, ROUTE_STUB_DISTANCE),
                    step_from_attachment(target, target_side, ROUTE_STUB_DISTANCE),
                );
            };
            let Some(target_bounds) = bounds.get(target_rank).copied() else {
                return (
                    step_from_attachment(source, source_side, ROUTE_STUB_DISTANCE),
                    step_from_attachment(target, target_side, ROUTE_STUB_DISTANCE),
                );
            };
            if source_rank < target_rank {
                (
                    (source_bounds.max + ROUTE_STUB_DISTANCE, source.1),
                    (target_bounds.min - ROUTE_STUB_DISTANCE, target.1),
                )
            } else {
                (
                    (source_bounds.min - ROUTE_STUB_DISTANCE, source.1),
                    (target_bounds.max + ROUTE_STUB_DISTANCE, target.1),
                )
            }
        }
        _ => (
            step_from_attachment(source, source_side, ROUTE_STUB_DISTANCE),
            step_from_attachment(target, target_side, ROUTE_STUB_DISTANCE),
        ),
    }
}

fn inter_rank_candidate_within_gap(
    coordinate: f32,
    source_rank: usize,
    target_rank: usize,
    rank_bounds: &[RankAxisBounds],
) -> bool {
    let Some(source) = rank_bounds.get(source_rank).copied() else {
        return false;
    };
    let Some(target) = rank_bounds.get(target_rank).copied() else {
        return false;
    };

    let (lower, upper) = if source.min <= target.min {
        (source.max, target.min)
    } else {
        (target.max, source.min)
    };

    if lower > upper {
        return true;
    }

    coordinate >= lower && coordinate <= upper
}

fn node_rank_for_edge_endpoint(
    graph: &LayoutGraph,
    node_ranks: &[usize],
    node_id: &str,
) -> Option<usize> {
    graph
        .node_index
        .get(node_id)
        .and_then(|&index| node_ranks.get(index))
        .copied()
}

fn score_channel_candidate(
    route: &EdgeRoute,
    obstacles: &[Rect],
    source_side: AttachmentSide,
    target_side: AttachmentSide,
    candidate: ObstacleAwareChannelCandidate,
    channel_usage: &BTreeMap<(ChannelAxis, i32), u32>,
) -> ChannelCandidateScore {
    let hard_constraint_violations = clipped_u16(route_obstacle_hit_count(route, obstacles, 0.0))
        + endpoint_side_violations(route, source_side, target_side);

    ChannelCandidateScore {
        hard_constraint_violations,
        clearance_penalty: route_clearance_penalty(route, obstacles, ROUTE_CLEARANCE_TARGET),
        total_length: rounded_metric(approximate_route_length(route)),
        bend_count: clipped_u16(route.control_points.len()),
        center_deviation: rounded_metric((candidate.coordinate - candidate.baseline).abs()),
        congestion_penalty: channel_congestion_penalty(
            channel_usage,
            candidate.axis,
            candidate.coordinate,
        ),
        stable_order: candidate.stable_order,
    }
}

fn route_needs_detour(route: &EdgeRoute, obstacles: &[Rect]) -> bool {
    route_obstacle_hit_count(route, obstacles, ROUTE_CLEARANCE_TARGET) > 0
}

fn route_obstacle_hit_count(route: &EdgeRoute, obstacles: &[Rect], padding: f32) -> usize {
    let points = route_points(route);
    obstacles
        .iter()
        .filter(|obstacle| {
            let inflated = inflate_rect(**obstacle, padding);
            points
                .windows(2)
                .any(|segment| segment_intersects_rect(segment[0], segment[1], &inflated))
        })
        .count()
}

fn route_clearance_penalty(route: &EdgeRoute, obstacles: &[Rect], clearance: f32) -> u32 {
    let points = route_points(route);
    obstacles
        .iter()
        .map(|obstacle| {
            points
                .windows(2)
                .map(|segment| {
                    segment_clearance_deficit(segment[0], segment[1], obstacle, clearance)
                })
                .max()
                .unwrap_or(0)
        })
        .sum()
}

fn endpoint_side_violations(
    route: &EdgeRoute,
    source_side: AttachmentSide,
    target_side: AttachmentSide,
) -> u16 {
    let points = route_points(route);
    let Some(first_segment) = points.windows(2).next() else {
        return 2;
    };
    let Some(last_segment) = points.windows(2).last() else {
        return 2;
    };

    u16::from(!segment_matches_side(
        first_segment[0],
        first_segment[1],
        source_side,
    )) + u16::from(!segment_matches_side(
        last_segment[1],
        last_segment[0],
        target_side,
    ))
}

fn segment_matches_side(start: (f32, f32), next: (f32, f32), side: AttachmentSide) -> bool {
    let dx = next.0 - start.0;
    let dy = next.1 - start.1;
    let epsilon = 0.5;

    match side {
        AttachmentSide::North => dx.abs() <= epsilon && dy < -epsilon,
        AttachmentSide::South => dx.abs() <= epsilon && dy > epsilon,
        AttachmentSide::East => dy.abs() <= epsilon && dx > epsilon,
        AttachmentSide::West => dy.abs() <= epsilon && dx < -epsilon,
    }
}

fn channel_congestion_penalty(
    channel_usage: &BTreeMap<(ChannelAxis, i32), u32>,
    axis: ChannelAxis,
    coordinate: f32,
) -> u32 {
    let quantized = quantize_channel_coordinate(coordinate);
    channel_usage
        .get(&(axis, quantized))
        .copied()
        .unwrap_or(0)
        .saturating_mul(2)
}

fn record_channel_usage(
    channel_usage: &mut BTreeMap<(ChannelAxis, i32), u32>,
    axis: ChannelAxis,
    coordinate: f32,
) {
    *channel_usage
        .entry((axis, quantize_channel_coordinate(coordinate)))
        .or_insert(0) += 1;
}

fn quantize_channel_coordinate(coordinate: f32) -> i32 {
    #[allow(clippy::cast_possible_truncation)]
    let quantized = (coordinate * 2.0).round() as i32;
    quantized
}

const fn rounded_metric(value: f32) -> u32 {
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let rounded = value.round().max(0.0) as u32;
    rounded
}

fn clipped_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn segment_clearance_deficit(
    start: (f32, f32),
    end: (f32, f32),
    rect: &Rect,
    clearance: f32,
) -> u32 {
    let distance = axis_aligned_segment_distance_to_rect(start, end, rect);
    rounded_metric((clearance - distance).max(0.0))
}

fn axis_aligned_segment_distance_to_rect(start: (f32, f32), end: (f32, f32), rect: &Rect) -> f32 {
    let rect_min_x = rect.x;
    let rect_max_x = rect.x + rect.w;
    let rect_min_y = rect.y;
    let rect_max_y = rect.y + rect.h;

    if (start.0 - end.0).abs() <= 0.5 {
        let x = start.0;
        let segment_min_y = start.1.min(end.1);
        let segment_max_y = start.1.max(end.1);
        let dx = interval_gap(x, x, rect_min_x, rect_max_x);
        let dy = interval_gap(segment_min_y, segment_max_y, rect_min_y, rect_max_y);
        dx.hypot(dy)
    } else {
        let y = start.1;
        let segment_min_x = start.0.min(end.0);
        let segment_max_x = start.0.max(end.0);
        let dx = interval_gap(segment_min_x, segment_max_x, rect_min_x, rect_max_x);
        let dy = interval_gap(y, y, rect_min_y, rect_max_y);
        dx.hypot(dy)
    }
}

fn interval_gap(start_min: f32, start_max: f32, end_min: f32, end_max: f32) -> f32 {
    if start_max < end_min {
        end_min - start_max
    } else if end_max < start_min {
        start_min - end_max
    } else {
        0.0
    }
}

fn inflate_rect(rect: Rect, padding: f32) -> Rect {
    Rect {
        x: rect.x - padding,
        y: rect.y - padding,
        w: padding.mul_add(2.0, rect.w),
        h: padding.mul_add(2.0, rect.h),
    }
}

fn segment_intersects_rect(start: (f32, f32), end: (f32, f32), rect: &Rect) -> bool {
    let margin = 2.0;
    let rx = rect.x + margin;
    let ry = rect.y + margin;
    let rw = (rect.w - margin * 2.0).max(0.0);
    let rh = (rect.h - margin * 2.0).max(0.0);

    let seg_min_x = start.0.min(end.0);
    let seg_max_x = start.0.max(end.0);
    let seg_min_y = start.1.min(end.1);
    let seg_max_y = start.1.max(end.1);
    if seg_max_x < rx || seg_min_x > rx + rw || seg_max_y < ry || seg_min_y > ry + rh {
        return false;
    }

    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let clips = [
        (-dx, start.0 - rx),
        (dx, rx + rw - start.0),
        (-dy, start.1 - ry),
        (dy, ry + rh - start.1),
    ];
    let mut t_enter: f32 = 0.0;
    let mut t_leave: f32 = 1.0;

    for (p, q) in clips {
        if p.abs() < 1e-9 {
            if q < 0.0 {
                return false;
            }
            continue;
        }

        let t = q / p;
        if p < 0.0 {
            t_enter = t_enter.max(t);
        } else {
            t_leave = t_leave.min(t);
        }
        if t_enter > t_leave {
            return false;
        }
    }

    true
}

fn stable_edge_routing_order(graph: &LayoutGraph, node_ranks: Option<&[usize]>) -> Vec<usize> {
    let mut edge_indices: Vec<_> = (0..graph.edges.len()).collect();
    edge_indices.sort_by(|&left_index, &right_index| {
        let left = &graph.edges[left_index];
        let right = &graph.edges[right_index];
        edge_sort_rank(graph, node_ranks, left.from.as_str())
            .cmp(&edge_sort_rank(graph, node_ranks, right.from.as_str()))
            .then_with(|| {
                edge_sort_rank(graph, node_ranks, left.to.as_str()).cmp(&edge_sort_rank(
                    graph,
                    node_ranks,
                    right.to.as_str(),
                ))
            })
            .then_with(|| left.from.cmp(&right.from))
            .then_with(|| left.to.cmp(&right.to))
            .then_with(|| left.from_columns.cmp(&right.from_columns))
            .then_with(|| left.to_columns.cmp(&right.to_columns))
            .then_with(|| left_index.cmp(&right_index))
    });
    edge_indices
}

fn edge_sort_rank(graph: &LayoutGraph, node_ranks: Option<&[usize]>, node_id: &str) -> usize {
    node_ranks
        .and_then(|ranks| node_rank_for_edge_endpoint(graph, ranks, node_id))
        .unwrap_or(usize::MAX)
}

fn edge_lane_indices(graph: &LayoutGraph) -> Vec<usize> {
    let mut seen = BTreeMap::new();
    graph
        .edges
        .iter()
        .map(|edge| {
            let key = canonical_edge_pair(&edge.from, &edge.to);
            let entry = seen.entry(key).or_insert(0usize);
            let lane_index = *entry;
            *entry += 1;
            lane_index
        })
        .collect()
}

fn canonical_edge_pair(from: &str, to: &str) -> (String, String) {
    if from <= to {
        (from.to_string(), to.to_string())
    } else {
        (to.to_string(), from.to_string())
    }
}

fn edge_lane_counts(graph: &LayoutGraph) -> std::collections::BTreeMap<(String, String), usize> {
    let mut counts = std::collections::BTreeMap::new();
    for edge in &graph.edges {
        *counts
            .entry(canonical_edge_pair(&edge.from, &edge.to))
            .or_insert(0) += 1;
    }
    counts
}

/// Half-size of sampled edge-path obstacles used during label collision avoidance.
const EDGE_ROUTE_OBSTACLE_HALF_SIZE: f32 = 7.0;
/// Target spacing between sampled edge-path obstacles.
const EDGE_ROUTE_OBSTACLE_SPACING: f32 = 10.0;
/// Number of label-relaxation passes after all edge routes are known.
const EDGE_LABEL_RELAXATION_PASSES: usize = 3;
/// Labels should stay away from edge endpoints and markers.
const MIN_LABEL_ROUTE_T: f32 = 0.16;
/// Candidate stride when sliding labels along their own route.
const LABEL_ROUTE_T_STEP: f32 = 0.08;
/// Maximum perpendicular fallback when a label cannot fit anywhere on its own route.
const LABEL_ROUTE_FALLBACK_MAX_OFFSET: f32 = 96.0;

#[allow(clippy::cast_precision_loss)] // Edge fan-out counts are small in practice and only affect presentation.
fn parallel_label_parameter(from: &str, to: &str, lane_index: usize, lane_total: usize) -> f32 {
    let position = (lane_index + 1) as f32 / (lane_total + 1) as f32;
    if from <= to { position } else { 1.0 - position }
}

fn label_rect(label_x: f32, label_y: f32, label_half_w: f32) -> Rect {
    Rect {
        x: label_x - label_half_w,
        y: label_y - LABEL_HALF_H,
        w: label_half_w * 2.0,
        h: LABEL_HALF_H * 2.0,
    }
}

fn rect_overlaps_any(label: Rect, obstacles: &[Rect], margin: f32) -> bool {
    obstacles.iter().any(|obstacle| {
        label.x + label.w + margin > obstacle.x
            && label.x - margin < obstacle.x + obstacle.w
            && label.y + label.h + margin > obstacle.y
            && label.y - margin < obstacle.y + obstacle.h
    })
}

fn label_candidate_parameters(preferred_t: f32) -> Vec<f32> {
    let clamped = preferred_t.clamp(MIN_LABEL_ROUTE_T, 1.0 - MIN_LABEL_ROUTE_T);
    let mut candidates = vec![clamped];
    let mut delta = LABEL_ROUTE_T_STEP;
    while clamped - delta >= MIN_LABEL_ROUTE_T || clamped + delta <= 1.0 - MIN_LABEL_ROUTE_T {
        if clamped - delta >= MIN_LABEL_ROUTE_T {
            candidates.push(clamped - delta);
        }
        if clamped + delta <= 1.0 - MIN_LABEL_ROUTE_T {
            candidates.push(clamped + delta);
        }
        delta += LABEL_ROUTE_T_STEP;
    }
    candidates
}

fn place_label_on_route(
    route: &EdgeRoute,
    preferred_t: f32,
    obstacles: &[Rect],
    margin: f32,
    label_half_w: f32,
) -> (f32, f32) {
    let candidates = label_candidate_parameters(preferred_t);
    let mut best = point_along_route(route, candidates[0]);
    let mut best_overlap_area = f32::MAX;

    for t in candidates {
        let candidate = point_along_route(route, t);
        let label = label_rect(candidate.0, candidate.1, label_half_w);
        if !rect_overlaps_any(label, obstacles, margin) {
            return candidate;
        }

        let overlap_area: f32 = obstacles
            .iter()
            .map(|obstacle| {
                let overlap_w =
                    (label.x + label.w).min(obstacle.x + obstacle.w) - label.x.max(obstacle.x);
                let overlap_h =
                    (label.y + label.h).min(obstacle.y + obstacle.h) - label.y.max(obstacle.y);
                overlap_w.max(0.0) * overlap_h.max(0.0)
            })
            .sum();
        if overlap_area < best_overlap_area {
            best_overlap_area = overlap_area;
            best = candidate;
        }
    }

    nudge_label(
        best,
        (route.x1, route.y1),
        (route.x2, route.y2),
        obstacles,
        margin,
        label_half_w,
        LABEL_ROUTE_FALLBACK_MAX_OFFSET.max(label_half_w),
    )
}

fn estimate_route_parameter(route: &EdgeRoute, point: (f32, f32)) -> f32 {
    let samples = 24usize;
    let mut best_t = 0.5;
    let mut best_distance = f32::MAX;
    #[allow(clippy::cast_precision_loss)]
    for index in 0..=samples {
        let t = index as f32 / samples as f32;
        let candidate = point_along_route(route, t);
        let distance = (candidate.0 - point.0).hypot(candidate.1 - point.1);
        if distance < best_distance {
            best_distance = distance;
            best_t = t;
        }
    }
    best_t
}

fn resolve_edge_label_collisions(
    edges: &mut [PositionedEdge],
    positioned_nodes: &[PositionedNode],
) {
    if edges.is_empty() {
        return;
    }

    let node_obstacles: Vec<Rect> = positioned_nodes
        .iter()
        .map(|node| Rect {
            x: node.x,
            y: node.y,
            w: node.width,
            h: node.height,
        })
        .collect();
    let route_obstacles: Vec<Vec<Rect>> = edges
        .iter()
        .map(|edge| {
            sample_route_obstacles(
                &edge.route,
                EDGE_ROUTE_OBSTACLE_HALF_SIZE,
                EDGE_ROUTE_OBSTACLE_SPACING,
            )
        })
        .collect();

    for _ in 0..EDGE_LABEL_RELAXATION_PASSES {
        let mut changed = false;

        for index in 0..edges.len() {
            let label_half_w = estimate_label_half_width(&edges[index].label);
            let mut obstacles =
                Vec::with_capacity(node_obstacles.len() + edges.len().saturating_mul(2));
            obstacles.extend_from_slice(&node_obstacles);

            for (other_index, other_edge) in edges.iter().enumerate() {
                if other_index == index {
                    continue;
                }

                obstacles.push(label_rect(
                    other_edge.label_x,
                    other_edge.label_y,
                    estimate_label_half_width(&other_edge.label),
                ));
                obstacles.extend_from_slice(&route_obstacles[other_index]);
            }

            let current_t = estimate_route_parameter(
                &edges[index].route,
                (edges[index].label_x, edges[index].label_y),
            );
            let updated = place_label_on_route(
                &edges[index].route,
                current_t,
                &obstacles,
                4.0,
                label_half_w,
            );

            if (updated.0 - edges[index].label_x).abs() > f32::EPSILON
                || (updated.1 - edges[index].label_y).abs() > f32::EPSILON
            {
                edges[index].label_x = updated.0;
                edges[index].label_y = updated.1;
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }
}

/// Calculate positions for groups.
fn position_groups(
    groups: &[crate::graph::LayoutGroup],
    positioned_nodes: &[PositionedNode],
) -> Vec<PositionedGroup> {
    if groups.is_empty() {
        return Vec::new();
    }

    let padding = 20.0;

    groups
        .iter()
        .map(|group| {
            let mut invalid_indices = Vec::new();
            let group_nodes: Vec<&PositionedNode> = group
                .node_indices
                .iter()
                .filter_map(|&idx| {
                    positioned_nodes.get(idx).or_else(|| {
                        invalid_indices.push(idx);
                        None
                    })
                })
                .collect();

            if !invalid_indices.is_empty() {
                warn!(
                    group = %group.id,
                    invalid_indices = ?invalid_indices,
                    "Skipping invalid group node indices"
                );
            }

            if group_nodes.is_empty() {
                return PositionedGroup {
                    id: group.id.clone(),
                    label: group.label.clone(),
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                };
            }

            let min_x = group_nodes.iter().map(|n| n.x).fold(f32::MAX, f32::min);
            let min_y = group_nodes.iter().map(|n| n.y).fold(f32::MAX, f32::min);
            let max_x = group_nodes
                .iter()
                .map(|n| n.x + n.width)
                .fold(f32::MIN, f32::max);
            let max_y = group_nodes
                .iter()
                .map(|n| n.y + n.height)
                .fold(f32::MIN, f32::max);

            PositionedGroup {
                id: group.id.clone(),
                label: group.label.clone(),
                x: min_x - padding,
                y: min_y - padding,
                width: max_x - min_x + padding * 2.0,
                height: max_y - min_y + padding * 2.0,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::graph::{LayoutEdge, LayoutGraph};
    use crate::port::column_y_offset_from_center;
    use relune_core::{
        Column, ColumnId, ForeignKey, LayoutCompactionSpec, LayoutSpec, ReferentialAction, Table,
        TableId,
    };

    fn make_test_schema() -> Schema {
        Schema {
            tables: vec![
                Table {
                    id: TableId(1),
                    stable_id: "users".to_string(),
                    schema_name: None,
                    name: "users".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(1),
                            name: "id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: true,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(2),
                            name: "name".to_string(),
                            data_type: "varchar".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(2),
                    stable_id: "posts".to_string(),
                    schema_name: None,
                    name: "posts".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(3),
                            name: "id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: true,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(4),
                            name: "user_id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![ForeignKey {
                        name: Some("fk_posts_user".to_string()),
                        from_columns: vec!["user_id".to_string()],
                        to_schema: None,
                        to_table: "users".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    }],
                    indexes: vec![],
                    comment: None,
                },
            ],
            views: vec![],
            enums: vec![],
        }
    }

    fn single_edge_graph(from: &str, to: &str) -> LayoutGraph {
        let mut node_index = std::collections::BTreeMap::new();
        node_index.insert(from.to_string(), 0usize);
        node_index.insert(to.to_string(), 1usize);

        LayoutGraph {
            nodes: Vec::new(),
            edges: vec![LayoutEdge {
                from: from.to_string(),
                to: to.to_string(),
                name: Some("fk".to_string()),
                from_columns: Vec::new(),
                to_columns: Vec::new(),
                kind: EdgeKind::ForeignKey,
                is_self_loop: false,
                nullable: false,
                target_cardinality: relune_core::layout::Cardinality::One,
                is_collapsed_join: false,
                collapsed_join_table: None,
            }],
            groups: Vec::new(),
            node_index,
            reverse_index: std::collections::BTreeMap::new(),
        }
    }

    fn make_variable_width_schema() -> Schema {
        Schema {
            tables: vec![
                Table {
                    id: TableId(10),
                    stable_id: "tiny".to_string(),
                    schema_name: None,
                    name: "tiny".to_string(),
                    columns: vec![Column {
                        id: ColumnId(10),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(11),
                    stable_id: "extraordinarily_verbose_audit_log_entries".to_string(),
                    schema_name: None,
                    name: "extraordinarily_verbose_audit_log_entries".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(11),
                            name: "id".to_string(),
                            data_type: "uuid".to_string(),
                            nullable: false,
                            is_primary_key: true,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(12),
                            name: "very_long_business_context_identifier".to_string(),
                            data_type: "timestamp with time zone".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(12),
                    stable_id: "medium".to_string(),
                    schema_name: None,
                    name: "medium".to_string(),
                    columns: vec![Column {
                        id: ColumnId(13),
                        name: "display_name".to_string(),
                        data_type: "varchar(255)".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                    }],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
            ],
            views: vec![],
            enums: vec![],
        }
    }

    fn make_tall_rank_schema() -> Schema {
        let columns = (0_u64..18)
            .map(|index| Column {
                id: ColumnId(100 + index),
                name: format!("extremely_long_column_name_{index:02}"),
                data_type: "character varying(255)".to_string(),
                nullable: index % 2 == 0,
                is_primary_key: index == 0,
                comment: None,
            })
            .collect();

        Schema {
            tables: vec![
                Table {
                    id: TableId(20),
                    stable_id: "audit_event_log_entries".to_string(),
                    schema_name: Some("analytics".to_string()),
                    name: "audit_event_log_entries".to_string(),
                    columns,
                    foreign_keys: vec![ForeignKey {
                        name: Some("fk_audit_event_log_entries_user_accounts".to_string()),
                        from_columns: vec!["extremely_long_column_name_01".to_string()],
                        to_schema: None,
                        to_table: "user_accounts".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    }],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(21),
                    stable_id: "user_accounts".to_string(),
                    schema_name: Some("analytics".to_string()),
                    name: "user_accounts".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(200),
                            name: "id".to_string(),
                            data_type: "uuid".to_string(),
                            nullable: false,
                            is_primary_key: true,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(201),
                            name: "display_name".to_string(),
                            data_type: "varchar(255)".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
            ],
            views: vec![],
            enums: vec![],
        }
    }

    fn make_fully_connected_cycle_schema() -> Schema {
        let table_names = ["accounts", "projects", "roles", "teams"];
        let tables = table_names
            .iter()
            .enumerate()
            .map(|(table_idx, table_name)| {
                let base_id = u64::try_from(table_idx * 10).unwrap();
                let columns = std::iter::once(Column {
                    id: ColumnId(base_id + 1),
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                    is_primary_key: true,
                    comment: None,
                })
                .chain(
                    table_names
                        .iter()
                        .enumerate()
                        .filter(move |(_, candidate)| *candidate != table_name)
                        .map(|(target_idx, target_name)| Column {
                            id: ColumnId(base_id + u64::try_from(target_idx).unwrap() + 2),
                            name: format!("{target_name}_id"),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        }),
                )
                .collect();
                let foreign_keys = table_names
                    .iter()
                    .filter(|candidate| *candidate != table_name)
                    .map(|target_name| ForeignKey {
                        name: Some(format!("fk_{table_name}_{target_name}")),
                        from_columns: vec![format!("{target_name}_id")],
                        to_schema: None,
                        to_table: (*target_name).to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    })
                    .collect();

                Table {
                    id: TableId(u64::try_from(table_idx).unwrap() + 40),
                    stable_id: (*table_name).to_string(),
                    schema_name: None,
                    name: (*table_name).to_string(),
                    columns,
                    foreign_keys,
                    indexes: vec![],
                    comment: None,
                }
            })
            .collect();

        Schema {
            tables,
            views: vec![],
            enums: vec![],
        }
    }

    fn nodes_overlap(left: &PositionedNode, right: &PositionedNode) -> bool {
        left.x < right.x + right.width
            && left.x + left.width > right.x
            && left.y < right.y + right.height
            && left.y + left.height > right.y
    }

    #[test]
    fn test_build_layout() {
        let schema = make_test_schema();
        let result = build_layout(&schema);

        assert!(result.is_ok());
        let graph = result.unwrap();
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_layout_deterministic() {
        let schema = make_test_schema();
        let config = LayoutConfig::default();
        let request = LayoutRequest::default();

        let result1 = build_layout_with_config(&schema, &request, &config).unwrap();
        let result2 = build_layout_with_config(&schema, &request, &config).unwrap();

        // Results should be identical
        assert_eq!(result1.nodes.len(), result2.nodes.len());
        for (n1, n2) in result1.nodes.iter().zip(result2.nodes.iter()) {
            assert_eq!(n1.x, n2.x);
            assert_eq!(n1.y, n2.y);
        }
    }

    #[test]
    fn test_edge_label_position() {
        let schema = make_test_schema();
        let result = build_layout(&schema).unwrap();

        // Check that edges have label positions
        for edge in &result.edges {
            // Label position should be set
            assert!(edge.label_x.is_finite());
            assert!(edge.label_y.is_finite());
        }
    }

    #[test]
    fn test_force_layout_valid_positions() {
        let schema = make_test_schema();
        let config = LayoutConfig {
            mode: LayoutAlgorithm::ForceDirected,
            ..Default::default()
        };

        let result = build_layout_with_config(&schema, &LayoutRequest::default(), &config);

        assert!(result.is_ok());
        let graph = result.unwrap();

        // Check that all nodes have valid positions
        assert_eq!(graph.nodes.len(), 2);
        for node in &graph.nodes {
            // Positions should be finite and positive
            assert!(node.x.is_finite());
            assert!(node.y.is_finite());
            assert!(node.x >= config.origin_x);
            assert!(node.y >= config.origin_y);
            // Width and height should be positive
            assert!(node.width > 0.0);
            assert!(node.height > 0.0);
        }

        // Graph dimensions should be positive
        assert!(graph.width > 0.0);
        assert!(graph.height > 0.0);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_force_layout_deterministic() {
        let schema = make_test_schema();
        let config = LayoutConfig {
            mode: LayoutAlgorithm::ForceDirected,
            ..Default::default()
        };

        let result1 =
            build_layout_with_config(&schema, &LayoutRequest::default(), &config).unwrap();
        let result2 =
            build_layout_with_config(&schema, &LayoutRequest::default(), &config).unwrap();

        // Force layout should also be deterministic
        assert_eq!(result1.nodes.len(), result2.nodes.len());
        for (n1, n2) in result1.nodes.iter().zip(result2.nodes.iter()) {
            assert_eq!(n1.x, n2.x);
            assert_eq!(n1.y, n2.y);
        }
    }

    #[test]
    fn test_force_layout_different_from_hierarchical() {
        let schema = make_test_schema();

        let hierarchical_config = LayoutConfig {
            mode: LayoutAlgorithm::Hierarchical,
            ..Default::default()
        };

        let force_config = LayoutConfig {
            mode: LayoutAlgorithm::ForceDirected,
            ..Default::default()
        };

        let hierarchical_result =
            build_layout_with_config(&schema, &LayoutRequest::default(), &hierarchical_config)
                .unwrap();
        let force_result =
            build_layout_with_config(&schema, &LayoutRequest::default(), &force_config).unwrap();

        // Collect positions sorted by node id for comparison
        let mut hierarchical_positions: Vec<(&String, f32, f32)> = hierarchical_result
            .nodes
            .iter()
            .map(|n| (&n.id, n.x, n.y))
            .collect();
        hierarchical_positions.sort_by(|a, b| a.0.cmp(b.0));

        let mut force_positions: Vec<(&String, f32, f32)> = force_result
            .nodes
            .iter()
            .map(|n| (&n.id, n.x, n.y))
            .collect();
        force_positions.sort_by(|a, b| a.0.cmp(b.0));

        // The layouts should produce different positions for at least some nodes
        let positions_differ = hierarchical_positions
            .iter()
            .zip(force_positions.iter())
            .any(|((_, x1, y1), (_, x2, y2))| (x1 - x2).abs() > 1.0 || (y1 - y2).abs() > 1.0);

        assert!(
            positions_differ,
            "Force layout should produce different positions than hierarchical layout"
        );
    }

    #[test]
    fn test_hierarchical_layout_avoids_overlap_with_variable_width_nodes() {
        let schema = make_variable_width_schema();
        let graph = build_layout(&schema).unwrap();

        let mut nodes = graph.nodes;
        nodes.sort_by(|left, right| left.x.total_cmp(&right.x));

        for pair in nodes.windows(2) {
            let current = &pair[0];
            let next = &pair[1];
            assert!(
                current.x + current.width <= next.x,
                "nodes {} and {} overlap on the same rank",
                current.id,
                next.id
            );
        }
    }

    #[test]
    fn test_layout_expands_node_width_for_long_content() {
        let schema = make_variable_width_schema();
        let graph = build_layout(&schema).unwrap();

        let tiny = graph.nodes.iter().find(|node| node.id == "tiny").unwrap();
        let verbose = graph
            .nodes
            .iter()
            .find(|node| node.id == "extraordinarily_verbose_audit_log_entries")
            .unwrap();

        assert!(verbose.width > tiny.width);
        assert!(verbose.width > LayoutConfig::default().node_width);
    }

    #[test]
    fn test_hierarchical_layout_avoids_overlap_between_tall_ranks() {
        let schema = make_tall_rank_schema();
        let graph = build_layout(&schema).unwrap();

        for (index, node) in graph.nodes.iter().enumerate() {
            for other in graph.nodes.iter().skip(index + 1) {
                assert!(
                    !nodes_overlap(node, other),
                    "nodes {} and {} overlap",
                    node.id,
                    other.id
                );
            }
        }
    }

    #[test]
    fn test_build_positioned_node_preserves_column_flags() {
        let node = crate::graph::LayoutNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            schema_name: None,
            table_name: "posts".to_string(),
            kind: NodeKind::Table,
            columns: vec![
                crate::graph::LayoutColumn {
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                    is_primary_key: true,
                    is_foreign_key: false,
                    is_indexed: false,
                },
                crate::graph::LayoutColumn {
                    name: "user_id".to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                    is_primary_key: false,
                    is_foreign_key: true,
                    is_indexed: true,
                },
            ],
            inbound_count: 0,
            outbound_count: 1,
            has_self_loop: false,
            is_join_table_candidate: false,
            group_index: None,
        };

        let positioned = build_positioned_node(&node, 10.0, 20.0, 200.0, 100.0);

        assert!(positioned.columns[0].is_primary_key);
        assert!(!positioned.columns[0].is_foreign_key);
        assert!(!positioned.columns[0].is_indexed);
        assert!(positioned.columns[1].is_foreign_key);
        assert!(positioned.columns[1].is_indexed);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_compaction_respects_layout_spec_overrides() {
        let spec = LayoutSpec {
            horizontal_spacing: 320.0,
            vertical_spacing: 180.0,
            compaction: LayoutCompactionSpec {
                threshold: 10,
                min_horizontal_spacing: 220.0,
                min_vertical_spacing: 120.0,
                min_node_width: 180.0,
                min_node_padding: 6.0,
                hide_columns_threshold_multiplier: 3,
            },
            ..Default::default()
        };

        let config = LayoutConfig::from(&spec);
        let compacted = config.compute_compacted_config(20);
        assert_eq!(compacted.horizontal_spacing, 220.0);
        assert_eq!(compacted.vertical_spacing, 120.0);
        assert_eq!(compacted.node_width, 180.0);
        assert_eq!(compacted.node_padding, 6.0);
        assert!(!compacted.hide_columns);

        let hidden_columns = config.compute_compacted_config(31);
        assert!(hidden_columns.hide_columns);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_auto_tuned_identity_for_empty_graph() {
        let config = LayoutConfig::default();
        let tuned = config.clone().auto_tuned(0, 0);
        assert_eq!(tuned.horizontal_spacing, config.horizontal_spacing);
        assert_eq!(tuned.vertical_spacing, config.vertical_spacing);
    }

    #[test]
    fn test_auto_tuned_shrinks_spacing_for_medium_graph() {
        let config = LayoutConfig::default();
        let tuned = config.clone().auto_tuned(20, 20);
        assert!(
            tuned.horizontal_spacing < config.horizontal_spacing,
            "medium graph should have tighter spacing"
        );
    }

    #[test]
    fn test_auto_tuned_widens_for_dense_graph() {
        let config = LayoutConfig::default();
        // 5 nodes, 15 edges => density = 3.0 (very dense)
        let sparse = config.clone().auto_tuned(5, 3);
        let dense = config.auto_tuned(5, 15);
        assert!(
            dense.horizontal_spacing > sparse.horizontal_spacing,
            "dense graph should have wider spacing than sparse one of same node count"
        );
    }

    #[test]
    fn test_auto_tuned_respects_minimum_spacing() {
        let config = LayoutConfig::default();
        let tuned = config.clone().auto_tuned(100, 50);
        assert!(tuned.horizontal_spacing >= config.compaction.min_horizontal_spacing);
        assert!(tuned.vertical_spacing >= config.compaction.min_vertical_spacing);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_auto_tune_disabled_preserves_custom_spacing() {
        let config = LayoutConfig {
            horizontal_spacing: 500.0,
            vertical_spacing: 200.0,
            auto_tune_spacing: false,
            ..Default::default()
        };

        // auto_tuned() always mutates; the guard is in build_layout_from_graph_with_config.
        // Verify the build-path logic inline.
        let effective = if config.auto_tune_spacing {
            config.clone().auto_tuned(50, 80)
        } else {
            config.clone()
        };
        assert_eq!(effective.horizontal_spacing, 500.0);
        assert_eq!(effective.vertical_spacing, 200.0);

        // Contrast: when enabled, spacing IS changed.
        let tuned = config.auto_tuned(50, 80);
        assert_ne!(tuned.horizontal_spacing, 500.0);
    }

    #[test]
    fn test_build_layout_from_graph_does_not_mutate_input_when_columns_are_hidden() {
        let schema = make_test_schema();
        let graph = LayoutGraphBuilder::new().build(&schema);
        let original_column_count = graph.nodes[0].columns.len();
        let original_first_column_name = graph.nodes[0].columns[0].name.clone();
        let config = LayoutConfig {
            show_columns: false,
            ..Default::default()
        };

        let positioned = build_layout_from_graph_with_config(&graph, &config).unwrap();

        assert_eq!(graph.nodes[0].columns.len(), original_column_count);
        assert_eq!(graph.nodes[0].columns[0].name, original_first_column_name);
        assert!(positioned.nodes[0].columns.is_empty());
    }

    #[test]
    fn test_column_y_offset_from_center_basic() {
        let config = LayoutConfig::default();
        let layout_node = crate::graph::LayoutNode {
            id: "t".to_string(),
            label: "t".to_string(),
            schema_name: None,
            table_name: "t".to_string(),
            kind: NodeKind::Table,
            columns: vec![
                crate::graph::LayoutColumn {
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    is_primary_key: true,
                    is_foreign_key: false,
                    is_indexed: false,
                    nullable: false,
                },
                crate::graph::LayoutColumn {
                    name: "user_id".to_string(),
                    data_type: "int".to_string(),
                    is_primary_key: false,
                    is_foreign_key: true,
                    is_indexed: true,
                    nullable: false,
                },
            ],
            inbound_count: 0,
            outbound_count: 0,
            has_self_loop: false,
            is_join_table_candidate: false,
            group_index: None,
        };
        let height = estimate_node_height(&layout_node, &config);
        let node = PositionedNode {
            id: "t".to_string(),
            label: "t".to_string(),
            kind: NodeKind::Table,
            columns: vec![
                PositionedColumn {
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    is_primary_key: true,
                    is_foreign_key: false,
                    is_indexed: false,
                    nullable: false,
                },
                PositionedColumn {
                    name: "user_id".to_string(),
                    data_type: "int".to_string(),
                    is_primary_key: false,
                    is_foreign_key: true,
                    is_indexed: true,
                    nullable: false,
                },
            ],
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        };

        // user_id is column index 1.
        let offset = column_y_offset_from_center(&node, &["user_id".to_string()], &config);
        let expected_col_y = 1.0f32.mul_add(
            config.column_height,
            config.node_padding + config.header_height,
        ) + config.column_height / 2.0;
        let expected = expected_col_y - node.height / 2.0;
        assert!(
            (offset - expected).abs() < 0.01,
            "got {offset}, expected {expected}"
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_column_y_offset_fallback_for_empty_or_missing_columns() {
        let config = LayoutConfig::default();
        let empty_node = PositionedNode {
            id: "t".to_string(),
            label: "t".to_string(),
            kind: NodeKind::Table,
            columns: vec![],
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 50.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        };

        // No columns in node → 0 (center).
        assert_eq!(
            column_y_offset_from_center(&empty_node, &["user_id".to_string()], &config),
            0.0
        );
        // Empty edge columns → 0 (center).
        assert_eq!(column_y_offset_from_center(&empty_node, &[], &config), 0.0);

        let node_with_col = PositionedNode {
            columns: vec![PositionedColumn {
                name: "id".to_string(),
                data_type: "int".to_string(),
                is_primary_key: true,
                is_foreign_key: false,
                is_indexed: false,
                nullable: false,
            }],
            height: 60.0,
            ..empty_node
        };
        // Column not found → 0 (center).
        assert_eq!(
            column_y_offset_from_center(&node_with_col, &["nonexistent".to_string()], &config),
            0.0
        );
    }

    #[test]
    fn test_hierarchical_layout_handles_fully_connected_cycles() {
        let schema = make_fully_connected_cycle_schema();
        let layout_graph = LayoutGraphBuilder::new().build(&schema);
        let ranks = assign_ranks(&layout_graph, RankAssignmentStrategy::LongestPath);
        let ordered_nodes = order_nodes_within_layers(&layout_graph, &ranks);
        let graph = build_layout(&schema).unwrap();

        let ordered_node_count: usize = ordered_nodes.iter().map(Vec::len).sum();
        assert_eq!(ordered_node_count, layout_graph.nodes.len());
        assert_eq!(ranks.node_rank.len(), layout_graph.nodes.len());

        assert_eq!(graph.nodes.len(), 4);
        assert_eq!(graph.edges.len(), 12);

        let node_ids: std::collections::BTreeSet<_> =
            graph.nodes.iter().map(|node| node.id.as_str()).collect();
        assert_eq!(node_ids.len(), 4);
        for node in &graph.nodes {
            assert!(node.x.is_finite());
            assert!(node.y.is_finite());
            assert!(node.width > 0.0);
            assert!(node.height > 0.0);
        }
    }

    fn make_empty_schema() -> Schema {
        Schema {
            tables: vec![],
            views: vec![],
            enums: vec![],
        }
    }

    fn make_single_table_schema() -> Schema {
        Schema {
            tables: vec![Table {
                id: TableId(1),
                stable_id: "users".to_string(),
                schema_name: None,
                name: "users".to_string(),
                columns: vec![Column {
                    id: ColumnId(1),
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                    is_primary_key: true,
                    comment: None,
                }],
                foreign_keys: vec![],
                indexes: vec![],
                comment: None,
            }],
            views: vec![],
            enums: vec![],
        }
    }

    #[test]
    fn test_empty_schema_hierarchical() {
        let schema = make_empty_schema();
        let result = build_layout(&schema).unwrap();
        assert!(result.nodes.is_empty());
        assert!(result.edges.is_empty());
        assert!(result.groups.is_empty());
    }

    #[test]
    fn test_empty_schema_force_directed() {
        let schema = make_empty_schema();
        let config = LayoutConfig {
            mode: LayoutAlgorithm::ForceDirected,
            ..Default::default()
        };
        let result = build_layout_with_config(&schema, &LayoutRequest::default(), &config).unwrap();
        assert!(result.nodes.is_empty());
        assert!(result.edges.is_empty());
    }

    #[test]
    fn test_single_node_hierarchical() {
        let schema = make_single_table_schema();
        let result = build_layout(&schema).unwrap();
        assert_eq!(result.nodes.len(), 1);
        assert!(result.edges.is_empty());
        let node = &result.nodes[0];
        assert!(node.x.is_finite());
        assert!(node.y.is_finite());
        assert!(node.width > 0.0);
        assert!(node.height > 0.0);
    }

    #[test]
    fn test_single_node_force_directed() {
        let schema = make_single_table_schema();
        let config = LayoutConfig {
            mode: LayoutAlgorithm::ForceDirected,
            ..Default::default()
        };
        let result = build_layout_with_config(&schema, &LayoutRequest::default(), &config).unwrap();
        assert_eq!(result.nodes.len(), 1);
        assert!(result.edges.is_empty());
        let node = &result.nodes[0];
        assert!(node.x.is_finite());
        assert!(node.y.is_finite());
        assert!(node.width > 0.0);
        assert!(node.height > 0.0);
    }

    #[test]
    fn test_estimate_text_width_counts_cjk_as_wider_than_ascii() {
        let ascii = estimate_text_width("users", COLUMN_FONT_SIZE);
        let cjk = estimate_text_width("利用者", COLUMN_FONT_SIZE);

        assert!(cjk > ascii);
    }

    #[test]
    fn test_parallel_label_parameter_mirrors_reverse_edges() {
        let route_forward = EdgeRoute {
            x1: 0.0,
            y1: 0.0,
            x2: 90.0,
            y2: 0.0,
            control_points: Vec::new(),
            style: RouteStyle::Straight,
            label_position: (45.0, 0.0),
        };
        let route_reverse = EdgeRoute {
            x1: 90.0,
            y1: 0.0,
            x2: 0.0,
            y2: 0.0,
            control_points: Vec::new(),
            style: RouteStyle::Straight,
            label_position: (45.0, 0.0),
        };

        let forward = point_along_route(&route_forward, parallel_label_parameter("a", "b", 0, 2));
        let reverse = point_along_route(&route_reverse, parallel_label_parameter("b", "a", 1, 2));

        assert!((forward.0 - 30.0).abs() < 0.001);
        assert!((reverse.0 - 60.0).abs() < 0.001);
        assert!((forward.0 - reverse.0).abs() > 0.001);
    }

    #[test]
    fn test_parallel_edge_labels_avoid_endpoint_nodes() {
        let graph = LayoutGraph {
            nodes: Vec::new(),
            edges: vec![
                LayoutEdge {
                    from: "authors".to_string(),
                    to: "posts".to_string(),
                    name: Some("fk_posts_primary_author_identifier".to_string()),
                    from_columns: vec!["primary_author_id".to_string()],
                    to_columns: vec!["id".to_string()],
                    kind: EdgeKind::ForeignKey,
                    is_self_loop: false,
                    nullable: false,
                    target_cardinality: relune_core::layout::Cardinality::One,
                    is_collapsed_join: false,
                    collapsed_join_table: None,
                },
                LayoutEdge {
                    from: "authors".to_string(),
                    to: "posts".to_string(),
                    name: Some("fk_posts_review_author_identifier".to_string()),
                    from_columns: vec!["review_author_id".to_string()],
                    to_columns: vec!["id".to_string()],
                    kind: EdgeKind::ForeignKey,
                    is_self_loop: false,
                    nullable: false,
                    target_cardinality: relune_core::layout::Cardinality::One,
                    is_collapsed_join: false,
                    collapsed_join_table: None,
                },
            ],
            groups: Vec::new(),
            node_index: std::collections::BTreeMap::new(),
            reverse_index: std::collections::BTreeMap::new(),
        };
        let positioned_nodes = vec![
            PositionedNode {
                id: "authors".to_string(),
                label: "authors".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
            PositionedNode {
                id: "posts".to_string(),
                label: "posts".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 150.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
        ];

        let edges = route_edges(&graph, &positioned_nodes, &LayoutConfig::default(), None);
        assert_eq!(edges.len(), 2);

        for edge in &edges {
            let hw = estimate_label_half_width(&edge.label);
            for node in &positioned_nodes {
                let overlaps = edge.label_x + hw > node.x
                    && edge.label_x - hw < node.x + node.width
                    && edge.label_y + LABEL_HALF_H > node.y
                    && edge.label_y - LABEL_HALF_H < node.y + node.height;
                assert!(
                    !overlaps,
                    "Label {} overlaps node {} at ({}, {})",
                    edge.label, node.id, edge.label_x, edge.label_y
                );
            }
        }
    }

    #[test]
    fn test_route_edges_offsets_parallel_foreign_keys() {
        let schema = Schema {
            tables: vec![
                Table {
                    id: TableId(1),
                    stable_id: "users".to_string(),
                    schema_name: None,
                    name: "users".to_string(),
                    columns: vec![Column {
                        id: ColumnId(1),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(2),
                    stable_id: "posts".to_string(),
                    schema_name: None,
                    name: "posts".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(2),
                            name: "author_id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(3),
                            name: "reviewer_id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![
                        ForeignKey {
                            name: Some("fk_posts_author".to_string()),
                            from_columns: vec!["author_id".to_string()],
                            to_schema: None,
                            to_table: "users".to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        },
                        ForeignKey {
                            name: Some("fk_posts_reviewer".to_string()),
                            from_columns: vec!["reviewer_id".to_string()],
                            to_schema: None,
                            to_table: "users".to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        },
                    ],
                    indexes: vec![],
                    comment: None,
                },
            ],
            views: vec![],
            enums: vec![],
        };

        let graph = build_layout(&schema).unwrap();
        assert_eq!(graph.edges.len(), 2);
        assert!(
            (graph.edges[0].route.x1 - graph.edges[1].route.x1).abs() > f32::EPSILON
                || (graph.edges[0].route.y1 - graph.edges[1].route.y1).abs() > f32::EPSILON
        );
    }

    #[test]
    fn test_parallel_edge_labels_do_not_overlap() {
        // Two FK edges between the same pair of tables — their labels must not overlap.
        let schema = Schema {
            tables: vec![
                Table {
                    id: TableId(1),
                    stable_id: "users".to_string(),
                    schema_name: None,
                    name: "users".to_string(),
                    columns: vec![Column {
                        id: ColumnId(1),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    }],
                    foreign_keys: vec![],
                    indexes: vec![],
                    comment: None,
                },
                Table {
                    id: TableId(2),
                    stable_id: "posts".to_string(),
                    schema_name: None,
                    name: "posts".to_string(),
                    columns: vec![
                        Column {
                            id: ColumnId(2),
                            name: "author_id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                        Column {
                            id: ColumnId(3),
                            name: "editor_id".to_string(),
                            data_type: "int".to_string(),
                            nullable: false,
                            is_primary_key: false,
                            comment: None,
                        },
                    ],
                    foreign_keys: vec![
                        ForeignKey {
                            name: Some("fk_author".to_string()),
                            from_columns: vec!["author_id".to_string()],
                            to_schema: None,
                            to_table: "users".to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        },
                        ForeignKey {
                            name: Some("fk_editor".to_string()),
                            from_columns: vec!["editor_id".to_string()],
                            to_schema: None,
                            to_table: "users".to_string(),
                            to_columns: vec!["id".to_string()],
                            on_delete: ReferentialAction::NoAction,
                            on_update: ReferentialAction::NoAction,
                        },
                    ],
                    indexes: vec![],
                    comment: None,
                },
            ],
            views: vec![],
            enums: vec![],
        };

        let graph = build_layout(&schema).unwrap();
        assert_eq!(graph.edges.len(), 2);

        // Check that label bounding boxes do not overlap.
        let (lx0, ly0) = (graph.edges[0].label_x, graph.edges[0].label_y);
        let (lx1, ly1) = (graph.edges[1].label_x, graph.edges[1].label_y);
        let hw0 = estimate_label_half_width(&graph.edges[0].label);
        let hw1 = estimate_label_half_width(&graph.edges[1].label);
        let hh = LABEL_HALF_H;
        let overlaps = (lx0 + hw0 > lx1 - hw1)
            && (lx0 - hw0 < lx1 + hw1)
            && (ly0 + hh > ly1 - hh)
            && (ly0 - hh < ly1 + hh);
        assert!(
            !overlaps,
            "Parallel edge labels overlap: ({lx0},{ly0}) vs ({lx1},{ly1})"
        );
    }

    #[test]
    fn test_self_loop_label_outside_source_node() {
        // A self-referencing FK: the label must not sit inside the source node.
        let schema = Schema {
            tables: vec![Table {
                id: TableId(1),
                stable_id: "employees".to_string(),
                schema_name: None,
                name: "employees".to_string(),
                columns: vec![
                    Column {
                        id: ColumnId(1),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                    },
                    Column {
                        id: ColumnId(2),
                        name: "manager_id".to_string(),
                        data_type: "int".to_string(),
                        nullable: true,
                        is_primary_key: false,
                        comment: None,
                    },
                ],
                foreign_keys: vec![ForeignKey {
                    name: Some("fk_manager".to_string()),
                    from_columns: vec!["manager_id".to_string()],
                    to_schema: None,
                    to_table: "employees".to_string(),
                    to_columns: vec!["id".to_string()],
                    on_delete: ReferentialAction::NoAction,
                    on_update: ReferentialAction::NoAction,
                }],
                indexes: vec![],
                comment: None,
            }],
            views: vec![],
            enums: vec![],
        };

        let graph = build_layout(&schema).unwrap();
        assert_eq!(graph.edges.len(), 1);

        let edge = &graph.edges[0];
        assert!(edge.is_self_loop);

        // The node that owns the self-loop.
        let node = &graph.nodes[0];
        // Label center must be outside the node bounding box (allowing slight
        // overlap from the label's extent is OK, but the center should not be
        // inside the node).
        let center_inside = edge.label_x >= node.x
            && edge.label_x <= node.x + node.width
            && edge.label_y >= node.y
            && edge.label_y <= node.y + node.height;
        assert!(
            !center_inside,
            "Self-loop label center ({},{}) is inside node ({},{},{},{})",
            edge.label_x, edge.label_y, node.x, node.y, node.width, node.height
        );
    }

    #[test]
    fn test_route_edges_use_inter_rank_channel_for_hierarchical_flow() {
        let graph = single_edge_graph("authors", "posts");
        let positioned_nodes = vec![
            PositionedNode {
                id: "authors".to_string(),
                label: "authors".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
            PositionedNode {
                id: "posts".to_string(),
                label: "posts".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 200.0,
                y: 200.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
        ];
        let config = LayoutConfig::default();

        let edges = route_edges(&graph, &positioned_nodes, &config, Some(&[0, 1]));
        let edge = edges.first().expect("edge");

        assert_eq!(edge.route.control_points.len(), 2);
        assert!((edge.route.control_points[0].1 - 140.0).abs() < 0.001);
        assert!((edge.route.control_points[1].1 - 140.0).abs() < 0.001);
        assert!((edge.route.label_position.1 - 140.0).abs() < 0.001);
    }

    #[test]
    fn test_route_edges_use_separate_same_rank_channel_rule() {
        let graph = single_edge_graph("authors", "posts");
        let positioned_nodes = vec![
            PositionedNode {
                id: "authors".to_string(),
                label: "authors".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
            PositionedNode {
                id: "posts".to_string(),
                label: "posts".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 220.0,
                y: 120.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
        ];
        let config = LayoutConfig::default();

        let edges = route_edges(&graph, &positioned_nodes, &config, Some(&[0, 0]));
        let edge = edges.first().expect("edge");
        assert!(
            edge.route
                .control_points
                .iter()
                .any(|point| (point.0 - 160.0).abs() < 0.001)
        );
    }

    #[test]
    fn test_route_edges_shift_inter_rank_channel_away_from_obstacle() {
        let graph = single_edge_graph("authors", "posts");
        let mut positioned_nodes = vec![
            PositionedNode {
                id: "authors".to_string(),
                label: "authors".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
            PositionedNode {
                id: "posts".to_string(),
                label: "posts".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 200.0,
                y: 200.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
        ];
        positioned_nodes.push(PositionedNode {
            id: "blocker".to_string(),
            label: "blocker".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 120.0,
            y: 110.0,
            width: 60.0,
            height: 60.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        });

        let (edges, diagnostics) = route_edges_with_diagnostics(
            &graph,
            &positioned_nodes,
            &LayoutConfig::default(),
            Some(&[0, 1]),
        );
        let edge = edges.first().expect("edge");
        let channel_y = edge.route.control_points[0].1;

        assert_eq!(diagnostics.non_self_loop_detour_activations, 0);
        assert_eq!(edge.route.control_points.len(), 2);
        assert!(!(96.0..=184.0).contains(&channel_y));
        assert_eq!(
            route_obstacle_hit_count(
                &edge.route,
                &label_rects_from_nodes(&positioned_nodes[2..]),
                0.0
            ),
            0
        );
    }

    #[test]
    fn test_route_edges_spread_parallel_edges_across_channels() {
        let mut node_index = std::collections::BTreeMap::new();
        node_index.insert("authors".to_string(), 0usize);
        node_index.insert("posts".to_string(), 1usize);
        let graph = LayoutGraph {
            nodes: Vec::new(),
            edges: vec![
                LayoutEdge {
                    from: "authors".to_string(),
                    to: "posts".to_string(),
                    name: Some("fk_posts_author".to_string()),
                    from_columns: vec!["author_id".to_string()],
                    to_columns: vec!["id".to_string()],
                    kind: EdgeKind::ForeignKey,
                    is_self_loop: false,
                    nullable: false,
                    target_cardinality: relune_core::layout::Cardinality::One,
                    is_collapsed_join: false,
                    collapsed_join_table: None,
                },
                LayoutEdge {
                    from: "authors".to_string(),
                    to: "posts".to_string(),
                    name: Some("fk_posts_reviewer".to_string()),
                    from_columns: vec!["review_author_id".to_string()],
                    to_columns: vec!["id".to_string()],
                    kind: EdgeKind::ForeignKey,
                    is_self_loop: false,
                    nullable: false,
                    target_cardinality: relune_core::layout::Cardinality::One,
                    is_collapsed_join: false,
                    collapsed_join_table: None,
                },
            ],
            groups: Vec::new(),
            node_index,
            reverse_index: std::collections::BTreeMap::new(),
        };
        let positioned_nodes = vec![
            PositionedNode {
                id: "authors".to_string(),
                label: "authors".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
            PositionedNode {
                id: "posts".to_string(),
                label: "posts".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 200.0,
                y: 200.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
        ];

        let edges = route_edges(
            &graph,
            &positioned_nodes,
            &LayoutConfig::default(),
            Some(&[0, 1]),
        );
        assert_eq!(edges.len(), 2);

        let shared_trunk_y = |edge: &PositionedEdge| {
            route_points(&edge.route)
                .windows(2)
                .filter(|segment| (segment[0].1 - segment[1].1).abs() < 0.001)
                .max_by(|left, right| {
                    let left_len = (left[1].0 - left[0].0).abs();
                    let right_len = (right[1].0 - right[0].0).abs();
                    left_len.total_cmp(&right_len)
                })
                .map(|segment| segment[0].1)
                .expect("bundled route should keep a horizontal trunk")
        };

        let first_trunk = shared_trunk_y(&edges[0]);
        let second_trunk = shared_trunk_y(&edges[1]);

        assert!((first_trunk - second_trunk).abs() < 0.001);
        assert_ne!(route_points(&edges[0].route), route_points(&edges[1].route));
    }

    #[test]
    fn test_route_edges_shift_reverse_channel_away_from_obstacle() {
        let graph = single_edge_graph("posts", "authors");
        let mut positioned_nodes = vec![
            PositionedNode {
                id: "posts".to_string(),
                label: "posts".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 0.0,
                y: 220.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
            PositionedNode {
                id: "authors".to_string(),
                label: "authors".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 200.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
        ];
        positioned_nodes.push(PositionedNode {
            id: "blocker".to_string(),
            label: "blocker".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 120.0,
            y: 110.0,
            width: 60.0,
            height: 60.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        });

        let (edges, diagnostics) = route_edges_with_diagnostics(
            &graph,
            &positioned_nodes,
            &LayoutConfig::default(),
            Some(&[1, 0]),
        );
        let edge = edges.first().expect("edge");

        assert_eq!(diagnostics.non_self_loop_detour_activations, 0);
        assert!((edge.route.control_points[0].1 - 198.0).abs() < 0.001);
    }

    #[test]
    fn test_obstacle_aware_channel_rejects_candidates_that_violate_hard_constraints() {
        let graph = single_edge_graph("authors", "posts");
        let positioned_nodes = vec![
            PositionedNode {
                id: "authors".to_string(),
                label: "authors".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
            PositionedNode {
                id: "posts".to_string(),
                label: "posts".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 200.0,
                y: 200.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
        ];
        let node_ranks = [0usize, 1usize];
        let config = LayoutConfig {
            direction: LayoutDirection::TopToBottom,
            ..Default::default()
        };
        let rank_bounds = rank_axis_bounds(&positioned_nodes, &node_ranks, &config);
        let assignment = RegularPortAssignment {
            source_side: AttachmentSide::South,
            target_side: AttachmentSide::North,
            source_slot_offset: 0.0,
            source_slot_index: 0,
            source_slot_count: 1,
            target_slot_offset: 0.0,
            target_slot_index: 0,
            target_slot_count: 1,
            source_row_offset: 0.0,
            target_row_offset: 0.0,
        };
        let obstacles = vec![Rect {
            x: -300.0,
            y: 84.0,
            w: 900.0,
            h: 180.0,
        }];

        let candidate = obstacle_aware_channel_for_edge(
            &graph,
            &graph.edges[0],
            &node_ranks,
            Some(&rank_bounds),
            config.direction,
            (0.0, 0.0, 100.0, 80.0),
            (200.0, 200.0, 100.0, 80.0),
            &assignment,
            &obstacles,
            &BTreeMap::new(),
            RouteStyle::Orthogonal,
        );

        assert!(candidate.is_none());
    }

    #[test]
    fn test_route_edges_measure_detour_activation_without_ranked_channels() {
        let graph = single_edge_graph("authors", "posts");
        let mut positioned_nodes = vec![
            PositionedNode {
                id: "authors".to_string(),
                label: "authors".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
            PositionedNode {
                id: "posts".to_string(),
                label: "posts".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 200.0,
                y: 200.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
        ];
        positioned_nodes.push(PositionedNode {
            id: "blocker".to_string(),
            label: "blocker".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 120.0,
            y: 110.0,
            width: 60.0,
            height: 60.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        });

        let (_, diagnostics) =
            route_edges_with_diagnostics(&graph, &positioned_nodes, &LayoutConfig::default(), None);
        assert_eq!(diagnostics.non_self_loop_detour_activations, 1);
    }

    #[test]
    fn test_route_edges_bypass_intermediate_obstacle_for_skipped_vertical_rank() {
        let graph = single_edge_graph("comments", "users");
        let positioned_nodes = vec![
            PositionedNode {
                id: "comments".to_string(),
                label: "comments".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
            PositionedNode {
                id: "users".to_string(),
                label: "users".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 0.0,
                y: 420.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
            PositionedNode {
                id: "posts".to_string(),
                label: "posts".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 0.0,
                y: 180.0,
                width: 100.0,
                height: 120.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
        ];

        let (edges, diagnostics) = route_edges_with_diagnostics(
            &graph,
            &positioned_nodes,
            &LayoutConfig::default(),
            Some(&[0, 2]),
        );
        let edge = edges.first().expect("edge");

        assert_eq!(diagnostics.non_self_loop_detour_activations, 0);
        assert_eq!(
            route_obstacle_hit_count(
                &edge.route,
                &label_rects_from_nodes(&positioned_nodes[2..]),
                0.0
            ),
            0
        );
        assert!(edge.route.control_points.len() >= 4);
    }

    #[test]
    fn test_route_edges_bypass_intermediate_obstacle_for_skipped_horizontal_rank() {
        let graph = single_edge_graph("comments", "users");
        let config = LayoutConfig {
            direction: LayoutDirection::LeftToRight,
            ..Default::default()
        };
        let positioned_nodes = vec![
            PositionedNode {
                id: "comments".to_string(),
                label: "comments".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
            PositionedNode {
                id: "users".to_string(),
                label: "users".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 420.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
            PositionedNode {
                id: "posts".to_string(),
                label: "posts".to_string(),
                kind: NodeKind::Table,
                columns: Vec::new(),
                x: 180.0,
                y: 0.0,
                width: 120.0,
                height: 120.0,
                is_join_table_candidate: false,
                has_self_loop: false,
                group_index: None,
            },
        ];

        let (edges, diagnostics) =
            route_edges_with_diagnostics(&graph, &positioned_nodes, &config, Some(&[0, 2]));
        let edge = edges.first().expect("edge");

        assert_eq!(diagnostics.non_self_loop_detour_activations, 0);
        assert_eq!(
            route_obstacle_hit_count(
                &edge.route,
                &label_rects_from_nodes(&positioned_nodes[2..]),
                0.0
            ),
            0
        );
        assert!(edge.route.control_points.len() >= 4);
    }

    fn label_rects_from_nodes(nodes: &[PositionedNode]) -> Vec<Rect> {
        nodes
            .iter()
            .map(|node| Rect {
                x: node.x,
                y: node.y,
                w: node.width,
                h: node.height,
            })
            .collect()
    }
}
