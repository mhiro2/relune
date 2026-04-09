//! Export command implementation.

use anyhow::Context;

use super::input::InputSelection;
use crate::cli::{ColorWhen, ExportArgs, ExportFormat, GroupByMode};
use crate::config::ReluneConfig;
use crate::error::CliResult;
use crate::output::{check_diagnostics, print_success, write_output};
use relune_app::{
    ExportFormat as AppExportFormat, ExportRequest, FilterSpec, FocusSpec, GroupingSpec,
    GroupingStrategy, LayoutSpec, export,
};

/// Run the export command.
pub fn run_export(
    args: &ExportArgs,
    color: ColorWhen,
    quiet: bool,
    config: &ReluneConfig,
) -> CliResult<()> {
    // Resolve input source
    let input = InputSelection::from_export(args).resolve(args.dialect.into(), "input")?;

    // Merge config file with CLI args
    let merged = config.merge_export_args(args)?;
    merged.validate_semantics()?;

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
            direction: merged.direction.into(),
            ..Default::default()
        },
        output_path: args.out.clone(),
    };

    // Execute export
    let result = export(request).context("Failed to export schema")?;

    check_diagnostics(&result.diagnostics, color, merged.fail_on_warning)?;
    write_output(&result.content, args.out.as_deref(), color)?;

    // Print success message (unless quiet)
    if !quiet && let Some(ref out_path) = args.out {
        print_success(&format!("Exported {}", out_path.display()), color);
    }

    Ok(())
}
