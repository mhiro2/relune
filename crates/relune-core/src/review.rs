//! Schema review engine for migration risk analysis.
//!
//! Lint inspects a single schema for design quality. Review compares
//! `before` and `after` schemas to surface migration-time risks: dropped
//! references, narrowing type changes, NOT NULL on existing data, etc.
//!
//! `ReviewSeverity` is intentionally a different type from
//! `crate::Severity`: review severity describes "migration safety", while
//! lint severity describes "schema quality".

use crate::SqlDialect;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

mod rules;

pub use rules::run_rules;

/// Dialect that the review pipeline actually evaluates against.
///
/// Independent from the parser dialect carried by `InputSource` (which
/// only governs SQL lexing): this is the **single source** for dialect
/// decisions inside rule evaluation, in particular the lock-risk rules
/// added in Phase 4 that only fire when the dialect resolves to
/// `Postgres` or `Mysql`.
///
/// The value is produced by callers (`relune-app`, CLI, wasm) from
/// `--dialect` / `WasmReviewRequest.dialect` / `[review.dialect]` and
/// passed into `run_rules`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum EffectiveDialect {
    /// Dialect is not pinned; lock-risk rules do not fire.
    #[default]
    Auto,
    /// `PostgreSQL` semantics for lock-risk evaluation.
    Postgres,
    /// `MySQL` semantics for lock-risk evaluation.
    Mysql,
    /// `SQLite`; lock-risk rules do not fire (sqlite has no online DDL
    /// to opt into, so a caution would carry no actionable signal).
    Sqlite,
}

impl EffectiveDialect {
    /// Returns true when the dialect supports any lock-risk rule.
    ///
    /// Currently `Postgres` and `Mysql` only; `Auto` and `Sqlite` always
    /// skip the lock-risk rule set.
    #[must_use]
    pub const fn is_lock_risk_capable(&self) -> bool {
        matches!(self, Self::Postgres | Self::Mysql)
    }
}

impl fmt::Display for EffectiveDialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Postgres => write!(f, "postgres"),
            Self::Mysql => write!(f, "mysql"),
            Self::Sqlite => write!(f, "sqlite"),
        }
    }
}

impl From<SqlDialect> for EffectiveDialect {
    fn from(value: SqlDialect) -> Self {
        match value {
            SqlDialect::Auto => Self::Auto,
            SqlDialect::Postgres => Self::Postgres,
            SqlDialect::Mysql => Self::Mysql,
            SqlDialect::Sqlite => Self::Sqlite,
        }
    }
}

impl From<EffectiveDialect> for SqlDialect {
    fn from(value: EffectiveDialect) -> Self {
        match value {
            EffectiveDialect::Auto => Self::Auto,
            EffectiveDialect::Postgres => Self::Postgres,
            EffectiveDialect::Mysql => Self::Mysql,
            EffectiveDialect::Sqlite => Self::Sqlite,
        }
    }
}

/// Severity scale for migration risk findings.
///
/// Ordered `Info < Warning < Caution < Breaking` so callers can use `>=`
/// against a configured `--deny` threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSeverity {
    /// Informational hygiene signal.
    Info,
    /// Semantic shift or constraint that may reject existing data.
    Warning,
    /// Operationally risky at scale (locking, availability). Wired up in
    /// Phase 4 but reserved here so the ordering is stable.
    Caution,
    /// Migration is expected to fail or destroy data.
    Breaking,
}

impl ReviewSeverity {
    /// Stable lowercase representation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Caution => "caution",
            Self::Breaking => "breaking",
        }
    }
}

impl fmt::Display for ReviewSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ReviewSeverity {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "info" => Ok(Self::Info),
            "warning" => Ok(Self::Warning),
            "caution" => Ok(Self::Caution),
            "breaking" => Ok(Self::Breaking),
            other => Err(format!(
                "unknown review severity: {other}. Expected: info, warning, caution, breaking"
            )),
        }
    }
}

