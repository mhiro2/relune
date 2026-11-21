//! Ordered migration SQL as a sequence of operations.
//!
//! Review compares schema snapshots, which say what changed but not how.
//! A [`MigrationScript`] records the statements that make the change, in
//! execution order, with the transaction each one runs in and where it is
//! written, so review can tell `CREATE INDEX CONCURRENTLY` from a blocking
//! build, or a column rename from a drop and re-add with the same result.

use serde::{Deserialize, Serialize};

use crate::model::{Schema, Table, normalize_identifier, qualified_identifier};

/// Where a migration statement is written.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceLocation {
    /// Name of the migration file, as given by the caller.
    pub file: String,
    /// 1-based line the statement starts on.
    pub line: usize,
    /// 1-based column the statement starts at, telling apart statements
    /// that share a line.
    pub column: usize,
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.file, self.line)
    }
}

/// How migration files are wrapped in transactions when they run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransactionMode {
    /// Only `BEGIN` / `START TRANSACTION` ... `COMMIT` / `END` / `ROLLBACK`
    /// in the SQL open transaction blocks; other statements autocommit.
    #[default]
    Explicit,
    /// The migration tool runs each file inside one transaction.
    PerFile,
}

impl std::fmt::Display for TransactionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Explicit => "explicit",
            Self::PerFile => "per-file",
        })
    }
}

/// Migration statements in execution order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationScript {
    /// How the statements were assigned to transactions.
    pub transaction_mode: TransactionMode,
    /// Statements across every file, in the order they run.
    pub statements: Vec<MigrationStatement>,
}

/// One statement of a migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationStatement {
    /// Where the statement is written.
    pub location: SourceLocation,
    /// Statement text as written, without the trailing semicolon.
    pub sql: String,
    /// Transaction block the statement runs in; `None` when it runs on its
    /// own (autocommit). Statements sharing a value share one transaction.
    pub transaction: Option<usize>,
    /// What the statement does, in order; an `ALTER TABLE` may do several
    /// things.
    pub operations: Vec<MigrationOperation>,
}

/// Kind of constraint added by `ALTER TABLE ... ADD CONSTRAINT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    /// `FOREIGN KEY`.
    ForeignKey,
    /// `CHECK`.
    Check,
    /// `UNIQUE`.
    Unique,
    /// `PRIMARY KEY`.
    PrimaryKey,
}

/// What a migration statement does. Table names are normalized and keep
/// the schema qualification written in the SQL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MigrationOperation {
    /// `BEGIN` / `START TRANSACTION`.
    Begin,
    /// `COMMIT` / `END`.
    Commit,
    /// `ROLLBACK`.
    Rollback,
    /// `CREATE [UNIQUE] INDEX [CONCURRENTLY]`.
    CreateIndex {
        /// Indexed table.
        table: String,
        /// Index name, if given.
        name: Option<String>,
        /// Plain key columns.
        columns: Vec<String>,
        /// Whether the index is unique.
        unique: bool,
        /// Whether the build runs `CONCURRENTLY`.
        concurrently: bool,
    },
    /// `DROP INDEX [CONCURRENTLY]`.
    DropIndex {
        /// Dropped index names.
        names: Vec<String>,
        /// Whether the drop runs `CONCURRENTLY`.
        concurrently: bool,
    },
    /// `REINDEX [CONCURRENTLY]`.
    Reindex {
        /// Whether the rebuild runs `CONCURRENTLY`.
        concurrently: bool,
    },
    /// `ALTER TABLE ... ADD [CONSTRAINT name] ...`.
    AddConstraint {
        /// Table the constraint is added to.
        table: String,
        /// Constraint name, if given.
        name: Option<String>,
        /// Constraint kind.
        kind: ConstraintKind,
        /// Constrained columns (referencing columns of a foreign key).
        columns: Vec<String>,
        /// `CHECK` expression, normalized to lowercase.
        expression: Option<String>,
        /// Index a `UNIQUE` / `PRIMARY KEY` is built from (`USING INDEX`).
        using_index: Option<String>,
        /// Whether the constraint is added `NOT VALID`.
        not_valid: bool,
    },
    /// `ALTER TABLE ... VALIDATE CONSTRAINT`.
    ValidateConstraint {
        /// Table that owns the constraint.
        table: String,
        /// Validated constraint.
        name: String,
    },
    /// `ALTER TABLE ... DROP CONSTRAINT`.
    DropConstraint {
        /// Table that owns the constraint.
        table: String,
        /// Dropped constraint.
        name: String,
    },
    /// `ALTER TABLE ... ALTER COLUMN ... SET NOT NULL`.
    SetNotNull {
        /// Table of the column.
        table: String,
        /// Column made NOT NULL.
        column: String,
    },
    /// `ALTER TABLE ... ALTER COLUMN ... TYPE`.
    AlterColumnType {
        /// Table of the column.
        table: String,
        /// Column whose type changes.
        column: String,
    },
    /// `ALTER TABLE ... ADD COLUMN`.
    AddColumn {
        /// Table the column is added to.
        table: String,
        /// Added column.
        column: String,
    },
    /// `ALTER TABLE ... DROP COLUMN`.
    DropColumn {
        /// Table the column is dropped from.
        table: String,
        /// Dropped column.
        column: String,
    },
    /// `ALTER TABLE ... RENAME COLUMN`.
    RenameColumn {
        /// Table of the column.
        table: String,
        /// Name before the rename.
        from: String,
        /// Name after the rename.
        to: String,
    },
    /// `ALTER TABLE ... RENAME TO` / `RENAME TABLE`.
    RenameTable {
        /// Table name before the rename.
        from: String,
        /// Table name after the rename.
        to: String,
    },
    /// `CREATE TABLE`.
    CreateTable {
        /// Created table.
        table: String,
    },
    /// `DROP TABLE`.
    DropTable {
        /// Dropped tables.
        tables: Vec<String>,
    },
    /// `UPDATE`, such as a backfill. Whether it covers every row is not
    /// known from the SQL alone.
    Update {
        /// Updated table.
        table: String,
    },
    /// A statement that does not change what review checks, such as
    /// `SET lock_timeout` or `COMMENT ON`.
    Other,
    /// A statement Relune could not analyze; changes it makes are not
    /// reviewed.
    Unanalyzed {
        /// Why the statement was not analyzed.
        reason: String,
    },
}

