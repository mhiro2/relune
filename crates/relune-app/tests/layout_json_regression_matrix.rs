//! Layout-json regression snapshots and reusable routing invariants.

use relune_app::{
    ExportFormat, ExportRequest, InputSource, LayoutDirection, LayoutSpec, RouteStyle, export,
};
use relune_core::{GroupingSpec, GroupingStrategy, LayoutAlgorithm};
use relune_testkit::{
    assert_directional_layout_invariants, assert_layout_geometry, compact_layout_snapshot,
    layout_regression_fixture_names, parse_layout_json, sql_fixture_path,
};

const DIRECTIONS: &[(LayoutDirection, &str)] = &[
    (LayoutDirection::TopToBottom, "top_to_bottom"),
    (LayoutDirection::LeftToRight, "left_to_right"),
    (LayoutDirection::RightToLeft, "right_to_left"),
    (LayoutDirection::BottomToTop, "bottom_to_top"),
];

const EDGE_STYLES: &[(RouteStyle, &str)] = &[
    (RouteStyle::Straight, "straight"),
    (RouteStyle::Orthogonal, "orthogonal"),
    (RouteStyle::Curved, "curved"),
];

/// Must match `FORCE_CONNECTED_NODE_GAP` in `relune-layout` force-directed packing.
const MIN_FORCE_CONNECTED_NODE_GAP: f32 = 64.0;
/// Layout uses `f32` positions; allow sub-pixel drift vs the ideal gap target.
const FORCE_GAP_EPS: f32 = 0.05;

fn export_layout_fixture(
    fixture_name: &str,
    direction: LayoutDirection,
    edge_style: RouteStyle,
) -> relune_layout::PositionedGraph {
    export_layout_request(ExportRequest {
        input: InputSource::sql_file(sql_fixture_path(fixture_name)),
        format: ExportFormat::LayoutJson,
        layout: LayoutSpec {
            direction,
            edge_style,
            ..Default::default()
        },
        ..Default::default()
    })
}

fn export_layout_sql(
    sql: &str,
    direction: LayoutDirection,
    edge_style: RouteStyle,
) -> relune_layout::PositionedGraph {
    export_layout_request(
        ExportRequest::from_sql(sql)
            .with_format(ExportFormat::LayoutJson)
            .with_layout(LayoutSpec {
                direction,
                edge_style,
                ..Default::default()
            }),
    )
}

fn export_layout_request(request: ExportRequest) -> relune_layout::PositionedGraph {
    let input_label = match &request.input {
        InputSource::SqlText { .. } => "sql_text".to_string(),
        InputSource::SqlFile { path, .. } => path.display().to_string(),
        InputSource::SchemaJson { .. } => "schema_json".to_string(),
        InputSource::SchemaJsonFile { path } => path.display().to_string(),
        #[cfg(feature = "introspect")]
        InputSource::DbUrl { url } => url.clone(),
    };

    let direction = request.layout.direction;
    let edge_style = request.layout.edge_style;

    let result = export(request).unwrap_or_else(|err| {
        panic!(
            "failed to export layout-json for {input_label} ({direction:?}, {edge_style:?}): {err}"
        )
    });

    parse_layout_json(&result.content)
}

fn rects_overlap(left: (f32, f32, f32, f32), right: (f32, f32, f32, f32)) -> bool {
    left.0 < right.0 + right.2
        && left.0 + left.2 > right.0
        && left.1 < right.1 + right.3
        && left.1 + left.3 > right.1
}

fn rect_axis_gap(left: (f32, f32, f32, f32), right: (f32, f32, f32, f32)) -> (f32, f32) {
    let gap_x = if left.0 + left.2 <= right.0 {
        right.0 - (left.0 + left.2)
    } else if right.0 + right.2 <= left.0 {
        left.0 - (right.0 + right.2)
    } else {
        -((left.0 + left.2).min(right.0 + right.2) - left.0.max(right.0))
    };

    let gap_y = if left.1 + left.3 <= right.1 {
        right.1 - (left.1 + left.3)
    } else if right.1 + right.3 <= left.1 {
        left.1 - (right.1 + right.3)
    } else {
        -((left.1 + left.3).min(right.1 + right.3) - left.1.max(right.1))
    };

    (gap_x, gap_y)
}

