//! Edge routing, parallel-edge bundling, channel selection, and label placement.

use std::collections::BTreeMap;

use tracing::debug;

use relune_core::layout::{Cardinality, EdgeRoute, RouteStyle};
use relune_core::{EdgeKind, LayoutDirection};

use crate::channel::{
    CachedChannelCandidateScore, ChannelCandidateClass, ChannelCandidateScore, ChannelCostWeights,
    compare_cached_channel_candidate_scores,
};
use crate::graph::LayoutGraph;
use crate::port::{
    EdgePortAssignment, MIN_SLOT_GAP, PORT_CORNER_INSET, RegularPortAssignment, assign_edge_ports,
};
use crate::route::{
    AttachmentSide, BORDER_OUTSET, ChannelAxis, LABEL_HALF_H, Rect, approximate_route_length,
    detour_around_obstacles_with_endpoint_sizes, estimate_label_half_width, nudge_label,
    point_along_route, rebuild_route_from_points, route_edge_with_assigned_ports, route_points,
    route_self_loop_at_rows, sample_route_obstacles, step_from_attachment,
};

use super::routing_debug::{build_regular_edge_debug, channel_axis_name};
use super::spatial::{BBox, SpatialGrid};
use super::{
    LayoutConfig, LayoutError, PositionedEdge, PositionedEdgeRoutingDebug, PositionedNode,
};

/// Clearance target used while scoring obstacle-aware channel candidates.
const ROUTE_CLEARANCE_TARGET: f32 = 14.0;
/// Obstacle inflation for the hard intersection check. The segment test
/// ignores a rectangle's outer 2px; this cancels most of that so a channel
/// running just inside a node's edge still counts as crossing it.
pub(super) const HARD_OBSTACLE_PADDING: f32 = 1.5;
/// Maximum gap between nearby channel candidates that may share one visual bundle.
const BUNDLE_CHANNEL_TOLERANCE: f32 = 36.0;
/// Distance used to preserve endpoint approach direction while entering a
/// bypass channel; past the endpoint marker, plus the port's own offset.
const ROUTE_STUB_DISTANCE: f32 = MIN_ENDPOINT_STUB + 4.0;
/// Margin added when probing side corridors outside the endpoint nodes.
const BYPASS_CHANNEL_MARGIN: f32 = 24.0;
/// Distance between adjacent outer bypass lanes.
pub(super) const BYPASS_CHANNEL_LANE_STEP: f32 = 48.0;
/// Additional bypass lanes explored beyond the first outer lane on each side.
const BYPASS_CHANNEL_EXTRA_LANES: usize = 3;

/// Cell size of the routing spatial indexes. Large enough that a typical edge
/// or node footprint touches only a handful of cells.
const ROUTING_GRID_CELL_SIZE: f32 = 256.0;
/// Margin kept between a label and the obstacles around it during initial placement.
const LABEL_PLACEMENT_MARGIN: f32 = 4.0;

/// Half-size of sampled edge-path obstacles used during label collision avoidance.
const EDGE_ROUTE_OBSTACLE_HALF_SIZE: f32 = 7.0;
/// Target spacing between sampled edge-path obstacles.
const EDGE_ROUTE_OBSTACLE_SPACING: f32 = 10.0;
/// Number of label-relaxation passes after all edge routes are known.
const EDGE_LABEL_RELAXATION_PASSES: usize = 3;
/// Margin used when testing label/obstacle overlap during relaxation.
const EDGE_LABEL_COLLISION_MARGIN: f32 = 4.0;
/// Labels should stay away from edge endpoints and markers.
pub(super) const MIN_LABEL_ROUTE_T: f32 = 0.16;
/// Candidate stride when sliding labels along their own route.
const LABEL_ROUTE_T_STEP: f32 = 0.03;
/// Maximum perpendicular fallback when a label cannot fit anywhere on its own route.
const LABEL_ROUTE_FALLBACK_MAX_OFFSET: f32 = 96.0;
/// Extra clearance reserved from a FK endpoint for Crow's Foot markers.
const FK_MARKER_CLEARANCE: f32 = 30.0;
/// Extra clearance reserved from a generic arrow endpoint.
const ARROW_MARKER_CLEARANCE: f32 = 14.0;
/// Half-thickness of the axis-aligned obstacle reserved for endpoint markers.
const ENDPOINT_MARKER_HALF_THICKNESS: f32 = 14.0;
/// Shortest straight run a route keeps at each card before its first bend,
/// so a Crow's Foot marker never folds over the elbow.
const MIN_ENDPOINT_STUB: f32 = FK_MARKER_CLEARANCE;

