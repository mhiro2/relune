//! Core data model for database schema representation.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;

/// SQL dialect for parsing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SqlDialect {
    /// Automatically detect the dialect from SQL content.
    #[default]
    Auto,
    /// `PostgreSQL` dialect.
    #[serde(alias = "pg")]
    Postgres,
    /// `MySQL` dialect.
    Mysql,
    /// `SQLite` dialect.
    Sqlite,
}

impl fmt::Display for SqlDialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Postgres => write!(f, "postgres"),
            Self::Mysql => write!(f, "mysql"),
            Self::Sqlite => write!(f, "sqlite"),
        }
    }
}

impl std::str::FromStr for SqlDialect {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "postgres" | "postgresql" | "pg" => Ok(Self::Postgres),
            "mysql" => Ok(Self::Mysql),
            "sqlite" | "sqlite3" => Ok(Self::Sqlite),
            _ => Err(format!(
                "unknown dialect: {s}. Expected: auto, postgres, mysql, sqlite"
            )),
        }
    }
}

/// Normalizes an identifier to a consistent casing.
/// Currently uses lowercase, but this can be extended.
#[must_use]
pub fn normalize_identifier(name: &str) -> String {
    name.to_lowercase()
}

/// Unique identifier for a table within a schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct TableId(pub u64);

impl fmt::Display for TableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TableId({})", self.0)
    }
}

/// Unique identifier for a column within a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColumnId(pub u64);

impl fmt::Display for ColumnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ColumnId({})", self.0)
    }
}

/// A complete database schema with tables, views, and enums.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Schema {
    /// Tables in the schema.
    pub tables: Vec<Table>,
    /// Views in the schema.
    pub views: Vec<View>,
    /// Enums in the schema.
    pub enums: Vec<Enum>,
}

/// A validation error found in a schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// The table where the error was found, if applicable.
    pub table: Option<String>,
    /// Description of the validation error.
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.table {
            Some(t) => write!(f, "table '{}': {}", t, self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

impl Schema {
    /// Validates the schema for structural consistency.
    ///
    /// Checks for:
    /// - Duplicate table names (schema-qualified)
    /// - Empty table or column names
    /// - Empty column data types
    /// - FK `from_columns` referencing nonexistent columns in the source table
    /// - FK `to_columns` referencing nonexistent columns in the target table
    /// - FK `from_columns` / `to_columns` count mismatch
    /// - FK `to_table` referencing a nonexistent table
    #[must_use]
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        // Check for duplicate table names using (schema, name) tuple
        let mut seen_names: HashSet<(String, String)> = HashSet::new();
        for table in &self.tables {
            let schema = table.schema_name.as_deref().unwrap_or("").to_lowercase();
            let name = table.name.to_lowercase();
            if !seen_names.insert((schema, name)) {
                errors.push(ValidationError {
                    table: Some(table.qualified_name()),
                    message: "duplicate table name".to_string(),
                });
            }
        }

        // Build lookup from (schema_lower, name_lower) and (schema_lower, stable_id_lower)
        // to the table's column names, for FK target validation.
        let mut table_columns: HashMap<(String, String), HashSet<String>> = HashMap::new();
        for t in &self.tables {
            let schema = t.schema_name.as_deref().unwrap_or("").to_lowercase();
            let cols: HashSet<String> = t.columns.iter().map(|c| c.name.clone()).collect();
            table_columns.insert((schema.clone(), t.name.to_lowercase()), cols.clone());
            table_columns.insert((schema, t.stable_id.to_lowercase()), cols);
        }

        for table in &self.tables {
            Self::validate_table(table, &table_columns, &mut errors);
        }

        errors
    }

    fn validate_table(
        table: &Table,
        table_columns: &HashMap<(String, String), HashSet<String>>,
        errors: &mut Vec<ValidationError>,
    ) {
        if table.name.trim().is_empty() {
            errors.push(ValidationError {
                table: None,
                message: "table has empty name".to_string(),
            });
        }

        let col_names: HashSet<String> = table.columns.iter().map(|c| c.name.clone()).collect();

        for col in &table.columns {
            if col.name.trim().is_empty() {
                errors.push(ValidationError {
                    table: Some(table.name.clone()),
                    message: "column has empty name".to_string(),
                });
            }
            if col.data_type.trim().is_empty() {
                errors.push(ValidationError {
                    table: Some(table.name.clone()),
                    message: format!("column '{}' has empty data_type", col.name),
                });
            }
        }

        for fk in &table.foreign_keys {
            Self::validate_fk(table, fk, &col_names, table_columns, errors);
        }
    }

    fn validate_fk(
        table: &Table,
        fk: &ForeignKey,
        col_names: &HashSet<String>,
        table_columns: &HashMap<(String, String), HashSet<String>>,
        errors: &mut Vec<ValidationError>,
    ) {
        // Column count mismatch
        if !fk.from_columns.is_empty()
            && !fk.to_columns.is_empty()
            && fk.from_columns.len() != fk.to_columns.len()
        {
            errors.push(ValidationError {
                table: Some(table.name.clone()),
                message: format!(
                    "FK column count mismatch: {} from_columns vs {} to_columns",
                    fk.from_columns.len(),
                    fk.to_columns.len()
                ),
            });
        }

        // from_columns reference existing columns in source table
        for col in &fk.from_columns {
            if !col_names.contains(col) {
                errors.push(ValidationError {
                    table: Some(table.name.clone()),
                    message: format!("FK from_column '{col}' does not exist in table"),
                });
            }
        }

        // Resolve the target table using (to_schema, to_table).
        let to_key = fk.to_table.to_lowercase();
        let target_cols = if fk.to_schema.is_some() {
            let target_schema = fk.to_schema.as_deref().unwrap_or("").to_lowercase();
            table_columns.get(&(target_schema, to_key))
        } else {
            // No explicit schema — try empty schema first, then any match
            table_columns
                .get(&(String::new(), to_key.clone()))
                .or_else(|| {
                    table_columns
                        .iter()
                        .find(|((_, n), _)| *n == to_key)
                        .map(|(_, v)| v)
                })
        };

        match target_cols {
            None => {
                errors.push(ValidationError {
                    table: Some(table.name.clone()),
                    message: format!("FK references unknown table '{}'", fk.to_table),
                });
            }
            Some(ref_cols) => {
                // to_columns reference existing columns in the target table
                for col in &fk.to_columns {
                    if !ref_cols.contains(col) {
                        errors.push(ValidationError {
                            table: Some(table.name.clone()),
                            message: format!(
                                "FK to_column '{col}' does not exist in table '{}'",
                                fk.to_table
                            ),
                        });
                    }
                }
            }
        }
    }

    /// Returns statistics about the schema.
    #[must_use]
    pub fn stats(&self) -> SchemaStats {
        let table_count = self.tables.len();
        let column_count = self.tables.iter().map(|t| t.columns.len()).sum();
        let foreign_key_count = self.tables.iter().map(|t| t.foreign_keys.len()).sum();
        let view_count = self.views.len();
        SchemaStats {
            table_count,
            column_count,
            foreign_key_count,
            view_count,
        }
    }
}

/// Statistics about a schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaStats {
    /// Number of tables.
    pub table_count: usize,
    /// Total number of columns across all tables.
    pub column_count: usize,
    /// Total number of foreign keys.
    pub foreign_key_count: usize,
    /// Number of views.
    pub view_count: usize,
}

