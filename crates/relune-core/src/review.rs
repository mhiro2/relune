//! Schema review engine for migration risk analysis.
//!
//! Lint inspects a single schema for design quality. Review compares
//! `before` and `after` schemas to surface migration-time risks: dropped
//! references, narrowing type changes, NOT NULL on existing data, etc.
//!
//! `ReviewSeverity` is intentionally a different type from
//! `crate::Severity`: review severity describes "migration safety", while
//! lint severity describes "schema quality".

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

mod rules;

pub use rules::run_rules;

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
        }
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
    fn summary_threshold_checks() {
        let mut summary = ReviewSummary::default();
        summary.record(ReviewSeverity::Warning);
        assert!(summary.has_findings_at_or_above(ReviewSeverity::Warning));
        assert!(!summary.has_findings_at_or_above(ReviewSeverity::Caution));
        assert!(!summary.has_findings_at_or_above(ReviewSeverity::Breaking));
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