/// Route all edges in the graph.
#[cfg_attr(not(test), allow(dead_code))] // Test helpers exercise the wrapper directly.
pub(super) fn route_edges(
    graph: &LayoutGraph,
    positioned_nodes: &[PositionedNode],
    config: &LayoutConfig,
    node_ranks: Option<&[usize]>,
) -> Result<Vec<PositionedEdge>, LayoutError> {
    Ok(route_edges_with_diagnostics(graph, positioned_nodes, config, node_ranks)?.0)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct RoutingDiagnostics {
    pub(super) non_self_loop_detour_activations: usize,
    pub(super) channel_fallback_activations: usize,
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
    edge_index: usize,
    label: String,
    kind: EdgeKind,
    route: EdgeRoute,
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

struct SingleEdgeRoutingContext<'a> {
    graph: &'a LayoutGraph,
    config: &'a LayoutConfig,
    node_ranks: Option<&'a [usize]>,
    rank_bounds: Option<&'a [RankAxisBounds]>,
    obstacles: EdgeObstacles<'a>,
}

struct SingleEdgeResult {
    route: EdgeRoute,
    bundle_metadata: Option<BundleRouteMetadata>,
    routing_debug: PositionedEdgeRoutingDebug,
    used_detour_fallback: bool,
    used_channel_fallback: bool,
}

#[allow(clippy::too_many_lines)]
fn route_single_edge(
    ctx: &SingleEdgeRoutingContext<'_>,
    edge: &crate::graph::LayoutEdge,
    port_assignment: &EdgePortAssignment,
    from_pos: Option<&(f32, f32, f32, f32)>,
    to_pos: Option<&(f32, f32, f32, f32)>,
    channel_usage: &mut BTreeMap<(ChannelAxis, i32), u32>,
) -> Result<SingleEdgeResult, LayoutError> {
    let mut bundle_metadata = None;
    let mut used_channel_fallback = false;
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

    let route = if let EdgePortAssignment::SelfLoop(assignment) = port_assignment {
        routing_debug.self_loop_radius_offset = Some(assignment.radius_offset);
        let Some(&(x, y, w, h)) = from_pos else {
            return Err(LayoutError::RoutingInvariant {
                from: edge.from.clone(),
                to: edge.to.clone(),
                detail: "source node position missing for self-loop",
            });
        };
        route_self_loop_at_rows(
            x,
            y,
            w,
            h,
            ctx.config.edge_style,
            ctx.config.direction,
            assignment.radius_offset,
            assignment.rows,
            Some(assignment.side),
        )
    } else if let (
        EdgePortAssignment::Regular(assignment),
        Some(&(x1, y1, w1, h1)),
        Some(&(x2, y2, w2, h2)),
    ) = (port_assignment, from_pos, to_pos)
    {
        routing_debug = build_regular_edge_debug(*assignment);

        if let Some(ranks) = ctx.node_ranks {
            let Some(source_rank) =
                node_rank_for_edge_endpoint(ctx.graph, ranks, edge.from.as_str())
            else {
                return Err(LayoutError::RoutingInvariant {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    detail: "source node rank missing",
                });
            };
            let Some(target_rank) = node_rank_for_edge_endpoint(ctx.graph, ranks, edge.to.as_str())
            else {
                return Err(LayoutError::RoutingInvariant {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    detail: "target node rank missing",
                });
            };
            let source_rect = Rect {
                x: x1,
                y: y1,
                w: w1,
                h: h1,
            };
            let target_rect = Rect {
                x: x2,
                y: y2,
                w: w2,
                h: h2,
            };
            if let Some(candidate) = obstacle_aware_channel_for_edge(
                ObstacleRoutingContext {
                    graph: ctx.graph,
                    edge,
                    node_ranks: ranks,
                    rank_bounds: ctx.rank_bounds,
                    direction: ctx.config.direction,
                    assignment,
                    obstacles: ctx.obstacles,
                    channel_usage,
                    style: ctx.config.edge_style,
                },
                source_rect,
                target_rect,
            ) {
                record_channel_usage(channel_usage, candidate.axis, candidate.coordinate);
                bundle_metadata = Some(BundleRouteMetadata {
                    axis: candidate.axis,
                    coordinate: candidate.coordinate,
                    source_side: assignment.source_side,
                    target_side: assignment.target_side,
                });
                routing_debug.channel_axis = Some(channel_axis_name(candidate.axis).to_string());
                routing_debug.channel_coordinate = Some(candidate.coordinate);
                route_edge_with_candidate_channel(
                    source_rect,
                    target_rect,
                    ctx.config.edge_style,
                    assignment,
                    candidate,
                    RankedChannelContext {
                        direction: ctx.config.direction,
                        source_rank,
                        target_rank,
                        rank_bounds: ctx.rank_bounds,
                    },
                )
            } else {
                used_channel_fallback = true;
                debug!(
                    edge_from = edge.from,
                    edge_to = edge.to,
                    "No obstacle-aware channel candidate satisfied constraints; using simple backbone"
                );
                route_edge_with_assigned_ports(
                    x1,
                    y1,
                    w1,
                    h1,
                    x2,
                    y2,
                    w2,
                    h2,
                    ctx.config.edge_style,
                    assignment.source_side,
                    assignment.target_side,
                    assignment.source_slot_offset,
                    assignment.target_slot_offset,
                    assignment.source_row_offset,
                    assignment.target_row_offset,
                )
            }
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
                ctx.config.edge_style,
                assignment.source_side,
                assignment.target_side,
                assignment.source_slot_offset,
                assignment.target_slot_offset,
                assignment.source_row_offset,
                assignment.target_row_offset,
            )
        }
    } else {
        return Err(LayoutError::RoutingInvariant {
            from: edge.from.clone(),
            to: edge.to.clone(),
            detail: "edge endpoint position missing",
        });
    };

    let mut used_detour_fallback = false;
    let route = match edge.is_self_loop {
        true => {
            bundle_metadata = None;
            let self_size = match (from_pos, to_pos) {
                (Some(&(_, _, w, h)), _) | (_, Some(&(_, _, w, h))) => Some((w, h)),
                _ => None,
            };
            let loop_side = match port_assignment {
                EdgePortAssignment::SelfLoop(assignment) => assignment.side,
                EdgePortAssignment::Regular(_) => AttachmentSide::East,
            };
            // Detours may wander arbitrarily far from the loop, so self-loops
            // (rare in practice) check against every node.
            detour_around_obstacles_with_endpoint_sizes(
                &route,
                &ctx.obstacles.all(),
                Some(loop_side),
                Some(loop_side),
                self_size,
                self_size,
            )
        }
        false if route_needs_detour(&route, ctx.obstacles) => {
            bundle_metadata = None;
            used_detour_fallback = true;
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

    Ok(SingleEdgeResult {
        route,
        bundle_metadata,
        routing_debug,
        used_detour_fallback,
        used_channel_fallback,
    })
}

fn finalize_routed_edge(
    draft: &RoutedEdgeDraft,
    source_edge: &crate::graph::LayoutEdge,
    node_positions: &BTreeMap<&str, (f32, f32, f32, f32)>,
    node_obstacles: &NodeObstacleIndex<'_>,
    lane_index: usize,
    lane_total: usize,
    placed_labels: &mut PlacedLabels,
) -> PositionedEdge {
    let from_pos = node_positions.get(source_edge.from.as_str());
    let to_pos = node_positions.get(source_edge.to.as_str());

    // Labels stay within `label_reach` of their route, so only obstacles in
    // that region can influence placement; the order below (other nodes,
    // endpoint markers, earlier labels, endpoint nodes) is kept stable.
    let lhw = estimate_label_half_width(&draft.label);
    let reach = BBox::from_points(&route_points(&draft.route))
        .expanded(label_reach(lhw) + LABEL_PLACEMENT_MARGIN);
    let mut label_obstacles = EdgeObstacles::new(
        node_obstacles,
        source_edge.from.as_str(),
        source_edge.to.as_str(),
    )
    .near_bbox(&reach);
    label_obstacles.extend(edge_endpoint_marker_obstacles(
        &draft.route,
        source_edge.kind,
        source_edge.nullable,
        source_edge.target_cardinality,
    ));
    label_obstacles.extend(placed_labels.near(&reach));
    if let Some(&(x, y, w, h)) = from_pos {
        label_obstacles.push(Rect { x, y, w, h });
    }
    if !source_edge.is_self_loop
        && let Some(&(x, y, w, h)) = to_pos
    {
        label_obstacles.push(Rect { x, y, w, h });
    }

    let label_pos = if lane_total > 1 && !source_edge.is_self_loop {
        let t =
            parallel_label_parameter(&source_edge.from, &source_edge.to, lane_index, lane_total);
        point_along_route(&draft.route, t)
    } else {
        draft.route.label_position
    };

    let preferred_t = if lane_total > 1 && !source_edge.is_self_loop {
        parallel_label_parameter(&source_edge.from, &source_edge.to, lane_index, lane_total)
    } else {
        estimate_route_parameter(&draft.route, label_pos)
    };
    let (label_x, label_y) = place_label_on_route(
        &draft.route,
        preferred_t,
        &label_obstacles,
        LABEL_PLACEMENT_MARGIN,
        lhw,
    );
    placed_labels.push(label_rect(label_x, label_y, lhw));

    PositionedEdge {
        from: source_edge.from.clone(),
        to: source_edge.to.clone(),
        label: draft.label.clone(),
        kind: draft.kind,
        route: draft.route.clone(),
        is_self_loop: source_edge.is_self_loop,
        nullable: source_edge.nullable,
        target_cardinality: source_edge.target_cardinality,
        from_columns: source_edge.from_columns.clone(),
        to_columns: source_edge.to_columns.clone(),
        is_collapsed_join: source_edge.is_collapsed_join,
        collapsed_join_table: source_edge.collapsed_join_table.clone(),
        label_x,
        label_y,
        routing_debug: Some(draft.routing_debug.clone()),
    }
}

/// Node rectangles indexed by a uniform grid, so obstacle checks along a route
/// only visit nodes near its segments instead of every node in the graph.
#[derive(Debug)]
pub(super) struct NodeObstacleIndex<'a> {
    ids: Vec<&'a str>,
    rects: Vec<Rect>,
    grid: SpatialGrid,
}

impl<'a> NodeObstacleIndex<'a> {
    /// Indexes every node plus anonymous `rects` that no edge may cross.
    pub(super) fn from_nodes_and_rects(nodes: &'a [PositionedNode], rects: &[Rect]) -> Self {
        Self::build(
            nodes
                .iter()
                .map(|node| {
                    (
                        node.id.as_str(),
                        Rect {
                            x: node.x,
                            y: node.y,
                            w: node.width,
                            h: node.height,
                        },
                    )
                })
                .chain(rects.iter().map(|rect| ("", *rect)))
                .collect(),
        )
    }

    /// Indexes anonymous obstacles that are never excluded as edge endpoints.
    #[cfg(test)]
    pub(super) fn from_rects(rects: &[Rect]) -> Self {
        Self::build(rects.iter().map(|rect| ("", *rect)).collect())
    }

    fn build(entries: Vec<(&'a str, Rect)>) -> Self {
        let mut grid = SpatialGrid::new(ROUTING_GRID_CELL_SIZE);
        for (index, (_, rect)) in entries.iter().enumerate() {
            grid.insert(index, &BBox::from_rect(rect));
        }
        let (ids, rects) = entries.into_iter().unzip();
        Self { ids, rects, grid }
    }
}

/// Obstacles seen by one edge: every indexed node except its endpoints.
#[derive(Debug, Clone, Copy)]
pub(super) struct EdgeObstacles<'a> {
    index: &'a NodeObstacleIndex<'a>,
    endpoints: [&'a str; 2],
}

impl<'a> EdgeObstacles<'a> {
    pub(super) const fn new(index: &'a NodeObstacleIndex<'a>, from: &'a str, to: &'a str) -> Self {
        Self {
            index,
            endpoints: [from, to],
        }
    }

    fn is_obstacle(&self, index: usize) -> bool {
        let id = self.index.ids[index];
        id.is_empty() || !self.endpoints.contains(&id)
    }

    fn collect(&self, indices: impl IntoIterator<Item = usize>) -> Vec<Rect> {
        indices
            .into_iter()
            .filter(|&index| self.is_obstacle(index))
            .map(|index| self.index.rects[index])
            .collect()
    }

    /// Obstacles that may lie within `reach` of any route segment, in node order.
    ///
    /// Querying per segment keeps long orthogonal routes from pulling in every
    /// node inside their overall bounding box.
    fn near_route(&self, route: &EdgeRoute, reach: f32) -> Vec<Rect> {
        let points = route_points(route);
        let segments: Vec<BBox> = if points.len() < 2 {
            vec![BBox::from_points(&points).expanded(reach)]
        } else {
            points
                .windows(2)
                .map(|segment| BBox::from_points(segment).expanded(reach))
                .collect()
        };
        self.collect(self.index.grid.query_many_sorted(&segments))
    }

    /// Obstacles that may intersect `bbox`, in node order.
    fn near_bbox(&self, bbox: &BBox) -> Vec<Rect> {
        self.collect(self.index.grid.query_sorted(bbox))
    }

    /// Every obstacle, in node order.
    fn all(&self) -> Vec<Rect> {
        self.collect(0..self.index.rects.len())
    }
}

/// Labels placed so far, indexed for neighbourhood queries.
struct PlacedLabels {
    rects: Vec<Rect>,
    grid: SpatialGrid,
}

impl PlacedLabels {
    fn new() -> Self {
        Self {
            rects: Vec::new(),
            grid: SpatialGrid::new(ROUTING_GRID_CELL_SIZE),
        }
    }

    fn push(&mut self, rect: Rect) {
        self.grid.insert(self.rects.len(), &BBox::from_rect(&rect));
        self.rects.push(rect);
    }

    /// Placed labels that may intersect `bbox`, in placement order.
    fn near(&self, bbox: &BBox) -> impl Iterator<Item = Rect> + '_ {
        self.grid
            .query_sorted(bbox)
            .into_iter()
            .map(|index| self.rects[index])
    }
}

/// Distance from its route beyond which a label can never reach: candidate
/// centres lie on the route, nudging moves them up to the fallback offset along
/// the normal, and the label rect spans its half extents around the centre.
fn label_reach(label_half_w: f32) -> f32 {
    LABEL_ROUTE_FALLBACK_MAX_OFFSET.max(label_half_w) + label_half_w + LABEL_HALF_H
}

pub(super) fn route_edges_with_diagnostics(
    graph: &LayoutGraph,
    positioned_nodes: &[PositionedNode],
    config: &LayoutConfig,
    node_ranks: Option<&[usize]>,
) -> Result<(Vec<PositionedEdge>, RoutingDiagnostics), LayoutError> {
    route_edges_around(graph, positioned_nodes, config, node_ranks, &[])
}

/// Routes all edges, keeping lines and labels off `fixed_obstacles` (such as
/// group headings) as well as off every card.
pub(super) fn route_edges_around(
    graph: &LayoutGraph,
    positioned_nodes: &[PositionedNode],
    config: &LayoutConfig,
    node_ranks: Option<&[usize]>,
    fixed_obstacles: &[Rect],
) -> Result<(Vec<PositionedEdge>, RoutingDiagnostics), LayoutError> {
    let node_positions: BTreeMap<&str, (f32, f32, f32, f32)> = positioned_nodes
        .iter()
        .map(|node| (node.id.as_str(), (node.x, node.y, node.width, node.height)))
        .collect();
    let port_assignments = assign_edge_ports(graph, positioned_nodes, config, node_ranks);
    let rank_bounds = node_ranks.map(|ranks| rank_axis_bounds(positioned_nodes, ranks, config));
    let edge_counts = edge_lane_counts(graph);
    let lane_indices = edge_lane_indices(graph);
    let routing_order = stable_edge_routing_order(graph, node_ranks);
    let mut channel_usage = BTreeMap::new();
    let mut diagnostics = RoutingDiagnostics::default();

    let mut routed_edges = vec![None; graph.edges.len()];

    let node_obstacles = NodeObstacleIndex::from_nodes_and_rects(positioned_nodes, fixed_obstacles);

    for &edge_index in &routing_order {
        let edge = &graph.edges[edge_index];
        let from_pos = node_positions.get(edge.from.as_str());
        let to_pos = node_positions.get(edge.to.as_str());

        let Some(port_assignment) = port_assignments.get(edge_index).and_then(Option::as_ref)
        else {
            return Err(LayoutError::RoutingInvariant {
                from: edge.from.clone(),
                to: edge.to.clone(),
                detail: "port assignment missing",
            });
        };

        let ctx = SingleEdgeRoutingContext {
            graph,
            config,
            node_ranks,
            rank_bounds: rank_bounds.as_deref(),
            obstacles: EdgeObstacles::new(&node_obstacles, edge.from.as_str(), edge.to.as_str()),
        };
        let result = route_single_edge(
            &ctx,
            edge,
            port_assignment,
            from_pos,
            to_pos,
            &mut channel_usage,
        )?;

        if result.used_detour_fallback {
            diagnostics.non_self_loop_detour_activations += 1;
        }
        if result.used_channel_fallback {
            diagnostics.channel_fallback_activations += 1;
        }

        let label = edge.name.clone().unwrap_or_else(|| {
            if edge.from_columns.is_empty() {
                "fk".to_string()
            } else {
                edge.from_columns.join(",")
            }
        });

        routed_edges[edge_index] = Some(RoutedEdgeDraft {
            edge_index,
            label,
            kind: edge.kind,
            route: result.route,
            bundle_metadata: result.bundle_metadata,
            routing_debug: result.routing_debug,
        });
    }

    apply_parallel_edge_bundling(&mut routed_edges, positioned_nodes, graph, fixed_obstacles);
    straighten_small_jogs(&mut routed_edges, graph, positioned_nodes, &node_obstacles);

    let mut edges = vec![None; graph.edges.len()];
    let mut placed_labels = PlacedLabels::new();

    for &edge_index in &routing_order {
        let Some(draft) = routed_edges[edge_index].as_ref() else {
            continue;
        };
        let source_edge = &graph.edges[draft.edge_index];
        let lane_key = canonical_edge_pair(&source_edge.from, &source_edge.to);
        let lane_index = lane_indices[edge_index];
        let lane_total = edge_counts.get(&lane_key).copied().unwrap_or(1);

        edges[edge_index] = Some(finalize_routed_edge(
            draft,
            source_edge,
            &node_positions,
            &node_obstacles,
            lane_index,
            lane_total,
            &mut placed_labels,
        ));
    }

    let mut edges: Vec<_> = edges.into_iter().flatten().collect();
    resolve_edge_label_collisions(&mut edges, positioned_nodes, fixed_obstacles);
    Ok((edges, diagnostics))
}

fn apply_parallel_edge_bundling(
    routed_edges: &mut [Option<RoutedEdgeDraft>],
    positioned_nodes: &[PositionedNode],
    graph: &LayoutGraph,
    fixed_obstacles: &[Rect],
) {
    let mut groups: BTreeMap<BundleGroupKey, Vec<usize>> = BTreeMap::new();

    for (edge_index, edge) in routed_edges.iter().enumerate() {
        let Some(edge) = edge.as_ref() else {
            continue;
        };
        let Some(bundle_metadata) = edge.bundle_metadata else {
            continue;
        };
        let source_edge = &graph.edges[edge.edge_index];
        if source_edge.is_self_loop {
            continue;
        }

        groups
            .entry(BundleGroupKey {
                from: source_edge.from.clone(),
                to: source_edge.to.clone(),
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
                    if bundled_route_is_valid(
                        &candidate,
                        graph,
                        edge,
                        bundle_metadata,
                        positioned_nodes,
                        fixed_obstacles,
                    ) {
                        edge.route = candidate;
                    }
                }
            }

            cluster_start = cluster_end;
        }
    }
}