/// A database table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    /// Internal table identifier.
    pub id: TableId,
    /// Stable identifier for export/import.
    pub stable_id: String,
    /// Schema name if qualified.
    pub schema_name: Option<String>,
    /// Table name.
    pub name: String,
    /// Columns in the table.
    pub columns: Vec<Column>,
    /// Foreign keys from this table.
    pub foreign_keys: Vec<ForeignKey>,
    /// Indexes on this table.
    pub indexes: Vec<Index>,
    /// Optional table comment.
    pub comment: Option<String>,
}

impl Table {
    /// Returns the qualified name (schema.table) or just the name.
    #[must_use]
    pub fn qualified_name(&self) -> String {
        match &self.schema_name {
            Some(schema_name) => format!("{}.{}", schema_name, self.name),
            None => self.name.clone(),
        }
    }
}

/// A column in a table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    /// Column identifier.
    pub id: ColumnId,
    /// Column name.
    pub name: String,
    /// Data type name.
    pub data_type: String,
    /// Whether the column can be null.
    pub nullable: bool,
    /// Whether this column is part of the primary key.
    pub is_primary_key: bool,
    /// Optional column comment.
    pub comment: Option<String>,
}

/// Referential action for ON DELETE / ON UPDATE clauses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferentialAction {
    /// No action specified.
    #[default]
    NoAction,
    /// Restrict deletion/update.
    Restrict,
    /// Cascade the change to referencing rows.
    Cascade,
    /// Set referencing columns to NULL.
    SetNull,
    /// Set referencing columns to their default values.
    SetDefault,
}

impl fmt::Display for ReferentialAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAction => write!(f, "NO ACTION"),
            Self::Restrict => write!(f, "RESTRICT"),
            Self::Cascade => write!(f, "CASCADE"),
            Self::SetNull => write!(f, "SET NULL"),
            Self::SetDefault => write!(f, "SET DEFAULT"),
        }
    }
}

/// A foreign key constraint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKey {
    /// Constraint name if named.
    pub name: Option<String>,
    /// Source column names.
    pub from_columns: Vec<String>,
    /// Target schema name, if cross-schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_schema: Option<String>,
    /// Target table name.
    pub to_table: String,
    /// Target column names.
    pub to_columns: Vec<String>,
    /// ON DELETE action.
    #[serde(default, skip_serializing_if = "is_no_action")]
    pub on_delete: ReferentialAction,
    /// ON UPDATE action.
    #[serde(default, skip_serializing_if = "is_no_action")]
    pub on_update: ReferentialAction,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde skip_serializing_if requires &T
fn is_no_action(action: &ReferentialAction) -> bool {
    *action == ReferentialAction::NoAction
}

/// An index on a table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Index {
    /// Index name if named.
    pub name: Option<String>,
    /// Column names in the index.
    pub columns: Vec<String>,
    /// Whether the index is unique.
    pub is_unique: bool,
}

