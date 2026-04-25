//! Common types and mapping logic shared across database introspection modules.
//!
//! This module provides database-agnostic raw metadata types and functions to
//! convert them into `relune-core` `Schema` types. Each database-specific module
//! (postgres, mysql, sqlite) queries its own catalog/metadata and produces these
//! common raw types, which are then mapped uniformly.

use std::collections::{HashMap, HashSet};

use relune_core::{
    Column, ColumnId, Enum, ForeignKey, Index, ReferentialAction, Schema, Table, TableId, View,
};

use crate::error::IntrospectError;

// ============================================================================
// Raw metadata types
// ============================================================================

/// Raw table metadata from a database catalog.
#[derive(Debug, Clone)]
pub struct RawTable {
    /// Name of the table.
    pub table_name: String,
    /// Schema name containing the table.
    pub schema_name: String,
    /// Optional comment on the table.
    pub table_comment: Option<String>,
}

/// Raw column metadata from a database catalog.
#[derive(Debug, Clone)]
pub struct RawColumn {
    /// Name of the relation containing the column.
    pub table_name: String,
    /// Schema name containing the relation.
    pub schema_name: String,
    /// Name of the column.
    pub column_name: String,
    /// Data type of the column (e.g., "integer", "varchar(255)").
    pub data_type: String,
    /// Whether the column allows NULL values.
    pub is_nullable: bool,
    /// Whether the column is part of the primary key.
    pub is_primary_key: bool,
    /// Optional comment on the column.
    pub column_comment: Option<String>,
    /// Position of the column in the table (1-based).
    pub ordinal_position: i16,
}

/// Raw foreign key metadata from a database catalog.
#[derive(Debug, Clone)]
pub struct RawForeignKey {
    /// Name of the foreign key constraint.
    pub constraint_name: String,
    /// Schema name containing the constraint.
    pub schema_name: String,
    /// Name of the table that contains the foreign key.
    pub from_table: String,
    /// Names of the columns in the source table.
    pub from_columns: Vec<String>,
    /// Schema name of the referenced table.
    pub to_schema: Option<String>,
    /// Name of the referenced table.
    pub to_table: String,
    /// Names of the columns in the referenced table.
    pub to_columns: Vec<String>,
    /// ON DELETE referential action.
    pub on_delete: ReferentialAction,
    /// ON UPDATE referential action.
    pub on_update: ReferentialAction,
}

/// Parse a referential action string (as returned by `information_schema` / PRAGMA /
/// `pg_constraint`) into a [`ReferentialAction`]. Unrecognised values fall back to `NoAction`.
///
/// Accepts:
/// - Full names: `CASCADE`, `SET NULL`, `SET DEFAULT`, `RESTRICT`, `NO ACTION` (case-insensitive)
/// - `PostgreSQL` `pg_constraint` single-char codes: `a`/`r`/`c`/`n`/`d`
#[must_use]
pub fn parse_referential_action(s: &str) -> ReferentialAction {
    let trimmed = s.trim();
    // PostgreSQL pg_constraint single-char codes (confdeltype / confupdtype)
    match trimmed {
        "a" => return ReferentialAction::NoAction,
        "r" => return ReferentialAction::Restrict,
        "c" => return ReferentialAction::Cascade,
        "n" => return ReferentialAction::SetNull,
        "d" => return ReferentialAction::SetDefault,
        _ => {}
    }
    match trimmed.to_uppercase().as_str() {
        "CASCADE" => ReferentialAction::Cascade,
        "SET NULL" => ReferentialAction::SetNull,
        "SET DEFAULT" => ReferentialAction::SetDefault,
        "RESTRICT" => ReferentialAction::Restrict,
        _ => ReferentialAction::NoAction,
    }
}

