//! Lint engine for schema analysis.
//!
//! This module provides lint rules to detect potential issues and
//! anti-patterns in database schemas.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use petgraph::algo::kosaraju_scc;
use petgraph::graphmap::DiGraphMap;

use crate::diagnostic::{DiagnosticCode, Severity};
use crate::model::{
    ForeignKey, ForeignKeyTargetResolution, Schema, Table, TableId, resolve_table_reference,
};

/// Unique identifier for a lint rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintRuleId {
    /// Table has no primary key.
    NoPrimaryKey,
    /// Table has no comment.
    MissingTableComment,
    /// Column has no comment.
    MissingColumnComment,
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
    /// Foreign key references a table that cannot be resolved (missing or ambiguous).
    UnresolvedForeignKey,
    /// Table participates in a foreign key cycle.
    CircularForeignKey,
}

/// Category of a lint rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintRuleCategory {
    /// Structural schema design rules.
    Structure,
    /// Foreign key and relational integrity rules.
    Relationships,
    /// Identifier naming conventions.
    Naming,
    /// Documentation and comment coverage rules.
    Documentation,
}

impl LintRuleCategory {
    /// Returns the kebab-case string representation of the category.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Structure => "structure",
            Self::Relationships => "relationships",
            Self::Naming => "naming",
            Self::Documentation => "documentation",
        }
    }

    /// Returns a short human-readable description of the category.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::Structure => "Primary keys, nullability, and join/orphan heuristics",
            Self::Relationships => "Foreign key coverage, target integrity, ambiguity, and cycles",
            Self::Naming => "Identifier naming conventions",
            Self::Documentation => "Table and column comment coverage",
        }
    }
}

impl fmt::Display for LintRuleCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Lint profile for schema review.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintProfile {
    /// Balanced schema review profile for everyday CI checks.
    #[default]
    Default,
    /// Stricter review profile with full documentation coverage checks.
    Strict,
}

impl LintProfile {
    /// Returns the kebab-case string representation of the profile.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Strict => "strict",
        }
    }

    /// Returns the rules enabled by the profile.
    #[must_use]
    pub const fn default_rules(&self) -> &'static [LintRuleId] {
        match self {
            Self::Default => &[
                LintRuleId::NoPrimaryKey,
                LintRuleId::MissingTableComment,
                LintRuleId::OrphanTable,
                LintRuleId::TooManyNullable,
                LintRuleId::SuspiciousJoinTable,
                LintRuleId::DuplicatedFkPattern,
                LintRuleId::NonSnakeCaseIdentifier,
                LintRuleId::MissingForeignKeyIndex,
                LintRuleId::NullableForeignKeyLazyLoad,
                LintRuleId::ForeignKeyNonUniqueTarget,
                LintRuleId::UnresolvedForeignKey,
                LintRuleId::CircularForeignKey,
            ],
            Self::Strict => LintRuleId::all(),
        }
    }
}

impl fmt::Display for LintProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Serializable metadata for one lint rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LintRuleMetadata {
    /// Stable rule identifier.
    pub rule_id: LintRuleId,
    /// Rule category.
    pub category: LintRuleCategory,
    /// Default severity used by the rule.
    pub default_severity: Severity,
    /// Human-readable description of the rule.
    pub description: String,
}

