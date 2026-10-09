//! Card density: cards list the columns of their density, are sized for
//! them, and every relationship line meets a listed row or the card's edge.

use relune_app::{ExportFormat, ExportRequest, InputSource, LayoutSpec, export};
use relune_core::{CardDensity, LayoutDirection};
use relune_layout::metrics::{NODE_COLUMN_HEIGHT, NODE_HEADER_HEIGHT, column_row_center};
use relune_layout::{PositionedGraph, PositionedNode};
use relune_testkit::{parse_layout_json, sql_fixture_path};

const FIXTURES: &[&str] = &[
    "ecommerce.sql",
    "join_heavy.sql",
    "cyclic_fk.sql",
    "multi_schema.sql",
    "card_stress.sql",
];

/// Distance from a row center still inside that row.
const ROW_TOLERANCE: f32 = NODE_COLUMN_HEIGHT / 2.0;
/// Slack for the border outset and marker clearance around a card edge.
const EDGE_TOLERANCE: f32 = 6.0;

fn layout(fixture: &str, density: Option<CardDensity>) -> PositionedGraph {
    layout_in(fixture, density, LayoutDirection::default())
}

fn layout_in(
    fixture: &str,
    density: Option<CardDensity>,
    direction: LayoutDirection,
) -> PositionedGraph {
    let result = export(ExportRequest {
        input: InputSource::sql_file(sql_fixture_path(fixture)),
        format: ExportFormat::LayoutJson,
        layout: LayoutSpec {
            density,
            direction,
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap_or_else(|err| panic!("failed to lay out {fixture} at {density:?}: {err}"));
    parse_layout_json(&result.content)
}

fn node<'a>(graph: &'a PositionedGraph, id: &str) -> &'a PositionedNode {
    graph
        .nodes
        .iter()
        .find(|node| node.id == id)
        .unwrap_or_else(|| panic!("missing node {id}"))
}

fn listed_row(node: &PositionedNode, columns: &[String]) -> Option<usize> {
    node.columns
        .iter()
        .position(|column| columns.contains(&column.name))
}

/// An endpoint sits on the card's perimeter.
fn assert_endpoint_on_card_edge(node: &PositionedNode, point: (f32, f32), context: &str) {
    let (left, top) = (node.x, node.y);
    let (right, bottom) = (node.x + node.width, node.y + node.height);
    let within = |value: f32, low: f32, high: f32| {
        value >= low - EDGE_TOLERANCE && value <= high + EDGE_TOLERANCE
    };
    let near = |value: f32, edge: f32| (value - edge).abs() <= EDGE_TOLERANCE;
    let on_vertical_side =
        (near(point.0, left) || near(point.0, right)) && within(point.1, top, bottom);
    let on_horizontal_side =
        (near(point.1, top) || near(point.1, bottom)) && within(point.0, left, right);
    assert!(
        on_vertical_side || on_horizontal_side,
        "{context}: endpoint {point:?} is off the edge of {} ({left}, {top})-({right}, {bottom})",
        node.id,
    );
}

/// A line leaving a card's side meets the row of the column it refers to.
/// Returns whether the endpoint was checked against a row.
fn assert_side_endpoint_meets_row(
    node: &PositionedNode,
    columns: &[String],
    point: (f32, f32),
    context: &str,
) -> bool {
    let on_side = (point.0 - node.x).abs() <= ROW_TOLERANCE
        || (point.0 - (node.x + node.width)).abs() <= ROW_TOLERANCE;
    let Some(row) = listed_row(node, columns) else {
        return false;
    };
    if !on_side || point.1 <= node.y || point.1 >= node.y + node.height {
        return false;
    }
    let row_center = node.y + column_row_center(row);
    assert!(
        (point.1 - row_center).abs() <= ROW_TOLERANCE,
        "{context}: endpoint y {} is not near row {row} of {} (center {row_center})",
        point.1,
        node.id,
    );
    true
}

#[test]
fn every_density_keeps_line_endpoints_on_listed_rows_or_card_edges() {
    let mut row_endpoints = 0;
    for fixture in FIXTURES {
        for direction in [LayoutDirection::TopToBottom, LayoutDirection::LeftToRight] {
            for density in [CardDensity::Overview, CardDensity::Keys, CardDensity::Full] {
                let graph = layout_in(fixture, Some(density), direction);

                // Self-loops leave and re-enter the card on fixed sides.
                for edge in graph.edges.iter().filter(|edge| !edge.is_self_loop) {
                    let context = format!(
                        "{fixture} {direction:?} {density:?} {} -> {}",
                        edge.from, edge.to
                    );
                    let debug = edge.routing_debug.as_ref();
                    let ends = [
                        (
                            &edge.from,
                            &edge.from_columns,
                            (edge.route.x1, edge.route.y1),
                            debug.and_then(|debug| debug.source_slot_count),
                        ),
                        (
                            &edge.to,
                            &edge.to_columns,
                            (edge.route.x2, edge.route.y2),
                            debug.and_then(|debug| debug.target_slot_count),
                        ),
                    ];
                    for (id, columns, point, slot_count) in ends {
                        let card = node(&graph, id);
                        assert_endpoint_on_card_edge(card, point, &context);
                        // Lines sharing a side fan out around their rows.
                        if slot_count == Some(1)
                            && assert_side_endpoint_meets_row(card, columns, point, &context)
                        {
                            row_endpoints += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(row_endpoints > 0, "no endpoint was checked against a row");
}

#[test]
fn overview_cards_are_headers_alone() {
    for fixture in FIXTURES {
        let graph = layout(fixture, Some(CardDensity::Overview));
        for node in &graph.nodes {
            assert!(
                node.columns.is_empty(),
                "{fixture}: {} lists columns",
                node.id
            );
            assert!(
                (node.height - NODE_HEADER_HEIGHT).abs() < f32::EPSILON,
                "{fixture}: {} is {} tall",
                node.id,
                node.height
            );
        }
    }
}

#[test]
fn keys_cards_list_every_column_a_relationship_names() {
    for fixture in FIXTURES {
        let full = layout(fixture, Some(CardDensity::Full));
        let keys = layout(fixture, Some(CardDensity::Keys));

        for edge in &keys.edges {
            for (id, columns) in [
                (&edge.from, &edge.from_columns),
                (&edge.to, &edge.to_columns),
            ] {
                let node = node(&keys, id);
                for column in columns {
                    assert!(
                        node.columns.iter().any(|listed| &listed.name == column),
                        "{fixture}: {id} does not list {column}",
                    );
                }
            }
        }
        for node in &keys.nodes {
            let total = node.columns.len() + node.omitted_columns;
            assert_eq!(
                total,
                self::node(&full, &node.id).columns.len(),
                "{fixture}: {} loses track of its columns",
                node.id
            );
        }
        let area = |graph: &PositionedGraph| graph.width * graph.height;
        assert!(
            area(&keys) <= area(&full),
            "{fixture}: keys density should not grow the diagram"
        );
    }
}

#[test]
fn unset_density_lists_every_column() {
    let full = layout("ecommerce.sql", Some(CardDensity::Full));
    let unset = layout("ecommerce.sql", None);

    assert_eq!(
        serde_json::to_value(&full.nodes).unwrap(),
        serde_json::to_value(&unset.nodes).unwrap()
    );
}
