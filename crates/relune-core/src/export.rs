//! Stable export format for schema data.
//!
//! Types in this module are considered part of the public API contract
//! and should maintain backwards compatibility.

use serde::{Deserialize, Serialize};

use crate::model::{Column, ForeignKey, Index, Schema, Table};

/// Stable schema export format for JSON serialization.
/// This format is designed for long-term stability and should not
/// change in breaking ways between versions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaExport {
    /// Export format version.
    pub version: String,
    /// Tables in the schema.
    pub tables: Vec<TableExport>,
}

impl SchemaExport {
    /// Export format version.
    pub const VERSION: &'static str = "1.0.0";

    /// Creates a new schema export.
    #[must_use]
    pub fn new(tables: Vec<TableExport>) -> Self {
        Self {
            version: Self::VERSION.to_string(),
            tables,
        }
    }
}

/// Export format for a single table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableExport {
    /// Stable table identifier.
    pub id: String,
    /// Schema name, if qualified.
    pub schema: Option<String>,
    /// Table name.
    pub name: String,
    /// Columns in the table.
    pub columns: Vec<ColumnExport>,
    /// Foreign keys from this table.
    pub foreign_keys: Vec<ForeignKeyExport>,
    /// Indexes on this table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexExport>,
}

/// Export format for a column.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnExport {
    /// Column name.
    pub name: String,
    /// Data type name.
    pub data_type: String,
    /// Whether the column can be null.
    pub nullable: bool,
    /// Whether this column is part of the primary key.
    pub primary_key: bool,
}

/// Export format for a foreign key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForeignKeyExport {
    /// Constraint name, if named.
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
    #[serde(default, skip_serializing_if = "is_export_no_action")]
    pub on_delete: Option<String>,
    /// ON UPDATE action.
    #[serde(default, skip_serializing_if = "is_export_no_action")]
    pub on_update: Option<String>,
}

#[allow(clippy::ref_option)] // serde skip_serializing_if requires &T
fn is_export_no_action(action: &Option<String>) -> bool {
    action.is_none() || action.as_deref() == Some("NO ACTION")
}

/// Export format for an index.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexExport {
    /// Index name, if named.
    pub name: Option<String>,
    /// Column names in the index.
    pub columns: Vec<String>,
    /// Whether the index is unique.
    pub unique: bool,
}

/// Export a Schema to a stable JSON format.
pub fn export_schema(schema: &Schema) -> SchemaExport {
    SchemaExport {
        version: SchemaExport::VERSION.to_string(),
        tables: schema.tables.iter().map(export_table).collect(),
    }
}

/// Export a single Table to the stable format.
fn export_table(table: &Table) -> TableExport {
    TableExport {
        id: table.stable_id.clone(),
        schema: table.schema_name.clone(),
        name: table.name.clone(),
        columns: table.columns.iter().map(export_column).collect(),
        foreign_keys: table.foreign_keys.iter().map(export_fk).collect(),
        indexes: table.indexes.iter().map(export_index).collect(),
    }
}

/// Export a Column to the stable format.
fn export_column(col: &Column) -> ColumnExport {
    ColumnExport {
        name: col.name.clone(),
        data_type: col.data_type.clone(),
        nullable: col.nullable,
        primary_key: col.is_primary_key,
    }
}

/// Export a `ForeignKey` to the stable format.
fn export_fk(fk: &ForeignKey) -> ForeignKeyExport {
    use crate::model::ReferentialAction;

    let to_action_str = |a: ReferentialAction| -> Option<String> {
        match a {
            ReferentialAction::NoAction => None,
            other => Some(other.to_string()),
        }
    };

    ForeignKeyExport {
        name: fk.name.clone(),
        from_columns: fk.from_columns.clone(),
        to_schema: fk.to_schema.clone(),
        to_table: fk.to_table.clone(),
        to_columns: fk.to_columns.clone(),
        on_delete: to_action_str(fk.on_delete),
        on_update: to_action_str(fk.on_update),
    }
}

/// Export an Index to the stable format.
fn export_index(idx: &Index) -> IndexExport {
    IndexExport {
        name: idx.name.clone(),
        columns: idx.columns.clone(),
        unique: idx.is_unique,
    }
}

/// Import a Schema from the stable JSON format.
pub fn import_schema(export: &SchemaExport) -> Schema {
    Schema {
        tables: export.tables.iter().map(import_table).collect(),
        views: vec![],
        enums: vec![],
    }
}

/// Import a single Table from the stable format.
fn import_table(export: &TableExport) -> Table {
    use crate::model::TableId;

    Table {
        id: TableId(0), // Stable ID is in stable_id field
        stable_id: export.id.clone(),
        schema_name: export.schema.clone(),
        name: export.name.clone(),
        columns: export
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| import_column(i, c))
            .collect(),
        foreign_keys: export.foreign_keys.iter().map(import_fk).collect(),
        indexes: export.indexes.iter().map(import_index).collect(),
        comment: None,
    }
}

/// Import a Column from the stable format.
fn import_column(index: usize, export: &ColumnExport) -> Column {
    use crate::model::ColumnId;

    Column {
        id: ColumnId(index as u64),
        name: export.name.clone(),
        data_type: export.data_type.clone(),
        nullable: export.nullable,
        is_primary_key: export.primary_key,
        comment: None,
    }
}

/// Import a `ForeignKey` from the stable format.
fn import_fk(export: &ForeignKeyExport) -> ForeignKey {
    use crate::model::ReferentialAction;

    let from_action_str = |s: &Option<String>| -> ReferentialAction {
        match s.as_deref() {
            Some("CASCADE") => ReferentialAction::Cascade,
            Some("SET NULL") => ReferentialAction::SetNull,
            Some("SET DEFAULT") => ReferentialAction::SetDefault,
            Some("RESTRICT") => ReferentialAction::Restrict,
            _ => ReferentialAction::NoAction,
        }
    };

    ForeignKey {
        name: export.name.clone(),
        from_columns: export.from_columns.clone(),
        to_schema: export.to_schema.clone(),
        to_table: export.to_table.clone(),
        to_columns: export.to_columns.clone(),
        on_delete: from_action_str(&export.on_delete),
        on_update: from_action_str(&export.on_update),
    }
}

/// Import an Index from the stable format.
fn import_index(export: &IndexExport) -> Index {
    Index {
        name: export.name.clone(),
        columns: export.columns.clone(),
        is_unique: export.unique,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_export_roundtrip() {
        let export = SchemaExport::new(vec![TableExport {
            id: "users".to_string(),
            schema: Some("public".to_string()),
            name: "users".to_string(),
            columns: vec![
                ColumnExport {
                    name: "id".to_string(),
                    data_type: "bigint".to_string(),
                    nullable: false,
                    primary_key: true,
                },
                ColumnExport {
                    name: "email".to_string(),
                    data_type: "varchar(255)".to_string(),
                    nullable: false,
                    primary_key: false,
                },
            ],
            foreign_keys: vec![],
            indexes: vec![IndexExport {
                name: Some("idx_email".to_string()),
                columns: vec!["email".to_string()],
                unique: true,
            }],
        }]);

        let json = serde_json::to_string(&export).unwrap();
        let parsed: SchemaExport = serde_json::from_str(&json).unwrap();
        assert_eq!(export, parsed);
    }

    #[test]
    fn test_export_version() {
        let export = SchemaExport::new(vec![]);
        assert_eq!(export.version, "1.0.0");
    }
}
