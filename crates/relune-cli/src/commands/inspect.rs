//! Inspect command implementation.

use anyhow::Context;

use super::input::InputSelection;
use crate::cli::{ColorWhen, InspectArgs, InspectFormat};
use crate::config::ReluneConfig;
use crate::error::{CliError, CliResult};
use crate::output::{DiagnosticPrinter, OutputWriter};
use relune_app::{InspectFormat as AppInspectFormat, InspectRequest, format_inspect_text, inspect};

/// Run the inspect command.
pub fn run_inspect(args: &InspectArgs, color: ColorWhen, config: &ReluneConfig) -> CliResult<()> {
    // Merge config file with CLI args
    let merged = config.merge_inspect_args(args);

    // Resolve input source
    let input = InputSelection::from_inspect(args).resolve(args.dialect.into(), "input")?;

    // Build request using merged config
    let request = InspectRequest {
        input,
        table: args.table.clone(),
        format: match merged.format {
            InspectFormat::Text => AppInspectFormat::Text,
            InspectFormat::Json => AppInspectFormat::Json,
        },
    };

    // Execute inspect
    let result = inspect(request).context("Failed to inspect schema")?;

    // Print diagnostics
    let diag_printer = DiagnosticPrinter::new(color);
    diag_printer.print_all(&result.diagnostics);

    // Check for errors
    if DiagnosticPrinter::has_errors(&result.diagnostics) {
        return Err(CliError::general(anyhow::anyhow!(
            "Errors were encountered during inspection"
        )));
    }

    // Format output using merged config
    let output = match merged.format {
        InspectFormat::Json => {
            serde_json::to_string_pretty(&result).context("Failed to serialize result to JSON")?
        }
        InspectFormat::Text => format_inspect_text(&result),
    };

    // Write output (always to stdout for inspect)
    let mut writer = OutputWriter::new(None, color).context("Failed to create output writer")?;
    writer.write(&output).context("Failed to write output")?;

    Ok(())
}
