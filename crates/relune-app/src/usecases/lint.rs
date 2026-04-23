//! Lint use case implementation.

use std::collections::HashSet;
use std::fmt::Write;

use crate::error::AppError;
use crate::request::LintRequest;
use crate::result::{LintResult, LintReview};
use crate::schema_input::schema_from_input_with_context;
use relune_core::{
    LintIssue, LintRuleCategory, LintRuleId, LintRuleMetadata, Severity, lint_schema,
};

/// Execute a lint request.
#[allow(clippy::needless_pass_by_value)]
pub fn lint(request: LintRequest) -> Result<LintResult, AppError> {
    // Parse input
    let (schema, diagnostics, input_context) = schema_from_input_with_context(&request.input)?;

    // Run lint
    let lint_result = lint_schema(&schema);
    let active_rules = resolve_active_rules(&request, input_context.supports_comment_review)?;
    let mut issues = lint_result.issues;
    issues.retain(|issue| active_rules.contains(&issue.rule_id));
    let issue_count_before_exceptions = issues.len();
    issues.retain(|issue| !matches_except_table(&request.except_tables, issue));
    let suppressed_issue_count = issue_count_before_exceptions.saturating_sub(issues.len());
    let active_rules = rule_metadata_list(&active_rules);
    let stats = calculate_stats(&issues);

    Ok(LintResult {
        review: LintReview {
            profile: request.profile,
            active_rules,
            except_tables: request.except_tables,
            suppressed_issue_count,
        },
        issues,
        stats,
        diagnostics,
    })
}

/// Format lint result as text.
#[must_use]
pub fn format_lint_text(result: &LintResult) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "Schema Review ({}, {} active rules)",
        result.review.profile,
        result.review.active_rules.len()
    );
    let categories = active_categories(&result.review.active_rules);
    if !categories.is_empty() {
        let _ = writeln!(output, "Categories: {}", categories.join(", "));
    }
    if !result.review.except_tables.is_empty() {
        let _ = writeln!(
            output,
            "Exceptions: {}",
            result.review.except_tables.join(", ")
        );
    }
    if result.review.suppressed_issue_count > 0 {
        let _ = writeln!(
            output,
            "Suppressed By Exceptions: {} issue(s)",
            result.review.suppressed_issue_count
        );
    }
    output.push('\n');

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
            "\n[{}] {} / {}",
            format_severity(issue.severity),
            issue.category,
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
///
/// Accepts both kebab-case (`no-primary-key`) and `snake_case`
/// (`no_primary_key`) forms. The canonical set is derived from
/// `LintRuleId::all()` so that new variants are automatically
/// recognised without a manual match arm.
fn parse_rule_id(s: &str) -> Option<LintRuleId> {
    let normalized = s.to_lowercase().replace('_', "-");
    LintRuleId::all()
        .iter()
        .find(|rule| rule.as_str() == normalized)
        .copied()
}

