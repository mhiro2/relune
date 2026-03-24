//! Lint use case implementation.

use std::fmt::Write;

use crate::error::AppError;
use crate::request::LintRequest;
use crate::result::LintResult;
use crate::schema_input::schema_from_input;
use relune_core::{LintIssue, LintRuleId, Severity, lint_schema};

/// Execute a lint request.
#[allow(clippy::needless_pass_by_value)]
pub fn lint(request: LintRequest) -> Result<LintResult, AppError> {
    // Parse input
    let (schema, diagnostics) = schema_from_input(&request.input)?;

    // Run lint
    let mut lint_result = lint_schema(&schema);

    // Filter by rules if specified
    if !request.rules.is_empty() {
        let rules: Vec<LintRuleId> = request
            .rules
            .iter()
            .filter_map(|r| parse_rule_id(r))
            .collect();
        lint_result
            .issues
            .retain(|issue| rules.contains(&issue.rule_id));
        // Recalculate stats
        lint_result.stats = calculate_stats(&lint_result.issues);
    }

    Ok(LintResult {
        issues: lint_result.issues,
        stats: lint_result.stats,
        diagnostics,
    })
}

/// Format lint result as text.
#[must_use]
pub fn format_lint_text(result: &LintResult) -> String {
    let mut output = String::new();

    if result.issues.is_empty() {
        output.push_str("No lint issues found.\n");
        return output;
    }

    let plural = if result.stats.total == 1 { "" } else { "s" };
    let _ = writeln!(
        output,
        "Lint Results ({} issue{plural} found):",
        result.stats.total
    );
    let _ = writeln!(output, "{}", "=".repeat(60));

    for issue in &result.issues {
        let _ = writeln!(
            output,
            "\n[{}] {}",
            format_severity(issue.severity),
            issue.rule_id.as_str()
        );
        let _ = writeln!(output, "  {}", issue.message);

        if let Some(ref table) = issue.table_name {
            let _ = writeln!(output, "  Table: {table}");
        }
        if let Some(ref column) = issue.column_name {
            let _ = writeln!(output, "  Column: {column}");
        }
        if let Some(ref hint) = issue.hint {
            let _ = writeln!(output, "  Hint: {hint}");
        }
    }

    let _ = writeln!(output, "\n{}", "=".repeat(60));
    let err_plural = if result.stats.errors == 1 { "" } else { "s" };
    let warn_plural = if result.stats.warnings == 1 { "" } else { "s" };
    let hint_plural = if result.stats.hints == 1 { "" } else { "s" };
    let _ = writeln!(
        output,
        "Summary: {} error{}, {} warning{}, {} info, {} hint{}",
        result.stats.errors,
        err_plural,
        result.stats.warnings,
        warn_plural,
        result.stats.infos,
        result.stats.hints,
        hint_plural,
    );

    output
}

/// Format lint result as JSON.
pub fn format_lint_json(result: &LintResult) -> Result<String, AppError> {
    Ok(serde_json::to_string_pretty(result)?)
}

/// Parse a rule ID string into a `LintRuleId`.
fn parse_rule_id(s: &str) -> Option<LintRuleId> {
    match s.to_lowercase().as_str() {
        "no-primary-key" | "no_primary_key" => Some(LintRuleId::NoPrimaryKey),
        "orphan-table" | "orphan_table" => Some(LintRuleId::OrphanTable),
        "too-many-nullable" | "too_many_nullable" => Some(LintRuleId::TooManyNullable),
        "suspicious-join-table" | "suspicious_join_table" => Some(LintRuleId::SuspiciousJoinTable),
        "duplicated-fk-pattern" | "duplicated_fk_pattern" => Some(LintRuleId::DuplicatedFkPattern),
        "non-snake-case-identifier" | "non_snake_case_identifier" => {
            Some(LintRuleId::NonSnakeCaseIdentifier)
        }
        "missing-foreign-key-index" | "missing_foreign_key_index" => {
            Some(LintRuleId::MissingForeignKeyIndex)
        }
        "nullable-foreign-key-lazy-load" | "nullable_foreign_key_lazy_load" => {
            Some(LintRuleId::NullableForeignKeyLazyLoad)
        }
        "foreign-key-non-unique-target" | "foreign_key_non_unique_target" => {
            Some(LintRuleId::ForeignKeyNonUniqueTarget)
        }
        _ => None,
    }
}

