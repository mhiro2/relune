//! Layout module tests.

use std::collections::BTreeMap;

use super::edge_routing::{
    BYPASS_CHANNEL_LANE_STEP, MIN_LABEL_ROUTE_T, ObstacleRoutingContext, bypass_channel_candidates,
    bypass_channel_lane_count, edge_endpoint_marker_obstacles, edge_route_obstacle_spacing,
    label_rect, obstacle_aware_channel_for_edge, parallel_label_parameter, place_label_on_route,
    rank_axis_bounds, rect_overlaps_any, route_edges, route_edges_with_diagnostics,
    route_obstacle_hit_count,
};
use super::force::{
    FORCE_CONNECTED_NODE_GAP, force_layout_canonical_config, force_pair_axis_gaps,
    resolve_force_overlaps,
};
use super::spacing::{
    COLUMN_FONT_SIZE, build_positioned_node, estimate_node_height, estimate_text_width,
};
use super::*;
use crate::graph::{LayoutEdge, LayoutGraph};
use crate::port::{RegularPortAssignment, column_y_offset_from_center};
use crate::route::{
    AttachmentSide, ChannelAxis, LABEL_HALF_H, Rect, estimate_label_half_width, point_along_route,
    route_points,
};
use relune_core::layout::Cardinality;
use relune_core::{
    Column, ColumnId, ForeignKey, LayoutCompactionSpec, LayoutSpec, ReferentialAction, Table,
    TableId,
};

#[test]
fn edge_route_obstacle_spacing_scales_with_edge_count() {
    assert!((edge_route_obstacle_spacing(16) - 10.0).abs() <= f32::EPSILON);
    assert!((edge_route_obstacle_spacing(128) - 14.0).abs() <= f32::EPSILON);
    assert!((edge_route_obstacle_spacing(256) - 18.0).abs() <= f32::EPSILON);
    assert!((edge_route_obstacle_spacing(512) - 24.0).abs() <= f32::EPSILON);
}

#[test]
fn test_place_label_on_route_avoids_foreign_key_endpoint_markers() {
    let route = EdgeRoute {
        x1: 382.0,
        y1: 123.0,
        x2: 637.2,
        y2: 98.0,
        control_points: vec![(509.6, 123.0), (509.6, 98.0)],
        style: RouteStyle::Orthogonal,
        label_position: (509.6, 110.5),
    };
    let obstacles =
        edge_endpoint_marker_obstacles(&route, EdgeKind::ForeignKey, false, Cardinality::One);
    let label_half_w = estimate_label_half_width("product_id");

    let placed = place_label_on_route(&route, MIN_LABEL_ROUTE_T, &obstacles, 4.0, label_half_w);

    assert!(
        !rect_overlaps_any(
            label_rect(placed.0, placed.1, label_half_w),
            &obstacles,
            4.0
        ),
        "placed label still overlaps endpoint marker clearance: {placed:?}"
    );
    assert!(
        placed.0 > 450.0,
        "label should move away from the source Crow's Foot marker, got {placed:?}"
    );
}

#[test]
fn test_place_label_on_route_avoids_generic_arrow_endpoint_marker() {
    let route = EdgeRoute {
        x1: 100.0,
        y1: 200.0,
        x2: 320.0,
        y2: 200.0,
        control_points: Vec::new(),
        style: RouteStyle::Straight,
        label_position: (210.0, 200.0),
    };
    let obstacles =
        edge_endpoint_marker_obstacles(&route, EdgeKind::ViewDependency, false, Cardinality::One);
    let label_half_w = estimate_label_half_width("depends_on");

    let placed = place_label_on_route(
        &route,
        1.0 - MIN_LABEL_ROUTE_T,
        &obstacles,
        4.0,
        label_half_w,
    );

    assert!(
        !rect_overlaps_any(
            label_rect(placed.0, placed.1, label_half_w),
            &obstacles,
            4.0
        ),
        "placed label still overlaps arrow clearance: {placed:?}"
    );
    assert!(
        placed.0 < 300.0,
        "label should move away from the end arrow marker, got {placed:?}"
    );
}

