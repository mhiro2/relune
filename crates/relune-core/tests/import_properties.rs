//! Property tests for schema JSON import and the analyses that consume it.
//!
//! Schema JSON is an input format, so any document that deserializes must
//! import without panicking, and whatever import accepts must survive
//! validation, lint, diff, and every review rule in every dialect.

use proptest::prelude::*;
use proptest::sample::select;
use relune_core::export::{
    ColumnExport, EnumExport, ForeignKeyExport, IndexExport, SchemaExport, TableExport, ViewExport,
    export_schema, import_schema,
};
use relune_core::{
    CheckConstraint, ColumnSemantics, EffectiveDialect, GeneratedColumn, IdentitySpec, IndexColumn,
    IndexKey, NullsOrder, ReferentialAction, ReviewRuleId, Schema, SortOrder, diff_schemas,
    lint_schema, run_rules,
};

// Small pools so generated documents collide on names, differ only by case,
// or point at objects that do not exist.
const NAMES: &[&str] = &["users", "Users", "posts", "tags", "", "a.b", "テーブル"];
const SCHEMAS: &[&str] = &["public", "auth", ""];
const COLUMNS: &[&str] = &["id", "ID", "user_id", "name", "", "missing"];
const TYPES: &[&str] = &["bigint", "int", "text", "varchar(10)", "numeric(10,2)", ""];
const ACTIONS: &[ReferentialAction] = &[
    ReferentialAction::NoAction,
    ReferentialAction::Restrict,
    ReferentialAction::Cascade,
    ReferentialAction::SetNull,
    ReferentialAction::SetDefault,
];
const VERSIONS: &[&str] = &["2.0.0", "2.9", "2", "1.0.0", "3.0.0", "x", ""];

fn text(pool: &'static [&'static str]) -> impl Strategy<Value = String> {
    select(pool).prop_map(str::to_string)
}

fn optional(pool: &'static [&'static str]) -> impl Strategy<Value = Option<String>> {
    prop::option::of(text(pool))
}

fn names(pool: &'static [&'static str], max: usize) -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(text(pool), 0..max)
}

fn check_constraint() -> impl Strategy<Value = CheckConstraint> {
    (optional(NAMES), text(COLUMNS)).prop_map(|(name, column)| CheckConstraint {
        name,
        expression: format!("{column} > 0"),
    })
}

fn semantics() -> impl Strategy<Value = ColumnSemantics> {
    (
        optional(COLUMNS),
        prop::collection::vec(check_constraint(), 0..2),
        prop::option::of((text(COLUMNS), any::<bool>())),
        prop::option::of(any::<bool>()),
        any::<bool>(),
        optional(&["CURRENT_TIMESTAMP", ""]),
    )
        .prop_map(
            |(
                default_expression,
                check_constraints,
                generated,
                identity,
                auto_increment,
                on_update,
            )| {
                ColumnSemantics {
                    default_expression,
                    check_constraints,
                    generated: generated
                        .map(|(expression, stored)| GeneratedColumn { expression, stored }),
                    identity: identity.map(|always| IdentitySpec { always }),
                    auto_increment,
                    on_update,
                    ..ColumnSemantics::default()
                }
            },
        )
}

fn column() -> impl Strategy<Value = ColumnExport> {
    (
        text(COLUMNS),
        text(TYPES),
        any::<bool>(),
        any::<bool>(),
        optional(NAMES),
        prop::option::of(names(NAMES, 4)),
        semantics(),
    )
        .prop_map(
            |(name, data_type, nullable, primary_key, comment, enum_values, semantics)| {
                ColumnExport {
                    name,
                    data_type,
                    nullable,
                    primary_key,
                    comment,
                    enum_values,
                    semantics,
                }
            },
        )
}

fn foreign_key() -> impl Strategy<Value = ForeignKeyExport> {
    (
        optional(NAMES),
        names(COLUMNS, 3),
        optional(SCHEMAS),
        text(NAMES),
        names(COLUMNS, 3),
        select(ACTIONS),
        select(ACTIONS),
    )
        .prop_map(
            |(name, from_columns, to_schema, to_table, to_columns, on_delete, on_update)| {
                ForeignKeyExport {
                    name,
                    from_columns,
                    to_schema,
                    to_table,
                    to_columns,
                    on_delete,
                    on_update,
                }
            },
        )
}

fn index_key() -> impl Strategy<Value = IndexKey> {
    prop_oneof![
        (
            text(COLUMNS),
            prop::option::of(select(&[SortOrder::Asc, SortOrder::Desc][..])),
            prop::option::of(select(&[NullsOrder::First, NullsOrder::Last][..])),
            prop::option::of(0u32..16),
        )
            .prop_map(|(name, order, nulls, prefix_length)| IndexKey::Column(
                IndexColumn {
                    name,
                    order,
                    nulls,
                    prefix_length,
                }
            )),
        text(COLUMNS).prop_map(|column| IndexKey::Expression(format!("lower({column})"))),
    ]
}