/// Calculate stats from issues.
fn calculate_stats(issues: &[LintIssue]) -> relune_core::LintStats {
    let mut stats = relune_core::LintStats::default();
    for issue in issues {
        stats.total += 1;
        match issue.severity {
            Severity::Error => stats.errors += 1,
            Severity::Warning => stats.warnings += 1,
            Severity::Info => stats.infos += 1,
            Severity::Hint => stats.hints += 1,
        }
    }
    stats
}

/// Format severity for text output.
const fn format_severity(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "ERROR",
        Severity::Warning => "WARN",
        Severity::Info => "INFO",
        Severity::Hint => "HINT",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lint_no_issues() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255) NOT NULL
            );
            CREATE TABLE posts (
                id INT PRIMARY KEY,
                user_id INT NOT NULL REFERENCES users(id)
            );
            CREATE INDEX posts_user_id_idx ON posts (user_id);
        ";

        let request = LintRequest::from_sql(sql);
        let result = lint(request).unwrap();

        // Should not have no-primary-key or orphan-table issues
        assert!(
            !result
                .issues
                .iter()
                .any(|i| i.rule_id == LintRuleId::NoPrimaryKey)
        );
    }

    #[test]
    fn test_lint_no_primary_key() {
        let sql = r"
            CREATE TABLE users (
                name VARCHAR(255)
            );
        ";

        let request = LintRequest::from_sql(sql);
        let result = lint(request).unwrap();

        assert!(
            result
                .issues
                .iter()
                .any(|i| i.rule_id == LintRuleId::NoPrimaryKey)
        );
        assert!(result.stats.warnings >= 1);
    }

    #[test]
    fn test_lint_filter_by_rules() {
        let sql = r"
            CREATE TABLE users (
                name VARCHAR(255)
            );
        ";

        let request = LintRequest::from_sql(sql).with_rules(vec!["orphan-table".to_string()]);
        let result = lint(request).unwrap();

        // Should only have orphan-table issues, not no-primary-key
        assert!(
            result
                .issues
                .iter()
                .all(|i| i.rule_id == LintRuleId::OrphanTable)
        );
    }

    #[test]
    fn test_format_lint_text_empty() {
        let result = LintResult {
            issues: vec![],
            stats: relune_core::LintStats::default(),
            diagnostics: vec![],
        };

        let text = format_lint_text(&result);
        assert!(text.contains("No lint issues found"));
    }

    #[test]
    fn test_format_lint_text_with_issues() {
        let mut core_result = relune_core::lint_schema(&relune_core::Schema::default());
        core_result.add_issue(
            relune_core::LintIssue::new(
                LintRuleId::NoPrimaryKey,
                Severity::Warning,
                "Table 'test' has no primary key",
            )
            .with_table("test")
            .with_hint("Add a primary key"),
        );

        let result = LintResult {
            issues: core_result.issues,
            stats: core_result.stats,
            diagnostics: vec![],
        };

        let text = format_lint_text(&result);
        assert!(text.contains("[WARN]"));
        assert!(text.contains("no-primary-key"));
        assert!(text.contains("test"));
    }

    #[test]
    fn test_lint_result_has_failures() {
        let mut core_result = relune_core::LintResult::new();
        core_result.add_issue(relune_core::LintIssue::new(
            LintRuleId::NoPrimaryKey,
            Severity::Warning,
            "test",
        ));

        let result = LintResult {
            issues: core_result.issues,
            stats: core_result.stats,
            diagnostics: vec![],
        };

        assert!(result.has_failures(Some(Severity::Warning)));
        assert!(!result.has_failures(Some(Severity::Error)));
        assert!(!result.has_failures(None));
    }

    #[test]
    fn test_parse_rule_id() {
        assert_eq!(
            parse_rule_id("no-primary-key"),
            Some(LintRuleId::NoPrimaryKey)
        );
        assert_eq!(
            parse_rule_id("no_primary_key"),
            Some(LintRuleId::NoPrimaryKey)
        );
        assert_eq!(
            parse_rule_id("NO-PRIMARY-KEY"),
            Some(LintRuleId::NoPrimaryKey)
        );
        assert_eq!(parse_rule_id("unknown"), None);
    }
}
