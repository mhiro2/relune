//! Test utilities for relune.
//!
//! Provides builder patterns for constructing test fixtures and
//! utility functions for test assertions.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use relune_core::{
    Column, ColumnId, Enum, ForeignKey, Index, LayoutDirection, ReferentialAction, Schema, Table,
    TableId, View,
};
use relune_layout::route::LABEL_HALF_H;
use relune_layout::{PositionedEdge, PositionedGraph, PositionedNode};
use relune_render_svg::{EdgeRenderOptions, edge::rendered_path_points};

/// Normalizes SVG content by trimming whitespace and removing empty lines.
pub fn normalize_svg(svg: &str) -> String {
    svg.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Returns the workspace root for repository fixtures.
#[must_use]
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root should be resolvable from relune-testkit")
        .to_path_buf()
}

/// Returns the path to a SQL fixture in `fixtures/sql`.
#[must_use]
pub fn sql_fixture_path(name: &str) -> PathBuf {
    workspace_root().join("fixtures").join("sql").join(name)
}

/// Loads a SQL fixture from `fixtures/sql`.
#[must_use]
pub fn read_sql_fixture(name: &str) -> String {
    std::fs::read_to_string(sql_fixture_path(name))
        .unwrap_or_else(|err| panic!("failed to read SQL fixture {name}: {err}"))
}

/// Returns the path to a config fixture in `fixtures/config`.
#[must_use]
pub fn config_fixture_path(name: &str) -> PathBuf {
    workspace_root().join("fixtures").join("config").join(name)
}

/// Normalizes absolute workspace paths for stable snapshots.
#[must_use]
pub fn normalize_workspace_paths(text: &str) -> String {
    let root = workspace_root().display().to_string();
    text.replace(&root, "$WORKSPACE")
}

/// Fixture names used by the layout regression matrix.
pub const LAYOUT_REGRESSION_FIXTURES: &[&str] = &[
    "simple_blog.sql",
    "join_heavy.sql",
    "cyclic_fk.sql",
    "multi_schema.sql",
    "ecommerce.sql",
];

const SNAPSHOT_PRECISION_SCALE: f32 = 1_000.0;
const ENDPOINT_SIDE_TOLERANCE: f32 = 6.0;
const ROUTE_INTERSECTION_SAMPLES: u32 = 96;
const MONOTONICITY_SAMPLES: u32 = 48;
const MONOTONICITY_TOLERANCE: f32 = 6.0;
const SIDE_POLICY_MARGIN: f32 = 24.0;
const RENDERED_CURVE_SAMPLES: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndpointSide {
    North,
    South,
    East,
    West,
}

impl EndpointSide {
    const fn as_str(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::South => "south",
            Self::East => "east",
            Self::West => "west",
        }
    }
}

/// Returns the fixture names for layout regression tests.
#[must_use]
pub const fn layout_regression_fixture_names() -> &'static [&'static str] {
    LAYOUT_REGRESSION_FIXTURES
}

/// Parses `layout-json` output into a positioned graph.
#[must_use]
pub fn parse_layout_json(layout_json: &str) -> PositionedGraph {
    serde_json::from_str(layout_json)
        .unwrap_or_else(|err| panic!("failed to parse layout json: {err}"))
}