/// Raw index metadata from a database catalog.
#[derive(Debug, Clone)]
pub struct RawIndex {
    /// Name of the index.
    pub index_name: String,
    /// Schema name containing the index.
    pub schema_name: String,
    /// Name of the table the index is on.
    pub table_name: String,
    /// Names of the columns in the index (in order).
    pub columns: Vec<String>,
    /// Whether the index is unique.
    pub is_unique: bool,
    /// Whether the index is the primary key.
    pub is_primary: bool,
}

/// Raw view metadata from a database catalog.
#[derive(Debug, Clone)]
pub struct RawView {
    /// Name of the view.
    pub view_name: String,
    /// Schema name containing the view.
    pub schema_name: String,
    /// View definition (the SELECT statement).
    pub definition: Option<String>,
    /// Optional comment on the view.
    pub view_comment: Option<String>,
}

/// Raw enum type metadata from a database catalog.
#[derive(Debug, Clone)]
pub struct RawEnum {
    /// Name of the enum type.
    pub enum_name: String,
    /// Schema name containing the enum type.
    pub schema_name: String,
    /// Values of the enum type (in order).
    pub values: Vec<String>,
}

/// Aggregated raw schema data from a database catalog.
#[derive(Debug, Clone, Default)]
pub struct RawSchema {
    /// All tables in the database.
    pub tables: Vec<RawTable>,
    /// All columns in the database.
    pub columns: Vec<RawColumn>,
    /// All foreign keys in the database.
    pub foreign_keys: Vec<RawForeignKey>,
    /// All indexes in the database.
    pub indexes: Vec<RawIndex>,
    /// All views in the database.
    pub views: Vec<RawView>,
    /// All enum types in the database.
    pub enums: Vec<RawEnum>,
}

// ============================================================================
// Mapping functions
// ============================================================================

/// Generates a stable hash-based ID from a string using FNV-1a.
///
/// Unlike `DefaultHasher`, FNV-1a produces identical output across
/// Rust toolchain versions, so IDs remain stable for diff and caching.
fn generate_stable_id(input: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = FNV_OFFSET_BASIS;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Builds a human-readable stable identifier from schema and object name.
///
/// Components that contain `.` are quoted so that `("a.b", "c")` produces
/// `"a.b".c` instead of the ambiguous `a.b.c`.
fn qualified_stable_id(schema_name: &str, object_name: &str) -> String {
    fn quote_if_needed(s: &str) -> std::borrow::Cow<'_, str> {
        if s.contains('.') {
            std::borrow::Cow::Owned(format!("\"{s}\""))
        } else {
            std::borrow::Cow::Borrowed(s)
        }
    }
    format!(
        "{}.{}",
        quote_if_needed(schema_name),
        quote_if_needed(object_name)
    )
}

/// Generates a `TableId` from schema name and table name.
///
/// Uses `\0` as separator to avoid collisions when names contain `.`
/// (e.g. `PostgreSQL` quoted identifiers).
fn generate_table_id(schema_name: &str, table_name: &str) -> TableId {
    let stable_id = format!("{schema_name}\0{table_name}");
    TableId(generate_stable_id(&stable_id))
}

/// Generates a `ColumnId` from table stable id and column name.
///
/// Uses `\0` as separator to avoid collisions when names contain `.`.
fn generate_column_id(table_stable_id: &str, column_name: &str) -> ColumnId {
    let full_id = format!("{table_stable_id}\0{column_name}");
    ColumnId(generate_stable_id(&full_id))
}

/// Converts raw catalog data (`RawSchema`) to a `Schema`.
pub fn map_to_schema(raw_schema: RawSchema) -> Result<Schema, IntrospectError> {
    let RawSchema {
        tables,
        columns,
        foreign_keys,
        indexes,
        views,
        enums,
    } = raw_schema;
    map_schema(tables, &columns, &foreign_keys, &indexes, views, enums)
}

