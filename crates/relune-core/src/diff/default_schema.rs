//! Default-schema alignment for comparing schemas from different sources.
//!
//! DDL written without schema qualifiers parses into unqualified objects
//! (`schema_name = None`), while database introspection always reports the
//! owning schema (`public.users`). Diff matches objects by identity, so
//! comparing the two directly would report every table as removed and
//! re-added. [`align_default_schema`] rewrites objects that live in the
//! default schema as unqualified on both sides before they are compared.

use std::collections::{HashMap, HashSet};

use crate::model::{Schema, SqlDialect, qualified_identifier};

/// Returns the schema that unqualified names resolve to for `dialect`.
///
/// `PostgreSQL` uses `public` and `SQLite` uses `main`. `MySQL` has no
/// fixed name because its schema is the connected database, so it is
/// inferred when every object in `schema` belongs to the same schema.
/// `Auto` and unknown sources (`None`, e.g. schema JSON) have no default.
#[must_use]
pub fn default_schema_name(dialect: Option<SqlDialect>, schema: &Schema) -> Option<String> {
    match dialect? {
        SqlDialect::Postgres => Some("public".to_string()),
        SqlDialect::Sqlite => Some("main".to_string()),
        SqlDialect::Mysql => single_schema_name(schema),
        SqlDialect::Auto => None,
    }
}

/// Aligns schema qualification between `before` and `after` so that
/// unqualified objects and objects in the default schema compare as the
/// same object.
///
/// Alignment only runs when at least one side contains an unqualified
/// table, view, or enum; fully qualified inputs keep their names. Each
/// side strips its own default from [`default_schema_name`]. A side
/// without a dialect (schema JSON) has no default of its own and strips
/// the other side's default instead.
///
/// A table stays qualified when its side already has an unqualified table
/// of the same name, or when a foreign key from another schema that owns a
/// homonym references it explicitly (dropping the qualifier would make
/// that reference resolve to the homonym).
///
/// Returns the default schema names that were stripped from at least one
/// object, in lowercase.
pub fn align_default_schema(
    before: &mut Schema,
    before_dialect: Option<SqlDialect>,
    after: &mut Schema,
    after_dialect: Option<SqlDialect>,
) -> Vec<String> {
    if !has_unqualified_objects(before) && !has_unqualified_objects(after) {
        return Vec::new();
    }

    let before_default = default_schema_name(before_dialect, before);
    let after_default = default_schema_name(after_dialect, after);
    let before_strip = if before_dialect.is_some() {
        before_default.clone()
    } else {
        after_default.clone()
    };
    let after_strip = if after_dialect.is_some() {
        after_default
    } else {
        before_default
    };

    let mut stripped = Vec::new();
    for (schema, default) in [(before, before_strip), (after, after_strip)] {
        if let Some(default) = default.map(|name| name.to_lowercase())
            && unqualify_schema(schema, &default)
            && !stripped.contains(&default)
        {
            stripped.push(default);
        }
    }
    stripped
}

fn has_unqualified_objects(schema: &Schema) -> bool {
    schema
        .tables
        .iter()
        .any(|table| table.schema_name.is_none())
        || schema.views.iter().any(|view| view.schema_name.is_none())
        || schema.enums.iter().any(|enum_| enum_.schema_name.is_none())
}

/// Returns the only schema name used by `schema`, when every object is
/// qualified with the same schema (compared case-insensitively).
fn single_schema_name(schema: &Schema) -> Option<String> {
    let mut names = schema
        .tables
        .iter()
        .map(|table| table.schema_name.as_deref())
        .chain(schema.views.iter().map(|view| view.schema_name.as_deref()))
        .chain(
            schema
                .enums
                .iter()
                .map(|enum_| enum_.schema_name.as_deref()),
        );
    let first = names.next()??;
    names
        .all(|name| name.is_some_and(|name| name.eq_ignore_ascii_case(first)))
        .then(|| first.to_string())
}

fn in_schema(schema_name: Option<&str>, default: &str) -> bool {
    schema_name.is_some_and(|name| name.eq_ignore_ascii_case(default))
}

/// Drops the `schema.` prefix from an identifier built with
/// [`qualified_identifier`]. Identifiers that do not carry the prefix
/// (e.g. custom ids from schema JSON) are left as they are.
fn strip_qualifier(id: &mut String, schema_name: &str) {
    let prefix = qualified_identifier(Some(schema_name), "");
    if let Some(rest) = id.strip_prefix(&prefix) {
        *id = rest.to_string();
    }
}

