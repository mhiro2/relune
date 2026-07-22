//! Integration tests exercising validate → lint → diff pipelines
//! through the public `relune-core` API.

use relune_core::{
    Column, ColumnId, ForeignKey, Index, ReferentialAction, Schema, Table, TableId, diff_schemas,
    lint_schema,
};

fn make_table(
    id: u64,
    stable_id: &str,
    schema_name: Option<&str>,
    name: &str,
    columns: Vec<Column>,
    foreign_keys: Vec<ForeignKey>,
    indexes: Vec<Index>,
) -> Table {
    Table {
        id: TableId(id),
        stable_id: stable_id.to_string(),
        schema_name: schema_name.map(ToString::to_string),
        name: name.to_string(),
        columns,
        foreign_keys,
        indexes,
        primary_key_name: None,
        check_constraints: Vec::new(),
        comment: None,
    }
}

fn pk_col(id: u64, name: &str) -> Column {
    Column {
        id: ColumnId(id),
        name: name.to_string(),
        data_type: "bigint".to_string(),
        nullable: false,
        is_primary_key: true,
        comment: None,
        enum_values: None,
        semantics: relune_core::ColumnSemantics::default(),
    }
}

fn fk_col(id: u64, name: &str) -> Column {
    Column {
        id: ColumnId(id),
        name: name.to_string(),
        data_type: "bigint".to_string(),
        nullable: false,
        is_primary_key: false,
        comment: None,
        enum_values: None,
        semantics: relune_core::ColumnSemantics::default(),
    }
}

fn make_fk(
    name: &str,
    from_cols: &[&str],
    to_schema: Option<&str>,
    to_table: &str,
    to_cols: &[&str],
) -> ForeignKey {
    ForeignKey {
        name: Some(name.to_string()),
        from_columns: from_cols
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        to_schema: to_schema.map(ToString::to_string),
        to_table: to_table.to_string(),
        to_columns: to_cols
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        on_delete: ReferentialAction::NoAction,
        on_update: ReferentialAction::NoAction,
    }
}

// ============================================================================
// Scenario 1: Multi-schema FK resolution
// ============================================================================

#[test]
fn multi_schema_fk_resolves_across_schemas() {
    let schema = Schema {
        tables: vec![
            make_table(
                1,
                "auth.users",
                Some("auth"),
                "users",
                vec![pk_col(1, "id")],
                vec![],
                vec![],
            ),
            make_table(
                2,
                "public.orders",
                Some("public"),
                "orders",
                vec![pk_col(1, "id"), fk_col(2, "user_id")],
                vec![make_fk(
                    "fk_orders_user",
                    &["user_id"],
                    Some("auth"),
                    "users",
                    &["id"],
                )],
                vec![],
            ),
        ],
        views: vec![],
        enums: vec![],
    };

    // Lint: cross-schema FK should not fire unresolved-FK.
    let lint_result = lint_schema(&schema);
    assert!(
        !lint_result
            .issues
            .iter()
            .any(|i| i.rule_id == relune_core::LintRuleId::UnresolvedForeignKey),
        "cross-schema FK should be resolved"
    );
}

// ============================================================================
// Scenario 2: Circular FK detection
// ============================================================================

#[test]
fn circular_fk_detected_by_lint() {
    let schema = Schema {
        tables: vec![
            make_table(
                1,
                "public.a",
                Some("public"),
                "a",
                vec![pk_col(1, "id"), fk_col(2, "b_id")],
                vec![make_fk("fk_a_b", &["b_id"], None, "b", &["id"])],
                vec![],
            ),
            make_table(
                2,
                "public.b",
                Some("public"),
                "b",
                vec![pk_col(1, "id"), fk_col(2, "a_id")],
                vec![make_fk("fk_b_a", &["a_id"], None, "a", &["id"])],
                vec![],
            ),
        ],
        views: vec![],
        enums: vec![],
    };

    let lint_result = lint_schema(&schema);
    let circular_issues: Vec<_> = lint_result
        .issues
        .iter()
        .filter(|i| i.rule_id == relune_core::LintRuleId::CircularForeignKey)
        .collect();
    assert!(
        !circular_issues.is_empty(),
        "circular FK between a and b should be detected"
    );

    // Both tables should be flagged.
    let flagged: std::collections::HashSet<String> = circular_issues
        .iter()
        .filter_map(|i| i.table_id.clone())
        .collect();
    assert!(flagged.contains("public.a"), "table a should be flagged");
    assert!(flagged.contains("public.b"), "table b should be flagged");
}

// ============================================================================
// Scenario 3: Diff detects added and modified tables
// ============================================================================

#[test]
fn diff_detects_table_changes() {
    let before = Schema {
        tables: vec![make_table(
            1,
            "public.users",
            Some("public"),
            "users",
            vec![pk_col(1, "id")],
            vec![],
            vec![],
        )],
        views: vec![],
        enums: vec![],
    };

    let after = Schema {
        tables: vec![
            make_table(
                1,
                "public.users",
                Some("public"),
                "users",
                vec![pk_col(1, "id"), fk_col(2, "email")],
                vec![],
                vec![],
            ),
            make_table(
                2,
                "public.orders",
                Some("public"),
                "orders",
                vec![pk_col(1, "id")],
                vec![],
                vec![],
            ),
        ],
        views: vec![],
        enums: vec![],
    };

    let diff = diff_schemas(&before, &after);

    // "orders" was added
    assert!(
        diff.added_tables.contains(&"public.orders".to_string()),
        "orders should appear in added_tables"
    );

    // "users" was modified (column added)
    assert!(
        diff.modified_tables
            .iter()
            .any(|t| t.table_name == "public.users"),
        "users should appear in modified_tables"
    );
}