/// Maps raw database catalog data to a complete `Schema`.
pub fn map_schema(
    tables: Vec<RawTable>,
    columns: &[RawColumn],
    foreign_keys: &[RawForeignKey],
    indexes: &[RawIndex],
    views: Vec<RawView>,
    enums: Vec<RawEnum>,
) -> Result<Schema, IntrospectError> {
    // Build a set of primary key column identifiers for quick lookup
    let pk_set: HashSet<(&str, &str, &str)> = columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| {
            (
                c.schema_name.as_str(),
                c.table_name.as_str(),
                c.column_name.as_str(),
            )
        })
        .collect();

    // Group columns by relation name
    let mut columns_by_relation: HashMap<(&str, &str), Vec<&RawColumn>> = HashMap::new();
    for col in columns {
        columns_by_relation
            .entry((col.schema_name.as_str(), col.table_name.as_str()))
            .or_default()
            .push(col);
    }

    // Group foreign keys by table
    let mut fks_by_table: HashMap<(&str, &str), Vec<&RawForeignKey>> = HashMap::new();
    for fk in foreign_keys {
        fks_by_table
            .entry((fk.schema_name.as_str(), fk.from_table.as_str()))
            .or_default()
            .push(fk);
    }

    // Group indexes by table
    let mut indexes_by_table: HashMap<(&str, &str), Vec<&RawIndex>> = HashMap::new();
    for idx in indexes {
        indexes_by_table
            .entry((idx.schema_name.as_str(), idx.table_name.as_str()))
            .or_default()
            .push(idx);
    }

    // Map tables
    let mapped_tables: Vec<Table> = tables
        .into_iter()
        .map(|raw_table| {
            let key = (
                raw_table.schema_name.as_str(),
                raw_table.table_name.as_str(),
            );
            let table_columns = columns_by_relation.get(&key).cloned().unwrap_or_default();
            let table_fks = fks_by_table.get(&key).cloned().unwrap_or_default();
            let table_indexes = indexes_by_table.get(&key).cloned().unwrap_or_default();

            map_table(raw_table, table_columns, &pk_set, table_fks, table_indexes)
        })
        .collect();

    // Map views
    let mapped_views: Vec<View> = views
        .into_iter()
        .map(|raw_view| {
            let key = (raw_view.schema_name.as_str(), raw_view.view_name.as_str());
            let view_columns = columns_by_relation.get(&key).cloned().unwrap_or_default();

            map_view(raw_view, view_columns)
        })
        .collect();

    // Map enums
    let mapped_enums: Vec<Enum> = enums.into_iter().map(map_enum).collect();

    Ok(Schema {
        tables: mapped_tables,
        views: mapped_views,
        enums: mapped_enums,
    })
}

fn map_table(
    raw_table: RawTable,
    columns: Vec<&RawColumn>,
    pk_set: &HashSet<(&str, &str, &str)>,
    foreign_keys: Vec<&RawForeignKey>,
    indexes: Vec<&RawIndex>,
) -> Table {
    let stable_id = qualified_stable_id(&raw_table.schema_name, &raw_table.table_name);
    let id = generate_table_id(&raw_table.schema_name, &raw_table.table_name);

    let mapped_columns: Vec<Column> = columns
        .into_iter()
        .map(|col| {
            let is_pk = pk_set.contains(&(
                raw_table.schema_name.as_str(),
                raw_table.table_name.as_str(),
                col.column_name.as_str(),
            ));
            map_column(col, &stable_id, is_pk)
        })
        .collect();

    let mapped_fks: Vec<ForeignKey> = foreign_keys.into_iter().map(map_foreign_key).collect();

    let mapped_indexes: Vec<Index> = indexes
        .into_iter()
        .filter(|idx| !idx.is_primary)
        .map(map_index)
        .collect();

    Table {
        id,
        stable_id,
        schema_name: Some(raw_table.schema_name),
        name: raw_table.table_name,
        columns: mapped_columns,
        foreign_keys: mapped_fks,
        indexes: mapped_indexes,
        comment: raw_table.table_comment,
    }
}

