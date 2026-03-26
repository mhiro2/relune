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
    ForeignKey {
        name: export.name.clone(),
        from_columns: export.from_columns.clone(),
        to_schema: export.to_schema.clone(),
        to_table: export.to_table.clone(),
        to_columns: export.to_columns.clone(),
        on_delete: parse_referential_action(export.on_delete.as_deref()),
        on_update: parse_referential_action(export.on_update.as_deref()),
    }
}

fn parse_referential_action(action: Option<&str>) -> crate::model::ReferentialAction {
    use crate::model::ReferentialAction;

    let normalized = action.map(str::trim).filter(|s| !s.is_empty());
    let normalized = normalized.map(str::to_ascii_uppercase);
    let normalized = normalized.as_deref();

    let stripped = normalized
        .and_then(|value| value.strip_prefix("ON DELETE "))
        .or_else(|| normalized.and_then(|value| value.strip_prefix("ON UPDATE ")))
        .unwrap_or_else(|| normalized.unwrap_or(""));

    if stripped.is_empty() || stripped == "NO ACTION" || stripped == "NOACTION" {
        ReferentialAction::NoAction
    } else {
        match stripped {
            "CASCADE" => ReferentialAction::Cascade,
            "SET NULL" => ReferentialAction::SetNull,
            "SET DEFAULT" => ReferentialAction::SetDefault,
            "RESTRICT" => ReferentialAction::Restrict,
            _ => ReferentialAction::NoAction,
        }
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
    use crate::model::{ColumnId, ReferentialAction, TableId};

    fn roundtrip_schema(schema: &Schema) -> SchemaExport {
        let exported = export_schema(schema);
        let imported = import_schema(&exported);
        export_schema(&imported)
    }

    fn make_column(id: u64, name: &str, data_type: &str, primary_key: bool) -> Column {
        Column {
            id: ColumnId(id),
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: false,
            is_primary_key: primary_key,
            comment: None,
        }
    }

    fn make_table(
        id: u64,
        stable_id: &str,
        schema_name: Option<&str>,
        name: &str,
        columns: Vec<Column>,
        foreign_keys: Vec<ForeignKey>,
    ) -> Table {
        Table {
            id: TableId(id),
            stable_id: stable_id.to_string(),
            schema_name: schema_name.map(ToString::to_string),
            name: name.to_string(),
            columns,
            foreign_keys,
            indexes: vec![],
            comment: None,
        }
    }

    fn make_fk(
        name: &str,
        from_columns: &[&str],
        to_schema: Option<&str>,
        to_table: &str,
        to_columns: &[&str],
        on_delete: ReferentialAction,
        on_update: ReferentialAction,
    ) -> ForeignKey {
        ForeignKey {
            name: Some(name.to_string()),
            from_columns: from_columns.iter().map(ToString::to_string).collect(),
            to_schema: to_schema.map(ToString::to_string),
            to_table: to_table.to_string(),
            to_columns: to_columns.iter().map(ToString::to_string).collect(),
            on_delete,
            on_update,
        }
    }

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

    #[test]
    fn test_export_import_roundtrip_empty_schema() {
        let schema = Schema {
            tables: vec![],
            views: vec![],
            enums: vec![],
        };

        assert_eq!(roundtrip_schema(&schema), export_schema(&schema));
    }

    #[test]
    fn test_export_import_roundtrip_schema_qualified_tables() {
        let schema = Schema {
            tables: vec![
                make_table(
                    1,
                    "public.users",
                    Some("public"),
                    "users",
                    vec![make_column(1, "id", "bigint", true)],
                    vec![],
                ),
                make_table(
                    2,
                    "audit.users",
                    Some("audit"),
                    "users",
                    vec![make_column(1, "id", "bigint", true)],
                    vec![],
                ),
            ],
            views: vec![],
            enums: vec![],
        };

        let export = roundtrip_schema(&schema);
        assert_eq!(export.tables.len(), 2);
        assert_eq!(export.tables[0].schema.as_deref(), Some("public"));
        assert_eq!(export.tables[1].schema.as_deref(), Some("audit"));
        assert_eq!(export.tables[0].id, "public.users");
        assert_eq!(export.tables[1].id, "audit.users");
    }

    #[test]
    fn test_export_import_roundtrip_referential_actions() {
        let schema = Schema {
            tables: vec![
                make_table(
                    1,
                    "public.accounts",
                    Some("public"),
                    "accounts",
                    vec![make_column(1, "id", "uuid", true)],
                    vec![],
                ),
                make_table(
                    2,
                    "public.sessions",
                    Some("public"),
                    "sessions",
                    vec![
                        make_column(1, "id", "uuid", true),
                        make_column(2, "account_id", "uuid", false),
                    ],
                    vec![make_fk(
                        "fk_sessions_account",
                        &["account_id"],
                        Some("public"),
                        "accounts",
                        &["id"],
                        ReferentialAction::Cascade,
                        ReferentialAction::SetNull,
                    )],
                ),
                make_table(
                    3,
                    "public.audit_logs",
                    Some("public"),
                    "audit_logs",
                    vec![
                        make_column(1, "id", "uuid", true),
                        make_column(2, "session_id", "uuid", false),
                    ],
                    vec![make_fk(
                        "fk_audit_logs_session",
                        &["session_id"],
                        Some("public"),
                        "sessions",
                        &["id"],
                        ReferentialAction::Restrict,
                        ReferentialAction::SetDefault,
                    )],
                ),
            ],
            views: vec![],
            enums: vec![],
        };

        let export = roundtrip_schema(&schema);
        assert_eq!(
            export.tables[1].foreign_keys[0].on_delete.as_deref(),
            Some("CASCADE")
        );
        assert_eq!(
            export.tables[1].foreign_keys[0].on_update.as_deref(),
            Some("SET NULL")
        );
        assert_eq!(
            export.tables[2].foreign_keys[0].on_delete.as_deref(),
            Some("RESTRICT")
        );
        assert_eq!(
            export.tables[2].foreign_keys[0].on_update.as_deref(),
            Some("SET DEFAULT")
        );
    }

    #[test]
    fn test_import_normalizes_referential_action_strings() {
        let export = SchemaExport::new(vec![
            TableExport {
                id: "public.accounts".to_string(),
                schema: Some("public".to_string()),
                name: "accounts".to_string(),
                columns: vec![ColumnExport {
                    name: "id".to_string(),
                    data_type: "uuid".to_string(),
                    nullable: false,
                    primary_key: true,
                }],
                foreign_keys: vec![],
                indexes: vec![],
            },
            TableExport {
                id: "public.sessions".to_string(),
                schema: Some("public".to_string()),
                name: "sessions".to_string(),
                columns: vec![
                    ColumnExport {
                        name: "id".to_string(),
                        data_type: "uuid".to_string(),
                        nullable: false,
                        primary_key: true,
                    },
                    ColumnExport {
                        name: "account_id".to_string(),
                        data_type: "uuid".to_string(),
                        nullable: false,
                        primary_key: false,
                    },
                ],
                foreign_keys: vec![ForeignKeyExport {
                    name: Some("fk_sessions_account".to_string()),
                    from_columns: vec!["account_id".to_string()],
                    to_schema: Some("public".to_string()),
                    to_table: "accounts".to_string(),
                    to_columns: vec!["id".to_string()],
                    on_delete: Some(" on delete cascade ".to_string()),
                    on_update: Some("set default".to_string()),
                }],
                indexes: vec![],
            },
        ]);

        let schema = import_schema(&export);
        let fk = &schema.tables[1].foreign_keys[0];
        assert_eq!(fk.on_delete, ReferentialAction::Cascade);
        assert_eq!(fk.on_update, ReferentialAction::SetDefault);

        let normalized = export_schema(&schema);
        assert_eq!(
            normalized.tables[1].foreign_keys[0].on_delete.as_deref(),
            Some("CASCADE")
        );
        assert_eq!(
            normalized.tables[1].foreign_keys[0].on_update.as_deref(),
            Some("SET DEFAULT")
        );
    }
}
