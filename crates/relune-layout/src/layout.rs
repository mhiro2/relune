//! Main layout engine
//!
//! This module provides the main layout algorithm that combines
//! ranking, ordering, and coordinate assignment to produce
//! a positioned graph suitable for rendering.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{Level, debug, info, span};

use relune_core::{EdgeKind, LayoutAlgorithm, LayoutDirection, LayoutSpec, NodeKind, Schema};

use crate::focus::FocusExtractor;
use crate::graph::{CollapsedJoinTable, LayoutGraph, LayoutGraphBuilder, LayoutRequest};
use crate::order::order_nodes_within_layers;
use crate::rank::{RankAssignmentStrategy, assign_ranks};
use crate::route::{route_edge, route_self_loop};
use relune_core::layout::{EdgeRoute, RouteStyle};

/// Default threshold for enabling compact mode automatically.
pub const DEFAULT_LARGE_SCHEMA_THRESHOLD: usize = 50;

/// Layout mode alias shared with `relune-core`.
pub type LayoutMode = LayoutAlgorithm;

/// Default number of iterations for force-directed layout.
const fn default_force_iterations() -> usize {
    150
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
    /// Edge routing style.
    pub edge_style: RouteStyle,
    /// Whether to show column details in nodes.
    /// When false, only table names are displayed.
    pub show_columns: bool,
    /// Threshold for automatic compact mode.
    /// When the number of nodes exceeds this value, compact settings are applied.
    /// Set to 0 to disable automatic compaction.
    pub large_schema_threshold: usize,
    /// Layout mode (hierarchical or force-directed).
    #[serde(default)]
    pub mode: LayoutMode,
    /// Number of iterations for force-directed layout.
    #[serde(default = "default_force_iterations")]
    pub force_iterations: usize,
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
            large_schema_threshold: DEFAULT_LARGE_SCHEMA_THRESHOLD,
            mode: LayoutAlgorithm::default(),
            force_iterations: default_force_iterations(),
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
    /// When the node count exceeds `large_schema_threshold`, this method returns
    /// reduced spacing and sizing values to create a more compact layout.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn compute_compacted_config(&self, node_count: usize) -> CompactedConfig {
        if self.large_schema_threshold > 0 && node_count > self.large_schema_threshold {
            // Calculate compaction factor based on how much we exceed the threshold
            let excess_ratio = (node_count as f32 / self.large_schema_threshold as f32).min(2.0);
            let compaction_factor = 1.0 / excess_ratio;

            // Apply compaction with minimum bounds to maintain readability
            CompactedConfig {
                horizontal_spacing: (self.horizontal_spacing * compaction_factor).max(160.0),
                vertical_spacing: (self.vertical_spacing * compaction_factor).max(80.0),
                node_width: (self.node_width * compaction_factor).max(140.0),
                node_padding: (self.node_padding * compaction_factor).max(4.0),
                hide_columns: !self.show_columns || node_count > self.large_schema_threshold * 2,
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
        self.large_schema_threshold > 0 && node_count > self.large_schema_threshold
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
pub struct PositionedColumn {
    /// Column name.
    pub name: String,
    /// Column data type.
    pub data_type: String,
    /// Whether the column can be null.
    pub nullable: bool,
    /// Whether this column is part of the primary key.
    pub is_primary_key: bool,
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

    // Step 1: Build the graph from schema
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

    // Step 2: Apply focus if specified
    if let Some(ref focus) = request.focus {
        let extractor = FocusExtractor;
        graph = extractor.extract(&graph, focus)?;
        debug!("Applied focus, resulting in {} nodes", graph.nodes.len());
    }

    // Step 2b: Compute compacted config based on graph size and apply if needed
    let compacted = config.compute_compacted_config(graph.nodes.len());
    let effective_config = if config.should_compact(graph.nodes.len()) {
        info!(
            "Large schema detected ({} nodes > {} threshold), applying compact mode",
            graph.nodes.len(),
            config.large_schema_threshold
        );
        LayoutConfig {
            horizontal_spacing: compacted.horizontal_spacing,
            vertical_spacing: compacted.vertical_spacing,
            node_width: compacted.node_width,
            node_padding: compacted.node_padding,
            show_columns: !compacted.hide_columns,
            ..config.clone()
        }
    } else {
        config.clone()
    };

    // Step 2c: If compact mode hides columns, strip them from graph nodes
    if compacted.hide_columns {
        for node in &mut graph.nodes {
            node.columns.clear();
        }
    }

    // Step 3: Assign coordinates based on layout mode
    let (positioned_nodes, width, height) = match effective_config.mode {
        LayoutAlgorithm::Hierarchical => {
            // Hierarchical layout: assign ranks and order
            let ranks = assign_ranks(&graph, RankAssignmentStrategy::LongestPath);
            debug!("Assigned {} ranks", ranks.num_ranks);
            let ordered_nodes = order_nodes_within_layers(&graph, &ranks);
            assign_coordinates(&graph, &ordered_nodes, &effective_config)
        }
        LayoutAlgorithm::ForceDirected => {
            // Force-directed layout
            apply_force_layout(&graph, &effective_config)
        }
    };

    // Step 4: Route edges
    let positioned_edges = route_edges(&graph, &positioned_nodes, &effective_config);

    // Step 5: Position groups
    let positioned_groups = position_groups(&graph.groups, &positioned_nodes);

    info!("Layout complete: {}x{} pixels", width, height);

    Ok(PositionedGraph {
        nodes: positioned_nodes,
        edges: positioned_edges,
        groups: positioned_groups,
        width,
        height,
    })
}

/// Assign coordinates to nodes based on their ranks and order.
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::suboptimal_flops)]
fn assign_coordinates(
    graph: &LayoutGraph,
    ordered_nodes: &[Vec<usize>],
    config: &LayoutConfig,
) -> (Vec<PositionedNode>, f32, f32) {
    let mut positioned_nodes = Vec::new();

    let mut max_x = config.origin_x;
    let mut max_y = config.origin_y;

    let is_horizontal = matches!(
        config.direction,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft
    );

    for (rank_idx, rank_nodes) in ordered_nodes.iter().enumerate() {
        let mut secondary_offset = if is_horizontal {
            config.origin_y
        } else {
            config.origin_x
        };

        for &node_idx in rank_nodes {
            let node = &graph.nodes[node_idx];
            let node_height = config.header_height
                + node.columns.len() as f32 * config.column_height
                + config.node_padding * 2.0;

            let (node_x, node_y) = if is_horizontal {
                // Horizontal: ranks flow along X, nodes stack along Y
                let primary = config.origin_x + rank_idx as f32 * config.horizontal_spacing;
                let secondary = secondary_offset;
                secondary_offset += node_height + config.vertical_spacing;
                (primary, secondary)
            } else {
                // Vertical: ranks flow along Y, nodes stack along X
                let primary = config.origin_y + rank_idx as f32 * config.vertical_spacing;
                let secondary = secondary_offset;
                secondary_offset += config.node_width + config.horizontal_spacing;
                (secondary, primary)
            };

            let positioned = PositionedNode {
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
                    })
                    .collect(),
                x: node_x,
                y: node_y,
                width: config.node_width,
                height: node_height,
                is_join_table_candidate: node.is_join_table_candidate,
                has_self_loop: node.has_self_loop,
                group_index: node.group_index,
            };

            max_x = max_x.max(positioned.x + positioned.width);
            max_y = max_y.max(positioned.y + positioned.height);

            positioned_nodes.push(positioned);
        }
    }

    let width = max_x + config.origin_x;
    let height = max_y + config.origin_y;

    // Flip coordinates for reversed directions
    match config.direction {
        LayoutDirection::BottomToTop => {
            for node in &mut positioned_nodes {
                node.y = height - node.y - node.height;
            }
        }
        LayoutDirection::RightToLeft => {
            for node in &mut positioned_nodes {
                node.x = width - node.x - node.width;
            }
        }
        LayoutDirection::TopToBottom | LayoutDirection::LeftToRight => {}
    }

    (positioned_nodes, width, height)
}

