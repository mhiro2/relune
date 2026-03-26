//! Lint engine for schema analysis.
//!
//! This module provides lint rules to detect potential issues and
//! anti-patterns in database schemas.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use crate::diagnostic::{DiagnosticCode, Severity};
use crate::model::{ForeignKey, Schema, Table};

/// Unique identifier for a lint rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintRuleId {
    /// Table has no primary key.
    NoPrimaryKey,
    /// Table has no incoming or outgoing foreign keys.
    OrphanTable,
    /// More than 50% of columns are nullable.
    TooManyNullable,
    /// Table name suggests join table but has FK pattern issues.
    SuspiciousJoinTable,
    /// Multiple FKs to the same target table.
    DuplicatedFkPattern,
    /// Table or column name is not `snake_case` (ASCII lowercase with underscores).
    NonSnakeCaseIdentifier,
    /// Foreign key columns are not covered by the primary key or any index prefix.
    MissingForeignKeyIndex,
    /// Foreign key uses nullable columns, which often correlates with optional relations and lazy-loading (N+1) risk.
    NullableForeignKeyLazyLoad,
    /// Foreign key references columns that are not the full primary key or a unique index on the referenced table.
    ForeignKeyNonUniqueTarget,
}

impl LintRuleId {
    /// Returns the kebab-case string representation of the rule ID.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::NoPrimaryKey => "no-primary-key",
            Self::OrphanTable => "orphan-table",
            Self::TooManyNullable => "too-many-nullable",
            Self::SuspiciousJoinTable => "suspicious-join-table",
            Self::DuplicatedFkPattern => "duplicated-fk-pattern",
            Self::NonSnakeCaseIdentifier => "non-snake-case-identifier",
            Self::MissingForeignKeyIndex => "missing-foreign-key-index",
            Self::NullableForeignKeyLazyLoad => "nullable-foreign-key-lazy-load",
            Self::ForeignKeyNonUniqueTarget => "foreign-key-non-unique-target",
        }
    }

    /// Returns the default severity for this rule.
    #[must_use]
    pub const fn default_severity(&self) -> Severity {
        match self {
            Self::NoPrimaryKey
            | Self::TooManyNullable
            | Self::MissingForeignKeyIndex
            | Self::ForeignKeyNonUniqueTarget => Severity::Warning,
            Self::OrphanTable | Self::SuspiciousJoinTable | Self::NullableForeignKeyLazyLoad => {
                Severity::Info
            }
            Self::DuplicatedFkPattern | Self::NonSnakeCaseIdentifier => Severity::Hint,
        }
    }

    /// Returns a human-readable description of the rule.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::NoPrimaryKey => "Table has no primary key defined",
            Self::OrphanTable => "Table has no incoming or outgoing foreign keys",
            Self::TooManyNullable => "More than 50% of columns are nullable",
            Self::SuspiciousJoinTable => "Table name suggests join table but has FK pattern issues",
            Self::DuplicatedFkPattern => "Multiple foreign keys to the same target table",
            Self::NonSnakeCaseIdentifier => "Identifier is not snake_case ASCII",
            Self::MissingForeignKeyIndex => "Foreign key columns lack a supporting index prefix",
            Self::NullableForeignKeyLazyLoad => {
                "Nullable foreign key columns may encourage lazy-loading and N+1 queries"
            }
            Self::ForeignKeyNonUniqueTarget => {
                "Foreign key targets columns that are not a primary or unique key"
            }
        }
    }

    /// Returns the diagnostic code for this rule.
    #[must_use]
    pub fn diagnostic_code(&self) -> DiagnosticCode {
        let number = match self {
            Self::NoPrimaryKey => 1,
            Self::OrphanTable => 2,
            Self::TooManyNullable => 3,
            Self::SuspiciousJoinTable => 4,
            Self::DuplicatedFkPattern => 5,
            Self::NonSnakeCaseIdentifier => 6,
            Self::MissingForeignKeyIndex => 7,
            Self::NullableForeignKeyLazyLoad => 8,
            Self::ForeignKeyNonUniqueTarget => 9,
        };
        DiagnosticCode::new("LINT", number)
    }
}