impl LintRuleId {
    /// Returns every available lint rule.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::NoPrimaryKey,
            Self::MissingTableComment,
            Self::MissingColumnComment,
            Self::OrphanTable,
            Self::TooManyNullable,
            Self::SuspiciousJoinTable,
            Self::DuplicatedFkPattern,
            Self::NonSnakeCaseIdentifier,
            Self::MissingForeignKeyIndex,
            Self::NullableForeignKeyLazyLoad,
            Self::ForeignKeyNonUniqueTarget,
            Self::UnresolvedForeignKey,
            Self::CircularForeignKey,
        ]
    }

    /// Returns the kebab-case string representation of the rule ID.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::NoPrimaryKey => "no-primary-key",
            Self::MissingTableComment => "missing-table-comment",
            Self::MissingColumnComment => "missing-column-comment",
            Self::OrphanTable => "orphan-table",
            Self::TooManyNullable => "too-many-nullable",
            Self::SuspiciousJoinTable => "suspicious-join-table",
            Self::DuplicatedFkPattern => "duplicated-fk-pattern",
            Self::NonSnakeCaseIdentifier => "non-snake-case-identifier",
            Self::MissingForeignKeyIndex => "missing-foreign-key-index",
            Self::NullableForeignKeyLazyLoad => "nullable-foreign-key-lazy-load",
            Self::ForeignKeyNonUniqueTarget => "foreign-key-non-unique-target",
            Self::UnresolvedForeignKey => "unresolved-foreign-key",
            Self::CircularForeignKey => "circular-foreign-key",
        }
    }

    /// Returns the default severity for this rule.
    #[must_use]
    pub const fn default_severity(&self) -> Severity {
        match self {
            Self::NoPrimaryKey
            | Self::MissingTableComment
            | Self::TooManyNullable
            | Self::MissingForeignKeyIndex
            | Self::ForeignKeyNonUniqueTarget
            | Self::UnresolvedForeignKey
            | Self::CircularForeignKey => Severity::Warning,
            Self::OrphanTable | Self::SuspiciousJoinTable | Self::NullableForeignKeyLazyLoad => {
                Severity::Info
            }
            Self::DuplicatedFkPattern
            | Self::NonSnakeCaseIdentifier
            | Self::MissingColumnComment => Severity::Hint,
        }
    }

    /// Returns a human-readable description of the rule.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::NoPrimaryKey => "Table has no primary key defined",
            Self::MissingTableComment => "Table has no comment",
            Self::MissingColumnComment => "Column has no comment",
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
            Self::UnresolvedForeignKey => "Foreign key references a table that cannot be resolved",
            Self::CircularForeignKey => "Foreign key participates in a cross-table cycle",
        }
    }

    /// Returns the review category for this rule.
    #[must_use]
    pub const fn category(&self) -> LintRuleCategory {
        match self {
            Self::NoPrimaryKey
            | Self::OrphanTable
            | Self::TooManyNullable
            | Self::SuspiciousJoinTable => LintRuleCategory::Structure,
            Self::NonSnakeCaseIdentifier => LintRuleCategory::Naming,
            Self::MissingTableComment | Self::MissingColumnComment => {
                LintRuleCategory::Documentation
            }
            Self::DuplicatedFkPattern
            | Self::MissingForeignKeyIndex
            | Self::NullableForeignKeyLazyLoad
            | Self::ForeignKeyNonUniqueTarget
            | Self::UnresolvedForeignKey
            | Self::CircularForeignKey => LintRuleCategory::Relationships,
        }
    }

    /// Returns serializable metadata for this rule.
    #[must_use]
    pub fn metadata(&self) -> LintRuleMetadata {
        LintRuleMetadata {
            rule_id: *self,
            category: self.category(),
            default_severity: self.default_severity(),
            description: self.description().to_string(),
        }
    }

    /// Returns the diagnostic code for this rule.
    #[must_use]
    pub fn diagnostic_code(&self) -> DiagnosticCode {
        let number = match self {
            Self::NoPrimaryKey => 1,
            Self::MissingTableComment => 2,
            Self::MissingColumnComment => 3,
            Self::OrphanTable => 4,
            Self::TooManyNullable => 5,
            Self::SuspiciousJoinTable => 6,
            Self::DuplicatedFkPattern => 7,
            Self::NonSnakeCaseIdentifier => 8,
            Self::MissingForeignKeyIndex => 9,
            Self::NullableForeignKeyLazyLoad => 10,
            Self::ForeignKeyNonUniqueTarget => 11,
            Self::UnresolvedForeignKey => 12,
            Self::CircularForeignKey => 13,
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
    /// Review category for this issue.
    pub category: LintRuleCategory,
    /// Severity level for this issue.
    pub severity: Severity,
    /// Human-readable message describing the issue.
    pub message: String,
    /// Stable table identifier for programmatic use (matches `Table::stable_id`).
    ///
    /// Renderers and overlay builders use this to map lint issues to diagram nodes
    /// without ambiguity, even in multi-schema environments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_id: Option<String>,
    /// Human-readable table name for display (e.g., CLI output).
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
            category: rule_id.category(),
            severity,
            message: message.into(),
            table_id: None,
            table_name: None,
            column_name: None,
            hint: None,
        }
    }

    /// Sets the stable table identifier (matches `Table::stable_id`).
    #[must_use]
    pub fn with_table_id(mut self, table_id: impl Into<String>) -> Self {
        self.table_id = Some(table_id.into());
        self
    }

    /// Sets the human-readable table name for display.
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
        check_missing_table_comment(table, &mut result);
        check_missing_column_comments(table, &mut result);
        check_no_primary_key(table, &mut result);
        check_orphan_table(table, &incoming_fks, &outgoing_fks, &mut result);
        check_too_many_nullable(table, &mut result);
        check_suspicious_join_table(table, &mut result);
        check_duplicated_fk_pattern(table, &mut result);
        check_non_snake_case_identifiers(table, &mut result);
        check_unresolved_foreign_keys(schema, table, &mut result);
        check_foreign_key_indexes_nullable_and_target(schema, table, &mut result);
    }
    check_circular_foreign_keys(schema, &mut result);

    // Sort issues by severity (errors first) then by table identifier.
    result.issues.sort_by(|a, b| {
        let severity_order = |s: &Severity| match s {
            Severity::Error => 0,
            Severity::Warning => 1,
            Severity::Info => 2,
            Severity::Hint => 3,
        };
        severity_order(&a.severity)
            .cmp(&severity_order(&b.severity))
            .then_with(|| a.table_id.cmp(&b.table_id))
            .then_with(|| a.column_name.cmp(&b.column_name))
            .then_with(|| a.rule_id.as_str().cmp(b.rule_id.as_str()))
    });

    result
}

/// Type alias for FK map keyed by table identifier.
type FkMap = HashMap<TableId, Vec<Arc<ForeignKey>>>;

