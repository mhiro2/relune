//! Core data model for database schema representation.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    /// - Duplicate table names
    /// - Empty table or column names
    /// - Empty column data types
    /// - FK `from_columns` / `to_columns` referencing nonexistent columns
    /// - FK `from_columns` / `to_columns` count mismatch
    /// - FK `to_table` referencing a nonexistent table
    #[must_use]
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        // Check for duplicate table names (case-insensitive)
        let mut seen_names: HashSet<String> = HashSet::new();
        for table in &self.tables {
            let lower = table.name.to_lowercase();
            if !seen_names.insert(lower) {
                errors.push(ValidationError {
                    table: Some(table.name.clone()),
                    message: "duplicate table name".to_string(),
                });
            }
        }

        // Build set of known table names for FK target validation
        let known_tables: HashSet<String> = self
            .tables
            .iter()
            .flat_map(|t| {
                let mut names = vec![t.name.to_lowercase()];
                names.push(t.stable_id.to_lowercase());
                names
            })
            .collect();

        for table in &self.tables {
            // Empty table name
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

                // from_columns reference existing columns
                for col in &fk.from_columns {
                    if !col_names.contains(col) {
                        errors.push(ValidationError {
                            table: Some(table.name.clone()),
                            message: format!("FK from_column '{col}' does not exist in table"),
                        });
                    }
                }

                // to_table references a known table
                if !known_tables.contains(&fk.to_table.to_lowercase()) {
                    errors.push(ValidationError {
                        table: Some(table.name.clone()),
                        message: format!("FK references unknown table '{}'", fk.to_table),
                    });
                }
            }
        }

        errors
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