impl fmt::Display for LintRuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A single lint finding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LintIssue {
    /// The rule that triggered this issue.
    pub rule_id: LintRuleId,
    /// Severity level for this issue.
    pub severity: Severity,
    /// Human-readable message describing the issue.
    pub message: String,
    /// The table name where the issue was found, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
    /// The column name where the issue was found, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_name: Option<String>,
    /// Optional hint for how to fix the issue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl LintIssue {
    /// Creates a new lint issue.
    #[must_use]
    pub fn new(rule_id: LintRuleId, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            rule_id,
            severity,
            message: message.into(),
            table_name: None,
            column_name: None,
            hint: None,
        }
    }

    /// Adds a table name to the issue.
    #[must_use]
    pub fn with_table(mut self, table_name: impl Into<String>) -> Self {
        self.table_name = Some(table_name.into());
        self
    }

    /// Adds a column name to the issue.
    #[must_use]
    pub fn with_column(mut self, column_name: impl Into<String>) -> Self {
        self.column_name = Some(column_name.into());
        self
    }

    /// Adds a hint to the issue.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// Statistics about lint results.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LintStats {
    /// Total number of issues found.
    pub total: usize,
    /// Number of errors.
    pub errors: usize,
    /// Number of warnings.
    pub warnings: usize,
    /// Number of info messages.
    pub infos: usize,
    /// Number of hints.
    pub hints: usize,
}

/// Result of linting a schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LintResult {
    /// All lint issues found.
    pub issues: Vec<LintIssue>,
    /// Statistics about the issues.
    pub stats: LintStats,
}

impl LintResult {
    /// Creates an empty lint result.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an issue to the result and updates stats.
    pub fn add_issue(&mut self, issue: LintIssue) {
        self.stats.total += 1;
        match issue.severity {
            Severity::Error => self.stats.errors += 1,
            Severity::Warning => self.stats.warnings += 1,
            Severity::Info => self.stats.infos += 1,
            Severity::Hint => self.stats.hints += 1,
        }
        self.issues.push(issue);
    }

    /// Returns true if there are any issues.
    #[must_use]
    pub const fn has_issues(&self) -> bool {
        !self.issues.is_empty()
    }

    /// Returns issues filtered by severity.
    #[must_use]
    pub fn issues_by_severity(&self, severity: Severity) -> Vec<&LintIssue> {
        self.issues
            .iter()
            .filter(|i| i.severity == severity)
            .collect()
    }

    /// Returns issues filtered by rule.
    #[must_use]
    pub fn issues_by_rule(&self, rule_id: LintRuleId) -> Vec<&LintIssue> {
        self.issues
            .iter()
            .filter(|i| i.rule_id == rule_id)
            .collect()
    }
}

/// Main entry point for linting a schema.
#[must_use]
pub fn lint_schema(schema: &Schema) -> LintResult {
    let mut result = LintResult::new();

    // Build FK relationship maps for orphan detection
    let (incoming_fks, outgoing_fks) = build_fk_maps(schema);

    for table in &schema.tables {
        check_no_primary_key(table, &mut result);
        check_orphan_table(table, &incoming_fks, &outgoing_fks, &mut result);
        check_too_many_nullable(table, &mut result);
        check_suspicious_join_table(table, &mut result);
        check_duplicated_fk_pattern(table, &mut result);
        check_non_snake_case_identifiers(table, &mut result);
        check_foreign_key_indexes_nullable_and_target(schema, table, &mut result);
    }

    // Sort issues by severity (errors first) then by table name
    result.issues.sort_by(|a, b| {
        let severity_order = |s: &Severity| match s {
            Severity::Error => 0,
            Severity::Warning => 1,
            Severity::Info => 2,
            Severity::Hint => 3,
        };
        severity_order(&a.severity)
            .cmp(&severity_order(&b.severity))
            .then_with(|| a.table_name.cmp(&b.table_name))
            .then_with(|| a.column_name.cmp(&b.column_name))
            .then_with(|| a.rule_id.as_str().cmp(b.rule_id.as_str()))
    });

    result
}

/// Type alias for FK map (table name -> list of foreign keys).
type FkMap = HashMap<String, Vec<Arc<ForeignKey>>>;