/// Builds a compact JSON view of a positioned graph for stable snapshots.
#[must_use]
pub fn compact_layout_snapshot(graph: &PositionedGraph) -> serde_json::Value {
    let mut nodes = graph
        .nodes
        .iter()
        .map(|node| {
            serde_json::json!({
                "id": node.id,
                "kind": format!("{:?}", node.kind),
                "x": round_f32(node.x),
                "y": round_f32(node.y),
                "width": round_f32(node.width),
                "height": round_f32(node.height),
                "column_count": node.columns.len(),
                "has_self_loop": node.has_self_loop,
                "group_index": node.group_index,
            })
        })
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        left["id"]
            .as_str()
            .expect("snapshot node id")
            .cmp(right["id"].as_str().expect("snapshot node id"))
    });

    let node_index = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();

    let mut edges = graph
        .edges
        .iter()
        .map(|edge| {
            let source = node_index
                .get(edge.from.as_str())
                .unwrap_or_else(|| panic!("missing source node {}", edge.from));
            let target = node_index
                .get(edge.to.as_str())
                .unwrap_or_else(|| panic!("missing target node {}", edge.to));

            serde_json::json!({
                "from": edge.from,
                "to": edge.to,
                "label": edge.label,
                "style": format!("{:?}", edge.route.style),
                "is_self_loop": edge.is_self_loop,
                "nullable": edge.nullable,
                "target_cardinality": format!("{:?}", edge.target_cardinality),
                "from_columns": edge.from_columns,
                "to_columns": edge.to_columns,
                "source_side": infer_endpoint_side(source, (edge.route.x1, edge.route.y1)).map(EndpointSide::as_str),
                "target_side": infer_endpoint_side(target, (edge.route.x2, edge.route.y2)).map(EndpointSide::as_str),
                "start": round_point((edge.route.x1, edge.route.y1)),
                "end": round_point((edge.route.x2, edge.route.y2)),
                "control_points": edge.route.control_points.iter().copied().map(round_point).collect::<Vec<_>>(),
                "route_label_position": round_point(edge.route.label_position),
                "label_x": round_f32(edge.label_x),
                "label_y": round_f32(edge.label_y),
            })
        })
        .collect::<Vec<_>>();
    edges.sort_by_key(edge_snapshot_sort_key);

    serde_json::json!({
        "summary": {
            "width": round_f32(graph.width),
            "height": round_f32(graph.height),
            "node_count": graph.nodes.len(),
            "edge_count": graph.edges.len(),
        },
        "nodes": nodes,
        "edges": edges,
    })
}

/// Asserts basic geometry invariants for a positioned graph.
pub fn assert_layout_geometry(graph: &PositionedGraph) {
    let node_index = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();

    for edge in &graph.edges {
        let source = node_index
            .get(edge.from.as_str())
            .unwrap_or_else(|| panic!("missing source node {}", edge.from));
        let target = node_index
            .get(edge.to.as_str())
            .unwrap_or_else(|| panic!("missing target node {}", edge.to));

        assert!(
            edge.label_x.is_finite(),
            "edge {} -> {} has non-finite label_x {}",
            edge.from,
            edge.to,
            edge.label_x
        );
        assert!(
            edge.label_y.is_finite(),
            "edge {} -> {} has non-finite label_y {}",
            edge.from,
            edge.to,
            edge.label_y
        );

        assert_endpoint_on_perimeter(source, (edge.route.x1, edge.route.y1), "source", edge);
        assert_endpoint_on_perimeter(target, (edge.route.x2, edge.route.y2), "target", edge);

        // Straight-style routes are direct lines that may naturally cross
        // through intermediate nodes; only check orthogonal/curved routes.
        if edge.route.style != relune_core::layout::RouteStyle::Straight {
            for node in &graph.nodes {
                if node.id == edge.from || node.id == edge.to {
                    continue;
                }
                assert_route_stays_outside_node(edge, node, "non-endpoint");
            }
        }

        if edge.is_self_loop {
            assert_route_stays_outside_node(edge, source, "self-loop");
        }
    }
}

/// Asserts routing invariants that should survive the port-assignment refactor.
pub fn assert_directional_layout_invariants(graph: &PositionedGraph, direction: LayoutDirection) {
    let node_index = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();

    assert_side_policy(graph, &node_index, direction);
    assert_parallel_edge_spacing(graph);
    assert_route_monotonicity(graph, direction);
}

/// Builder for constructing [`Schema`] test fixtures.
#[derive(Default)]
pub struct SchemaBuilder {
    tables: Vec<Table>,
    views: Vec<View>,
    enums: Vec<Enum>,
    next_table_id: u64,
}

impl SchemaBuilder {
    /// Creates a new empty schema builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_table_id: 1,
            ..Default::default()
        }
    }

    /// Adds a table using a [`TableBuilder`].
    #[must_use]
    pub fn table(mut self, name: &str, f: impl FnOnce(TableBuilder) -> TableBuilder) -> Self {
        let id = TableId(self.next_table_id);
        self.next_table_id += 1;
        let builder = f(TableBuilder::new(id, name));
        self.tables.push(builder.build());
        self
    }

    /// Adds a view.
    #[must_use]
    pub fn view(mut self, name: &str, columns: Vec<Column>, definition: Option<&str>) -> Self {
        self.views.push(View {
            id: name.to_string(),
            schema_name: None,
            name: name.to_string(),
            columns,
            definition: definition.map(String::from),
        });
        self
    }

    /// Adds an enum type.
    #[must_use]
    pub fn enum_type(mut self, name: &str, values: &[&str]) -> Self {
        self.enums.push(Enum {
            id: name.to_string(),
            schema_name: None,
            name: name.to_string(),
            values: values.iter().map(|v| (*v).to_string()).collect(),
        });
        self
    }

    /// Builds the schema.
    #[must_use]
    pub fn build(self) -> Schema {
        validate_foreign_key_targets(&self.tables);
        Schema {
            tables: self.tables,
            views: self.views,
            enums: self.enums,
        }
    }
}