/// Stable identifier for a review rule.
///
/// Uses a custom `Serialize` / `Deserialize` impl that emits the
/// `risk/<kebab>` form to keep the namespace separate from lint rules
/// even though `/` is not friendly to `serde(rename_all)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ReviewRuleId {
    /// Dropping a column referenced by an existing FK.
    DropColumnReferenced,
    /// Dropping a table referenced by an existing FK.
    DropTableReferenced,
    /// Adding NOT NULL to a column on an existing table.
    AddNotNullOnExisting,
    /// Narrowing a column type in a way that may truncate existing data.
    TypeNarrow,
    /// Dropping a primary key or unique constraint.
    DropPkOrUnique,
    /// Adding a UNIQUE constraint to a column on an existing table.
    AddUniqueOnExisting,
    /// Adding `ON DELETE CASCADE` to a foreign key.
    AddCascadeDelete,
    /// New foreign key without a supporting index.
    FkWithoutIndex,
    /// Index added on an existing table; non-CONCURRENT / non-INPLACE
    /// builds block writes for the duration of the rebuild.
    AddIndexOnLargeTable,
    /// Foreign key added between two existing tables; validation locks
    /// the referencing table while every existing row is checked.
    AddFkOnExisting,
    /// Existing column's data type was changed; many type changes
    /// rewrite the entire table under an exclusive lock.
    AlterColumnType,
    /// Schema change forces a full table rebuild on `MySQL` 5.7-compatible
    /// engines (PK rotation or existing column drop).
    RewriteTable,
}