/// Build maps of incoming and outgoing foreign keys for each table.
///
/// Uses `Arc<ForeignKey>` to share FK references between incoming and outgoing maps
/// without cloning the full FK struct twice.
fn build_fk_maps(schema: &Schema) -> (FkMap, FkMap) {
    let mut incoming: FkMap = HashMap::new();
    let mut outgoing: FkMap = HashMap::new();

    for table in &schema.tables {
        let table_name = table.name.to_lowercase();

        for fk in &table.foreign_keys {
            let fk = Arc::new(fk.clone());

            // Record outgoing FK from this table
            outgoing
                .entry(table_name.clone())
                .or_default()
                .push(Arc::clone(&fk));

            // Record incoming FK to the target table
            let target_name = fk.to_table.to_lowercase();
            incoming.entry(target_name).or_default().push(fk);
        }
    }

    (incoming, outgoing)
}

/// Check: Table has no primary key.
fn check_no_primary_key(table: &Table, result: &mut LintResult) {
    let has_pk = table.columns.iter().any(|c| c.is_primary_key);
    let has_pk_index = table
        .indexes
        .iter()
        .any(|idx| idx.is_unique && idx.columns.len() == 1);

    if !has_pk && !has_pk_index {
        result.add_issue(
            LintIssue::new(
                LintRuleId::NoPrimaryKey,
                LintRuleId::NoPrimaryKey.default_severity(),
                format!("Table '{}' has no primary key defined", table.name),
            )
            .with_table(&table.name)
            .with_hint("Consider adding a primary key column (e.g., 'id') or a unique index"),
        );
    }
}

/// Check: Table has no incoming or outgoing foreign keys.
fn check_orphan_table(
    table: &Table,
    incoming_fks: &FkMap,
    outgoing_fks: &FkMap,
    result: &mut LintResult,
) {
    let table_name_lower = table.name.to_lowercase();
    let has_incoming = incoming_fks
        .get(&table_name_lower)
        .is_some_and(|fks| !fks.is_empty());
    let has_outgoing = outgoing_fks
        .get(&table_name_lower)
        .is_some_and(|fks| !fks.is_empty());

    if !has_incoming && !has_outgoing {
        result.add_issue(
            LintIssue::new(
                LintRuleId::OrphanTable,
                LintRuleId::OrphanTable.default_severity(),
                format!("Table '{}' has no foreign key relationships", table.name),
            )
            .with_table(&table.name)
            .with_hint("Consider if this table should reference or be referenced by other tables"),
        );
    }
}

/// Check: More than 50% of columns are nullable.
fn check_too_many_nullable(table: &Table, result: &mut LintResult) {
    if table.columns.is_empty() {
        return;
    }

    let nullable_count = table.columns.iter().filter(|c| c.nullable).count();
    let total_columns = table.columns.len();
    #[allow(clippy::cast_precision_loss)]
    let nullable_ratio = nullable_count as f64 / total_columns as f64;

    if nullable_ratio > 0.5 {
        let nullable_columns: Vec<&str> = table
            .columns
            .iter()
            .filter(|c| c.nullable)
            .map(|c| c.name.as_str())
            .collect();

        result.add_issue(
            LintIssue::new(
                LintRuleId::TooManyNullable,
                LintRuleId::TooManyNullable.default_severity(),
                format!(
                    "Table '{}' has {}/{} ({:.0}%) nullable columns",
                    table.name,
                    nullable_count,
                    total_columns,
                    nullable_ratio * 100.0
                ),
            )
            .with_table(&table.name)
            .with_hint(format!(
                "Nullable columns: {}. Consider making frequently used columns NOT NULL",
                nullable_columns.join(", ")
            )),
        );
    }
}

