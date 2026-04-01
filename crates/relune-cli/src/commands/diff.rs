//! Diff command implementation.

use anyhow::Context;

use super::input::DiffInputSelection;
use crate::cli::{ColorWhen, DiffArgs, DiffFormat};
use crate::config::ReluneConfig;
use crate::error::CliResult;
use crate::output::{check_diagnostics, print_success, write_output};
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
            DiffFormat::Svg => relune_app::DiffFormat::Svg,
            DiffFormat::Html => relune_app::DiffFormat::Html,
        },
        output_path: args.out.clone(),
        ..Default::default()
    };

    // Execute diff
    let mut result = diff(request).context("Failed to compute schema diff")?;

    check_diagnostics(&result.diagnostics, color, false)?;

    // Format output
    let rendered = result.rendered.take();
    let content = match merged.format {
        DiffFormat::Text => format_diff_text(&result),
        DiffFormat::Json => serde_json::to_string_pretty(&result.diff)
            .context("Failed to serialize diff to JSON")?,
        DiffFormat::Svg | DiffFormat::Html => rendered.unwrap_or_default(),
    };
    write_output(&content, args.out.as_deref(), color)?;

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