fn validate_foreign_key_targets(tables: &[Table]) {
    let qualified_names: HashSet<&str> = tables
        .iter()
        .map(|table| table.stable_id.as_str())
        .collect();
    let unqualified_names: HashSet<&str> = tables.iter().map(|table| table.name.as_str()).collect();

    for table in tables {
        for foreign_key in &table.foreign_keys {
            let target_exists = if foreign_key.to_table.contains('.') {
                qualified_names.contains(foreign_key.to_table.as_str())
            } else {
                unqualified_names.contains(foreign_key.to_table.as_str())
            };

            assert!(
                target_exists,
                "foreign key from '{}' references unknown table '{}'",
                table.name, foreign_key.to_table
            );
        }
    }
}

/// Builder for constructing [`Table`] test fixtures.
pub struct TableBuilder {
    table: Table,
    next_column_id: u64,
}

impl TableBuilder {
    /// Creates a new table builder.
    #[must_use]
    pub fn new(id: TableId, name: &str) -> Self {
        Self {
            table: Table {
                id,
                stable_id: name.to_string(),
                schema_name: None,
                name: name.to_string(),
                columns: Vec::new(),
                foreign_keys: Vec::new(),
                indexes: Vec::new(),
                primary_key_name: None,
                comment: None,
            },
            next_column_id: 1,
        }
    }

    /// Adds a column with the given name and data type.
    #[must_use]
    pub fn column(mut self, name: &str, data_type: &str) -> Self {
        self.table.columns.push(Column {
            id: ColumnId(self.next_column_id),
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: true,
            is_primary_key: false,
            comment: None,
        });
        self.next_column_id += 1;
        self
    }

    /// Adds a primary key column.
    #[must_use]
    pub fn pk(mut self, name: &str, data_type: &str) -> Self {
        self.table.columns.push(Column {
            id: ColumnId(self.next_column_id),
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: false,
            is_primary_key: true,
            comment: None,
        });
        self.next_column_id += 1;
        self
    }

    /// Adds a foreign key to another table.
    #[must_use]
    pub fn fk(mut self, to_table: &str, from_columns: &[&str], to_columns: &[&str]) -> Self {
        self.table.foreign_keys.push(ForeignKey {
            name: None,
            from_columns: from_columns.iter().map(|c| (*c).to_string()).collect(),
            to_schema: None,
            to_table: to_table.to_string(),
            to_columns: to_columns.iter().map(|c| (*c).to_string()).collect(),
            on_delete: ReferentialAction::NoAction,
            on_update: ReferentialAction::NoAction,
        });
        self
    }

    /// Adds a named foreign key.
    #[must_use]
    pub fn named_fk(
        mut self,
        name: &str,
        to_table: &str,
        from_columns: &[&str],
        to_columns: &[&str],
    ) -> Self {
        self.table.foreign_keys.push(ForeignKey {
            name: Some(name.to_string()),
            from_columns: from_columns.iter().map(|c| (*c).to_string()).collect(),
            to_schema: None,
            to_table: to_table.to_string(),
            to_columns: to_columns.iter().map(|c| (*c).to_string()).collect(),
            on_delete: ReferentialAction::NoAction,
            on_update: ReferentialAction::NoAction,
        });
        self
    }

    /// Adds an index.
    #[must_use]
    pub fn index(mut self, name: Option<&str>, columns: &[&str], is_unique: bool) -> Self {
        self.table.indexes.push(Index {
            name: name.map(String::from),
            columns: columns.iter().map(|c| (*c).to_string()).collect(),
            is_unique,
        });
        self
    }

    /// Sets the schema name.
    #[must_use]
    pub fn schema(mut self, schema_name: &str) -> Self {
        self.table.schema_name = Some(schema_name.to_string());
        let qualified = format!("{}.{}", schema_name, self.table.name);
        self.table.stable_id = qualified;
        self
    }

    /// Builds the table.
    #[must_use]
    pub fn build(self) -> Table {
        self.table
    }
}

