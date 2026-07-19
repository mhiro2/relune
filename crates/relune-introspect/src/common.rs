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
#[allow(clippy::struct_excessive_bools)] // distinct catalog attributes, not a state enum
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
    /// `DEFAULT` expression, if any (canonical SQL text).
    pub default_expression: Option<String>,
    /// Generated/computed column expression, if any.
    pub generated_expression: Option<String>,
    /// Whether a generated column is `STORED` (`false` = `VIRTUAL`/unknown).
    pub generated_stored: bool,
    /// Identity column: `Some(true)` = `GENERATED ALWAYS`, `Some(false)` = `BY DEFAULT`.
    pub identity_always: Option<bool>,
    /// Whether the column carries an auto-increment attribute.
    pub auto_increment: bool,
    /// Column collation.
    pub collation: Option<String>,
    /// Column character set.
    pub character_set: Option<String>,
    /// `ON UPDATE` expression (e.g. `MySQL` `ON UPDATE CURRENT_TIMESTAMP`).
    pub on_update: Option<String>,
}

impl RawColumn {
    /// Constructs a `RawColumn` with the given core fields and no extended
    /// semantics. Dialect catalogs fill the semantic fields afterwards.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        table_name: String,
        schema_name: String,
        column_name: String,
        data_type: String,
        is_nullable: bool,
        is_primary_key: bool,
        column_comment: Option<String>,
        ordinal_position: i16,
    ) -> Self {
        Self {
            table_name,
            schema_name,
            column_name,
            data_type,
            is_nullable,
            is_primary_key,
            column_comment,
            ordinal_position,
            default_expression: None,
            generated_expression: None,
            generated_stored: false,
            identity_always: None,
            auto_increment: false,
            collation: None,
            character_set: None,
            on_update: None,
        }
    }
}

/// Raw `CHECK` constraint metadata from a database catalog.
#[derive(Debug, Clone)]
pub struct RawCheckConstraint {
    /// Schema name containing the table.
    pub schema_name: String,
    /// Table the constraint is defined on.
    pub table_name: String,
    /// Constraint name, if named.
    pub name: Option<String>,
    /// The check expression, as catalog-rendered SQL text.
    pub expression: String,
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

/// A single key part of a raw catalog index: a plain column or an expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawIndexKeyPart {
    /// A plain column reference (with optional indexed prefix length).
    Column {
        /// Column name.
        name: String,
        /// Indexed prefix length (e.g. `MySQL` `col(10)`), if any.
        prefix_length: Option<u32>,
    },
    /// A functional/expression key part, as catalog-rendered SQL text.
    Expression(String),
}

impl RawIndexKeyPart {
    /// Convenience constructor for a plain column key part.
    #[must_use]
    pub fn column(name: impl Into<String>) -> Self {
        Self::Column {
            name: name.into(),
            prefix_length: None,
        }
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
    /// Ordered key parts (columns and expressions).
    pub key_parts: Vec<RawIndexKeyPart>,
    /// Whether the index is unique.
    pub is_unique: bool,
    /// Whether the index is the primary key.
    pub is_primary: bool,
    /// Partial-index predicate (`WHERE ...`), if any.
    pub predicate: Option<String>,
    /// Non-key columns carried by the index (`INCLUDE (...)`).
    pub included_columns: Vec<String>,
    /// Index access method (e.g. `btree`, `gin`), if known.
    pub method: Option<String>,
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
    /// All table-level `CHECK` constraints in the database.
    pub checks: Vec<RawCheckConstraint>,
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
        checks,
    } = raw_schema;
    map_schema(
        tables,
        &columns,
        &foreign_keys,
        &indexes,
        views,
        enums,
        &checks,
    )
}