/// Check: Table name suggests join table but has FK pattern issues.
fn check_suspicious_join_table(table: &Table, result: &mut LintResult) {
    let name_lower = table.name.to_lowercase();

    // Common join table naming patterns
    let is_likely_join_table = name_lower.contains('_')
        && (name_lower.ends_with("_map")
            || name_lower.ends_with("_link")
            || name_lower.ends_with("_junction")
            || name_lower.ends_with("_association")
            // Pattern like "user_role" or "order_product"
            || looks_like_join_table_name(&name_lower));

    if !is_likely_join_table {
        return;
    }

    // Check FK count - join tables typically have exactly 2 FKs
    let fk_count = table.foreign_keys.len();

    if fk_count == 0 {
        result.add_issue(
            LintIssue::new(
                LintRuleId::SuspiciousJoinTable,
                LintRuleId::SuspiciousJoinTable.default_severity(),
                format!(
                    "Table '{}' appears to be a join table but has no foreign keys",
                    table.name
                ),
            )
            .with_table(&table.name)
            .with_hint("Join tables should have foreign keys to the tables they connect"),
        );
    } else if fk_count == 1 {
        result.add_issue(
            LintIssue::new(
                LintRuleId::SuspiciousJoinTable,
                LintRuleId::SuspiciousJoinTable.default_severity(),
                format!(
                    "Table '{}' appears to be a join table but only has 1 foreign key",
                    table.name
                ),
            )
            .with_table(&table.name)
            .with_hint("Join tables typically have 2 foreign keys for many-to-many relationships"),
        );
    } else if fk_count > 2 {
        // Check if FKs go to different tables
        let target_tables: HashSet<String> = table
            .foreign_keys
            .iter()
            .map(|fk| fk.to_table.to_lowercase())
            .collect();

        if target_tables.len() < fk_count {
            // Some FKs go to the same table - might be intentional but worth noting
            result.add_issue(
                LintIssue::new(
                    LintRuleId::SuspiciousJoinTable,
                    LintRuleId::SuspiciousJoinTable.default_severity(),
                    format!(
                        "Table '{}' has {} foreign keys to {} different table(s)",
                        table.name,
                        fk_count,
                        target_tables.len()
                    ),
                )
                .with_table(&table.name)
                .with_hint("Review if multiple FKs to the same table are intentional"),
            );
        }
    }
}

/// Check if the table name looks like a join table (e.g., `"user_role"`, `"order_product"`).
fn looks_like_join_table_name(name: &str) -> bool {
    // Split by underscore and check if we have exactly 2 parts
    // that could each be a table name
    let parts: Vec<&str> = name.split('_').collect();
    if parts.len() != 2 {
        return false;
    }

    // Each part should be at least 3 characters (heuristic for valid table name)
    parts.iter().all(|p| p.len() >= 3)
}

/// Check: Multiple FKs to the same target table.
fn check_duplicated_fk_pattern(table: &Table, result: &mut LintResult) {
    // Group FKs by target table (case-insensitive)
    let mut fk_by_target: HashMap<String, Vec<&ForeignKey>> = HashMap::new();

    for fk in &table.foreign_keys {
        fk_by_target
            .entry(fk.to_table.to_lowercase())
            .or_default()
            .push(fk);
    }

    // Find tables with multiple FKs to the same target
    for (target_table, fks) in &fk_by_target {
        if fks.len() > 1 {
            let fk_names: Vec<String> = fks
                .iter()
                .map(|fk| {
                    fk.name
                        .clone()
                        .unwrap_or_else(|| fk.from_columns.join(", "))
                })
                .collect();

            result.add_issue(
                LintIssue::new(
                    LintRuleId::DuplicatedFkPattern,
                    LintRuleId::DuplicatedFkPattern.default_severity(),
                    format!(
                        "Table '{}' has {} foreign keys to table '{}'",
                        table.name,
                        fks.len(),
                        target_table
                    ),
                )
                .with_table(&table.name)
                .with_hint(format!(
                    "FKs: {}. This may indicate a design pattern or potential consolidation",
                    fk_names.join("; ")
                )),
            );
        }
    }
}

/// `true` if `s` is non-empty ASCII `snake_case`: starts with `a-z`, then only `a-z`, `0-9`, `_`.
fn is_snake_case(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Returns whether `index_cols` has `fk_cols` as an ordered prefix (exact name match).
fn column_list_has_prefix(index_cols: &[String], fk_cols: &[String]) -> bool {
    if index_cols.len() < fk_cols.len() {
        return false;
    }
    index_cols.iter().zip(fk_cols.iter()).all(|(a, b)| a == b)
}

/// Returns whether the FK column list is covered by the table PK column order or any index prefix.
fn fk_columns_are_indexed(table: &Table, fk_cols: &[String]) -> bool {
    if fk_cols.is_empty() {
        return false;
    }
    let pk_cols: Vec<String> = table
        .columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.clone())
        .collect();
    if column_list_has_prefix(&pk_cols, fk_cols) {
        return true;
    }
    table
        .indexes
        .iter()
        .any(|idx| column_list_has_prefix(&idx.columns, fk_cols))
}

