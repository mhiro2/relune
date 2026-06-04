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
use crate::port::{EdgePortAssignment, RegularPortAssignment, assign_edge_ports};
use crate::route::{
    AttachmentSide, ChannelAxis, LABEL_HALF_H, Rect, approximate_route_length,
    detour_around_obstacles_with_endpoint_sizes, estimate_label_half_width, nudge_label,
    point_along_route, rebuild_route_from_points, route_edge_with_assigned_ports, route_points,
    route_self_loop_with_offset, sample_route_obstacles, step_from_attachment,
};

use super::routing_debug::{build_regular_edge_debug, channel_axis_name};
use super::{
    LayoutConfig, LayoutError, PositionedEdge, PositionedEdgeRoutingDebug, PositionedNode,
};

/// Clearance target used while scoring obstacle-aware channel candidates.
const ROUTE_CLEARANCE_TARGET: f32 = 14.0;
/// Maximum gap between nearby channel candidates that may share one visual bundle.
const BUNDLE_CHANNEL_TOLERANCE: f32 = 36.0;
/// Distance used to preserve endpoint approach direction while entering a bypass channel.
const ROUTE_STUB_DISTANCE: f32 = 28.0;
/// Margin added when probing side corridors outside the endpoint nodes.
const BYPASS_CHANNEL_MARGIN: f32 = 24.0;
/// Distance between adjacent outer bypass lanes.
pub(super) const BYPASS_CHANNEL_LANE_STEP: f32 = 48.0;
/// Additional bypass lanes explored beyond the first outer lane on each side.
const BYPASS_CHANNEL_EXTRA_LANES: usize = 3;

