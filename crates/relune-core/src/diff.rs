//! Schema diff engine for comparing database schemas.
//!
//! This module provides functionality to compare two schemas and identify
//! the differences between them, including added, removed, and modified
//! tables, columns, and constraints.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::export::{ColumnExport, ForeignKeyExport, IndexExport};
use crate::model::{Column, ForeignKey, Schema, Table};

/// The kind of change detected in a diff.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// The item was added in the new schema.
    #[default]
    Added,
    /// The item was removed in the new schema.
    Removed,
    /// The item was modified between schemas.
    Modified,
}

impl std::fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Added => write!(f, "added"),
            Self::Removed => write!(f, "removed"),
            Self::Modified => write!(f, "modified"),
        }
    }
}

/// Summary statistics for a schema diff.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffSummary {
    /// Number of tables added.
    pub tables_added: usize,
    /// Number of tables removed.
    pub tables_removed: usize,
    /// Number of tables modified.
    pub tables_modified: usize,
    /// Total number of column changes.
    pub columns_changed: usize,
    /// Total number of foreign key changes.
    pub foreign_keys_changed: usize,
    /// Total number of index changes.
    pub indexes_changed: usize,
}

impl DiffSummary {
    /// Returns true if there are no changes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.tables_added == 0
            && self.tables_removed == 0
            && self.tables_modified == 0
            && self.columns_changed == 0
            && self.foreign_keys_changed == 0
            && self.indexes_changed == 0
    }

    /// Returns the total number of changes.
    #[must_use]
    pub const fn total_changes(&self) -> usize {
        self.tables_added
            + self.tables_removed
            + self.tables_modified
            + self.columns_changed
            + self.foreign_keys_changed
            + self.indexes_changed
    }
}

/// Diff for a single column between two schemas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnDiff {
    /// Column name.
    pub column_name: String,
    /// Kind of change.
    pub change_kind: ChangeKind,
    /// Old value (present for Modified and Removed).
    pub old_value: Option<ColumnExport>,
    /// New value (present for Modified and Added).
    pub new_value: Option<ColumnExport>,
}

impl ColumnDiff {
    /// Creates a new column diff for an added column.
    #[must_use]
    pub fn added(column: &Column) -> Self {
        Self {
            column_name: column.name.clone(),
            change_kind: ChangeKind::Added,
            old_value: None,
            new_value: Some(Self::export_column(column)),
        }
    }

    /// Creates a new column diff for a removed column.
    #[must_use]
    pub fn removed(column: &Column) -> Self {
        Self {
            column_name: column.name.clone(),
            change_kind: ChangeKind::Removed,
            old_value: Some(Self::export_column(column)),
            new_value: None,
        }
    }

    /// Creates a new column diff for a modified column.
    #[must_use]
    pub fn modified(old_column: &Column, new_column: &Column) -> Self {
        Self {
            column_name: new_column.name.clone(),
            change_kind: ChangeKind::Modified,
            old_value: Some(Self::export_column(old_column)),
            new_value: Some(Self::export_column(new_column)),
        }
    }

    fn export_column(col: &Column) -> ColumnExport {
        ColumnExport {
            name: col.name.clone(),
            data_type: col.data_type.clone(),
            nullable: col.nullable,
            primary_key: col.is_primary_key,
        }
    }
}

/// Diff for a single foreign key between two schemas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForeignKeyDiff {
    /// Foreign key name (if named).
    pub name: Option<String>,
    /// Kind of change.
    pub change_kind: ChangeKind,
    /// Old value (present for Modified and Removed).
    pub old_value: Option<ForeignKeyExport>,
    /// New value (present for Modified and Added).
    pub new_value: Option<ForeignKeyExport>,
}

impl ForeignKeyDiff {
    /// Creates a new foreign key diff for an added FK.
    #[must_use]
    pub fn added(fk: &ForeignKey) -> Self {
        Self {
            name: fk.name.clone(),
            change_kind: ChangeKind::Added,
            old_value: None,
            new_value: Some(Self::export_fk(fk)),
        }
    }