fn index() -> impl Strategy<Value = IndexExport> {
    (
        optional(NAMES),
        prop::collection::vec(index_key(), 1..3),
        any::<bool>(),
        optional(COLUMNS),
        names(COLUMNS, 2),
        optional(&["btree", "hash", ""]),
    )
        .prop_map(
            |(name, key_parts, unique, predicate, included_columns, method)| IndexExport {
                name,
                key_parts,
                unique,
                predicate: predicate.map(|column| format!("{column} IS NOT NULL")),
                included_columns,
                method,
            },
        )
}

fn qualified_id(schema: Option<&str>, name: &str) -> String {
    schema.map_or_else(|| name.to_string(), |schema| format!("{schema}.{name}"))
}

fn table() -> impl Strategy<Value = TableExport> {
    (
        optional(SCHEMAS),
        text(NAMES),
        any::<bool>(),
        prop::collection::vec(column(), 0..5),
        prop::collection::vec(foreign_key(), 0..3),
        prop::collection::vec(index(), 0..3),
        optional(NAMES),
        prop::collection::vec(check_constraint(), 0..2),
    )
        .prop_map(
            |(
                schema,
                name,
                id_matches_name,
                columns,
                foreign_keys,
                indexes,
                primary_key_name,
                check_constraints,
            )| {
                // Usually derive the id from the name, as exports do, but
                // sometimes let a hand-edited document disagree.
                let id = if id_matches_name {
                    qualified_id(schema.as_deref(), &name)
                } else {
                    format!("{name}_id")
                };
                TableExport {
                    id,
                    schema,
                    name,
                    columns,
                    foreign_keys,
                    indexes,
                    primary_key_name,
                    comment: None,
                    check_constraints,
                }
            },
        )
}

fn view() -> impl Strategy<Value = ViewExport> {
    (
        optional(SCHEMAS),
        text(NAMES),
        prop::collection::vec(column(), 0..3),
    )
        .prop_map(|(schema, name, columns)| ViewExport {
            id: qualified_id(schema.as_deref(), &name),
            definition: Some(format!("SELECT * FROM {name}")),
            schema,
            name,
            columns,
        })
}

fn enum_export() -> impl Strategy<Value = EnumExport> {
    (optional(SCHEMAS), text(NAMES), names(COLUMNS, 4)).prop_map(|(schema, name, values)| {
        EnumExport {
            id: qualified_id(schema.as_deref(), &name),
            schema,
            name,
            values,
        }
    })
}

/// Keeps the first object for each id, so most documents get past import's
/// duplicate check and exercise the analyses behind it.
fn first_per_id<T>(items: Vec<T>, id: impl Fn(&T) -> &str) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(id(item).to_string()))
        .collect()
}

fn schema_export() -> impl Strategy<Value = SchemaExport> {
    (
        prop_oneof![5 => Just(SchemaExport::VERSION.to_string()), 1 => text(VERSIONS)],
        prop::collection::vec(table(), 0..6),
        prop::collection::vec(view(), 0..2),
        prop::collection::vec(enum_export(), 0..2),
        prop::bool::weighted(0.8),
    )
        .prop_map(|(version, tables, views, enums, unique_ids)| {
            if unique_ids {
                SchemaExport {
                    version,
                    tables: first_per_id(tables, |table| &table.id),
                    views: first_per_id(views, |view| &view.id),
                    enums: first_per_id(enums, |enum_type| &enum_type.id),
                }
            } else {
                SchemaExport {
                    version,
                    tables,
                    views,
                    enums,
                }
            }
        })
}

/// Imports a document that went through JSON, as CLI and WASM inputs do.
fn import_json(export: &SchemaExport) -> Option<Schema> {
    let json = serde_json::to_string(export).expect("schema export serializes");
    let parsed: SchemaExport = serde_json::from_str(&json).expect("schema export deserializes");
    import_schema(&parsed).ok()
}

const DIALECTS: [EffectiveDialect; 4] = [
    EffectiveDialect::Auto,
    EffectiveDialect::Postgres,
    EffectiveDialect::Mysql,
    EffectiveDialect::Sqlite,
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn imported_schemas_export_back_unchanged(export in schema_export()) {
        let Some(schema) = import_json(&export) else {
            return Ok(());
        };
        let _ = schema.validate();
        let normalized = export_schema(&schema);
        let reimported = import_schema(&normalized).expect("exported schemas import again");
        prop_assert_eq!(export_schema(&reimported), normalized);
        prop_assert!(diff_schemas(&schema, &schema).is_empty());
    }

    #[test]
    fn analyses_accept_any_imported_schema(
        before in schema_export(),
        after in schema_export(),
    ) {
        let (Some(before), Some(after)) = (import_json(&before), import_json(&after)) else {
            return Ok(());
        };
        let _ = lint_schema(&before);
        let _ = lint_schema(&after);
        let diff = diff_schemas(&before, &after);
        for dialect in DIALECTS {
            let _ = run_rules(&diff, &before, &after, ReviewRuleId::all_rules(), dialect);
        }
    }
}
