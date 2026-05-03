//! Force-directed layout, repulsion, overlap resolution, and group packing.

use relune_core::LayoutDirection;

use crate::graph::LayoutGraph;

use super::hierarchical::assign_coordinates;
use super::spacing::{
    build_positioned_node, compute_graph_bounds, mirror_positioned_nodes_for_direction,
};
use super::{
    GROUP_PADDING, GROUP_TOP_PADDING, LayoutConfig, LayoutError, NodeSize, PositionedNode,
};

/// Node count threshold above which the spatial grid is used for repulsion.
const SPATIAL_GRID_THRESHOLD: usize = 64;
/// Minimum gap preserved between packed force-directed groups.
const FORCE_GROUP_GAP: f32 = 28.0;
/// Minimum visible gap preserved between connected node rectangles in force-directed mode.
///
/// Applied on **both** axes between endpoints so orthogonal edge stubs stay long enough for
/// Crow's Foot SVG markers (roughly 24–26px along the path from each vertex).  Hierarchical
/// layout never uses this constant.
pub(super) const FORCE_CONNECTED_NODE_GAP: f32 = 64.0;

/// Apply a single repulsion pair force between nodes `i` and `j`.
///
/// `node_radii` carries the precomputed `max(width, height) * 0.5` for every
/// node so that the per-pair spacing reduces to a couple of additions instead
/// of recomputing the radii on every iteration.
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
    node_radii: &[f32],
    config: &LayoutConfig,
    repulsion_strength: f32,
    min_distance: f32,
    forces: &mut [(f32, f32)],
) {
    let dx = positions[i].0 - positions[j].0;
    let dy = positions[i].1 - positions[j].1;

    // Compute axis-aware minimum separation for rectangular nodes.
    let half_w = (node_sizes[i].width + node_sizes[j].width).mul_add(0.5, config.node_padding);
    let half_h = (node_sizes[i].height + node_sizes[j].height).mul_add(0.5, config.node_padding);

    let overlap_x = half_w - dx.abs();
    let overlap_y = half_h - dy.abs();

    if overlap_x > 0.0 && overlap_y > 0.0 {
        // Nodes overlap — apply a strong separation impulse along the axis of
        // least penetration so the simulation can resolve it quickly.
        let push = repulsion_strength * 0.002;
        if overlap_x < overlap_y {
            let sign = if dx >= 0.0 { 1.0 } else { -1.0 };
            forces[i].0 += push * sign * overlap_x;
            forces[j].0 -= push * sign * overlap_x;
        } else {
            let sign = if dy >= 0.0 { 1.0 } else { -1.0 };
            forces[i].1 += push * sign * overlap_y;
            forces[j].1 -= push * sign * overlap_y;
        }
    }

    // Standard distance-based repulsion (keeps non-overlapping nodes apart).
    let min_gap = config
        .node_padding
        .mul_add(2.0, node_radii[i] + node_radii[j]);
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
    node_radii: &[f32],
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

    // Collect candidate pairs first, then apply repulsion in a sorted order so
    // the per-pair traversal is deterministic regardless of `HashMap` iteration
    // order. Without sorting, floating-point non-associativity would make the
    // accumulated force values depend on the random `HashMap` seed once the
    // graph crosses the spatial-grid threshold.
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (&(cx, cy), cell_nodes) in &grid {
        for (a, &i) in cell_nodes.iter().enumerate() {
            for &j in &cell_nodes[a + 1..] {
                pairs.push((i.min(j), i.max(j)));
            }
        }
        for &(dx, dy) in &neighbour_offsets {
            if let Some(neighbour_nodes) = grid.get(&(cx + dx, cy + dy)) {
                for &i in cell_nodes {
                    for &j in neighbour_nodes {
                        pairs.push((i.min(j), i.max(j)));
                    }
                }
            }
        }
    }

    pairs.sort_unstable();

    for &(i, j) in &pairs {
        apply_repulsion_pair(
            i,
            j,
            positions,
            node_sizes,
            node_radii,
            config,
            repulsion_strength,
            min_distance,
            forces,
        );
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
pub(super) fn apply_force_layout(
    graph: &LayoutGraph,
    config: &LayoutConfig,
    node_sizes: &[NodeSize],
    ordered_nodes: &[Vec<usize>],
) -> Result<(Vec<PositionedNode>, f32, f32), LayoutError> {
    let n = graph.nodes.len();
    if n == 0 {
        return Ok((Vec::new(), config.origin_x * 2.0, config.origin_y * 2.0));
    }

    // Force parameters
    let repulsion_strength = 5000.0;
    let attraction_strength = 0.05;
    let gravity_strength = 0.1;
    let primary_axis_gravity_strength = 0.18;
    let damping = 0.9;
    let min_distance = 1.0;

    let canonical_config = force_layout_canonical_config(config);
    let seed_nodes = force_layout_seed_nodes(graph, ordered_nodes, &canonical_config, node_sizes)?;
    let mut positions: Vec<(f32, f32)> = seed_nodes.iter().map(|node| (node.x, node.y)).collect();
    let primary_targets: Vec<f32> = seed_nodes.iter().map(|node| node.y).collect();
    let (center_x, center_y) = force_layout_seed_center(&positions, node_sizes);

    // Precompute the per-node "radius" used by `node_pair_spacing`. These do
    // not change during simulation, so caching them avoids `iterations × pairs`
    // recomputations of the same `max(w, h) * 0.5` value.
    let node_radii: Vec<f32> = node_sizes
        .iter()
        .map(|s| s.width.max(s.height) * 0.5)
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
                &node_radii,
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
                        &node_radii,
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
                edge_target_distance(node_sizes[from_idx], node_sizes[to_idx], dx, dy, config);

            // Attractive force: F = k * d
            let force = attraction_strength * (dist - target_distance);
            let fx = force * dx / dist;
            let fy = force * dy / dist;

            forces[from_idx].0 += fx;
            forces[from_idx].1 += fy;
            forces[to_idx].0 -= fx;
            forces[to_idx].1 -= fy;
        }

        // Rank-guided gravity keeps the semantic parent/child order aligned with
        // the canonical top-to-bottom simulation axis while still allowing force
        // relaxation on the secondary axis.
        for i in 0..n {
            let primary_delta = primary_targets[i] - positions[i].1;
            forces[i].1 += primary_axis_gravity_strength * primary_delta;

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

    // Post-simulation overlap resolution: iteratively push apart any
    // remaining overlapping node pairs.
    resolve_force_overlaps(&mut positions, node_sizes, config.node_padding);

    // Compact: pull connected nodes closer to remove excess space introduced
    // by the overlap resolution cascade.
    compact_toward_neighbours(&mut positions, node_sizes, &edges, config);

    // Preserve a visible corridor between connected nodes so short orthogonal
    // routes do not collapse into barely-visible edge stubs.
    enforce_force_edge_clearance(&mut positions, node_sizes, &edges);

    // Re-resolve any overlaps introduced while widening connected node gaps.
    resolve_force_overlaps(&mut positions, node_sizes, config.node_padding);

    // Grouped force-directed layouts need an explicit packing pass so schema
    // containers do not overlap and cover each other's label bands. Packing
    // always advances simulation-X so a later LR/RL transpose maps it to screen Y.
    separate_force_groups(graph, &mut positions, node_sizes, false);

    // Group packing can tighten connected pairs again, especially in
    // left-to-right layouts where ungrouped nodes share the same column.
    enforce_force_edge_clearance(&mut positions, node_sizes, &edges);
    resolve_force_overlaps(&mut positions, node_sizes, config.node_padding);
    separate_force_groups(graph, &mut positions, node_sizes, false);

    // Last group pack only moves along the secondary axis; restore FK corridor
    // gaps so edge backbones (especially first/last orthogonal legs) stay long
    // enough for markers after packing.
    enforce_force_edge_clearance(&mut positions, node_sizes, &edges);
    resolve_force_overlaps(&mut positions, node_sizes, config.node_padding);
    restore_force_primary_axis_positions(&mut positions, &primary_targets);
    resolve_force_overlaps(&mut positions, node_sizes, config.node_padding);
    enforce_force_edge_clearance(&mut positions, node_sizes, &edges);
    resolve_force_overlaps(&mut positions, node_sizes, config.node_padding);

    if matches!(
        config.direction,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft
    ) {
        for pos in &mut positions {
            std::mem::swap(&mut pos.0, &mut pos.1);
        }
        // Clearance was enforced in canonical TB simulation space; swapping axes
        // can leave the former vertical gap as the on-screen horizontal gap.
        enforce_force_edge_clearance(&mut positions, node_sizes, &edges);
        resolve_force_overlaps(&mut positions, node_sizes, config.node_padding);
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
    let mut positioned_nodes: Vec<PositionedNode> = graph
        .nodes
        .iter()
        .zip(positions.iter().zip(node_sizes.iter()))
        .map(|(node, (&(x, y), size))| {
            build_positioned_node(node, x, y, size.width, size.height, config.show_columns)
        })
        .collect();

    let graph_bounds = compute_graph_bounds(&positioned_nodes, config);
    mirror_positioned_nodes_for_direction(&mut positioned_nodes, graph_bounds, config.direction);
    let (width, height) = compute_graph_bounds(&positioned_nodes, config);

    Ok((positioned_nodes, width, height))
}

pub(super) fn force_layout_canonical_config(config: &LayoutConfig) -> LayoutConfig {
    let mut canonical = config.clone();
    canonical.direction = LayoutDirection::TopToBottom;
    if matches!(
        config.direction,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft
    ) {
        std::mem::swap(
            &mut canonical.horizontal_spacing,
            &mut canonical.vertical_spacing,
        );
    }
    canonical
}

fn force_layout_seed_nodes(
    graph: &LayoutGraph,
    ordered_nodes: &[Vec<usize>],
    config: &LayoutConfig,
    node_sizes: &[NodeSize],
) -> Result<Vec<PositionedNode>, LayoutError> {
    let (positioned_nodes, _, _) = assign_coordinates(graph, ordered_nodes, config, node_sizes)?;
    Ok(positioned_nodes)
}

fn force_layout_seed_center(positions: &[(f32, f32)], node_sizes: &[NodeSize]) -> (f32, f32) {
    let min_x = positions.iter().map(|pos| pos.0).fold(f32::MAX, f32::min);
    let min_y = positions.iter().map(|pos| pos.1).fold(f32::MAX, f32::min);
    let max_x = positions
        .iter()
        .zip(node_sizes)
        .map(|(pos, size)| pos.0 + size.width)
        .fold(f32::MIN, f32::max);
    let max_y = positions
        .iter()
        .zip(node_sizes)
        .map(|(pos, size)| pos.1 + size.height)
        .fold(f32::MIN, f32::max);

    (f32::midpoint(min_x, max_x), f32::midpoint(min_y, max_y))
}

fn restore_force_primary_axis_positions(positions: &mut [(f32, f32)], primary_targets: &[f32]) {
    for ((_, y), target_y) in positions.iter_mut().zip(primary_targets) {
        *y = *target_y;
    }
}

/// Direction-aware target distance for edge attraction.
///
/// Uses the actual width/height projected along the centre-to-centre axis
/// instead of the conservative `max(w,h)` used for repulsion.  This keeps
/// connected nodes closer together and produces shorter edges.
#[allow(clippy::similar_names)]
fn edge_target_distance(a: NodeSize, b: NodeSize, dx: f32, dy: f32, config: &LayoutConfig) -> f32 {
    let abs_dx = dx.abs();
    let abs_dy = dy.abs();
    // Blend between width-based and height-based separation according to
    // the direction between the two centres.
    let sum = abs_dx + abs_dy;
    if sum < 1.0 {
        // Nearly coincident — fall back to average half-extent.
        let avg = (a.width + a.height + b.width + b.height) * 0.25;
        return config.node_padding.mul_add(2.0, avg);
    }
    let wx = abs_dx / sum; // weight towards horizontal
    let wy = abs_dy / sum; // weight towards vertical
    let half_a = (wx * a.width).mul_add(0.5, wy * a.height * 0.5);
    let half_b = (wx * b.width).mul_add(0.5, wy * b.height * 0.5);
    // Add directional spacing so edges remain visible between nodes.
    let edge_gap =
        (wx * config.horizontal_spacing).mul_add(0.4, wy * config.vertical_spacing * 0.4);
    config.node_padding.mul_add(2.0, half_a + half_b) + edge_gap
}

/// Push apart any overlapping node pairs after force simulation.
///
/// Iteratively resolves rectangle-rectangle overlaps by displacing each
/// pair along the axis of minimum penetration. Candidate pairs are pruned
/// with a uniform spatial grid so the per-pass cost scales with the number
/// of actually-nearby nodes rather than `N(N-1)/2`.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names
)]
pub(super) fn resolve_force_overlaps(
    positions: &mut [(f32, f32)],
    node_sizes: &[NodeSize],
    padding: f32,
) {
    use std::collections::HashMap;

    let n = positions.len();
    if n <= 1 {
        return;
    }
    let max_passes = 80;

    // Pick a cell size large enough that any two padded AABBs which actually
    // overlap fall into the same cell or into adjacent cells. Two rectangles
    // with widths `W_a`, `W_b` and `padding` margin overlap on the X axis only
    // when `|x_a - x_b| <= max(W_a, W_b) + padding`, so taking
    // `cell_size = max(W, H) + padding` bounds the cell distance to <= 1.
    let max_span = node_sizes
        .iter()
        .map(|s| s.width.max(s.height))
        .fold(0.0_f32, f32::max);
    let cell_size = (max_span + padding).max(1.0);
    let inv_cell = 1.0 / cell_size;

    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    // Forward neighbours only: each cross-cell pair is reported exactly once
    // because reverse offsets are not in this list.
    let neighbour_offsets: [(i32, i32); 4] = [(1, 0), (0, 1), (1, 1), (-1, 1)];

    for _ in 0..max_passes {
        grid.clear();
        candidates.clear();

        // Bin nodes by the cell containing their top-left corner.
        for (i, &(px, py)) in positions.iter().enumerate() {
            let cx = (px * inv_cell).floor() as i32;
            let cy = (py * inv_cell).floor() as i32;
            grid.entry((cx, cy)).or_default().push(i);
        }

        // Collect candidate pairs from same-cell and forward-neighbour cells.
        for (&(cx, cy), cell_nodes) in &grid {
            for (a, &i) in cell_nodes.iter().enumerate() {
                for &j in &cell_nodes[a + 1..] {
                    candidates.push((i.min(j), i.max(j)));
                }
            }
            for &(dx, dy) in &neighbour_offsets {
                if let Some(neighbour_nodes) = grid.get(&(cx + dx, cy + dy)) {
                    for &i in cell_nodes {
                        for &j in neighbour_nodes {
                            candidates.push((i.min(j), i.max(j)));
                        }
                    }
                }
            }
        }

        // Sort so the per-pair traversal order is deterministic regardless of
        // `HashMap` iteration order. Forward-only neighbour walks already make
        // each pair appear at most once, so no dedup pass is needed.
        candidates.sort_unstable();

        let mut moved = false;
        for &(i, j) in &candidates {
            let dx = positions[j].0 - positions[i].0;
            let dy = positions[j].1 - positions[i].1;

            let overlap_x = if dx >= 0.0 {
                positions[i].0 + node_sizes[i].width + padding - positions[j].0
            } else {
                positions[j].0 + node_sizes[j].width + padding - positions[i].0
            };
            let overlap_y = if dy >= 0.0 {
                positions[i].1 + node_sizes[i].height + padding - positions[j].1
            } else {
                positions[j].1 + node_sizes[j].height + padding - positions[i].1
            };

            if overlap_x > 0.0 && overlap_y > 0.0 {
                // Push apart along the axis of least overlap.
                if overlap_x < overlap_y {
                    let push = overlap_x * 0.5 + 0.5;
                    let sign = if dx >= 0.0 { 1.0_f32 } else { -1.0 };
                    positions[i].0 -= push * sign;
                    positions[j].0 += push * sign;
                } else {
                    let push = overlap_y * 0.5 + 0.5;
                    let sign = if dy >= 0.0 { 1.0_f32 } else { -1.0 };
                    positions[i].1 -= push * sign;
                    positions[j].1 += push * sign;
                }
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
}

/// Pull connected nodes closer after overlap resolution.
///
/// The overlap cascade can push nodes far from their neighbours. This pass
/// moves each node toward the centroid of its connected neighbours, then
/// re-runs overlap resolution to guarantee no new overlaps are introduced.
#[allow(clippy::cast_precision_loss)]
fn compact_toward_neighbours(
    positions: &mut [(f32, f32)],
    node_sizes: &[NodeSize],
    edges: &[(usize, usize)],
    config: &LayoutConfig,
) {
    let n = positions.len();
    if n <= 1 || edges.is_empty() {
        return;
    }

    // Build adjacency: for each node, collect its neighbours.
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in edges {
        adj[a].push(b);
        adj[b].push(a);
    }

    let step = 0.25_f32; // fraction of the gap to close per pass
    let passes = 15;
    // Minimum centre-to-centre distance to preserve between connected nodes
    // so that edges remain clearly visible.
    let min_edge_gap = config.horizontal_spacing.min(config.vertical_spacing) * 0.35;

    for _ in 0..passes {
        let mut any_moved = false;
        let old_positions = positions.to_vec();

        for i in 0..n {
            if adj[i].is_empty() {
                continue;
            }
            // Centroid of neighbours.
            let mut cx = 0.0_f32;
            let mut cy = 0.0_f32;
            for &j in &adj[i] {
                cx += old_positions[j].0;
                cy += old_positions[j].1;
            }
            cx /= adj[i].len() as f32;
            cy /= adj[i].len() as f32;

            let dx = cx - positions[i].0;
            let dy = cy - positions[i].1;

            // Only pull toward centroid if we are far enough from all
            // neighbours; otherwise compaction squeezes edges too short.
            let too_close = adj[i].iter().any(|&j| {
                let ndx = old_positions[j].0 - positions[i].0;
                let ndy = old_positions[j].1 - positions[i].1;
                let half_w = (node_sizes[i].width + node_sizes[j].width) * 0.5;
                let half_h = (node_sizes[i].height + node_sizes[j].height) * 0.5;
                let clear_x = ndx.abs() - half_w;
                let clear_y = ndy.abs() - half_h;
                clear_x.max(clear_y) < min_edge_gap
            });
            if too_close {
                continue;
            }

            let move_x = dx * step;
            let move_y = dy * step;

            if move_x.abs() > 1.0 || move_y.abs() > 1.0 {
                positions[i].0 += move_x;
                positions[i].1 += move_y;
                any_moved = true;
            }
        }

        if !any_moved {
            break;
        }

        // Re-resolve any overlaps introduced by compaction.
        resolve_force_overlaps(positions, node_sizes, config.node_padding);
    }
}

/// Axis-aligned separation between two node rectangles (`x`, `y`, `width`, `height`).
///
/// Positive `gap_x` / `gap_y` mean the boxes are separated on that axis; negative values
/// mean overlap along that axis (projection overlap).
#[allow(clippy::similar_names, clippy::too_many_arguments)] // Eight floats are clearer than a bespoke rect pair type here.
pub(super) fn force_pair_axis_gaps(
    ax: f32,
    ay: f32,
    aw: f32,
    ah: f32,
    bx: f32,
    by: f32,
    bw: f32,
    bh: f32,
) -> (f32, f32) {
    let gap_x = if ax + aw <= bx {
        bx - (ax + aw)
    } else if bx + bw <= ax {
        ax - (bx + bw)
    } else {
        -((ax + aw).min(bx + bw) - ax.max(bx))
    };

    let gap_y = if ay + ah <= by {
        by - (ay + ah)
    } else if by + bh <= ay {
        ay - (by + bh)
    } else {
        -((ay + ah).min(by + bh) - ay.max(by))
    };

    (gap_x, gap_y)
}

#[allow(clippy::cast_precision_loss, clippy::similar_names)]
fn enforce_force_edge_clearance(
    positions: &mut [(f32, f32)],
    node_sizes: &[NodeSize],
    edges: &[(usize, usize)],
) {
    if edges.is_empty() {
        return;
    }

    // Separating one axis can tighten the other; a few extra passes stabilise diagonals.
    let passes = 12;
    for _ in 0..passes {
        let mut moved = false;

        for &(from_idx, to_idx) in edges {
            let from_center_x = node_sizes[from_idx]
                .width
                .mul_add(0.5, positions[from_idx].0);
            let from_center_y = node_sizes[from_idx]
                .height
                .mul_add(0.5, positions[from_idx].1);
            let to_center_x = node_sizes[to_idx].width.mul_add(0.5, positions[to_idx].0);
            let to_center_y = node_sizes[to_idx].height.mul_add(0.5, positions[to_idx].1);

            let dx = to_center_x - from_center_x;
            let dy = to_center_y - from_center_y;

            let aw = node_sizes[from_idx].width;
            let ah = node_sizes[from_idx].height;
            let bw = node_sizes[to_idx].width;
            let bh = node_sizes[to_idx].height;

            let (gap_x, gap_y) = force_pair_axis_gaps(
                positions[from_idx].0,
                positions[from_idx].1,
                aw,
                ah,
                positions[to_idx].0,
                positions[to_idx].1,
                bw,
                bh,
            );

            if gap_x < FORCE_CONNECTED_NODE_GAP {
                let gap = if dx >= 0.0 {
                    positions[to_idx].0 - (positions[from_idx].0 + aw)
                } else {
                    positions[from_idx].0 - (positions[to_idx].0 + bw)
                };
                let push = (FORCE_CONNECTED_NODE_GAP - gap) * 0.5;
                let sign = if dx >= 0.0 { 1.0_f32 } else { -1.0 };
                positions[from_idx].0 -= push * sign;
                positions[to_idx].0 += push * sign;
                moved = true;
            }

            // Horizontal nudges do not change `gap_y`, so the initial `gap_y` stays valid here.
            if gap_y < FORCE_CONNECTED_NODE_GAP {
                let gap = if dy >= 0.0 {
                    positions[to_idx].1 - (positions[from_idx].1 + ah)
                } else {
                    positions[from_idx].1 - (positions[to_idx].1 + bh)
                };
                let push = (FORCE_CONNECTED_NODE_GAP - gap) * 0.5;
                let sign = if dy >= 0.0 { 1.0_f32 } else { -1.0 };
                positions[from_idx].1 -= push * sign;
                positions[to_idx].1 += push * sign;
                moved = true;
            }
        }

        if !moved {
            break;
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PackedBounds {
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

#[derive(Debug, Clone, Copy)]
enum ForcePackItem {
    Group(usize),
    UngroupedNode(usize),
}

#[allow(clippy::cast_precision_loss)]
fn separate_force_groups(
    graph: &LayoutGraph,
    positions: &mut [(f32, f32)],
    node_sizes: &[NodeSize],
    pack_along_sim_y: bool,
) {
    // With two or more logical groups (prefix clusters, multi-schema, …) pack
    // group bounding boxes plus any truly-ungrouped nodes along the secondary axis.
    //
    // With **no** groups (`GroupingStrategy::None`) or a **single** schema bucket
    // (`BySchema` on one schema), `packed_items` would otherwise contain at most
    // one `Group` entry and this pass would become a no-op — tables then stay
    // stacked on the secondary axis and FK edges stay too short for markers.
    let mut packed_items: Vec<(ForcePackItem, PackedBounds)> = if graph.groups.len() >= 2 {
        graph
            .groups
            .iter()
            .enumerate()
            .filter_map(|(group_idx, group)| {
                force_group_bounds(group, positions, node_sizes)
                    .map(|bounds| (ForcePackItem::Group(group_idx), bounds))
            })
            .chain(
                graph
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, node)| node.group_index.is_none())
                    .map(|(node_idx, _)| {
                        (
                            ForcePackItem::UngroupedNode(node_idx),
                            force_node_bounds(node_idx, positions, node_sizes),
                        )
                    }),
            )
            .collect()
    } else {
        (0..graph.nodes.len())
            .map(|node_idx| {
                (
                    ForcePackItem::UngroupedNode(node_idx),
                    force_node_bounds(node_idx, positions, node_sizes),
                )
            })
            .collect()
    };

    if packed_items.len() < 2 {
        return;
    }

    packed_items.sort_by(|(left_item, left_bounds), (right_item, right_bounds)| {
        let left_min = if pack_along_sim_y {
            left_bounds.min_y
        } else {
            left_bounds.min_x
        };
        let right_min = if pack_along_sim_y {
            right_bounds.min_y
        } else {
            right_bounds.min_x
        };
        left_min.total_cmp(&right_min).then_with(|| {
            force_pack_item_order(*left_item).cmp(&force_pack_item_order(*right_item))
        })
    });

    let mut previous_item = packed_items[0].0;
    let mut previous_end = if pack_along_sim_y {
        packed_items[0].1.max_y
    } else {
        packed_items[0].1.max_x
    };

    for &(item, bounds) in packed_items.iter().skip(1) {
        let current_min = if pack_along_sim_y {
            bounds.min_y
        } else {
            bounds.min_x
        };
        let required_min = previous_end + force_pack_gap(graph, previous_item, item);
        if current_min < required_min {
            let delta = required_min - current_min;
            shift_force_pack_item(graph, positions, item, delta, pack_along_sim_y);
            previous_end = if pack_along_sim_y {
                bounds.max_y + delta
            } else {
                bounds.max_x + delta
            };
        } else {
            previous_end = if pack_along_sim_y {
                bounds.max_y
            } else {
                bounds.max_x
            };
        }
        previous_item = item;
    }
}

fn force_pack_gap(graph: &LayoutGraph, left: ForcePackItem, right: ForcePackItem) -> f32 {
    if force_pack_items_are_connected(graph, left, right) {
        FORCE_CONNECTED_NODE_GAP.max(FORCE_GROUP_GAP)
    } else {
        FORCE_GROUP_GAP
    }
}

fn force_pack_items_are_connected(
    graph: &LayoutGraph,
    left: ForcePackItem,
    right: ForcePackItem,
) -> bool {
    match (left, right) {
        (ForcePackItem::UngroupedNode(left_idx), ForcePackItem::UngroupedNode(right_idx)) => {
            force_nodes_are_connected(graph, left_idx, right_idx)
        }
        (ForcePackItem::Group(group_idx), ForcePackItem::UngroupedNode(node_idx))
        | (ForcePackItem::UngroupedNode(node_idx), ForcePackItem::Group(group_idx)) => graph.groups
            [group_idx]
            .node_indices
            .iter()
            .any(|&group_node_idx| force_nodes_are_connected(graph, group_node_idx, node_idx)),
        (ForcePackItem::Group(left_group_idx), ForcePackItem::Group(right_group_idx)) => graph
            .groups[left_group_idx]
            .node_indices
            .iter()
            .any(|&left_node_idx| {
                graph.groups[right_group_idx]
                    .node_indices
                    .iter()
                    .any(|&right_node_idx| {
                        force_nodes_are_connected(graph, left_node_idx, right_node_idx)
                    })
            }),
    }
}

fn force_nodes_are_connected(graph: &LayoutGraph, left_idx: usize, right_idx: usize) -> bool {
    let left_id = &graph.nodes[left_idx].id;
    let right_id = &graph.nodes[right_idx].id;

    graph.edges.iter().any(|edge| {
        (edge.from == *left_id && edge.to == *right_id)
            || (edge.from == *right_id && edge.to == *left_id)
    })
}

fn force_group_bounds(
    group: &crate::graph::LayoutGroup,
    positions: &[(f32, f32)],
    node_sizes: &[NodeSize],
) -> Option<PackedBounds> {
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    let mut has_nodes = false;

    for &node_idx in &group.node_indices {
        let Some(&(x, y)) = positions.get(node_idx) else {
            continue;
        };
        let Some(size) = node_sizes.get(node_idx) else {
            continue;
        };
        has_nodes = true;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + size.width);
        max_y = max_y.max(y + size.height);
    }

    has_nodes.then_some(PackedBounds {
        min_x: min_x - GROUP_PADDING,
        max_x: max_x + GROUP_PADDING,
        min_y: min_y - GROUP_TOP_PADDING,
        max_y: max_y + GROUP_PADDING,
    })
}

fn force_node_bounds(
    node_idx: usize,
    positions: &[(f32, f32)],
    node_sizes: &[NodeSize],
) -> PackedBounds {
    let (x, y) = positions[node_idx];
    let size = node_sizes[node_idx];
    PackedBounds {
        min_x: x,
        max_x: x + size.width,
        min_y: y,
        max_y: y + size.height,
    }
}

const fn force_pack_item_order(item: ForcePackItem) -> (u8, usize) {
    match item {
        ForcePackItem::Group(index) => (0, index),
        ForcePackItem::UngroupedNode(index) => (1, index),
    }
}

fn shift_force_pack_item(
    graph: &LayoutGraph,
    positions: &mut [(f32, f32)],
    item: ForcePackItem,
    delta: f32,
    pack_along_sim_y: bool,
) {
    match item {
        ForcePackItem::Group(group_idx) => {
            for &node_idx in &graph.groups[group_idx].node_indices {
                if let Some((x, y)) = positions.get_mut(node_idx) {
                    if pack_along_sim_y {
                        *y += delta;
                    } else {
                        *x += delta;
                    }
                }
            }
        }
        ForcePackItem::UngroupedNode(node_idx) => {
            if let Some((x, y)) = positions.get_mut(node_idx) {
                if pack_along_sim_y {
                    *y += delta;
                } else {
                    *x += delta;
                }
            }
        }
    }
}
