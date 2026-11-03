//! Review use case: compare two schemas and flag migration risks.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use relune_core::{
    Diagnostic, EffectiveDialect, ExecutionVerdict, MigrationOperation, MigrationScript,
    ReviewResult as CoreReviewResult, ReviewRuleId, ReviewRuleMetadata, ReviewSeverity,
    ReviewSeverityOverride, ReviewSummary, RiskFinding, Schema, SqlDialect, assess_migration,
    diagnostic::codes, diff_schemas, lock_risk_skip_diagnostic, pattern::matches_table_pattern,
};
use relune_parser_sql::{MigrationFile, parse_migration};

use crate::error::AppError;
use crate::markdown;
use crate::request::{ReviewFormat, ReviewRequest};
use crate::result::{
    MigrationCoverage, ReviewInputCoverage, ReviewInputs, ReviewResult, UnanalyzedStatement,
};
use crate::schema_input::{SchemaValidation, align_input_schemas, schema_from_input_checked};
use crate::usecases::diff::render_review_diagram;
use crate::usecases::review_report::build_review_report;

/// Execute a review request.
#[allow(clippy::needless_pass_by_value)]
pub fn review(request: ReviewRequest) -> Result<ReviewResult, AppError> {
    let (mut before_schema, mut diagnostics, before_context) = schema_from_input_checked(
        &request.before,
        SchemaValidation::for_comparison("before", request.allow_invalid_schema),
    )?;
    let (mut after_schema, after_diagnostics, after_context) = schema_from_input_checked(
        &request.after,
        SchemaValidation::for_comparison("after", request.allow_invalid_schema),
    )?;
    align_input_schemas(
        &mut before_schema,
        before_context,
        &mut after_schema,
        after_context,
    );
    let inputs = ReviewInputs {
        before: input_coverage(&before_schema, &diagnostics),
        after: input_coverage(&after_schema, &after_diagnostics),
    };
    diagnostics.extend(after_diagnostics);
    let before_dialect = before_context.resolved_dialect;
    let after_dialect = after_context.resolved_dialect;

    let (effective, promotion_diagnostic) =
        resolve_effective_dialect(request.dialect, before_dialect, after_dialect);
    if let Some(diagnostic) = promotion_diagnostic {
        diagnostics.push(diagnostic);
    }

    // Renames in the migration line the schemas up before they are compared,
    // so a renamed column is not reported as dropped and re-added.
    let script = read_migration(&request, effective, &mut diagnostics);
    if let Some(script) = &script {
        script.apply_renames(&mut before_schema);
    }

    let schema_diff = diff_schemas(&before_schema, &after_schema);
    let RuleEvaluation {
        findings,
        suppressed,
        applied_rules,
    } = evaluate_rules(
        RuleSelection {
            rules: &request.rules,
            except_rules: &request.except_rules,
            except_tables: &request.except_tables,
            severity_overrides: &request.severity_overrides,
        },
        &schema_diff,
        &before_schema,
        &after_schema,
        effective,
        script.as_ref(),
    )?;

    // Surface a single info-level diagnostic when the caller explicitly
    // opted in to a lock-risk rule that the resolved dialect cannot
    // evaluate. Default profiles stay silent to avoid CI noise.
    let explicit_rule_ids = parse_explicit_rule_ids(&request.rules)?;
    if let Some(diagnostic) =
        lock_risk_skip_diagnostic(&explicit_rule_ids, &applied_rules, effective)
    {
        diagnostics.push(diagnostic);
    }

    let core_result = CoreReviewResult::from_parts(findings, suppressed, applied_rules);
    let denied = request
        .deny
        .is_some_and(|threshold| core_result.summary.has_findings_at_or_above(threshold));

    let mut result = ReviewResult {
        review: core_result,
        diagnostics,
        denied,
        requested_dialect: request.dialect,
        effective_dialect: effective.into(),
        inputs,
        migration: script
            .as_ref()
            .map(|script| migration_coverage(&request, script)),
        rendered: None,
    };

    if request.format == ReviewFormat::Html {
        let report = build_review_report(
            &result,
            &before_schema,
            &after_schema,
            script.as_ref(),
            effective_dialect_text(&result),
            coverage_notes(&result),
        );
        let (html, notice) = render_review_diagram(
            &before_schema,
            &after_schema,
            &schema_diff,
            &result.review.findings,
            &request.diagram,
            &report,
        )?;
        result.diagnostics.extend(notice);
        result.rendered = Some(html);
    }

    Ok(result)
}