fn make_test_schema() -> Schema {
    Schema {
        tables: vec![
            Table {
                id: TableId(1),
                stable_id: "users".to_string(),
                schema_name: None,
                name: "users".to_string(),
                columns: vec![
                    Column {
                        id: ColumnId(1),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                        enum_values: None,
                    },
                    Column {
                        id: ColumnId(2),
                        name: "name".to_string(),
                        data_type: "varchar".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                        enum_values: None,
                    },
                ],
                foreign_keys: vec![],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
            Table {
                id: TableId(2),
                stable_id: "posts".to_string(),
                schema_name: None,
                name: "posts".to_string(),
                columns: vec![
                    Column {
                        id: ColumnId(3),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                        enum_values: None,
                    },
                    Column {
                        id: ColumnId(4),
                        name: "user_id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                        enum_values: None,
                    },
                ],
                foreign_keys: vec![ForeignKey {
                    name: Some("fk_posts_user".to_string()),
                    from_columns: vec!["user_id".to_string()],
                    to_schema: None,
                    to_table: "users".to_string(),
                    to_columns: vec!["id".to_string()],
                    on_delete: ReferentialAction::NoAction,
                    on_update: ReferentialAction::NoAction,
                }],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
        ],
        views: vec![],
        enums: vec![],
    }
}

fn single_edge_graph(from: &str, to: &str) -> LayoutGraph {
    let mut node_index = std::collections::BTreeMap::new();
    node_index.insert(from.to_string(), 0usize);
    node_index.insert(to.to_string(), 1usize);

    LayoutGraph {
        nodes: Vec::new(),
        edges: vec![LayoutEdge {
            from: from.to_string(),
            to: to.to_string(),
            name: Some("fk".to_string()),
            from_columns: Vec::new(),
            to_columns: Vec::new(),
            kind: EdgeKind::ForeignKey,
            is_self_loop: false,
            nullable: false,
            target_cardinality: relune_core::layout::Cardinality::One,
            is_collapsed_join: false,
            collapsed_join_table: None,
        }],
        groups: Vec::new(),
        node_index,
        reverse_index: std::collections::BTreeMap::new(),
    }
}

fn make_variable_width_schema() -> Schema {
    Schema {
        tables: vec![
            Table {
                id: TableId(10),
                stable_id: "tiny".to_string(),
                schema_name: None,
                name: "tiny".to_string(),
                columns: vec![Column {
                    id: ColumnId(10),
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                    is_primary_key: true,
                    comment: None,
                    enum_values: None,
                }],
                foreign_keys: vec![],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
            Table {
                id: TableId(11),
                stable_id: "extraordinarily_verbose_audit_log_entries".to_string(),
                schema_name: None,
                name: "extraordinarily_verbose_audit_log_entries".to_string(),
                columns: vec![
                    Column {
                        id: ColumnId(11),
                        name: "id".to_string(),
                        data_type: "uuid".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                        enum_values: None,
                    },
                    Column {
                        id: ColumnId(12),
                        name: "very_long_business_context_identifier".to_string(),
                        data_type: "timestamp with time zone".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                        enum_values: None,
                    },
                ],
                foreign_keys: vec![],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
            Table {
                id: TableId(12),
                stable_id: "medium".to_string(),
                schema_name: None,
                name: "medium".to_string(),
                columns: vec![Column {
                    id: ColumnId(13),
                    name: "display_name".to_string(),
                    data_type: "varchar(255)".to_string(),
                    nullable: false,
                    is_primary_key: false,
                    comment: None,
                    enum_values: None,
                }],
                foreign_keys: vec![],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
        ],
        views: vec![],
        enums: vec![],
    }
}

fn make_tall_rank_schema() -> Schema {
    let columns = (0_u64..18)
        .map(|index| Column {
            id: ColumnId(100 + index),
            name: format!("extremely_long_column_name_{index:02}"),
            data_type: "character varying(255)".to_string(),
            nullable: index % 2 == 0,
            is_primary_key: index == 0,
            comment: None,
            enum_values: None,
        })
        .collect();

    Schema {
        tables: vec![
            Table {
                id: TableId(20),
                stable_id: "audit_event_log_entries".to_string(),
                schema_name: Some("analytics".to_string()),
                name: "audit_event_log_entries".to_string(),
                columns,
                foreign_keys: vec![ForeignKey {
                    name: Some("fk_audit_event_log_entries_user_accounts".to_string()),
                    from_columns: vec!["extremely_long_column_name_01".to_string()],
                    to_schema: None,
                    to_table: "user_accounts".to_string(),
                    to_columns: vec!["id".to_string()],
                    on_delete: ReferentialAction::NoAction,
                    on_update: ReferentialAction::NoAction,
                }],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
            Table {
                id: TableId(21),
                stable_id: "user_accounts".to_string(),
                schema_name: Some("analytics".to_string()),
                name: "user_accounts".to_string(),
                columns: vec![
                    Column {
                        id: ColumnId(200),
                        name: "id".to_string(),
                        data_type: "uuid".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                        enum_values: None,
                    },
                    Column {
                        id: ColumnId(201),
                        name: "display_name".to_string(),
                        data_type: "varchar(255)".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                        enum_values: None,
                    },
                ],
                foreign_keys: vec![],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
        ],
        views: vec![],
        enums: vec![],
    }
}

fn make_fully_connected_cycle_schema() -> Schema {
    let table_names = ["accounts", "projects", "roles", "teams"];
    let tables = table_names
        .iter()
        .enumerate()
        .map(|(table_idx, table_name)| {
            let base_id = u64::try_from(table_idx * 10).unwrap();
            let columns = std::iter::once(Column {
                id: ColumnId(base_id + 1),
                name: "id".to_string(),
                data_type: "int".to_string(),
                nullable: false,
                is_primary_key: true,
                comment: None,
                enum_values: None,
            })
            .chain(
                table_names
                    .iter()
                    .enumerate()
                    .filter(move |(_, candidate)| *candidate != table_name)
                    .map(|(target_idx, target_name)| Column {
                        id: ColumnId(base_id + u64::try_from(target_idx).unwrap() + 2),
                        name: format!("{target_name}_id"),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                        enum_values: None,
                    }),
            )
            .collect();
            let foreign_keys = table_names
                .iter()
                .filter(|candidate| *candidate != table_name)
                .map(|target_name| ForeignKey {
                    name: Some(format!("fk_{table_name}_{target_name}")),
                    from_columns: vec![format!("{target_name}_id")],
                    to_schema: None,
                    to_table: (*target_name).to_string(),
                    to_columns: vec!["id".to_string()],
                    on_delete: ReferentialAction::NoAction,
                    on_update: ReferentialAction::NoAction,
                })
                .collect();

            Table {
                id: TableId(u64::try_from(table_idx).unwrap() + 40),
                stable_id: (*table_name).to_string(),
                schema_name: None,
                name: (*table_name).to_string(),
                columns,
                foreign_keys,
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            }
        })
        .collect();

    Schema {
        tables,
        views: vec![],
        enums: vec![],
    }
}

fn nodes_overlap(left: &PositionedNode, right: &PositionedNode) -> bool {
    left.x < right.x + right.width
        && left.x + left.width > right.x
        && left.y < right.y + right.height
        && left.y + left.height > right.y
}

#[test]
fn test_build_layout() {
    let schema = make_test_schema();
    let result = build_layout(&schema);

    assert!(result.is_ok());
    let graph = result.unwrap();
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.edges.len(), 1);
}

fn make_multi_schema_for_grouping() -> Schema {
    // Three schemas, two tables each, with a cross-schema FK so that the
    // hierarchical layout interleaves nodes across ranks. Without
    // swimlanes the resulting group bounding boxes overlap.
    let mk_table = |id: u64, schema: &str, name: &str, fk: Option<(&str, &str)>| Table {
        id: TableId(id),
        stable_id: format!("{schema}.{name}"),
        schema_name: Some(schema.to_string()),
        name: name.to_string(),
        columns: vec![
            Column {
                id: ColumnId(id * 10),
                name: "id".to_string(),
                data_type: "int".to_string(),
                nullable: false,
                is_primary_key: true,
                comment: None,
                enum_values: None,
            },
            Column {
                id: ColumnId(id * 10 + 1),
                name: "ref".to_string(),
                data_type: "int".to_string(),
                nullable: true,
                is_primary_key: false,
                comment: None,
                enum_values: None,
            },
        ],
        foreign_keys: fk
            .map(|(target_schema, target_table)| {
                vec![ForeignKey {
                    name: Some(format!("fk_{name}")),
                    from_columns: vec!["ref".to_string()],
                    to_table: target_table.to_string(),
                    to_schema: Some(target_schema.to_string()),
                    to_columns: vec!["id".to_string()],
                    on_delete: ReferentialAction::NoAction,
                    on_update: ReferentialAction::NoAction,
                }]
            })
            .unwrap_or_default(),
        indexes: vec![],
        primary_key_name: None,
        comment: None,
    };

    Schema {
        tables: vec![
            mk_table(1, "public", "users", None),
            mk_table(2, "public", "posts", Some(("public", "users"))),
            mk_table(3, "inventory", "products", None),
            mk_table(4, "inventory", "stock", Some(("inventory", "products"))),
            mk_table(5, "sales", "orders", Some(("public", "users"))),
            mk_table(6, "sales", "shipments", Some(("inventory", "products"))),
        ],
        ..Schema::default()
    }
}

fn make_prefix_grouping_schema() -> Schema {
    let mk_table = |id: u64, name: &str, fk: Option<&str>| Table {
        id: TableId(id),
        stable_id: name.to_string(),
        schema_name: None,
        name: name.to_string(),
        columns: vec![
            Column {
                id: ColumnId(id * 10),
                name: "id".to_string(),
                data_type: "int".to_string(),
                nullable: false,
                is_primary_key: true,
                comment: None,
                enum_values: None,
            },
            Column {
                id: ColumnId(id * 10 + 1),
                name: "ref".to_string(),
                data_type: "int".to_string(),
                nullable: true,
                is_primary_key: false,
                comment: None,
                enum_values: None,
            },
        ],
        foreign_keys: fk
            .map(|target_table| {
                vec![ForeignKey {
                    name: Some(format!("fk_{name}")),
                    from_columns: vec!["ref".to_string()],
                    to_table: target_table.to_string(),
                    to_schema: None,
                    to_columns: vec!["id".to_string()],
                    on_delete: ReferentialAction::NoAction,
                    on_update: ReferentialAction::NoAction,
                }]
            })
            .unwrap_or_default(),
        indexes: vec![],
        primary_key_name: None,
        comment: None,
    };

    Schema {
        tables: vec![
            mk_table(1, "product", None),
            mk_table(2, "product_categories", Some("product")),
            mk_table(3, "orders", Some("product")),
            mk_table(4, "order_items", Some("orders")),
            mk_table(5, "user_profile", None),
            mk_table(6, "user_preferences", Some("user_profile")),
        ],
        ..Schema::default()
    }
}

#[test]
fn swimlane_layout_produces_disjoint_group_bboxes() {
    use relune_core::{GroupingSpec, GroupingStrategy};

    let schema = make_multi_schema_for_grouping();
    let request = LayoutRequest {
        grouping: GroupingSpec {
            strategy: GroupingStrategy::BySchema,
        },
        ..LayoutRequest::default()
    };
    let config = LayoutConfig {
        direction: LayoutDirection::LeftToRight,
        ..LayoutConfig::default()
    };
    let positioned = build_layout_with_config(&schema, &request, &config).unwrap();

    assert!(
        positioned.groups.len() >= 2,
        "expected multiple groups, got {}",
        positioned.groups.len()
    );

    // For a horizontal layout (LR) the lanes are stacked along Y, so all
    // group Y-ranges must be pairwise disjoint.
    for (i, a) in positioned.groups.iter().enumerate() {
        for b in positioned.groups.iter().skip(i + 1) {
            let a_top = a.y;
            let a_bot = a.y + a.height;
            let b_top = b.y;
            let b_bot = b.y + b.height;
            let overlap = a_top < b_bot && b_top < a_bot;
            assert!(
                !overlap,
                "groups {} and {} overlap on Y axis: [{:.1},{:.1}] vs [{:.1},{:.1}]",
                a.id, b.id, a_top, a_bot, b_top, b_bot
            );
        }
    }
}

#[test]
fn swimlane_layout_vertical_disjoint_on_x() {
    use relune_core::{GroupingSpec, GroupingStrategy};

    let schema = make_multi_schema_for_grouping();
    let request = LayoutRequest {
        grouping: GroupingSpec {
            strategy: GroupingStrategy::BySchema,
        },
        ..LayoutRequest::default()
    };
    let config = LayoutConfig {
        direction: LayoutDirection::TopToBottom,
        ..LayoutConfig::default()
    };
    let positioned = build_layout_with_config(&schema, &request, &config).unwrap();

    // For a vertical layout (TB) the lanes are stacked along X.
    for (i, a) in positioned.groups.iter().enumerate() {
        for b in positioned.groups.iter().skip(i + 1) {
            let a_left = a.x;
            let a_right = a.x + a.width;
            let b_left = b.x;
            let b_right = b.x + b.width;
            let overlap = a_left < b_right && b_left < a_right;
            assert!(
                !overlap,
                "groups {} and {} overlap on X axis: [{:.1},{:.1}] vs [{:.1},{:.1}]",
                a.id, b.id, a_left, a_right, b_left, b_right
            );
        }
    }
}

#[test]
fn force_grouped_layout_produces_disjoint_group_bboxes_on_y() {
    use relune_core::{GroupingSpec, GroupingStrategy};

    let schema = make_multi_schema_for_grouping();
    let request = LayoutRequest {
        grouping: GroupingSpec {
            strategy: GroupingStrategy::BySchema,
        },
        ..LayoutRequest::default()
    };
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        direction: LayoutDirection::LeftToRight,
        ..LayoutConfig::default()
    };
    let positioned = build_layout_with_config(&schema, &request, &config).unwrap();

    for (i, a) in positioned.groups.iter().enumerate() {
        for b in positioned.groups.iter().skip(i + 1) {
            let overlap = a.y < b.y + b.height && b.y < a.y + a.height;
            assert!(
                !overlap,
                "force-grouped layout produced overlapping Y ranges for {} and {}",
                a.id, b.id
            );
        }
    }
}

#[test]
fn force_grouped_layout_produces_disjoint_group_bboxes_on_x() {
    use relune_core::{GroupingSpec, GroupingStrategy};

    let schema = make_multi_schema_for_grouping();
    let request = LayoutRequest {
        grouping: GroupingSpec {
            strategy: GroupingStrategy::BySchema,
        },
        ..LayoutRequest::default()
    };
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        direction: LayoutDirection::TopToBottom,
        ..LayoutConfig::default()
    };
    let positioned = build_layout_with_config(&schema, &request, &config).unwrap();

    for (i, a) in positioned.groups.iter().enumerate() {
        for b in positioned.groups.iter().skip(i + 1) {
            let overlap = a.x < b.x + b.width && b.x < a.x + a.width;
            assert!(
                !overlap,
                "force-grouped layout produced overlapping X ranges for {} and {}",
                a.id, b.id
            );
        }
    }
}

#[test]
fn force_prefix_grouped_layout_produces_disjoint_group_bboxes_on_y() {
    use relune_core::{GroupingSpec, GroupingStrategy};

    let schema = make_prefix_grouping_schema();
    let request = LayoutRequest {
        grouping: GroupingSpec {
            strategy: GroupingStrategy::ByPrefix,
        },
        ..LayoutRequest::default()
    };
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        direction: LayoutDirection::LeftToRight,
        ..LayoutConfig::default()
    };
    let positioned = build_layout_with_config(&schema, &request, &config).unwrap();

    for (i, a) in positioned.groups.iter().enumerate() {
        for b in positioned.groups.iter().skip(i + 1) {
            let overlap = a.y < b.y + b.height && b.y < a.y + a.height;
            assert!(
                !overlap,
                "force-grouped prefix layout produced overlapping Y ranges for {} and {}",
                a.id, b.id
            );
        }
    }
}

#[test]
fn force_prefix_grouped_layout_produces_disjoint_group_bboxes_on_x() {
    use relune_core::{GroupingSpec, GroupingStrategy};

    let schema = make_prefix_grouping_schema();
    let request = LayoutRequest {
        grouping: GroupingSpec {
            strategy: GroupingStrategy::ByPrefix,
        },
        ..LayoutRequest::default()
    };
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        direction: LayoutDirection::TopToBottom,
        ..LayoutConfig::default()
    };
    let positioned = build_layout_with_config(&schema, &request, &config).unwrap();

    for (i, a) in positioned.groups.iter().enumerate() {
        for b in positioned.groups.iter().skip(i + 1) {
            let overlap = a.x < b.x + b.width && b.x < a.x + a.width;
            assert!(
                !overlap,
                "force-grouped prefix layout produced overlapping X ranges for {} and {}",
                a.id, b.id
            );
        }
    }
}

#[test]
#[allow(clippy::float_cmp)]
fn test_layout_deterministic() {
    let schema = make_test_schema();
    let config = LayoutConfig::default();
    let request = LayoutRequest::default();

    let result1 = build_layout_with_config(&schema, &request, &config).unwrap();
    let result2 = build_layout_with_config(&schema, &request, &config).unwrap();

    // Results should be identical
    assert_eq!(result1.nodes.len(), result2.nodes.len());
    for (n1, n2) in result1.nodes.iter().zip(result2.nodes.iter()) {
        assert_eq!(n1.x, n2.x);
        assert_eq!(n1.y, n2.y);
    }
}

#[test]
fn test_edge_label_position() {
    let schema = make_test_schema();
    let result = build_layout(&schema).unwrap();

    // Check that edges have label positions
    for edge in &result.edges {
        // Label position should be set
        assert!(edge.label_x.is_finite());
        assert!(edge.label_y.is_finite());
    }
}

#[test]
fn test_force_layout_bt_mirrors_tb_and_rl_mirrors_lr() {
    let schema = make_test_schema();
    let request = LayoutRequest::default();
    let cfg_tb = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        direction: LayoutDirection::TopToBottom,
        ..Default::default()
    };
    let cfg_bt = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        direction: LayoutDirection::BottomToTop,
        ..Default::default()
    };
    let cfg_lr = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        direction: LayoutDirection::LeftToRight,
        ..Default::default()
    };
    let cfg_rl = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        direction: LayoutDirection::RightToLeft,
        ..Default::default()
    };

    let tb = build_layout_with_config(&schema, &request, &cfg_tb).unwrap();
    let bt = build_layout_with_config(&schema, &request, &cfg_bt).unwrap();
    let lr = build_layout_with_config(&schema, &request, &cfg_lr).unwrap();
    let rl = build_layout_with_config(&schema, &request, &cfg_rl).unwrap();

    let bounds_tb = super::spacing::compute_graph_bounds(&tb.nodes, &cfg_tb);
    let bounds_lr = super::spacing::compute_graph_bounds(&lr.nodes, &cfg_lr);

    let mut by_id_tb: BTreeMap<&str, _> = tb.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let by_id_bt: BTreeMap<&str, _> = bt.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let by_id_lr: BTreeMap<&str, _> = lr.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let by_id_rl: BTreeMap<&str, _> = rl.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    for (id, n_tb) in &by_id_tb {
        let n_bt = by_id_bt.get(id).expect("same schema");
        let expected_y = bounds_tb.1 - n_tb.y - n_tb.height;
        assert!(
            (n_bt.y - expected_y).abs() < 1e-3,
            "BT Y mirror mismatch for {id}: got {} expected {}",
            n_bt.y,
            expected_y
        );
    }

    for (id, n_lr) in &by_id_lr {
        let n_rl = by_id_rl.get(id).expect("same schema");
        let expected_x = bounds_lr.0 - n_lr.x - n_lr.width;
        assert!(
            (n_rl.x - expected_x).abs() < 1e-3,
            "RL X mirror mismatch for {id}: got {} expected {}",
            n_rl.x,
            expected_x
        );
    }

    // Same underlying placement: canvas size unchanged by mirroring.
    assert!((tb.width - bt.width).abs() < 1e-3);
    assert!((tb.height - bt.height).abs() < 1e-3);
    assert!((lr.width - rl.width).abs() < 1e-3);
    assert!((lr.height - rl.height).abs() < 1e-3);

    // Mirroring must actually move at least one node when the graph is non-degenerate.
    by_id_tb.retain(|_, n| {
        let n_bt = by_id_bt.get(n.id.as_str()).unwrap();
        (n.y - n_bt.y).abs() > 1e-3
    });
    assert!(
        !by_id_tb.is_empty(),
        "expected BT to differ from TB on Y for make_test_schema"
    );
}