fn fixture_slug(fixture_name: &str) -> &str {
    fixture_name.strip_suffix(".sql").unwrap_or(fixture_name)
}

fn snapshot_value_for_fixture_direction(
    fixture_name: &str,
    direction: LayoutDirection,
    direction_name: &str,
) -> serde_json::Value {
    let styles = EDGE_STYLES
        .iter()
        .map(|(edge_style, edge_style_name)| {
            let graph = export_layout_fixture(fixture_name, direction, *edge_style);
            (
                (*edge_style_name).to_string(),
                compact_layout_snapshot(&graph),
            )
        })
        .collect::<serde_json::Map<_, _>>();

    serde_json::json!({
        "fixture": fixture_name,
        "direction": direction_name,
        "styles": styles,
    })
}

fn layout_snapshot_without_route_style(
    graph: &relune_layout::PositionedGraph,
) -> serde_json::Value {
    let mut snapshot = compact_layout_snapshot(graph);
    if let Some(edges) = snapshot["edges"].as_array_mut() {
        for edge in edges {
            let edge = edge
                .as_object_mut()
                .expect("compact layout snapshot edge should be an object");
            edge.remove("style");
        }
    }
    snapshot
}

fn assert_fixture_direction_snapshots(fixture_name: &str) {
    for (direction, direction_name) in DIRECTIONS {
        let snapshot_name = format!(
            "layout_regression__{}__{}",
            fixture_slug(fixture_name),
            direction_name,
        );
        let snapshot =
            snapshot_value_for_fixture_direction(fixture_name, *direction, direction_name);
        insta::assert_json_snapshot!(snapshot_name, snapshot);
    }
}

macro_rules! layout_regression_matrix_test {
    ($test_name:ident, $fixture_name:literal) => {
        #[test]
        fn $test_name() {
            assert_fixture_direction_snapshots($fixture_name);
        }
    };
}

layout_regression_matrix_test!(layout_regression_simple_blog, "simple_blog.sql");
layout_regression_matrix_test!(layout_regression_join_heavy, "join_heavy.sql");
layout_regression_matrix_test!(layout_regression_cyclic_fk, "cyclic_fk.sql");
layout_regression_matrix_test!(layout_regression_multi_schema, "multi_schema.sql");
layout_regression_matrix_test!(layout_regression_ecommerce, "ecommerce.sql");

#[test]
fn layout_regression_fixture_list_stays_in_sync() {
    assert_eq!(
        layout_regression_fixture_names(),
        &[
            "simple_blog.sql",
            "join_heavy.sql",
            "cyclic_fk.sql",
            "multi_schema.sql",
            "ecommerce.sql",
        ],
    );
}

#[test]
fn layout_geometry_and_directional_invariants_hold_for_linear_schema_matrix() {
    let sql = r"
        CREATE TABLE users (
            id SERIAL PRIMARY KEY
        );
        CREATE TABLE posts (
            id SERIAL PRIMARY KEY,
            user_id INTEGER NOT NULL REFERENCES users(id)
        );
        CREATE TABLE comments (
            id SERIAL PRIMARY KEY,
            post_id INTEGER NOT NULL REFERENCES posts(id)
        );
    ";

    for (direction, _) in DIRECTIONS {
        for (edge_style, _) in EDGE_STYLES {
            let graph = export_layout_sql(sql, *direction, *edge_style);
            assert_layout_geometry(&graph);
            assert_directional_layout_invariants(&graph, *direction);
        }
    }
}

#[test]
fn layout_parallel_edge_spacing_holds_for_matrix() {
    let sql = r"
        CREATE TABLE users (
            id SERIAL PRIMARY KEY
        );
        CREATE TABLE posts (
            id SERIAL PRIMARY KEY,
            author_id INTEGER NOT NULL REFERENCES users(id),
            reviewer_id INTEGER NOT NULL REFERENCES users(id)
        );
    ";

    for (direction, _) in DIRECTIONS {
        for (edge_style, _) in EDGE_STYLES {
            let graph = export_layout_sql(sql, *direction, *edge_style);
            assert_layout_geometry(&graph);
            assert_directional_layout_invariants(&graph, *direction);
        }
    }
}