impl MigrationStatement {
    /// Returns true when the statement could not be analyzed.
    #[must_use]
    pub fn is_unanalyzed(&self) -> bool {
        self.operations
            .iter()
            .any(|operation| matches!(operation, MigrationOperation::Unanalyzed { .. }))
    }
}

impl MigrationScript {
    /// Statements that could not be analyzed.
    pub fn unanalyzed(&self) -> impl Iterator<Item = &MigrationStatement> {
        self.statements
            .iter()
            .filter(|statement| statement.is_unanalyzed())
    }

    /// Every operation with the index of its statement, in execution order.
    pub fn operations(&self) -> impl Iterator<Item = (usize, &MigrationOperation)> {
        self.statements
            .iter()
            .enumerate()
            .flat_map(|(index, statement)| {
                statement
                    .operations
                    .iter()
                    .map(move |operation| (index, operation))
            })
    }

    /// Applies the table and column renames of the script to `schema`, in
    /// order, so a schema taken before the migration lines up with the one
    /// taken after it: a renamed column then compares as the same column
    /// instead of a dropped one and an added one.
    ///
    /// Foreign keys, indexes, and the referencing side of other tables'
    /// foreign keys follow each rename. Renames of tables or columns the
    /// schema does not have are ignored.
    pub fn apply_renames(&self, schema: &mut Schema) {
        for (_, operation) in self.operations() {
            match operation {
                MigrationOperation::RenameTable { from, to } => rename_table(schema, from, to),
                MigrationOperation::RenameColumn { table, from, to } => {
                    rename_column(schema, table, from, to);
                }
                _ => {}
            }
        }
    }
}

/// Splits a normalized, possibly qualified table name into schema and name.
#[must_use]
pub fn split_table_name(name: &str) -> (Option<&str>, &str) {
    name.rsplit_once('.')
        .map_or((None, name), |(schema, table)| (Some(schema), table))
}

/// Schema names review strips before comparing (`PostgreSQL` and `SQLite`).
const DEFAULT_SCHEMAS: [&str; 2] = ["public", "main"];

/// The table a name written in migration SQL refers to, or `None` when no
/// table, or more than one, could be meant.
///
/// An exact `stable_id` match wins. A name qualified with the default
/// schema also names the unqualified table, since review strips that
/// schema before comparing; another qualifier must match exactly. An
/// unqualified name may still find the one qualified table of that name
/// through the search path, but never one of several.
#[must_use]
pub fn find_table<'a>(schema: &'a Schema, name: &str) -> Option<&'a Table> {
    if let Some(table) = schema
        .tables
        .iter()
        .find(|table| table.stable_id.eq_ignore_ascii_case(name))
    {
        return Some(table);
    }
    let (written_schema, bare) = split_table_name(name);
    let named = |table: &&Table| table.name.eq_ignore_ascii_case(bare);
    let candidates: Vec<&Table> = match written_schema {
        Some(written)
            if DEFAULT_SCHEMAS
                .iter()
                .any(|default| default.eq_ignore_ascii_case(written)) =>
        {
            schema
                .tables
                .iter()
                .filter(named)
                .filter(|table| table.schema_name.is_none())
                .collect()
        }
        Some(_) => return None,
        None => schema.tables.iter().filter(named).collect(),
    };
    match candidates.as_slice() {
        [table] => Some(table),
        _ => None,
    }
}