/// Creates a simple blog schema for testing.
///
/// Contains: users, posts, comments tables with FK relationships.
#[must_use]
pub fn blog_schema() -> Schema {
    SchemaBuilder::new()
        .table("users", |t| {
            t.pk("id", "integer")
                .column("name", "text")
                .column("email", "text")
        })
        .table("posts", |t| {
            t.pk("id", "integer")
                .column("title", "text")
                .column("body", "text")
                .column("author_id", "integer")
                .fk("users", &["author_id"], &["id"])
        })
        .table("comments", |t| {
            t.pk("id", "integer")
                .column("body", "text")
                .column("post_id", "integer")
                .column("user_id", "integer")
                .fk("posts", &["post_id"], &["id"])
                .fk("users", &["user_id"], &["id"])
        })
        .build()
}

/// Creates a minimal schema with a single table.
#[must_use]
pub fn single_table_schema(name: &str) -> Schema {
    SchemaBuilder::new()
        .table(name, |t| t.pk("id", "integer"))
        .build()
}

fn round_f32(value: f32) -> f32 {
    (value * SNAPSHOT_PRECISION_SCALE).round() / SNAPSHOT_PRECISION_SCALE
}

fn round_point(point: (f32, f32)) -> [f32; 2] {
    [round_f32(point.0), round_f32(point.1)]
}

fn edge_snapshot_sort_key(value: &serde_json::Value) -> String {
    format!(
        "{}|{}|{}|{}",
        value["from"].as_str().expect("snapshot edge from"),
        value["to"].as_str().expect("snapshot edge to"),
        value["label"].as_str().expect("snapshot edge label"),
        value["style"].as_str().expect("snapshot edge style"),
    )
}

const fn node_center(node: &PositionedNode) -> (f32, f32) {
    (
        node.width.mul_add(0.5, node.x),
        node.height.mul_add(0.5, node.y),
    )
}

fn infer_endpoint_side(node: &PositionedNode, point: (f32, f32)) -> Option<EndpointSide> {
    let distances = [
        (
            EndpointSide::North,
            (point.1 - node.y).abs(),
            point.0 >= node.x - ENDPOINT_SIDE_TOLERANCE
                && point.0 <= node.x + node.width + ENDPOINT_SIDE_TOLERANCE,
        ),
        (
            EndpointSide::South,
            (point.1 - (node.y + node.height)).abs(),
            point.0 >= node.x - ENDPOINT_SIDE_TOLERANCE
                && point.0 <= node.x + node.width + ENDPOINT_SIDE_TOLERANCE,
        ),
        (
            EndpointSide::West,
            (point.0 - node.x).abs(),
            point.1 >= node.y - ENDPOINT_SIDE_TOLERANCE
                && point.1 <= node.y + node.height + ENDPOINT_SIDE_TOLERANCE,
        ),
        (
            EndpointSide::East,
            (point.0 - (node.x + node.width)).abs(),
            point.1 >= node.y - ENDPOINT_SIDE_TOLERANCE
                && point.1 <= node.y + node.height + ENDPOINT_SIDE_TOLERANCE,
        ),
    ];

    distances
        .into_iter()
        .filter(|(_, distance, in_span)| *distance <= ENDPOINT_SIDE_TOLERANCE && *in_span)
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(side, _, _)| side)
}

fn assert_endpoint_on_perimeter(
    node: &PositionedNode,
    point: (f32, f32),
    role: &str,
    edge: &PositionedEdge,
) {
    let side = infer_endpoint_side(node, point).unwrap_or_else(|| {
        panic!(
            "{role} endpoint for edge {} -> {} is not on node {} perimeter: {:?}",
            edge.from,
            edge.to,
            node.id,
            round_point(point),
        )
    });

    let outside_inset = point.0 >= node.x - ENDPOINT_SIDE_TOLERANCE
        && point.0 <= node.x + node.width + ENDPOINT_SIDE_TOLERANCE
        && point.1 >= node.y - ENDPOINT_SIDE_TOLERANCE
        && point.1 <= node.y + node.height + ENDPOINT_SIDE_TOLERANCE;
    assert!(
        outside_inset,
        "{role} endpoint for edge {} -> {} on side {} is too far from node {} perimeter: {:?}",
        edge.from,
        edge.to,
        side.as_str(),
        node.id,
        round_point(point),
    );
}

fn assert_route_stays_outside_node(edge: &PositionedEdge, node: &PositionedNode, context: &str) {
    for sample in sampled_route_points(&edge.route, ROUTE_INTERSECTION_SAMPLES) {
        assert!(
            !point_inside_node(sample, node),
            "{context} route {} -> {} intersects node {} at {:?}",
            edge.from,
            edge.to,
            node.id,
            round_point(sample),
        );
    }
}

