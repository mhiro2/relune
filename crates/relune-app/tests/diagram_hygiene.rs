//! Geometric checks that keep rendered diagrams free of visual glitches.
//!
//! Every SQL fixture is laid out in all four directions, plus grouped by
//! schema and by prefix, and each layout must keep markers whole, lines off
//! cards and group headings, and neighbouring ports apart.

use relune_app::{
    ExportFormat, ExportRequest, GroupingSpec, GroupingStrategy, InputSource, LayoutDirection,
    LayoutSpec, RouteStyle, export,
};
use relune_layout::metrics::{
    GROUP_LABEL_BASELINE, GROUP_LABEL_FONT_SIZE, GROUP_LABEL_INSET, group_label_width,
};
use relune_layout::{PositionedEdge, PositionedGraph, PositionedGroup};
use relune_testkit::sql_fixture_path;

const FIXTURES: &[&str] = &[
    "card_stress.sql",
    "cyclic_fk.sql",
    "ecommerce.sql",
    "join_heavy.sql",
    "multi_schema.sql",
    "mysql_ecommerce.sql",
    "simple_blog.sql",
    "sqlite_blog.sql",
];
const DIRECTIONS: &[LayoutDirection] = &[
    LayoutDirection::TopToBottom,
    LayoutDirection::LeftToRight,
    LayoutDirection::BottomToTop,
    LayoutDirection::RightToLeft,
];
const GROUPINGS: &[GroupingStrategy] = &[
    GroupingStrategy::None,
    GroupingStrategy::BySchema,
    GroupingStrategy::ByPrefix,
];

/// Straight run a Crow's Foot marker needs before the first bend.
const MARKER_STUB: f32 = 30.0;
/// Closest two ports on one card side may sit before their markers merge.
const MIN_PORT_SPACING: f32 = 14.0;

type Point = (f32, f32);

fn layouts() -> impl Iterator<Item = (String, PositionedGraph)> {
    FIXTURES.iter().flat_map(|fixture| {
        DIRECTIONS.iter().flat_map(move |&direction| {
            GROUPINGS.iter().map(move |&strategy| {
                let result = export(ExportRequest {
                    input: InputSource::sql_file(sql_fixture_path(fixture)),
                    format: ExportFormat::LayoutJson,
                    grouping: GroupingSpec { strategy },
                    layout: LayoutSpec {
                        direction,
                        edge_style: RouteStyle::Orthogonal,
                        ..LayoutSpec::default()
                    },
                    ..Default::default()
                })
                .unwrap_or_else(|err| panic!("failed to lay out {fixture}: {err}"));
                let graph = serde_json::from_str(&result.content)
                    .unwrap_or_else(|err| panic!("failed to parse layout of {fixture}: {err}"));
                (format!("{fixture} {direction:?} {strategy:?}"), graph)
            })
        })
    })
}

fn route_points(edge: &PositionedEdge) -> Vec<Point> {
    let route = &edge.route;
    std::iter::once((route.x1, route.y1))
        .chain(route.control_points.iter().copied())
        .chain(std::iter::once((route.x2, route.y2)))
        .collect()
}

fn length(a: Point, b: Point) -> f32 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

/// Whether the axis-aligned segment `start`–`end` passes through `rect`,
/// given as left, top, width, and height.
fn crosses(start: Point, end: Point, rect: (f32, f32, f32, f32)) -> bool {
    let (left, top, width, height) = rect;
    let (min_x, max_x) = (start.0.min(end.0), start.0.max(end.0));
    let (min_y, max_y) = (start.1.min(end.1), start.1.max(end.1));
    max_x > left && min_x < left + width && max_y > top && min_y < top + height
}

fn heading_rect(group: &PositionedGroup) -> (f32, f32, f32, f32) {
    (
        group.x + GROUP_LABEL_INSET,
        group.y + GROUP_LABEL_BASELINE - GROUP_LABEL_FONT_SIZE,
        group_label_width(&group.label),
        GROUP_LABEL_FONT_SIZE + 3.0,
    )
}

#[test]
fn foreign_key_markers_keep_a_straight_run_before_the_first_bend() {
    let mut failures = Vec::new();
    for (name, graph) in layouts() {
        for edge in graph.edges.iter().filter(|edge| !edge.is_self_loop) {
            let points = route_points(edge);
            if points.len() < 3 {
                continue;
            }
            let first = length(points[0], points[1]);
            let last = length(points[points.len() - 2], points[points.len() - 1]);
            if first.min(last) + 0.5 < MARKER_STUB {
                failures.push(format!("{name}: {} {first:.0}/{last:.0}", edge.label));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "short marker runs:\n{}",
        failures.join("\n")
    );
}

#[test]
fn lines_stay_off_group_headings_and_other_cards() {
    let mut failures = Vec::new();
    for (name, graph) in layouts() {
        for edge in &graph.edges {
            let points = route_points(edge);
            for segment in points.windows(2) {
                for group in graph.groups.iter().filter(|group| !group.label.is_empty()) {
                    if crosses(segment[0], segment[1], heading_rect(group)) {
                        failures.push(format!(
                            "{name}: {} crosses heading {}",
                            edge.label, group.label
                        ));
                    }
                }
                for node in &graph.nodes {
                    // Endpoints touch their own cards; only the inside counts.
                    let inside = (
                        node.x + 3.0,
                        node.y + 3.0,
                        node.width - 6.0,
                        node.height - 6.0,
                    );
                    if crosses(segment[0], segment[1], inside) {
                        failures.push(format!("{name}: {} crosses {}", edge.label, node.id));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "lines over headings or cards:\n{}",
        failures.join("\n")
    );
}

#[test]
fn ports_on_one_card_side_keep_their_markers_apart() {
    let mut failures = Vec::new();
    for (name, graph) in layouts() {
        let mut ends: Vec<(&str, Point, &str)> = Vec::new();
        for edge in graph.edges.iter().filter(|edge| !edge.is_self_loop) {
            ends.push((&edge.from, (edge.route.x1, edge.route.y1), &edge.label));
            ends.push((&edge.to, (edge.route.x2, edge.route.y2), &edge.label));
        }
        for (index, &(node, a, label_a)) in ends.iter().enumerate() {
            for &(other, b, label_b) in &ends[index + 1..] {
                let same_side = (a.0 - b.0).abs() < 0.5 || (a.1 - b.1).abs() < 0.5;
                if node == other && same_side && length(a, b) < MIN_PORT_SPACING {
                    failures.push(format!("{name}: {node} {label_a}/{label_b}"));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "crowded ports:\n{}",
        failures.join("\n")
    );
}