impl ReviewRuleId {
    /// Returns every rule defined in this module.
    #[must_use]
    pub const fn all_rules() -> &'static [Self] {
        &[
            Self::DropColumnReferenced,
            Self::DropTableReferenced,
            Self::AddNotNullOnExisting,
            Self::TypeNarrow,
            Self::DropPkOrUnique,
            Self::AddUniqueOnExisting,
            Self::AddCascadeDelete,
            Self::FkWithoutIndex,
            // Phase 4 lock-risk rules. Append-only so the listing order
            // stays stable for callers (CLI listings, fixtures, docs).
            Self::AddIndexOnLargeTable,
            Self::AddFkOnExisting,
            Self::AlterColumnType,
            Self::RewriteTable,
        ]
    }

    /// Stable string identifier in `risk/<kebab>` form.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::DropColumnReferenced => "risk/drop-column-referenced",
            Self::DropTableReferenced => "risk/drop-table-referenced",
            Self::AddNotNullOnExisting => "risk/add-not-null-on-existing",
            Self::TypeNarrow => "risk/type-narrow",
            Self::DropPkOrUnique => "risk/drop-pk-or-unique",
            Self::AddUniqueOnExisting => "risk/add-unique-on-existing",
            Self::AddCascadeDelete => "risk/add-cascade-delete",
            Self::FkWithoutIndex => "risk/fk-without-index",
            Self::AddIndexOnLargeTable => "risk/add-index-on-large-table",
            Self::AddFkOnExisting => "risk/add-fk-on-existing",
            Self::AlterColumnType => "risk/alter-column-type",
            Self::RewriteTable => "risk/rewrite-table",
        }
    }

    /// Human-readable description used by CLI listings.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::DropColumnReferenced => {
                "Column being dropped is still referenced by a foreign key"
            }
            Self::DropTableReferenced => "Table being dropped is still referenced by a foreign key",
            Self::AddNotNullOnExisting => {
                "NOT NULL added to a column on an existing table; existing rows may fail"
            }
            Self::TypeNarrow => {
                "Column type is being narrowed in a way that may reject existing data"
            }
            Self::DropPkOrUnique => "Primary key or unique constraint is being dropped",
            Self::AddUniqueOnExisting => {
                "UNIQUE added to an existing table; existing duplicates will fail"
            }
            Self::AddCascadeDelete => "Foreign key now uses ON DELETE CASCADE",
            Self::FkWithoutIndex => "New foreign key has no supporting index",
            Self::AddIndexOnLargeTable => {
                "New index on existing table; non-CONCURRENT/INPLACE builds lock the rebuild"
            }
            Self::AddFkOnExisting => {
                "New foreign key validates every existing row under a blocking lock"
            }
            Self::AlterColumnType => {
                "Existing column's type change may rewrite the table under an exclusive lock"
            }
            Self::RewriteTable => {
                "Schema change forces a table rebuild on MySQL 5.7-compatible engines"
            }
        }
    }

    /// Representative severity for CLI listings. The actual finding
    /// severity is computed per-case by the rule (e.g.
    /// `risk/drop-pk-or-unique` is breaking with a referencing FK,
    /// warning otherwise).
    #[must_use]
    pub const fn default_severity(&self) -> ReviewSeverity {
        match self {
            Self::DropColumnReferenced | Self::DropTableReferenced | Self::TypeNarrow => {
                ReviewSeverity::Breaking
            }
            Self::AddNotNullOnExisting
            | Self::AddUniqueOnExisting
            | Self::AddCascadeDelete
            | Self::DropPkOrUnique => ReviewSeverity::Warning,
            Self::FkWithoutIndex => ReviewSeverity::Info,
            Self::AddIndexOnLargeTable
            | Self::AddFkOnExisting
            | Self::AlterColumnType
            | Self::RewriteTable => ReviewSeverity::Caution,
        }
    }

    /// Build the metadata snapshot for this rule.
    ///
    /// Mirrors `LintRuleId::metadata` and is the single source of truth
    /// surfaced by `relune review --list-rules`, the WASM bindings, and
    /// the playground rule legend.
    #[must_use]
    pub fn metadata(&self) -> ReviewRuleMetadata {
        ReviewRuleMetadata {
            rule_id: *self,
            default_severity: self.default_severity(),
            description: self.description().to_string(),
        }
    }

    /// Serializable metadata snapshot for every review rule.
    #[must_use]
    pub fn all_metadata() -> Vec<ReviewRuleMetadata> {
        Self::all_rules().iter().map(Self::metadata).collect()
    }

    /// Parse a stable rule identifier (`risk/<kebab>`).
    pub fn parse(value: &str) -> Result<Self, String> {
        for rule in Self::all_rules() {
            if rule.as_str().eq_ignore_ascii_case(value) {
                return Ok(*rule);
            }
        }
        Err(format!(
            "unknown review rule: {value}. Expected one of: {}",
            Self::all_rules()
                .iter()
                .map(Self::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

impl fmt::Display for ReviewRuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ReviewRuleId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for ReviewRuleId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReviewRuleId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// Serializable metadata for a review rule.
///
/// Mirrors `LintRuleMetadata` (lint side) and is the single source of
/// truth surfaced by `relune review --list-rules`, the WASM bindings,
/// and the playground rule legend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewRuleMetadata {
    /// Stable rule identifier in `risk/<kebab>` form.
    pub rule_id: ReviewRuleId,
    /// Representative severity for CLI listings; the per-finding
    /// severity may differ when a rule decides severity case-by-case.
    pub default_severity: ReviewSeverity,
    /// Human-readable description (1 line).
    pub description: String,
}

/// Per-rule severity override applied after rule evaluation.
///
/// Replaces the case-by-case severity produced by the rule with a fixed
/// value from configuration. Used to suppress noisy rules (downgrade) or
/// escalate rules that the project cares about (upgrade). See
/// `docs/configuration.md`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewSeverityOverride {
    /// Rule whose finding severity should be overridden (`risk/<kebab>` form).
    pub rule_id: ReviewRuleId,
    /// Severity value to use in place of the rule's default / case-by-case result.
    pub severity: ReviewSeverity,
}

/// A single review finding produced by a rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RiskFinding {
    /// Rule that produced this finding.
    pub rule_id: ReviewRuleId,
    /// Computed severity for this finding (not necessarily the rule default).
    pub severity: ReviewSeverity,
    /// Human-readable description of the risk.
    pub message: String,
    /// Optional mitigation hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mitigation: Option<String>,
    /// Stable table identifier (matches `Table::stable_id`) for the
    /// primary affected table, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_id: Option<String>,
    /// Schema-qualified human-readable table name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
    /// Affected column name, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_name: Option<String>,
    /// Affected foreign key name, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fk_name: Option<String>,
    /// Stable identifier of a related table (e.g. the referencing table
    /// for `risk/drop-column-referenced`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_table_id: Option<String>,
}

impl RiskFinding {
    /// Creates a new finding with the given rule, severity and message.
    #[must_use]
    pub fn new(
        rule_id: ReviewRuleId,
        severity: ReviewSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            rule_id,
            severity,
            message: message.into(),
            mitigation: None,
            table_id: None,
            table_name: None,
            column_name: None,
            fk_name: None,
            related_table_id: None,
        }
    }

    /// Sets the mitigation hint.
    #[must_use]
    pub fn with_mitigation(mut self, hint: impl Into<String>) -> Self {
        self.mitigation = Some(hint.into());
        self
    }

    /// Sets the primary table identifiers.
    #[must_use]
    pub fn with_table(
        mut self,
        table_id: impl Into<String>,
        table_name: impl Into<String>,
    ) -> Self {
        self.table_id = Some(table_id.into());
        self.table_name = Some(table_name.into());
        self
    }

    /// Sets the column name.
    #[must_use]
    pub fn with_column(mut self, column_name: impl Into<String>) -> Self {
        self.column_name = Some(column_name.into());
        self
    }

    /// Sets the foreign key name.
    #[must_use]
    pub fn with_fk_name(mut self, fk_name: impl Into<String>) -> Self {
        self.fk_name = Some(fk_name.into());
        self
    }

    /// Sets the related table identifier.
    #[must_use]
    pub fn with_related_table(mut self, related_table_id: impl Into<String>) -> Self {
        self.related_table_id = Some(related_table_id.into());
        self
    }
}

