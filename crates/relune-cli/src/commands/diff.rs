//! Diff command implementation.

use std::io::IsTerminal;

use anyhow::Context;

use super::input::DiffInputSelection;
use crate::cli::{ColorWhen, DiffArgs, DiffFormat};
use crate::config::ReluneConfig;
use crate::error::CliResult;
use crate::output::{check_diagnostics, print_success, validate_markup_stdout_usage, write_output};
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
    validate_stdout_usage(
        merged.format,
        args.out.is_some(),
        args.stdout,
        std::io::stdout().is_terminal(),
    )?;

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
                s.added_items(),
                s.removed_items(),
                s.modified_items()
            )
        } else {
            "no changes".to_string()
        };
        print_success(
            &format!(
                "Diff report written to {} ({})",
                out_path.display(),
                change_summary
            ),
            color,
        );
    }

    Ok(())
}

fn validate_stdout_usage(
    diff_format: DiffFormat,
    has_output_path: bool,
    explicit_stdout: bool,
    stdout_is_terminal: bool,
) -> CliResult<()> {
    match diff_format {
        DiffFormat::Svg | DiffFormat::Html => validate_markup_stdout_usage(
            "SVG/HTML",
            has_output_path,
            explicit_stdout,
            stdout_is_terminal,
        ),
        DiffFormat::Text | DiffFormat::Json => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_markup_stdout_on_terminal_without_opt_in() {
        let result = validate_stdout_usage(DiffFormat::Svg, false, false, true);

        let error = result.expect_err("interactive stdout should require opt-in");
        assert!(
            error.to_string().contains("Use --out <FILE> or --stdout"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn allows_markup_stdout_when_explicitly_requested() {
        validate_stdout_usage(DiffFormat::Html, false, true, true)
            .expect("explicit stdout should be allowed");
    }

    #[test]
    fn allows_text_stdout_on_terminal() {
        validate_stdout_usage(DiffFormat::Text, false, false, true)
            .expect("text stdout should stay allowed");
    }

    #[test]
    fn allows_markup_stdout_when_piped() {
        validate_stdout_usage(DiffFormat::Svg, false, false, false)
            .expect("piped stdout should stay allowed");
    }
}
