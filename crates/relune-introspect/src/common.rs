//! Common types and mapping logic shared across database introspection modules.
//!
//! This module provides database-agnostic raw metadata types and functions to
//! convert them into `relune-core` `Schema` types. Each database-specific module
//! (postgres, mysql, sqlite) queries its own catalog/metadata and produces these
//! common raw types, which are then mapped uniformly.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

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
    /// Name of the table containing the column.
    pub table_name: String,
    /// Schema name containing the table.
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

/// Generates a stable hash-based ID from a string.
fn generate_stable_id(input: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

/// Generates a `TableId` from schema name and table name.
fn generate_table_id(schema_name: &str, table_name: &str) -> TableId {
    let stable_id = format!("{schema_name}.{table_name}");
    TableId(generate_stable_id(&stable_id))
}

/// Generates a `ColumnId` from table stable id and column name.
fn generate_column_id(table_stable_id: &str, column_name: &str) -> ColumnId {
    let full_id = format!("{table_stable_id}.{column_name}");
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
    let pk_set: HashSet<(String, String, String)> = columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| {
            (
                c.schema_name.clone(),
                c.table_name.clone(),
                c.column_name.clone(),
            )
        })
        .collect();

    // Group columns by table
    let mut columns_by_table: HashMap<(String, String), Vec<&RawColumn>> = HashMap::new();
    for col in columns {
        columns_by_table
            .entry((col.schema_name.clone(), col.table_name.clone()))
            .or_default()
            .push(col);
    }

    // Group foreign keys by table
    let mut fks_by_table: HashMap<(String, String), Vec<&RawForeignKey>> = HashMap::new();
    for fk in foreign_keys {
        fks_by_table
            .entry((fk.schema_name.clone(), fk.from_table.clone()))
            .or_default()
            .push(fk);
    }

    // Group indexes by table
    let mut indexes_by_table: HashMap<(String, String), Vec<&RawIndex>> = HashMap::new();
    for idx in indexes {
        indexes_by_table
            .entry((idx.schema_name.clone(), idx.table_name.clone()))
            .or_default()
            .push(idx);
    }

    // Map tables
    let mapped_tables: Vec<Table> = tables
        .into_iter()
        .map(|raw_table| {
            let key = (raw_table.schema_name.clone(), raw_table.table_name.clone());
            let table_columns = columns_by_table.remove(&key).unwrap_or_default();
            let table_fks = fks_by_table.remove(&key).unwrap_or_default();
            let table_indexes = indexes_by_table.remove(&key).unwrap_or_default();

            map_table(raw_table, table_columns, &pk_set, table_fks, table_indexes)
        })
        .collect();

    // Map views
    let mapped_views: Vec<View> = views.into_iter().map(map_view).collect();

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
    pk_set: &HashSet<(String, String, String)>,
    foreign_keys: Vec<&RawForeignKey>,
    indexes: Vec<&RawIndex>,
) -> Table {
    let stable_id = format!("{}.{}", raw_table.schema_name, raw_table.table_name);
    let id = generate_table_id(&raw_table.schema_name, &raw_table.table_name);

    let mapped_columns: Vec<Column> = columns
        .into_iter()
        .map(|col| {
            let is_pk = pk_set.contains(&(
                raw_table.schema_name.clone(),
                raw_table.table_name.clone(),
                col.column_name.clone(),
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
        on_delete: ReferentialAction::NoAction,
        on_update: ReferentialAction::NoAction,
    }
}

fn map_index(raw_index: &RawIndex) -> Index {
    Index {
        name: Some(raw_index.index_name.clone()),
        columns: raw_index.columns.clone(),
        is_unique: raw_index.is_unique,
    }
}

fn map_view(raw_view: RawView) -> View {
    let id = format!("{}.{}", raw_view.schema_name, raw_view.view_name);

    View {
        id,
        schema_name: Some(raw_view.schema_name),
        name: raw_view.view_name,
        columns: Vec::new(),
        definition: raw_view.definition,
    }
}

fn map_enum(raw_enum: RawEnum) -> Enum {
    let id = format!("{}.{}", raw_enum.schema_name, raw_enum.enum_name);

    Enum {
        id,
        schema_name: Some(raw_enum.schema_name),
        name: raw_enum.enum_name,
        values: raw_enum.values,
    }
}
