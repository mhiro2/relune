//! Review report embedded next to the diagram metadata.
//!
//! A review report turns the viewer into a migration review: the findings
//! are listed by severity, and choosing one selects its table, marks its
//! columns, and shows the table definition before and after the change.

use std::collections::BTreeMap;

use relune_core::{ReviewSummary, RiskFinding, SourceLocation};
use serde::{Deserialize, Serialize};

/// Review findings and the context needed to judge them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewReport {
    /// Counts of the reported findings by severity.
    pub summary: ReviewSummary,
    /// Findings, most severe first.
    pub findings: Vec<ReviewReportFinding>,
    /// Number of findings suppressed by table exceptions.
    pub suppressed: usize,
    /// How the review resolved its dialect, when worth stating.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialect_note: Option<String>,
    /// What the review could not evaluate, one note per gap.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coverage_notes: Vec<String>,
    /// Definitions of every table a finding names, keyed by stable id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub definitions: BTreeMap<String, DefinitionDiff>,
    /// Which migration SQL the review read, when it was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_note: Option<String>,
    /// Migration statements the findings point at.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statements: Vec<ReportStatement>,
}

/// A migration statement a finding points at.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportStatement {
    /// Where the statement is written.
    #[serde(flatten)]
    pub location: SourceLocation,
    /// Statement text.
    pub sql: String,
    /// Whether it runs inside a transaction block.
    pub in_transaction: bool,
}

/// One finding with the reasoning the viewer shows next to it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewReportFinding {
    /// The finding as `relune review` reports it.
    #[serde(flatten)]
    pub finding: RiskFinding,
    /// What the rule checks, i.e. why the change was flagged.
    pub rule_description: String,
    /// Indexes of the definition lines the finding is about.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_lines: Vec<usize>,
}

/// A table definition before and after the change, as one line diff.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefinitionDiff {
    /// Lines in reading order.
    pub lines: Vec<DefinitionLine>,
}

/// One line of a definition diff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefinitionLine {
    /// Whether the line exists before, after, or on both sides.
    pub change: LineChange,
    /// Line text.
    pub text: String,
}

/// Side a definition line belongs to.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LineChange {
    /// Present before and after.
    Unchanged,
    /// Present only before the change.
    Removed,
    /// Present only after the change.
    Added,
}
