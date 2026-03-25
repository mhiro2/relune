//! Lint command implementation.

use anyhow::Context;

use crate::cli::{ColorWhen, LintArgs, LintFormat, LintSeverity};
use crate::config::ReluneConfig;
use crate::error::{CliError, CliResult};
use crate::output::{DiagnosticPrinter, OutputWriter};
use relune_app::{InputSource, LintFormat as AppLintFormat, LintRequest, format_lint_text, lint};
use relune_core::Severity;

/// Run the lint command.
pub fn run_lint(args: &LintArgs, color: ColorWhen, config: &ReluneConfig) -> CliResult<()> {
    // Merge config file with CLI args
    let merged = config.merge_lint_args(args);

    // Resolve input source
    let input = resolve_input(args)?;

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

/// Resolve input source from CLI arguments.
fn resolve_input(args: &LintArgs) -> CliResult<InputSource> {
    let count =
        args.sql.iter().count() + args.db_url.iter().count() + args.schema_json.iter().count();

    if count == 0 {
        return Err(CliError::usage(anyhow::anyhow!(
            "At least one input option is required: --sql, --db-url, or --schema-json"
        )));
    }

    if count > 1 {
        return Err(CliError::usage(anyhow::anyhow!(
            "Only one input option can be specified: --sql, --db-url, or --schema-json"
        )));
    }

    let dialect = args.dialect.into();

    if let Some(ref path) = args.sql {
        let content = std::fs::read_to_string(path).map_err(|e| {
            CliError::usage(anyhow::anyhow!(
                "Failed to read SQL file: {}: {}",
                path.display(),
                e
            ))
        })?;
        return Ok(InputSource::sql_text_with_dialect(content, dialect));
    }

    if let Some(ref url) = args.db_url {
        return Ok(InputSource::db_url(url.clone()));
    }

    if let Some(ref path) = args.schema_json {
        let content = std::fs::read_to_string(path).map_err(|e| {
            CliError::usage(anyhow::anyhow!(
                "Failed to read schema JSON file: {}: {}",
                path.display(),
                e
            ))
        })?;
        return Ok(InputSource::schema_json(content));
    }

    unreachable!()
}