/// Build maps of incoming and outgoing foreign keys for each table.
///
/// Uses `Arc<ForeignKey>` to share FK references between incoming and outgoing maps
/// without cloning the full FK struct twice.
fn build_fk_maps(schema: &Schema) -> (FkMap, FkMap) {
    let mut incoming: FkMap = HashMap::new();
    let mut outgoing: FkMap = HashMap::new();

    for table in &schema.tables {
        for fk in &table.foreign_keys {
            let fk = Arc::new(fk.clone());

            // Record outgoing FK from this table
            outgoing.entry(table.id).or_default().push(Arc::clone(&fk));

            // Record incoming FK to the target table
            if let Some(target_table) = resolve_referenced_table(schema, table, fk.as_ref()) {
                incoming.entry(target_table.id).or_default().push(fk);
            }
        }
    }

    (incoming, outgoing)
}

/// Build a table dependency graph from resolved foreign keys.
fn fk_dependency_graph(schema: &Schema) -> DiGraphMap<TableId, ()> {
    let mut graph = DiGraphMap::new();
    for table in &schema.tables {
        graph.add_node(table.id);
        for fk in &table.foreign_keys {
            if let Some(target_table) = resolve_referenced_table(schema, table, fk) {
                graph.add_edge(table.id, target_table.id, ());
            }
        }
    }
    graph
}

/// Check: table comments should be present for schema review.
fn check_missing_table_comment(table: &Table, result: &mut LintResult) {
    if table
        .comment
        .as_deref()
        .is_some_and(|comment| !comment.trim().is_empty())
    {
        return;
    }

    result.add_issue(
        LintIssue::new(
            LintRuleId::MissingTableComment,
            LintRuleId::MissingTableComment.default_severity(),
            format!(
                "Table '{}' is missing a comment that explains its role",
                table.qualified_name()
            ),
        )
        .with_table_id(&table.stable_id)
        .with_table(table.qualified_name())
        .with_hint(
            "Add a table comment in the source schema so reviewers can understand the table intent",
        ),
    );
}

/// Check: column comments should be present for stricter schema review.
fn check_missing_column_comments(table: &Table, result: &mut LintResult) {
    for column in &table.columns {
        if column
            .comment
            .as_deref()
            .is_some_and(|comment| !comment.trim().is_empty())
        {
            continue;
        }

        result.add_issue(
            LintIssue::new(
                LintRuleId::MissingColumnComment,
                LintRuleId::MissingColumnComment.default_severity(),
                format!(
                    "Column '{}' on table '{}' is missing a comment",
                    column.name,
                    table.qualified_name()
                ),
            )
            .with_table_id(&table.stable_id)
            .with_table(table.qualified_name())
            .with_column(&column.name)
            .with_hint("Document non-obvious semantics with a column comment in the source schema"),
        );
    }
}

/// Check: Table has no primary key.
fn check_no_primary_key(table: &Table, result: &mut LintResult) {
    let has_pk = table.columns.iter().any(|c| c.is_primary_key);

    if !has_pk {
        result.add_issue(
            LintIssue::new(
                LintRuleId::NoPrimaryKey,
                LintRuleId::NoPrimaryKey.default_severity(),
                format!(
                    "Table '{}' has no primary key defined",
                    table.qualified_name()
                ),
            )
            .with_table_id(&table.stable_id)
            .with_table(table.qualified_name())
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
    let has_incoming = incoming_fks
        .get(&table.id)
        .is_some_and(|fks| !fks.is_empty());
    let has_outgoing = outgoing_fks
        .get(&table.id)
        .is_some_and(|fks| !fks.is_empty());

    if !has_incoming && !has_outgoing {
        result.add_issue(
            LintIssue::new(
                LintRuleId::OrphanTable,
                LintRuleId::OrphanTable.default_severity(),
                format!(
                    "Table '{}' has no foreign key relationships",
                    table.qualified_name()
                ),
            )
            .with_table_id(&table.stable_id)
            .with_table(table.qualified_name())
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

    if nullable_count * 2 > total_columns {
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
                    "Table '{}' has {}/{} ({}%) nullable columns",
                    table.qualified_name(),
                    nullable_count,
                    total_columns,
                    (nullable_count * 100) / total_columns
                ),
            )
            .with_table_id(&table.stable_id)
            .with_table(table.qualified_name())
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
                    table.qualified_name()
                ),
            )
            .with_table_id(&table.stable_id)
            .with_table(table.qualified_name())
            .with_hint("Join tables should have foreign keys to the tables they connect"),
        );
    } else if fk_count == 1 {
        result.add_issue(
            LintIssue::new(
                LintRuleId::SuspiciousJoinTable,
                LintRuleId::SuspiciousJoinTable.default_severity(),
                format!(
                    "Table '{}' appears to be a join table but only has 1 foreign key",
                    table.qualified_name()
                ),
            )
            .with_table_id(&table.stable_id)
            .with_table(table.qualified_name())
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
                        table.qualified_name(),
                        fk_count,
                        target_tables.len()
                    ),
                )
                .with_table_id(&table.stable_id)
                .with_table(table.qualified_name())
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
                        table.qualified_name(),
                        fks.len(),
                        target_table
                    ),
                )
                .with_table_id(&table.stable_id)
                .with_table(table.qualified_name())
                .with_hint(format!(
                    "FKs: {}. This may indicate a design pattern or potential consolidation",
                    fk_names.join("; ")
                )),
            );
        }
    }
}