/// Name given to an object that a rename replaces: the migration must have
/// dropped it first, so it stays in the comparison as a dropped object.
fn replaced_name(name: &str) -> String {
    format!("{name} (dropped)")
}

fn rename_table(schema: &mut Schema, from: &str, to: &str) {
    let Some(index) = find_table(schema, from).and_then(|found| {
        schema
            .tables
            .iter()
            .position(|table| table.stable_id == found.stable_id)
    }) else {
        return;
    };
    let old_name = schema.tables[index].name.clone();
    let old_schema = schema.tables[index].schema_name.clone();
    let (_, new_name) = split_table_name(to);
    let new_name = normalize_identifier(new_name);

    // A table already holding the new name was dropped earlier in the
    // migration; it must not merge with the renamed one.
    let new_id = qualified_identifier(old_schema.as_deref(), &new_name);
    if let Some(taken) = schema
        .tables
        .iter_mut()
        .find(|table| table.stable_id == new_id)
    {
        taken.name = replaced_name(&taken.name);
        taken.stable_id = qualified_identifier(taken.schema_name.as_deref(), &taken.name);
    }

    let table = &mut schema.tables[index];
    table.name.clone_from(&new_name);
    table.stable_id = qualified_identifier(table.schema_name.as_deref(), &new_name);

    for table in &mut schema.tables {
        for fk in &mut table.foreign_keys {
            let same_schema = match (&fk.to_schema, &old_schema) {
                (Some(target), Some(old)) => target.eq_ignore_ascii_case(old),
                (Some(_), None) => false,
                (None, _) => true,
            };
            if same_schema && fk.to_table.eq_ignore_ascii_case(&old_name) {
                fk.to_table.clone_from(&new_name);
            }
        }
    }
}

