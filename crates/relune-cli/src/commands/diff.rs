//! Diff command implementation.

use anyhow::Context;

use super::input::DiffInputSelection;
use crate::cli::{ColorWhen, DiffArgs, DiffFormat};
use crate::config::ReluneConfig;
use crate::error::{CliError, CliResult};
use crate::output::{DiagnosticPrinter, OutputWriter, print_success};
use relune_app::{DiffRequest, diff, format_diff_text};

/// Run the diff command.
pub fn run_diff(
    args: &DiffArgs,
    color: ColorWhen,
    quiet: bool,
    config: &ReluneConfig,
) -> CliResult<()> {
    let merged = config.merge_diff_args(args);
    let dialect = merged.dialect.into();

    // Resolve before input source
    let before = DiffInputSelection::from_before(args).resolve(dialect, "before")?;

    // Resolve after input source
    let after = DiffInputSelection::from_after(args).resolve(dialect, "after")?;

    // Build request
    let request = DiffRequest {
        before,
        after,
        format: match merged.format {
            DiffFormat::Text => relune_app::DiffFormat::Text,
            DiffFormat::Json => relune_app::DiffFormat::Json,
        },
        output_path: args.out.clone(),
    };

    // Execute diff
    let result = diff(request).context("Failed to compute schema diff")?;

    // Print diagnostics
    let diag_printer = DiagnosticPrinter::new(color);
    diag_printer.print_all(&result.diagnostics);

    // Check for errors
    if DiagnosticPrinter::has_errors(&result.diagnostics) {
        return Err(CliError::general(anyhow::anyhow!(
            "Errors were encountered during diff computation"
        )));
    }

    // Format output
    let content = match merged.format {
        DiffFormat::Text => format_diff_text(&result),
        DiffFormat::Json => serde_json::to_string_pretty(&result.diff)
            .context("Failed to serialize diff to JSON")?,
    };

    // Write output
    let mut writer =
        OutputWriter::new(args.out.as_deref(), color).context("Failed to create output writer")?;
    writer.write(&content).context("Failed to write output")?;
    writer.finish().context("Failed to finalize output")?;

    // Print success message (unless quiet)
    if !quiet && let Some(ref out_path) = args.out {
        let change_summary = if result.has_changes() {
            let s = &result.diff.summary;
            format!(
                "{} added, {} removed, {} modified",
                s.tables_added, s.tables_removed, s.tables_modified
            )
        } else {
            "no changes".to_string()
        };
        print_success(
            &format!(
                "Diff output written to {} ({})",
                out_path.display(),
                change_summary
            ),
            color,
        );
    }

    Ok(())
}
