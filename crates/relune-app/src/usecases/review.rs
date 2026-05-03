//! Review use case: compare two schemas and flag migration risks.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use relune_core::{
    Diagnostic, EffectiveDialect, ReviewResult as CoreReviewResult, ReviewRuleId,
    ReviewRuleMetadata, ReviewSeverity, ReviewSeverityOverride, ReviewSummary, RiskFinding,
    SqlDialect, diagnostic::codes, diff_schemas, lock_risk_skip_diagnostic,
};

use crate::error::AppError;
use crate::request::ReviewRequest;
use crate::result::ReviewResult;
use crate::schema_input::schema_from_input_with_dialect;

/// Execute a review request.
#[allow(clippy::needless_pass_by_value)]
pub fn review(request: ReviewRequest) -> Result<ReviewResult, AppError> {
    let (before_schema, mut diagnostics, before_dialect) =
        schema_from_input_with_dialect(&request.before)?;
    let (after_schema, after_diagnostics, after_dialect) =
        schema_from_input_with_dialect(&request.after)?;
    diagnostics.extend(after_diagnostics);

    let schema_diff = diff_schemas(&before_schema, &after_schema);

    let applied_rules = resolve_active_rules(&request.rules, &request.except_rules)?;
    let override_map = build_override_map(&request.severity_overrides)?;
    let (effective, promotion_diagnostic) =
        resolve_effective_dialect(request.dialect, before_dialect, after_dialect);
    if let Some(diagnostic) = promotion_diagnostic {
        diagnostics.push(diagnostic);
    }
    let mut raw_findings = relune_core::run_rules(
        &schema_diff,
        &before_schema,
        &after_schema,
        &applied_rules,
        effective,
    );

    // Surface a single info-level diagnostic when the caller explicitly
    // opted in to a lock-risk rule that the resolved dialect cannot
    // evaluate. Default profiles stay silent to avoid CI noise.
    let explicit_rule_ids = parse_explicit_rule_ids(&request.rules)?;
    if let Some(diagnostic) =
        lock_risk_skip_diagnostic(&explicit_rule_ids, &applied_rules, effective)
    {
        diagnostics.push(diagnostic);
    }

    apply_severity_overrides(&mut raw_findings, &override_map);

    let (findings, suppressed) = partition_suppressed(raw_findings, &request.except_tables);

    let core_result = CoreReviewResult::from_parts(findings, suppressed, applied_rules);
    let denied = request
        .deny
        .is_some_and(|threshold| core_result.summary.has_findings_at_or_above(threshold));

    Ok(ReviewResult {
        review: core_result,
        diagnostics,
        denied,
        requested_dialect: request.dialect,
        effective_dialect: effective.into(),
    })
}

/// Resolve the effective review dialect from the requested dialect and
/// the parser-detected dialects of the before/after inputs.
///
/// When the caller passed an explicit dialect (`Postgres` / `Mysql` /
/// `Sqlite`), it is honored as-is. When the caller passed `Auto`, the
/// effective dialect is promoted to the parser-detected concrete dialect
/// when both sides agree; if the two sides resolved to different
/// concrete dialects, a warning diagnostic is emitted and the effective
/// dialect stays `Auto` (so lock-risk rules continue to skip rather than
/// running against a guessed dialect). Schema-JSON inputs carry no
/// parser dialect signal, so a missing side leaves the effective dialect
/// as `Auto` silently.
///
/// Returns the resolved effective dialect plus an optional warning
/// diagnostic surfaced to the caller.
fn resolve_effective_dialect(
    requested: SqlDialect,
    before: Option<SqlDialect>,
    after: Option<SqlDialect>,
) -> (EffectiveDialect, Option<Diagnostic>) {
    if requested != SqlDialect::Auto {
        return (requested.into(), None);
    }

    match (before, after) {
        (Some(b), Some(a)) if b == a => (b.into(), None),
        (Some(b), Some(a)) => {
            let diagnostic = Diagnostic::warning(
                codes::review_dialect_mismatch(),
                format!(
                    "Auto dialect resolved to {b} for the before input but {a} for the after input; \
                     lock-risk rules will not fire. Pass --dialect explicitly to pin the review dialect."
                ),
            );
            (EffectiveDialect::Auto, Some(diagnostic))
        }
        _ => (EffectiveDialect::Auto, None),
    }
}