    /// Creates a new foreign key diff for a removed FK.
    #[must_use]
    pub fn removed(fk: &ForeignKey) -> Self {
        Self {
            name: fk.name.clone(),
            change_kind: ChangeKind::Removed,
            old_value: Some(Self::export_fk(fk)),
            new_value: None,
        }
    }

    /// Creates a new foreign key diff for a modified FK.
    #[must_use]
    pub fn modified(old_fk: &ForeignKey, new_fk: &ForeignKey) -> Self {
        Self {
            name: new_fk.name.clone(),
            change_kind: ChangeKind::Modified,
            old_value: Some(Self::export_fk(old_fk)),
            new_value: Some(Self::export_fk(new_fk)),
        }
    }

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
}

/// Diff for a single index between two schemas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexDiff {
    /// Index name (if named).
    pub name: Option<String>,
    /// Kind of change.
    pub change_kind: ChangeKind,
    /// Old value (present for Modified and Removed).
    pub old_value: Option<IndexExport>,
    /// New value (present for Modified and Added).
    pub new_value: Option<IndexExport>,
}

impl IndexDiff {
    /// Creates a new index diff for an added index.
    #[must_use]
    pub fn added(index: &crate::model::Index) -> Self {
        Self {
            name: index.name.clone(),
            change_kind: ChangeKind::Added,
            old_value: None,
            new_value: Some(Self::export_index(index)),
        }
    }

    /// Creates a new index diff for a removed index.
    #[must_use]
    pub fn removed(index: &crate::model::Index) -> Self {
        Self {
            name: index.name.clone(),
            change_kind: ChangeKind::Removed,
            old_value: Some(Self::export_index(index)),
            new_value: None,
        }
    }

    /// Creates a new index diff for a modified index.
    #[must_use]
    pub fn modified(old_index: &crate::model::Index, new_index: &crate::model::Index) -> Self {
        Self {
            name: new_index.name.clone(),
            change_kind: ChangeKind::Modified,
            old_value: Some(Self::export_index(old_index)),
            new_value: Some(Self::export_index(new_index)),
        }
    }

    fn export_index(idx: &crate::model::Index) -> IndexExport {
        IndexExport {
            name: idx.name.clone(),
            columns: idx.columns.clone(),
            unique: idx.is_unique,
        }
    }
}

/// Diff for a single table between two schemas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableDiff {
    /// Table name (qualified with schema if applicable).
    pub table_name: String,
    /// Kind of change.
    pub change_kind: ChangeKind,
    /// Column diffs within this table.
    pub column_diffs: Vec<ColumnDiff>,
    /// Foreign key diffs within this table.
    pub fk_diffs: Vec<ForeignKeyDiff>,
    /// Index diffs within this table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index_diffs: Vec<IndexDiff>,
}

impl TableDiff {
    /// Creates a new table diff for an added table.
    #[must_use]
    pub fn added(table: &Table) -> Self {
        Self {
            table_name: table.qualified_name(),
            change_kind: ChangeKind::Added,
            column_diffs: table.columns.iter().map(ColumnDiff::added).collect(),
            fk_diffs: table
                .foreign_keys
                .iter()
                .map(ForeignKeyDiff::added)
                .collect(),
            index_diffs: table.indexes.iter().map(IndexDiff::added).collect(),
        }
    }

    /// Creates a new table diff for a removed table.
    #[must_use]
    pub fn removed(table: &Table) -> Self {
        Self {
            table_name: table.qualified_name(),
            change_kind: ChangeKind::Removed,
            column_diffs: table.columns.iter().map(ColumnDiff::removed).collect(),
            fk_diffs: table
                .foreign_keys
                .iter()
                .map(ForeignKeyDiff::removed)
                .collect(),
            index_diffs: table.indexes.iter().map(IndexDiff::removed).collect(),
        }
    }

    /// Creates a new table diff for a modified table.
    #[must_use]
    pub fn modified(old_table: &Table, new_table: &Table) -> Self {
        let column_diffs = Self::diff_columns(&old_table.columns, &new_table.columns);
        let fk_diffs = Self::diff_foreign_keys(&old_table.foreign_keys, &new_table.foreign_keys);
        let index_diffs = Self::diff_indexes(&old_table.indexes, &new_table.indexes);

        Self {
            table_name: new_table.qualified_name(),
            change_kind: ChangeKind::Modified,
            column_diffs,
            fk_diffs,
            index_diffs,
        }
    }