/// Aggregate counts grouped by severity.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewSummary {
    /// Number of `Breaking` findings.
    pub breaking: usize,
    /// Number of `Caution` findings.
    pub caution: usize,
    /// Number of `Warning` findings.
    pub warning: usize,
    /// Number of `Info` findings.
    pub info: usize,
}

impl ReviewSummary {
    /// Total finding count.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.breaking + self.caution + self.warning + self.info
    }

    /// Returns true when the summary contains any finding at or above
    /// the given threshold.
    #[must_use]
    pub const fn has_findings_at_or_above(&self, threshold: ReviewSeverity) -> bool {
        match threshold {
            ReviewSeverity::Info => self.total() > 0,
            ReviewSeverity::Warning => self.warning + self.caution + self.breaking > 0,
            ReviewSeverity::Caution => self.caution + self.breaking > 0,
            ReviewSeverity::Breaking => self.breaking > 0,
        }
    }

    const fn record(&mut self, severity: ReviewSeverity) {
        match severity {
            ReviewSeverity::Info => self.info += 1,
            ReviewSeverity::Warning => self.warning += 1,
            ReviewSeverity::Caution => self.caution += 1,
            ReviewSeverity::Breaking => self.breaking += 1,
        }
    }
}

/// Result of running the review rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewResult {
    /// Findings that survived suppression.
    pub findings: Vec<RiskFinding>,
    /// Suppressed findings (excluded by `--except-rule` / `--except-table`).
    pub suppressed: Vec<RiskFinding>,
    /// Summary counts of `findings`.
    pub summary: ReviewSummary,
    /// Rules that were actually evaluated for this run.
    pub applied_rules: Vec<ReviewRuleId>,
}

