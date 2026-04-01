//! Fixture-level routing audit snapshots for channel design work.

use std::collections::BTreeMap;

use relune_app::{
    ExportFormat, ExportRequest, InputSource, LayoutDirection, LayoutSpec, OutputFormat,
    RenderOptions, RenderRequest, RenderTheme, RouteStyle, export, render,
};
use relune_core::LayoutAlgorithm;
use relune_layout::{
    LayoutGraphBuilder, PositionedEdge, PositionedGraph, PositionedNode, assign_ranks,
};
use relune_parser_sql::parse_sql_to_schema;
use relune_testkit::{layout_regression_fixture_names, read_sql_fixture, sql_fixture_path};

const SAMPLE_POINTS_PER_EDGE: usize = 96;
const TIGHT_CLEARANCE_THRESHOLD: f32 = 24.0;
const LONG_DETOUR_THRESHOLD: f32 = 1.8;

#[derive(Debug, Clone)]
struct RankedEdgeSummary {
    key: String,
    source_rank: usize,
    target_rank: usize,
    bend_count: usize,
}

#[derive(Debug, Clone)]
struct ClearanceSummary {
    key: String,
    clearance: f32,
}

#[derive(Debug, Clone)]
struct DetourSummary {
    key: String,
    ratio: f32,
}

fn orthogonal_top_to_bottom_layout() -> LayoutSpec {
    LayoutSpec {
        algorithm: LayoutAlgorithm::Hierarchical,
        direction: LayoutDirection::TopToBottom,
        edge_style: RouteStyle::Orthogonal,
        ..Default::default()
    }
}

const fn render_options() -> RenderOptions {
    RenderOptions {
        theme: RenderTheme::Dark,
        show_legend: false,
        show_stats: false,
    }
}

fn export_layout_fixture(fixture_name: &str) -> PositionedGraph {
    let result = export(ExportRequest {
        input: InputSource::sql_file(sql_fixture_path(fixture_name)),
        format: ExportFormat::LayoutJson,
        layout: orthogonal_top_to_bottom_layout(),
        ..Default::default()
    })
    .unwrap_or_else(|err| panic!("failed to export layout-json for {fixture_name}: {err}"));

    serde_json::from_str(&result.content)
        .unwrap_or_else(|err| panic!("failed to parse layout-json for {fixture_name}: {err}"))
}

fn render_fixture(fixture_name: &str, output_format: OutputFormat) -> String {
    let result = render(RenderRequest {
        input: InputSource::sql_file(sql_fixture_path(fixture_name)),
        output_format,
        layout: orthogonal_top_to_bottom_layout(),
        options: render_options(),
        ..Default::default()
    })
    .unwrap_or_else(|err| panic!("failed to render {output_format:?} for {fixture_name}: {err}"));

    result.content
}

fn fixture_graph(fixture_name: &str) -> relune_layout::LayoutGraph {
    let sql = read_sql_fixture(fixture_name);
    let schema = parse_sql_to_schema(&sql)
        .unwrap_or_else(|err| panic!("failed to parse {fixture_name}: {err}"));
    LayoutGraphBuilder::new().build(&schema)
}

fn edge_key(from: &str, to: &str, from_columns: &[String], to_columns: &[String]) -> String {
    format!(
        "{from}[{}] -> {to}[{}]",
        from_columns.join(","),
        to_columns.join(",")
    )
}

fn route_points(edge: &PositionedEdge) -> Vec<(f32, f32)> {
    let mut points = Vec::with_capacity(edge.route.control_points.len() + 2);
    points.push((edge.route.x1, edge.route.y1));
    points.extend(edge.route.control_points.iter().copied());
    points.push((edge.route.x2, edge.route.y2));
    points
}

fn polyline_length(points: &[(f32, f32)]) -> f32 {
    points
        .windows(2)
        .map(|segment| {
            let dx = segment[1].0 - segment[0].0;
            let dy = segment[1].1 - segment[0].1;
            dx.hypot(dy)
        })
        .sum()
}

fn sample_polyline(points: &[(f32, f32)], samples: usize) -> Vec<(f32, f32)> {
    if points.len() <= 1 {
        return points.to_vec();
    }

    let total_length = polyline_length(points);
    if total_length <= f32::EPSILON {
        return vec![points[0]; samples.max(1)];
    }

    let sample_count = u16::try_from(samples.max(2)).expect("sample count should fit into u16");
    let denominator = f32::from(sample_count.saturating_sub(1));
    let targets = (0..sample_count)
        .map(|index| total_length * f32::from(index) / denominator)
        .collect::<Vec<_>>();
    let mut sampled_points = Vec::with_capacity(targets.len());
    let mut segment_index = 0usize;
    let mut traveled = 0.0f32;

    for target in targets {
        while segment_index + 1 < points.len() {
            let start = points[segment_index];
            let end = points[segment_index + 1];
            let segment_length = (end.0 - start.0).hypot(end.1 - start.1);
            if traveled + segment_length >= target || segment_index + 2 == points.len() {
                let offset = (target - traveled).clamp(0.0, segment_length);
                let ratio = if segment_length <= f32::EPSILON {
                    0.0
                } else {
                    offset / segment_length
                };
                sampled_points.push((
                    (end.0 - start.0).mul_add(ratio, start.0),
                    (end.1 - start.1).mul_add(ratio, start.1),
                ));
                break;
            }
            traveled += segment_length;
            segment_index += 1;
        }
    }

    sampled_points
}

