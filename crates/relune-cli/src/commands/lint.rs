//! Lint command implementation.

use anyhow::Context;

use super::input::InputSelection;
use crate::cli::{ColorWhen, LintArgs, LintFormat, LintSeverity};
use crate::config::ReluneConfig;
use crate::error::{CliError, CliResult};
use crate::output::{DiagnosticPrinter, OutputWriter};
use relune_app::{LintFormat as AppLintFormat, LintRequest, format_lint_text, lint};
use relune_core::Severity;

/// Run the lint command.
pub fn run_lint(args: &LintArgs, color: ColorWhen, config: &ReluneConfig) -> CliResult<()> {
    // Merge config file with CLI args
    let merged = config.merge_lint_args(args);

    // Resolve input source
    let input = InputSelection::from_lint(args).resolve(args.dialect.into(), "input")?;

    // Convert severity from CLI to core type
    let fail_on = merged.deny.map(|s| match s {
        LintSeverity::Error => Severity::Error,
        LintSeverity::Warning => Severity::Warning,
        LintSeverity::Info => Severity::Info,
        LintSeverity::Hint => Severity::Hint,
    });

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

    // Print diagnostics
    let diag_printer = DiagnosticPrinter::new(color);
    diag_printer.print_all(&result.diagnostics);

    // Check for errors
    if DiagnosticPrinter::has_errors(&result.diagnostics) {
        return Err(CliError::general(anyhow::anyhow!(
            "Errors were encountered during linting"
        )));
    }

    // Format output
    let output = match merged.format {
        LintFormat::Json => {
            serde_json::to_string_pretty(&result).context("Failed to serialize result to JSON")?
        }
        LintFormat::Text => format_lint_text(&result),
    };

    // Write output (always to stdout for lint)
    let mut writer = OutputWriter::new(None, color).context("Failed to create output writer")?;
    writer.write(&output).context("Failed to write output")?;

    // Check if we should exit with non-zero code based on --deny
    if result.has_failures(fail_on) {
        return Err(CliError::general(anyhow::anyhow!(
            "Lint issues found at or above the configured severity threshold"
        )));
    }

    Ok(())
}