/// Half-size of sampled edge-path obstacles used during label collision avoidance.
const EDGE_ROUTE_OBSTACLE_HALF_SIZE: f32 = 7.0;
/// Target spacing between sampled edge-path obstacles.
const EDGE_ROUTE_OBSTACLE_SPACING: f32 = 10.0;
/// Number of label-relaxation passes after all edge routes are known.
const EDGE_LABEL_RELAXATION_PASSES: usize = 3;
/// Labels should stay away from edge endpoints and markers.
pub(super) const MIN_LABEL_ROUTE_T: f32 = 0.16;
/// Candidate stride when sliding labels along their own route.
const LABEL_ROUTE_T_STEP: f32 = 0.08;
/// Maximum perpendicular fallback when a label cannot fit anywhere on its own route.
const LABEL_ROUTE_FALLBACK_MAX_OFFSET: f32 = 96.0;
/// Extra clearance reserved from a FK endpoint for Crow's Foot markers.
const FK_MARKER_CLEARANCE: f32 = 30.0;
/// Extra clearance reserved from a generic arrow endpoint.
const ARROW_MARKER_CLEARANCE: f32 = 14.0;
/// Half-thickness of the axis-aligned obstacle reserved for endpoint markers.
const ENDPOINT_MARKER_HALF_THICKNESS: f32 = 14.0;

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
    detour_obstacles: &'a [Rect],
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
        route_self_loop_with_offset(
            x,
            y,
            w,
            h,
            ctx.config.edge_style,
            ctx.config.direction,
            assignment.radius_offset,
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
                    obstacles: ctx.detour_obstacles,
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
            detour_around_obstacles_with_endpoint_sizes(
                &route,
                ctx.detour_obstacles,
                Some(AttachmentSide::East),
                Some(AttachmentSide::East),
                self_size,
                self_size,
            )
        }
        false if route_needs_detour(&route, ctx.detour_obstacles) => {
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
    positioned_nodes: &[PositionedNode],
    lane_index: usize,
    lane_total: usize,
    placed_labels: &mut Vec<Rect>,
) -> PositionedEdge {
    let from_pos = node_positions.get(source_edge.from.as_str());
    let to_pos = node_positions.get(source_edge.to.as_str());

    let mut label_obstacles: Vec<Rect> = positioned_nodes
        .iter()
        .filter(|node| node.id != source_edge.from && node.id != source_edge.to)
        .map(|node| Rect {
            x: node.x,
            y: node.y,
            w: node.width,
            h: node.height,
        })
        .collect();
    label_obstacles.extend(edge_endpoint_marker_obstacles(
        &draft.route,
        source_edge.kind,
        source_edge.nullable,
        source_edge.target_cardinality,
    ));
    label_obstacles.extend_from_slice(placed_labels);
    if let Some(&(x, y, w, h)) = from_pos {
        label_obstacles.push(Rect { x, y, w, h });
    }
    if !source_edge.is_self_loop
        && let Some(&(x, y, w, h)) = to_pos
    {
        label_obstacles.push(Rect { x, y, w, h });
    }

    let lhw = estimate_label_half_width(&draft.label);
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
    let (label_x, label_y) =
        place_label_on_route(&draft.route, preferred_t, &label_obstacles, 4.0, lhw);
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

fn collect_node_obstacles(positioned_nodes: &[PositionedNode]) -> Vec<(&str, Rect)> {
    positioned_nodes
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
        .collect()
}

pub(super) fn route_edges_with_diagnostics(
    graph: &LayoutGraph,
    positioned_nodes: &[PositionedNode],
    config: &LayoutConfig,
    node_ranks: Option<&[usize]>,
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

    let node_obstacles = collect_node_obstacles(positioned_nodes);
    let mut detour_obstacles: Vec<Rect> = Vec::with_capacity(node_obstacles.len());

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

        detour_obstacles.clear();
        let from_id = edge.from.as_str();
        let to_id = edge.to.as_str();
        detour_obstacles.extend(
            node_obstacles
                .iter()
                .filter(|(id, _)| *id != from_id && *id != to_id)
                .map(|(_, rect)| *rect),
        );

        let ctx = SingleEdgeRoutingContext {
            graph,
            config,
            node_ranks,
            rank_bounds: rank_bounds.as_deref(),
            detour_obstacles: &detour_obstacles,
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

    apply_parallel_edge_bundling(&mut routed_edges, positioned_nodes, graph);

    let mut edges = vec![None; graph.edges.len()];
    let mut placed_labels: Vec<Rect> = Vec::new();

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
            positioned_nodes,
            lane_index,
            lane_total,
            &mut placed_labels,
        ));
    }

    let mut edges: Vec<_> = edges.into_iter().flatten().collect();
    resolve_edge_label_collisions(&mut edges, positioned_nodes);
    Ok((edges, diagnostics))
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
                    ) {
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
    graph: &LayoutGraph,
    edge: &RoutedEdgeDraft,
    metadata: BundleRouteMetadata,
    positioned_nodes: &[PositionedNode],
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
struct ChannelSearchPlan {
    axis: ChannelAxis,
    baseline: f32,
    class: ChannelCandidateClass,
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
    pub(super) obstacles: &'a [Rect],
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
        candidates.extend(bypass_channel_candidates(
            context.direction,
            source_rect,
            target_rect,
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

fn channel_search_plan(
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

pub(super) fn bypass_channel_candidates(
    direction: LayoutDirection,
    source_rect: Rect,
    target_rect: Rect,
    start_order: u32,
) -> Vec<ObstacleAwareChannelCandidate> {
    let mut candidates = Vec::with_capacity(bypass_channel_lane_count().saturating_mul(2));

    match direction {
        LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => {
            let right_baseline = (source_rect.x + source_rect.w).max(target_rect.x + target_rect.w)
                + BYPASS_CHANNEL_MARGIN;
            let left_baseline = source_rect.x.min(target_rect.x) - BYPASS_CHANNEL_MARGIN;
            append_bypass_candidates(
                &mut candidates,
                ChannelAxis::X,
                right_baseline,
                left_baseline,
                start_order,
            );
        }
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => {
            let bottom_baseline = (source_rect.y + source_rect.h)
                .max(target_rect.y + target_rect.h)
                + BYPASS_CHANNEL_MARGIN;
            let top_baseline = source_rect.y.min(target_rect.y) - BYPASS_CHANNEL_MARGIN;
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
            if context.source_rank < context.target_rank {
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
            if context.source_rank < context.target_rank {
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
    obstacles: &[Rect],
    direction: LayoutDirection,
    source_rank: usize,
    target_rank: usize,
    source_side: AttachmentSide,
    target_side: AttachmentSide,
    candidate: ObstacleAwareChannelCandidate,
    channel_usage: &BTreeMap<(ChannelAxis, i32), u32>,
) -> ChannelCandidateScore {
    let hard_constraint_violations = clipped_u16(route_obstacle_hit_count(route, obstacles, 0.0))
        + route_primary_direction_violations(route, direction, source_rank, target_rank)
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

fn route_needs_detour(route: &EdgeRoute, obstacles: &[Rect]) -> bool {
    route_obstacle_hit_count(route, obstacles, ROUTE_CLEARANCE_TARGET) > 0
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

    for _ in 0..EDGE_LABEL_RELAXATION_PASSES {
        let mut changed = false;

        for index in 0..edges.len() {
            let label_half_w = estimate_label_half_width(&edges[index].label);
            let mut obstacles =
                Vec::with_capacity(node_obstacles.len() + edges.len().saturating_mul(4));
            obstacles.extend_from_slice(&node_obstacles);
            obstacles.extend_from_slice(&endpoint_marker_obstacles[index]);

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
                obstacles.extend_from_slice(&endpoint_marker_obstacles[other_index]);
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