fn sampled_route_points(route: &relune_core::EdgeRoute, samples: u32) -> Vec<(f32, f32)> {
    let rendered_points = rendered_path_points(
        route,
        EdgeRenderOptions::default().curve_offset,
        RENDERED_CURVE_SAMPLES,
    );
    sample_polyline_points(&rendered_points, samples)
}

fn point_inside_node(point: (f32, f32), node: &PositionedNode) -> bool {
    point.0 > node.x + 0.5
        && point.0 < node.x + node.width - 0.5
        && point.1 > node.y + 0.5
        && point.1 < node.y + node.height - 0.5
}

fn sample_polyline_points(points: &[(f32, f32)], samples: u32) -> Vec<(f32, f32)> {
    if points.len() <= 1 {
        return points.to_vec();
    }

    let sample_count = samples.max(2);
    let total_length = polyline_length(points);
    if total_length <= f32::EPSILON {
        return vec![points[0]; sample_count as usize];
    }

    let mut sampled_points = Vec::with_capacity(sample_count.saturating_sub(1) as usize);
    let mut segment_index = 0usize;
    let mut traveled = 0.0f32;

    for index in 1..sample_count {
        #[allow(clippy::cast_precision_loss)]
        let target = total_length * index as f32 / sample_count as f32;
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

fn assert_side_policy(
    graph: &PositionedGraph,
    node_index: &BTreeMap<&str, &PositionedNode>,
    direction: LayoutDirection,
) {
    let (expected_source, expected_target) = expected_primary_sides(direction);

    for edge in &graph.edges {
        if edge.is_self_loop {
            continue;
        }

        let source = node_index
            .get(edge.from.as_str())
            .unwrap_or_else(|| panic!("missing source node {}", edge.from));
        let target = node_index
            .get(edge.to.as_str())
            .unwrap_or_else(|| panic!("missing target node {}", edge.to));

        let (source_center_x, source_center_y) = node_center(source);
        let (target_center_x, target_center_y) = node_center(target);

        let (primary_delta, cross_delta) = match direction {
            LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => (
                target_center_y - source_center_y,
                target_center_x - source_center_x,
            ),
            LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => (
                target_center_x - source_center_x,
                target_center_y - source_center_y,
            ),
        };

        if primary_delta.abs() < cross_delta.abs() + SIDE_POLICY_MARGIN {
            continue;
        }

        let source_side = infer_endpoint_side(source, (edge.route.x1, edge.route.y1))
            .unwrap_or_else(|| {
                panic!(
                    "failed to infer source side for {} -> {}",
                    edge.from, edge.to
                )
            });
        let target_side = infer_endpoint_side(target, (edge.route.x2, edge.route.y2))
            .unwrap_or_else(|| {
                panic!(
                    "failed to infer target side for {} -> {}",
                    edge.from, edge.to
                )
            });

        assert_eq!(
            source_side,
            expected_source,
            "edge {} -> {} violates source side policy for {:?}: expected {}, got {}",
            edge.from,
            edge.to,
            direction,
            expected_source.as_str(),
            source_side.as_str(),
        );
        assert_eq!(
            target_side,
            expected_target,
            "edge {} -> {} violates target side policy for {:?}: expected {}, got {}",
            edge.from,
            edge.to,
            direction,
            expected_target.as_str(),
            target_side.as_str(),
        );
    }
}

const fn expected_primary_sides(direction: LayoutDirection) -> (EndpointSide, EndpointSide) {
    // FK edges go from child to parent (against the hierarchy direction),
    // so the source exits on the side facing the parent (upstream).
    match direction {
        LayoutDirection::TopToBottom => (EndpointSide::North, EndpointSide::South),
        LayoutDirection::BottomToTop => (EndpointSide::South, EndpointSide::North),
        LayoutDirection::LeftToRight => (EndpointSide::West, EndpointSide::East),
        LayoutDirection::RightToLeft => (EndpointSide::East, EndpointSide::West),
    }
}

fn assert_parallel_edge_spacing(graph: &PositionedGraph) {
    let mut groups: BTreeMap<(&str, &str, bool), Vec<&PositionedEdge>> = BTreeMap::new();
    for edge in &graph.edges {
        groups
            .entry((edge.from.as_str(), edge.to.as_str(), edge.is_self_loop))
            .or_default()
            .push(edge);
    }

    for ((from, to, is_self_loop), edges) in groups {
        if edges.len() < 2 {
            continue;
        }

        for (index, left) in edges.iter().enumerate() {
            for right in edges.iter().skip(index + 1) {
                assert_ne!(
                    route_signature(left),
                    route_signature(right),
                    "parallel edges {from} -> {to} share the same route signature (self_loop={is_self_loop})",
                );
                assert!(
                    !label_boxes_overlap(left, right),
                    "parallel edges {from} -> {to} have overlapping labels: {:?} vs {:?}",
                    round_point((left.label_x, left.label_y)),
                    round_point((right.label_x, right.label_y)),
                );
            }
        }
    }
}

fn route_signature(edge: &PositionedEdge) -> Vec<[f32; 2]> {
    let mut signature = Vec::with_capacity(edge.route.control_points.len() + 2);
    signature.push(round_point((edge.route.x1, edge.route.y1)));
    signature.extend(edge.route.control_points.iter().copied().map(round_point));
    signature.push(round_point((edge.route.x2, edge.route.y2)));
    signature
}

fn label_boxes_overlap(left: &PositionedEdge, right: &PositionedEdge) -> bool {
    let left_half_width = estimate_label_half_width(&left.label);
    let right_half_width = estimate_label_half_width(&right.label);

    left.label_x + left_half_width > right.label_x - right_half_width
        && left.label_x - left_half_width < right.label_x + right_half_width
        && left.label_y + LABEL_HALF_H > right.label_y - LABEL_HALF_H
        && left.label_y - LABEL_HALF_H < right.label_y + LABEL_HALF_H
}

fn estimate_label_half_width(text: &str) -> f32 {
    let char_width: f32 = text
        .chars()
        .map(|ch| if ch.is_ascii() { 6.4 } else { 10.0 })
        .sum();
    (char_width + 18.0) * 0.5
}

fn assert_route_monotonicity(graph: &PositionedGraph, direction: LayoutDirection) {
    for edge in &graph.edges {
        if edge.is_self_loop {
            continue;
        }

        let samples = sampled_route_points(&edge.route, MONOTONICITY_SAMPLES);
        let mut previous = axis_value(samples[0], direction);

        for sample in samples.iter().skip(1) {
            let current = axis_value(*sample, direction);
            // FK edges flow from child to parent (against the hierarchy direction),
            // so coordinates move opposite to the layout direction.
            match direction {
                LayoutDirection::TopToBottom | LayoutDirection::LeftToRight => assert!(
                    current - MONOTONICITY_TOLERANCE <= previous,
                    "edge {} -> {} backtracks for {:?}: {} -> {}",
                    edge.from,
                    edge.to,
                    direction,
                    round_f32(previous),
                    round_f32(current),
                ),
                LayoutDirection::BottomToTop | LayoutDirection::RightToLeft => assert!(
                    current + MONOTONICITY_TOLERANCE >= previous,
                    "edge {} -> {} backtracks for {:?}: {} -> {}",
                    edge.from,
                    edge.to,
                    direction,
                    round_f32(previous),
                    round_f32(current),
                ),
            }
            previous = current;
        }
    }
}

const fn axis_value(point: (f32, f32), direction: LayoutDirection) -> f32 {
    match direction {
        LayoutDirection::TopToBottom | LayoutDirection::BottomToTop => point.1,
        LayoutDirection::LeftToRight | LayoutDirection::RightToLeft => point.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blog_schema() {
        let schema = blog_schema();
        assert_eq!(schema.tables.len(), 3);
        assert_eq!(schema.tables[0].name, "users");
        assert_eq!(schema.tables[1].name, "posts");
        assert_eq!(schema.tables[1].foreign_keys.len(), 1);
        assert_eq!(schema.tables[2].name, "comments");
        assert_eq!(schema.tables[2].foreign_keys.len(), 2);
    }

    #[test]
    fn test_schema_builder() {
        let schema = SchemaBuilder::new()
            .table("t1", |t| t.pk("id", "int"))
            .enum_type("status", &["active", "inactive"])
            .build();
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.enums.len(), 1);
        assert_eq!(schema.enums[0].values, vec!["active", "inactive"]);
    }

    #[test]
    #[should_panic(expected = "references unknown table 'accounts'")]
    fn test_schema_builder_rejects_unknown_foreign_key_target() {
        let _ = SchemaBuilder::new()
            .table("posts", |t| {
                t.pk("id", "int")
                    .column("author_id", "int")
                    .fk("accounts", &["author_id"], &["id"])
            })
            .build();
    }
}