fn resolve_referenced_table<'a>(schema: &'a Schema, to_table: &str) -> Option<&'a Table> {
    let key = to_table.to_lowercase();
    schema
        .tables
        .iter()
        .find(|t| t.name.to_lowercase() == key || t.stable_id.to_lowercase() == key)
}

/// `true` when `to_columns` exactly matches the referenced table PK column list or some unique index column list.
fn referenced_columns_are_unique(ref_table: &Table, to_columns: &[String]) -> bool {
    if to_columns.is_empty() {
        return false;
    }
    let pk_cols: Vec<String> = ref_table
        .columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.clone())
        .collect();
    if !pk_cols.is_empty() && pk_cols == to_columns {
        return true;
    }
    ref_table
        .indexes
        .iter()
        .any(|idx| idx.is_unique && idx.columns == to_columns)
}

fn fk_labels_for_message(fk: &ForeignKey) -> String {
    fk.name.clone().unwrap_or_else(|| {
        format!(
            "{} -> {}({})",
            fk.from_columns.join(", "),
            fk.to_table,
            fk.to_columns.join(", ")
        )
    })
}

fn table_nullable_columns(table: &Table) -> HashMap<&str, bool> {
    table
        .columns
        .iter()
        .map(|column| (column.name.as_str(), column.nullable))
        .collect()
}

/// Check: foreign key columns should be indexed.
fn check_foreign_key_index_coverage(table: &Table, fk: &ForeignKey, result: &mut LintResult) {
    if fk.from_columns.is_empty() || fk_columns_are_indexed(table, &fk.from_columns) {
        return;
    }

    result.add_issue(
        LintIssue::new(
            LintRuleId::MissingForeignKeyIndex,
            LintRuleId::MissingForeignKeyIndex.default_severity(),
            format!(
                "Foreign key on table '{}' ({}) has no index whose leading columns match {:?}",
                table.name,
                fk_labels_for_message(fk),
                fk.from_columns
            ),
        )
        .with_table(&table.name)
        .with_hint(
            "Add an index starting with the FK columns (same order) to speed joins and cascades",
        ),
    );
}

/// Check: nullable FK columns may encourage lazy loading.
fn check_nullable_foreign_key_lazy_load(
    table: &Table,
    fk: &ForeignKey,
    nullable_columns: &HashMap<&str, bool>,
    result: &mut LintResult,
) {
    if fk.from_columns.is_empty() {
        return;
    }

    let any_from_nullable = fk.from_columns.iter().any(|col_name| {
        nullable_columns
            .get(col_name.as_str())
            .copied()
            .unwrap_or(false)
    });
    if !any_from_nullable {
        return;
    }

    result.add_issue(
        LintIssue::new(
            LintRuleId::NullableForeignKeyLazyLoad,
            LintRuleId::NullableForeignKeyLazyLoad.default_severity(),
            format!(
                "Foreign key on table '{}' ({}) includes nullable column(s); optional relations often trigger per-row lookups (N+1) in ORMs",
                table.name,
                fk_labels_for_message(fk)
            ),
        )
        .with_table(&table.name)
        .with_hint(
            "Use eager loading, joins, or dataloader patterns; consider NOT NULL if the relation is required",
        ),
    );
}

/// Check: FK targets should resolve to a primary or unique key.
fn check_foreign_key_target_uniqueness(
    schema: &Schema,
    table: &Table,
    fk: &ForeignKey,
    result: &mut LintResult,
) {
    if fk.to_columns.is_empty() {
        return;
    }

    let Some(ref_table) = resolve_referenced_table(schema, &fk.to_table) else {
        return;
    };
    if referenced_columns_are_unique(ref_table, &fk.to_columns) {
        return;
    }

    result.add_issue(
        LintIssue::new(
            LintRuleId::ForeignKeyNonUniqueTarget,
            LintRuleId::ForeignKeyNonUniqueTarget.default_severity(),
            format!(
                "Foreign key on table '{}' ({}) references columns on '{}' that are not the full primary key or a unique index",
                table.name,
                fk_labels_for_message(fk),
                ref_table.name
            ),
        )
        .with_table(&table.name)
        .with_hint(
            "Point the FK at the referenced table primary key or a unique constraint with matching column order",
        ),
    );
}