impl ReviewResult {
    /// Builds a `ReviewResult` from a list of findings; the summary is
    /// derived from `findings`.
    #[must_use]
    pub fn from_parts(
        findings: Vec<RiskFinding>,
        suppressed: Vec<RiskFinding>,
        applied_rules: Vec<ReviewRuleId>,
    ) -> Self {
        let mut summary = ReviewSummary::default();
        for finding in &findings {
            summary.record(finding.severity);
        }
        Self {
            findings,
            suppressed,
            summary,
            applied_rules,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_orders_ascend_to_breaking() {
        assert!(ReviewSeverity::Info < ReviewSeverity::Warning);
        assert!(ReviewSeverity::Warning < ReviewSeverity::Caution);
        assert!(ReviewSeverity::Caution < ReviewSeverity::Breaking);
    }

    #[test]
    fn severity_round_trips_via_string() {
        for severity in [
            ReviewSeverity::Info,
            ReviewSeverity::Warning,
            ReviewSeverity::Caution,
            ReviewSeverity::Breaking,
        ] {
            assert_eq!(
                severity.as_str().parse::<ReviewSeverity>().unwrap(),
                severity
            );
        }
    }

    #[test]
    fn rule_id_round_trips_via_string() {
        for rule in ReviewRuleId::all_rules() {
            let parsed = ReviewRuleId::parse(rule.as_str()).unwrap();
            assert_eq!(parsed, *rule);
        }
    }

    #[test]
    fn rule_id_serializes_with_risk_prefix() {
        let json = serde_json::to_string(&ReviewRuleId::DropColumnReferenced).unwrap();
        assert_eq!(json, "\"risk/drop-column-referenced\"");
        let back: ReviewRuleId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ReviewRuleId::DropColumnReferenced);
    }

    #[test]
    fn metadata_matches_rule_accessors() {
        for rule in ReviewRuleId::all_rules() {
            let metadata = rule.metadata();
            assert_eq!(metadata.rule_id, *rule);
            assert_eq!(metadata.default_severity, rule.default_severity());
            assert_eq!(metadata.description, rule.description());
        }
    }

    #[test]
    fn all_metadata_covers_every_rule() {
        let metadata = ReviewRuleId::all_metadata();
        assert_eq!(metadata.len(), ReviewRuleId::all_rules().len());
        for (entry, rule) in metadata.iter().zip(ReviewRuleId::all_rules().iter()) {
            assert_eq!(entry.rule_id, *rule);
        }
    }

    #[test]
    fn metadata_serializes_with_risk_prefix() {
        let metadata = ReviewRuleId::FkWithoutIndex.metadata();
        let json = serde_json::to_value(&metadata).unwrap();
        assert_eq!(json["rule_id"], "risk/fk-without-index");
        assert_eq!(json["default_severity"], "info");
        let round_trip: ReviewRuleMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(round_trip, metadata);
    }

    #[test]
    fn severity_override_round_trips_via_json() {
        let override_value = ReviewSeverityOverride {
            rule_id: ReviewRuleId::AddNotNullOnExisting,
            severity: ReviewSeverity::Info,
        };
        let json = serde_json::to_value(&override_value).unwrap();
        assert_eq!(json["rule_id"], "risk/add-not-null-on-existing");
        assert_eq!(json["severity"], "info");
        let round_trip: ReviewSeverityOverride = serde_json::from_value(json).unwrap();
        assert_eq!(round_trip, override_value);
    }

    #[test]
    fn summary_threshold_checks() {
        let mut summary = ReviewSummary::default();
        summary.record(ReviewSeverity::Warning);
        assert!(summary.has_findings_at_or_above(ReviewSeverity::Warning));
        assert!(!summary.has_findings_at_or_above(ReviewSeverity::Caution));
        assert!(!summary.has_findings_at_or_above(ReviewSeverity::Breaking));
    }

    #[test]
    fn effective_dialect_round_trips_with_sql_dialect() {
        for dialect in [
            SqlDialect::Auto,
            SqlDialect::Postgres,
            SqlDialect::Mysql,
            SqlDialect::Sqlite,
        ] {
            let effective: EffectiveDialect = dialect.into();
            let back: SqlDialect = effective.into();
            assert_eq!(back, dialect);
        }
    }

    #[test]
    fn effective_dialect_lock_risk_capable_is_pg_or_mysql() {
        assert!(EffectiveDialect::Postgres.is_lock_risk_capable());
        assert!(EffectiveDialect::Mysql.is_lock_risk_capable());
        assert!(!EffectiveDialect::Auto.is_lock_risk_capable());
        assert!(!EffectiveDialect::Sqlite.is_lock_risk_capable());
    }

    #[test]
    fn effective_dialect_default_is_auto() {
        assert_eq!(EffectiveDialect::default(), EffectiveDialect::Auto);
    }

    #[test]
    fn summary_breaking_dominates_threshold() {
        let mut summary = ReviewSummary::default();
        summary.record(ReviewSeverity::Breaking);
        assert!(summary.has_findings_at_or_above(ReviewSeverity::Breaking));
        assert!(summary.has_findings_at_or_above(ReviewSeverity::Caution));
        assert!(summary.has_findings_at_or_above(ReviewSeverity::Warning));
        assert!(summary.has_findings_at_or_above(ReviewSeverity::Info));
    }
}