/// Apply force-directed layout algorithm.
///
/// This is a simple "force-lite" implementation that uses:
/// - Repulsive forces between all pairs of nodes
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
    let ideal_spacing = config.horizontal_spacing.max(config.vertical_spacing);
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

        // Repulsive forces between all pairs of nodes
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = positions[i].0 - positions[j].0;
                let dy = positions[i].1 - positions[j].1;
                let dist_sq = dx * dx + dy * dy + min_distance;
                let dist = dist_sq.sqrt();

                // Repulsive force: F = k / d^2
                let force = repulsion_strength / dist_sq;
                let fx = force * dx / dist;
                let fy = force * dy / dist;

                forces[i].0 += fx;
                forces[i].1 += fy;
                forces[j].0 -= fx;
                forces[j].1 -= fy;
            }
        }

        // Attractive forces along edges
        for &(from_idx, to_idx) in &edges {
            let dx = positions[to_idx].0 - positions[from_idx].0;
            let dy = positions[to_idx].1 - positions[from_idx].1;
            let dist = (dx * dx + dy * dy).sqrt().max(min_distance);

            // Attractive force: F = k * d
            let force = attraction_strength * dist;
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
        .zip(positions.iter())
        .map(|(node, &(x, y))| {
            let height = config.header_height
                + node.columns.len() as f32 * config.column_height
                + config.node_padding * 2.0;

            max_x = max_x.max(x + config.node_width);
            max_y = max_y.max(y + height);

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
                    })
                    .collect(),
                x,
                y,
                width: config.node_width,
                height,
                is_join_table_candidate: node.is_join_table_candidate,
                has_self_loop: node.has_self_loop,
                group_index: node.group_index,
            }
        })
        .collect();

    let width = max_x + config.origin_x;
    let height = max_y + config.origin_y;

    (positioned_nodes, width, height)
}