/// Rule settings shared by `review` and the risk labels of visual diffs.
#[derive(Clone, Copy)]
pub(crate) struct RuleSelection<'a> {
    pub(crate) rules: &'a [String],
    pub(crate) except_rules: &'a [String],
    pub(crate) except_tables: &'a [String],
    pub(crate) severity_overrides: &'a [ReviewSeverityOverride],
}

/// Findings of the selected rules, after severity overrides.
pub(crate) struct RuleEvaluation {
    /// Findings on tables no `except_tables` pattern matches.
    pub(crate) findings: Vec<RiskFinding>,
    /// Findings on tables an `except_tables` pattern suppresses.
    pub(crate) suppressed: Vec<RiskFinding>,
    /// Rules that ran.
    pub(crate) applied_rules: Vec<ReviewRuleId>,
}

/// Runs the selected rules on a schema diff and applies severity overrides
/// and table suppression, the same way for `review` and visual diffs.
///
/// With a migration `script`, findings are then reconciled with the
/// statements that make the change; a finding whose risk the statements
/// avoid keeps the `info` severity that sets, whatever the overrides say.
pub(crate) fn evaluate_rules(
    selection: RuleSelection<'_>,
    schema_diff: &relune_core::SchemaDiff,
    before: &Schema,
    after: &Schema,
    dialect: EffectiveDialect,
    script: Option<&MigrationScript>,
) -> Result<RuleEvaluation, AppError> {
    let applied_rules = resolve_active_rules(selection.rules, selection.except_rules)?;
    let override_map = build_override_map(selection.severity_overrides)?;
    let mut raw_findings =
        relune_core::run_rules(schema_diff, before, after, &applied_rules, dialect);
    if let Some(script) = script {
        assess_migration(
            &mut raw_findings,
            script,
            before,
            after,
            &applied_rules,
            dialect,
        );
    }
    apply_severity_overrides(&mut raw_findings, &override_map);
    let (findings, suppressed) = partition_suppressed(raw_findings, selection.except_tables);
    Ok(RuleEvaluation {
        findings,
        suppressed,
        applied_rules,
    })
}