/// Rewrites objects in the `default` schema of `schema` as unqualified.
/// Returns whether anything changed.
fn unqualify_schema(schema: &mut Schema, default: &str) -> bool {
    let mut changed = false;

    let unqualified_tables: HashSet<String> = schema
        .tables
        .iter()
        .filter(|table| table.schema_name.is_none())
        .map(|table| table.name.to_lowercase())
        .collect();
    let mut candidates: HashSet<String> = schema
        .tables
        .iter()
        .filter(|table| in_schema(table.schema_name.as_deref(), default))
        .map(|table| table.name.to_lowercase())
        .filter(|name| !unqualified_tables.contains(name))
        .collect();

    // An unqualified reference resolves against the owner's schema first.
    // When an owner outside the default schema has a homonym of an
    // explicitly referenced default-schema table, the reference must keep
    // its qualifier, so the target has to stay qualified as well.
    let qualified_tables: HashSet<(String, String)> = schema
        .tables
        .iter()
        .filter_map(|table| {
            let schema_name = table.schema_name.as_deref()?;
            Some((schema_name.to_lowercase(), table.name.to_lowercase()))
        })
        .collect();
    for table in &schema.tables {
        let Some(owner) = table.schema_name.as_deref().map(str::to_lowercase) else {
            continue;
        };
        if owner == default {
            continue;
        }
        for fk in &table.foreign_keys {
            let target = fk.to_table.to_lowercase();
            if in_schema(fk.to_schema.as_deref(), default)
                && qualified_tables.contains(&(owner.clone(), target.clone()))
            {
                candidates.remove(&target);
            }
        }
    }

    // References may name a table or its stable id, so map the name and the
    // stable id (before and after stripping) of every table that loses its
    // qualifier to the reference that still resolves afterwards.
    let mut stripped_refs: HashMap<String, Option<String>> = HashMap::new();
    for table in &mut schema.tables {
        if in_schema(table.schema_name.as_deref(), default)
            && candidates.contains(&table.name.to_lowercase())
            && let Some(schema_name) = table.schema_name.take()
        {
            let old_stable_id = table.stable_id.to_lowercase();
            strip_qualifier(&mut table.stable_id, &schema_name);
            stripped_refs.insert(table.name.to_lowercase(), None);
            stripped_refs.insert(table.stable_id.to_lowercase(), None);
            if old_stable_id != table.stable_id.to_lowercase() {
                stripped_refs.insert(old_stable_id, Some(table.stable_id.clone()));
            }
            changed = true;
        }
    }
    for table in &mut schema.tables {
        for fk in &mut table.foreign_keys {
            if !in_schema(fk.to_schema.as_deref(), default) {
                continue;
            }
            if let Some(renamed) = stripped_refs.get(&fk.to_table.to_lowercase()) {
                if let Some(stable_id) = renamed {
                    fk.to_table.clone_from(stable_id);
                }
                fk.to_schema = None;
                changed = true;
            }
        }
    }

    let mut unqualified_views: HashSet<String> = schema
        .views
        .iter()
        .filter(|view| view.schema_name.is_none())
        .map(|view| view.name.to_lowercase())
        .collect();
    for view in &mut schema.views {
        if in_schema(view.schema_name.as_deref(), default)
            && unqualified_views.insert(view.name.to_lowercase())
            && let Some(schema_name) = view.schema_name.take()
        {
            strip_qualifier(&mut view.id, &schema_name);
            changed = true;
        }
    }

    let mut unqualified_enums: HashSet<String> = schema
        .enums
        .iter()
        .filter(|enum_| enum_.schema_name.is_none())
        .map(|enum_| enum_.name.to_lowercase())
        .collect();
    for enum_ in &mut schema.enums {
        if in_schema(enum_.schema_name.as_deref(), default)
            && unqualified_enums.insert(enum_.name.to_lowercase())
            && let Some(schema_name) = enum_.schema_name.take()
        {
            strip_qualifier(&mut enum_.id, &schema_name);
            changed = true;
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::diff_schemas;
    use crate::model::{
        Column, ColumnId, ColumnSemantics, Enum, ForeignKey, ReferentialAction, Table, TableId,
        View,
    };

    fn table(schema: Option<&str>, name: &str, fks: Vec<ForeignKey>) -> Table {
        Table {
            id: TableId(0),
            stable_id: qualified_identifier(schema, name),
            schema_name: schema.map(ToString::to_string),
            name: name.to_string(),
            columns: vec![Column {
                id: ColumnId(0),
                name: "id".to_string(),
                data_type: "integer".to_string(),
                nullable: false,
                is_primary_key: true,
                comment: None,
                enum_values: None,
                semantics: ColumnSemantics::default(),
            }],
            foreign_keys: fks,
            indexes: Vec::new(),
            primary_key_name: None,
            comment: None,
            check_constraints: Vec::new(),
        }
    }

    fn fk(to_schema: Option<&str>, to_table: &str) -> ForeignKey {
        ForeignKey {
            name: None,
            from_columns: vec!["id".to_string()],
            to_schema: to_schema.map(ToString::to_string),
            to_table: to_table.to_string(),
            to_columns: vec!["id".to_string()],
            on_delete: ReferentialAction::NoAction,
            on_update: ReferentialAction::NoAction,
        }
    }

    fn schema(tables: Vec<Table>) -> Schema {
        Schema {
            tables,
            views: Vec::new(),
            enums: Vec::new(),
        }
    }

    #[test]
    fn unqualified_ddl_matches_introspected_public_tables() {
        let mut ddl = schema(vec![
            table(None, "users", Vec::new()),
            table(None, "orders", vec![fk(None, "users")]),
        ]);
        let mut db = schema(vec![
            table(Some("public"), "users", Vec::new()),
            table(Some("public"), "orders", vec![fk(Some("public"), "users")]),
        ]);

        let aligned = align_default_schema(
            &mut ddl,
            Some(SqlDialect::Postgres),
            &mut db,
            Some(SqlDialect::Postgres),
        );

        assert_eq!(aligned, vec!["public".to_string()]);
        assert!(diff_schemas(&ddl, &db).is_empty());
        assert_eq!(db.tables[1].stable_id, "orders");
        assert_eq!(db.tables[1].foreign_keys[0].to_schema, None);
    }

    #[test]
    fn fully_qualified_inputs_keep_their_names() {
        let mut before = schema(vec![table(Some("public"), "users", Vec::new())]);
        let mut after = schema(vec![table(Some("public"), "users", Vec::new())]);

        let aligned = align_default_schema(
            &mut before,
            Some(SqlDialect::Postgres),
            &mut after,
            Some(SqlDialect::Postgres),
        );

        assert!(aligned.is_empty());
        assert_eq!(after.tables[0].qualified_name(), "public.users");
    }

    #[test]
    fn non_default_schemas_stay_qualified() {
        let mut ddl = schema(vec![table(None, "users", Vec::new())]);
        let mut db = schema(vec![
            table(Some("public"), "users", Vec::new()),
            table(Some("auth"), "users", Vec::new()),
        ]);

        align_default_schema(&mut ddl, None, &mut db, Some(SqlDialect::Postgres));

        let names: Vec<String> = db.tables.iter().map(Table::qualified_name).collect();
        assert_eq!(names, vec!["users".to_string(), "auth.users".to_string()]);
    }

    #[test]
    fn existing_unqualified_homonym_blocks_stripping() {
        let mut before = schema(vec![
            table(None, "users", Vec::new()),
            table(Some("public"), "users", Vec::new()),
        ]);
        let mut after = schema(vec![table(None, "users", Vec::new())]);

        align_default_schema(
            &mut before,
            Some(SqlDialect::Postgres),
            &mut after,
            Some(SqlDialect::Postgres),
        );

        assert_eq!(before.tables[1].qualified_name(), "public.users");
        assert!(before.validate().is_empty());
    }

    #[test]
    fn foreign_key_keeps_schema_when_owner_schema_has_homonym() {
        let mut before = schema(vec![
            table(Some("public"), "users", Vec::new()),
            table(Some("app"), "users", Vec::new()),
            table(Some("app"), "orders", vec![fk(Some("public"), "users")]),
        ]);
        let mut after = schema(vec![table(None, "users", Vec::new())]);

        align_default_schema(
            &mut before,
            Some(SqlDialect::Postgres),
            &mut after,
            Some(SqlDialect::Postgres),
        );

        assert_eq!(
            before.tables[2].foreign_keys[0].to_schema.as_deref(),
            Some("public")
        );
        assert_eq!(before.tables[0].qualified_name(), "public.users");
        assert!(before.validate().is_empty(), "{:?}", before.validate());
    }

    #[test]
    fn foreign_key_by_stable_id_follows_stripped_table() {
        let mut users = table(Some("public"), "users", Vec::new());
        users.stable_id = "user-v1".to_string();
        let mut before = schema(vec![
            users,
            table(
                Some("public"),
                "orders",
                vec![fk(Some("public"), "user-v1")],
            ),
        ]);
        let mut after = schema(vec![table(None, "users", Vec::new())]);

        align_default_schema(
            &mut before,
            Some(SqlDialect::Postgres),
            &mut after,
            Some(SqlDialect::Postgres),
        );

        assert_eq!(before.tables[1].foreign_keys[0].to_schema, None);
        assert!(before.validate().is_empty(), "{:?}", before.validate());
    }

    #[test]
    fn foreign_key_by_qualified_stable_id_follows_stripped_table() {
        let mut before = schema(vec![
            table(Some("public"), "users", Vec::new()),
            table(
                Some("public"),
                "orders",
                vec![fk(Some("public"), "public.users")],
            ),
        ]);
        let mut after = schema(vec![table(None, "users", Vec::new())]);

        align_default_schema(
            &mut before,
            Some(SqlDialect::Postgres),
            &mut after,
            Some(SqlDialect::Postgres),
        );

        let fk = &before.tables[1].foreign_keys[0];
        assert_eq!(
            (fk.to_schema.as_deref(), fk.to_table.as_str()),
            (None, "users")
        );
        assert!(before.validate().is_empty(), "{:?}", before.validate());
    }

    #[test]
    fn each_dialect_strips_only_its_own_default() {
        let mut pg = schema(vec![
            table(None, "users", Vec::new()),
            table(Some("main"), "accounts", Vec::new()),
        ]);
        let mut sqlite = schema(vec![table(Some("main"), "accounts", Vec::new())]);

        let aligned = align_default_schema(
            &mut pg,
            Some(SqlDialect::Postgres),
            &mut sqlite,
            Some(SqlDialect::Sqlite),
        );

        assert_eq!(aligned, vec!["main".to_string()]);
        assert_eq!(pg.tables[1].qualified_name(), "main.accounts");
        assert_eq!(sqlite.tables[0].qualified_name(), "accounts");
    }

    #[test]
    fn mysql_default_is_the_single_database_schema() {
        let mut ddl = schema(vec![table(None, "users", Vec::new())]);
        let mut db = schema(vec![table(Some("shop"), "users", Vec::new())]);

        let aligned = align_default_schema(
            &mut ddl,
            Some(SqlDialect::Mysql),
            &mut db,
            Some(SqlDialect::Mysql),
        );

        assert_eq!(aligned, vec!["shop".to_string()]);
        assert!(diff_schemas(&ddl, &db).is_empty());
    }

    #[test]
    fn schema_json_side_uses_the_other_sides_default() {
        let mut json = schema(vec![table(Some("public"), "users", Vec::new())]);
        let mut ddl = schema(vec![table(None, "users", Vec::new())]);

        align_default_schema(&mut json, None, &mut ddl, Some(SqlDialect::Postgres));

        assert!(diff_schemas(&json, &ddl).is_empty());
    }

    #[test]
    fn views_and_enums_are_aligned() {
        let mut ddl = Schema {
            tables: Vec::new(),
            views: vec![View {
                id: "active_users".to_string(),
                schema_name: None,
                name: "active_users".to_string(),
                columns: Vec::new(),
                definition: None,
            }],
            enums: vec![Enum {
                id: "mood".to_string(),
                schema_name: None,
                name: "mood".to_string(),
                values: vec!["happy".to_string()],
            }],
        };
        let mut db = Schema {
            tables: Vec::new(),
            views: vec![View {
                id: "public.active_users".to_string(),
                schema_name: Some("public".to_string()),
                name: "active_users".to_string(),
                columns: Vec::new(),
                definition: None,
            }],
            enums: vec![Enum {
                id: "public.mood".to_string(),
                schema_name: Some("public".to_string()),
                name: "mood".to_string(),
                values: vec!["happy".to_string()],
            }],
        };

        align_default_schema(
            &mut ddl,
            Some(SqlDialect::Postgres),
            &mut db,
            Some(SqlDialect::Postgres),
        );

        assert_eq!(db.views[0].id, "active_users");
        assert_eq!(db.enums[0].id, "mood");
        assert!(diff_schemas(&ddl, &db).is_empty());
    }
}