#[test]
fn test_force_layout_keeps_connected_nodes_clear_after_primary_axis_restore() {
    let schema = make_test_schema();
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        direction: LayoutDirection::TopToBottom,
        horizontal_spacing: 320.0,
        vertical_spacing: 8.0,
        compaction: LayoutCompactionSpec {
            min_horizontal_spacing: 220.0,
            min_vertical_spacing: 8.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let graph = build_layout_with_config(&schema, &LayoutRequest::default(), &config).unwrap();
    let users = graph
        .nodes
        .iter()
        .find(|node| node.id == "users")
        .expect("users node");
    let posts = graph
        .nodes
        .iter()
        .find(|node| node.id == "posts")
        .expect("posts node");
    let (_, gap_y) = force_pair_axis_gaps(
        users.x,
        users.y,
        users.width,
        users.height,
        posts.x,
        posts.y,
        posts.width,
        posts.height,
    );

    assert!(
        gap_y >= FORCE_CONNECTED_NODE_GAP,
        "expected force layout to preserve vertical clearance after restoring rank-guided primary positions, got {gap_y}"
    );
}

#[test]
fn test_force_layout_canonical_config_preserves_screen_spacing_semantics_for_lr() {
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        direction: LayoutDirection::LeftToRight,
        horizontal_spacing: 480.0,
        vertical_spacing: 120.0,
        ..Default::default()
    };

    let canonical = force_layout_canonical_config(&config);

    assert_eq!(canonical.direction, LayoutDirection::TopToBottom);
    assert!((canonical.vertical_spacing - 480.0).abs() < f32::EPSILON);
    assert!((canonical.horizontal_spacing - 120.0).abs() < f32::EPSILON);
}

#[test]
fn test_force_layout_valid_positions() {
    let schema = make_test_schema();
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        ..Default::default()
    };

    let result = build_layout_with_config(&schema, &LayoutRequest::default(), &config);

    assert!(result.is_ok());
    let graph = result.unwrap();

    // Check that all nodes have valid positions
    assert_eq!(graph.nodes.len(), 2);
    for node in &graph.nodes {
        // Positions should be finite and positive
        assert!(node.x.is_finite());
        assert!(node.y.is_finite());
        assert!(node.x >= config.origin_x);
        assert!(node.y >= config.origin_y);
        // Width and height should be positive
        assert!(node.width > 0.0);
        assert!(node.height > 0.0);
    }

    // Graph dimensions should be positive
    assert!(graph.width > 0.0);
    assert!(graph.height > 0.0);
}

#[test]
#[allow(clippy::float_cmp)]
fn test_force_layout_deterministic() {
    let schema = make_test_schema();
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        ..Default::default()
    };

    let result1 = build_layout_with_config(&schema, &LayoutRequest::default(), &config).unwrap();
    let result2 = build_layout_with_config(&schema, &LayoutRequest::default(), &config).unwrap();

    // Force layout should also be deterministic
    assert_eq!(result1.nodes.len(), result2.nodes.len());
    for (n1, n2) in result1.nodes.iter().zip(result2.nodes.iter()) {
        assert_eq!(n1.x, n2.x);
        assert_eq!(n1.y, n2.y);
    }
}

#[test]
fn test_force_layout_different_from_hierarchical() {
    let schema = make_test_schema();

    let hierarchical_config = LayoutConfig {
        mode: LayoutAlgorithm::Hierarchical,
        ..Default::default()
    };

    let force_config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        ..Default::default()
    };

    let hierarchical_result =
        build_layout_with_config(&schema, &LayoutRequest::default(), &hierarchical_config).unwrap();
    let force_result =
        build_layout_with_config(&schema, &LayoutRequest::default(), &force_config).unwrap();

    // Collect positions sorted by node id for comparison
    let mut hierarchical_positions: Vec<(&String, f32, f32)> = hierarchical_result
        .nodes
        .iter()
        .map(|n| (&n.id, n.x, n.y))
        .collect();
    hierarchical_positions.sort_by(|a, b| a.0.cmp(b.0));

    let mut force_positions: Vec<(&String, f32, f32)> = force_result
        .nodes
        .iter()
        .map(|n| (&n.id, n.x, n.y))
        .collect();
    force_positions.sort_by(|a, b| a.0.cmp(b.0));

    // The layouts should produce different positions for at least some nodes
    let positions_differ = hierarchical_positions
        .iter()
        .zip(force_positions.iter())
        .any(|((_, x1, y1), (_, x2, y2))| (x1 - x2).abs() > 1.0 || (y1 - y2).abs() > 1.0);

    assert!(
        positions_differ,
        "Force layout should produce different positions than hierarchical layout"
    );
}

#[test]
fn test_hierarchical_layout_avoids_overlap_with_variable_width_nodes() {
    let schema = make_variable_width_schema();
    let graph = build_layout(&schema).unwrap();

    let mut nodes = graph.nodes;
    nodes.sort_by(|left, right| left.x.total_cmp(&right.x));

    for pair in nodes.windows(2) {
        let current = &pair[0];
        let next = &pair[1];
        assert!(
            current.x + current.width <= next.x,
            "nodes {} and {} overlap on the same rank",
            current.id,
            next.id
        );
    }
}

