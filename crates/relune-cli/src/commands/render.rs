//! Render command implementation.

use std::io::IsTerminal;

use anyhow::Context;

use super::input::InputSelection;
use crate::cli::{ColorWhen, GroupByMode, RenderArgs, RenderFormat, Theme};
use crate::config::ReluneConfig;
use crate::error::{CliError, CliResult};
use crate::output::{check_diagnostics, print_stats, print_success, write_output};
use crate::png;
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

    // Convert merged format to app format.
    // PNG is rendered as SVG internally, then rasterized after the render step.
    let is_png = merged.format == RenderFormat::Png;
    let output_format = match merged.format {
        RenderFormat::Svg | RenderFormat::Png => OutputFormat::Svg,
        RenderFormat::Html => OutputFormat::Html,
        RenderFormat::GraphJson => OutputFormat::GraphJson,
        RenderFormat::SchemaJson => OutputFormat::SchemaJson,
    };
    validate_stdout_usage(
        merged.format,
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
        direction: merged.direction.into(),
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

    check_diagnostics(&result.diagnostics, color, args.fail_on_warning)?;

    // Write output (PNG needs byte-level writer for rasterized data)
    if is_png {
        let png_data =
            png::svg_to_png(&result.content).context("Failed to rasterize SVG to PNG")?;
        let mut writer = crate::output::OutputWriter::new(args.out.as_deref(), color)
            .context("Failed to create output writer")?;
        writer
            .write_bytes(&png_data)
            .context("Failed to write PNG output")?;
        writer.finish().context("Failed to finalize output")?;
    } else {
        write_output(&result.content, args.out.as_deref(), color)?;
    }

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
    render_format: RenderFormat,
    has_output_path: bool,
    explicit_stdout: bool,
    stdout_is_terminal: bool,
) -> CliResult<()> {
    if !has_output_path && !explicit_stdout && stdout_is_terminal {
        match render_format {
            RenderFormat::Svg | RenderFormat::Html => {
                return Err(CliError::usage(anyhow::anyhow!(
                    "Refusing to write raw SVG/HTML to an interactive terminal. Use --out <FILE> or --stdout."
                )));
            }
            RenderFormat::Png => {
                return Err(CliError::usage(anyhow::anyhow!(
                    "Refusing to write binary PNG data to an interactive terminal. Use --out <FILE>."
                )));
            }
            RenderFormat::GraphJson | RenderFormat::SchemaJson => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_markup_stdout_on_terminal_without_opt_in() {
        let result = validate_stdout_usage(RenderFormat::Svg, false, false, true);

        let error = result.expect_err("interactive stdout should require opt-in");
        assert!(
            error.to_string().contains("Use --out <FILE> or --stdout"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_png_stdout_on_terminal() {
        let result = validate_stdout_usage(RenderFormat::Png, false, false, true);

        let error = result.expect_err("PNG stdout on terminal should be rejected");
        assert!(
            error.to_string().contains("binary PNG data"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_png_stdout_even_with_explicit_stdout_flag() {
        // PNG cannot use --stdout because it conflicts with --out in clap,
        // but if somehow both are false, it should still reject on terminal.
        let result = validate_stdout_usage(RenderFormat::Png, false, false, true);
        assert!(result.is_err());
    }

    #[test]
    fn allows_markup_stdout_when_explicitly_requested() {
        validate_stdout_usage(RenderFormat::Html, false, true, true)
            .expect("explicit stdout should be allowed");
    }

    #[test]
    fn allows_json_stdout_on_terminal() {
        validate_stdout_usage(RenderFormat::GraphJson, false, false, true)
            .expect("json stdout should stay allowed");
    }

    #[test]
    fn allows_markup_stdout_when_piped() {
        validate_stdout_usage(RenderFormat::Svg, false, false, false)
            .expect("piped stdout should stay allowed");
    }

    #[test]
    fn allows_png_stdout_when_piped() {
        validate_stdout_usage(RenderFormat::Png, false, false, false)
            .expect("piped PNG stdout should be allowed");
    }
}