/// Route all edges in the graph.
fn route_edges(
    graph: &LayoutGraph,
    positioned_nodes: &[PositionedNode],
    config: &LayoutConfig,
) -> Vec<PositionedEdge> {
    let node_positions: std::collections::BTreeMap<&str, (f32, f32, f32, f32)> = positioned_nodes
        .iter()
        .map(|n| (n.id.as_str(), (n.x, n.y, n.width, n.height)))
        .collect();

    let mut edges = Vec::new();

    for edge in &graph.edges {
        let from_pos = node_positions.get(edge.from.as_str());
        let to_pos = node_positions.get(edge.to.as_str());

        let route = if edge.is_self_loop {
            if let Some(&(x, y, w, h)) = from_pos {
                route_self_loop(x, y, w, h, config.edge_style)
            } else {
                continue;
            }
        } else if let (Some(&(x1, y1, w1, h1)), Some(&(x2, y2, w2, h2))) = (from_pos, to_pos) {
            route_edge(x1, y1, w1, h1, x2, y2, w2, h2, config.edge_style)
        } else {
            continue;
        };

        let label = edge.name.clone().unwrap_or_else(|| {
            if edge.from_columns.is_empty() {
                "fk".to_string()
            } else {
                edge.from_columns.join(",")
            }
        });

        let (label_x, label_y) = route.label_position;

        edges.push(PositionedEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
            label,
            kind: edge.kind,
            route,
            is_self_loop: edge.is_self_loop,
            nullable: edge.nullable,
            from_columns: edge.from_columns.clone(),
            to_columns: edge.to_columns.clone(),
            is_collapsed_join: edge.is_collapsed_join,
            collapsed_join_table: edge.collapsed_join_table.clone(),
            label_x,
            label_y,
        });
    }

    edges
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
            let group_nodes: Vec<&PositionedNode> = group
                .node_indices
                .iter()
                .filter_map(|&idx| positioned_nodes.get(idx))
                .collect();

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
    use super::*;
    use relune_core::{Column, ColumnId, ForeignKey, ReferentialAction, Table, TableId};

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
}
