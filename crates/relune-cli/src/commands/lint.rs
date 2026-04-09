//! Lint command implementation.

use anyhow::Context;

use super::input::InputSelection;
use crate::cli::{ColorWhen, LintArgs, LintFormat, LintSeverity};
use crate::config::ReluneConfig;
use crate::error::{CliError, CliResult};
use crate::output::{check_diagnostics_at_or_above, print_success, write_output};
use relune_app::{LintFormat as AppLintFormat, LintRequest, format_lint_text, lint};
use relune_core::Severity;

/// Run the lint command.
pub fn run_lint(
    args: &LintArgs,
    color: ColorWhen,
    quiet: bool,
    config: &ReluneConfig,
) -> CliResult<()> {
    // Merge config file with CLI args
    let merged = config.merge_lint_args(args);

    // Resolve input source
    let input = InputSelection::from_lint(args).resolve(args.dialect.into(), "input")?;

    // Convert severity from CLI to core type
    let fail_on = merged.deny.map(lint_severity_to_core);

    // Build request
    let request = LintRequest {
        input,
        format: match merged.format {
            LintFormat::Text => AppLintFormat::Text,
            LintFormat::Json => AppLintFormat::Json,
        },
        rules: args.rules.clone(),
        fail_on,
    };

    // Execute lint
    let result = lint(request).context("Failed to lint schema")?;

    let diagnostic_threshold = merged
        .deny
        .map(lint_severity_to_core)
        .or(if merged.fail_on_warning {
            Some(Severity::Warning)
        } else {
            None
        })
        .unwrap_or(Severity::Error);

    check_diagnostics_at_or_above(&result.diagnostics, color, diagnostic_threshold)?;

    // Format and write output.
    let output = match merged.format {
        LintFormat::Json => {
            serde_json::to_string_pretty(&result).context("Failed to serialize result to JSON")?
        }
        LintFormat::Text => format_lint_text(&result),
    };
    write_output(&output, args.out.as_deref(), color)?;

    // Check if we should exit with non-zero code based on --deny
    if result.has_failures(fail_on) {
        return Err(CliError::general(anyhow::anyhow!(
            "Lint issues found at or above the configured severity threshold"
        )));
    }

    if !quiet && let Some(ref out_path) = args.out {
        print_success(
            &format!(
                "Lint report written to {} ({} issue{})",
                out_path.display(),
                result.issues.len(),
                if result.issues.len() == 1 { "" } else { "s" }
            ),
            color,
        );
    }

    Ok(())
}

const fn lint_severity_to_core(severity: LintSeverity) -> Severity {
    match severity {
        LintSeverity::Error => Severity::Error,
        LintSeverity::Warning => Severity::Warning,
        LintSeverity::Info => Severity::Info,
        LintSeverity::Hint => Severity::Hint,
    }
}