/// A database view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct View {
    /// View identifier.
    pub id: String,
    /// Schema name if qualified.
    pub schema_name: Option<String>,
    /// View name.
    pub name: String,
    /// Columns in the view.
    pub columns: Vec<Column>,
    /// View definition SQL.
    pub definition: Option<String>,
}

impl View {
    /// Returns the qualified name (schema.view) or just the name.
    #[must_use]
    pub fn qualified_name(&self) -> String {
        match &self.schema_name {
            Some(schema_name) => format!("{}.{}", schema_name, self.name),
            None => self.name.clone(),
        }
    }
}

/// A database enum type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Enum {
    /// Enum identifier.
    pub id: String,
    /// Schema name if qualified.
    pub schema_name: Option<String>,
    /// Enum name.
    pub name: String,
    /// Enum values.
    pub values: Vec<String>,
}

impl Enum {
    /// Returns the qualified name (schema.enum) or just the name.
    #[must_use]
    pub fn qualified_name(&self) -> String {
        match &self.schema_name {
            Some(schema_name) => format!("{}.{}", schema_name, self.name),
            None => self.name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_table(name: &str, schema: Option<&str>, cols: &[&str], fks: Vec<ForeignKey>) -> Table {
        Table {
            id: TableId(0),
            stable_id: name.to_string(),
            schema_name: schema.map(ToString::to_string),
            name: name.to_string(),
            columns: cols
                .iter()
                .enumerate()
                .map(|(i, c)| Column {
                    id: ColumnId(i as u64),
                    name: (*c).to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                    is_primary_key: i == 0,
                    comment: None,
                })
                .collect(),
            foreign_keys: fks,
            indexes: vec![],
            comment: None,
        }
    }

    fn make_fk(to_table: &str, from_cols: &[&str], to_cols: &[&str]) -> ForeignKey {
        ForeignKey {
            name: None,
            from_columns: from_cols.iter().map(|c| (*c).to_string()).collect(),
            to_schema: None,
            to_table: to_table.to_string(),
            to_columns: to_cols.iter().map(|c| (*c).to_string()).collect(),
            on_delete: ReferentialAction::NoAction,
            on_update: ReferentialAction::NoAction,
        }
    }

    #[test]
    fn validate_clean_schema_returns_no_errors() {
        let schema = Schema {
            tables: vec![
                make_table("users", None, &["id", "name"], vec![]),
                make_table(
                    "posts",
                    None,
                    &["id", "user_id"],
                    vec![make_fk("users", &["user_id"], &["id"])],
                ),
            ],
            views: vec![],
            enums: vec![],
        };
        assert!(schema.validate().is_empty());
    }

    #[test]
    fn validate_same_name_different_schemas_is_ok() {
        let schema = Schema {
            tables: vec![
                make_table("users", Some("public"), &["id"], vec![]),
                make_table("users", Some("auth"), &["id"], vec![]),
            ],
            views: vec![],
            enums: vec![],
        };
        assert!(schema.validate().is_empty());
    }

    #[test]
    fn validate_duplicate_within_same_schema() {
        let schema = Schema {
            tables: vec![
                make_table("users", Some("public"), &["id"], vec![]),
                make_table("users", Some("public"), &["id"], vec![]),
            ],
            views: vec![],
            enums: vec![],
        };
        let errs = schema.validate();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("duplicate"));
    }

    #[test]
    fn validate_to_columns_referencing_nonexistent_column() {
        let schema = Schema {
            tables: vec![
                make_table("users", None, &["id"], vec![]),
                make_table(
                    "posts",
                    None,
                    &["id", "user_id"],
                    vec![make_fk("users", &["user_id"], &["nonexistent"])],
                ),
            ],
            views: vec![],
            enums: vec![],
        };
        let errs = schema.validate();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("to_column 'nonexistent'"));
    }

    #[test]
    fn validate_fk_unknown_table() {
        let schema = Schema {
            tables: vec![make_table(
                "posts",
                None,
                &["id", "user_id"],
                vec![make_fk("missing", &["user_id"], &["id"])],
            )],
            views: vec![],
            enums: vec![],
        };
        let errs = schema.validate();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("unknown table"));
    }

    #[test]
    fn validate_fk_from_column_nonexistent() {
        let schema = Schema {
            tables: vec![
                make_table("users", None, &["id"], vec![]),
                make_table(
                    "posts",
                    None,
                    &["id"],
                    vec![make_fk("users", &["ghost"], &["id"])],
                ),
            ],
            views: vec![],
            enums: vec![],
        };
        let errs = schema.validate();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("from_column 'ghost'"));
    }

    #[test]
    fn validate_fk_column_count_mismatch() {
        let schema = Schema {
            tables: vec![
                make_table("users", None, &["id", "name"], vec![]),
                make_table(
                    "posts",
                    None,
                    &["id", "user_id"],
                    vec![make_fk("users", &["user_id"], &["id", "name"])],
                ),
            ],
            views: vec![],
            enums: vec![],
        };
        let errs = schema.validate();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("count mismatch"));
    }
}
