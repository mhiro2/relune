//! Relune layout engine
//!
//! This crate provides graph construction and layout algorithms for ERD visualization.
//! It takes a normalized `Schema` from `relune-core` and produces a positioned graph
//! suitable for rendering.

pub mod channel;
pub mod diagram_export;
pub mod focus;
pub mod graph;
pub mod layout;
pub mod order;
pub mod overlay;
pub mod port;
pub mod rank;
pub mod route;

pub use channel::{
    ChannelCandidateClass, ChannelCandidateScore, ChannelCostWeights,
    compare_channel_candidate_scores,
};
pub use diagram_export::{layout_graph_to_d2, layout_graph_to_dot, layout_graph_to_mermaid};
pub use focus::{FocusError, FocusExtractor};
pub use graph::{
    CollapsedJoinTable, LayoutEdge, LayoutGraph, LayoutGraphBuilder, LayoutGroup, LayoutNode,
    LayoutRequest,
};
pub use layout::{
    ColumnFlags, ColumnRelationFlags, LayoutConfig, LayoutError, LayoutMode, PositionedColumn,
    PositionedEdge, PositionedEdgeRoutingDebug, PositionedGraph, PositionedGraphRoutingDebug,
    PositionedGroup, PositionedNode, build_layout, build_layout_from_graph_with_config,
    build_layout_with_config,
};
pub use order::{
    CrossingReductionStrategy, order_nodes_within_layers, order_nodes_within_layers_with_strategy,
};
pub use overlay::{
    Annotation, DiagramOverlay, EdgeOverlay, NodeOverlay, OverlaySeverity, edge_key,
};
pub use rank::{RankAssignmentStrategy, assign_ranks};
pub use relune_core::layout::{EdgeRoute, RouteStyle};
pub use route::{Rect, detour_around_obstacles, nudge_label, route_edge};