/// Parses the request's migration SQL, when it has any, in the review dialect.
fn read_migration(
    request: &ReviewRequest,
    dialect: EffectiveDialect,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MigrationScript> {
    if request.migrations.is_empty() {
        return None;
    }
    let files: Vec<MigrationFile> = request
        .migrations
        .iter()
        .map(|migration| MigrationFile {
            name: migration.name.clone(),
            sql: migration.sql.clone(),
        })
        .collect();
    let output = parse_migration(&files, dialect.into(), request.migration_transaction);
    diagnostics.extend(output.diagnostics);
    Some(output.script)
}

fn migration_coverage(request: &ReviewRequest, script: &MigrationScript) -> MigrationCoverage {
    MigrationCoverage {
        files: request
            .migrations
            .iter()
            .map(|migration| migration.name.clone())
            .collect(),
        statements: script.statements.len(),
        transaction_mode: script.transaction_mode,
        unanalyzed: script
            .unanalyzed()
            .map(|statement| UnanalyzedStatement {
                location: statement.location.clone(),
                reason: statement
                    .operations
                    .iter()
                    .find_map(|operation| match operation {
                        MigrationOperation::Unanalyzed { reason } => Some(reason.clone()),
                        _ => None,
                    })
                    .unwrap_or_default(),
            })
            .collect(),
    }
}

/// Everything the review could not evaluate: incomplete inputs and
/// migration statements it could not read.
fn coverage_notes(result: &ReviewResult) -> Vec<String> {
    let mut notes = input_coverage_notes(&result.inputs);
    if let Some(migration) = &result.migration {
        notes.extend(migration.unanalyzed.iter().map(|statement| {
            format!(
                "Migration statement at {} was not analyzed ({}); changes it makes are not reviewed.",
                statement.location, statement.reason
            )
        }));
    }
    notes
}

fn input_coverage(schema: &Schema, diagnostics: &[Diagnostic]) -> ReviewInputCoverage {
    let unsupported_code = codes::parse_unsupported();
    ReviewInputCoverage {
        empty: schema.tables.is_empty() && schema.views.is_empty() && schema.enums.is_empty(),
        unsupported_constructs: diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == unsupported_code)
            .count(),
    }
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
pub(crate) fn resolve_effective_dialect(
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
    let mitigated = |finding: &RiskFinding| {
        finding
            .execution
            .as_ref()
            .is_some_and(|note| note.verdict == ExecutionVerdict::Mitigated)
    };
    for finding in findings.iter_mut().filter(|finding| !mitigated(finding)) {
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
    if let Some(line) = migration_text(result) {
        let _ = writeln!(output, "  {line}");
    }
    output.push('\n');

    let coverage_notes = coverage_notes(result);
    if !coverage_notes.is_empty() {
        let _ = writeln!(output, "  {INCOMPLETE_COVERAGE_HEADING}");
        for note in &coverage_notes {
            let _ = writeln!(output, "    - {note}");
        }
        output.push('\n');
    }

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
    if let Some(line) = migration_text(result) {
        let _ = writeln!(output, "_{}_", markdown::text(&line));
    }
    output.push('\n');

    let coverage_notes = coverage_notes(result);
    if !coverage_notes.is_empty() {
        let _ = writeln!(output, "> [!WARNING]");
        let _ = writeln!(output, "> {INCOMPLETE_COVERAGE_HEADING}");
        for note in &coverage_notes {
            let _ = writeln!(output, "> - {note}");
        }
        output.push('\n');
    }

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

const INCOMPLETE_COVERAGE_HEADING: &str =
    "Review coverage is incomplete; missing findings do not mean the migration is safe:";

/// Describe why the review may have missed changes, one note per issue.
fn input_coverage_notes(inputs: &ReviewInputs) -> Vec<String> {
    let mut notes = Vec::new();
    for (label, coverage, empty_effect) in [
        (
            "before",
            &inputs.before,
            "every object in the after input is treated as newly added",
        ),
        (
            "after",
            &inputs.after,
            "every object in the before input is treated as dropped",
        ),
    ] {
        if coverage.empty {
            notes.push(format!(
                "The {label} input produced no schema objects; {empty_effect}."
            ));
        }
        match coverage.unsupported_constructs {
            0 => {}
            1 => notes.push(format!(
                "1 unsupported SQL construct in the {label} input was skipped; changes it contains are not reviewed."
            )),
            count => notes.push(format!(
                "{count} unsupported SQL constructs in the {label} input were skipped; changes they contain are not reviewed."
            )),
        }
    }
    notes
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
    if let Some(sources) = sources_text(finding) {
        let _ = writeln!(output, "      at: {sources}");
    }
    if let Some(execution) = &finding.execution {
        let _ = writeln!(
            output,
            "      execution ({}): {}",
            execution.verdict.as_str(),
            execution.detail
        );
    }
    if let Some(mitigation) = &finding.mitigation {
        let _ = writeln!(output, "      mitigation: {mitigation}");
    }
}

/// Migration statements a finding points at, e.g. `001.sql:3, 002.sql:1`.
fn sources_text(finding: &RiskFinding) -> Option<String> {
    (!finding.sources.is_empty()).then(|| {
        finding
            .sources
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    })
}

/// One-line summary of the migration SQL the review read.
fn migration_text(result: &ReviewResult) -> Option<String> {
    result.migration.as_ref().map(|migration| {
        format!(
            "Migration SQL: {} file(s), {} statement(s); transactions: {}",
            migration.files.len(),
            migration.statements,
            migration.transaction_mode
        )
    })
}

fn write_markdown_finding(output: &mut String, finding: &RiskFinding) {
    let target = format_target(finding);
    let _ = writeln!(
        output,
        "- **{}** — {}",
        markdown::code(finding.rule_id.as_str()),
        markdown::code(&target)
    );
    let _ = writeln!(output, "  {}", markdown::text(&finding.message));
    if let Some(sources) = sources_text(finding) {
        let _ = writeln!(output, "  at {}", markdown::code(&sources));
    }
    if let Some(execution) = &finding.execution {
        let _ = writeln!(
            output,
            "  _execution ({}): {}_",
            execution.verdict.as_str(),
            markdown::text(&execution.detail)
        );
    }
    if let Some(mitigation) = &finding.mitigation {
        let _ = writeln!(output, "  _mitigation: {}_", markdown::text(mitigation));
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
    matches_table_pattern(patterns, name, short)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(request: ReviewRequest) -> ReviewResult {
        review(request).expect("review should succeed")
    }

    #[test]
    fn review_rejects_identity_errors_unless_allowed() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);";
        let after = "CREATE TABLE users (id INT PRIMARY KEY, id TEXT);";

        let error = review(ReviewRequest::from_sql(before, after))
            .expect_err("duplicate columns should be rejected");
        assert!(
            matches!(&error, AppError::InvalidSchema { input, errors }
                if input == "after" && errors[0].contains("duplicate column name 'id'")),
            "unexpected error: {error:?}"
        );

        let result = run(ReviewRequest {
            allow_invalid_schema: true,
            ..ReviewRequest::from_sql(before, after)
        });
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("duplicate column name 'id'"))
        );
    }

    #[test]
    fn review_keeps_dangling_foreign_keys_as_warnings() {
        let before = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
        ";
        let after = "CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));";

        let result = run(ReviewRequest::from_sql(before, after));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("FK references unknown table 'users'"))
        );
        assert!(
            result
                .review
                .findings
                .iter()
                .any(|f| f.rule_id == ReviewRuleId::DropTableReferenced)
        );
    }

    #[test]
    fn review_matches_unqualified_ddl_with_default_schema() {
        use crate::request::InputSource;

        let before = "
            CREATE TABLE public.users (id INT PRIMARY KEY, email TEXT);
            CREATE TABLE public.orders (id INT PRIMARY KEY, user_id INT REFERENCES public.users(id));
        ";
        let after = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
        ";

        let result = run(ReviewRequest {
            before: InputSource::sql_text_with_dialect(before, SqlDialect::Postgres),
            after: InputSource::sql_text_with_dialect(after, SqlDialect::Postgres),
            ..ReviewRequest::from_sql("", "")
        });
        let rule_ids: Vec<ReviewRuleId> = result
            .review
            .findings
            .iter()
            .map(|finding| finding.rule_id)
            .collect();
        assert_eq!(rule_ids, vec![ReviewRuleId::DropColumn]);
    }

    #[test]
    fn html_review_renders_the_report_only_when_asked() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY, email TEXT);";
        let after = "CREATE TABLE users (id INT PRIMARY KEY);";

        let text = run(ReviewRequest::from_sql(before, after));
        assert!(text.rendered.is_none());

        let html = run(ReviewRequest::from_sql(before, after).with_format(ReviewFormat::Html));
        let rendered = html
            .rendered
            .as_deref()
            .expect("HTML review renders a report");
        assert!(rendered.contains(r#"id="relune-review""#));
        assert!(rendered.contains("risk/drop-column"));
        assert!(rendered.contains("  email TEXT"));
        // The JSON report stays the same shape.
        assert!(!format_review_json(&html).unwrap().contains("rendered"));
    }

    fn migration_review(before: &str, after: &str, files: &[(&str, &str)]) -> ReviewResult {
        use crate::request::{InputSource, MigrationInput};
        run(ReviewRequest {
            before: InputSource::sql_text_with_dialect(before, SqlDialect::Postgres),
            after: InputSource::sql_text_with_dialect(after, SqlDialect::Postgres),
            dialect: SqlDialect::Postgres,
            migrations: files
                .iter()
                .map(|(name, sql)| MigrationInput {
                    name: (*name).to_string(),
                    sql: (*sql).to_string(),
                })
                .collect(),
            ..ReviewRequest::from_sql("", "")
        })
    }

    fn finding(result: &ReviewResult, rule: ReviewRuleId) -> &RiskFinding {
        result
            .review
            .findings
            .iter()
            .find(|finding| finding.rule_id == rule)
            .unwrap_or_else(|| panic!("expected {rule} in {:#?}", result.review.findings))
    }

    fn rule_ids(result: &ReviewResult) -> Vec<ReviewRuleId> {
        result.review.findings.iter().map(|f| f.rule_id).collect()
    }

    const ORDERS_BEFORE: &str = "
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT);
    ";
    const ORDERS_INDEXED: &str = "
        CREATE TABLE users (id BIGINT PRIMARY KEY);
        CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT);
        CREATE INDEX orders_user_id_idx ON orders (user_id);
    ";

    #[test]
    fn index_build_method_decides_the_lock_risk() {
        use relune_core::ExecutionVerdict;

        let blocking = migration_review(
            ORDERS_BEFORE,
            ORDERS_INDEXED,
            &[(
                "001.sql",
                "CREATE INDEX orders_user_id_idx ON orders (user_id);",
            )],
        );
        let index = finding(&blocking, ReviewRuleId::AddIndexOnLargeTable);
        assert_eq!(index.severity, ReviewSeverity::Caution);
        assert_eq!(
            index.execution.as_ref().unwrap().verdict,
            ExecutionVerdict::Confirmed
        );
        assert_eq!(index.sources[0].to_string(), "001.sql:1");

        let concurrent = migration_review(
            ORDERS_BEFORE,
            ORDERS_INDEXED,
            &[(
                "001.sql",
                "-- no transaction\nCREATE INDEX CONCURRENTLY orders_user_id_idx ON orders (user_id);",
            )],
        );
        let index = finding(&concurrent, ReviewRuleId::AddIndexOnLargeTable);
        assert_eq!(index.severity, ReviewSeverity::Info);
        let note = index.execution.as_ref().unwrap();
        assert_eq!(note.verdict, ExecutionVerdict::Mitigated);
        assert!(note.detail.contains("001.sql:2"), "{}", note.detail);
        assert!(
            !concurrent
                .review
                .summary
                .has_findings_at_or_above(ReviewSeverity::Caution)
        );

        let in_transaction = migration_review(
            ORDERS_BEFORE,
            ORDERS_INDEXED,
            &[(
                "001.sql",
                "BEGIN;\nCREATE INDEX CONCURRENTLY orders_user_id_idx ON orders (user_id);\nCOMMIT;",
            )],
        );
        let failure = finding(&in_transaction, ReviewRuleId::ConcurrentlyInTransaction);
        assert_eq!(failure.severity, ReviewSeverity::Breaking);
        assert_eq!(failure.table_id.as_deref(), Some("orders"));

        let per_file = run(ReviewRequest {
            migration_transaction: relune_core::TransactionMode::PerFile,
            ..ReviewRequest {
                before: crate::request::InputSource::sql_text_with_dialect(
                    ORDERS_BEFORE,
                    SqlDialect::Postgres,
                ),
                after: crate::request::InputSource::sql_text_with_dialect(
                    ORDERS_INDEXED,
                    SqlDialect::Postgres,
                ),
                migrations: vec![crate::request::MigrationInput {
                    name: "001.sql".to_string(),
                    sql: "CREATE INDEX CONCURRENTLY orders_user_id_idx ON orders (user_id);"
                        .to_string(),
                }],
                ..ReviewRequest::from_sql("", "")
            }
        });
        assert!(rule_ids(&per_file).contains(&ReviewRuleId::ConcurrentlyInTransaction));
    }

    #[test]
    fn foreign_key_validation_steps_decide_the_lock_risk() {
        use relune_core::ExecutionVerdict;

        let after = "
            CREATE TABLE users (id BIGINT PRIMARY KEY);
            CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT,
                CONSTRAINT orders_user_fk FOREIGN KEY (user_id) REFERENCES users (id));
            CREATE INDEX orders_user_id_idx ON orders (user_id);
        ";
        let before = ORDERS_INDEXED;

        let direct = migration_review(
            before,
            after,
            &[(
                "001.sql",
                "ALTER TABLE orders ADD CONSTRAINT orders_user_fk FOREIGN KEY (user_id) REFERENCES users (id);",
            )],
        );
        let fk = finding(&direct, ReviewRuleId::AddFkOnExisting);
        assert_eq!(
            fk.execution.as_ref().unwrap().verdict,
            ExecutionVerdict::Confirmed
        );
        assert_eq!(fk.severity, ReviewSeverity::Caution);

        let staged = migration_review(
            before,
            after,
            &[
                (
                    "001.sql",
                    "ALTER TABLE orders ADD CONSTRAINT orders_user_fk FOREIGN KEY (user_id) REFERENCES users (id) NOT VALID;",
                ),
                (
                    "002.sql",
                    "ALTER TABLE orders VALIDATE CONSTRAINT orders_user_fk;",
                ),
            ],
        );
        let fk = finding(&staged, ReviewRuleId::AddFkOnExisting);
        assert_eq!(
            fk.execution.as_ref().unwrap().verdict,
            ExecutionVerdict::Mitigated
        );
        assert_eq!(fk.severity, ReviewSeverity::Info);
        let sources: Vec<String> = fk.sources.iter().map(ToString::to_string).collect();
        assert_eq!(sources, ["001.sql:1", "002.sql:1"]);
        assert!(!rule_ids(&staged).contains(&ReviewRuleId::ConstraintNotValidated));

        let same_transaction = migration_review(
            before,
            after,
            &[(
                "001.sql",
                "BEGIN;
                 ALTER TABLE orders ADD CONSTRAINT orders_user_fk FOREIGN KEY (user_id) REFERENCES users (id) NOT VALID;
                 ALTER TABLE orders VALIDATE CONSTRAINT orders_user_fk;
                 COMMIT;",
            )],
        );
        let fk = finding(&same_transaction, ReviewRuleId::AddFkOnExisting);
        assert_eq!(
            fk.execution.as_ref().unwrap().verdict,
            ExecutionVerdict::Confirmed
        );
        assert!(
            fk.execution
                .as_ref()
                .unwrap()
                .detail
                .contains("same transaction")
        );

        let never_validated = migration_review(
            before,
            after,
            &[(
                "001.sql",
                "ALTER TABLE orders ADD CONSTRAINT orders_user_fk FOREIGN KEY (user_id) REFERENCES users (id) NOT VALID;",
            )],
        );
        assert!(rule_ids(&never_validated).contains(&ReviewRuleId::ConstraintNotValidated));
    }

    #[test]
    fn rename_keeps_data_where_drop_and_add_lose_it() {
        let before = "CREATE TABLE users (id BIGINT PRIMARY KEY, name TEXT);
                      CREATE INDEX users_name_idx ON users (name);";
        let after = "CREATE TABLE users (id BIGINT PRIMARY KEY, full_name TEXT);
                     CREATE INDEX users_name_idx ON users (full_name);";

        let renamed = migration_review(
            before,
            after,
            &[(
                "001.sql",
                "ALTER TABLE users RENAME COLUMN name TO full_name;",
            )],
        );
        assert_eq!(rule_ids(&renamed), [ReviewRuleId::RenameColumn]);
        let rename = finding(&renamed, ReviewRuleId::RenameColumn);
        assert_eq!(rename.column_name.as_deref(), Some("full_name"));
        assert!(
            rename
                .message
                .contains("users.name is renamed to full_name")
        );

        let rebuilt = migration_review(
            before,
            after,
            &[(
                "001.sql",
                "ALTER TABLE users DROP COLUMN name;
                 ALTER TABLE users ADD COLUMN full_name TEXT;
                 DROP INDEX users_name_idx;
                 CREATE INDEX CONCURRENTLY users_name_idx ON users (full_name);",
            )],
        );
        let drop = finding(&rebuilt, ReviewRuleId::DropColumn);
        assert_eq!(drop.severity, ReviewSeverity::Breaking);
        assert_eq!(drop.sources[0].to_string(), "001.sql:1");
        assert!(!rule_ids(&rebuilt).contains(&ReviewRuleId::RenameColumn));
    }

    #[test]
    fn set_not_null_scan_and_backfill_follow_the_statements() {
        use relune_core::ExecutionVerdict;

        let before = "CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT);";
        let after = "CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT NOT NULL);";

        let direct = migration_review(
            before,
            after,
            &[(
                "001.sql",
                "ALTER TABLE users ALTER COLUMN email SET NOT NULL;",
            )],
        );
        assert!(rule_ids(&direct).contains(&ReviewRuleId::SetNotNullScan));
        let not_null = finding(&direct, ReviewRuleId::AddNotNullOnExisting);
        assert_eq!(
            not_null.execution.as_ref().unwrap().verdict,
            ExecutionVerdict::Confirmed
        );

        let staged = migration_review(
            before,
            after,
            &[
                (
                    "001.sql",
                    "UPDATE users SET email = '' WHERE email IS NULL;
                     ALTER TABLE users ADD CONSTRAINT email_nn CHECK (email IS NOT NULL) NOT VALID;",
                ),
                (
                    "002.sql",
                    "ALTER TABLE users VALIDATE CONSTRAINT email_nn;
                     ALTER TABLE users ALTER COLUMN email SET NOT NULL;
                     ALTER TABLE users DROP CONSTRAINT email_nn;",
                ),
            ],
        );
        assert!(!rule_ids(&staged).contains(&ReviewRuleId::SetNotNullScan));
        // An UPDATE is not proof that every NULL was filled.
        let not_null = finding(&staged, ReviewRuleId::AddNotNullOnExisting);
        let note = not_null.execution.as_ref().unwrap();
        assert_eq!(note.verdict, ExecutionVerdict::Unverified);
        assert_eq!(not_null.severity, ReviewSeverity::Warning);
    }

    #[test]
    fn unreadable_migration_statements_are_reported_as_not_reviewed() {
        let result = migration_review(
            ORDERS_BEFORE,
            ORDERS_INDEXED,
            &[(
                "001.sql",
                "DO $$ BEGIN PERFORM 1; END $$;\nCREATE INDEX CONCURRENTLY orders_user_id_idx ON orders (user_id);",
            )],
        );
        let migration = result.migration.as_ref().expect("migration coverage");
        assert_eq!(migration.statements, 2);
        assert_eq!(migration.unanalyzed[0].location.to_string(), "001.sql:1");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == codes::review_migration_unanalyzed())
        );
        let text = format_review_text(&result);
        assert!(text.contains("Migration statement at 001.sql:1 was not analyzed"));
        assert!(text.contains("execution (mitigated)"));
        assert!(text.contains("at: 001.sql:2"));
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
    fn complete_inputs_report_full_coverage_without_notes() {
        let sql = "CREATE TABLE users (id INT PRIMARY KEY);";
        let result = run(ReviewRequest::from_sql(sql, sql));
        assert!(result.inputs.is_complete());
        assert!(!format_review_text(&result).contains("coverage is incomplete"));
        assert!(!format_review_markdown(&result).contains("[!WARNING]"));
    }

    #[test]
    fn unsupported_constructs_are_counted_per_input() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);";
        let after = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE SEQUENCE order_seq;
            CREATE SEQUENCE invoice_seq;
        ";
        let result = run(ReviewRequest::from_sql(before, after));
        assert_eq!(result.inputs.before, ReviewInputCoverage::default());
        assert_eq!(
            result.inputs.after,
            ReviewInputCoverage {
                empty: false,
                unsupported_constructs: 2,
            }
        );
        assert!(result.review.findings.is_empty());

        let text = format_review_text(&result);
        assert!(text.contains("Review coverage is incomplete"), "{text}");
        assert!(
            text.contains("2 unsupported SQL constructs in the after input were skipped"),
            "{text}"
        );

        let md = format_review_markdown(&result);
        assert!(md.contains("> [!WARNING]\n"), "{md}");
        assert!(
            md.contains("> - 2 unsupported SQL constructs in the after input were skipped"),
            "{md}"
        );

        let json: serde_json::Value =
            serde_json::from_str(&format_review_json(&result).expect("json")).expect("parse");
        assert_eq!(json["inputs"]["after"]["unsupported_constructs"], 2);
        assert_eq!(json["inputs"]["before"]["empty"], false);
    }

    #[test]
    fn empty_before_input_is_reported_in_quiet_output() {
        let after = "CREATE TABLE users (id INT PRIMARY KEY);";
        let result = run(ReviewRequest::from_sql("-- nothing yet", after));
        assert!(result.inputs.before.empty);
        assert!(!result.inputs.after.empty);

        let text = format_review_text_with(&result, true);
        assert!(
            text.contains("The before input produced no schema objects"),
            "{text}"
        );
        let md = format_review_markdown_with(&result, true);
        assert!(
            md.contains("> - The before input produced no schema objects"),
            "{md}"
        );
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

    #[test]
    fn format_markdown_escapes_finding_fields() {
        let before = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (id INT PRIMARY KEY);
        ";
        let after = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
        ";
        let mut result = run(ReviewRequest::from_sql(before, after));
        let finding = &mut result.review.findings[0];
        finding.table_name = Some("or`ders".to_string());
        finding.column_name = None;
        finding.fk_name = None;
        finding.message = "fine\n### Breaking\n- **fake**".to_string();
        finding.mitigation = Some("_x_ <b>".to_string());

        let md = format_review_markdown(&result);
        assert!(md.contains(" — ``or`ders``\n"), "{md}");
        assert!(
            md.contains("  fine \\#\\#\\# Breaking - \\*\\*fake\\*\\*\n"),
            "{md}"
        );
        assert!(md.contains("  _mitigation: \\_x\\_ \\<b\\>_\n"), "{md}");
    }
}
