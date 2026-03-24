//! Export command implementation.

use anyhow::{Context, Result, bail};

use crate::cli::{ColorWhen, ExportArgs, ExportFormat, GroupByMode};
use crate::config::ReluneConfig;
use crate::output::{DiagnosticPrinter, OutputWriter, print_success};
use relune_app::{
    ExportFormat as AppExportFormat, ExportRequest, FilterSpec, FocusSpec, GroupingSpec,
    GroupingStrategy, InputSource, LayoutSpec, export,
};

/// Run the export command.
pub fn run_export(
    args: &ExportArgs,
    color: ColorWhen,
    quiet: bool,
    config: &ReluneConfig,
) -> Result<()> {
    // Resolve input source
    let input = resolve_input(args)?;

    // Merge config file with CLI args
    let merged = config.merge_export_args(args)?;

    // Convert merged format to app format
    let export_format = match merged.format {
        ExportFormat::SchemaJson => AppExportFormat::SchemaJson,
        ExportFormat::GraphJson => AppExportFormat::GraphJson,
        ExportFormat::LayoutJson => AppExportFormat::LayoutJson,
        ExportFormat::Mermaid => AppExportFormat::Mermaid,
        ExportFormat::D2 => AppExportFormat::D2,
        ExportFormat::Dot => AppExportFormat::Dot,
    };

    // Build filter spec (empty for now, could be extended)
    let filter = FilterSpec::default();

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

    // Build request
    let request = ExportRequest {
        input,
        format: export_format,
        filter,
        focus,
        grouping,
        layout: LayoutSpec {
            algorithm: merged.layout.into(),
            edge_style: merged.edge_style.into(),
            ..Default::default()
        },
        output_path: args.out.clone(),
    };

    // Execute export
    let result = export(request).context("Failed to export schema")?;

    // Print diagnostics
    let diag_printer = DiagnosticPrinter::new(color);
    diag_printer.print_all(&result.diagnostics);

    // Check for errors
    if DiagnosticPrinter::has_errors(&result.diagnostics) {
        bail!("Errors were encountered during export");
    }

    // Write output
    let mut writer =
        OutputWriter::new(args.out.as_deref(), color).context("Failed to create output writer")?;
    writer
        .write(&result.content)
        .context("Failed to write output")?;

    // Print success message (unless quiet)
    if !quiet && let Some(ref out_path) = args.out {
        print_success(&format!("Exported {}", out_path.display()), color);
    }

    Ok(())
}

/// Resolve input source from CLI arguments.
fn resolve_input(args: &ExportArgs) -> Result<InputSource> {
    let count = args.sql.iter().count()
        + args.sql_text.iter().count()
        + args.schema_json.iter().count()
        + args.db_url.iter().count();

    if count == 0 {
        bail!(
            "At least one input option is required: --sql, --sql-text, --schema-json, or --db-url"
        );
    }

    if count > 1 {
        bail!(
            "Only one input option can be specified: --sql, --sql-text, --schema-json, or --db-url"
        );
    }

    let dialect = args.dialect.into();

    if let Some(ref path) = args.sql {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read SQL file: {}", path.display()))?;
        return Ok(InputSource::sql_text_with_dialect(content, dialect));
    }

    if let Some(ref text) = args.sql_text {
        return Ok(InputSource::sql_text_with_dialect(text.clone(), dialect));
    }

    if let Some(ref path) = args.schema_json {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read schema JSON file: {}", path.display()))?;
        return Ok(InputSource::schema_json(content));
    }

    if let Some(ref url) = args.db_url {
        return Ok(InputSource::db_url(url.clone()));
    }

    unreachable!()
}