#[test]
fn test_layout_expands_node_width_for_long_content() {
    let schema = make_variable_width_schema();
    let graph = build_layout(&schema).unwrap();

    let tiny = graph.nodes.iter().find(|node| node.id == "tiny").unwrap();
    let verbose = graph
        .nodes
        .iter()
        .find(|node| node.id == "extraordinarily_verbose_audit_log_entries")
        .unwrap();

    assert!(verbose.width > tiny.width);
    assert!(verbose.width > LayoutConfig::default().node_width);
}

#[test]
fn test_hierarchical_layout_avoids_overlap_between_tall_ranks() {
    let schema = make_tall_rank_schema();
    let graph = build_layout(&schema).unwrap();

    for (index, node) in graph.nodes.iter().enumerate() {
        for other in graph.nodes.iter().skip(index + 1) {
            assert!(
                !nodes_overlap(node, other),
                "nodes {} and {} overlap",
                node.id,
                other.id
            );
        }
    }
}

#[test]
fn test_build_positioned_node_preserves_column_flags() {
    let node = crate::graph::LayoutNode {
        id: "posts".to_string(),
        label: "posts".to_string(),
        schema_name: None,
        table_name: "posts".to_string(),
        kind: NodeKind::Table,
        columns: vec![
            crate::graph::LayoutColumn {
                name: "id".to_string(),
                data_type: "int".to_string(),
                nullable: false,
                is_primary_key: true,
                is_foreign_key: false,
                is_indexed: false,
            },
            crate::graph::LayoutColumn {
                name: "user_id".to_string(),
                data_type: "int".to_string(),
                nullable: false,
                is_primary_key: false,
                is_foreign_key: true,
                is_indexed: true,
            },
        ],
        inbound_count: 0,
        outbound_count: 1,
        has_self_loop: false,
        is_join_table_candidate: false,
        group_index: None,
    };

    let positioned = build_positioned_node(&node, 10.0, 20.0, 200.0, 100.0, true);

    assert!(positioned.columns[0].flags.relation.is_primary_key);
    assert!(!positioned.columns[0].flags.relation.is_foreign_key);
    assert!(!positioned.columns[0].flags.relation.is_indexed);
    assert!(positioned.columns[1].flags.relation.is_foreign_key);
    assert!(positioned.columns[1].flags.relation.is_indexed);
}

#[test]
#[allow(clippy::float_cmp)]
fn test_compaction_respects_layout_spec_overrides() {
    let spec = LayoutSpec {
        horizontal_spacing: 320.0,
        vertical_spacing: 180.0,
        compaction: LayoutCompactionSpec {
            threshold: 10,
            min_horizontal_spacing: 220.0,
            min_vertical_spacing: 120.0,
            min_node_width: 180.0,
            min_node_padding: 6.0,
            hide_columns_threshold_multiplier: 3,
        },
        ..Default::default()
    };

    let config = LayoutConfig::from(&spec);
    let compacted = config.compute_compacted_config(20);
    assert_eq!(compacted.horizontal_spacing, 220.0);
    assert_eq!(compacted.vertical_spacing, 120.0);
    assert_eq!(compacted.node_width, 180.0);
    assert_eq!(compacted.node_padding, 6.0);
    assert!(!compacted.hide_columns);

    let hidden_columns = config.compute_compacted_config(31);
    assert!(hidden_columns.hide_columns);
}

#[test]
fn test_layout_config_validate_rejects_invalid_values() {
    let config = LayoutConfig {
        horizontal_spacing: 0.0,
        node_padding: -1.0,
        force_iterations: 0,
        ..Default::default()
    };

    let error = config.validate().expect_err("config should be invalid");
    let message = error.to_string();

    assert!(message.contains("horizontal_spacing must be greater than 0"));
    assert!(message.contains("node_padding must be at least 0"));
    assert!(message.contains("force_iterations must be greater than 0"));
}

#[test]
fn test_build_layout_rejects_inconsistent_compaction_bounds() {
    let schema = make_test_schema();
    let config = LayoutConfig {
        horizontal_spacing: 200.0,
        compaction: LayoutCompactionSpec {
            min_horizontal_spacing: 220.0,
            ..LayoutCompactionSpec::default()
        },
        ..Default::default()
    };

    let error = build_layout_with_config(&schema, &LayoutRequest::default(), &config)
        .expect_err("config should fail validation");

    assert!(matches!(error, LayoutError::InvalidConfig(_)));
    assert!(error.to_string().contains(
        "compaction.min_horizontal_spacing must be less than or equal to horizontal_spacing"
    ));
}

#[test]
#[allow(clippy::float_cmp)]
fn test_auto_tuned_identity_for_empty_graph() {
    let config = LayoutConfig::default();
    let tuned = config.clone().auto_tuned(0, 0);
    assert_eq!(tuned.horizontal_spacing, config.horizontal_spacing);
    assert_eq!(tuned.vertical_spacing, config.vertical_spacing);
}

#[test]
fn test_auto_tuned_shrinks_spacing_for_medium_graph() {
    let config = LayoutConfig::default();
    let tuned = config.clone().auto_tuned(20, 20);
    assert!(
        tuned.horizontal_spacing < config.horizontal_spacing,
        "medium graph should have tighter spacing"
    );
}

#[test]
fn test_auto_tuned_widens_for_dense_graph() {
    let config = LayoutConfig::default();
    // 5 nodes, 15 edges => density = 3.0 (very dense)
    let sparse = config.clone().auto_tuned(5, 3);
    let dense = config.auto_tuned(5, 15);
    assert!(
        dense.horizontal_spacing > sparse.horizontal_spacing,
        "dense graph should have wider spacing than sparse one of same node count"
    );
}

#[test]
fn test_auto_tuned_respects_minimum_spacing() {
    let config = LayoutConfig::default();
    let tuned = config.clone().auto_tuned(100, 50);
    assert!(tuned.horizontal_spacing >= config.compaction.min_horizontal_spacing);
    assert!(tuned.vertical_spacing >= config.compaction.min_vertical_spacing);
}

#[test]
#[allow(clippy::float_cmp)]
fn test_auto_tune_disabled_preserves_custom_spacing() {
    let config = LayoutConfig {
        horizontal_spacing: 500.0,
        vertical_spacing: 200.0,
        auto_tune_spacing: false,
        ..Default::default()
    };

    // auto_tuned() always mutates; the guard is in build_layout_from_graph_with_config.
    // Verify the build-path logic inline.
    let effective = if config.auto_tune_spacing {
        config.clone().auto_tuned(50, 80)
    } else {
        config.clone()
    };
    assert_eq!(effective.horizontal_spacing, 500.0);
    assert_eq!(effective.vertical_spacing, 200.0);

    // Contrast: when enabled, spacing IS changed.
    let tuned = config.auto_tuned(50, 80);
    assert_ne!(tuned.horizontal_spacing, 500.0);
}

#[test]
fn test_build_layout_from_graph_does_not_mutate_input_when_columns_are_hidden() {
    let schema = make_test_schema();
    let graph = LayoutGraphBuilder::new().build(&schema);
    let original_column_count = graph.nodes[0].columns.len();
    let original_first_column_name = graph.nodes[0].columns[0].name.clone();
    let config = LayoutConfig {
        show_columns: false,
        ..Default::default()
    };

    let positioned = build_layout_from_graph_with_config(&graph, &config).unwrap();

    assert_eq!(graph.nodes[0].columns.len(), original_column_count);
    assert_eq!(graph.nodes[0].columns[0].name, original_first_column_name);
    assert!(positioned.nodes[0].columns.is_empty());
}

#[test]
fn test_column_y_offset_from_center_basic() {
    let config = LayoutConfig::default();
    let layout_node = crate::graph::LayoutNode {
        id: "t".to_string(),
        label: "t".to_string(),
        schema_name: None,
        table_name: "t".to_string(),
        kind: NodeKind::Table,
        columns: vec![
            crate::graph::LayoutColumn {
                name: "id".to_string(),
                data_type: "int".to_string(),
                is_primary_key: true,
                is_foreign_key: false,
                is_indexed: false,
                nullable: false,
            },
            crate::graph::LayoutColumn {
                name: "user_id".to_string(),
                data_type: "int".to_string(),
                is_primary_key: false,
                is_foreign_key: true,
                is_indexed: true,
                nullable: false,
            },
        ],
        inbound_count: 0,
        outbound_count: 0,
        has_self_loop: false,
        is_join_table_candidate: false,
        group_index: None,
    };
    let height = estimate_node_height(&layout_node, &config);
    let node = PositionedNode {
        id: "t".to_string(),
        label: "t".to_string(),
        kind: NodeKind::Table,
        columns: vec![
            PositionedColumn {
                name: "id".to_string(),
                data_type: "int".to_string(),
                flags: ColumnFlags {
                    nullable: false,
                    relation: ColumnRelationFlags {
                        is_primary_key: true,
                        is_foreign_key: false,
                        is_indexed: false,
                    },
                },
            },
            PositionedColumn {
                name: "user_id".to_string(),
                data_type: "int".to_string(),
                flags: ColumnFlags {
                    nullable: false,
                    relation: ColumnRelationFlags {
                        is_primary_key: false,
                        is_foreign_key: true,
                        is_indexed: true,
                    },
                },
            },
        ],
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height,
        is_join_table_candidate: false,
        has_self_loop: false,
        group_index: None,
    };

    // user_id is column index 1.
    let offset = column_y_offset_from_center(&node, &["user_id".to_string()], &config);
    let expected_col_y = 1.0f32.mul_add(
        config.column_height,
        config.node_padding + config.header_height,
    ) + config.column_height / 2.0;
    let expected = expected_col_y - node.height / 2.0;
    assert!(
        (offset - expected).abs() < 0.01,
        "got {offset}, expected {expected}"
    );
}

