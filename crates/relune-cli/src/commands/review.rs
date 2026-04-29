//! Review command implementation.

use super::input::DiffInputSelection;
use crate::cli::{ColorWhen, ReviewArgs, ReviewFormat, ReviewSeverityArg};
use crate::config::ReluneConfig;
use crate::error::{CliError, CliResult};
use crate::output::{check_diagnostics, print_success, write_output};
use relune_app::{
    ReviewFormat as AppReviewFormat, ReviewRequest, format_review_json,
    format_review_markdown_with, format_review_text_with, review,
};
use relune_core::ReviewSeverity;

/// Run the review command.
pub fn run_review(
    args: &ReviewArgs,
    color: ColorWhen,
    quiet: bool,
    config: &ReluneConfig,
) -> CliResult<()> {
    let merged = config.merge_review_args(args)?;
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
    };

    let result = review(request)
        .map_err(|error| CliError::general(anyhow::anyhow!("Failed to review schema: {error}")))?;

    check_diagnostics(&result.diagnostics, color, false)?;

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

const fn review_severity_to_core(severity: ReviewSeverityArg) -> ReviewSeverity {
    match severity {
        ReviewSeverityArg::Info => ReviewSeverity::Info,
        ReviewSeverityArg::Warning => ReviewSeverity::Warning,
        ReviewSeverityArg::Caution => ReviewSeverity::Caution,
        ReviewSeverityArg::Breaking => ReviewSeverity::Breaking,
    }
}