/// Check: table and column identifiers use `snake_case`.
fn check_non_snake_case_identifiers(table: &Table, result: &mut LintResult) {
    if !is_snake_case(&table.name) {
        result.add_issue(
            LintIssue::new(
                LintRuleId::NonSnakeCaseIdentifier,
                LintRuleId::NonSnakeCaseIdentifier.default_severity(),
                format!(
                    "Table name '{}' should use snake_case (lowercase ASCII letters, digits, underscores)",
                    table.name
                ),
            )
            .with_table(&table.name)
            .with_hint("Example: rename to `user_accounts` instead of `UserAccounts`"),
        );
    }
    for col in &table.columns {
        if !is_snake_case(&col.name) {
            result.add_issue(
                LintIssue::new(
                    LintRuleId::NonSnakeCaseIdentifier,
                    LintRuleId::NonSnakeCaseIdentifier.default_severity(),
                    format!(
                        "Column '{}' on table '{}' should use snake_case (lowercase ASCII letters, digits, underscores)",
                        col.name, table.name
                    ),
                )
                .with_table(&table.name)
                .with_column(&col.name),
            );
        }
    }
}

/// Foreign key index coverage, nullable / lazy-load signal, and referenced unique target.
fn check_foreign_key_indexes_nullable_and_target(
    schema: &Schema,
    table: &Table,
    result: &mut LintResult,
) {
    let nullable_columns = table_nullable_columns(table);

    for fk in &table.foreign_keys {
        check_foreign_key_index_coverage(table, fk, result);
        check_nullable_foreign_key_lazy_load(table, fk, &nullable_columns, result);
        check_foreign_key_target_uniqueness(schema, table, fk, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, ColumnId, Index, ReferentialAction, TableId};

    fn create_test_table(
        name: &str,
        columns: Vec<Column>,
        foreign_keys: Vec<ForeignKey>,
        indexes: Vec<crate::model::Index>,
    ) -> Table {
        Table {
            id: TableId(0),
            stable_id: name.to_string(),
            schema_name: None,
            name: name.to_string(),
            columns,
            foreign_keys,
            indexes,
            comment: None,
        }
    }

    fn create_column(name: &str, nullable: bool, is_pk: bool) -> Column {
        Column {
            id: ColumnId(0),
            name: name.to_string(),
            data_type: "varchar".to_string(),
            nullable,
            is_primary_key: is_pk,
            comment: None,
        }
    }

    fn create_fk(to_table: &str, from_columns: &[&str]) -> ForeignKey {
        ForeignKey {
            name: None,
            from_columns: from_columns.iter().map(ToString::to_string).collect(),
            to_schema: None,
            to_table: to_table.to_string(),
            to_columns: vec!["id".to_string()],
            on_delete: ReferentialAction::NoAction,
            on_update: ReferentialAction::NoAction,
        }
    }

    #[test]
    fn test_lint_rule_id_as_str() {
        assert_eq!(LintRuleId::NoPrimaryKey.as_str(), "no-primary-key");
        assert_eq!(LintRuleId::OrphanTable.as_str(), "orphan-table");
        assert_eq!(LintRuleId::TooManyNullable.as_str(), "too-many-nullable");
        assert_eq!(
            LintRuleId::SuspiciousJoinTable.as_str(),
            "suspicious-join-table"
        );
        assert_eq!(
            LintRuleId::DuplicatedFkPattern.as_str(),
            "duplicated-fk-pattern"
        );
        assert_eq!(
            LintRuleId::NonSnakeCaseIdentifier.as_str(),
            "non-snake-case-identifier"
        );
        assert_eq!(
            LintRuleId::MissingForeignKeyIndex.as_str(),
            "missing-foreign-key-index"
        );
        assert_eq!(
            LintRuleId::NullableForeignKeyLazyLoad.as_str(),
            "nullable-foreign-key-lazy-load"
        );
        assert_eq!(
            LintRuleId::ForeignKeyNonUniqueTarget.as_str(),
            "foreign-key-non-unique-target"
        );
    }

    #[test]
    fn test_lint_rule_id_default_severity() {
        assert_eq!(
            LintRuleId::NoPrimaryKey.default_severity(),
            Severity::Warning
        );
        assert_eq!(LintRuleId::OrphanTable.default_severity(), Severity::Info);
        assert_eq!(
            LintRuleId::TooManyNullable.default_severity(),
            Severity::Warning
        );
        assert_eq!(
            LintRuleId::SuspiciousJoinTable.default_severity(),
            Severity::Info
        );
        assert_eq!(
            LintRuleId::DuplicatedFkPattern.default_severity(),
            Severity::Hint
        );
        assert_eq!(
            LintRuleId::NonSnakeCaseIdentifier.default_severity(),
            Severity::Hint
        );
        assert_eq!(
            LintRuleId::MissingForeignKeyIndex.default_severity(),
            Severity::Warning
        );
        assert_eq!(
            LintRuleId::NullableForeignKeyLazyLoad.default_severity(),
            Severity::Info
        );
        assert_eq!(
            LintRuleId::ForeignKeyNonUniqueTarget.default_severity(),
            Severity::Warning
        );
    }

    #[test]
    fn test_no_primary_key_detection() {
        let table = create_test_table(
            "users",
            vec![
                create_column("name", false, false),
                create_column("email", false, false),
            ],
            vec![],
            vec![],
        );

        let mut result = LintResult::new();
        check_no_primary_key(&table, &mut result);

        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].rule_id, LintRuleId::NoPrimaryKey);
        assert_eq!(result.stats.warnings, 1);
    }

    #[test]
    fn test_no_primary_key_with_pk() {
        let table = create_test_table(
            "users",
            vec![
                create_column("id", false, true),
                create_column("name", false, false),
            ],
            vec![],
            vec![],
        );

        let mut result = LintResult::new();
        check_no_primary_key(&table, &mut result);

        assert_eq!(result.issues.len(), 0);
    }

    #[test]
    fn test_too_many_nullable_detection() {
        let table = create_test_table(
            "profiles",
            vec![
                create_column("id", false, true),
                create_column("bio", true, false),
                create_column("avatar", true, false),
                create_column("website", true, false),
            ],
            vec![],
            vec![],
        );

        let mut result = LintResult::new();
        check_too_many_nullable(&table, &mut result);

        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].rule_id, LintRuleId::TooManyNullable);
    }

    #[test]
    fn test_duplicated_fk_pattern() {
        let table = create_test_table(
            "orders",
            vec![create_column("id", false, true)],
            vec![
                create_fk("users", &["created_by"]),
                create_fk("users", &["updated_by"]),
            ],
            vec![],
        );

        let mut result = LintResult::new();
        check_duplicated_fk_pattern(&table, &mut result);

        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].rule_id, LintRuleId::DuplicatedFkPattern);
    }

    #[test]
    fn test_lint_result_stats() {
        let mut result = LintResult::new();
        result.add_issue(LintIssue::new(
            LintRuleId::NoPrimaryKey,
            Severity::Warning,
            "test",
        ));
        result.add_issue(LintIssue::new(
            LintRuleId::OrphanTable,
            Severity::Info,
            "test",
        ));

        assert_eq!(result.stats.total, 2);
        assert_eq!(result.stats.warnings, 1);
        assert_eq!(result.stats.infos, 1);
    }

    #[test]
    fn test_lint_schema_empty() {
        let schema = Schema::default();
        let result = lint_schema(&schema);

        assert_eq!(result.issues.len(), 0);
        assert_eq!(result.stats.total, 0);
    }

    #[test]
    fn test_lint_issue_with_options() {
        let issue = LintIssue::new(LintRuleId::NoPrimaryKey, Severity::Warning, "Test message")
            .with_table("users")
            .with_column("id")
            .with_hint("Add a primary key");

        assert_eq!(issue.table_name, Some("users".to_string()));
        assert_eq!(issue.column_name, Some("id".to_string()));
        assert_eq!(issue.hint, Some("Add a primary key".to_string()));
    }

    #[test]
    fn test_lint_result_filtering() {
        let mut result = LintResult::new();
        result.add_issue(LintIssue::new(
            LintRuleId::NoPrimaryKey,
            Severity::Warning,
            "warning1",
        ));
        result.add_issue(LintIssue::new(
            LintRuleId::NoPrimaryKey,
            Severity::Warning,
            "warning2",
        ));
        result.add_issue(LintIssue::new(
            LintRuleId::OrphanTable,
            Severity::Info,
            "info1",
        ));

        let warnings = result.issues_by_severity(Severity::Warning);
        assert_eq!(warnings.len(), 2);

        let no_pk_issues = result.issues_by_rule(LintRuleId::NoPrimaryKey);
        assert_eq!(no_pk_issues.len(), 2);
    }

    #[test]
    fn test_non_snake_case_table_name() {
        let table = create_test_table(
            "UserAccounts",
            vec![create_column("id", false, true)],
            vec![],
            vec![],
        );
        let mut result = LintResult::new();
        check_non_snake_case_identifiers(&table, &mut result);
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].rule_id, LintRuleId::NonSnakeCaseIdentifier);
    }

    #[test]
    fn test_missing_foreign_key_index() {
        let table = create_test_table(
            "posts",
            vec![
                create_column("id", false, true),
                create_column("user_id", false, false),
            ],
            vec![create_fk("users", &["user_id"])],
            vec![],
        );
        let mut result = LintResult::new();
        check_foreign_key_indexes_nullable_and_target(&Schema::default(), &table, &mut result);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.rule_id == LintRuleId::MissingForeignKeyIndex)
        );
    }

    #[test]
    fn test_foreign_key_index_covered_by_index() {
        let table = create_test_table(
            "posts",
            vec![
                create_column("id", false, true),
                create_column("user_id", false, false),
            ],
            vec![create_fk("users", &["user_id"])],
            vec![Index {
                name: Some("posts_user_id_idx".to_string()),
                columns: vec!["user_id".to_string()],
                is_unique: false,
            }],
        );
        let mut result = LintResult::new();
        check_foreign_key_indexes_nullable_and_target(&Schema::default(), &table, &mut result);
        assert!(
            !result
                .issues
                .iter()
                .any(|i| i.rule_id == LintRuleId::MissingForeignKeyIndex)
        );
    }

    #[test]
    fn test_fk_columns_are_indexed_rejects_empty_fk_columns() {
        let table = create_test_table(
            "posts",
            vec![create_column("id", false, true)],
            vec![],
            vec![],
        );

        assert!(!fk_columns_are_indexed(&table, &[]));
    }

    #[test]
    fn test_nullable_foreign_key_lazy_load() {
        let table = create_test_table(
            "posts",
            vec![
                create_column("id", false, true),
                create_column("user_id", true, false),
            ],
            vec![create_fk("users", &["user_id"])],
            vec![Index {
                name: Some("i".to_string()),
                columns: vec!["user_id".to_string()],
                is_unique: false,
            }],
        );
        let mut result = LintResult::new();
        check_foreign_key_indexes_nullable_and_target(&Schema::default(), &table, &mut result);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.rule_id == LintRuleId::NullableForeignKeyLazyLoad)
        );
    }

    #[test]
    fn test_foreign_key_non_unique_target() {
        let users = create_test_table(
            "users",
            vec![
                create_column("id", false, true),
                create_column("email", false, false),
            ],
            vec![],
            vec![],
        );
        let posts = create_test_table(
            "posts",
            vec![
                create_column("id", false, true),
                create_column("user_ref", false, false),
            ],
            vec![ForeignKey {
                name: None,
                from_columns: vec!["user_ref".to_string()],
                to_schema: None,
                to_table: "users".to_string(),
                to_columns: vec!["email".to_string()],
                on_delete: ReferentialAction::NoAction,
                on_update: ReferentialAction::NoAction,
            }],
            vec![Index {
                name: Some("posts_user_ref".to_string()),
                columns: vec!["user_ref".to_string()],
                is_unique: false,
            }],
        );
        let schema = Schema {
            tables: vec![users, posts],
            ..Schema::default()
        };
        let result = lint_schema(&schema);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.rule_id == LintRuleId::ForeignKeyNonUniqueTarget)
        );
    }
}