    fn diff_columns(old_columns: &[Column], new_columns: &[Column]) -> Vec<ColumnDiff> {
        let old_map: HashMap<&str, &Column> =
            old_columns.iter().map(|c| (c.name.as_str(), c)).collect();
        let new_map: HashMap<&str, &Column> =
            new_columns.iter().map(|c| (c.name.as_str(), c)).collect();

        let old_names: HashSet<&str> = old_map.keys().copied().collect();
        let new_names: HashSet<&str> = new_map.keys().copied().collect();

        let mut diffs = Vec::new();

        // Removed columns
        for name in old_names.difference(&new_names) {
            diffs.push(ColumnDiff::removed(old_map[name]));
        }

        // Added columns
        for name in new_names.difference(&old_names) {
            diffs.push(ColumnDiff::added(new_map[name]));
        }

        // Modified columns
        for name in old_names.intersection(&new_names) {
            let old_col = old_map[name];
            let new_col = new_map[name];
            if Self::columns_differ(old_col, new_col) {
                diffs.push(ColumnDiff::modified(old_col, new_col));
            }
        }

        diffs
    }

    fn columns_differ(a: &Column, b: &Column) -> bool {
        a.data_type != b.data_type
            || a.nullable != b.nullable
            || a.is_primary_key != b.is_primary_key
    }

    fn diff_foreign_keys(old_fks: &[ForeignKey], new_fks: &[ForeignKey]) -> Vec<ForeignKeyDiff> {
        let old_map: HashMap<Cow<'_, str>, &ForeignKey> =
            old_fks.iter().map(|fk| (Self::fk_key(fk), fk)).collect();
        let new_map: HashMap<Cow<'_, str>, &ForeignKey> =
            new_fks.iter().map(|fk| (Self::fk_key(fk), fk)).collect();

        let old_keys: HashSet<&Cow<'_, str>> = old_map.keys().collect();
        let new_keys: HashSet<&Cow<'_, str>> = new_map.keys().collect();

        let mut diffs = Vec::new();

        // Removed FKs
        for key in old_keys.difference(&new_keys) {
            diffs.push(ForeignKeyDiff::removed(old_map[*key]));
        }

        // Added FKs
        for key in new_keys.difference(&old_keys) {
            diffs.push(ForeignKeyDiff::added(new_map[*key]));
        }

        // Modified FKs
        for key in old_keys.intersection(&new_keys) {
            let old_fk = old_map[*key];
            let new_fk = new_map[*key];
            if Self::fks_differ(old_fk, new_fk) {
                diffs.push(ForeignKeyDiff::modified(old_fk, new_fk));
            }
        }

        diffs
    }

    fn fk_key(fk: &ForeignKey) -> Cow<'_, str> {
        // Use name if available (borrow), otherwise create a key from columns
        match &fk.name {
            Some(name) => Cow::Borrowed(name),
            None => Cow::Owned(format!(
                "{}_{}_{}",
                fk.from_columns.join("_"),
                fk.to_table,
                fk.to_columns.join("_")
            )),
        }
    }

    fn fks_differ(a: &ForeignKey, b: &ForeignKey) -> bool {
        a.from_columns != b.from_columns
            || a.to_schema != b.to_schema
            || a.to_table != b.to_table
            || a.to_columns != b.to_columns
    }

    fn diff_indexes(
        old_indexes: &[crate::model::Index],
        new_indexes: &[crate::model::Index],
    ) -> Vec<IndexDiff> {
        let old_map: HashMap<Cow<'_, str>, &crate::model::Index> = old_indexes
            .iter()
            .map(|idx| (Self::index_key(idx), idx))
            .collect();
        let new_map: HashMap<Cow<'_, str>, &crate::model::Index> = new_indexes
            .iter()
            .map(|idx| (Self::index_key(idx), idx))
            .collect();

        let old_keys: HashSet<&Cow<'_, str>> = old_map.keys().collect();
        let new_keys: HashSet<&Cow<'_, str>> = new_map.keys().collect();

        let mut diffs = Vec::new();

        // Removed indexes
        for key in old_keys.difference(&new_keys) {
            diffs.push(IndexDiff::removed(old_map[*key]));
        }

        // Added indexes
        for key in new_keys.difference(&old_keys) {
            diffs.push(IndexDiff::added(new_map[*key]));
        }

        // Modified indexes
        for key in old_keys.intersection(&new_keys) {
            let old_idx = old_map[*key];
            let new_idx = new_map[*key];
            if Self::indexes_differ(old_idx, new_idx) {
                diffs.push(IndexDiff::modified(old_idx, new_idx));
            }
        }

        diffs
    }

    fn index_key(idx: &crate::model::Index) -> Cow<'_, str> {
        match &idx.name {
            Some(name) if !name.is_empty() => Cow::Borrowed(name),
            _ => Cow::Owned(format!("idx_{}", idx.columns.join("_"))),
        }
    }

    fn indexes_differ(a: &crate::model::Index, b: &crate::model::Index) -> bool {
        a.columns != b.columns || a.is_unique != b.is_unique
    }

    /// Returns true if this table has any changes.
    #[must_use]
    pub const fn has_changes(&self) -> bool {
        !self.column_diffs.is_empty() || !self.fk_diffs.is_empty() || !self.index_diffs.is_empty()
    }
}

