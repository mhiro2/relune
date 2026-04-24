//! Inspect command implementation.

use anyhow::Context;

use super::input::InputSelection;
use crate::cli::{ColorWhen, InspectArgs, InspectFormat};
use crate::config::ReluneConfig;
use crate::error::CliResult;
use crate::output::{check_diagnostics, print_success, write_output};
use relune_app::{InspectFormat as AppInspectFormat, InspectRequest, format_inspect_text, inspect};

/// Run the inspect command.
pub fn run_inspect(
    args: &InspectArgs,
    color: ColorWhen,
    quiet: bool,
    config: &ReluneConfig,
) -> CliResult<()> {
    // Merge config file with CLI args
    let merged = config.merge_inspect_args(args);

    // Resolve input source
    let input = InputSelection::from_inspect(args).resolve(merged.dialect.into(), "input")?;

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

    check_diagnostics(&result.diagnostics, color, merged.fail_on_warning)?;

    // Format and write output.
    let output = match merged.format {
        InspectFormat::Json => {
            serde_json::to_string_pretty(&result).context("Failed to serialize result to JSON")?
        }
        InspectFormat::Text => format_inspect_text(&result),
    };
    write_output(&output, args.out.as_deref(), color)?;

    if !quiet && let Some(ref out_path) = args.out {
        print_success(
            &format!("Inspection output written to {}", out_path.display()),
            color,
        );
    }

    Ok(())
}