fn build_override_map(
    overrides: &[ReviewSeverityOverride],
) -> Result<HashMap<ReviewRuleId, ReviewSeverity>, AppError> {
    let mut map = HashMap::with_capacity(overrides.len());
    for entry in overrides {
        if map.insert(entry.rule_id, entry.severity).is_some() {
            return Err(AppError::input(format!(
                "duplicate severity override for rule_id {}",
                entry.rule_id.as_str()
            )));
        }
    }
    Ok(map)
}

fn apply_severity_overrides(
    findings: &mut [RiskFinding],
    overrides: &HashMap<ReviewRuleId, ReviewSeverity>,
) {
    if overrides.is_empty() {
        return;
    }
    for finding in findings.iter_mut() {
        if let Some(severity) = overrides.get(&finding.rule_id) {
            finding.severity = *severity;
        }
    }
}

/// Render the review result as plain text.
#[must_use]
pub fn format_review_text(result: &ReviewResult) -> String {
    format_review_text_with(result, false)
}

/// Render the review result as plain text, optionally suppressing
/// findings detail bodies when `quiet` is true (summary only).
#[must_use]
pub fn format_review_text_with(result: &ReviewResult, quiet: bool) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "Schema review");
    output.push('\n');

    write_summary_line(&mut output, &result.review.summary);
    if let Some(line) = effective_dialect_text(result) {
        let _ = writeln!(output, "  {line}");
    }
    output.push('\n');

    if quiet {
        return output;
    }

    if result.review.findings.is_empty() {
        output.push_str("No risk findings.\n");
        return output;
    }

    for severity in [
        ReviewSeverity::Breaking,
        ReviewSeverity::Caution,
        ReviewSeverity::Warning,
        ReviewSeverity::Info,
    ] {
        let group: Vec<&RiskFinding> = result
            .review
            .findings
            .iter()
            .filter(|f| f.severity == severity)
            .collect();
        if group.is_empty() {
            continue;
        }
        let _ = writeln!(output, "  {}", severity.as_str());
        for finding in group {
            write_text_finding(&mut output, finding);
        }
        output.push('\n');
    }

    output
}

/// Render the review result as markdown.
#[must_use]
pub fn format_review_markdown(result: &ReviewResult) -> String {
    format_review_markdown_with(result, false)
}

/// Render the review result as markdown, optionally suppressing
/// findings detail bodies when `quiet` is true (summary only).
#[must_use]
pub fn format_review_markdown_with(result: &ReviewResult, quiet: bool) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "## Schema review");
    output.push('\n');

    let s = &result.review.summary;
    let breaking = bold_when(s.breaking > 0, &format!("{} breaking", s.breaking));
    let caution = bold_when(s.caution > 0, &format!("{} caution", s.caution));
    let warning = bold_when(s.warning > 0, &format!("{} warning", s.warning));
    let _ = writeln!(
        output,
        "{breaking} · {caution} · {warning} · {} info",
        s.info,
    );
    if let Some(line) = effective_dialect_text(result) {
        let _ = writeln!(output, "_{line}_");
    }
    output.push('\n');

    if quiet {
        return output;
    }

    if result.review.findings.is_empty() {
        let _ = writeln!(output, "_No risk findings._");
        return output;
    }

    for (severity, heading) in [
        (ReviewSeverity::Breaking, "### Breaking"),
        (ReviewSeverity::Caution, "### Caution"),
        (ReviewSeverity::Warning, "### Warning"),
        (ReviewSeverity::Info, "### Info"),
    ] {
        let group: Vec<&RiskFinding> = result
            .review
            .findings
            .iter()
            .filter(|f| f.severity == severity)
            .collect();
        if group.is_empty() {
            continue;
        }
        let _ = writeln!(output, "{heading}");
        output.push('\n');
        for finding in group {
            write_markdown_finding(&mut output, finding);
        }
    }

    output
}

/// Render the review result as JSON.
pub fn format_review_json(result: &ReviewResult) -> Result<String, AppError> {
    Ok(serde_json::to_string_pretty(result)?)
}

/// Expand the applied rules of a review result into per-rule metadata snapshots.
///
/// Used by the WASM bindings (and any future CLI surface) to surface the
/// `rule_id` / `default_severity` / `description` triple alongside the review
/// payload without bloating the core `ReviewResult` JSON shape.
#[must_use]
pub fn applied_rule_metadata(result: &ReviewResult) -> Vec<ReviewRuleMetadata> {
    result
        .review
        .applied_rules
        .iter()
        .map(ReviewRuleId::metadata)
        .collect()
}