fn assert_force_directed_ecommerce_layout(direction: LayoutDirection, grouping: GroupingStrategy) {
    let graph = export_layout_request(ExportRequest {
        input: InputSource::sql_file(sql_fixture_path("ecommerce.sql")),
        format: ExportFormat::LayoutJson,
        grouping: GroupingSpec { strategy: grouping },
        layout: LayoutSpec {
            algorithm: LayoutAlgorithm::ForceDirected,
            direction,
            ..Default::default()
        },
        ..Default::default()
    });

    for (index, node) in graph.nodes.iter().enumerate() {
        for other in graph.nodes.iter().skip(index + 1) {
            assert!(
                !rects_overlap(
                    (node.x, node.y, node.width, node.height),
                    (other.x, other.y, other.width, other.height),
                ),
                "force-directed ecommerce nodes overlap: {} and {}",
                node.id,
                other.id
            );
        }
    }

    if !graph.groups.is_empty() {
        for (group_idx, group) in graph.groups.iter().enumerate() {
            for node in &graph.nodes {
                if node.group_index == Some(group_idx) {
                    continue;
                }
                assert!(
                    !rects_overlap(
                        (group.x, group.y, group.width, group.height),
                        (node.x, node.y, node.width, node.height),
                    ),
                    "force-directed ecommerce group {} overlaps node {}",
                    group.label,
                    node.id
                );
            }
        }
    }

    let orders = graph
        .nodes
        .iter()
        .find(|node| node.id == "orders")
        .expect("orders node");
    let order_items = graph
        .nodes
        .iter()
        .find(|node| node.id == "order_items")
        .expect("order_items node");
    let (orders_gap_x, orders_gap_y) = rect_axis_gap(
        (orders.x, orders.y, orders.width, orders.height),
        (
            order_items.x,
            order_items.y,
            order_items.width,
            order_items.height,
        ),
    );
    assert!(
        orders_gap_y + FORCE_GAP_EPS >= MIN_FORCE_CONNECTED_NODE_GAP
            && orders_gap_x + FORCE_GAP_EPS >= MIN_FORCE_CONNECTED_NODE_GAP,
        "orders and order_items are too close: gap_x={orders_gap_x}, gap_y={orders_gap_y}"
    );

    let customers = graph
        .nodes
        .iter()
        .find(|node| node.id == "customers")
        .expect("customers node");
    let addresses = graph
        .nodes
        .iter()
        .find(|node| node.id == "addresses")
        .expect("addresses node");
    let (customers_gap_x, customers_gap_y) = rect_axis_gap(
        (customers.x, customers.y, customers.width, customers.height),
        (addresses.x, addresses.y, addresses.width, addresses.height),
    );
    assert!(
        customers_gap_y + FORCE_GAP_EPS >= MIN_FORCE_CONNECTED_NODE_GAP
            && customers_gap_x + FORCE_GAP_EPS >= MIN_FORCE_CONNECTED_NODE_GAP,
        "customers and addresses are too close: gap_x={customers_gap_x}, gap_y={customers_gap_y}"
    );

    assert_directional_layout_invariants(&graph, direction);
}

#[test]
fn layout_force_directed_prefix_grouping_ecommerce_avoids_overlaps() {
    assert_force_directed_ecommerce_layout(
        LayoutDirection::TopToBottom,
        GroupingStrategy::ByPrefix,
    );
}

#[test]
fn layout_force_directed_prefix_grouping_ecommerce_left_to_right_preserves_edge_margin() {
    assert_force_directed_ecommerce_layout(
        LayoutDirection::LeftToRight,
        GroupingStrategy::ByPrefix,
    );
}

#[test]
fn layout_force_directed_ecommerce_none_grouping_preserves_connected_gaps() {
    assert_force_directed_ecommerce_layout(LayoutDirection::TopToBottom, GroupingStrategy::None);
    assert_force_directed_ecommerce_layout(LayoutDirection::LeftToRight, GroupingStrategy::None);
}

#[test]
fn layout_force_directed_ecommerce_by_schema_preserves_connected_gaps() {
    assert_force_directed_ecommerce_layout(
        LayoutDirection::TopToBottom,
        GroupingStrategy::BySchema,
    );
    assert_force_directed_ecommerce_layout(
        LayoutDirection::LeftToRight,
        GroupingStrategy::BySchema,
    );
}