/// Complete diff between two schemas.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaDiff {
    /// Names of tables that were added.
    pub added_tables: Vec<String>,
    /// Names of tables that were removed.
    pub removed_tables: Vec<String>,
    /// Diffs for tables that were modified.
    pub modified_tables: Vec<TableDiff>,
    /// Summary statistics.
    pub summary: DiffSummary,
}

impl SchemaDiff {
    /// Returns true if there are no changes between the schemas.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.summary.is_empty()
    }

    /// Returns the total number of changes.
    #[must_use]
    pub const fn total_changes(&self) -> usize {
        self.summary.total_changes()
    }

    /// Returns all table names that have any kind of change.
    #[must_use]
    pub fn changed_table_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        names.extend(self.added_tables.iter().map(String::as_str));
        names.extend(self.removed_tables.iter().map(String::as_str));
        names.extend(self.modified_tables.iter().map(|t| t.table_name.as_str()));
        names
    }
}

/// Compare two schemas and produce a diff.
///
/// # Arguments
///
/// * `before` - The original schema.
/// * `after` - The new schema to compare against.
///
/// # Returns
///
/// A `SchemaDiff` containing all detected changes.
#[must_use]
pub fn diff_schemas(before: &Schema, after: &Schema) -> SchemaDiff {
    let before_map: HashMap<&str, &Table> = before
        .tables
        .iter()
        .map(|t| (t.stable_id.as_str(), t))
        .collect();
    let after_map: HashMap<&str, &Table> = after
        .tables
        .iter()
        .map(|t| (t.stable_id.as_str(), t))
        .collect();

    let before_ids: HashSet<&str> = before_map.keys().copied().collect();
    let after_ids: HashSet<&str> = after_map.keys().copied().collect();

    let mut added_tables = Vec::new();
    let mut removed_tables = Vec::new();
    let mut modified_tables = Vec::new();

    // Removed tables
    for id in before_ids.difference(&after_ids) {
        let table = before_map[*id];
        removed_tables.push(table.qualified_name());
    }

    // Added tables
    for id in after_ids.difference(&before_ids) {
        let table = after_map[*id];
        added_tables.push(table.qualified_name());
    }

    // Modified tables
    for id in before_ids.intersection(&after_ids) {
        let old_table = before_map[*id];
        let new_table = after_map[*id];
        let diff = TableDiff::modified(old_table, new_table);
        if diff.has_changes() {
            modified_tables.push(diff);
        }
    }

    // Sort for consistent output
    added_tables.sort();
    removed_tables.sort();

    // Calculate summary
    let columns_changed: usize = modified_tables.iter().map(|t| t.column_diffs.len()).sum();
    let foreign_keys_changed: usize = modified_tables.iter().map(|t| t.fk_diffs.len()).sum();
    let indexes_changed: usize = modified_tables.iter().map(|t| t.index_diffs.len()).sum();

    let summary = DiffSummary {
        tables_added: added_tables.len(),
        tables_removed: removed_tables.len(),
        tables_modified: modified_tables.len(),
        columns_changed,
        foreign_keys_changed,
        indexes_changed,
    };

    SchemaDiff {
        added_tables,
        removed_tables,
        modified_tables,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ColumnId, ReferentialAction, TableId};

    fn create_test_table(
        name: &str,
        columns: Vec<(&str, &str, bool, bool)>,
        fks: Vec<(&str, Vec<&str>, &str, Vec<&str>)>,
    ) -> Table {
        Table {
            id: TableId(0),
            stable_id: name.to_string(),
            schema_name: None,
            name: name.to_string(),
            columns: columns
                .into_iter()
                .enumerate()
                .map(|(i, (name, dtype, nullable, pk))| Column {
                    id: ColumnId(i as u64),
                    name: name.to_string(),
                    data_type: dtype.to_string(),
                    nullable,
                    is_primary_key: pk,
                    comment: None,
                })
                .collect(),
            foreign_keys: fks
                .into_iter()
                .map(|(name, from_cols, to_table, to_cols)| ForeignKey {
                    name: if name.is_empty() {
                        None
                    } else {
                        Some(name.to_string())
                    },
                    from_columns: from_cols.into_iter().map(ToString::to_string).collect(),
                    to_schema: None,
                    to_table: to_table.to_string(),
                    to_columns: to_cols.into_iter().map(ToString::to_string).collect(),
                    on_delete: ReferentialAction::NoAction,
                    on_update: ReferentialAction::NoAction,
                })
                .collect(),
            indexes: vec![],
            comment: None,
        }
    }

    #[test]
    fn test_empty_schemas() {
        let before = Schema::default();
        let after = Schema::default();

        let diff = diff_schemas(&before, &after);

        assert!(diff.is_empty());
        assert!(diff.added_tables.is_empty());
        assert!(diff.removed_tables.is_empty());
        assert!(diff.modified_tables.is_empty());
    }

    #[test]
    fn test_added_table() {
        let before = Schema::default();
        let after = Schema {
            tables: vec![create_test_table(
                "users",
                vec![("id", "bigint", false, true)],
                vec![],
            )],
            views: vec![],
            enums: vec![],
        };

        let diff = diff_schemas(&before, &after);

        assert!(!diff.is_empty());
        assert_eq!(diff.added_tables, vec!["users"]);
        assert!(diff.removed_tables.is_empty());
        assert!(diff.modified_tables.is_empty());
        assert_eq!(diff.summary.tables_added, 1);
    }

    #[test]
    fn test_removed_table() {
        let before = Schema {
            tables: vec![create_test_table(
                "users",
                vec![("id", "bigint", false, true)],
                vec![],
            )],
            views: vec![],
            enums: vec![],
        };
        let after = Schema::default();

        let diff = diff_schemas(&before, &after);

        assert!(!diff.is_empty());
        assert!(diff.added_tables.is_empty());
        assert_eq!(diff.removed_tables, vec!["users"]);
        assert!(diff.modified_tables.is_empty());
        assert_eq!(diff.summary.tables_removed, 1);
    }

    #[test]
    fn test_modified_table_added_column() {
        let before = Schema {
            tables: vec![create_test_table(
                "users",
                vec![("id", "bigint", false, true)],
                vec![],
            )],
            views: vec![],
            enums: vec![],
        };
        let after = Schema {
            tables: vec![create_test_table(
                "users",
                vec![
                    ("id", "bigint", false, true),
                    ("email", "varchar(255)", false, false),
                ],
                vec![],
            )],
            views: vec![],
            enums: vec![],
        };

        let diff = diff_schemas(&before, &after);

        assert!(diff.added_tables.is_empty());
        assert!(diff.removed_tables.is_empty());
        assert_eq!(diff.modified_tables.len(), 1);

        let table_diff = &diff.modified_tables[0];
        assert_eq!(table_diff.table_name, "users");
        assert_eq!(table_diff.change_kind, ChangeKind::Modified);
        assert_eq!(table_diff.column_diffs.len(), 1);

        let col_diff = &table_diff.column_diffs[0];
        assert_eq!(col_diff.column_name, "email");
        assert_eq!(col_diff.change_kind, ChangeKind::Added);
        assert!(col_diff.old_value.is_none());
        assert!(col_diff.new_value.is_some());
    }

    #[test]
    fn test_modified_table_removed_column() {
        let before = Schema {
            tables: vec![create_test_table(
                "users",
                vec![
                    ("id", "bigint", false, true),
                    ("email", "varchar(255)", false, false),
                ],
                vec![],
            )],
            views: vec![],
            enums: vec![],
        };
        let after = Schema {
            tables: vec![create_test_table(
                "users",
                vec![("id", "bigint", false, true)],
                vec![],
            )],
            views: vec![],
            enums: vec![],
        };

        let diff = diff_schemas(&before, &after);

        assert_eq!(diff.modified_tables.len(), 1);
        let table_diff = &diff.modified_tables[0];
        assert_eq!(table_diff.column_diffs.len(), 1);

        let col_diff = &table_diff.column_diffs[0];
        assert_eq!(col_diff.column_name, "email");
        assert_eq!(col_diff.change_kind, ChangeKind::Removed);
    }

    #[test]
    fn test_modified_column_type() {
        let before = Schema {
            tables: vec![create_test_table(
                "users",
                vec![("id", "int", false, true)],
                vec![],
            )],
            views: vec![],
            enums: vec![],
        };
        let after = Schema {
            tables: vec![create_test_table(
                "users",
                vec![("id", "bigint", false, true)],
                vec![],
            )],
            views: vec![],
            enums: vec![],
        };

        let diff = diff_schemas(&before, &after);

        assert_eq!(diff.modified_tables.len(), 1);
        let table_diff = &diff.modified_tables[0];
        assert_eq!(table_diff.column_diffs.len(), 1);

        let col_diff = &table_diff.column_diffs[0];
        assert_eq!(col_diff.column_name, "id");
        assert_eq!(col_diff.change_kind, ChangeKind::Modified);
        assert_eq!(col_diff.old_value.as_ref().unwrap().data_type, "int");
        assert_eq!(col_diff.new_value.as_ref().unwrap().data_type, "bigint");
    }

    #[test]
    fn test_added_foreign_key() {
        let before = Schema {
            tables: vec![
                create_test_table("users", vec![("id", "bigint", false, true)], vec![]),
                create_test_table("posts", vec![("id", "bigint", false, true)], vec![]),
            ],
            views: vec![],
            enums: vec![],
        };
        let after = Schema {
            tables: vec![
                create_test_table("users", vec![("id", "bigint", false, true)], vec![]),
                create_test_table(
                    "posts",
                    vec![
                        ("id", "bigint", false, true),
                        ("user_id", "bigint", false, false),
                    ],
                    vec![("fk_posts_user", vec!["user_id"], "users", vec!["id"])],
                ),
            ],
            views: vec![],
            enums: vec![],
        };

        let diff = diff_schemas(&before, &after);

        assert_eq!(diff.modified_tables.len(), 1);
        let posts_diff = diff
            .modified_tables
            .iter()
            .find(|t| t.table_name == "posts")
            .unwrap();
        assert_eq!(posts_diff.column_diffs.len(), 1); // user_id added
        assert_eq!(posts_diff.fk_diffs.len(), 1); // FK added
    }

    #[test]
    fn test_schema_diff_serialization() {
        let diff = SchemaDiff {
            added_tables: vec!["new_table".to_string()],
            removed_tables: vec![],
            modified_tables: vec![],
            summary: DiffSummary {
                tables_added: 1,
                tables_removed: 0,
                tables_modified: 0,
                columns_changed: 0,
                foreign_keys_changed: 0,
                indexes_changed: 0,
            },
        };

        let json = serde_json::to_string(&diff).unwrap();
        let parsed: SchemaDiff = serde_json::from_str(&json).unwrap();
        assert_eq!(diff, parsed);
    }

    #[test]
    fn test_change_kind_display() {
        assert_eq!(format!("{}", ChangeKind::Added), "added");
        assert_eq!(format!("{}", ChangeKind::Removed), "removed");
        assert_eq!(format!("{}", ChangeKind::Modified), "modified");
    }
}
