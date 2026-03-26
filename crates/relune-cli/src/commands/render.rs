//! Render command implementation.

use std::io::IsTerminal;

use anyhow::Context;

use super::input::InputSelection;
use crate::cli::{ColorWhen, GroupByMode, RenderArgs, RenderFormat, Theme};
use crate::config::ReluneConfig;
use crate::error::{CliError, CliResult};
use crate::output::{DiagnosticPrinter, OutputWriter, print_stats, print_success};
use relune_app::{
    FilterSpec, FocusSpec, GroupingSpec, GroupingStrategy, LayoutSpec, OutputFormat, RenderOptions,
    RenderRequest, RenderTheme, render,
};

/// Run the render command.
pub fn run_render(
    args: &RenderArgs,
    color: ColorWhen,
    quiet: bool,
    config: &ReluneConfig,
) -> CliResult<()> {
    // Resolve input source
    let input = InputSelection::from_render(args).resolve(args.dialect.into(), "input")?;

    // Merge config file with CLI args
    let merged = config.merge_render_args(args);
    merged.validate_semantics()?;

    // Convert merged format to app format
    let output_format = match merged.format {
        RenderFormat::Svg => OutputFormat::Svg,
        RenderFormat::Html => OutputFormat::Html,
        RenderFormat::GraphJson => OutputFormat::GraphJson,
        RenderFormat::SchemaJson => OutputFormat::SchemaJson,
    };
    validate_stdout_usage(
        output_format,
        args.out.is_some(),
        args.stdout,
        std::io::stdout().is_terminal(),
    )?;

    // Build filter spec from merged config
    let filter = FilterSpec {
        include: merged.include,
        exclude: merged.exclude,
    };

    // Build focus spec from merged config
    let focus = merged.focus.as_ref().map(|table| FocusSpec {
        table: table.clone(),
        depth: merged.depth,
    });

    // Build grouping spec from merged config
    let grouping = GroupingSpec {
        strategy: match merged.group_by.unwrap_or(GroupByMode::None) {
            GroupByMode::None => GroupingStrategy::None,
            GroupByMode::Schema => GroupingStrategy::BySchema,
            GroupByMode::Prefix => GroupingStrategy::ByPrefix,
        },
    };

    let layout = LayoutSpec {
        algorithm: merged.layout.into(),
        edge_style: merged.edge_style.into(),
        ..Default::default()
    };

    // Build request
    let request = RenderRequest {
        input,
        output_format,
        filter,
        focus,
        grouping,
        layout,
        options: RenderOptions {
            theme: match merged.theme {
                Theme::Light => RenderTheme::Light,
                Theme::Dark => RenderTheme::Dark,
            },
            show_legend: merged.show_legend,
            show_stats: merged.show_stats,
        },
        ..Default::default()
    };

    // Execute render
    let result = render(request).context("Failed to render schema")?;

    // Print diagnostics
    let diag_printer = DiagnosticPrinter::new(color);
    diag_printer.print_all(&result.diagnostics);

    // Check for fail-on-warning
    if args.fail_on_warning && DiagnosticPrinter::has_warnings(&result.diagnostics) {
        return Err(CliError::warning(anyhow::anyhow!(
            "Warnings were emitted and --fail-on-warning is set"
        )));
    }

    // Check for errors
    if DiagnosticPrinter::has_errors(&result.diagnostics) {
        return Err(CliError::general(anyhow::anyhow!(
            "Errors were encountered during rendering"
        )));
    }

    // Write output
    let mut writer =
        OutputWriter::new(args.out.as_deref(), color).context("Failed to create output writer")?;
    writer
        .write(&result.content)
        .context("Failed to write output")?;
    writer.finish().context("Failed to finalize output")?;

    // Print stats if requested (from merged config)
    if merged.show_stats {
        print_stats(&result.stats);
    }

    // Print success message (unless quiet)
    if !quiet && let Some(ref out_path) = args.out {
        print_success(
            &format!(
                "Generated {} ({} tables, {} columns)",
                out_path.display(),
                result.stats.table_count,
                result.stats.column_count
            ),
            color,
        );
    }

    Ok(())
}

fn validate_stdout_usage(
    output_format: OutputFormat,
    has_output_path: bool,
    explicit_stdout: bool,
    stdout_is_terminal: bool,
) -> CliResult<()> {
    let renders_markup = matches!(output_format, OutputFormat::Svg | OutputFormat::Html);

    if renders_markup && !has_output_path && !explicit_stdout && stdout_is_terminal {
        return Err(CliError::usage(anyhow::anyhow!(
            "Refusing to write raw SVG/HTML to an interactive terminal. Use --out <FILE> or --stdout."
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_markup_stdout_on_terminal_without_opt_in() {
        let result = validate_stdout_usage(OutputFormat::Svg, false, false, true);

        let error = result.expect_err("interactive stdout should require opt-in");
        assert!(
            error.to_string().contains("Use --out <FILE> or --stdout"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn allows_markup_stdout_when_explicitly_requested() {
        validate_stdout_usage(OutputFormat::Html, false, true, true)
            .expect("explicit stdout should be allowed");
    }

    #[test]
    fn allows_json_stdout_on_terminal() {
        validate_stdout_usage(OutputFormat::GraphJson, false, false, true)
            .expect("json stdout should stay allowed");
    }

    #[test]
    fn allows_markup_stdout_when_piped() {
        validate_stdout_usage(OutputFormat::Svg, false, false, false)
            .expect("piped stdout should stay allowed");
    }
}