fn rename_column(schema: &mut Schema, table_name: &str, from: &str, to: &str) {
    let Some(found) = find_table(schema, table_name) else {
        return;
    };
    let (stable_id, owner_name, owner_schema) = (
        found.stable_id.clone(),
        found.name.clone(),
        found.schema_name.clone(),
    );
    let from = normalize_identifier(from);
    let to = normalize_identifier(to);

    // A column already holding the new name was dropped earlier in the
    // migration; it stays apart so its loss is still reported.
    if let Some(owner) = schema
        .tables
        .iter_mut()
        .find(|table| table.stable_id == stable_id)
        && owner
            .columns
            .iter()
            .any(|column| column.name.eq_ignore_ascii_case(&from))
        && let Some(taken) = owner
            .columns
            .iter_mut()
            .find(|column| column.name.eq_ignore_ascii_case(&to))
    {
        taken.name = replaced_name(&taken.name);
    }
    let rename = |name: &mut String| {
        if name.eq_ignore_ascii_case(&from) {
            name.clone_from(&to);
        }
    };

    for table in &mut schema.tables {
        if table.stable_id == stable_id {
            for column in &mut table.columns {
                rename(&mut column.name);
            }
            for fk in &mut table.foreign_keys {
                fk.from_columns.iter_mut().for_each(rename);
            }
            for index in &mut table.indexes {
                for part in &mut index.key_parts {
                    if let crate::model::IndexKey::Column(column) = part {
                        rename(&mut column.name);
                    }
                }
                index.included_columns.iter_mut().for_each(rename);
            }
        }
        for fk in &mut table.foreign_keys {
            let targets_owner = fk.to_table.eq_ignore_ascii_case(&owner_name)
                && match (&fk.to_schema, &owner_schema) {
                    (Some(target), Some(owner)) => target.eq_ignore_ascii_case(owner),
                    (Some(_), None) => false,
                    (None, _) => true,
                };
            if targets_owner {
                fk.to_columns.iter_mut().for_each(rename);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, ColumnId, ForeignKey, Index, TableId};

    fn column(name: &str) -> Column {
        Column {
            id: ColumnId(0),
            name: name.to_string(),
            data_type: "int".to_string(),
            nullable: true,
            is_primary_key: false,
            comment: None,
            enum_values: None,
            semantics: crate::model::ColumnSemantics::default(),
        }
    }

    fn table(name: &str, columns: &[&str]) -> Table {
        Table {
            id: TableId(0),
            stable_id: name.to_string(),
            schema_name: None,
            name: name.to_string(),
            columns: columns.iter().map(|name| column(name)).collect(),
            foreign_keys: Vec::new(),
            indexes: Vec::new(),
            primary_key_name: None,
            comment: None,
            check_constraints: Vec::new(),
        }
    }

    fn script(operations: Vec<MigrationOperation>) -> MigrationScript {
        MigrationScript {
            transaction_mode: TransactionMode::Explicit,
            statements: vec![MigrationStatement {
                location: SourceLocation {
                    file: "001.sql".to_string(),
                    line: 1,
                    column: 1,
                },
                sql: String::new(),
                transaction: None,
                operations,
            }],
        }
    }

    fn schema() -> Schema {
        let mut users = table("users", &["id", "name"]);
        users
            .indexes
            .push(Index::from_columns(None, ["name".to_string()], false));
        let mut orders = table("orders", &["id", "user_name"]);
        orders.foreign_keys.push(ForeignKey {
            name: None,
            from_columns: vec!["user_name".to_string()],
            to_schema: None,
            to_table: "users".to_string(),
            to_columns: vec!["name".to_string()],
            on_delete: crate::model::ReferentialAction::NoAction,
            on_update: crate::model::ReferentialAction::NoAction,
        });
        Schema {
            tables: vec![users, orders],
            views: Vec::new(),
            enums: Vec::new(),
        }
    }

    #[test]
    fn column_rename_follows_indexes_and_referencing_foreign_keys() {
        let mut schema = schema();
        script(vec![MigrationOperation::RenameColumn {
            table: "public.users".to_string(),
            from: "name".to_string(),
            to: "full_name".to_string(),
        }])
        .apply_renames(&mut schema);

        let users = &schema.tables[0];
        assert_eq!(users.columns[1].name, "full_name");
        assert_eq!(users.indexes[0].column_names(), ["full_name"]);
        assert_eq!(schema.tables[1].foreign_keys[0].to_columns, ["full_name"]);
    }

    #[test]
    fn table_rename_updates_identity_and_references() {
        let mut schema = schema();
        script(vec![MigrationOperation::RenameTable {
            from: "users".to_string(),
            to: "members".to_string(),
        }])
        .apply_renames(&mut schema);

        assert_eq!(schema.tables[0].name, "members");
        assert_eq!(schema.tables[0].stable_id, "members");
        assert_eq!(schema.tables[1].foreign_keys[0].to_table, "members");
    }

    #[test]
    fn renames_of_unknown_objects_are_ignored() {
        let mut schema = schema();
        let before = schema.clone();
        script(vec![
            MigrationOperation::RenameTable {
                from: "missing".to_string(),
                to: "other".to_string(),
            },
            MigrationOperation::RenameColumn {
                table: "users".to_string(),
                from: "missing".to_string(),
                to: "other".to_string(),
            },
        ])
        .apply_renames(&mut schema);
        assert_eq!(
            serde_json::to_value(crate::export::export_schema(&schema)).unwrap(),
            serde_json::to_value(crate::export::export_schema(&before)).unwrap()
        );
    }

    #[test]
    fn table_names_resolve_without_guessing_between_schemas() {
        let mut audit = table("orders", &["id"]);
        audit.schema_name = Some("audit".to_string());
        audit.stable_id = "audit.orders".to_string();
        let mut sales = audit.clone();
        sales.schema_name = Some("sales".to_string());
        sales.stable_id = "sales.orders".to_string();
        let schema = Schema {
            tables: vec![table("users", &["id"]), audit, sales],
            views: Vec::new(),
            enums: Vec::new(),
        };

        assert_eq!(
            find_table(&schema, "public.users").map(|t| t.stable_id.as_str()),
            Some("users")
        );
        assert_eq!(
            find_table(&schema, "users").map(|t| t.stable_id.as_str()),
            Some("users")
        );
        assert!(find_table(&schema, "billing.users").is_none());
        assert_eq!(
            find_table(&schema, "audit.orders").map(|t| t.stable_id.as_str()),
            Some("audit.orders")
        );
        // Two schemas hold an `orders`; the search path decides, so neither is assumed.
        assert!(find_table(&schema, "orders").is_none());
    }

    #[test]
    fn a_rename_onto_a_dropped_name_keeps_the_dropped_column_apart() {
        let mut schema = schema();
        script(vec![
            MigrationOperation::DropColumn {
                table: "users".to_string(),
                column: "name".to_string(),
            },
            MigrationOperation::RenameColumn {
                table: "users".to_string(),
                from: "id".to_string(),
                to: "name".to_string(),
            },
        ])
        .apply_renames(&mut schema);

        let names: Vec<&str> = schema.tables[0]
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect();
        assert_eq!(names, ["name", "name (dropped)"]);
    }
}