fn map_column(raw_column: &RawColumn, table_stable_id: &str, is_primary_key: bool) -> Column {
    Column {
        id: generate_column_id(table_stable_id, &raw_column.column_name),
        name: raw_column.column_name.clone(),
        data_type: raw_column.data_type.clone(),
        nullable: raw_column.is_nullable,
        is_primary_key,
        comment: raw_column.column_comment.clone(),
    }
}

fn map_foreign_key(raw_fk: &RawForeignKey) -> ForeignKey {
    ForeignKey {
        name: Some(raw_fk.constraint_name.clone()),
        from_columns: raw_fk.from_columns.clone(),
        to_schema: raw_fk.to_schema.clone(),
        to_table: raw_fk.to_table.clone(),
        to_columns: raw_fk.to_columns.clone(),
        on_delete: raw_fk.on_delete,
        on_update: raw_fk.on_update,
    }
}

fn map_index(raw_index: &RawIndex) -> Index {
    Index {
        name: Some(raw_index.index_name.clone()),
        columns: raw_index.columns.clone(),
        is_unique: raw_index.is_unique,
    }
}

fn map_view(raw_view: RawView, columns: Vec<&RawColumn>) -> View {
    let id = qualified_stable_id(&raw_view.schema_name, &raw_view.view_name);
    let mapped_columns = columns
        .into_iter()
        .map(|column| map_column(column, &id, false))
        .collect();

    View {
        id,
        schema_name: Some(raw_view.schema_name),
        name: raw_view.view_name,
        columns: mapped_columns,
        definition: raw_view.definition,
    }
}

fn map_enum(raw_enum: RawEnum) -> Enum {
    let id = qualified_stable_id(&raw_enum.schema_name, &raw_enum.enum_name);

    Enum {
        id,
        schema_name: Some(raw_enum.schema_name),
        name: raw_enum.enum_name,
        values: raw_enum.values,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_schema_populates_view_columns() {
        let schema = map_schema(
            vec![RawTable {
                table_name: "users".to_string(),
                schema_name: "public".to_string(),
                table_comment: None,
            }],
            &[
                RawColumn {
                    table_name: "users".to_string(),
                    schema_name: "public".to_string(),
                    column_name: "id".to_string(),
                    data_type: "integer".to_string(),
                    is_nullable: false,
                    is_primary_key: true,
                    column_comment: None,
                    ordinal_position: 1,
                },
                RawColumn {
                    table_name: "active_users".to_string(),
                    schema_name: "public".to_string(),
                    column_name: "id".to_string(),
                    data_type: "integer".to_string(),
                    is_nullable: false,
                    is_primary_key: false,
                    column_comment: None,
                    ordinal_position: 1,
                },
                RawColumn {
                    table_name: "active_users".to_string(),
                    schema_name: "public".to_string(),
                    column_name: "email".to_string(),
                    data_type: "text".to_string(),
                    is_nullable: false,
                    is_primary_key: false,
                    column_comment: None,
                    ordinal_position: 2,
                },
            ],
            &[],
            &[],
            vec![RawView {
                view_name: "active_users".to_string(),
                schema_name: "public".to_string(),
                definition: Some("SELECT id, email FROM users".to_string()),
                view_comment: None,
            }],
            vec![],
        )
        .expect("schema mapping should succeed");

        let view = schema.views.first().expect("view should be mapped");
        assert_eq!(view.columns.len(), 2);
        assert_eq!(view.columns[0].name, "id");
        assert_eq!(view.columns[1].name, "email");

        let table = schema.tables.first().expect("table should be mapped");
        assert!(table.columns[0].is_primary_key);
    }

    #[test]
    fn generate_stable_id_is_deterministic() {
        // Fixed expected values guarantee the hash algorithm is version-stable.
        assert_eq!(generate_stable_id("public.users"), 0x10a3_9729_896e_6dda);
        assert_eq!(
            generate_stable_id("public.users"),
            generate_stable_id("public.users")
        );
        assert_ne!(
            generate_stable_id("public.users"),
            generate_stable_id("public.orders")
        );
    }
}
