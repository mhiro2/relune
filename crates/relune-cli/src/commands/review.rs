//! Review command implementation.

use std::path::{Path, PathBuf};

use super::input::DiffInputSelection;
use crate::cli::{ColorWhen, ReviewArgs, ReviewFormat, ReviewSeverityArg};
use crate::config::ReluneConfig;
use crate::error::{CliError, CliResult};
use crate::output::{check_diagnostics, print_success, write_output};
use relune_app::{
    ReviewFormat as AppReviewFormat, ReviewRequest, ReviewResult, format_review_json,
    format_review_markdown_with, format_review_text_with, review,
};
use relune_core::{ReviewRuleId, ReviewRuleMetadata, ReviewSeverity};

/// Run the review command.
pub fn run_review(
    args: &ReviewArgs,
    color: ColorWhen,
    quiet: bool,
    config: &ReluneConfig,
) -> CliResult<()> {
    let merged = config.merge_review_args(args)?;

    if args.list_rules {
        return run_list_rules(args, merged.format, color);
    }

    if let Some(emit_path) = args.emit_summary.as_deref()
        && let Some(out_path) = args.out.as_deref()
        && paths_are_equal(emit_path, out_path)
    {
        return Err(CliError::usage(anyhow::anyhow!(
            "Cannot reuse --out path as --emit-summary"
        )));
    }

    let dialect = merged.dialect.into();
    let before = DiffInputSelection::from_review_before(args).resolve(dialect, "before")?;
    let after = DiffInputSelection::from_review_after(args).resolve(dialect, "after")?;

    let deny = merged.deny.map(review_severity_to_core);

    let request = ReviewRequest {
        before,
        after,
        format: match merged.format {
            ReviewFormat::Text => AppReviewFormat::Text,
            ReviewFormat::Markdown => AppReviewFormat::Markdown,
            ReviewFormat::Json => AppReviewFormat::Json,
        },
        output_path: args.out.clone(),
        rules: merged.rules,
        except_rules: merged.except_rules,
        except_tables: merged.except_tables,
        deny,
        severity_overrides: merged.severity_overrides,
        dialect,
    };

    let result = review(request)
        .map_err(|error| CliError::general(anyhow::anyhow!("Failed to review schema: {error}")))?;

    check_diagnostics(&result.diagnostics, color, false)?;

    // Emit the structured summary file before any deny short-circuit so CI
    // can rely on `--emit-summary` being written even when rc=10.
    if let Some(emit_path) = args.emit_summary.as_deref() {
        write_emit_summary(&result, emit_path, color)?;
    }

    let content = match merged.format {
        ReviewFormat::Text => format_review_text_with(&result, quiet),
        ReviewFormat::Markdown => format_review_markdown_with(&result, quiet),
        ReviewFormat::Json => format_review_json(&result).map_err(|error| {
            CliError::general(anyhow::anyhow!(
                "Failed to serialize review result to JSON: {error}"
            ))
        })?,
    };
    write_output(&content, args.out.as_deref(), color)?;

    if !quiet && let Some(ref out_path) = args.out {
        let s = &result.review.summary;
        print_success(
            &format!(
                "Review report written to {} ({} breaking, {} caution, {} warning, {} info)",
                out_path.display(),
                s.breaking,
                s.caution,
                s.warning,
                s.info,
            ),
            color,
        );
    }

    if result.denied {
        return Err(CliError::review_denied(anyhow::anyhow!(
            "Review findings reached the configured --deny threshold"
        )));
    }

    if args.exit_code && !result.review.findings.is_empty() {
        return Err(CliError::DiffChangesDetected);
    }

    Ok(())
}

fn run_list_rules(args: &ReviewArgs, format: ReviewFormat, color: ColorWhen) -> CliResult<()> {
    let metadata = ReviewRuleId::all_metadata();
    let content = match format {
        ReviewFormat::Text => format_rule_list_text(&metadata),
        ReviewFormat::Json => format_rule_list_json(&metadata)?,
        ReviewFormat::Markdown => {
            return Err(CliError::usage(anyhow::anyhow!(
                "markdown is not a supported format for --list-rules"
            )));
        }
    };
    write_output(&content, args.out.as_deref(), color)
}

fn format_rule_list_text(metadata: &[ReviewRuleMetadata]) -> String {
    let id_width = metadata
        .iter()
        .map(|entry| entry.rule_id.as_str().len())
        .max()
        .unwrap_or(0);
    let severity_width = metadata
        .iter()
        .map(|entry| entry.default_severity.as_str().len())
        .max()
        .unwrap_or(0);

    let mut buffer = String::new();
    for entry in metadata {
        use std::fmt::Write as _;
        let _ = writeln!(
            &mut buffer,
            "{:<id_width$}  {:<severity_width$}  {}",
            entry.rule_id.as_str(),
            entry.default_severity.as_str(),
            entry.description,
        );
    }
    buffer
}

fn format_rule_list_json(metadata: &[ReviewRuleMetadata]) -> CliResult<String> {
    let mut json = serde_json::to_string_pretty(metadata).map_err(|error| {
        CliError::general(anyhow::anyhow!(
            "Failed to serialize review rule metadata to JSON: {error}"
        ))
    })?;
    json.push('\n');
    Ok(json)
}

fn write_emit_summary(result: &ReviewResult, path: &Path, color: ColorWhen) -> CliResult<()> {
    let json = format_review_json(result).map_err(|error| {
        CliError::general(anyhow::anyhow!(
            "Failed to serialize review summary to JSON: {error}"
        ))
    })?;
    write_output(&json, Some(path), color)
}

fn paths_are_equal(left: &Path, right: &Path) -> bool {
    if let (Ok(a), Ok(b)) = (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        return a == b;
    }
    // Either path may not exist yet (we are about to create it). Fall back to
    // canonicalizing the parent directory and comparing it together with the
    // file name so that `./out.json` vs `$PWD/out.json` are treated as equal.
    if let (Some(a), Some(b)) = (resolve_target_path(left), resolve_target_path(right)) {
        return a == b;
    }
    left == right
}

fn resolve_target_path(path: &Path) -> Option<PathBuf> {
    let file_name = path.file_name()?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canonical_parent = std::fs::canonicalize(parent).ok()?;
    Some(canonical_parent.join(file_name))
}

const fn review_severity_to_core(severity: ReviewSeverityArg) -> ReviewSeverity {
    match severity {
        ReviewSeverityArg::Info => ReviewSeverity::Info,
        ReviewSeverityArg::Warning => ReviewSeverity::Warning,
        ReviewSeverityArg::Caution => ReviewSeverity::Caution,
        ReviewSeverityArg::Breaking => ReviewSeverity::Breaking,
    }
}