#[test]
#[allow(clippy::float_cmp)]
fn test_column_y_offset_fallback_for_empty_or_missing_columns() {
    let config = LayoutConfig::default();
    let empty_node = PositionedNode {
        id: "t".to_string(),
        label: "t".to_string(),
        kind: NodeKind::Table,
        columns: vec![],
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 50.0,
        is_join_table_candidate: false,
        has_self_loop: false,
        group_index: None,
    };

    // No columns in node → 0 (center).
    assert_eq!(
        column_y_offset_from_center(&empty_node, &["user_id".to_string()], &config),
        0.0
    );
    // Empty edge columns → 0 (center).
    assert_eq!(column_y_offset_from_center(&empty_node, &[], &config), 0.0);

    let node_with_col = PositionedNode {
        columns: vec![PositionedColumn {
            name: "id".to_string(),
            data_type: "int".to_string(),
            flags: ColumnFlags {
                nullable: false,
                relation: ColumnRelationFlags {
                    is_primary_key: true,
                    is_foreign_key: false,
                    is_indexed: false,
                },
            },
        }],
        height: 60.0,
        ..empty_node
    };
    // Column not found → 0 (center).
    assert_eq!(
        column_y_offset_from_center(&node_with_col, &["nonexistent".to_string()], &config),
        0.0
    );
}

#[test]
fn test_hierarchical_layout_handles_fully_connected_cycles() {
    let schema = make_fully_connected_cycle_schema();
    let layout_graph = LayoutGraphBuilder::new().build(&schema);
    let ranks = assign_ranks(&layout_graph, RankAssignmentStrategy::LongestPath);
    let ordered_nodes = order_nodes_within_layers(&layout_graph, &ranks);
    let graph = build_layout(&schema).unwrap();

    let ordered_node_count: usize = ordered_nodes.iter().map(Vec::len).sum();
    assert_eq!(ordered_node_count, layout_graph.nodes.len());
    assert_eq!(ranks.node_rank.len(), layout_graph.nodes.len());

    assert_eq!(graph.nodes.len(), 4);
    assert_eq!(graph.edges.len(), 12);

    let node_ids: std::collections::BTreeSet<_> =
        graph.nodes.iter().map(|node| node.id.as_str()).collect();
    assert_eq!(node_ids.len(), 4);
    for node in &graph.nodes {
        assert!(node.x.is_finite());
        assert!(node.y.is_finite());
        assert!(node.width > 0.0);
        assert!(node.height > 0.0);
    }
}

fn make_empty_schema() -> Schema {
    Schema {
        tables: vec![],
        views: vec![],
        enums: vec![],
    }
}

fn make_single_table_schema() -> Schema {
    Schema {
        tables: vec![Table {
            id: TableId(1),
            stable_id: "users".to_string(),
            schema_name: None,
            name: "users".to_string(),
            columns: vec![Column {
                id: ColumnId(1),
                name: "id".to_string(),
                data_type: "int".to_string(),
                nullable: false,
                is_primary_key: true,
                comment: None,
                enum_values: None,
            }],
            foreign_keys: vec![],
            indexes: vec![],
            primary_key_name: None,
            comment: None,
        }],
        views: vec![],
        enums: vec![],
    }
}

#[test]
fn test_empty_schema_hierarchical() {
    let schema = make_empty_schema();
    let result = build_layout(&schema).unwrap();
    assert!(result.nodes.is_empty());
    assert!(result.edges.is_empty());
    assert!(result.groups.is_empty());
}

#[test]
fn test_empty_schema_force_directed() {
    let schema = make_empty_schema();
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        ..Default::default()
    };
    let result = build_layout_with_config(&schema, &LayoutRequest::default(), &config).unwrap();
    assert!(result.nodes.is_empty());
    assert!(result.edges.is_empty());
}

#[test]
fn test_single_node_hierarchical() {
    let schema = make_single_table_schema();
    let result = build_layout(&schema).unwrap();
    assert_eq!(result.nodes.len(), 1);
    assert!(result.edges.is_empty());
    let node = &result.nodes[0];
    assert!(node.x.is_finite());
    assert!(node.y.is_finite());
    assert!(node.width > 0.0);
    assert!(node.height > 0.0);
}

#[test]
fn test_force_layout_avoids_overlap() {
    let schema = make_test_schema();
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        ..Default::default()
    };
    let graph = build_layout_with_config(&schema, &LayoutRequest::default(), &config).unwrap();

    for (i, node) in graph.nodes.iter().enumerate() {
        for other in graph.nodes.iter().skip(i + 1) {
            assert!(
                !nodes_overlap(node, other),
                "force layout: nodes {} and {} overlap",
                node.id,
                other.id
            );
        }
    }
}

#[test]
fn test_force_layout_avoids_overlap_many_tables() {
    // Stress test with more nodes to exercise the overlap resolution pass.
    let tables: Vec<_> = (0_u64..10)
        .map(|i| Table {
            id: TableId(i + 1),
            stable_id: format!("t{i}"),
            schema_name: None,
            name: format!("table_{i}"),
            columns: (0_u64..5)
                .map(|c| Column {
                    id: ColumnId(i * 10 + c + 1),
                    name: format!("col_{c}"),
                    data_type: "text".to_string(),
                    nullable: false,
                    is_primary_key: c == 0,
                    comment: None,
                    enum_values: None,
                })
                .collect(),
            foreign_keys: if i > 0 {
                vec![ForeignKey {
                    name: None,
                    from_columns: vec!["col_1".to_string()],
                    to_schema: None,
                    to_table: format!("table_{}", i - 1),
                    to_columns: vec!["col_0".to_string()],
                    on_delete: ReferentialAction::NoAction,
                    on_update: ReferentialAction::NoAction,
                }]
            } else {
                vec![]
            },
            indexes: vec![],
            primary_key_name: None,
            comment: None,
        })
        .collect();
    let schema = Schema {
        tables,
        views: vec![],
        enums: vec![],
    };
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        ..Default::default()
    };
    let graph = build_layout_with_config(&schema, &LayoutRequest::default(), &config).unwrap();

    for (i, node) in graph.nodes.iter().enumerate() {
        for other in graph.nodes.iter().skip(i + 1) {
            assert!(
                !nodes_overlap(node, other),
                "force layout: nodes {} and {} overlap",
                node.id,
                other.id
            );
        }
    }
}

#[test]
fn test_resolve_force_overlaps_with_asymmetric_node_sizes() {
    let mut positions = vec![(364.75726, 1350.5088), (320.7149, 1578.5088)];
    let node_sizes = vec![
        NodeSize {
            width: 319.0,
            height: 264.0,
        },
        NodeSize {
            width: 291.0,
            height: 174.0,
        },
    ];

    resolve_force_overlaps(&mut positions, &node_sizes, 8.0);

    let left = PositionedNode {
        id: "orders".to_string(),
        label: "orders".to_string(),
        kind: relune_core::NodeKind::Table,
        columns: vec![],
        x: positions[0].0,
        y: positions[0].1,
        width: node_sizes[0].width,
        height: node_sizes[0].height,
        is_join_table_candidate: false,
        has_self_loop: false,
        group_index: None,
    };
    let right = PositionedNode {
        id: "order_items".to_string(),
        label: "order_items".to_string(),
        kind: relune_core::NodeKind::Table,
        columns: vec![],
        x: positions[1].0,
        y: positions[1].1,
        width: node_sizes[1].width,
        height: node_sizes[1].height,
        is_join_table_candidate: false,
        has_self_loop: false,
        group_index: None,
    };

    assert!(!nodes_overlap(&left, &right));
}

#[test]
#[allow(clippy::similar_names)]
fn test_resolve_force_overlaps_grid_handles_many_nodes() {
    // Place a 6x6 grid of identical rectangles with overlapping bounds so
    // that the spatial-grid candidate pruning has to find the overlapping
    // pairs across same-cell and forward-neighbour cells alike.
    let cols = 6;
    let rows = 6;
    let cell_w = 120.0_f32;
    let cell_h = 80.0_f32;
    let mut positions: Vec<(f32, f32)> = Vec::with_capacity(cols * rows);
    let mut node_sizes: Vec<NodeSize> = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        for c in 0..cols {
            #[allow(clippy::cast_precision_loss)]
            let x = c as f32 * cell_w * 0.6;
            #[allow(clippy::cast_precision_loss)]
            let y = r as f32 * cell_h * 0.6;
            positions.push((x, y));
            node_sizes.push(NodeSize {
                width: cell_w,
                height: cell_h,
            });
        }
    }

    resolve_force_overlaps(&mut positions, &node_sizes, 8.0);

    for i in 0..positions.len() {
        for j in (i + 1)..positions.len() {
            let (xi, yi) = positions[i];
            let (xj, yj) = positions[j];
            let dx_overlap = (xi + node_sizes[i].width).min(xj + node_sizes[j].width) - xi.max(xj);
            let dy_overlap =
                (yi + node_sizes[i].height).min(yj + node_sizes[j].height) - yi.max(yj);
            assert!(
                dx_overlap <= 0.0 || dy_overlap <= 0.0,
                "nodes {i} and {j} still overlap after resolution",
            );
        }
    }
}

#[test]
fn test_single_node_force_directed() {
    let schema = make_single_table_schema();
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        ..Default::default()
    };
    let result = build_layout_with_config(&schema, &LayoutRequest::default(), &config).unwrap();
    assert_eq!(result.nodes.len(), 1);
    assert!(result.edges.is_empty());
    let node = &result.nodes[0];
    assert!(node.x.is_finite());
    assert!(node.y.is_finite());
    assert!(node.width > 0.0);
    assert!(node.height > 0.0);
}

#[test]
fn test_estimate_text_width_counts_cjk_as_wider_than_ascii() {
    let ascii = estimate_text_width("users", COLUMN_FONT_SIZE);
    let cjk = estimate_text_width("利用者", COLUMN_FONT_SIZE);

    assert!(cjk > ascii);
}

