//! Schema diff engine for comparing database schemas.
//!
//! This module provides functionality to compare two schemas and identify
//! the differences between them, including added, removed, and modified
//! tables, views, enums, columns, and constraints.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::export::{ColumnExport, ForeignKeyExport, IndexExport};
use crate::model::{Column, Enum, ForeignKey, Schema, Table, View};

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
    /// Number of views added.
    pub views_added: usize,
    /// Number of views removed.
    pub views_removed: usize,
    /// Number of views modified.
    pub views_modified: usize,
    /// Total number of view column changes.
    pub view_columns_changed: usize,
    /// Total number of view definition changes.
    pub view_definitions_changed: usize,
    /// Number of enums added.
    pub enums_added: usize,
    /// Number of enums removed.
    pub enums_removed: usize,
    /// Number of enums modified.
    pub enums_modified: usize,
    /// Total number of enum value changes.
    pub enum_values_changed: usize,
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
            && self.views_added == 0
            && self.views_removed == 0
            && self.views_modified == 0
            && self.view_columns_changed == 0
            && self.view_definitions_changed == 0
            && self.enums_added == 0
            && self.enums_removed == 0
            && self.enums_modified == 0
            && self.enum_values_changed == 0
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
            + self.views_added
            + self.views_removed
            + self.views_modified
            + self.view_columns_changed
            + self.view_definitions_changed
            + self.enums_added
            + self.enums_removed
            + self.enums_modified
            + self.enum_values_changed
    }

    /// Returns the number of added schema objects.
    #[must_use]
    pub const fn added_items(&self) -> usize {
        self.tables_added + self.views_added + self.enums_added
    }

    /// Returns the number of removed schema objects.
    #[must_use]
    pub const fn removed_items(&self) -> usize {
        self.tables_removed + self.views_removed + self.enums_removed
    }

    /// Returns the number of modified schema objects.
    #[must_use]
    pub const fn modified_items(&self) -> usize {
        self.tables_modified + self.views_modified + self.enums_modified
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

fn diff_columns(old_columns: &[Column], new_columns: &[Column]) -> Vec<ColumnDiff> {
    diff_columns_with(old_columns, new_columns, columns_differ)
}

fn diff_columns_with(
    old_columns: &[Column],
    new_columns: &[Column],
    differs: impl Fn(&Column, &Column) -> bool,
) -> Vec<ColumnDiff> {
    let old_map: HashMap<&str, &Column> =
        old_columns.iter().map(|c| (c.name.as_str(), c)).collect();
    let new_map: HashMap<&str, &Column> =
        new_columns.iter().map(|c| (c.name.as_str(), c)).collect();

    let old_names: HashSet<&str> = old_map.keys().copied().collect();
    let new_names: HashSet<&str> = new_map.keys().copied().collect();

    let mut diffs = Vec::new();

    for name in old_names.difference(&new_names) {
        diffs.push(ColumnDiff::removed(old_map[name]));
    }

    for name in new_names.difference(&old_names) {
        diffs.push(ColumnDiff::added(new_map[name]));
    }

    for name in old_names.intersection(&new_names) {
        let old_col = old_map[name];
        let new_col = new_map[name];
        if differs(old_col, new_col) {
            diffs.push(ColumnDiff::modified(old_col, new_col));
        }
    }

    diffs
}

fn columns_differ(a: &Column, b: &Column) -> bool {
    a.data_type != b.data_type || a.nullable != b.nullable || a.is_primary_key != b.is_primary_key
}

fn view_columns_differ(a: &Column, b: &Column) -> bool {
    matches!(
        (known_view_data_type(a), known_view_data_type(b)),
        (Some(left), Some(right)) if left != right
    )
}

fn known_view_data_type(column: &Column) -> Option<&str> {
    let data_type = column.data_type.trim();
    if data_type.eq_ignore_ascii_case("unknown") || data_type.is_empty() {
        None
    } else {
        Some(data_type)
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
        let column_diffs = diff_columns(&old_table.columns, &new_table.columns);
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

    fn diff_foreign_keys(old_fks: &[ForeignKey], new_fks: &[ForeignKey]) -> Vec<ForeignKeyDiff> {
        let old_map: HashMap<String, &ForeignKey> =
            old_fks.iter().map(|fk| (Self::fk_key(fk), fk)).collect();
        let new_map: HashMap<String, &ForeignKey> =
            new_fks.iter().map(|fk| (Self::fk_key(fk), fk)).collect();

        let old_keys: HashSet<&String> = old_map.keys().collect();
        let new_keys: HashSet<&String> = new_map.keys().collect();

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

    fn fk_key(fk: &ForeignKey) -> String {
        // Keep unnamed FK identity stable without relying on ambiguous separators.
        if let Some(name) = &fk.name {
            name.clone()
        } else {
            let mut key = String::new();
            Self::push_key_option(&mut key, fk.to_schema.as_deref());
            Self::push_key_part(&mut key, &fk.to_table);
            Self::push_key_pairs(&mut key, &Self::fk_column_pairs(fk));
            key
        }
    }

    fn push_key_option(key: &mut String, value: Option<&str>) {
        match value {
            Some(value) => {
                key.push('1');
                key.push(':');
                Self::push_key_part(key, value);
            }
            None => key.push_str("0;"),
        }
    }

    fn push_key_pairs(key: &mut String, pairs: &[(String, String)]) {
        key.push_str(&pairs.len().to_string());
        key.push('[');
        for (from_column, to_column) in pairs {
            Self::push_key_part(key, from_column);
            Self::push_key_part(key, to_column);
        }
        key.push(']');
    }

    fn push_key_part(key: &mut String, value: &str) {
        key.push_str(&value.len().to_string());
        key.push(':');
        key.push_str(value);
        key.push(';');
    }

    fn fks_differ(a: &ForeignKey, b: &ForeignKey) -> bool {
        a.from_columns.len() != b.from_columns.len()
            || a.to_columns.len() != b.to_columns.len()
            || Self::fk_column_pairs(a) != Self::fk_column_pairs(b)
            || a.to_schema != b.to_schema
            || a.to_table != b.to_table
    }

    fn fk_column_pairs(fk: &ForeignKey) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = fk
            .from_columns
            .iter()
            .cloned()
            .zip(fk.to_columns.iter().cloned())
            .collect();
        pairs.sort_unstable();
        pairs
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

/// Diff for a single view between two schemas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ViewDiff {
    /// View name (qualified with schema if applicable).
    pub view_name: String,
    /// Kind of change.
    pub change_kind: ChangeKind,
    /// Column diffs within this view.
    pub column_diffs: Vec<ColumnDiff>,
    /// Previous definition when the definition changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_definition: Option<String>,
    /// New definition when the definition changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_definition: Option<String>,
}

impl ViewDiff {
    /// Creates a new view diff for a modified view.
    #[must_use]
    pub fn modified(old_view: &View, new_view: &View) -> Self {
        let column_diffs =
            diff_columns_with(&old_view.columns, &new_view.columns, view_columns_differ);
        let definition_changed = old_view.definition != new_view.definition;
        let (old_definition, new_definition) = if definition_changed {
            (old_view.definition.clone(), new_view.definition.clone())
        } else {
            (None, None)
        };

        Self {
            view_name: new_view.qualified_name(),
            change_kind: ChangeKind::Modified,
            column_diffs,
            old_definition,
            new_definition,
        }
    }

    /// Returns true if this view has any changes.
    #[must_use]
    pub const fn has_changes(&self) -> bool {
        !self.column_diffs.is_empty() || self.definition_changed()
    }

    /// Returns true when the view definition changed.
    #[must_use]
    pub const fn definition_changed(&self) -> bool {
        self.old_definition.is_some() || self.new_definition.is_some()
    }
}

/// Diff for a single enum value between two schemas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnumValueDiff {
    /// Enum value label.
    pub value: String,
    /// Kind of change.
    pub change_kind: ChangeKind,
    /// Previous position in the enum value list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_position: Option<usize>,
    /// New position in the enum value list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_position: Option<usize>,
}

impl EnumValueDiff {
    /// Creates a diff for an enum value added at `new_position`.
    #[must_use]
    pub fn added(value: &str, new_position: usize) -> Self {
        Self {
            value: value.to_string(),
            change_kind: ChangeKind::Added,
            old_position: None,
            new_position: Some(new_position),
        }
    }

    /// Creates a diff for an enum value removed from `old_position`.
    #[must_use]
    pub fn removed(value: &str, old_position: usize) -> Self {
        Self {
            value: value.to_string(),
            change_kind: ChangeKind::Removed,
            old_position: Some(old_position),
            new_position: None,
        }
    }

    /// Creates a diff for an enum value moved from `old_position` to `new_position`.
    #[must_use]
    pub fn modified(value: &str, old_position: usize, new_position: usize) -> Self {
        Self {
            value: value.to_string(),
            change_kind: ChangeKind::Modified,
            old_position: Some(old_position),
            new_position: Some(new_position),
        }
    }
}

/// Diff for a single enum between two schemas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnumDiff {
    /// Enum name (qualified with schema if applicable).
    pub enum_name: String,
    /// Kind of change.
    pub change_kind: ChangeKind,
    /// Enum value diffs within this enum.
    pub value_diffs: Vec<EnumValueDiff>,
}

impl EnumDiff {
    /// Creates a new enum diff for a modified enum.
    #[must_use]
    pub fn modified(old_enum: &Enum, new_enum: &Enum) -> Self {
        let old_map: HashMap<&str, usize> = old_enum
            .values
            .iter()
            .enumerate()
            .map(|(index, value)| (value.as_str(), index))
            .collect();
        let new_map: HashMap<&str, usize> = new_enum
            .values
            .iter()
            .enumerate()
            .map(|(index, value)| (value.as_str(), index))
            .collect();

        let old_values: HashSet<&str> = old_map.keys().copied().collect();
        let new_values: HashSet<&str> = new_map.keys().copied().collect();

        let mut value_diffs = Vec::new();

        for value in old_values.difference(&new_values) {
            value_diffs.push(EnumValueDiff::removed(value, old_map[value]));
        }

        for value in new_values.difference(&old_values) {
            value_diffs.push(EnumValueDiff::added(value, new_map[value]));
        }

        for value in old_values.intersection(&new_values) {
            let old_position = old_map[value];
            let new_position = new_map[value];
            if old_position != new_position {
                value_diffs.push(EnumValueDiff::modified(value, old_position, new_position));
            }
        }

        value_diffs.sort_by(|left, right| left.value.cmp(&right.value));

        Self {
            enum_name: new_enum.qualified_name(),
            change_kind: ChangeKind::Modified,
            value_diffs,
        }
    }

    /// Returns true if this enum has any changes.
    #[must_use]
    pub const fn has_changes(&self) -> bool {
        !self.value_diffs.is_empty()
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
    /// Names of views that were added.
    pub added_views: Vec<String>,
    /// Names of views that were removed.
    pub removed_views: Vec<String>,
    /// Diffs for views that were modified.
    pub modified_views: Vec<ViewDiff>,
    /// Names of enums that were added.
    pub added_enums: Vec<String>,
    /// Names of enums that were removed.
    pub removed_enums: Vec<String>,
    /// Diffs for enums that were modified.
    pub modified_enums: Vec<EnumDiff>,
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

/// Identity key for matching views across schemas.
///
/// Built from the (lowercased schema, lowercased name) pair so that the
/// match is stable regardless of how the upstream parser or introspector
/// composes its `View.id` field.
fn view_identity(view: &View) -> (String, String) {
    let schema = view.schema_name.as_deref().unwrap_or("").to_lowercase();
    (schema, view.name.to_lowercase())
}

/// Identity key for matching enums across schemas. See `view_identity`.
fn enum_identity(enum_type: &Enum) -> (String, String) {
    let schema = enum_type
        .schema_name
        .as_deref()
        .unwrap_or("")
        .to_lowercase();
    (schema, enum_type.name.to_lowercase())
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
#[allow(clippy::too_many_lines)] // Aggregating per-kind diffs in one place keeps summary logic aligned.
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
    let before_view_map: HashMap<(String, String), &View> = before
        .views
        .iter()
        .map(|view| (view_identity(view), view))
        .collect();
    let after_view_map: HashMap<(String, String), &View> = after
        .views
        .iter()
        .map(|view| (view_identity(view), view))
        .collect();
    let before_view_ids: HashSet<&(String, String)> = before_view_map.keys().collect();
    let after_view_ids: HashSet<&(String, String)> = after_view_map.keys().collect();
    let before_enum_map: HashMap<(String, String), &Enum> = before
        .enums
        .iter()
        .map(|enum_type| (enum_identity(enum_type), enum_type))
        .collect();
    let after_enum_map: HashMap<(String, String), &Enum> = after
        .enums
        .iter()
        .map(|enum_type| (enum_identity(enum_type), enum_type))
        .collect();
    let before_enum_ids: HashSet<&(String, String)> = before_enum_map.keys().collect();
    let after_enum_ids: HashSet<&(String, String)> = after_enum_map.keys().collect();

    let mut added_tables = Vec::new();
    let mut removed_tables = Vec::new();
    let mut modified_tables = Vec::new();
    let mut added_views = Vec::new();
    let mut removed_views = Vec::new();
    let mut modified_views = Vec::new();
    let mut added_enums = Vec::new();
    let mut removed_enums = Vec::new();
    let mut modified_enums = Vec::new();

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

    for id in before_view_ids.difference(&after_view_ids) {
        let view = before_view_map[*id];
        removed_views.push(view.qualified_name());
    }

    for id in after_view_ids.difference(&before_view_ids) {
        let view = after_view_map[*id];
        added_views.push(view.qualified_name());
    }

    for id in before_view_ids.intersection(&after_view_ids) {
        let old_view = before_view_map[*id];
        let new_view = after_view_map[*id];
        let diff = ViewDiff::modified(old_view, new_view);
        if diff.has_changes() {
            modified_views.push(diff);
        }
    }

    for id in before_enum_ids.difference(&after_enum_ids) {
        let enum_type = before_enum_map[*id];
        removed_enums.push(enum_type.qualified_name());
    }

    for id in after_enum_ids.difference(&before_enum_ids) {
        let enum_type = after_enum_map[*id];
        added_enums.push(enum_type.qualified_name());
    }

    for id in before_enum_ids.intersection(&after_enum_ids) {
        let old_enum = before_enum_map[*id];
        let new_enum = after_enum_map[*id];
        let diff = EnumDiff::modified(old_enum, new_enum);
        if diff.has_changes() {
            modified_enums.push(diff);
        }
    }

    // Sort for consistent output
    added_tables.sort();
    removed_tables.sort();
    modified_tables.sort_by(|left, right| left.table_name.cmp(&right.table_name));
    added_views.sort();
    removed_views.sort();
    modified_views.sort_by(|left, right| left.view_name.cmp(&right.view_name));
    added_enums.sort();
    removed_enums.sort();
    modified_enums.sort_by(|left, right| left.enum_name.cmp(&right.enum_name));

    // Calculate summary
    let columns_changed: usize = modified_tables.iter().map(|t| t.column_diffs.len()).sum();
    let foreign_keys_changed: usize = modified_tables.iter().map(|t| t.fk_diffs.len()).sum();
    let indexes_changed: usize = modified_tables.iter().map(|t| t.index_diffs.len()).sum();
    let view_columns_changed: usize = modified_views
        .iter()
        .map(|view| view.column_diffs.len())
        .sum();
    let view_definitions_changed: usize = modified_views
        .iter()
        .filter(|view| view.definition_changed())
        .count();
    let enum_values_changed: usize = modified_enums
        .iter()
        .map(|enum_| enum_.value_diffs.len())
        .sum();

    let summary = DiffSummary {
        tables_added: added_tables.len(),
        tables_removed: removed_tables.len(),
        tables_modified: modified_tables.len(),
        columns_changed,
        foreign_keys_changed,
        indexes_changed,
        views_added: added_views.len(),
        views_removed: removed_views.len(),
        views_modified: modified_views.len(),
        view_columns_changed,
        view_definitions_changed,
        enums_added: added_enums.len(),
        enums_removed: removed_enums.len(),
        enums_modified: modified_enums.len(),
        enum_values_changed,
    };

    SchemaDiff {
        added_tables,
        removed_tables,
        modified_tables,
        added_views,
        removed_views,
        modified_views,
        added_enums,
        removed_enums,
        modified_enums,
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
            primary_key_name: None,
            comment: None,
        }
    }

    fn create_test_view(name: &str, columns: Vec<(&str, &str)>, definition: Option<&str>) -> View {
        View {
            id: name.to_string(),
            schema_name: None,
            name: name.to_string(),
            columns: columns
                .into_iter()
                .enumerate()
                .map(|(index, (column_name, data_type))| Column {
                    id: ColumnId(index as u64),
                    name: column_name.to_string(),
                    data_type: data_type.to_string(),
                    nullable: true,
                    is_primary_key: false,
                    comment: None,
                })
                .collect(),
            definition: definition.map(ToString::to_string),
        }
    }

    fn create_test_enum(name: &str, values: &[&str]) -> Enum {
        Enum {
            id: name.to_string(),
            schema_name: None,
            name: name.to_string(),
            values: values.iter().map(ToString::to_string).collect(),
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
    fn test_unnamed_foreign_keys_with_ambiguous_column_names_are_distinct() {
        let before = Schema {
            tables: vec![
                create_test_table(
                    "targets",
                    vec![
                        ("id1", "bigint", false, true),
                        ("id2", "bigint", false, true),
                    ],
                    vec![],
                ),
                create_test_table(
                    "child",
                    vec![
                        ("a_b", "bigint", false, false),
                        ("c", "bigint", false, false),
                        ("a", "bigint", false, false),
                        ("b_c", "bigint", false, false),
                    ],
                    vec![
                        ("", vec!["a_b", "c"], "targets", vec!["id1", "id2"]),
                        ("", vec!["a", "b_c"], "targets", vec!["id1", "id2"]),
                    ],
                ),
            ],
            views: vec![],
            enums: vec![],
        };
        let after = Schema {
            tables: vec![
                create_test_table(
                    "targets",
                    vec![
                        ("id1", "bigint", false, true),
                        ("id2", "bigint", false, true),
                    ],
                    vec![],
                ),
                create_test_table(
                    "child",
                    vec![
                        ("a_b", "bigint", false, false),
                        ("c", "bigint", false, false),
                        ("a", "bigint", false, false),
                        ("b_c", "bigint", false, false),
                    ],
                    vec![
                        ("", vec!["a", "b_c"], "targets", vec!["id1", "id2"]),
                        ("", vec!["a_b", "c"], "targets", vec!["id1", "id2"]),
                    ],
                ),
            ],
            views: vec![],
            enums: vec![],
        };

        let diff = diff_schemas(&before, &after);

        assert!(diff.is_empty());
        assert!(diff.added_tables.is_empty());
        assert!(diff.removed_tables.is_empty());
        assert!(diff.modified_tables.is_empty());
    }

    #[test]
    fn test_unnamed_foreign_keys_ignore_column_order_when_pairs_match() {
        let before = Schema {
            tables: vec![
                create_test_table(
                    "users",
                    vec![
                        ("id", "bigint", false, true),
                        ("tenant_id", "bigint", false, true),
                    ],
                    vec![],
                ),
                create_test_table(
                    "posts",
                    vec![
                        ("id", "bigint", false, true),
                        ("user_id", "bigint", false, false),
                        ("tenant_id", "bigint", false, false),
                    ],
                    vec![(
                        "",
                        vec!["user_id", "tenant_id"],
                        "users",
                        vec!["id", "tenant_id"],
                    )],
                ),
            ],
            views: vec![],
            enums: vec![],
        };
        let after = Schema {
            tables: vec![
                create_test_table(
                    "users",
                    vec![
                        ("id", "bigint", false, true),
                        ("tenant_id", "bigint", false, true),
                    ],
                    vec![],
                ),
                create_test_table(
                    "posts",
                    vec![
                        ("id", "bigint", false, true),
                        ("user_id", "bigint", false, false),
                        ("tenant_id", "bigint", false, false),
                    ],
                    vec![(
                        "",
                        vec!["tenant_id", "user_id"],
                        "users",
                        vec!["tenant_id", "id"],
                    )],
                ),
            ],
            views: vec![],
            enums: vec![],
        };

        let diff = diff_schemas(&before, &after);

        assert!(diff.is_empty());
    }

    #[test]
    fn test_added_view() {
        let before = Schema::default();
        let after = Schema {
            tables: vec![],
            views: vec![create_test_view(
                "active_users",
                vec![("id", "int")],
                Some("SELECT id FROM users"),
            )],
            enums: vec![],
        };

        let diff = diff_schemas(&before, &after);

        assert_eq!(diff.added_views, vec!["active_users"]);
        assert_eq!(diff.summary.views_added, 1);
    }

    #[test]
    fn test_modified_view_tracks_columns_and_definition() {
        let before = Schema {
            tables: vec![],
            views: vec![create_test_view(
                "active_users",
                vec![("id", "int"), ("email", "text")],
                Some("SELECT id, email FROM users"),
            )],
            enums: vec![],
        };
        let after = Schema {
            tables: vec![],
            views: vec![create_test_view(
                "active_users",
                vec![("id", "int")],
                Some("SELECT id FROM users"),
            )],
            enums: vec![],
        };

        let diff = diff_schemas(&before, &after);

        assert_eq!(diff.modified_views.len(), 1);
        let view_diff = &diff.modified_views[0];
        assert_eq!(view_diff.view_name, "active_users");
        assert_eq!(view_diff.column_diffs.len(), 1);
        assert!(view_diff.definition_changed());
        assert_eq!(diff.summary.views_modified, 1);
        assert_eq!(diff.summary.view_columns_changed, 1);
        assert_eq!(diff.summary.view_definitions_changed, 1);
    }

    #[test]
    fn test_modified_view_ignores_unknown_column_metadata_from_sql_parser() {
        let before = Schema {
            tables: vec![],
            views: vec![create_test_view(
                "active_users",
                vec![("id", "unknown"), ("email", "unknown")],
                Some("SELECT id, email FROM users"),
            )],
            enums: vec![],
        };
        let after = Schema {
            tables: vec![],
            views: vec![View {
                id: "active_users".to_string(),
                schema_name: None,
                name: "active_users".to_string(),
                columns: vec![
                    Column {
                        id: ColumnId(0),
                        name: "id".to_string(),
                        data_type: "integer".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                    },
                    Column {
                        id: ColumnId(1),
                        name: "email".to_string(),
                        data_type: "text".to_string(),
                        nullable: false,
                        is_primary_key: false,
                        comment: None,
                    },
                ],
                definition: Some("SELECT id, email FROM users".to_string()),
            }],
            enums: vec![],
        };

        let diff = diff_schemas(&before, &after);

        assert!(diff.modified_views.is_empty());
        assert_eq!(diff.summary.view_columns_changed, 0);
    }

    #[test]
    fn test_modified_view_still_tracks_known_column_type_changes() {
        let before = Schema {
            tables: vec![],
            views: vec![create_test_view(
                "active_users",
                vec![("id", "integer")],
                Some("SELECT id FROM users"),
            )],
            enums: vec![],
        };
        let after = Schema {
            tables: vec![],
            views: vec![create_test_view(
                "active_users",
                vec![("id", "bigint")],
                Some("SELECT id FROM users"),
            )],
            enums: vec![],
        };

        let diff = diff_schemas(&before, &after);

        assert_eq!(diff.modified_views.len(), 1);
        assert_eq!(diff.modified_views[0].column_diffs.len(), 1);
        assert_eq!(diff.summary.view_columns_changed, 1);
    }

    #[test]
    fn test_modified_enum_tracks_removed_and_reordered_values() {
        let before = Schema {
            tables: vec![],
            views: vec![],
            enums: vec![create_test_enum(
                "status",
                &["draft", "published", "archived"],
            )],
        };
        let after = Schema {
            tables: vec![],
            views: vec![],
            enums: vec![create_test_enum("status", &["published", "draft"])],
        };

        let diff = diff_schemas(&before, &after);

        assert_eq!(diff.modified_enums.len(), 1);
        let enum_diff = &diff.modified_enums[0];
        assert_eq!(enum_diff.enum_name, "status");
        assert_eq!(enum_diff.value_diffs.len(), 3);
        assert!(
            enum_diff
                .value_diffs
                .iter()
                .any(|value| value.value == "archived" && value.change_kind == ChangeKind::Removed)
        );
        assert!(
            enum_diff
                .value_diffs
                .iter()
                .any(|value| value.value == "draft" && value.change_kind == ChangeKind::Modified)
        );
        assert_eq!(diff.summary.enums_modified, 1);
        assert_eq!(diff.summary.enum_values_changed, 3);
    }

    #[test]
    fn test_view_diff_matches_by_qualified_name_regardless_of_id() {
        let before = Schema {
            tables: vec![],
            views: vec![View {
                id: "public.active_users".to_string(),
                schema_name: Some("public".to_string()),
                name: "active_users".to_string(),
                columns: vec![Column {
                    id: ColumnId(0),
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: true,
                    is_primary_key: false,
                    comment: None,
                }],
                definition: Some("SELECT id FROM users".to_string()),
            }],
            enums: vec![],
        };
        let after = Schema {
            tables: vec![],
            views: vec![View {
                // Different `id` representation (e.g. introspect vs parser drift),
                // but the qualified name is the same so it must match.
                id: "v_42".to_string(),
                schema_name: Some("Public".to_string()),
                name: "Active_Users".to_string(),
                columns: vec![Column {
                    id: ColumnId(0),
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: true,
                    is_primary_key: false,
                    comment: None,
                }],
                definition: Some("SELECT id FROM users WHERE active".to_string()),
            }],
            enums: vec![],
        };

        let diff = diff_schemas(&before, &after);

        assert!(diff.added_views.is_empty(), "expected no added views");
        assert!(diff.removed_views.is_empty(), "expected no removed views");
        assert_eq!(diff.modified_views.len(), 1);
        assert!(diff.modified_views[0].definition_changed());
    }

    #[test]
    fn test_enum_diff_matches_by_qualified_name_regardless_of_id() {
        let before = Schema {
            tables: vec![],
            views: vec![],
            enums: vec![Enum {
                id: "public.status".to_string(),
                schema_name: Some("public".to_string()),
                name: "status".to_string(),
                values: vec!["draft".to_string()],
            }],
        };
        let after = Schema {
            tables: vec![],
            views: vec![],
            enums: vec![Enum {
                id: "enum_99".to_string(),
                schema_name: Some("Public".to_string()),
                name: "Status".to_string(),
                values: vec!["draft".to_string(), "published".to_string()],
            }],
        };

        let diff = diff_schemas(&before, &after);

        assert!(diff.added_enums.is_empty());
        assert!(diff.removed_enums.is_empty());
        assert_eq!(diff.modified_enums.len(), 1);
        assert!(
            diff.modified_enums[0]
                .value_diffs
                .iter()
                .any(|value| value.value == "published")
        );
    }

    #[test]
    fn test_schema_diff_serialization() {
        let diff = SchemaDiff {
            added_tables: vec!["new_table".to_string()],
            removed_tables: vec![],
            modified_tables: vec![],
            added_views: vec![],
            removed_views: vec![],
            modified_views: vec![],
            added_enums: vec![],
            removed_enums: vec![],
            modified_enums: vec![],
            summary: DiffSummary {
                tables_added: 1,
                tables_removed: 0,
                tables_modified: 0,
                columns_changed: 0,
                foreign_keys_changed: 0,
                indexes_changed: 0,
                views_added: 0,
                views_removed: 0,
                views_modified: 0,
                view_columns_changed: 0,
                view_definitions_changed: 0,
                enums_added: 0,
                enums_removed: 0,
                enums_modified: 0,
                enum_values_changed: 0,
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