#[test]
fn layout_force_directed_directional_invariants_hold_for_linear_schema_matrix() {
    let sql = r"
        CREATE TABLE users (
            id SERIAL PRIMARY KEY
        );
        CREATE TABLE posts (
            id SERIAL PRIMARY KEY,
            user_id INTEGER NOT NULL REFERENCES users(id)
        );
        CREATE TABLE comments (
            id SERIAL PRIMARY KEY,
            post_id INTEGER NOT NULL REFERENCES posts(id)
        );
    ";

    for (direction, _) in DIRECTIONS {
        for (edge_style, _) in EDGE_STYLES {
            let graph = export_layout_request(
                ExportRequest::from_sql(sql)
                    .with_format(ExportFormat::LayoutJson)
                    .with_layout(LayoutSpec {
                        algorithm: LayoutAlgorithm::ForceDirected,
                        direction: *direction,
                        edge_style: *edge_style,
                        ..Default::default()
                    }),
            );
            assert_layout_geometry(&graph);
            assert_directional_layout_invariants(&graph, *direction);
        }
    }
}

#[test]
fn layout_route_backbone_is_consistent_across_edge_styles() {
    let fixtures = layout_regression_fixture_names();

    for fixture_name in fixtures {
        for (direction, _) in DIRECTIONS {
            // Orthogonal and Curved share the same backbone control points;
            // Straight uses direct lines (no control points) so it is
            // intentionally different.
            let baseline = export_layout_fixture(fixture_name, *direction, RouteStyle::Orthogonal);
            let baseline = layout_snapshot_without_route_style(&baseline);

            let curved = export_layout_fixture(fixture_name, *direction, RouteStyle::Curved);
            let curved = layout_snapshot_without_route_style(&curved);
            assert_eq!(
                baseline, curved,
                "route backbone changed for fixture {fixture_name} in direction {direction:?} when rendering as Curved"
            );
        }
    }
}

#[test]
fn layout_self_loop_geometry_holds_for_matrix() {
    let sql = r"
        CREATE TABLE employees (
            id SERIAL PRIMARY KEY,
            manager_id INTEGER REFERENCES employees(id)
        );
    ";

    for (direction, _) in DIRECTIONS {
        for (edge_style, _) in EDGE_STYLES {
            let graph = export_layout_sql(sql, *direction, *edge_style);
            assert_layout_geometry(&graph);
        }
    }
}

#[test]
fn layout_geometry_holds_for_top_to_bottom_fixture_matrix() {
    for fixture_name in layout_regression_fixture_names() {
        for (edge_style, _) in EDGE_STYLES {
            let graph =
                export_layout_fixture(fixture_name, LayoutDirection::TopToBottom, *edge_style);
            assert_layout_geometry(&graph);
        }
    }
}

#[test]
fn layout_directional_invariants_hold_for_same_rank_and_reverse_cases() {
    let sql = r"
        CREATE TABLE accounts (
            id SERIAL PRIMARY KEY
        );
        CREATE TABLE projects (
            id SERIAL PRIMARY KEY,
            owner_id INTEGER REFERENCES accounts(id)
        );
        CREATE TABLE audits (
            id SERIAL PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            account_id INTEGER REFERENCES accounts(id)
        );
    ";

    for (direction, _) in &DIRECTIONS[..2] {
        for (edge_style, _) in EDGE_STYLES {
            let graph = export_layout_sql(sql, *direction, *edge_style);
            assert_layout_geometry(&graph);
            assert_directional_layout_invariants(&graph, *direction);
        }
    }
}

#[test]
fn layout_json_exports_include_routing_debug_metadata() {
    let graph = export_layout_fixture(
        "join_heavy.sql",
        LayoutDirection::TopToBottom,
        RouteStyle::Orthogonal,
    );

    assert_eq!(
        graph
            .routing_debug
            .as_ref()
            .expect("graph routing debug should be present")
            .non_self_loop_detour_activations,
        0
    );
    assert!(
        graph
            .edges
            .iter()
            .all(|edge| edge.routing_debug.as_ref().is_some_and(|debug| {
                if edge.is_self_loop {
                    debug.self_loop_radius_offset.is_some()
                } else {
                    debug.source_side.is_some()
                        && debug.target_side.is_some()
                        && debug.source_slot_index.is_some()
                        && debug.source_slot_count.is_some()
                        && debug.target_slot_index.is_some()
                        && debug.target_slot_count.is_some()
                }
            }))
    );
}