/// `true` if `s` is non-empty ASCII `snake_case`: optionally starts with one or more `_`,
/// then only `a-z`, `0-9`, `_`.  The first non-underscore character (if any) must be `a-z`.
/// Leading underscores are allowed because identifiers like `_id` or `__metadata` are common
/// and valid in SQL.
fn is_snake_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars().peekable();
    // Skip any leading underscores.
    while chars.peek() == Some(&'_') {
        chars.next();
    }
    // If the string is all underscores, allow it.
    // Check that the first non-underscore character is a lowercase ASCII letter.
    if let Some(&c) = chars.peek()
        && !c.is_ascii_lowercase()
    {
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
        return true;
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

fn resolve_referenced_table<'a>(
    schema: &'a Schema,
    table: &Table,
    fk: &ForeignKey,
) -> Option<&'a Table> {
    match resolve_table_reference(schema, Some(table), fk.to_schema.as_deref(), &fk.to_table) {
        ForeignKeyTargetResolution::Found(ref_table) => Some(ref_table),
        ForeignKeyTargetResolution::Missing | ForeignKeyTargetResolution::Ambiguous => None,
    }
}

/// Check: foreign keys should not form cycles across multiple tables.
fn check_circular_foreign_keys(schema: &Schema, result: &mut LintResult) {
    let graph = fk_dependency_graph(schema);
    let table_by_id: HashMap<TableId, &Table> = schema
        .tables
        .iter()
        .map(|table| (table.id, table))
        .collect();

    for component in kosaraju_scc(&graph) {
        if component.len() <= 1 {
            continue;
        }

        let mut participants: Vec<&Table> = component
            .iter()
            .filter_map(|table_id| table_by_id.get(table_id).copied())
            .collect();
        participants.sort_by_key(|table| table.qualified_name());
        let participant_names: Vec<String> = participants
            .iter()
            .map(|table| table.qualified_name())
            .collect();

        for table in participants {
            result.add_issue(
                LintIssue::new(
                    LintRuleId::CircularForeignKey,
                    LintRuleId::CircularForeignKey.default_severity(),
                    format!(
                        "Table '{}' participates in a foreign key cycle",
                        table.qualified_name()
                    ),
                )
                .with_table_id(&table.stable_id)
                .with_table(table.qualified_name())
                .with_hint(format!(
                    "Cycle members: {}. Review whether one edge should be optional, deferred, or removed",
                    participant_names.join(", ")
                )),
            );
        }
    }
}

/// `true` when `to_columns` matches the referenced table PK column set or some unique index column set,
/// regardless of column order.
fn referenced_columns_are_unique(ref_table: &Table, to_columns: &[String]) -> bool {
    if to_columns.is_empty() {
        return false;
    }
    let mut sorted_to: Vec<&str> = to_columns.iter().map(String::as_str).collect();
    sorted_to.sort_unstable();

    let mut pk_cols: Vec<&str> = ref_table
        .columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.as_str())
        .collect();
    pk_cols.sort_unstable();
    if !pk_cols.is_empty() && pk_cols == sorted_to {
        return true;
    }
    ref_table.indexes.iter().any(|idx| {
        if !idx.is_unique {
            return false;
        }
        let mut sorted_idx: Vec<&str> = idx.columns.iter().map(String::as_str).collect();
        sorted_idx.sort_unstable();
        sorted_idx == sorted_to
    })
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
    if fk_columns_are_indexed(table, &fk.from_columns) {
        return;
    }

    result.add_issue(
        LintIssue::new(
            LintRuleId::MissingForeignKeyIndex,
            LintRuleId::MissingForeignKeyIndex.default_severity(),
            format!(
                "Foreign key on table '{}' ({}) has no index whose leading columns match {:?}",
                table.qualified_name(),
                fk_labels_for_message(fk),
                fk.from_columns
            ),
        )
        .with_table_id(&table.stable_id)
        .with_table(table.qualified_name())
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
                table.qualified_name(),
                fk_labels_for_message(fk)
            ),
        )
        .with_table_id(&table.stable_id)
        .with_table(table.qualified_name())
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

    let Some(ref_table) = resolve_referenced_table(schema, table, fk) else {
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
                table.qualified_name(),
                fk_labels_for_message(fk),
                ref_table.qualified_name()
            ),
        )
        .with_table_id(&table.stable_id)
        .with_table(table.qualified_name())
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
                    table.qualified_name()
                ),
            )
            .with_table_id(&table.stable_id)
            .with_table(table.qualified_name())
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
                        col.name, table.qualified_name()
                    ),
                )
                .with_table_id(&table.stable_id)
                .with_table(table.qualified_name())
                .with_column(&col.name),
            );
        }
    }
}