fn point_to_rect_clearance(point: (f32, f32), node: &PositionedNode) -> f32 {
    let min_x = node.x;
    let max_x = node.x + node.width;
    let min_y = node.y;
    let max_y = node.y + node.height;
    let dx = if point.0 < min_x {
        min_x - point.0
    } else if point.0 > max_x {
        point.0 - max_x
    } else {
        0.0
    };
    let dy = if point.1 < min_y {
        min_y - point.1
    } else if point.1 > max_y {
        point.1 - max_y
    } else {
        0.0
    };

    dx.hypot(dy)
}

fn edge_min_clearance(edge: &PositionedEdge, graph: &PositionedGraph) -> f32 {
    let points = route_points(edge);
    let samples = sample_polyline(&points, SAMPLE_POINTS_PER_EDGE);

    graph
        .nodes
        .iter()
        .filter(|node| node.id != edge.from && node.id != edge.to)
        .flat_map(|node| {
            samples
                .iter()
                .map(move |&point| point_to_rect_clearance(point, node))
        })
        .fold(f32::INFINITY, f32::min)
}

fn edge_detour_ratio(edge: &PositionedEdge) -> f32 {
    let points = route_points(edge);
    let route_length = polyline_length(&points);
    let direct_distance =
        (edge.route.x2 - edge.route.x1).abs() + (edge.route.y2 - edge.route.y1).abs();
    if direct_distance <= f32::EPSILON {
        1.0
    } else {
        route_length / direct_distance
    }
}

fn ranked_edge_examples(
    graph: &relune_layout::LayoutGraph,
    bend_counts: &BTreeMap<String, usize>,
    predicate: impl Fn(usize, usize) -> bool,
) -> Vec<RankedEdgeSummary> {
    let ranks = assign_ranks(graph, relune_layout::RankAssignmentStrategy::LongestPath).node_rank;

    let mut edges = graph
        .edges
        .iter()
        .filter_map(|edge| {
            let source_rank = *ranks.get(*graph.node_index.get(edge.from.as_str())?)?;
            let target_rank = *ranks.get(*graph.node_index.get(edge.to.as_str())?)?;
            let key = edge_key(&edge.from, &edge.to, &edge.from_columns, &edge.to_columns);
            predicate(source_rank, target_rank).then(|| RankedEdgeSummary {
                bend_count: bend_counts.get(&key).copied().unwrap_or(0),
                key,
                source_rank,
                target_rank,
            })
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| left.key.cmp(&right.key));
    edges
}

fn parallel_group_examples(graph: &relune_layout::LayoutGraph) -> Vec<(String, usize)> {
    let mut groups = BTreeMap::<(String, String), usize>::new();
    for edge in &graph.edges {
        let key = if edge.from <= edge.to {
            (edge.from.clone(), edge.to.clone())
        } else {
            (edge.to.clone(), edge.from.clone())
        };
        *groups.entry(key).or_insert(0) += 1;
    }

    let mut groups = groups
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|((left, right), count)| (format!("{left} <-> {right}"), count))
        .collect::<Vec<_>>();
    groups.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    groups
}

