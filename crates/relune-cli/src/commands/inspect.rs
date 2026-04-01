//! Inspect command implementation.

use anyhow::Context;

use super::input::InputSelection;
use crate::cli::{ColorWhen, InspectArgs, InspectFormat};
use crate::config::ReluneConfig;
use crate::error::CliResult;
use crate::output::{check_diagnostics, write_output};
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

    check_diagnostics(&result.diagnostics, color, false)?;

    // Format and write output (always to stdout for inspect)
    let output = match merged.format {
        InspectFormat::Json => {
            serde_json::to_string_pretty(&result).context("Failed to serialize result to JSON")?
        }
        InspectFormat::Text => format_inspect_text(&result),
    };
    write_output(&output, None, color)?;

    Ok(())
}
