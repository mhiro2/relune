//! Render command implementation.

use anyhow::Context;

use crate::cli::{ColorWhen, GroupByMode, RenderArgs, RenderFormat, Theme};
use crate::config::ReluneConfig;
use crate::error::{CliError, CliResult};
use crate::output::{DiagnosticPrinter, OutputWriter, print_stats, print_success};
use relune_app::{
    FilterSpec, FocusSpec, GroupingSpec, GroupingStrategy, InputSource, LayoutSpec, OutputFormat,
    RenderOptions, RenderRequest, RenderTheme, render,
};

/// Run the render command.
pub fn run_render(
    args: &RenderArgs,
    color: ColorWhen,
    quiet: bool,
    config: &ReluneConfig,
) -> CliResult<()> {
    // Resolve input source
    let input = resolve_input(args)?;

    // Merge config file with CLI args
    let merged = config.merge_render_args(args);

    // Convert merged format to app format
    let output_format = match merged.format {
        RenderFormat::Svg => OutputFormat::Svg,
        RenderFormat::Html => OutputFormat::Html,
        RenderFormat::GraphJson => OutputFormat::GraphJson,
        RenderFormat::SchemaJson => OutputFormat::SchemaJson,
    };

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

/// Resolve input source from CLI arguments.
fn resolve_input(args: &RenderArgs) -> CliResult<InputSource> {
    let count = args.sql.iter().count()
        + args.sql_text.iter().count()
        + args.schema_json.iter().count()
        + args.db_url.iter().count();

    if count == 0 {
        return Err(CliError::usage(anyhow::anyhow!(
            "At least one input option is required: --sql, --sql-text, --schema-json, or --db-url"
        )));
    }

    if count > 1 {
        return Err(CliError::usage(anyhow::anyhow!(
            "Only one input option can be specified: --sql, --sql-text, --schema-json, or --db-url"
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

    if let Some(ref text) = args.sql_text {
        return Ok(InputSource::sql_text_with_dialect(text.clone(), dialect));
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

    if let Some(ref url) = args.db_url {
        return Ok(InputSource::db_url(url.clone()));
    }

    unreachable!()
}
