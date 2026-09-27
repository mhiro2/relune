//! Regression tests for laying out large schemas.
//!
//! These are slow in debug builds, so they are ignored by default and run in
//! release mode by `make test-large`.

mod support;

use relune_core::{GroupingSpec, GroupingStrategy, LayoutAlgorithm};
use relune_layout::{
    EdgeRoute, LayoutConfig, LayoutRequest, PositionedGraph, PositionedNode,
    build_layout_with_config,
};

const SEED: u64 = 0x5eed;
/// Slack for `f32` rounding in the canvas bounds check.
const EPS: f32 = 0.5;

fn route_points(route: &EdgeRoute) -> impl Iterator<Item = (f32, f32)> + '_ {
    std::iter::once((route.x1, route.y1))
        .chain(route.control_points.iter().copied())
        .chain(std::iter::once((route.x2, route.y2)))
}

fn overlaps(a: &PositionedNode, b: &PositionedNode) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

fn assert_sound(graph: &PositionedGraph, tables: usize) {
    assert_eq!(graph.nodes.len(), tables);
    assert!(graph.width.is_finite() && graph.height.is_finite());
    let inside = |x: f32, y: f32| {
        x.is_finite()
            && y.is_finite()
            && (-EPS..=graph.width + EPS).contains(&x)
            && (-EPS..=graph.height + EPS).contains(&y)
    };

    for node in &graph.nodes {
        assert!(
            inside(node.x, node.y) && inside(node.x + node.width, node.y + node.height),
            "node {} ({}, {}, {} x {}) leaves the {} x {} canvas of {tables} tables",
            node.id,
            node.x,
            node.y,
            node.width,
            node.height,
            graph.width,
            graph.height
        );
    }
    for edge in &graph.edges {
        assert!(
            route_points(&edge.route).all(|(x, y)| inside(x, y)),
            "edge {} -> {} leaves the canvas",
            edge.from,
            edge.to
        );
        assert!(inside(edge.label_x, edge.label_y));
    }

    // Sweep along x so the overlap check stays near-linear for large graphs.
    let mut nodes: Vec<&PositionedNode> = graph.nodes.iter().collect();
    nodes.sort_by(|a, b| a.x.total_cmp(&b.x));
    for (index, node) in nodes.iter().enumerate() {
        for other in nodes[index + 1..]
            .iter()
            .take_while(|other| other.x < node.x + node.width)
        {
            assert!(
                !overlaps(node, other),
                "nodes {} and {} overlap",
                node.id,
                other.id
            );
        }
    }
}

fn check(tables: usize, mode: LayoutAlgorithm, strategy: GroupingStrategy) {
    let schema = support::synthetic_schema(tables, SEED);
    let request = LayoutRequest {
        grouping: GroupingSpec { strategy },
        ..LayoutRequest::default()
    };
    let config = LayoutConfig {
        mode,
        ..LayoutConfig::default()
    };
    let graph = build_layout_with_config(&schema, &request, &config).unwrap();
    assert_sound(&graph, tables);
}

#[test]
#[ignore = "slow in debug builds; run with `make test-large`"]
fn hierarchical_layout_of_large_schemas_is_sound() {
    for tables in [500, 1000] {
        check(
            tables,
            LayoutAlgorithm::Hierarchical,
            GroupingStrategy::None,
        );
        check(
            tables,
            LayoutAlgorithm::Hierarchical,
            GroupingStrategy::BySchema,
        );
    }
}

#[test]
#[ignore = "slow in debug builds; run with `make test-large`"]
fn force_layout_of_large_schemas_is_sound() {
    for tables in [500, 1000, 2000] {
        check(
            tables,
            LayoutAlgorithm::ForceDirected,
            GroupingStrategy::None,
        );
    }
}