/// Maps raw database catalog data to a complete `Schema`.
#[allow(clippy::too_many_arguments)]
pub fn map_schema(
    tables: Vec<RawTable>,
    columns: &[RawColumn],
    foreign_keys: &[RawForeignKey],
    indexes: &[RawIndex],
    views: Vec<RawView>,
    enums: Vec<RawEnum>,
    checks: &[RawCheckConstraint],
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

    // Group CHECK constraints by table
    let mut checks_by_table: HashMap<(&str, &str), Vec<&RawCheckConstraint>> = HashMap::new();
    for check in checks {
        checks_by_table
            .entry((check.schema_name.as_str(), check.table_name.as_str()))
            .or_default()
            .push(check);
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
            let table_checks = checks_by_table.get(&key).cloned().unwrap_or_default();

            map_table(
                raw_table,
                table_columns,
                &pk_set,
                table_fks,
                table_indexes,
                table_checks,
            )
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
    checks: Vec<&RawCheckConstraint>,
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

    let mapped_checks: Vec<relune_core::CheckConstraint> = checks
        .into_iter()
        .map(|c| relune_core::CheckConstraint {
            name: c.name.clone(),
            expression: c.expression.clone(),
        })
        .collect();

    Table {
        id,
        stable_id,
        schema_name: Some(raw_table.schema_name),
        name: raw_table.table_name,
        columns: mapped_columns,
        foreign_keys: mapped_fks,
        indexes: mapped_indexes,
        primary_key_name: None,
        check_constraints: mapped_checks,
        comment: raw_table.table_comment,
    }
}

fn map_column(raw_column: &RawColumn, table_stable_id: &str, is_primary_key: bool) -> Column {
    use relune_core::{ColumnSemantics, GeneratedColumn, IdentitySpec};

    let semantics = ColumnSemantics {
        default_expression: raw_column.default_expression.clone(),
        // Catalogs expose CHECK constraints at table level; they are mapped onto
        // `Table::check_constraints` rather than split back onto columns.
        check_constraints: Vec::new(),
        generated: raw_column
            .generated_expression
            .clone()
            .map(|expression| GeneratedColumn {
                expression,
                stored: raw_column.generated_stored,
            }),
        identity: raw_column
            .identity_always
            .map(|always| IdentitySpec { always }),
        collation: raw_column.collation.clone(),
        character_set: raw_column.character_set.clone(),
        auto_increment: raw_column.auto_increment,
        on_update: raw_column.on_update.clone(),
    };

    Column {
        id: generate_column_id(table_stable_id, &raw_column.column_name),
        name: raw_column.column_name.clone(),
        data_type: raw_column.data_type.clone(),
        nullable: raw_column.is_nullable,
        is_primary_key,
        comment: raw_column.column_comment.clone(),
        enum_values: None,
        semantics,
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
    use relune_core::{IndexColumn, IndexKey};

    let key_parts = raw_index
        .key_parts
        .iter()
        .map(|part| match part {
            RawIndexKeyPart::Column {
                name,
                prefix_length,
            } => IndexKey::Column(IndexColumn {
                name: name.clone(),
                order: None,
                nulls: None,
                prefix_length: *prefix_length,
            }),
            RawIndexKeyPart::Expression(expr) => IndexKey::Expression(expr.clone()),
        })
        .collect();

    Index {
        name: Some(raw_index.index_name.clone()),
        key_parts,
        is_unique: raw_index.is_unique,
        predicate: raw_index.predicate.clone(),
        included_columns: raw_index.included_columns.clone(),
        method: raw_index.method.clone(),
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
                RawColumn::new(
                    "users".to_string(),
                    "public".to_string(),
                    "id".to_string(),
                    "integer".to_string(),
                    false,
                    true,
                    None,
                    1,
                ),
                RawColumn::new(
                    "active_users".to_string(),
                    "public".to_string(),
                    "id".to_string(),
                    "integer".to_string(),
                    false,
                    false,
                    None,
                    1,
                ),
                RawColumn::new(
                    "active_users".to_string(),
                    "public".to_string(),
                    "email".to_string(),
                    "text".to_string(),
                    false,
                    false,
                    None,
                    2,
                ),
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
            &[],
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