/// Check: FK target table is resolvable (not missing or ambiguous).
fn check_unresolved_foreign_keys(schema: &Schema, table: &Table, result: &mut LintResult) {
    for fk in &table.foreign_keys {
        let message = match resolve_table_reference(
            schema,
            Some(table),
            fk.to_schema.as_deref(),
            &fk.to_table,
        ) {
            ForeignKeyTargetResolution::Found(_) => continue,
            ForeignKeyTargetResolution::Missing => {
                format!(
                    "FK on table '{}' references unknown table '{}'",
                    table.qualified_name(),
                    fk.to_table,
                )
            }
            ForeignKeyTargetResolution::Ambiguous => {
                format!(
                    "FK on table '{}' references ambiguous table '{}'; specify a schema name",
                    table.qualified_name(),
                    fk.to_table,
                )
            }
        };
        result.add_issue(
            LintIssue::new(
                LintRuleId::UnresolvedForeignKey,
                LintRuleId::UnresolvedForeignKey.default_severity(),
                message,
            )
            .with_table_id(&table.stable_id)
            .with_table(table.qualified_name()),
        );
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
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn test_table_id(schema_name: Option<&str>, name: &str) -> TableId {
        let mut hasher = DefaultHasher::new();
        schema_name.hash(&mut hasher);
        name.hash(&mut hasher);
        TableId(hasher.finish())
    }

    fn create_test_table(
        name: &str,
        columns: Vec<Column>,
        foreign_keys: Vec<ForeignKey>,
        indexes: Vec<crate::model::Index>,
    ) -> Table {
        Table {
            id: test_table_id(None, name),
            stable_id: name.to_string(),
            schema_name: None,
            name: name.to_string(),
            columns,
            foreign_keys,
            indexes,
            comment: None,
        }
    }

    fn create_test_table_with_schema(
        schema_name: Option<&str>,
        name: &str,
        columns: Vec<Column>,
        foreign_keys: Vec<ForeignKey>,
        indexes: Vec<crate::model::Index>,
    ) -> Table {
        Table {
            id: test_table_id(schema_name, name),
            stable_id: schema_name
                .map_or_else(|| name.to_string(), |schema| format!("{schema}.{name}")),
            schema_name: schema_name.map(str::to_string),
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
        assert_eq!(
            LintRuleId::MissingTableComment.as_str(),
            "missing-table-comment"
        );
        assert_eq!(
            LintRuleId::MissingColumnComment.as_str(),
            "missing-column-comment"
        );
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
        assert_eq!(
            LintRuleId::UnresolvedForeignKey.as_str(),
            "unresolved-foreign-key"
        );
        assert_eq!(
            LintRuleId::CircularForeignKey.as_str(),
            "circular-foreign-key"
        );
    }

    #[test]
    fn test_lint_rule_id_default_severity() {
        assert_eq!(
            LintRuleId::NoPrimaryKey.default_severity(),
            Severity::Warning
        );
        assert_eq!(
            LintRuleId::MissingTableComment.default_severity(),
            Severity::Warning
        );
        assert_eq!(
            LintRuleId::MissingColumnComment.default_severity(),
            Severity::Hint
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
        assert_eq!(
            LintRuleId::CircularForeignKey.default_severity(),
            Severity::Warning
        );
    }

    #[test]
    fn test_lint_profile_default_excludes_missing_column_comment() {
        assert!(
            !LintProfile::Default
                .default_rules()
                .contains(&LintRuleId::MissingColumnComment)
        );
        assert!(
            LintProfile::Strict
                .default_rules()
                .contains(&LintRuleId::MissingColumnComment)
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
    fn test_missing_table_comment_detection() {
        let table = create_test_table(
            "users",
            vec![create_column("id", false, true)],
            vec![],
            vec![],
        );

        let mut result = LintResult::new();
        check_missing_table_comment(&table, &mut result);

        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].rule_id, LintRuleId::MissingTableComment);
    }

    #[test]
    fn test_missing_column_comment_detection() {
        let table = create_test_table(
            "users",
            vec![create_column("id", false, true)],
            vec![],
            vec![],
        );

        let mut result = LintResult::new();
        check_missing_column_comments(&table, &mut result);

        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].rule_id, LintRuleId::MissingColumnComment);
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
    fn test_no_primary_key_with_unique_index_only() {
        // A single-column unique index on a non-PK column must NOT suppress the lint.
        let table = create_test_table(
            "users",
            vec![
                create_column("email", false, false),
                create_column("name", false, false),
            ],
            vec![],
            vec![Index {
                name: Some("users_email_unique".to_string()),
                columns: vec!["email".to_string()],
                is_unique: true,
            }],
        );

        let mut result = LintResult::new();
        check_no_primary_key(&table, &mut result);

        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].rule_id, LintRuleId::NoPrimaryKey);
        assert_eq!(result.stats.warnings, 1);
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
    fn test_too_many_nullable_exact_half_is_ok() {
        let table = create_test_table(
            "profiles",
            vec![
                create_column("id", false, true),
                create_column("bio", true, false),
                create_column("avatar", true, false),
                create_column("website", false, false),
            ],
            vec![],
            vec![],
        );

        let mut result = LintResult::new();
        check_too_many_nullable(&table, &mut result);

        assert!(result.issues.is_empty());
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
            .with_table_id("public.users")
            .with_table("public.users")
            .with_column("id")
            .with_hint("Add a primary key");

        assert_eq!(issue.table_id, Some("public.users".to_string()));
        assert_eq!(issue.table_name, Some("public.users".to_string()));
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
    fn test_fk_columns_are_indexed_treats_empty_fk_columns_as_covered() {
        let table = create_test_table(
            "posts",
            vec![create_column("id", false, true)],
            vec![],
            vec![],
        );

        assert!(fk_columns_are_indexed(&table, &[]));
    }

    #[test]
    fn test_orphan_table_respects_schema_qualified_targets() {
        let public_users = create_test_table_with_schema(
            Some("public"),
            "users",
            vec![create_column("id", false, true)],
            vec![],
            vec![],
        );
        let auth_users = create_test_table_with_schema(
            Some("auth"),
            "users",
            vec![
                create_column("id", false, true),
                create_column("email", false, false),
            ],
            vec![],
            vec![Index {
                name: Some("auth_users_email_idx".to_string()),
                columns: vec!["email".to_string()],
                is_unique: true,
            }],
        );
        let posts = create_test_table_with_schema(
            None,
            "posts",
            vec![
                create_column("id", false, true),
                create_column("user_id", false, false),
            ],
            vec![ForeignKey {
                name: None,
                from_columns: vec!["user_id".to_string()],
                to_schema: Some("auth".to_string()),
                to_table: "users".to_string(),
                to_columns: vec!["id".to_string()],
                on_delete: ReferentialAction::NoAction,
                on_update: ReferentialAction::NoAction,
            }],
            vec![],
        );

        let result = lint_schema(&Schema {
            tables: vec![public_users, auth_users, posts],
            ..Schema::default()
        });

        assert!(
            result
                .issues
                .iter()
                .any(|i| i.rule_id == LintRuleId::OrphanTable
                    && i.table_id == Some("public.users".to_string()))
        );
        assert!(
            !result
                .issues
                .iter()
                .any(|i| i.rule_id == LintRuleId::OrphanTable
                    && i.table_id == Some("auth.users".to_string()))
        );
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
    fn test_circular_foreign_key_detection() {
        let cycle_a = create_test_table(
            "cycle_a",
            vec![
                create_column("id", false, true),
                create_column("cycle_b_id", false, false),
            ],
            vec![create_fk("cycle_b", &["cycle_b_id"])],
            vec![Index {
                name: Some("idx_cycle_a_ref".to_string()),
                columns: vec!["cycle_b_id".to_string()],
                is_unique: false,
            }],
        );
        let cycle_b = create_test_table(
            "cycle_b",
            vec![
                create_column("id", false, true),
                create_column("cycle_a_id", false, false),
            ],
            vec![create_fk("cycle_a", &["cycle_a_id"])],
            vec![Index {
                name: Some("idx_cycle_b_ref".to_string()),
                columns: vec!["cycle_a_id".to_string()],
                is_unique: false,
            }],
        );
        let result = lint_schema(&Schema {
            tables: vec![cycle_a, cycle_b],
            ..Schema::default()
        });

        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.rule_id == LintRuleId::CircularForeignKey)
        );
    }

    #[test]
    fn test_self_referential_foreign_key_is_not_treated_as_circular_table_cycle() {
        let employees = create_test_table(
            "employees",
            vec![
                create_column("id", false, true),
                create_column("manager_id", true, false),
            ],
            vec![create_fk("employees", &["manager_id"])],
            vec![Index {
                name: Some("employees_manager_id_idx".to_string()),
                columns: vec!["manager_id".to_string()],
                is_unique: false,
            }],
        );
        let result = lint_schema(&Schema {
            tables: vec![employees],
            ..Schema::default()
        });

        assert!(
            !result
                .issues
                .iter()
                .any(|issue| issue.rule_id == LintRuleId::CircularForeignKey)
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

    #[test]
    fn test_foreign_key_non_unique_target_order_independent() {
        // PK columns in different order than FK to_columns should still match.
        let users = create_test_table(
            "users",
            vec![
                create_column("tenant_id", false, true),
                create_column("id", false, true),
            ],
            vec![],
            vec![],
        );
        let posts = create_test_table(
            "posts",
            vec![
                create_column("id", false, true),
                create_column("user_id", false, false),
                create_column("tenant_id", false, false),
            ],
            vec![ForeignKey {
                name: None,
                from_columns: vec!["tenant_id".to_string(), "user_id".to_string()],
                to_schema: None,
                to_table: "users".to_string(),
                // Reversed order relative to PK declaration
                to_columns: vec!["id".to_string(), "tenant_id".to_string()],
                on_delete: ReferentialAction::NoAction,
                on_update: ReferentialAction::NoAction,
            }],
            vec![Index {
                name: Some("idx_posts_fk".to_string()),
                columns: vec!["tenant_id".to_string(), "user_id".to_string()],
                is_unique: false,
            }],
        );
        let schema = Schema {
            tables: vec![users, posts],
            ..Schema::default()
        };
        let result = lint_schema(&schema);
        assert!(
            !result
                .issues
                .iter()
                .any(|i| i.rule_id == LintRuleId::ForeignKeyNonUniqueTarget),
            "should not report ForeignKeyNonUniqueTarget when to_columns match PK in different order"
        );
    }

    #[test]
    fn test_foreign_key_non_unique_target_respects_schema() {
        let public_users = create_test_table_with_schema(
            Some("public"),
            "users",
            vec![
                create_column("id", false, true),
                create_column("email", false, false),
            ],
            vec![],
            vec![],
        );
        let auth_users = create_test_table_with_schema(
            Some("auth"),
            "users",
            vec![
                create_column("id", false, true),
                create_column("email", false, false),
            ],
            vec![],
            vec![Index {
                name: Some("auth_users_email_unique".to_string()),
                columns: vec!["email".to_string()],
                is_unique: true,
            }],
        );
        let posts = create_test_table_with_schema(
            None,
            "posts",
            vec![
                create_column("id", false, true),
                create_column("user_email", false, false),
            ],
            vec![ForeignKey {
                name: None,
                from_columns: vec!["user_email".to_string()],
                to_schema: Some("auth".to_string()),
                to_table: "users".to_string(),
                to_columns: vec!["email".to_string()],
                on_delete: ReferentialAction::NoAction,
                on_update: ReferentialAction::NoAction,
            }],
            vec![Index {
                name: Some("posts_user_email_idx".to_string()),
                columns: vec!["user_email".to_string()],
                is_unique: false,
            }],
        );
        let schema = Schema {
            tables: vec![public_users, auth_users, posts],
            ..Schema::default()
        };
        let result = lint_schema(&schema);

        assert!(
            !result
                .issues
                .iter()
                .any(|i| i.rule_id == LintRuleId::ForeignKeyNonUniqueTarget)
        );
    }

    #[test]
    fn test_multi_schema_table_id_uniqueness() {
        // Two tables with the same name in different schemas must produce
        // distinct table_id values so overlay mapping is unambiguous.
        let public_users = create_test_table_with_schema(
            Some("public"),
            "users",
            vec![
                create_column("name", false, false),
                create_column("email", false, false),
            ],
            vec![],
            vec![],
        );
        let auth_users = create_test_table_with_schema(
            Some("auth"),
            "users",
            vec![
                create_column("name", false, false),
                create_column("email", false, false),
            ],
            vec![],
            vec![],
        );

        let result = lint_schema(&Schema {
            tables: vec![public_users, auth_users],
            ..Schema::default()
        });

        // Both tables should have NoPrimaryKey issues with distinct table_ids
        let no_pk_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.rule_id == LintRuleId::NoPrimaryKey)
            .collect();
        assert_eq!(no_pk_issues.len(), 2);

        let table_ids: HashSet<_> = no_pk_issues
            .iter()
            .filter_map(|i| i.table_id.as_deref())
            .collect();
        assert_eq!(table_ids.len(), 2, "table_id must be unique across schemas");
        assert!(table_ids.contains("public.users"));
        assert!(table_ids.contains("auth.users"));

        // table_name should also be schema-qualified for display
        assert!(
            no_pk_issues
                .iter()
                .all(|i| i.table_name.as_deref().is_some_and(|n| n.contains('.')))
        );
    }

    #[test]
    fn test_table_id_set_on_all_issues() {
        // Every issue produced by lint_schema should have a table_id set.
        let table = create_test_table(
            "UserAccounts",
            vec![
                create_column("Name", true, false),
                create_column("Email", true, false),
                create_column("Bio", true, false),
            ],
            vec![],
            vec![],
        );
        let result = lint_schema(&Schema {
            tables: vec![table],
            ..Schema::default()
        });

        assert!(!result.issues.is_empty());
        for issue in &result.issues {
            assert!(
                issue.table_id.is_some(),
                "issue {:?} should have table_id set",
                issue.rule_id
            );
        }
    }

    #[test]
    fn test_foreign_key_non_unique_target_prefers_same_schema_when_unqualified() {
        let public_users = create_test_table_with_schema(
            Some("public"),
            "users",
            vec![
                create_column("id", false, true),
                create_column("email", false, false),
            ],
            vec![],
            vec![],
        );
        let auth_users = create_test_table_with_schema(
            Some("auth"),
            "users",
            vec![
                create_column("id", false, true),
                create_column("email", false, false),
            ],
            vec![],
            vec![Index {
                name: Some("auth_users_email_unique".to_string()),
                columns: vec!["email".to_string()],
                is_unique: true,
            }],
        );
        let posts = create_test_table_with_schema(
            Some("auth"),
            "posts",
            vec![
                create_column("id", false, true),
                create_column("user_email", false, false),
            ],
            vec![ForeignKey {
                name: None,
                from_columns: vec!["user_email".to_string()],
                to_schema: None,
                to_table: "users".to_string(),
                to_columns: vec!["email".to_string()],
                on_delete: ReferentialAction::NoAction,
                on_update: ReferentialAction::NoAction,
            }],
            vec![Index {
                name: Some("posts_user_email_idx".to_string()),
                columns: vec!["user_email".to_string()],
                is_unique: false,
            }],
        );
        let schema = Schema {
            tables: vec![public_users, auth_users, posts],
            ..Schema::default()
        };
        let result = lint_schema(&schema);

        assert!(
            !result
                .issues
                .iter()
                .any(|i| i.rule_id == LintRuleId::ForeignKeyNonUniqueTarget)
        );
    }

    #[test]
    fn test_is_snake_case_edge_cases() {
        // Standard valid snake_case identifiers.
        assert!(is_snake_case("id"));
        assert!(is_snake_case("user_name"));
        assert!(is_snake_case("col1"));
        assert!(is_snake_case("a"));

        // Leading underscores — valid in SQL, must be accepted.
        assert!(is_snake_case("_id"));
        assert!(is_snake_case("__metadata"));
        assert!(is_snake_case("_"));

        // Leading underscore followed by uppercase — invalid.
        assert!(!is_snake_case("_Id"));
        assert!(!is_snake_case("__Bad"));

        // Starts with uppercase — invalid.
        assert!(!is_snake_case("Id"));
        assert!(!is_snake_case("UserName"));

        // Starts with a digit — invalid.
        assert!(!is_snake_case("0_col"));
        assert!(!is_snake_case("1name"));

        // Empty string — invalid.
        assert!(!is_snake_case(""));

        // Consecutive and trailing underscores — still valid.
        assert!(is_snake_case("user__old"));
        assert!(is_snake_case("id_"));
    }

    #[test]
    fn test_unresolved_foreign_key_missing() {
        // FK points to a table that does not exist in the schema.
        let posts = create_test_table(
            "posts",
            vec![
                create_column("id", false, true),
                create_column("user_id", false, false),
            ],
            vec![create_fk("nonexistent_table", &["user_id"])],
            vec![Index {
                name: Some("posts_user_id_idx".to_string()),
                columns: vec!["user_id".to_string()],
                is_unique: false,
            }],
        );
        let schema = Schema {
            tables: vec![posts],
            ..Schema::default()
        };
        let mut result = LintResult::new();
        check_unresolved_foreign_keys(&schema, &schema.tables[0], &mut result);

        assert_eq!(result.issues.len(), 1);
        let issue = &result.issues[0];
        assert_eq!(issue.rule_id, LintRuleId::UnresolvedForeignKey);
        assert_eq!(issue.severity, Severity::Warning);
        assert!(
            issue.message.contains("unknown table"),
            "message should mention 'unknown table': {}",
            issue.message
        );
        assert!(
            issue.message.contains("nonexistent_table"),
            "message should contain the target table name: {}",
            issue.message
        );
        assert_eq!(issue.table_id, Some("posts".to_string()));
    }

    #[test]
    fn test_unresolved_foreign_key_ambiguous() {
        // FK points to a table name that exists in two schemas without a schema qualifier.
        let public_users = create_test_table_with_schema(
            Some("public"),
            "users",
            vec![create_column("id", false, true)],
            vec![],
            vec![],
        );
        let auth_users = create_test_table_with_schema(
            Some("auth"),
            "users",
            vec![create_column("id", false, true)],
            vec![],
            vec![],
        );
        // posts has no schema, so "users" is ambiguous between public.users and auth.users.
        let posts = create_test_table(
            "posts",
            vec![
                create_column("id", false, true),
                create_column("user_id", false, false),
            ],
            vec![ForeignKey {
                name: None,
                from_columns: vec!["user_id".to_string()],
                to_schema: None,
                to_table: "users".to_string(),
                to_columns: vec!["id".to_string()],
                on_delete: ReferentialAction::NoAction,
                on_update: ReferentialAction::NoAction,
            }],
            vec![Index {
                name: Some("posts_user_id_idx".to_string()),
                columns: vec!["user_id".to_string()],
                is_unique: false,
            }],
        );
        let schema = Schema {
            tables: vec![public_users, auth_users, posts],
            ..Schema::default()
        };
        let posts_table = &schema.tables[2];
        let mut result = LintResult::new();
        check_unresolved_foreign_keys(&schema, posts_table, &mut result);

        assert_eq!(result.issues.len(), 1);
        let issue = &result.issues[0];
        assert_eq!(issue.rule_id, LintRuleId::UnresolvedForeignKey);
        assert_eq!(issue.severity, Severity::Warning);
        assert!(
            issue.message.contains("ambiguous table"),
            "message should mention 'ambiguous table': {}",
            issue.message
        );
        assert!(
            issue.message.contains("specify a schema name"),
            "message should suggest specifying a schema name: {}",
            issue.message
        );
        assert!(
            issue.message.contains("users"),
            "message should contain the target table name: {}",
            issue.message
        );
        assert_eq!(issue.table_id, Some("posts".to_string()));
    }

    #[test]
    fn test_unresolved_foreign_key_fires_in_lint_schema() {
        // Verify that lint_schema surfaces UnresolvedForeignKey via the per-table loop.
        let posts = create_test_table(
            "posts",
            vec![
                create_column("id", false, true),
                create_column("user_id", false, false),
            ],
            vec![create_fk("ghost_table", &["user_id"])],
            vec![Index {
                name: Some("posts_user_id_idx".to_string()),
                columns: vec!["user_id".to_string()],
                is_unique: false,
            }],
        );
        let result = lint_schema(&Schema {
            tables: vec![posts],
            ..Schema::default()
        });

        assert!(
            result
                .issues
                .iter()
                .any(|i| i.rule_id == LintRuleId::UnresolvedForeignKey),
            "lint_schema should report UnresolvedForeignKey for a FK pointing to a missing table"
        );
    }

    #[test]
    fn test_unresolved_foreign_key_not_fired_for_resolved_fk() {
        // When the FK resolves correctly, no UnresolvedForeignKey issue is emitted.
        let users = create_test_table(
            "users",
            vec![create_column("id", false, true)],
            vec![],
            vec![],
        );
        let posts = create_test_table(
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
        let schema = Schema {
            tables: vec![users, posts],
            ..Schema::default()
        };
        let mut result = LintResult::new();
        check_unresolved_foreign_keys(&schema, &schema.tables[1], &mut result);

        assert!(
            result.issues.is_empty(),
            "no UnresolvedForeignKey issue expected when FK resolves correctly"
        );
    }
}