/// Sideways step below which a vertical route reads as one line drawn
/// slightly off, rather than as a deliberate bend; it covers the half slot
/// gap that two centered port groups leave between nearly aligned ports.
const SMALL_JOG: f32 = 16.0;

/// Straightens vertical routes whose ends are only a few pixels apart.
///
/// Port slots are spread per card side, so two nearly aligned ports can
/// leave a route with a tiny sideways step between its channel bends. When
/// one endpoint can slide along its top or bottom side to line up with the
/// other, without crowding another port or crossing a card, the route
/// becomes a single straight line. Side ports stay put: their height names
/// a column row.
fn straighten_small_jogs(
    routed_edges: &mut [Option<RoutedEdgeDraft>],
    graph: &LayoutGraph,
    positioned_nodes: &[PositionedNode],
    obstacles: &NodeObstacleIndex<'_>,
) {
    let node_by_id: BTreeMap<&str, &PositionedNode> = positioned_nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut ports: Vec<(usize, &str, (f32, f32))> = Vec::new();
    for (index, draft) in routed_edges.iter().enumerate() {
        let Some(draft) = draft else { continue };
        let edge = &graph.edges[draft.edge_index];
        ports.push((index, edge.from.as_str(), (draft.route.x1, draft.route.y1)));
        ports.push((index, edge.to.as_str(), (draft.route.x2, draft.route.y2)));
    }

    for (index, slot) in routed_edges.iter_mut().enumerate() {
        let Some(draft) = slot.as_mut() else {
            continue;
        };
        let edge = &graph.edges[draft.edge_index];
        if edge.is_self_loop || draft.route.style == RouteStyle::Straight {
            continue;
        }
        let points = route_points(&draft.route);
        let [source, bend_a, bend_b, target] = points[..] else {
            continue;
        };
        let vertical_z = (source.0 - bend_a.0).abs() < 0.01
            && (bend_b.0 - target.0).abs() < 0.01
            && (bend_a.1 - bend_b.1).abs() < 0.01;
        let jog = (source.0 - target.0).abs();
        if !vertical_z || !(0.01..SMALL_JOG).contains(&jog) {
            continue;
        }

        // Prefer moving the target end, which carries the lighter marker.
        let moves = [
            (edge.to.as_str(), target, source.0),
            (edge.from.as_str(), source, target.0),
        ];
        let straightened = moves.into_iter().find_map(|(node_id, port, new_x)| {
            let node = node_by_id.get(node_id)?;
            let within_side = new_x >= node.x + PORT_CORNER_INSET
                && new_x <= node.x + node.width - PORT_CORNER_INSET;
            let crowds_neighbour = ports.iter().any(|&(other, other_node, other_port)| {
                other != index
                    && other_node == node_id
                    && (other_port.1 - port.1).abs() < 0.5
                    && (other_port.0 - new_x).abs() < MIN_SLOT_GAP
            });
            if !within_side || crowds_neighbour {
                return None;
            }
            let points = if port == target {
                [source, (new_x, target.1)]
            } else {
                [(new_x, source.1), target]
            };
            let route = rebuild_route_from_points(&points, draft.route.style);
            let edge_obstacles = EdgeObstacles::new(obstacles, &edge.from, &edge.to);
            let nearby = edge_obstacles.near_route(&route, ROUTE_CLEARANCE_TARGET);
            (route_obstacle_hit_count(&route, &nearby, HARD_OBSTACLE_PADDING) == 0).then_some((
                route,
                port == target,
                new_x,
            ))
        });

        if let Some((route, moved_target, new_x)) = straightened {
            for port in &mut ports {
                if port.0 == index && (port.2 == if moved_target { target } else { source }) {
                    port.2.0 = new_x;
                }
            }
            draft.route = route;
            draft.bundle_metadata = None;
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
    // Bundled lines still need room for their endpoint markers.
    (base + (extra * 4.0).min(12.0)).max(MIN_ENDPOINT_STUB)
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
    graph: &LayoutGraph,
    edge: &RoutedEdgeDraft,
    metadata: BundleRouteMetadata,
    positioned_nodes: &[PositionedNode],
    fixed_obstacles: &[Rect],
) -> bool {
    let source_edge = &graph.edges[edge.edge_index];
    if endpoint_side_violations(route, metadata.source_side, metadata.target_side) > 0 {
        return false;
    }

    let obstacles = positioned_nodes
        .iter()
        .filter(|node| node.id != source_edge.from && node.id != source_edge.to)
        .map(|node| Rect {
            x: node.x,
            y: node.y,
            w: node.width,
            h: node.height,
        })
        .chain(fixed_obstacles.iter().copied())
        .collect::<Vec<_>>();

    route_obstacle_hit_count(route, &obstacles, HARD_OBSTACLE_PADDING) == 0
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
pub(super) struct RankAxisBounds {
    pub(super) min: f32,
    pub(super) max: f32,
}

pub(super) fn rank_axis_bounds(
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

fn same_rank_x_channel(source_rect: Rect, target_rect: Rect) -> f32 {
    let source_center = source_rect.x + source_rect.w / 2.0;
    let target_center = target_rect.x + target_rect.w / 2.0;
    if source_center <= target_center {
        f32::midpoint(source_rect.x + source_rect.w, target_rect.x)
    } else {
        f32::midpoint(source_rect.x, target_rect.x + target_rect.w)
    }
}

fn same_rank_y_channel(source_rect: Rect, target_rect: Rect) -> f32 {
    let source_center = source_rect.y + source_rect.h / 2.0;
    let target_center = target_rect.y + target_rect.h / 2.0;
    if source_center <= target_center {
        f32::midpoint(source_rect.y + source_rect.h, target_rect.y)
    } else {
        f32::midpoint(source_rect.y, target_rect.y + target_rect.h)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ChannelSearchPlan {
    axis: ChannelAxis,
    baseline: f32,
    pub(super) class: ChannelCandidateClass,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ObstacleAwareChannelCandidate {
    pub(super) axis: ChannelAxis,
    pub(super) coordinate: f32,
    pub(super) baseline: f32,
    pub(super) stable_order: u32,
}

#[derive(Debug, Clone, Copy)]
struct RankedChannelContext<'a> {
    direction: LayoutDirection,
    source_rank: usize,
    target_rank: usize,
    rank_bounds: Option<&'a [RankAxisBounds]>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ObstacleRoutingContext<'a> {
    pub(super) graph: &'a LayoutGraph,
    pub(super) edge: &'a crate::graph::LayoutEdge,
    pub(super) node_ranks: &'a [usize],
    pub(super) rank_bounds: Option<&'a [RankAxisBounds]>,
    pub(super) direction: LayoutDirection,
    pub(super) assignment: &'a RegularPortAssignment,
    pub(super) obstacles: EdgeObstacles<'a>,
    pub(super) channel_usage: &'a BTreeMap<(ChannelAxis, i32), u32>,
    pub(super) style: RouteStyle,
}

pub(super) fn obstacle_aware_channel_for_edge(
    context: ObstacleRoutingContext<'_>,
    source_rect: Rect,
    target_rect: Rect,
) -> Option<ObstacleAwareChannelCandidate> {
    let source_rank = node_rank_for_edge_endpoint(
        context.graph,
        context.node_ranks,
        context.edge.from.as_str(),
    )?;
    let target_rank =
        node_rank_for_edge_endpoint(context.graph, context.node_ranks, context.edge.to.as_str())?;
    let ranked_context = RankedChannelContext {
        direction: context.direction,
        source_rank,
        target_rank,
        rank_bounds: context.rank_bounds,
    };
    let rank_bounds = context.rank_bounds?;
    let search_plan = channel_search_plan(
        source_rank,
        target_rank,
        rank_bounds,
        context.direction,
        source_rect,
        target_rect,
    )?;
    let weights = ChannelCostWeights::default();
    let mut best_candidate = None;
    let mut best_score = None;
    let mut candidates = channel_candidates(search_plan, source_rank, target_rank, rank_bounds);
    if search_plan.class != ChannelCandidateClass::SameRank {
        let start_order = u32::try_from(candidates.len()).unwrap_or(u32::MAX);
        let mut span = BBox::from_rect(&source_rect);
        span.include_rect(&target_rect);
        let blockers = context.obstacles.near_bbox(&span);
        candidates.extend(bypass_channel_candidates(
            context.direction,
            source_rect,
            target_rect,
            &blockers,
            start_order,
        ));
    }

    for candidate in candidates {
        let route = route_edge_with_candidate_channel(
            source_rect,
            target_rect,
            context.style,
            context.assignment,
            candidate,
            ranked_context,
        );
        let score = score_channel_candidate(
            &route,
            context.obstacles,
            context.direction,
            source_rank,
            target_rank,
            context.assignment.source_side,
            context.assignment.target_side,
            candidate,
            context.channel_usage,
        );
        if score.hard_constraint_violations != 0 {
            continue;
        }
        let cached_score = CachedChannelCandidateScore::new(score, weights);
        let is_better = best_score
            .is_none_or(|best| compare_cached_channel_candidate_scores(cached_score, best).is_lt());
        if is_better {
            best_candidate = Some(candidate);
            best_score = Some(cached_score);
        }
    }

    best_candidate
}

pub(super) fn channel_search_plan(
    source_rank: usize,
    target_rank: usize,
    rank_bounds: &[RankAxisBounds],
    direction: LayoutDirection,
    source_rect: Rect,
    target_rect: Rect,
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
    // Foreign keys point from a child to the parent ranked above it, so the
    // regular flow runs from a higher rank to a lower one. Only edges pointing
    // down the ranks (cycle-closing edges) are reverse edges.
    let class = if same_rank {
        ChannelCandidateClass::SameRank
    } else if source_rank < target_rank {
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

pub(super) fn channel_candidates(
    plan: ChannelSearchPlan,
    source_rank: usize,
    target_rank: usize,
    rank_bounds: &[RankAxisBounds],
) -> Vec<ObstacleAwareChannelCandidate> {
    let mut candidates = Vec::with_capacity(plan.class.search_offsets().len());
    let stub_range = (plan.class == ChannelCandidateClass::InterRank)
        .then(|| inter_rank_stub_range(source_rank, target_rank, rank_bounds))
        .flatten();

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

    // Lanes inside the part of the gap that keeps endpoint markers clear of
    // the elbow, so edges sharing a gap can each keep their markers whole
    // without stacking on one channel.
    if let Some((lower, upper)) = stub_range {
        let center = f32::midpoint(lower, upper);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps = (((center - lower) / STUB_RANGE_LANE_STEP).floor() as usize)
            .min(STUB_RANGE_LANES_PER_SIDE);
        let mut lanes = Vec::with_capacity(steps * 2 + 4);
        #[allow(clippy::cast_precision_loss)]
        for step in (0..=steps).map(|index| index as f32 * STUB_RANGE_LANE_STEP) {
            lanes.extend([center - step, center + step]);
        }
        // The range edges keep the most room from cards in the middle.
        lanes.extend([lower, upper]);
        for coordinate in lanes {
            let duplicate = candidates
                .iter()
                .any(|candidate| (candidate.coordinate - coordinate).abs() < 1.0);
            if coordinate <= upper && !duplicate {
                #[allow(clippy::cast_possible_truncation)]
                candidates.push(ObstacleAwareChannelCandidate {
                    axis: plan.axis,
                    coordinate,
                    baseline: plan.baseline,
                    stable_order: candidates.len() as u32,
                });
            }
        }
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

/// Spacing of the extra channel lanes tried inside a gap's marker-safe range.
const STUB_RANGE_LANE_STEP: f32 = 10.0;
/// Extra lanes tried on each side of the range's center; long edges across
/// many ranks would otherwise add a lane for every 10px of their gap.
const STUB_RANGE_LANES_PER_SIDE: usize = 6;

/// Endpoints whose straight run before the first bend is too short for the
/// endpoint marker; zero for a route without bends.
fn short_endpoint_stubs(route: &EdgeRoute) -> u16 {
    let points = route_points(route);
    if points.len() < 3 {
        return 0;
    }
    let length = |a: (f32, f32), b: (f32, f32)| (b.0 - a.0).hypot(b.1 - a.1);
    let first = length(points[0], points[1]);
    let last = length(points[points.len() - 2], points[points.len() - 1]);
    u16::from(first + 0.5 < MIN_ENDPOINT_STUB) + u16::from(last + 0.5 < MIN_ENDPOINT_STUB)
}

/// Range of the gap between two ranks whose channels leave every card in
/// either rank at least [`MIN_ENDPOINT_STUB`] of straight line, or `None`
/// when the gap is too narrow to keep it on both sides.
fn inter_rank_stub_range(
    source_rank: usize,
    target_rank: usize,
    rank_bounds: &[RankAxisBounds],
) -> Option<(f32, f32)> {
    let source = rank_bounds.get(source_rank).copied()?;
    let target = rank_bounds.get(target_rank).copied()?;
    let (lower, upper) = if source.min <= target.min {
        (source.max, target.min)
    } else {
        (target.max, source.min)
    };
    // Ports sit just outside the card border, so the stub starts there.
    let reach = MIN_ENDPOINT_STUB + BORDER_OUTSET;
    let range = (lower + reach, upper - reach);
    (range.0 <= range.1).then_some(range)
}

/// Candidates in the outer corridors on both sides of an edge.
///
/// The first lane on each side starts outside the endpoints and any `blockers`
/// that overlap the rectangle spanned by them, so a lane never runs through
/// (or hugs) a node sitting between the endpoints.
pub(super) fn bypass_channel_candidates(
    direction: LayoutDirection,
    source_rect: Rect,
    target_rect: Rect,
    blockers: &[Rect],
    start_order: u32,
) -> Vec<ObstacleAwareChannelCandidate> {
    let mut candidates = Vec::with_capacity(bypass_channel_lane_count().saturating_mul(2));
    let mut span = BBox::from_rect(&source_rect);
    span.include_rect(&target_rect);
    let mut corridor = span;
    for blocker in blockers {
        let overlaps_span = blocker.x < span.max_x
            && blocker.x + blocker.w > span.min_x
            && blocker.y < span.max_y
            && blocker.y + blocker.h > span.min_y;
        if overlaps_span {
            corridor.include_rect(blocker);
        }
    }

    match direction {
        LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => {
            append_bypass_candidates(
                &mut candidates,
                ChannelAxis::X,
                corridor.max_x + BYPASS_CHANNEL_MARGIN,
                corridor.min_x - BYPASS_CHANNEL_MARGIN,
                start_order,
            );
        }
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => {
            append_bypass_candidates(
                &mut candidates,
                ChannelAxis::Y,
                corridor.max_y + BYPASS_CHANNEL_MARGIN,
                corridor.min_y - BYPASS_CHANNEL_MARGIN,
                start_order,
            );
        }
    }

    candidates
}

pub(super) const fn bypass_channel_lane_count() -> usize {
    BYPASS_CHANNEL_EXTRA_LANES + 1
}

fn bypass_channel_offsets() -> impl ExactSizeIterator<Item = f32> {
    (0..bypass_channel_lane_count()).map(|lane_index| {
        BYPASS_CHANNEL_LANE_STEP * f32::from(u16::try_from(lane_index).unwrap_or(u16::MAX))
    })
}

fn append_bypass_candidates(
    candidates: &mut Vec<ObstacleAwareChannelCandidate>,
    axis: ChannelAxis,
    positive_baseline: f32,
    negative_baseline: f32,
    start_order: u32,
) {
    for (offset_index, offset) in bypass_channel_offsets().enumerate() {
        let stable_order = start_order
            .saturating_add(u32::try_from(offset_index.saturating_mul(2)).unwrap_or(u32::MAX));
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

fn route_edge_with_candidate_channel(
    source_rect: Rect,
    target_rect: Rect,
    style: RouteStyle,
    assignment: &RegularPortAssignment,
    candidate: ObstacleAwareChannelCandidate,
    context: RankedChannelContext<'_>,
) -> EdgeRoute {
    let seed_route = route_edge_with_assigned_ports(
        source_rect.x,
        source_rect.y,
        source_rect.w,
        source_rect.h,
        target_rect.x,
        target_rect.y,
        target_rect.w,
        target_rect.h,
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
        context,
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

fn candidate_channel_anchors(
    source: (f32, f32),
    target: (f32, f32),
    source_side: AttachmentSide,
    target_side: AttachmentSide,
    candidate: ObstacleAwareChannelCandidate,
    context: RankedChannelContext<'_>,
) -> ((f32, f32), (f32, f32)) {
    match (context.direction, candidate.axis, context.rank_bounds) {
        (
            LayoutDirection::TopToBottom | LayoutDirection::BottomToTop,
            ChannelAxis::X,
            Some(bounds),
        ) if context.source_rank != context.target_rank => {
            let Some(source_bounds) = bounds.get(context.source_rank).copied() else {
                return (
                    step_from_attachment(source, source_side, ROUTE_STUB_DISTANCE),
                    step_from_attachment(target, target_side, ROUTE_STUB_DISTANCE),
                );
            };
            let Some(target_bounds) = bounds.get(context.target_rank).copied() else {
                return (
                    step_from_attachment(source, source_side, ROUTE_STUB_DISTANCE),
                    step_from_attachment(target, target_side, ROUTE_STUB_DISTANCE),
                );
            };
            // Step out of each rank on the side facing the other one; rank
            // order says nothing about which side that is in bottom-to-top.
            if source_bounds.min <= target_bounds.min {
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
        ) if context.source_rank != context.target_rank => {
            let Some(source_bounds) = bounds.get(context.source_rank).copied() else {
                return (
                    step_from_attachment(source, source_side, ROUTE_STUB_DISTANCE),
                    step_from_attachment(target, target_side, ROUTE_STUB_DISTANCE),
                );
            };
            let Some(target_bounds) = bounds.get(context.target_rank).copied() else {
                return (
                    step_from_attachment(source, source_side, ROUTE_STUB_DISTANCE),
                    step_from_attachment(target, target_side, ROUTE_STUB_DISTANCE),
                );
            };
            if source_bounds.min <= target_bounds.min {
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

#[allow(clippy::too_many_arguments)] // Channel scoring stays clearer with explicit ranking and routing inputs.
fn score_channel_candidate(
    route: &EdgeRoute,
    obstacles: EdgeObstacles<'_>,
    direction: LayoutDirection,
    source_rank: usize,
    target_rank: usize,
    source_side: AttachmentSide,
    target_side: AttachmentSide,
    candidate: ObstacleAwareChannelCandidate,
    channel_usage: &BTreeMap<(ChannelAxis, i32), u32>,
) -> ChannelCandidateScore {
    // Obstacles farther than the clearance target neither intersect the route
    // nor add a clearance penalty, so only nearby nodes are scored.
    let obstacles = obstacles.near_route(route, ROUTE_CLEARANCE_TARGET);
    let hard_constraint_violations =
        clipped_u16(route_obstacle_hit_count(
            route,
            &obstacles,
            HARD_OBSTACLE_PADDING,
        )) + route_primary_direction_violations(route, direction, source_rank, target_rank)
            + endpoint_side_violations(route, source_side, target_side);

    ChannelCandidateScore {
        hard_constraint_violations,
        short_endpoint_stubs: short_endpoint_stubs(route),
        clearance_penalty: route_clearance_penalty(route, &obstacles, ROUTE_CLEARANCE_TARGET),
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

fn route_primary_direction_violations(
    route: &EdgeRoute,
    direction: LayoutDirection,
    source_rank: usize,
    target_rank: usize,
) -> u16 {
    if source_rank == target_rank {
        return 0;
    }

    let points = route_points(route);
    let epsilon = 0.5;
    let should_increase = match direction {
        LayoutDirection::TopToBottom | LayoutDirection::LeftToRight => source_rank < target_rank,
        LayoutDirection::BottomToTop | LayoutDirection::RightToLeft => source_rank > target_rank,
    };
    let violations = points
        .windows(2)
        .filter(|segment| {
            let start = primary_axis_value(segment[0], direction);
            let end = primary_axis_value(segment[1], direction);
            if should_increase {
                end + epsilon < start
            } else {
                end > start + epsilon
            }
        })
        .count();
    clipped_u16(violations)
}

const fn primary_axis_value(point: (f32, f32), direction: LayoutDirection) -> f32 {
    match direction {
        LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => point.1,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => point.0,
    }
}

fn route_needs_detour(route: &EdgeRoute, obstacles: EdgeObstacles<'_>) -> bool {
    let nearby = obstacles.near_route(route, ROUTE_CLEARANCE_TARGET);
    route_obstacle_hit_count(route, &nearby, ROUTE_CLEARANCE_TARGET) > 0
}

pub(super) fn route_obstacle_hit_count(
    route: &EdgeRoute,
    obstacles: &[Rect],
    padding: f32,
) -> usize {
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

pub(super) const fn edge_route_obstacle_spacing(edge_count: usize) -> f32 {
    match edge_count {
        0..=127 => EDGE_ROUTE_OBSTACLE_SPACING,
        128..=255 => 14.0,
        256..=511 => 18.0,
        _ => 24.0,
    }
}

#[allow(clippy::cast_precision_loss)] // Edge fan-out counts are small in practice and only affect presentation.
pub(super) fn parallel_label_parameter(
    from: &str,
    to: &str,
    lane_index: usize,
    lane_total: usize,
) -> f32 {
    let position = (lane_index + 1) as f32 / (lane_total + 1) as f32;
    if from <= to { position } else { 1.0 - position }
}

pub(super) fn label_rect(label_x: f32, label_y: f32, label_half_w: f32) -> Rect {
    Rect {
        x: label_x - label_half_w,
        y: label_y - LABEL_HALF_H,
        w: label_half_w * 2.0,
        h: LABEL_HALF_H * 2.0,
    }
}

pub(super) fn edge_endpoint_marker_obstacles(
    route: &EdgeRoute,
    kind: EdgeKind,
    _nullable: bool,
    target_cardinality: Cardinality,
) -> Vec<Rect> {
    let points = route_points(route);
    let mut obstacles = Vec::with_capacity(2);

    if kind == EdgeKind::ForeignKey {
        if let Some(next) = distinct_route_neighbor(&points, true) {
            obstacles.push(endpoint_marker_obstacle(
                (route.x1, route.y1),
                next,
                FK_MARKER_CLEARANCE,
            ));
        }
        if let Some(prev) = distinct_route_neighbor(&points, false) {
            let target_clearance = match target_cardinality {
                Cardinality::ZeroOrOne => FK_MARKER_CLEARANCE,
                Cardinality::One | Cardinality::Many => FK_MARKER_CLEARANCE - 4.0,
            };
            obstacles.push(endpoint_marker_obstacle(
                (route.x2, route.y2),
                prev,
                target_clearance,
            ));
        }
        return obstacles;
    }

    if let Some(prev) = distinct_route_neighbor(&points, false) {
        obstacles.push(endpoint_marker_obstacle(
            (route.x2, route.y2),
            prev,
            ARROW_MARKER_CLEARANCE,
        ));
    }

    obstacles
}

fn distinct_route_neighbor(points: &[(f32, f32)], from_start: bool) -> Option<(f32, f32)> {
    if from_start {
        let anchor = points.first().copied()?;
        points.iter().copied().skip(1).find(|point| {
            (point.0 - anchor.0).abs() > f32::EPSILON || (point.1 - anchor.1).abs() > f32::EPSILON
        })
    } else {
        let anchor = points.last().copied()?;
        points.iter().rev().copied().skip(1).find(|point| {
            (point.0 - anchor.0).abs() > f32::EPSILON || (point.1 - anchor.1).abs() > f32::EPSILON
        })
    }
}

fn endpoint_marker_obstacle(endpoint: (f32, f32), toward: (f32, f32), clearance: f32) -> Rect {
    let dx = toward.0 - endpoint.0;
    let dy = toward.1 - endpoint.1;

    if dx.abs() >= dy.abs() {
        let min_x = endpoint.0.min(dx.signum().mul_add(clearance, endpoint.0));
        Rect {
            x: min_x,
            y: endpoint.1 - ENDPOINT_MARKER_HALF_THICKNESS,
            w: clearance,
            h: ENDPOINT_MARKER_HALF_THICKNESS * 2.0,
        }
    } else {
        let min_y = endpoint.1.min(dy.signum().mul_add(clearance, endpoint.1));
        Rect {
            x: endpoint.0 - ENDPOINT_MARKER_HALF_THICKNESS,
            y: min_y,
            w: ENDPOINT_MARKER_HALF_THICKNESS * 2.0,
            h: clearance,
        }
    }
}

#[cfg(test)]
pub(super) fn rect_overlaps_any(label: Rect, obstacles: &[Rect], margin: f32) -> bool {
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

pub(super) fn place_label_on_route(
    route: &EdgeRoute,
    preferred_t: f32,
    obstacles: &[Rect],
    margin: f32,
    label_half_w: f32,
) -> (f32, f32) {
    place_label_avoiding(route, preferred_t, obstacles, &[], margin, label_half_w)
}

/// Gap a nudged label may leave between its pill and its own line.
const LABEL_NEAR_GAP: f32 = 4.0;
/// Step of the sideways search near the line.
const LABEL_NEAR_STEP: f32 = 4.0;
/// Half size of the box kept clear around each bend of a label's own route,
/// so a pill never hides the corner it sits on.
const LABEL_BEND_HALF_SIZE: f32 = 4.0;

/// Places a label on its route, off every `hard` obstacle (cards, headings,
/// markers, other labels) and, when it can, off every `soft` one (other
/// lines) and off its own bends.
///
/// A label first slides along its route. If no spot is fully clear, it
/// steps sideways only as far as still keeps the pill on its line; a label
/// that merely crosses another line stays on its own line rather than
/// floating away. Only when every spot on the route would cover something
/// hard does it move farther out.
fn place_label_avoiding(
    route: &EdgeRoute,
    preferred_t: f32,
    hard: &[Rect],
    soft: &[Rect],
    margin: f32,
    label_half_w: f32,
) -> (f32, f32) {
    let points = route_points(route);
    let bends: Vec<Rect> = points
        .iter()
        .skip(1)
        .take(points.len().saturating_sub(2))
        .map(|&(x, y)| Rect {
            x: x - LABEL_BEND_HALF_SIZE,
            y: y - LABEL_BEND_HALF_SIZE,
            w: LABEL_BEND_HALF_SIZE * 2.0,
            h: LABEL_BEND_HALF_SIZE * 2.0,
        })
        .collect();
    let cost = |center: (f32, f32)| {
        let label = label_rect(center.0, center.1, label_half_w);
        (
            overlap_area(label, hard, margin),
            overlap_area(label, soft, 0.0) + overlap_area(label, &bends, 0.0),
        )
    };

    let mut ranked: Vec<LabelSpot> = Vec::new();
    for t in label_candidate_parameters(preferred_t) {
        let center = point_along_route(route, t);
        let candidate_cost = cost(center);
        if candidate_cost == (0.0, 0.0) {
            return center;
        }
        ranked.push((candidate_cost, center, t));
    }
    // Stable sort keeps the preferred parameter first among equal costs.
    ranked.sort_by(|left, right| {
        left.0
            .0
            .total_cmp(&right.0.0)
            .then_with(|| left.0.1.total_cmp(&right.0.1))
    });

    for &(_, center, t) in &ranked {
        let normal = route_normal_at(route, t);
        // Half the pill's extent across the line: its width beside a
        // vertical run, its height beside a horizontal one.
        let extent = normal
            .0
            .abs()
            .mul_add(label_half_w, normal.1.abs() * LABEL_HALF_H);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps = ((extent + LABEL_NEAR_GAP) / LABEL_NEAR_STEP).floor() as usize;
        #[allow(clippy::cast_precision_loss)]
        for offset in (1..=steps).map(|index| index as f32 * LABEL_NEAR_STEP) {
            for sign in [1.0, -1.0] {
                let shifted = (
                    (normal.0 * sign).mul_add(offset, center.0),
                    (normal.1 * sign).mul_add(offset, center.1),
                );
                if cost(shifted) == (0.0, 0.0) {
                    return shifted;
                }
            }
        }
    }

    let &((best_hard, _), best, best_t) = ranked
        .first()
        .expect("a route always yields at least one label candidate");
    if best_hard == 0.0 {
        return best;
    }

    let normal = route_normal_at(route, best_t);
    let mut all = hard.to_vec();
    all.extend_from_slice(soft);
    nudge_label(
        best,
        (best.0 - normal.1, best.1 + normal.0),
        (best.0 + normal.1, best.1 - normal.0),
        &all,
        margin,
        label_half_w,
        LABEL_ROUTE_FALLBACK_MAX_OFFSET.max(label_half_w),
    )
}

/// A candidate label spot: its hard and soft overlap, its center, and its
/// parameter along the route.
type LabelSpot = ((f32, f32), (f32, f32), f32);

/// Unit normal of the route around parameter `t`.
fn route_normal_at(route: &EdgeRoute, t: f32) -> (f32, f32) {
    let before = point_along_route(route, (t - 0.01).max(0.0));
    let after = point_along_route(route, (t + 0.01).min(1.0));
    let (dx, dy) = (after.0 - before.0, after.1 - before.1);
    let length = dx.hypot(dy);
    if length < f32::EPSILON {
        (0.0, -1.0)
    } else {
        (-dy / length, dx / length)
    }
}

/// Total area by which `label`, grown by `margin`, overlaps `obstacles`.
fn overlap_area(label: Rect, obstacles: &[Rect], margin: f32) -> f32 {
    obstacles
        .iter()
        .map(|obstacle| {
            let overlap_w = (label.x + label.w + margin).min(obstacle.x + obstacle.w)
                - (label.x - margin).max(obstacle.x);
            let overlap_h = (label.y + label.h + margin).min(obstacle.y + obstacle.h)
                - (label.y - margin).max(obstacle.y);
            overlap_w.max(0.0) * overlap_h.max(0.0)
        })
        .sum()
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

#[allow(clippy::too_many_lines)] // Obstacle setup plus the relaxation loop read most clearly together.
fn resolve_edge_label_collisions(
    edges: &mut [PositionedEdge],
    positioned_nodes: &[PositionedNode],
    fixed_obstacles: &[Rect],
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
        .chain(fixed_obstacles.iter().copied())
        .collect();
    let route_obstacles: Vec<Vec<Rect>> = edges
        .iter()
        .map(|edge| {
            sample_route_obstacles(
                &edge.route,
                EDGE_ROUTE_OBSTACLE_HALF_SIZE,
                edge_route_obstacle_spacing(edges.len()),
            )
        })
        .collect();
    let endpoint_marker_obstacles: Vec<Vec<Rect>> = edges
        .iter()
        .map(|edge| {
            edge_endpoint_marker_obstacles(
                &edge.route,
                edge.kind,
                edge.nullable,
                edge.target_cardinality,
            )
        })
        .collect();

    // Label text never changes during relaxation, so cache the half widths.
    let label_half_widths: Vec<f32> = edges
        .iter()
        .map(|edge| estimate_label_half_width(&edge.label))
        .collect();

    // Reachable region of an edge's candidate labels: centers lie on the route
    // polyline, nudging shifts them by up to `max_offset` along the normal, and
    // the rect spans the label half extents. Any obstacle outside the route
    // bbox grown by this reach can never overlap (and contributes exactly zero
    // to the overlap-area sum), so excluding it leaves the result unchanged.
    let label_reach: Vec<f32> = label_half_widths
        .iter()
        .map(|&half_w| label_reach(half_w))
        .collect();
    let route_bboxes: Vec<BBox> = edges
        .iter()
        .map(|edge| BBox::from_points(&route_points(&edge.route)))
        .collect();

    // Footprint covering every obstacle an edge contributes (route samples,
    // endpoint markers, and its reachable label), used to populate the index.
    let mut edge_grid = SpatialGrid::new(ROUTING_GRID_CELL_SIZE);
    for index in 0..edges.len() {
        let mut footprint = route_bboxes[index].expanded(label_reach[index]);
        for rect in &route_obstacles[index] {
            footprint.include_rect(rect);
        }
        for rect in &endpoint_marker_obstacles[index] {
            footprint.include_rect(rect);
        }
        edge_grid.insert(index, &footprint);
    }
    let mut node_grid = SpatialGrid::new(ROUTING_GRID_CELL_SIZE);
    for (index, node) in node_obstacles.iter().enumerate() {
        node_grid.insert(index, &BBox::from_rect(node));
    }

    for _ in 0..EDGE_LABEL_RELAXATION_PASSES {
        let mut changed = false;

        for index in 0..edges.len() {
            let label_half_w = label_half_widths[index];
            let query =
                route_bboxes[index].expanded(label_reach[index] + EDGE_LABEL_COLLISION_MARGIN);
            let nearby_nodes = node_grid.query_sorted(&query);
            let nearby_edges = edge_grid.query_sorted(&query);

            // Cards, headings, markers, and other labels are hard; other
            // lines are soft, so a label may cross one rather than drift off
            // its own line.
            let mut hard =
                Vec::with_capacity(nearby_nodes.len() + nearby_edges.len().saturating_mul(3));
            let mut soft = Vec::new();
            for &node_index in &nearby_nodes {
                hard.push(node_obstacles[node_index]);
            }
            hard.extend_from_slice(&endpoint_marker_obstacles[index]);

            for &other_index in &nearby_edges {
                if other_index == index {
                    continue;
                }
                hard.push(label_rect(
                    edges[other_index].label_x,
                    edges[other_index].label_y,
                    label_half_widths[other_index],
                ));
                hard.extend_from_slice(&endpoint_marker_obstacles[other_index]);
                soft.extend_from_slice(&route_obstacles[other_index]);
            }
            // Nearby edges contribute every sample along their whole route;
            // samples outside the reachable region can never overlap a label.
            hard.retain(|obstacle| query.intersects_rect(obstacle));
            soft.retain(|obstacle| query.intersects_rect(obstacle));

            let current_t = estimate_route_parameter(
                &edges[index].route,
                (edges[index].label_x, edges[index].label_y),
            );
            let updated = place_label_avoiding(
                &edges[index].route,
                current_t,
                &hard,
                &soft,
                EDGE_LABEL_COLLISION_MARGIN,
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