/// Parse and validate requested rule IDs.
fn parse_rule_ids(rule_ids: &[String]) -> Result<HashSet<LintRuleId>, AppError> {
    let mut parsed = HashSet::with_capacity(rule_ids.len());
    let mut invalid = Vec::new();

    for rule_id in rule_ids {
        match parse_rule_id(rule_id) {
            Some(parsed_rule) => {
                parsed.insert(parsed_rule);
            }
            None => invalid.push(rule_id.clone()),
        }
    }

    if invalid.is_empty() {
        Ok(parsed)
    } else {
        Err(AppError::input(format!(
            "Unknown lint rule id(s): {}",
            invalid.join(", ")
        )))
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

fn resolve_active_rules(
    request: &LintRequest,
    supports_comment_review: bool,
) -> Result<HashSet<LintRuleId>, AppError> {
    validate_comment_rule_support(request, supports_comment_review)?;

    let mut active_rules = if request.rules.is_empty() {
        request.profile.default_rules().iter().copied().collect()
    } else {
        parse_rule_ids(&request.rules)?
    };

    if !supports_comment_review {
        active_rules.retain(|rule_id| !is_comment_rule(*rule_id));
    }

    if !request.categories.is_empty() {
        let categories: HashSet<LintRuleCategory> = request.categories.iter().copied().collect();
        active_rules.retain(|rule_id| categories.contains(&rule_id.category()));
    }

    if !request.exclude_rules.is_empty() {
        let excluded_rules = parse_rule_ids(&request.exclude_rules)?;
        active_rules.retain(|rule_id| !excluded_rules.contains(rule_id));
    }

    if active_rules.is_empty() {
        return Err(AppError::input(
            "No lint rules remain after applying the selected profile and filters".to_string(),
        ));
    }

    Ok(active_rules)
}

fn validate_comment_rule_support(
    request: &LintRequest,
    supports_comment_review: bool,
) -> Result<(), AppError> {
    if supports_comment_review {
        return Ok(());
    }

    if !request.rules.is_empty() {
        let explicit_rules = parse_rule_ids(&request.rules)?;
        let unsupported_rules: Vec<&str> = explicit_rules
            .into_iter()
            .filter(|rule_id| is_comment_rule(*rule_id))
            .map(|rule_id| rule_id.as_str())
            .collect();
        if !unsupported_rules.is_empty() {
            return Err(AppError::input(format!(
                "Documentation lint rules are unavailable for this input source: {}",
                unsupported_rules.join(", ")
            )));
        }
    }

    if request
        .categories
        .contains(&LintRuleCategory::Documentation)
    {
        return Err(AppError::input(
            "The documentation lint category is unavailable for this input source".to_string(),
        ));
    }

    Ok(())
}

const fn is_comment_rule(rule_id: LintRuleId) -> bool {
    matches!(
        rule_id,
        LintRuleId::MissingTableComment | LintRuleId::MissingColumnComment
    )
}

fn rule_metadata_list(active_rules: &HashSet<LintRuleId>) -> Vec<LintRuleMetadata> {
    let mut metadata: Vec<_> = active_rules.iter().map(LintRuleId::metadata).collect();
    metadata.sort_by(|left, right| {
        left.category
            .as_str()
            .cmp(right.category.as_str())
            .then_with(|| left.rule_id.as_str().cmp(right.rule_id.as_str()))
    });
    metadata
}

fn active_categories(active_rules: &[LintRuleMetadata]) -> Vec<String> {
    let mut categories = Vec::<String>::new();
    for rule in active_rules {
        let category = rule.category.as_str().to_string();
        if !categories.contains(&category) {
            categories.push(category);
        }
    }
    categories
}

fn matches_except_table(patterns: &[String], issue: &LintIssue) -> bool {
    let Some(table_name) = issue.table_name.as_deref() else {
        return false;
    };
    let short_name = table_name.rsplit('.').next().unwrap_or(table_name);
    patterns
        .iter()
        .any(|pattern| matches_pattern(pattern, table_name) || matches_pattern(pattern, short_name))
}

fn matches_pattern(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.starts_with('*') && pattern.ends_with('*') && pattern.len() > 2 {
        return value.contains(&pattern[1..pattern.len() - 1]);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return value.ends_with(suffix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    value == pattern
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
    use crate::error::AppError;

    fn test_review() -> LintReview {
        LintReview {
            profile: relune_core::LintProfile::Default,
            active_rules: vec![LintRuleId::NoPrimaryKey.metadata()],
            except_tables: vec![],
            suppressed_issue_count: 0,
        }
    }

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
            COMMENT ON TABLE users IS 'Application users';
            COMMENT ON TABLE posts IS 'Blog posts';
        ";

        let request = LintRequest::from_sql(sql);
        let result = lint(request).unwrap();

        assert!(result.issues.is_empty());
        assert_eq!(result.stats.total, 0);
        assert_eq!(result.stats.errors, 0);
        assert_eq!(result.stats.warnings, 0);
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
    fn test_lint_rejects_unknown_rule_ids() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
        ";

        let request =
            LintRequest::from_sql(sql).with_rules(vec!["definitely-not-a-rule".to_string()]);
        let err = lint(request).expect_err("unknown rule ids should be rejected");

        match err {
            AppError::Input { message, .. } => {
                assert!(message.contains("Unknown lint rule id"));
                assert!(message.contains("definitely-not-a-rule"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn test_format_lint_text_empty() {
        let result = LintResult {
            review: test_review(),
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
            .with_table_id("test")
            .with_table("test")
            .with_hint("Add a primary key"),
        );

        let result = LintResult {
            review: test_review(),
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
            review: test_review(),
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
            parse_rule_id("missing-table-comment"),
            Some(LintRuleId::MissingTableComment)
        );
        assert_eq!(
            parse_rule_id("NO-PRIMARY-KEY"),
            Some(LintRuleId::NoPrimaryKey)
        );
        assert_eq!(parse_rule_id("unknown"), None);
    }

    #[test]
    fn test_lint_profile_filters_missing_column_comment_by_default() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name TEXT
            );
            COMMENT ON TABLE users IS 'Application users';
        ";

        let result = lint(LintRequest::from_sql(sql)).unwrap();
        assert!(
            !result
                .issues
                .iter()
                .any(|issue| issue.rule_id == LintRuleId::MissingColumnComment)
        );
    }

    #[test]
    fn test_lint_profile_strict_enables_missing_column_comment() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name TEXT
            );
            COMMENT ON TABLE users IS 'Application users';
        ";

        let result =
            lint(LintRequest::from_sql(sql).with_profile(relune_core::LintProfile::Strict))
                .unwrap();
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.rule_id == LintRuleId::MissingColumnComment)
        );
    }

    #[test]
    fn test_lint_except_tables_suppresses_matching_issue() {
        let sql = r"
            CREATE TABLE audit_log (
                event_name TEXT
            );
        ";

        let result =
            lint(LintRequest::from_sql(sql).with_except_tables(vec!["audit_*".to_string()]))
                .unwrap();
        assert!(result.issues.is_empty());
        assert!(result.review.suppressed_issue_count > 0);
    }

    #[test]
    fn test_suppressed_issue_count_only_tracks_table_exceptions() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name TEXT
            );
            COMMENT ON TABLE users IS 'Application users';
        ";

        let result = lint(LintRequest::from_sql(sql)).unwrap();
        assert_eq!(result.review.suppressed_issue_count, 0);
    }

    #[test]
    fn test_sqlite_sql_input_disables_comment_review_rules() {
        let sql = r"
            CREATE TABLE posts (
                id INTEGER PRIMARY KEY,
                title TEXT NOT NULL
            );
        ";

        let request = LintRequest {
            input: crate::request::InputSource::sql_text_with_dialect(
                sql,
                relune_core::SqlDialect::Sqlite,
            ),
            ..LintRequest::default()
        };
        let result = lint(request).unwrap();
        assert!(
            !result
                .review
                .active_rules
                .iter()
                .any(|rule| is_comment_rule(rule.rule_id))
        );
        assert!(
            !result
                .issues
                .iter()
                .any(|issue| is_comment_rule(issue.rule_id))
        );
    }

    #[test]
    fn test_mysql_sql_input_disables_comment_review_rules() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255) NOT NULL
            ) COMMENT='Application users';
        ";

        let request = LintRequest {
            input: crate::request::InputSource::sql_text_with_dialect(
                sql,
                relune_core::SqlDialect::Mysql,
            ),
            ..LintRequest::default()
        };
        let result = lint(request).unwrap();
        assert!(
            !result
                .review
                .active_rules
                .iter()
                .any(|rule| is_comment_rule(rule.rule_id))
        );
        assert!(
            !result
                .issues
                .iter()
                .any(|issue| is_comment_rule(issue.rule_id))
        );
    }

    #[test]
    fn test_schema_json_input_disables_comment_review_rules() {
        let json = r#"
        {
          "version": "1.0.0",
          "tables": [
            {
              "id": "users",
              "schema": null,
              "name": "users",
              "columns": [
                {
                  "name": "id",
                  "data_type": "INT",
                  "nullable": false,
                  "primary_key": true
                }
              ],
              "foreign_keys": [],
              "indexes": []
            }
          ]
        }
        "#;

        let request = LintRequest {
            input: crate::request::InputSource::schema_json(json),
            ..LintRequest::default()
        };
        let result = lint(request).unwrap();
        assert!(
            !result
                .review
                .active_rules
                .iter()
                .any(|rule| is_comment_rule(rule.rule_id))
        );
        assert!(
            !result
                .issues
                .iter()
                .any(|issue| is_comment_rule(issue.rule_id))
        );
    }

    #[test]
    fn test_explicit_comment_rule_errors_when_input_does_not_support_comment_review() {
        let sql = r"
            CREATE TABLE posts (
                id INTEGER PRIMARY KEY
            );
        ";

        let request = LintRequest {
            input: crate::request::InputSource::sql_text_with_dialect(
                sql,
                relune_core::SqlDialect::Sqlite,
            ),
            rules: vec![
                "missing-table-comment".to_string(),
                "no-primary-key".to_string(),
            ],
            ..LintRequest::default()
        };

        let err = lint(request).expect_err("unsupported explicit comment rule should fail");
        match err {
            AppError::Input { message, .. } => {
                assert!(message.contains("Documentation lint rules are unavailable"));
                assert!(message.contains("missing-table-comment"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn test_explicit_documentation_category_errors_when_input_does_not_support_comment_review() {
        let sql = r"
            CREATE TABLE posts (
                id INTEGER PRIMARY KEY
            );
        ";

        let request = LintRequest {
            input: crate::request::InputSource::sql_text_with_dialect(
                sql,
                relune_core::SqlDialect::Sqlite,
            ),
            categories: vec![LintRuleCategory::Documentation],
            ..LintRequest::default()
        };

        let err = lint(request).expect_err("unsupported documentation category should fail");
        match err {
            AppError::Input { message, .. } => {
                assert!(message.contains("documentation lint category"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