/// Format a one-line note describing how the review pipeline resolved
/// the dialect, suitable for the text and markdown formatters.
///
/// Returns `None` when the requested dialect was already concrete and
/// matches the effective dialect (no UX value in restating it). Otherwise
/// either announces the auto-promotion (`auto resolved to postgres`) or
/// the fallback when promotion could not happen.
fn effective_dialect_text(result: &ReviewResult) -> Option<String> {
    use relune_core::SqlDialect;

    let requested = result.requested_dialect;
    let effective = result.effective_dialect;

    match (requested, effective) {
        (SqlDialect::Auto, SqlDialect::Auto) => Some(
            "Effective dialect: auto (lock-risk rules inactive; pass --dialect to enable)"
                .to_string(),
        ),
        (SqlDialect::Auto, concrete) => Some(format!(
            "Effective dialect: {concrete} (resolved from auto)"
        )),
        _ => None,
    }
}

fn write_summary_line(output: &mut String, summary: &ReviewSummary) {
    let _ = writeln!(
        output,
        "  {} breaking · {} caution · {} warning · {} info",
        summary.breaking, summary.caution, summary.warning, summary.info,
    );
}

fn write_text_finding(output: &mut String, finding: &RiskFinding) {
    let target = format_target(finding);
    let _ = writeln!(output, "    [{}] {}", finding.rule_id.as_str(), target);
    let _ = writeln!(output, "      {}", finding.message);
    if let Some(mitigation) = &finding.mitigation {
        let _ = writeln!(output, "      mitigation: {mitigation}");
    }
}

fn write_markdown_finding(output: &mut String, finding: &RiskFinding) {
    let target = format_target(finding);
    let _ = writeln!(
        output,
        "- **`{}`** — `{}`",
        finding.rule_id.as_str(),
        target
    );
    let _ = writeln!(output, "  {}", finding.message);
    if let Some(mitigation) = &finding.mitigation {
        let _ = writeln!(output, "  _mitigation: {mitigation}_");
    }
}

fn format_target(finding: &RiskFinding) -> String {
    match (&finding.table_name, &finding.column_name, &finding.fk_name) {
        (Some(t), Some(c), _) => format!("{t}.{c}"),
        (Some(t), None, Some(fk)) => format!("{t}.{fk}"),
        (Some(t), None, None) => t.clone(),
        (None, _, Some(fk)) => fk.clone(),
        (None, _, None) => "(schema)".to_string(),
    }
}

fn bold_when(condition: bool, text: &str) -> String {
    if condition {
        format!("**{text}**")
    } else {
        text.to_string()
    }
}

fn resolve_active_rules(
    rules: &[String],
    except_rules: &[String],
) -> Result<Vec<ReviewRuleId>, AppError> {
    let mut active: Vec<ReviewRuleId> = if rules.is_empty() {
        ReviewRuleId::all_rules().to_vec()
    } else {
        let mut parsed = Vec::with_capacity(rules.len());
        for rule in rules {
            parsed.push(parse_rule_id(rule)?);
        }
        parsed
    };

    if !except_rules.is_empty() {
        let exclude: HashSet<ReviewRuleId> = except_rules
            .iter()
            .map(|s| parse_rule_id(s))
            .collect::<Result<_, _>>()?;
        active.retain(|rule| !exclude.contains(rule));
    }

    if active.is_empty() {
        return Err(AppError::input(
            "No review rules remain after applying the selected filters".to_string(),
        ));
    }

    Ok(active)
}

fn parse_rule_id(value: &str) -> Result<ReviewRuleId, AppError> {
    let normalized = if value.contains('/') {
        value.to_string()
    } else {
        format!("risk/{value}")
    };
    ReviewRuleId::parse(&normalized).map_err(AppError::input)
}

fn parse_explicit_rule_ids(rules: &[String]) -> Result<Vec<ReviewRuleId>, AppError> {
    rules.iter().map(|s| parse_rule_id(s)).collect()
}

fn partition_suppressed(
    findings: Vec<RiskFinding>,
    except_tables: &[String],
) -> (Vec<RiskFinding>, Vec<RiskFinding>) {
    if except_tables.is_empty() {
        return (findings, Vec::new());
    }
    let mut active = Vec::with_capacity(findings.len());
    let mut suppressed = Vec::new();
    for finding in findings {
        if matches_except_table(except_tables, &finding) {
            suppressed.push(finding);
        } else {
            active.push(finding);
        }
    }
    (active, suppressed)
}