#[allow(clippy::too_many_lines)] // Snapshot assembly keeps the audit report readable in one place.
fn fixture_audit_snapshot(fixture_name: &str) -> serde_json::Value {
    let layout = export_layout_fixture(fixture_name);
    let svg = render_fixture(fixture_name, OutputFormat::Svg);
    let html = render_fixture(fixture_name, OutputFormat::Html);
    let graph = fixture_graph(fixture_name);
    let bend_counts = layout
        .edges
        .iter()
        .map(|edge| {
            (
                edge_key(&edge.from, &edge.to, &edge.from_columns, &edge.to_columns),
                edge.route.control_points.len(),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let same_rank_edges = ranked_edge_examples(&graph, &bend_counts, |source_rank, target_rank| {
        source_rank == target_rank
    });
    let reverse_edges = ranked_edge_examples(&graph, &bend_counts, |source_rank, target_rank| {
        source_rank > target_rank
    });

    let mut clearance_examples = layout
        .edges
        .iter()
        .filter(|edge| !edge.is_self_loop)
        .map(|edge| ClearanceSummary {
            key: edge_key(&edge.from, &edge.to, &edge.from_columns, &edge.to_columns),
            clearance: edge_min_clearance(edge, &layout),
        })
        .collect::<Vec<_>>();
    clearance_examples.sort_by(|left, right| {
        left.clearance
            .total_cmp(&right.clearance)
            .then_with(|| left.key.cmp(&right.key))
    });

    let mut detour_examples = layout
        .edges
        .iter()
        .filter(|edge| !edge.is_self_loop)
        .map(|edge| DetourSummary {
            key: edge_key(&edge.from, &edge.to, &edge.from_columns, &edge.to_columns),
            ratio: edge_detour_ratio(edge),
        })
        .collect::<Vec<_>>();
    detour_examples.sort_by(|left, right| {
        right
            .ratio
            .total_cmp(&left.ratio)
            .then_with(|| left.key.cmp(&right.key))
    });

    let parallel_groups = parallel_group_examples(&graph);
    let svg_edge_groups = svg.matches("class=\"edge").count();
    let svg_node_groups = svg.matches("class=\"table-node").count()
        + svg.matches("class=\"enum-node").count()
        + svg.matches("class=\"view-node").count();

    assert!(
        svg.contains("<svg"),
        "svg output is missing the root element"
    );
    assert!(
        html.contains("<!DOCTYPE html>"),
        "html output is missing the doctype"
    );
    assert!(
        html.contains(r#"id="relune-metadata""#),
        "html output is missing embedded metadata"
    );

    serde_json::json!({
        "fixture": fixture_name,
        "layout": {
            "width": round_f32(layout.width),
            "height": round_f32(layout.height),
            "node_count": layout.nodes.len(),
            "edge_count": layout.edges.len(),
        },
        "surfaces": {
            "svg": {
                "bytes": svg.len(),
                "edge_groups": svg_edge_groups,
                "node_groups": svg_node_groups,
            },
            "html": {
                "bytes": html.len(),
                "contains_embedded_svg": html.contains("<svg"),
                "contains_metadata": html.contains("data-relune-metadata"),
                "contains_viewport": html.contains(r#"id="viewport""#),
            },
        },
        "patterns": {
            "same_rank_edges": {
                "count": same_rank_edges.len(),
                "examples": same_rank_edges.iter().take(3).map(|edge| {
                    format!("{} (rank {} -> {}, bends {})", edge.key, edge.source_rank, edge.target_rank, edge.bend_count)
                }).collect::<Vec<_>>(),
            },
            "reverse_edges": {
                "count": reverse_edges.len(),
                "examples": reverse_edges.iter().take(3).map(|edge| {
                    format!("{} (rank {} -> {}, bends {})", edge.key, edge.source_rank, edge.target_rank, edge.bend_count)
                }).collect::<Vec<_>>(),
            },
            "tight_clearance": {
                "count_under_24px": clearance_examples.iter().filter(|edge| edge.clearance < TIGHT_CLEARANCE_THRESHOLD).count(),
                "worst_examples": clearance_examples.iter().take(3).map(|edge| {
                    format!("{} ({:.1}px)", edge.key, edge.clearance)
                }).collect::<Vec<_>>(),
            },
            "long_detours": {
                "count_over_1_8x": detour_examples.iter().filter(|edge| edge.ratio > LONG_DETOUR_THRESHOLD).count(),
                "worst_examples": detour_examples.iter().take(3).map(|edge| {
                    format!("{} ({:.2}x)", edge.key, edge.ratio)
                }).collect::<Vec<_>>(),
            },
            "parallel_groups": {
                "count": parallel_groups.len(),
                "max_size": parallel_groups.first().map_or(0, |(_, count)| *count),
                "examples": parallel_groups.iter().take(3).map(|(key, count)| {
                    format!("{key} ({count})")
                }).collect::<Vec<_>>(),
            },
        },
    })
}

fn round_f32(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

#[test]
fn fixture_render_audit_stays_in_sync_with_layout_fixture_list() {
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

macro_rules! fixture_render_audit_test {
    ($test_name:ident, $fixture_name:literal) => {
        #[test]
        fn $test_name() {
            let snapshot_name = format!(
                "fixture_render_audit__{}",
                $fixture_name.trim_end_matches(".sql")
            );
            insta::assert_json_snapshot!(snapshot_name, fixture_audit_snapshot($fixture_name));
        }
    };
}

fixture_render_audit_test!(fixture_render_audit_simple_blog, "simple_blog.sql");
fixture_render_audit_test!(fixture_render_audit_join_heavy, "join_heavy.sql");
fixture_render_audit_test!(fixture_render_audit_cyclic_fk, "cyclic_fk.sql");
fixture_render_audit_test!(fixture_render_audit_multi_schema, "multi_schema.sql");
fixture_render_audit_test!(fixture_render_audit_ecommerce, "ecommerce.sql");