#[test]
fn test_parallel_label_parameter_mirrors_reverse_edges() {
    let route_forward = EdgeRoute {
        x1: 0.0,
        y1: 0.0,
        x2: 90.0,
        y2: 0.0,
        control_points: Vec::new(),
        style: RouteStyle::Straight,
        label_position: (45.0, 0.0),
    };
    let route_reverse = EdgeRoute {
        x1: 90.0,
        y1: 0.0,
        x2: 0.0,
        y2: 0.0,
        control_points: Vec::new(),
        style: RouteStyle::Straight,
        label_position: (45.0, 0.0),
    };

    let forward = point_along_route(&route_forward, parallel_label_parameter("a", "b", 0, 2));
    let reverse = point_along_route(&route_reverse, parallel_label_parameter("b", "a", 1, 2));

    assert!((forward.0 - 30.0).abs() < 0.001);
    assert!((reverse.0 - 60.0).abs() < 0.001);
    assert!((forward.0 - reverse.0).abs() > 0.001);
}

#[test]
fn test_parallel_edge_labels_avoid_endpoint_nodes() {
    let graph = LayoutGraph {
        nodes: Vec::new(),
        edges: vec![
            LayoutEdge {
                from: "authors".to_string(),
                to: "posts".to_string(),
                name: Some("fk_posts_primary_author_identifier".to_string()),
                from_columns: vec!["primary_author_id".to_string()],
                to_columns: vec!["id".to_string()],
                kind: EdgeKind::ForeignKey,
                is_self_loop: false,
                nullable: false,
                target_cardinality: relune_core::layout::Cardinality::One,
                is_collapsed_join: false,
                collapsed_join_table: None,
            },
            LayoutEdge {
                from: "authors".to_string(),
                to: "posts".to_string(),
                name: Some("fk_posts_review_author_identifier".to_string()),
                from_columns: vec!["review_author_id".to_string()],
                to_columns: vec!["id".to_string()],
                kind: EdgeKind::ForeignKey,
                is_self_loop: false,
                nullable: false,
                target_cardinality: relune_core::layout::Cardinality::One,
                is_collapsed_join: false,
                collapsed_join_table: None,
            },
        ],
        groups: Vec::new(),
        node_index: std::collections::BTreeMap::new(),
        reverse_index: std::collections::BTreeMap::new(),
    };
    let positioned_nodes = vec![
        PositionedNode {
            id: "authors".to_string(),
            label: "authors".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 150.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
    ];

    let edges = route_edges(&graph, &positioned_nodes, &LayoutConfig::default(), None)
        .expect("parallel edge routing should succeed");
    assert_eq!(edges.len(), 2);

    for edge in &edges {
        let hw = estimate_label_half_width(&edge.label);
        for node in &positioned_nodes {
            let overlaps = edge.label_x + hw > node.x
                && edge.label_x - hw < node.x + node.width
                && edge.label_y + LABEL_HALF_H > node.y
                && edge.label_y - LABEL_HALF_H < node.y + node.height;
            assert!(
                !overlaps,
                "Label {} overlaps node {} at ({}, {})",
                edge.label, node.id, edge.label_x, edge.label_y
            );
        }
    }
}

#[test]
fn test_route_edges_offsets_parallel_foreign_keys() {
    let schema = Schema {
        tables: vec![
            Table {
                id: TableId(1),
                stable_id: "users".to_string(),
                schema_name: None,
                name: "users".to_string(),
                columns: vec![Column {
                    id: ColumnId(1),
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                    is_primary_key: true,
                    comment: None,
                    enum_values: None,
                }],
                foreign_keys: vec![],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
            Table {
                id: TableId(2),
                stable_id: "posts".to_string(),
                schema_name: None,
                name: "posts".to_string(),
                columns: vec![
                    Column {
                        id: ColumnId(2),
                        name: "author_id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                        enum_values: None,
                    },
                    Column {
                        id: ColumnId(3),
                        name: "reviewer_id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                        enum_values: None,
                    },
                ],
                foreign_keys: vec![
                    ForeignKey {
                        name: Some("fk_posts_author".to_string()),
                        from_columns: vec!["author_id".to_string()],
                        to_schema: None,
                        to_table: "users".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    },
                    ForeignKey {
                        name: Some("fk_posts_reviewer".to_string()),
                        from_columns: vec!["reviewer_id".to_string()],
                        to_schema: None,
                        to_table: "users".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    },
                ],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
        ],
        views: vec![],
        enums: vec![],
    };

    let graph = build_layout(&schema).unwrap();
    assert_eq!(graph.edges.len(), 2);
    assert!(
        (graph.edges[0].route.x1 - graph.edges[1].route.x1).abs() > f32::EPSILON
            || (graph.edges[0].route.y1 - graph.edges[1].route.y1).abs() > f32::EPSILON
    );
}

#[test]
fn test_parallel_edge_labels_do_not_overlap() {
    // Two FK edges between the same pair of tables — their labels must not overlap.
    let schema = Schema {
        tables: vec![
            Table {
                id: TableId(1),
                stable_id: "users".to_string(),
                schema_name: None,
                name: "users".to_string(),
                columns: vec![Column {
                    id: ColumnId(1),
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                    is_primary_key: true,
                    comment: None,
                    enum_values: None,
                }],
                foreign_keys: vec![],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
            Table {
                id: TableId(2),
                stable_id: "posts".to_string(),
                schema_name: None,
                name: "posts".to_string(),
                columns: vec![
                    Column {
                        id: ColumnId(2),
                        name: "author_id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                        enum_values: None,
                    },
                    Column {
                        id: ColumnId(3),
                        name: "editor_id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                        enum_values: None,
                    },
                ],
                foreign_keys: vec![
                    ForeignKey {
                        name: Some("fk_author".to_string()),
                        from_columns: vec!["author_id".to_string()],
                        to_schema: None,
                        to_table: "users".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    },
                    ForeignKey {
                        name: Some("fk_editor".to_string()),
                        from_columns: vec!["editor_id".to_string()],
                        to_schema: None,
                        to_table: "users".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    },
                ],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
        ],
        views: vec![],
        enums: vec![],
    };

    let graph = build_layout(&schema).unwrap();
    assert_eq!(graph.edges.len(), 2);

    // Check that label bounding boxes do not overlap.
    let (lx0, ly0) = (graph.edges[0].label_x, graph.edges[0].label_y);
    let (lx1, ly1) = (graph.edges[1].label_x, graph.edges[1].label_y);
    let hw0 = estimate_label_half_width(&graph.edges[0].label);
    let hw1 = estimate_label_half_width(&graph.edges[1].label);
    let hh = LABEL_HALF_H;
    let overlaps = (lx0 + hw0 > lx1 - hw1)
        && (lx0 - hw0 < lx1 + hw1)
        && (ly0 + hh > ly1 - hh)
        && (ly0 - hh < ly1 + hh);
    assert!(
        !overlaps,
        "Parallel edge labels overlap: ({lx0},{ly0}) vs ({lx1},{ly1})"
    );
}

#[test]
fn test_self_loop_label_outside_source_node() {
    // A self-referencing FK: the label must not sit inside the source node.
    let schema = Schema {
        tables: vec![Table {
            id: TableId(1),
            stable_id: "employees".to_string(),
            schema_name: None,
            name: "employees".to_string(),
            columns: vec![
                Column {
                    id: ColumnId(1),
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                    is_primary_key: true,
                    comment: None,
                    enum_values: None,
                },
                Column {
                    id: ColumnId(2),
                    name: "manager_id".to_string(),
                    data_type: "int".to_string(),
                    nullable: true,
                    is_primary_key: false,
                    comment: None,
                    enum_values: None,
                },
            ],
            foreign_keys: vec![ForeignKey {
                name: Some("fk_manager".to_string()),
                from_columns: vec!["manager_id".to_string()],
                to_schema: None,
                to_table: "employees".to_string(),
                to_columns: vec!["id".to_string()],
                on_delete: ReferentialAction::NoAction,
                on_update: ReferentialAction::NoAction,
            }],
            indexes: vec![],
            primary_key_name: None,
            comment: None,
        }],
        views: vec![],
        enums: vec![],
    };

    let graph = build_layout(&schema).unwrap();
    assert_eq!(graph.edges.len(), 1);

    let edge = &graph.edges[0];
    assert!(edge.is_self_loop);

    // The node that owns the self-loop.
    let node = &graph.nodes[0];
    // Label center must be outside the node bounding box (allowing slight
    // overlap from the label's extent is OK, but the center should not be
    // inside the node).
    let center_inside = edge.label_x >= node.x
        && edge.label_x <= node.x + node.width
        && edge.label_y >= node.y
        && edge.label_y <= node.y + node.height;
    assert!(
        !center_inside,
        "Self-loop label center ({},{}) is inside node ({},{},{},{})",
        edge.label_x, edge.label_y, node.x, node.y, node.width, node.height
    );
}

#[test]
fn test_route_edges_use_inter_rank_channel_for_hierarchical_flow() {
    let graph = single_edge_graph("authors", "posts");
    let positioned_nodes = vec![
        PositionedNode {
            id: "authors".to_string(),
            label: "authors".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 200.0,
            y: 200.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
    ];
    let config = LayoutConfig::default();

    let edges = route_edges(&graph, &positioned_nodes, &config, Some(&[0, 1]))
        .expect("ranked routing should succeed");
    let edge = edges.first().expect("edge");

    assert_eq!(edge.route.control_points.len(), 2);
    assert!((edge.route.control_points[0].1 - 140.0).abs() < 0.001);
    assert!((edge.route.control_points[1].1 - 140.0).abs() < 0.001);
    assert!((edge.route.label_position.1 - 140.0).abs() < 0.001);
}

#[test]
fn test_route_edges_use_separate_same_rank_channel_rule() {
    let graph = single_edge_graph("authors", "posts");
    let positioned_nodes = vec![
        PositionedNode {
            id: "authors".to_string(),
            label: "authors".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 220.0,
            y: 120.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
    ];
    let config = LayoutConfig::default();

    let edges = route_edges(&graph, &positioned_nodes, &config, Some(&[0, 0]))
        .expect("same-rank routing should succeed");
    let edge = edges.first().expect("edge");
    assert!(
        edge.route
            .control_points
            .iter()
            .any(|point| (point.0 - 160.0).abs() < 0.001)
    );
}

#[test]
fn test_route_edges_shift_inter_rank_channel_away_from_obstacle() {
    let graph = single_edge_graph("authors", "posts");
    let mut positioned_nodes = vec![
        PositionedNode {
            id: "authors".to_string(),
            label: "authors".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 200.0,
            y: 200.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
    ];
    positioned_nodes.push(PositionedNode {
        id: "blocker".to_string(),
        label: "blocker".to_string(),
        kind: NodeKind::Table,
        columns: Vec::new(),
        x: 120.0,
        y: 110.0,
        width: 60.0,
        height: 60.0,
        is_join_table_candidate: false,
        has_self_loop: false,
        group_index: None,
    });

    let (edges, diagnostics) = route_edges_with_diagnostics(
        &graph,
        &positioned_nodes,
        &LayoutConfig::default(),
        Some(&[0, 1]),
    )
    .expect("diagnostic routing should succeed");
    let edge = edges.first().expect("edge");
    let channel_y = edge.route.control_points[0].1;

    assert_eq!(diagnostics.non_self_loop_detour_activations, 0);
    assert_eq!(edge.route.control_points.len(), 2);
    assert!(!(96.0..=184.0).contains(&channel_y));
    assert_eq!(
        route_obstacle_hit_count(
            &edge.route,
            &label_rects_from_nodes(&positioned_nodes[2..]),
            0.0
        ),
        0
    );
}

#[test]
fn test_route_edges_spread_parallel_edges_across_channels() {
    let mut node_index = std::collections::BTreeMap::new();
    node_index.insert("authors".to_string(), 0usize);
    node_index.insert("posts".to_string(), 1usize);
    let graph = LayoutGraph {
        nodes: Vec::new(),
        edges: vec![
            LayoutEdge {
                from: "authors".to_string(),
                to: "posts".to_string(),
                name: Some("fk_posts_author".to_string()),
                from_columns: vec!["author_id".to_string()],
                to_columns: vec!["id".to_string()],
                kind: EdgeKind::ForeignKey,
                is_self_loop: false,
                nullable: false,
                target_cardinality: relune_core::layout::Cardinality::One,
                is_collapsed_join: false,
                collapsed_join_table: None,
            },
            LayoutEdge {
                from: "authors".to_string(),
                to: "posts".to_string(),
                name: Some("fk_posts_reviewer".to_string()),
                from_columns: vec!["review_author_id".to_string()],
                to_columns: vec!["id".to_string()],
                kind: EdgeKind::ForeignKey,
                is_self_loop: false,
                nullable: false,
                target_cardinality: relune_core::layout::Cardinality::One,
                is_collapsed_join: false,
                collapsed_join_table: None,
            },
        ],
        groups: Vec::new(),
        node_index,
        reverse_index: std::collections::BTreeMap::new(),
    };
    let positioned_nodes = vec![
        PositionedNode {
            id: "authors".to_string(),
            label: "authors".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 200.0,
            y: 200.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
    ];

    let edges = route_edges(
        &graph,
        &positioned_nodes,
        &LayoutConfig::default(),
        Some(&[0, 1]),
    )
    .expect("bundled routing should succeed");
    assert_eq!(edges.len(), 2);

    let shared_trunk_y = |edge: &PositionedEdge| {
        route_points(&edge.route)
            .windows(2)
            .filter(|segment| (segment[0].1 - segment[1].1).abs() < 0.001)
            .max_by(|left, right| {
                let left_len = (left[1].0 - left[0].0).abs();
                let right_len = (right[1].0 - right[0].0).abs();
                left_len.total_cmp(&right_len)
            })
            .map(|segment| segment[0].1)
            .expect("bundled route should keep a horizontal trunk")
    };

    let first_trunk = shared_trunk_y(&edges[0]);
    let second_trunk = shared_trunk_y(&edges[1]);

    assert!((first_trunk - second_trunk).abs() < 0.001);
    assert_ne!(route_points(&edges[0].route), route_points(&edges[1].route));
}

#[test]
fn test_route_edges_shift_reverse_channel_away_from_obstacle() {
    let graph = single_edge_graph("posts", "authors");
    let mut positioned_nodes = vec![
        PositionedNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 220.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "authors".to_string(),
            label: "authors".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 200.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
    ];
    positioned_nodes.push(PositionedNode {
        id: "blocker".to_string(),
        label: "blocker".to_string(),
        kind: NodeKind::Table,
        columns: Vec::new(),
        x: 120.0,
        y: 110.0,
        width: 60.0,
        height: 60.0,
        is_join_table_candidate: false,
        has_self_loop: false,
        group_index: None,
    });

    let (edges, diagnostics) = route_edges_with_diagnostics(
        &graph,
        &positioned_nodes,
        &LayoutConfig::default(),
        Some(&[1, 0]),
    )
    .expect("reverse-channel routing should succeed");
    let edge = edges.first().expect("edge");

    assert_eq!(diagnostics.non_self_loop_detour_activations, 0);
    assert!((edge.route.control_points[0].1 - 198.0).abs() < 0.001);
}

#[test]
fn test_obstacle_aware_channel_rejects_candidates_that_violate_hard_constraints() {
    let graph = single_edge_graph("authors", "posts");
    let positioned_nodes = vec![
        PositionedNode {
            id: "authors".to_string(),
            label: "authors".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 200.0,
            y: 200.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
    ];
    let node_ranks = [0usize, 1usize];
    let config = LayoutConfig {
        direction: LayoutDirection::TopToBottom,
        ..Default::default()
    };
    let rank_bounds = rank_axis_bounds(&positioned_nodes, &node_ranks, &config);
    let assignment = RegularPortAssignment {
        source_side: AttachmentSide::South,
        target_side: AttachmentSide::North,
        source_slot_offset: 0.0,
        source_slot_index: 0,
        source_slot_count: 1,
        target_slot_offset: 0.0,
        target_slot_index: 0,
        target_slot_count: 1,
        source_row_offset: 0.0,
        target_row_offset: 0.0,
    };
    let obstacles = vec![Rect {
        x: -300.0,
        y: 84.0,
        w: 900.0,
        h: 180.0,
    }];

    let candidate = obstacle_aware_channel_for_edge(
        ObstacleRoutingContext {
            graph: &graph,
            edge: &graph.edges[0],
            node_ranks: &node_ranks,
            rank_bounds: Some(&rank_bounds),
            direction: config.direction,
            assignment: &assignment,
            obstacles: &obstacles,
            channel_usage: &BTreeMap::new(),
            style: RouteStyle::Orthogonal,
        },
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 80.0,
        },
        Rect {
            x: 200.0,
            y: 200.0,
            w: 100.0,
            h: 80.0,
        },
    );

    assert!(candidate.is_none());
}

#[test]
fn test_route_edges_measure_detour_activation_without_ranked_channels() {
    let graph = single_edge_graph("authors", "posts");
    let mut positioned_nodes = vec![
        PositionedNode {
            id: "authors".to_string(),
            label: "authors".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 200.0,
            y: 200.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
    ];
    positioned_nodes.push(PositionedNode {
        id: "blocker".to_string(),
        label: "blocker".to_string(),
        kind: NodeKind::Table,
        columns: Vec::new(),
        x: 120.0,
        y: 110.0,
        width: 60.0,
        height: 60.0,
        is_join_table_candidate: false,
        has_self_loop: false,
        group_index: None,
    });

    let (_, diagnostics) =
        route_edges_with_diagnostics(&graph, &positioned_nodes, &LayoutConfig::default(), None)
            .expect("unranked routing should succeed");
    assert_eq!(diagnostics.non_self_loop_detour_activations, 1);
}

#[test]
fn test_route_edges_channel_fallback_diagnostic_when_all_candidates_blocked() {
    let graph = single_edge_graph("authors", "posts");
    let positioned_nodes = vec![
        PositionedNode {
            id: "authors".to_string(),
            label: "authors".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 200.0,
            y: 200.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "blocker".to_string(),
            label: "blocker".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: -300.0,
            y: 84.0,
            width: 900.0,
            height: 180.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
    ];

    let (edges, diagnostics) = route_edges_with_diagnostics(
        &graph,
        &positioned_nodes,
        &LayoutConfig::default(),
        Some(&[0, 1, 0]),
    )
    .expect("ranked routing with blocked channels should succeed via fallback");

    assert_eq!(diagnostics.channel_fallback_activations, 1);
    assert!(!edges.is_empty());
}

#[test]
fn test_route_edges_bypass_intermediate_obstacle_for_skipped_vertical_rank() {
    let graph = single_edge_graph("comments", "users");
    let positioned_nodes = vec![
        PositionedNode {
            id: "comments".to_string(),
            label: "comments".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "users".to_string(),
            label: "users".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 420.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 180.0,
            width: 100.0,
            height: 120.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
    ];

    let (edges, diagnostics) = route_edges_with_diagnostics(
        &graph,
        &positioned_nodes,
        &LayoutConfig::default(),
        Some(&[0, 2]),
    )
    .expect("vertical bypass routing should succeed");
    let edge = edges.first().expect("edge");

    assert_eq!(diagnostics.non_self_loop_detour_activations, 0);
    assert_eq!(
        route_obstacle_hit_count(
            &edge.route,
            &label_rects_from_nodes(&positioned_nodes[2..]),
            0.0
        ),
        0
    );
    assert!(edge.route.control_points.len() >= 4);
}

#[test]
fn test_bypass_channel_candidates_expand_symmetrically_per_lane() {
    let source_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 80.0,
    };
    let target_rect = Rect {
        x: 0.0,
        y: 420.0,
        w: 100.0,
        h: 80.0,
    };

    let candidates =
        bypass_channel_candidates(LayoutDirection::TopToBottom, source_rect, target_rect, 7);

    assert_eq!(candidates.len(), bypass_channel_lane_count() * 2);
    for (lane_index, pair) in candidates.chunks_exact(2).enumerate() {
        let expected_offset =
            BYPASS_CHANNEL_LANE_STEP * f32::from(u16::try_from(lane_index).unwrap_or(u16::MAX));
        assert_eq!(pair[0].axis, ChannelAxis::X);
        assert_eq!(pair[1].axis, ChannelAxis::X);
        assert!((pair[0].coordinate - pair[0].baseline - expected_offset).abs() < f32::EPSILON);
        assert!((pair[1].baseline - pair[1].coordinate - expected_offset).abs() < f32::EPSILON);
        assert_eq!(pair[0].stable_order + 1, pair[1].stable_order);
    }
}

#[test]
fn test_route_edges_bypass_intermediate_obstacle_for_skipped_horizontal_rank() {
    let graph = single_edge_graph("comments", "users");
    let config = LayoutConfig {
        direction: LayoutDirection::LeftToRight,
        ..Default::default()
    };
    let positioned_nodes = vec![
        PositionedNode {
            id: "comments".to_string(),
            label: "comments".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "users".to_string(),
            label: "users".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 420.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
        PositionedNode {
            id: "posts".to_string(),
            label: "posts".to_string(),
            kind: NodeKind::Table,
            columns: Vec::new(),
            x: 180.0,
            y: 0.0,
            width: 120.0,
            height: 120.0,
            is_join_table_candidate: false,
            has_self_loop: false,
            group_index: None,
        },
    ];

    let (edges, diagnostics) =
        route_edges_with_diagnostics(&graph, &positioned_nodes, &config, Some(&[0, 2]))
            .expect("horizontal bypass routing should succeed");
    let edge = edges.first().expect("edge");

    assert_eq!(diagnostics.non_self_loop_detour_activations, 0);
    assert_eq!(
        route_obstacle_hit_count(
            &edge.route,
            &label_rects_from_nodes(&positioned_nodes[2..]),
            0.0
        ),
        0
    );
    assert!(edge.route.control_points.len() >= 4);
}

fn label_rects_from_nodes(nodes: &[PositionedNode]) -> Vec<Rect> {
    nodes
        .iter()
        .map(|node| Rect {
            x: node.x,
            y: node.y,
            w: node.width,
            h: node.height,
        })
        .collect()
}

fn make_self_loop_schema() -> Schema {
    Schema {
        tables: vec![Table {
            id: TableId(1),
            stable_id: "categories".to_string(),
            schema_name: None,
            name: "categories".to_string(),
            columns: vec![
                Column {
                    id: ColumnId(1),
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                    is_primary_key: true,
                    comment: None,
                    enum_values: None,
                },
                Column {
                    id: ColumnId(2),
                    name: "parent_id".to_string(),
                    data_type: "int".to_string(),
                    nullable: true,
                    is_primary_key: false,
                    comment: None,
                    enum_values: None,
                },
            ],
            foreign_keys: vec![ForeignKey {
                name: Some("fk_parent".to_string()),
                from_columns: vec!["parent_id".to_string()],
                to_schema: None,
                to_table: "categories".to_string(),
                to_columns: vec!["id".to_string()],
                on_delete: ReferentialAction::SetNull,
                on_update: ReferentialAction::NoAction,
            }],
            indexes: vec![],
            primary_key_name: None,
            comment: None,
        }],
        views: vec![],
        enums: vec![],
    }
}

fn make_parallel_edges_schema() -> Schema {
    Schema {
        tables: vec![
            Table {
                id: TableId(1),
                stable_id: "users".to_string(),
                schema_name: None,
                name: "users".to_string(),
                columns: vec![Column {
                    id: ColumnId(1),
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                    is_primary_key: true,
                    comment: None,
                    enum_values: None,
                }],
                foreign_keys: vec![],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
            Table {
                id: TableId(2),
                stable_id: "messages".to_string(),
                schema_name: None,
                name: "messages".to_string(),
                columns: vec![
                    Column {
                        id: ColumnId(2),
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: true,
                        comment: None,
                        enum_values: None,
                    },
                    Column {
                        id: ColumnId(3),
                        name: "sender_id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                        enum_values: None,
                    },
                    Column {
                        id: ColumnId(4),
                        name: "recipient_id".to_string(),
                        data_type: "int".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                        enum_values: None,
                    },
                ],
                foreign_keys: vec![
                    ForeignKey {
                        name: Some("fk_sender".to_string()),
                        from_columns: vec!["sender_id".to_string()],
                        to_schema: None,
                        to_table: "users".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    },
                    ForeignKey {
                        name: Some("fk_recipient".to_string()),
                        from_columns: vec!["recipient_id".to_string()],
                        to_schema: None,
                        to_table: "users".to_string(),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    },
                ],
                indexes: vec![],
                primary_key_name: None,
                comment: None,
            },
        ],
        views: vec![],
        enums: vec![],
    }
}

fn assert_layout_invariants(graph: &PositionedGraph) {
    let node_ids: std::collections::BTreeSet<&str> =
        graph.nodes.iter().map(|n| n.id.as_str()).collect();

    for node in &graph.nodes {
        assert!(
            node.x.is_finite() && node.y.is_finite(),
            "node {} has non-finite position ({}, {})",
            node.id,
            node.x,
            node.y
        );
        assert!(
            node.width.is_finite() && node.height.is_finite(),
            "node {} has non-finite size ({} x {})",
            node.id,
            node.width,
            node.height
        );
        assert!(
            node.width > 0.0 && node.height > 0.0,
            "node {} has non-positive size ({} x {})",
            node.id,
            node.width,
            node.height
        );
    }

    assert!(
        graph.width.is_finite() && graph.height.is_finite(),
        "graph bounds are not finite: {} x {}",
        graph.width,
        graph.height
    );

    for edge in &graph.edges {
        let points = route_points(&edge.route);
        assert!(
            points.len() >= 2,
            "edge {} -> {} produced fewer than two route points",
            edge.from,
            edge.to
        );
        for point in &points {
            assert!(
                point.0.is_finite() && point.1.is_finite(),
                "edge {} -> {} has non-finite route point {:?}",
                edge.from,
                edge.to,
                point
            );
        }
        assert!(
            edge.label_x.is_finite() && edge.label_y.is_finite(),
            "edge {} -> {} has non-finite label position",
            edge.from,
            edge.to
        );
        assert!(
            node_ids.contains(edge.from.as_str()),
            "edge endpoint {} not present in positioned nodes",
            edge.from
        );
        assert!(
            node_ids.contains(edge.to.as_str()),
            "edge endpoint {} not present in positioned nodes",
            edge.to
        );
    }
}

#[test]
fn invariants_hold_for_self_loop_schema() {
    let schema = make_self_loop_schema();
    let result = build_layout(&schema).unwrap();
    assert_layout_invariants(&result);
    assert!(
        result.edges.iter().any(|edge| edge.is_self_loop),
        "expected at least one self-loop edge"
    );
}

#[test]
fn invariants_hold_for_parallel_edges_schema() {
    let schema = make_parallel_edges_schema();
    let result = build_layout(&schema).unwrap();
    assert_layout_invariants(&result);
    let parallel = result
        .edges
        .iter()
        .filter(|e| e.from == "messages" && e.to == "users")
        .count();
    assert_eq!(parallel, 2, "expected two parallel edges, got {parallel}");
}

#[test]
fn invariants_hold_for_cyclic_foreign_keys() {
    let schema = make_fully_connected_cycle_schema();
    let result = build_layout(&schema).unwrap();
    assert_layout_invariants(&result);
}

#[test]
fn invariants_hold_for_force_layout_self_loop_and_parallel() {
    let config = LayoutConfig {
        mode: LayoutAlgorithm::ForceDirected,
        ..LayoutConfig::default()
    };
    let request = LayoutRequest::default();

    for schema in [make_self_loop_schema(), make_parallel_edges_schema()] {
        let result = build_layout_with_config(&schema, &request, &config).unwrap();
        assert_layout_invariants(&result);
    }
}

#[test]
fn focus_extraction_does_not_leave_isolated_edges() {
    use relune_core::FocusSpec;

    let schema = make_fully_connected_cycle_schema();
    for depth in 0..=2 {
        let request = LayoutRequest {
            focus: Some(FocusSpec::new("accounts", depth)),
            ..LayoutRequest::default()
        };
        let result = build_layout_with_config(&schema, &request, &LayoutConfig::default()).unwrap();
        assert_layout_invariants(&result);
    }
}

#[test]
fn grouping_by_schema_does_not_leave_isolated_edges() {
    use relune_core::{GroupingSpec, GroupingStrategy};

    let schema = make_multi_schema_for_grouping();
    let request = LayoutRequest {
        grouping: GroupingSpec {
            strategy: GroupingStrategy::BySchema,
        },
        ..LayoutRequest::default()
    };
    let result = build_layout_with_config(&schema, &request, &LayoutConfig::default()).unwrap();
    assert_layout_invariants(&result);
}

#[test]
fn layout_is_deterministic_across_repeated_runs() {
    let configs = [
        LayoutConfig {
            mode: LayoutAlgorithm::Hierarchical,
            ..LayoutConfig::default()
        },
        LayoutConfig {
            mode: LayoutAlgorithm::ForceDirected,
            ..LayoutConfig::default()
        },
    ];
    let request = LayoutRequest::default();
    for schema in [
        make_test_schema(),
        make_self_loop_schema(),
        make_parallel_edges_schema(),
    ] {
        for config in &configs {
            let first = build_layout_with_config(&schema, &request, config).unwrap();
            let second = build_layout_with_config(&schema, &request, config).unwrap();
            assert_eq!(first.nodes.len(), second.nodes.len());
            for (a, b) in first.nodes.iter().zip(second.nodes.iter()) {
                assert_eq!(a.id, b.id);
                assert_eq!(a.x.to_bits(), b.x.to_bits());
                assert_eq!(a.y.to_bits(), b.y.to_bits());
                assert_eq!(a.width.to_bits(), b.width.to_bits());
                assert_eq!(a.height.to_bits(), b.height.to_bits());
            }
            assert_eq!(first.edges.len(), second.edges.len());
            for (a, b) in first.edges.iter().zip(second.edges.iter()) {
                assert_eq!(a.from, b.from);
                assert_eq!(a.to, b.to);
                assert_eq!(a.label_x.to_bits(), b.label_x.to_bits());
                assert_eq!(a.label_y.to_bits(), b.label_y.to_bits());
            }
        }
    }
}