fn matches_except_table(patterns: &[String], finding: &RiskFinding) -> bool {
    let Some(name) = finding.table_name.as_deref() else {
        return false;
    };
    let short = name.rsplit('.').next().unwrap_or(name);
    patterns
        .iter()
        .any(|pattern| matches_pattern(pattern, name) || matches_pattern(pattern, short))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn run(request: ReviewRequest) -> ReviewResult {
        review(request).expect("review should succeed")
    }

    #[test]
    fn review_no_changes_has_no_findings() {
        let sql = "CREATE TABLE users (id INT PRIMARY KEY);";
        let result = run(ReviewRequest::from_sql(sql, sql));
        assert!(result.review.findings.is_empty());
        assert_eq!(result.review.summary.total(), 0);
        assert!(!result.denied);
    }

    #[test]
    fn drop_referenced_column_emits_breaking() {
        let before = "
            CREATE TABLE users (id INT PRIMARY KEY, email TEXT);
            CREATE TABLE orders (
                id INT PRIMARY KEY,
                user_email TEXT REFERENCES users(email)
            );
        ";
        let after = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (
                id INT PRIMARY KEY,
                user_email TEXT REFERENCES users(email)
            );
        ";
        let result = run(ReviewRequest::from_sql(before, after));
        let breaking_finding = result
            .review
            .findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::DropColumnReferenced)
            .expect("expected drop-column-referenced finding");
        assert_eq!(breaking_finding.severity, ReviewSeverity::Breaking);
    }

    #[test]
    fn except_rules_suppresses_target_rule() {
        let before = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (id INT PRIMARY KEY);
        ";
        let after = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
        ";
        let baseline = run(ReviewRequest::from_sql(before, after));
        assert!(
            baseline
                .review
                .findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::FkWithoutIndex),
            "baseline should produce the rule we then suppress",
        );

        let request = ReviewRequest::from_sql(before, after)
            .with_except_rules(vec!["risk/fk-without-index".to_string()]);
        let result = run(request);
        assert!(
            !result
                .review
                .findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::FkWithoutIndex)
        );
        assert!(
            !result
                .review
                .applied_rules
                .contains(&ReviewRuleId::FkWithoutIndex)
        );
    }

    #[test]
    fn except_tables_moves_finding_to_suppressed() {
        let before = "CREATE TABLE audit_log (id INT PRIMARY KEY);";
        let after = "CREATE TABLE audit_log (id INT PRIMARY KEY, created_at TIMESTAMP NOT NULL);";
        let request =
            ReviewRequest::from_sql(before, after).with_except_tables(vec!["audit_*".to_string()]);
        let result = run(request);
        assert!(result.review.findings.is_empty());
        assert!(!result.review.suppressed.is_empty());
    }

    #[test]
    fn deny_threshold_flips_denied() {
        let before = "
            CREATE TABLE users (id INT PRIMARY KEY, email TEXT);
            CREATE TABLE orders (
                id INT PRIMARY KEY,
                user_email TEXT REFERENCES users(email)
            );
        ";
        let after = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (
                id INT PRIMARY KEY,
                user_email TEXT REFERENCES users(email)
            );
        ";
        let request = ReviewRequest::from_sql(before, after).with_deny(ReviewSeverity::Breaking);
        let result = run(request);
        assert!(result.denied);
    }

    #[test]
    fn unknown_rule_id_errors_out() {
        let request =
            ReviewRequest::from_sql("CREATE TABLE t (id INT);", "CREATE TABLE t (id INT);")
                .with_rules(vec!["risk/bogus".to_string()]);
        let err = review(request).expect_err("unknown rule should error");
        match err {
            AppError::Input { message, .. } => {
                assert!(message.contains("unknown review rule"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn rule_id_accepts_short_name_without_prefix() {
        let request =
            ReviewRequest::from_sql("CREATE TABLE t (id INT);", "CREATE TABLE t (id INT);")
                .with_rules(vec!["fk-without-index".to_string()]);
        let result = review(request).expect("short rule id should be accepted");
        assert!(result.review.findings.is_empty());
        assert_eq!(
            result.review.applied_rules,
            vec![ReviewRuleId::FkWithoutIndex]
        );
    }

    #[test]
    fn format_text_includes_rule_ids_and_severity_buckets() {
        let before = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (id INT PRIMARY KEY);
        ";
        let after = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
        ";
        let result = run(ReviewRequest::from_sql(before, after));
        let text = format_review_text(&result);
        assert!(text.contains("Schema review"));
        assert!(text.contains("info"));
        assert!(text.contains("risk/fk-without-index"));
    }

    #[test]
    fn severity_override_downgrades_warning_to_info() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);";
        let after = "CREATE TABLE users (id INT PRIMARY KEY, email TEXT NOT NULL);";

        // Sanity: the baseline produces a warning-level finding.
        let baseline = run(ReviewRequest::from_sql(before, after));
        assert!(
            baseline
                .review
                .findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::AddNotNullOnExisting
                    && f.severity == ReviewSeverity::Warning),
            "baseline should produce add-not-null-on-existing at warning",
        );
        assert_eq!(baseline.review.summary.warning, 1);

        let request = ReviewRequest::from_sql(before, after).with_severity_overrides(vec![
            ReviewSeverityOverride {
                rule_id: ReviewRuleId::AddNotNullOnExisting,
                severity: ReviewSeverity::Info,
            },
        ]);
        let result = run(request);

        assert_eq!(result.review.summary.warning, 0);
        assert_eq!(result.review.summary.info, 1);
        let finding = result
            .review
            .findings
            .iter()
            .find(|f| f.rule_id == ReviewRuleId::AddNotNullOnExisting)
            .expect("override should not drop the finding");
        assert_eq!(finding.severity, ReviewSeverity::Info);
    }

    #[test]
    fn severity_override_upgrades_info_to_warning_and_trips_deny() {
        let before = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (id INT PRIMARY KEY);
        ";
        let after = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
        ";

        // Schema-JSON inputs prevent auto-promotion to a concrete
        // dialect so this test isolates the severity-override logic
        // from the lock-risk rules that would otherwise fire under
        // promoted Postgres / MySQL.
        let baseline = schema_json_request(before, after).with_deny(ReviewSeverity::Warning);
        assert!(!run(baseline).denied);

        let request = schema_json_request(before, after)
            .with_deny(ReviewSeverity::Warning)
            .with_severity_overrides(vec![ReviewSeverityOverride {
                rule_id: ReviewRuleId::FkWithoutIndex,
                severity: ReviewSeverity::Warning,
            }]);
        let result = run(request);

        assert_eq!(result.review.summary.warning, 1);
        assert_eq!(result.review.summary.info, 0);
        assert!(result.denied);
    }

    #[test]
    fn severity_override_rejects_duplicate_rule_id() {
        let request =
            ReviewRequest::from_sql("CREATE TABLE t (id INT);", "CREATE TABLE t (id INT);")
                .with_severity_overrides(vec![
                    ReviewSeverityOverride {
                        rule_id: ReviewRuleId::AddNotNullOnExisting,
                        severity: ReviewSeverity::Info,
                    },
                    ReviewSeverityOverride {
                        rule_id: ReviewRuleId::AddNotNullOnExisting,
                        severity: ReviewSeverity::Warning,
                    },
                ]);

        let err = review(request).expect_err("duplicate override should fail");
        match err {
            AppError::Input { message, .. } => {
                assert!(message.contains("duplicate severity override"));
                assert!(message.contains("risk/add-not-null-on-existing"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn dialect_default_is_auto_and_emits_no_diagnostic() {
        // Default profile (no `request.rules`) under `Auto` should
        // produce zero findings and zero lock-risk skip diagnostics.
        let sql = "CREATE TABLE users (id INT PRIMARY KEY);";
        let result = run(ReviewRequest::from_sql(sql, sql));
        assert!(result.diagnostics.is_empty());
        assert_eq!(
            relune_core::SqlDialect::default(),
            relune_core::SqlDialect::Auto,
        );
    }

    #[test]
    fn explicit_lock_risk_opt_in_under_auto_emits_skip_diagnostic() {
        // Schema-JSON inputs carry no parser-dialect signal, so the
        // review pipeline cannot promote `Auto` to a concrete dialect.
        // Lock-risk rules opted in via `--rules` must therefore emit the
        // info-level skip diagnostic so callers know nothing fired.
        let request = schema_json_request(
            "CREATE TABLE users (id INT PRIMARY KEY);",
            "CREATE TABLE users (id INT PRIMARY KEY);",
        )
        .with_rules(vec!["risk/add-index-on-large-table".to_string()]);
        let result = run(request);
        assert_eq!(result.diagnostics.len(), 1);
        let diag = &result.diagnostics[0];
        assert_eq!(diag.severity, relune_core::Severity::Info);
        assert!(
            diag.message
                .contains("Lock-risk review rules require an explicit --dialect"),
            "unexpected message: {}",
            diag.message,
        );
    }

    #[test]
    fn auto_promotes_to_postgres_when_both_sides_resolve_postgres() {
        // Postgres-leaning syntax (SERIAL) makes the parser detect
        // postgres on both sides; under `Auto` the review pipeline
        // should promote to `Postgres` so lock-risk rules can fire.
        let before = "CREATE TABLE users (id SERIAL PRIMARY KEY);";
        let after = "
            CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT);
            CREATE INDEX users_email_idx ON users(email);
        ";
        let result = run(ReviewRequest::from_sql(before, after));
        assert_eq!(result.requested_dialect, relune_core::SqlDialect::Auto);
        assert_eq!(result.effective_dialect, relune_core::SqlDialect::Postgres);
        assert!(
            result
                .review
                .findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::AddIndexOnLargeTable),
            "expected risk/add-index-on-large-table to fire after promotion",
        );
        assert!(
            result.diagnostics.is_empty(),
            "promotion should not emit diagnostics: {:?}",
            result.diagnostics,
        );
    }

    #[test]
    fn auto_promotes_to_mysql_when_both_sides_resolve_mysql() {
        let before = "CREATE TABLE `users` (id INT AUTO_INCREMENT PRIMARY KEY) ENGINE=InnoDB;";
        let after = "CREATE TABLE `users` (id INT AUTO_INCREMENT PRIMARY KEY, email VARCHAR(255)) ENGINE=InnoDB;";
        let result = run(ReviewRequest::from_sql(before, after));
        assert_eq!(result.effective_dialect, relune_core::SqlDialect::Mysql);
    }

    #[test]
    fn auto_emits_warning_on_dialect_mismatch_and_stays_auto() {
        // Postgres-leaning before vs. MySQL-leaning after: the parser
        // resolves to different concrete dialects, so the review
        // pipeline must keep `Auto` (lock-risk rules stay inactive) and
        // surface a warning so the operator can pin the dialect.
        let before = "CREATE TABLE users (id SERIAL PRIMARY KEY);";
        let after = "CREATE TABLE `users` (id INT AUTO_INCREMENT PRIMARY KEY) ENGINE=InnoDB;";
        let result = run(ReviewRequest::from_sql(before, after));
        assert_eq!(result.effective_dialect, relune_core::SqlDialect::Auto);
        let warning = result
            .diagnostics
            .iter()
            .find(|d| d.severity == relune_core::Severity::Warning)
            .expect("mismatch should emit a warning diagnostic");
        assert_eq!(
            warning.code,
            relune_core::diagnostic::codes::review_dialect_mismatch()
        );
        assert!(warning.message.contains("postgres"));
        assert!(warning.message.contains("mysql"));
    }

    #[test]
    fn auto_promotes_to_sqlite_when_both_sides_resolve_sqlite() {
        // Sqlite-leaning syntax (AUTOINCREMENT) makes the parser detect
        // sqlite on both sides; the review pipeline must promote to
        // `Sqlite` (so the playground / CLI can show the resolved
        // dialect), but lock-risk rules stay inactive on sqlite.
        let sql = "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, email TEXT);";
        let result = run(ReviewRequest::from_sql(sql, sql));
        assert_eq!(result.effective_dialect, relune_core::SqlDialect::Sqlite);
        assert!(
            result.review.findings.is_empty(),
            "sqlite should keep lock-risk inactive; got: {:?}",
            result.review.findings,
        );
    }

    #[test]
    fn auto_stays_auto_when_only_one_side_carries_dialect_signal() {
        // Mixing SQL (parser dialect available) with schema JSON (no
        // parser-side dialect) should leave `Auto` intact and stay
        // silent: the schema-JSON side gives no signal to compare
        // against, so there is nothing to promote and nothing to warn
        // about. Lock-risk callers can pin `--dialect` explicitly when
        // they know the JSON came from a specific engine.
        use crate::request::InputSource;
        use relune_core::export::export_schema;
        use relune_parser_sql::parse_sql_to_schema;

        let before_sql = "CREATE TABLE users (id SERIAL PRIMARY KEY);";
        let after_json = serde_json::to_string(&export_schema(
            &parse_sql_to_schema("CREATE TABLE users (id SERIAL PRIMARY KEY);").expect("parse"),
        ))
        .expect("serialize");
        let request = ReviewRequest {
            before: InputSource::sql_text_with_dialect(before_sql, relune_core::SqlDialect::Auto),
            after: InputSource::SchemaJson { json: after_json },
            ..Default::default()
        };
        let result = run(request);
        assert_eq!(result.effective_dialect, relune_core::SqlDialect::Auto);
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.code != relune_core::diagnostic::codes::review_dialect_mismatch()),
            "missing parser signal must not emit REVIEW002: {:?}",
            result.diagnostics,
        );
    }

    #[test]
    fn explicit_dialect_overrides_parser_resolution() {
        // Even when the parser would resolve to MySQL on both sides,
        // the explicit `--dialect postgres` request must win so the
        // operator stays in control of which lock-risk rules fire.
        let sql = "CREATE TABLE `users` (id INT AUTO_INCREMENT PRIMARY KEY) ENGINE=InnoDB;";
        let request =
            ReviewRequest::from_sql(sql, sql).with_dialect(relune_core::SqlDialect::Postgres);
        let result = run(request);
        assert_eq!(result.effective_dialect, relune_core::SqlDialect::Postgres);
    }

    fn schema_json_request(before_sql: &str, after_sql: &str) -> ReviewRequest {
        use crate::request::InputSource;
        use relune_core::export::export_schema;
        use relune_parser_sql::parse_sql_to_schema;

        let before_json = serde_json::to_string(&export_schema(
            &parse_sql_to_schema(before_sql).expect("parse before"),
        ))
        .expect("serialize before");
        let after_json = serde_json::to_string(&export_schema(
            &parse_sql_to_schema(after_sql).expect("parse after"),
        ))
        .expect("serialize after");

        ReviewRequest {
            before: InputSource::SchemaJson { json: before_json },
            after: InputSource::SchemaJson { json: after_json },
            ..Default::default()
        }
    }

    #[test]
    fn explicit_opt_in_under_postgres_does_not_emit_skip_diagnostic() {
        let sql = "CREATE TABLE users (id INT PRIMARY KEY);";
        let request = ReviewRequest::from_sql(sql, sql)
            .with_rules(vec!["risk/add-index-on-large-table".to_string()])
            .with_dialect(relune_core::SqlDialect::Postgres);
        let result = run(request);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn explicit_opt_in_dialect_mismatch_emits_skip_diagnostic() {
        let sql = "CREATE TABLE users (id INT PRIMARY KEY);";
        let request = ReviewRequest::from_sql(sql, sql)
            .with_rules(vec!["risk/rewrite-table".to_string()])
            .with_dialect(relune_core::SqlDialect::Postgres);
        let result = run(request);
        assert_eq!(result.diagnostics.len(), 1);
        assert!(
            result.diagnostics[0]
                .message
                .contains("require dialect mysql"),
            "unexpected message: {}",
            result.diagnostics[0].message,
        );
    }

    #[test]
    fn except_rule_suppresses_skip_diagnostic_for_opted_in_lock_risk() {
        // The user opted in via `--rules`, then removed the same rule
        // via `--except-rule`. Nothing was actually attempted, so no
        // skip diagnostic should fire (and the request must keep at
        // least one active rule to avoid the "no rules remain" error).
        let sql = "CREATE TABLE users (id INT PRIMARY KEY);";
        let request = ReviewRequest::from_sql(sql, sql)
            .with_rules(vec![
                "risk/add-index-on-large-table".to_string(),
                "risk/fk-without-index".to_string(),
            ])
            .with_except_rules(vec!["risk/add-index-on-large-table".to_string()]);
        let result = run(request);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn format_markdown_uses_bullet_per_finding() {
        let before = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (id INT PRIMARY KEY);
        ";
        let after = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
        ";
        let result = run(ReviewRequest::from_sql(before, after));
        let md = format_review_markdown(&result);
        assert!(md.contains("## Schema review"));
        assert!(md.contains("- **`risk/fk-without-index`**"));
    }
}
