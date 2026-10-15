//! Diff use case implementation.

use std::collections::HashMap;
use std::fmt::Write;

use relune_core::diff::{EnumDiff, TableDiff, ViewDiff};
use relune_core::{
    ChangeKind, Enum as SchemaEnum, ReviewRuleId, RiskFinding, Schema, SqlDialect, Table, View,
    diff_schemas,
};
use relune_layout::metrics::TYPE_CHANGE_SEPARATOR;
use relune_layout::{Annotation, ColumnChange, DiagramOverlay, NodeChange};

use crate::error::AppError;
use crate::markdown;
use crate::request::DiffRequest;
use crate::result::DiffResult;
use crate::schema_input::{SchemaValidation, align_input_schemas, schema_from_input_checked};
use crate::usecases::review::resolve_effective_dialect;

/// Execute a diff request.
#[allow(clippy::needless_pass_by_value)]
pub fn diff(request: DiffRequest) -> Result<DiffResult, AppError> {
    use crate::request::DiffFormat;

    // Step 1: Resolve schemas
    let (mut before_schema, mut diagnostics, before_context) = schema_from_input_checked(
        &request.before,
        SchemaValidation::for_comparison("before", request.allow_invalid_schema),
    )?;
    let (mut after_schema, after_diagnostics, after_context) = schema_from_input_checked(
        &request.after,
        SchemaValidation::for_comparison("after", request.allow_invalid_schema),
    )?;
    diagnostics.extend(after_diagnostics);
    align_input_schemas(
        &mut before_schema,
        before_context,
        &mut after_schema,
        after_context,
    );

    // Step 2: Compute diff
    let schema_diff = diff_schemas(&before_schema, &after_schema);

    // Step 3: Render visual output if requested, ranking each change's risk
    // with the review rules for the dialect both inputs resolved to.
    let rendered = match request.format {
        DiffFormat::Svg | DiffFormat::Html => {
            let (dialect, _) = resolve_effective_dialect(
                SqlDialect::Auto,
                before_context.resolved_dialect,
                after_context.resolved_dialect,
            );
            let findings = relune_core::run_rules(
                &schema_diff,
                &before_schema,
                &after_schema,
                ReviewRuleId::all_rules(),
                dialect,
            );
            let content = render_diff_visual(
                &before_schema,
                &after_schema,
                &schema_diff,
                &findings,
                &request,
            )?;
            Some(content)
        }
        DiffFormat::Text | DiffFormat::Json | DiffFormat::Markdown => None,
    };

    Ok(DiffResult {
        diff: schema_diff,
        diagnostics,
        rendered,
    })
}

/// Render a diff as SVG or HTML using the merged schema and diff overlay.
fn render_diff_visual(
    before: &Schema,
    after: &Schema,
    schema_diff: &relune_core::SchemaDiff,
    findings: &[RiskFinding],
    request: &DiffRequest,
) -> Result<String, AppError> {
    use crate::request::{DiffFormat, OutputFormat, RenderRequest};

    let merged = build_diff_schema(before, after, schema_diff);
    let overlay = build_diff_overlay(before, after, schema_diff, findings);

    let output_format = match request.format {
        DiffFormat::Svg => OutputFormat::Svg,
        DiffFormat::Html => OutputFormat::Html,
        _ => unreachable!(),
    };

    let render_request = RenderRequest {
        input: crate::request::InputSource::default(),
        output_format,
        filter: request.filter.clone(),
        focus: request.focus.clone(),
        grouping: request.grouping,
        layout: request.layout.clone(),
        options: request.options,
        output_path: None,
        overlay: Some(overlay),
    };

    // Use the render pipeline directly, but with the merged schema
    render_with_schema(&merged, &render_request)
}

/// Render pipeline that takes a pre-parsed schema instead of an input source.
fn render_with_schema(
    schema: &Schema,
    request: &crate::request::RenderRequest,
) -> Result<String, AppError> {
    use relune_layout::{LayoutConfig, build_layout_from_graph_with_config};
    use relune_render_html::{HtmlRenderOptions, Theme as HtmlTheme};
    use relune_render_svg::{SvgRenderOptions, Theme as SvgTheme, render_svg_with_overlay};

    use crate::request::{OutputFormat, RenderTheme};

    let layout_config = LayoutConfig::from(&request.layout);
    // The diff result reports input diagnostics only.
    let (graph, _) = crate::usecases::render::build_graph(request, schema)?;
    let positioned = build_layout_from_graph_with_config(&graph, &layout_config)?;

    let svg_theme = match request.options.theme {
        RenderTheme::Light => SvgTheme::Light,
        RenderTheme::Dark => SvgTheme::Dark,
    };
    let svg_options = SvgRenderOptions {
        theme: svg_theme,
        show_legend: request.options.show_legend,
        show_stats: request.options.show_stats,
        embed_css: true,
        show_tooltips: true,
    };
    let svg = render_svg_with_overlay(&positioned, svg_options, request.overlay.as_ref())?;

    match request.output_format {
        OutputFormat::Svg => Ok(svg.into_string()),
        OutputFormat::Html => {
            let html_theme = match request.options.theme {
                RenderTheme::Light => HtmlTheme::Light,
                RenderTheme::Dark => HtmlTheme::Dark,
            };
            let html_options = HtmlRenderOptions {
                theme: html_theme,
                ..Default::default()
            };
            let html = relune_render_html::render_html_with_overlay(
                &graph,
                &svg,
                &html_options,
                request.overlay.as_ref(),
            )?;
            Ok(html)
        }
        _ => unreachable!(),
    }
}

/// Format diff result as human-readable text.
#[must_use]
#[allow(clippy::too_many_lines)] // Text output groups every schema object kind in one formatter.
pub fn format_diff_text(result: &DiffResult) -> String {
    let mut output = String::new();

    if result.diff.is_empty() {
        return "No changes detected.\n".to_string();
    }

    let summary = &result.diff.summary;

    // Added tables
    if !result.diff.added_tables.is_empty() {
        output.push_str("\nAdded tables:\n");
        for table in &result.diff.added_tables {
            let _ = writeln!(output, "  + {table}");
        }
    }

    // Removed tables
    if !result.diff.removed_tables.is_empty() {
        output.push_str("\nRemoved tables:\n");
        for table in &result.diff.removed_tables {
            let _ = writeln!(output, "  - {table}");
        }
    }

    // Modified tables
    if !result.diff.modified_tables.is_empty() {
        output.push_str("\nModified tables:\n");
        for table_diff in &result.diff.modified_tables {
            let change_count = table_diff.change_count();
            let _ = writeln!(
                output,
                "  ~ {} ({change_count} changes)",
                table_diff.table_name
            );

            // Column changes
            if !table_diff.column_diffs.is_empty() {
                output.push_str("    Columns:\n");
                for col_diff in &table_diff.column_diffs {
                    let indicator = match col_diff.change_kind {
                        ChangeKind::Added => "+",
                        ChangeKind::Removed => "-",
                        ChangeKind::Modified => "~",
                    };
                    let _ = writeln!(output, "      {indicator} {}", col_diff.column_name);
                }
            }

            // FK changes
            if !table_diff.fk_diffs.is_empty() {
                output.push_str("    Foreign keys:\n");
                for fk_diff in &table_diff.fk_diffs {
                    let indicator = match fk_diff.change_kind {
                        ChangeKind::Added => "+",
                        ChangeKind::Removed => "-",
                        ChangeKind::Modified => "~",
                    };
                    let fk_name = fk_diff.name.as_deref().unwrap_or("unnamed");
                    let _ = writeln!(output, "      {indicator} {fk_name}");
                }
            }

            if !table_diff.index_diffs.is_empty() {
                output.push_str("    Indexes:\n");
                for index_diff in &table_diff.index_diffs {
                    let indicator = match index_diff.change_kind {
                        ChangeKind::Added => "+",
                        ChangeKind::Removed => "-",
                        ChangeKind::Modified => "~",
                    };
                    let index_name = index_diff.name.as_deref().unwrap_or("unnamed");
                    let _ = writeln!(output, "      {indicator} {index_name}");
                }
            }

            if !table_diff.check_diffs.is_empty() {
                output.push_str("    Checks:\n");
                for check_diff in &table_diff.check_diffs {
                    let indicator = match check_diff.change_kind {
                        ChangeKind::Added => "+",
                        ChangeKind::Removed => "-",
                        ChangeKind::Modified => "~",
                    };
                    let check_name = check_diff.name.as_deref().unwrap_or("unnamed");
                    let _ = writeln!(output, "      {indicator} {check_name}");
                }
            }
        }
    }

    if !result.diff.added_views.is_empty() {
        output.push_str("\nAdded views:\n");
        for view in &result.diff.added_views {
            let _ = writeln!(output, "  + {view}");
        }
    }

    if !result.diff.removed_views.is_empty() {
        output.push_str("\nRemoved views:\n");
        for view in &result.diff.removed_views {
            let _ = writeln!(output, "  - {view}");
        }
    }

    if !result.diff.modified_views.is_empty() {
        output.push_str("\nModified views:\n");
        for view_diff in &result.diff.modified_views {
            let change_count =
                view_diff.column_diffs.len() + usize::from(view_diff.definition_changed());
            let _ = writeln!(
                output,
                "  ~ {} ({change_count} changes)",
                view_diff.view_name
            );

            if !view_diff.column_diffs.is_empty() {
                output.push_str("    Columns:\n");
                for col_diff in &view_diff.column_diffs {
                    let indicator = match col_diff.change_kind {
                        ChangeKind::Added => "+",
                        ChangeKind::Removed => "-",
                        ChangeKind::Modified => "~",
                    };
                    let _ = writeln!(output, "      {indicator} {}", col_diff.column_name);
                }
            }

            if view_diff.definition_changed() {
                output.push_str("    Definition:\n");
                output.push_str("      ~ definition\n");
            }
        }
    }

    if !result.diff.added_enums.is_empty() {
        output.push_str("\nAdded enums:\n");
        for enum_name in &result.diff.added_enums {
            let _ = writeln!(output, "  + {enum_name}");
        }
    }

    if !result.diff.removed_enums.is_empty() {
        output.push_str("\nRemoved enums:\n");
        for enum_name in &result.diff.removed_enums {
            let _ = writeln!(output, "  - {enum_name}");
        }
    }

    if !result.diff.modified_enums.is_empty() {
        output.push_str("\nModified enums:\n");
        for enum_diff in &result.diff.modified_enums {
            let _ = writeln!(
                output,
                "  ~ {} ({} changes)",
                enum_diff.enum_name,
                enum_diff.value_diffs.len()
            );

            output.push_str("    Values:\n");
            for value_diff in &enum_diff.value_diffs {
                let indicator = match value_diff.change_kind {
                    ChangeKind::Added => "+",
                    ChangeKind::Removed => "-",
                    ChangeKind::Modified => "~",
                };
                let detail = match (value_diff.old_position, value_diff.new_position) {
                    (Some(old_position), Some(new_position)) => {
                        format!(" (position {} -> {})", old_position + 1, new_position + 1)
                    }
                    (Some(old_position), None) => format!(" (position {})", old_position + 1),
                    (None, Some(new_position)) => format!(" (position {})", new_position + 1),
                    (None, None) => String::new(),
                };
                let _ = writeln!(output, "      {indicator} {}{detail}", value_diff.value);
            }
        }
    }

    // Summary
    let _ = writeln!(
        output,
        "\nSummary: {} item(s) added, {} removed, {} modified",
        summary.added_items(),
        summary.removed_items(),
        summary.modified_items()
    );
    let _ = writeln!(
        output,
        "         tables: {} added, {} removed, {} modified",
        summary.tables_added, summary.tables_removed, summary.tables_modified
    );
    let _ = writeln!(
        output,
        "         views: {} added, {} removed, {} modified",
        summary.views_added, summary.views_removed, summary.views_modified
    );
    let _ = writeln!(
        output,
        "         enums: {} added, {} removed, {} modified",
        summary.enums_added, summary.enums_removed, summary.enums_modified
    );
    let _ = writeln!(
        output,
        "         table internals: {} column change(s), {} FK change(s), {} index change(s), {} check change(s)",
        summary.columns_changed,
        summary.foreign_keys_changed,
        summary.indexes_changed,
        summary.check_constraints_changed
    );
    let _ = writeln!(
        output,
        "         view internals: {} column change(s), {} definition change(s)",
        summary.view_columns_changed, summary.view_definitions_changed
    );
    let _ = writeln!(
        output,
        "         enum internals: {} value change(s)",
        summary.enum_values_changed
    );

    output
}

/// Format diff result as GitHub-flavored Markdown.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn format_diff_markdown(result: &DiffResult) -> String {
    let mut out = String::new();

    if result.diff.is_empty() {
        return "No schema changes detected.\n".to_string();
    }

    let summary = &result.diff.summary;

    // Header
    let _ = writeln!(
        out,
        "## Schema Diff: {} added, {} removed, {} modified\n",
        summary.added_items(),
        summary.removed_items(),
        summary.modified_items()
    );

    // Summary table
    out.push_str("| Category | Added | Removed | Modified |\n");
    out.push_str("|----------|-------|---------|----------|\n");
    let _ = writeln!(
        out,
        "| Tables | {} | {} | {} |",
        summary.tables_added, summary.tables_removed, summary.tables_modified
    );
    let _ = writeln!(
        out,
        "| Views | {} | {} | {} |",
        summary.views_added, summary.views_removed, summary.views_modified
    );
    let _ = writeln!(
        out,
        "| Enums | {} | {} | {} |",
        summary.enums_added, summary.enums_removed, summary.enums_modified
    );

    // Added tables
    if !result.diff.added_tables.is_empty() {
        out.push_str("\n### Added tables\n\n");
        for table in &result.diff.added_tables {
            let _ = writeln!(out, "- {}", markdown::code(table));
        }
    }

    // Removed tables
    if !result.diff.removed_tables.is_empty() {
        out.push_str("\n### Removed tables\n\n");
        for table in &result.diff.removed_tables {
            let _ = writeln!(out, "- {}", markdown::code(table));
        }
    }

    // Modified tables
    if !result.diff.modified_tables.is_empty() {
        out.push_str("\n### Modified tables\n\n");
        for table_diff in &result.diff.modified_tables {
            write_table_diff_markdown(&mut out, table_diff);
        }
    }

    // Views
    if !result.diff.added_views.is_empty() {
        out.push_str("\n### Added views\n\n");
        for view in &result.diff.added_views {
            let _ = writeln!(out, "- {}", markdown::code(view));
        }
    }

    if !result.diff.removed_views.is_empty() {
        out.push_str("\n### Removed views\n\n");
        for view in &result.diff.removed_views {
            let _ = writeln!(out, "- {}", markdown::code(view));
        }
    }

    if !result.diff.modified_views.is_empty() {
        out.push_str("\n### Modified views\n\n");
        for view_diff in &result.diff.modified_views {
            write_view_diff_markdown(&mut out, view_diff);
        }
    }

    // Enums
    if !result.diff.added_enums.is_empty() {
        out.push_str("\n### Added enums\n\n");
        for enum_name in &result.diff.added_enums {
            let _ = writeln!(out, "- {}", markdown::code(enum_name));
        }
    }

    if !result.diff.removed_enums.is_empty() {
        out.push_str("\n### Removed enums\n\n");
        for enum_name in &result.diff.removed_enums {
            let _ = writeln!(out, "- {}", markdown::code(enum_name));
        }
    }

    if !result.diff.modified_enums.is_empty() {
        out.push_str("\n### Modified enums\n\n");
        for enum_diff in &result.diff.modified_enums {
            write_enum_diff_markdown(&mut out, enum_diff);
        }
    }

    out
}

fn write_table_diff_markdown(out: &mut String, table_diff: &TableDiff) {
    let change_count = table_diff.change_count();
    let name = markdown::html(&table_diff.table_name);
    let _ = writeln!(
        out,
        "<details>\n<summary><code>{name}</code> ({change_count} changes)</summary>\n"
    );

    if !table_diff.column_diffs.is_empty() {
        out.push_str("**Columns:**\n\n");
        for col_diff in &table_diff.column_diffs {
            let indicator = change_indicator(col_diff.change_kind);
            let col_name = markdown::code(&col_diff.column_name);
            let _ = writeln!(out, "- `{indicator}` {col_name}");
        }
        out.push('\n');
    }

    if !table_diff.fk_diffs.is_empty() {
        out.push_str("**Foreign keys:**\n\n");
        for fk_diff in &table_diff.fk_diffs {
            let indicator = change_indicator(fk_diff.change_kind);
            let fk_name = markdown::code(fk_diff.name.as_deref().unwrap_or("unnamed"));
            let _ = writeln!(out, "- `{indicator}` {fk_name}");
        }
        out.push('\n');
    }

    if !table_diff.index_diffs.is_empty() {
        out.push_str("**Indexes:**\n\n");
        for index_diff in &table_diff.index_diffs {
            let indicator = change_indicator(index_diff.change_kind);
            let index_name = markdown::code(index_diff.name.as_deref().unwrap_or("unnamed"));
            let _ = writeln!(out, "- `{indicator}` {index_name}");
        }
        out.push('\n');
    }

    if !table_diff.check_diffs.is_empty() {
        out.push_str("**Checks:**\n\n");
        for check_diff in &table_diff.check_diffs {
            let indicator = change_indicator(check_diff.change_kind);
            let check_name = markdown::code(check_diff.name.as_deref().unwrap_or("unnamed"));
            let _ = writeln!(out, "- `{indicator}` {check_name}");
        }
        out.push('\n');
    }

    out.push_str("</details>\n");
}

fn write_view_diff_markdown(out: &mut String, view_diff: &ViewDiff) {
    let change_count = view_diff.column_diffs.len() + usize::from(view_diff.definition_changed());
    let name = markdown::html(&view_diff.view_name);
    let _ = writeln!(
        out,
        "<details>\n<summary><code>{name}</code> ({change_count} changes)</summary>\n"
    );

    if !view_diff.column_diffs.is_empty() {
        out.push_str("**Columns:**\n\n");
        for col_diff in &view_diff.column_diffs {
            let indicator = change_indicator(col_diff.change_kind);
            let col_name = markdown::code(&col_diff.column_name);
            let _ = writeln!(out, "- `{indicator}` {col_name}");
        }
        out.push('\n');
    }

    if view_diff.definition_changed() {
        out.push_str("**Definition:** changed\n\n");
    }

    out.push_str("</details>\n");
}

fn write_enum_diff_markdown(out: &mut String, enum_diff: &EnumDiff) {
    let name = markdown::html(&enum_diff.enum_name);
    let _ = writeln!(
        out,
        "<details>\n<summary><code>{name}</code> ({} changes)</summary>\n",
        enum_diff.value_diffs.len()
    );

    out.push_str("**Values:**\n\n");
    for value_diff in &enum_diff.value_diffs {
        let indicator = change_indicator(value_diff.change_kind);
        let value = markdown::code(&value_diff.value);
        let detail = match (value_diff.old_position, value_diff.new_position) {
            (Some(old), Some(new)) => format!(" (position {} → {})", old + 1, new + 1),
            (Some(old), None) => format!(" (position {})", old + 1),
            (None, Some(new)) => format!(" (position {})", new + 1),
            (None, None) => String::new(),
        };
        let _ = writeln!(out, "- `{indicator}` {value}{detail}");
    }
    out.push('\n');

    out.push_str("</details>\n");
}

/// Build a merged schema that contains the union of both schemas for diff visualization.
///
/// The merged schema includes:
/// - All tables from `after` (current state)
/// - All views and enums from `after` (current state)
/// - Removed tables from `before` (so they appear ghosted in the diagram)
/// - Removed views and enums from `before`
/// - For modified tables with removed FKs, the removed FKs are added back so that
///   removed edges are visible in the diagram.
/// - For modified views and enums, removed columns and values are added back so they
///   remain visible in the diagram.
#[must_use]
#[allow(clippy::too_many_lines)] // Merging added and removed artifacts is easier to audit in one pass.
pub fn build_diff_schema(
    before: &Schema,
    after: &Schema,
    diff: &relune_core::SchemaDiff,
) -> Schema {
    let before_by_id: HashMap<&str, &Table> = before
        .tables
        .iter()
        .map(|t| (t.stable_id.as_str(), t))
        .collect();
    let before_views_by_name: HashMap<String, &View> = before
        .views
        .iter()
        .map(|view| (view.qualified_name(), view))
        .collect();
    let before_enums_by_name: HashMap<String, &SchemaEnum> = before
        .enums
        .iter()
        .map(|enum_type| (enum_type.qualified_name(), enum_type))
        .collect();

    // Start with all after tables
    let mut tables: Vec<Table> = after.tables.clone();
    let mut views = after.views.clone();
    let mut enums = after.enums.clone();

    // For modified tables, add back removed columns and FKs so they remain visible
    for table_diff in &diff.modified_tables {
        let has_removed_cols = table_diff
            .column_diffs
            .iter()
            .any(|c| c.change_kind == ChangeKind::Removed);
        let has_removed_fks = table_diff
            .fk_diffs
            .iter()
            .any(|fk| fk.change_kind == ChangeKind::Removed);
        if !has_removed_cols && !has_removed_fks {
            continue;
        }
        // Find the before table to get the actual column/FK objects
        if let Some(before_table) = before_by_id.get(table_diff.table_name.as_str())
            && let Some(after_table) = tables
                .iter_mut()
                .find(|t| t.stable_id == before_table.stable_id)
        {
            // Restore removed columns
            if has_removed_cols {
                let after_col_names: std::collections::HashSet<&str> = after_table
                    .columns
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect();
                let cols_to_add: Vec<_> = before_table
                    .columns
                    .iter()
                    .filter(|c| !after_col_names.contains(c.name.as_str()))
                    .cloned()
                    .collect();
                after_table.columns.extend(cols_to_add);
            }

            // Restore removed FKs using structural key (handles unnamed FKs)
            if has_removed_fks {
                let after_fk_keys: std::collections::HashSet<String> = after_table
                    .foreign_keys
                    .iter()
                    .map(fk_structural_key)
                    .collect();
                let fks_to_add: Vec<_> = before_table
                    .foreign_keys
                    .iter()
                    .filter(|fk| !after_fk_keys.contains(&fk_structural_key(fk)))
                    .cloned()
                    .collect();
                after_table.foreign_keys.extend(fks_to_add);
            }
        }
    }

    // Add removed tables from before
    let after_ids: std::collections::HashSet<&str> =
        after.tables.iter().map(|t| t.stable_id.as_str()).collect();
    for table in &before.tables {
        if !after_ids.contains(table.stable_id.as_str()) {
            tables.push(table.clone());
        }
    }

    for view_diff in &diff.modified_views {
        let has_removed_cols = view_diff
            .column_diffs
            .iter()
            .any(|column| column.change_kind == ChangeKind::Removed);
        if !has_removed_cols {
            continue;
        }

        if let Some(before_view) = before_views_by_name.get(&view_diff.view_name)
            && let Some(after_view) = views
                .iter_mut()
                .find(|view| view.qualified_name() == view_diff.view_name)
        {
            let after_column_names: std::collections::HashSet<&str> = after_view
                .columns
                .iter()
                .map(|column| column.name.as_str())
                .collect();
            let columns_to_add: Vec<_> = before_view
                .columns
                .iter()
                .filter(|column| !after_column_names.contains(column.name.as_str()))
                .cloned()
                .collect();
            after_view.columns.extend(columns_to_add);
        }
    }

    let after_view_ids: std::collections::HashSet<&str> =
        after.views.iter().map(|view| view.id.as_str()).collect();
    for view in &before.views {
        if !after_view_ids.contains(view.id.as_str()) {
            views.push(view.clone());
        }
    }

    for enum_diff in &diff.modified_enums {
        let has_removed_values = enum_diff
            .value_diffs
            .iter()
            .any(|value| value.change_kind == ChangeKind::Removed);
        if !has_removed_values {
            continue;
        }

        if let Some(before_enum) = before_enums_by_name.get(&enum_diff.enum_name)
            && let Some(after_enum) = enums
                .iter_mut()
                .find(|enum_type| enum_type.qualified_name() == enum_diff.enum_name)
        {
            let after_values: std::collections::HashSet<&str> =
                after_enum.values.iter().map(String::as_str).collect();
            let values_to_add: Vec<_> = before_enum
                .values
                .iter()
                .filter(|value| !after_values.contains(value.as_str()))
                .cloned()
                .collect();
            after_enum.values.extend(values_to_add);
        }
    }

    let after_enum_ids: std::collections::HashSet<&str> = after
        .enums
        .iter()
        .map(|enum_type| enum_type.id.as_str())
        .collect();
    for enum_type in &before.enums {
        if !after_enum_ids.contains(enum_type.id.as_str()) {
            enums.push(enum_type.clone());
        }
    }

    Schema {
        tables,
        views,
        enums,
    }
}

/// Build a [`DiagramOverlay`] from a [`SchemaDiff`] for diff visualization.
///
/// Records what changed apart from how risky it is:
/// - Every added, removed, or modified table, view, enum, column, and
///   relationship gets its [`ChangeKind`]; a column whose type changed also
///   keeps its previous type.
/// - Each review finding becomes a risk annotation on the table it names,
///   ranked by its [`ReviewSeverity`](relune_core::ReviewSeverity).
#[must_use]
#[allow(clippy::too_many_lines)] // Overlay changes mirror every diff kind and stay easier to align in one function.
pub fn build_diff_overlay(
    before: &Schema,
    after: &Schema,
    diff: &relune_core::SchemaDiff,
    findings: &[RiskFinding],
) -> DiagramOverlay {
    let mut overlay = DiagramOverlay::new();

    let before_by_name: HashMap<String, &Table> = before
        .tables
        .iter()
        .map(|t| (t.qualified_name(), t))
        .collect();
    let after_by_name: HashMap<String, &Table> = after
        .tables
        .iter()
        .map(|t| (t.qualified_name(), t))
        .collect();
    let before_views_by_name: HashMap<String, &View> = before
        .views
        .iter()
        .map(|view| (view.qualified_name(), view))
        .collect();
    let after_views_by_name: HashMap<String, &View> = after
        .views
        .iter()
        .map(|view| (view.qualified_name(), view))
        .collect();
    let before_enums_by_name: HashMap<String, &SchemaEnum> = before
        .enums
        .iter()
        .map(|enum_type| (enum_type.qualified_name(), enum_type))
        .collect();
    let after_enums_by_name: HashMap<String, &SchemaEnum> = after
        .enums
        .iter()
        .map(|enum_type| (enum_type.qualified_name(), enum_type))
        .collect();

    for (names, tables, schema, kind) in [
        (&diff.added_tables, &after_by_name, after, ChangeKind::Added),
        (
            &diff.removed_tables,
            &before_by_name,
            before,
            ChangeKind::Removed,
        ),
    ] {
        for table in names.iter().filter_map(|name| tables.get(name)) {
            overlay.set_node_change(
                &table.stable_id,
                whole_node_change(kind, "table", table.columns.len(), "columns"),
            );
            mark_table_edges(&mut overlay, table, schema, kind);
        }
    }

    for table_diff in &diff.modified_tables {
        let stable_id = after_by_name
            .get(&table_diff.table_name)
            .or_else(|| before_by_name.get(&table_diff.table_name))
            .map(|t| t.stable_id.as_str());

        if let Some(stable_id) = stable_id {
            mark_modified_table(&mut overlay, stable_id, table_diff, before, after);
        }
    }

    for (names, views, kind) in [
        (&diff.added_views, &after_views_by_name, ChangeKind::Added),
        (
            &diff.removed_views,
            &before_views_by_name,
            ChangeKind::Removed,
        ),
    ] {
        for view in names.iter().filter_map(|name| views.get(name)) {
            overlay.set_node_change(
                &view.id,
                whole_node_change(kind, "view", view.columns.len(), "columns"),
            );
        }
    }

    for view_diff in &diff.modified_views {
        let view_id = after_views_by_name
            .get(&view_diff.view_name)
            .or_else(|| before_views_by_name.get(&view_diff.view_name))
            .map(|view| view.id.as_str());
        if let Some(view_id) = view_id {
            mark_modified_view(&mut overlay, view_id, view_diff);
        }
    }

    for (names, enums, kind) in [
        (&diff.added_enums, &after_enums_by_name, ChangeKind::Added),
        (
            &diff.removed_enums,
            &before_enums_by_name,
            ChangeKind::Removed,
        ),
    ] {
        for enum_type in names.iter().filter_map(|name| enums.get(name)) {
            overlay.set_node_change(
                &enum_type.id,
                whole_node_change(kind, "enum", enum_type.values.len(), "values"),
            );
        }
    }

    for enum_diff in &diff.modified_enums {
        let enum_id = after_enums_by_name
            .get(&enum_diff.enum_name)
            .or_else(|| before_enums_by_name.get(&enum_diff.enum_name))
            .map(|enum_type| enum_type.id.as_str());
        if let Some(enum_id) = enum_id {
            mark_modified_enum(&mut overlay, enum_id, enum_diff);
        }
    }

    for finding in findings {
        if let Some(table_id) = &finding.table_id {
            overlay.add_node_annotation(
                table_id,
                Annotation {
                    severity: finding.severity,
                    message: finding.message.clone(),
                    hint: finding.mitigation.clone(),
                    rule_id: Some(finding.rule_id.as_str().to_string()),
                },
            );
        }
    }

    overlay
}

/// Change of a node that was added or removed as a whole.
fn whole_node_change(kind: ChangeKind, noun: &str, parts: usize, part_noun: &str) -> NodeChange {
    let label = match kind {
        ChangeKind::Added => "Added",
        ChangeKind::Removed => "Removed",
        ChangeKind::Modified => "Modified",
    };
    NodeChange {
        kind,
        summary: format!("{label} {noun} ({parts} {part_noun})"),
        details: Vec::new(),
    }
}

/// Change of a node whose parts changed.
fn modified_node_change(change_count: usize, details: Vec<String>) -> NodeChange {
    NodeChange {
        kind: ChangeKind::Modified,
        summary: format!("Modified ({change_count} changes)"),
        details,
    }
}

fn mark_table_edges(
    overlay: &mut DiagramOverlay,
    table: &Table,
    schema: &Schema,
    kind: ChangeKind,
) {
    for fk in &table.foreign_keys {
        let target_id = resolve_fk_target_stable_id(schema, fk.to_schema.as_deref(), &fk.to_table);
        let to_id = target_id.as_deref().unwrap_or(&fk.to_table);
        overlay.set_edge_change(
            &table.stable_id,
            to_id,
            &fk.from_columns,
            &fk.to_columns,
            kind,
        );
    }
}

const fn change_indicator(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "+",
        ChangeKind::Removed => "-",
        ChangeKind::Modified => "~",
    }
}

/// Type a modified column had before, when its type changed.
fn previous_type(column: &relune_core::diff::ColumnDiff) -> Option<&str> {
    let old = column.old_value.as_ref()?;
    let new = column.new_value.as_ref()?;
    (!old.data_type.eq_ignore_ascii_case(&new.data_type)).then_some(old.data_type.as_str())
}

/// Records each column change on `node_id` and returns its detail lines.
fn mark_column_changes(
    overlay: &mut DiagramOverlay,
    node_id: &str,
    column_diffs: &[relune_core::diff::ColumnDiff],
) -> Vec<String> {
    column_diffs
        .iter()
        .map(|column| {
            let previous_type = previous_type(column);
            overlay.set_column_change(
                node_id,
                &column.column_name,
                ColumnChange {
                    kind: column.change_kind,
                    previous_type: previous_type.map(str::to_string),
                },
            );
            let indicator = change_indicator(column.change_kind);
            match (previous_type, &column.new_value) {
                (Some(previous), Some(new)) => format!(
                    "{indicator} {}: {previous}{TYPE_CHANGE_SEPARATOR}{}",
                    column.column_name, new.data_type
                ),
                _ => format!("{indicator} {}", column.column_name),
            }
        })
        .collect()
}

fn mark_modified_table(
    overlay: &mut DiagramOverlay,
    stable_id: &str,
    table_diff: &TableDiff,
    before: &Schema,
    after: &Schema,
) {
    let mut details = mark_column_changes(overlay, stable_id, &table_diff.column_diffs);
    for fk in &table_diff.fk_diffs {
        let name = fk.name.as_deref().unwrap_or("unnamed FK");
        details.push(format!("{} {name}", change_indicator(fk.change_kind)));
    }
    for idx in &table_diff.index_diffs {
        let name = idx.name.as_deref().unwrap_or("unnamed index");
        details.push(format!("{} {name}", change_indicator(idx.change_kind)));
    }
    for check in &table_diff.check_diffs {
        let name = check.name.as_deref().unwrap_or("unnamed check");
        details.push(format!("{} {name}", change_indicator(check.change_kind)));
    }
    overlay.set_node_change(
        stable_id,
        modified_node_change(table_diff.change_count(), details),
    );

    for fk_diff in &table_diff.fk_diffs {
        let fk_ref = fk_diff.new_value.as_ref().or(fk_diff.old_value.as_ref());
        let Some(fk_ref) = fk_ref.filter(|fk| !fk.to_table.is_empty()) else {
            continue;
        };
        let to_schema = fk_ref.to_schema.as_deref();
        let to_table = fk_ref.to_table.as_str();
        let target_id = resolve_fk_target_stable_id(after, to_schema, to_table)
            .or_else(|| resolve_fk_target_stable_id(before, to_schema, to_table))
            .unwrap_or_else(|| to_table.to_string());
        overlay.set_edge_change(
            stable_id,
            &target_id,
            &fk_ref.from_columns,
            &fk_ref.to_columns,
            fk_diff.change_kind,
        );
    }
}

fn mark_modified_view(overlay: &mut DiagramOverlay, view_id: &str, view_diff: &ViewDiff) {
    let mut details = mark_column_changes(overlay, view_id, &view_diff.column_diffs);
    if view_diff.definition_changed() {
        details.push("~ definition".to_string());
    }

    let change_count = view_diff.column_diffs.len() + usize::from(view_diff.definition_changed());
    overlay.set_node_change(view_id, modified_node_change(change_count, details));
}

fn mark_modified_enum(overlay: &mut DiagramOverlay, enum_id: &str, enum_diff: &EnumDiff) {
    // Enum nodes list their values as columns.
    for value_diff in &enum_diff.value_diffs {
        overlay.set_column_change(
            enum_id,
            &value_diff.value,
            ColumnChange::new(value_diff.change_kind),
        );
    }
    let details = enum_diff
        .value_diffs
        .iter()
        .map(|value_diff| {
            if let (Some(old_position), Some(new_position)) =
                (value_diff.old_position, value_diff.new_position)
            {
                format!(
                    "{} {} ({} -> {})",
                    change_indicator(value_diff.change_kind),
                    value_diff.value,
                    old_position + 1,
                    new_position + 1
                )
            } else {
                format!(
                    "{} {}",
                    change_indicator(value_diff.change_kind),
                    value_diff.value
                )
            }
        })
        .collect::<Vec<_>>();

    overlay.set_node_change(
        enum_id,
        modified_node_change(enum_diff.value_diffs.len(), details),
    );
}

/// Compute a structural identity key for a FK, matching the diff engine's approach.
///
/// Named FKs use their name; unnamed FKs use a composite of target schema,
/// target table, and sorted column pairs.
fn fk_structural_key(fk: &relune_core::ForeignKey) -> String {
    use std::fmt::Write;

    if let Some(name) = &fk.name {
        return name.clone();
    }
    let mut key = String::new();
    if let Some(ref s) = fk.to_schema {
        let _ = write!(key, "{s}.");
    }
    let _ = write!(key, "{}", fk.to_table);
    let mut pairs: Vec<_> = fk
        .from_columns
        .iter()
        .zip(fk.to_columns.iter())
        .map(|(f, t)| format!("{f}->{t}"))
        .collect();
    pairs.sort_unstable();
    for p in &pairs {
        let _ = write!(key, "/{p}");
    }
    key
}

/// Resolve a FK target to its `stable_id` within a schema, considering `to_schema`.
fn resolve_fk_target_stable_id(
    schema: &Schema,
    to_schema: Option<&str>,
    to_table: &str,
) -> Option<String> {
    if let Some(schema_name) = to_schema {
        // Prefer exact schema + table match for multi-schema correctness
        if let Some(t) = schema
            .tables
            .iter()
            .find(|t| t.schema_name.as_deref() == Some(schema_name) && t.name == to_table)
        {
            return Some(t.stable_id.clone());
        }
    }
    // Fallback: match by name or qualified_name
    schema
        .tables
        .iter()
        .find(|t| t.name == to_table || t.qualified_name() == to_table)
        .map(|t| t.stable_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema_input::schema_from_input;

    #[test]
    fn test_diff_rejects_identity_errors_unless_allowed() {
        let before = "CREATE TABLE t (id INT, id INT);";
        let after = "CREATE TABLE t (id INT);";

        let error = diff(DiffRequest::from_sql(before, after))
            .expect_err("duplicate columns should be rejected");
        assert!(
            matches!(&error, AppError::InvalidSchema { input, .. } if input == "before"),
            "unexpected error: {error:?}"
        );
        assert_eq!(error.category_code(), Some("INVALID_SCHEMA"));

        let result = diff(DiffRequest {
            allow_invalid_schema: true,
            ..DiffRequest::from_sql(before, after)
        })
        .expect("allow_invalid_schema should continue");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("duplicate column name 'id'"))
        );
    }

    #[test]
    fn test_diff_keeps_dangling_foreign_keys_as_warnings() {
        let before = "CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));";
        let after =
            "CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id INT REFERENCES users(id));";

        let result = diff(DiffRequest::from_sql(before, after)).expect("diff should succeed");
        assert!(result.has_changes());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("FK references unknown table 'users'"))
        );
    }

    #[test]
    fn test_diff_no_changes() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);";
        let after = "CREATE TABLE users (id INT PRIMARY KEY);";

        let request = DiffRequest::from_sql(before, after);
        let result = diff(request).unwrap();

        assert!(result.diff.is_empty());
        assert!(!result.has_changes());
    }

    #[test]
    fn test_diff_matches_unqualified_ddl_with_default_schema() {
        use crate::request::InputSource;
        use relune_core::SqlDialect;

        let before = "
            CREATE TABLE public.users (id INT PRIMARY KEY);
            CREATE TABLE public.orders (id INT PRIMARY KEY, user_id INT REFERENCES public.users(id));
        ";
        let after = "
            CREATE TABLE users (id INT PRIMARY KEY, name TEXT);
            CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));
        ";

        let result = diff(DiffRequest {
            before: InputSource::sql_text_with_dialect(before, SqlDialect::Postgres),
            after: InputSource::sql_text_with_dialect(after, SqlDialect::Postgres),
            ..DiffRequest::from_sql("", "")
        })
        .unwrap();

        assert!(result.diff.added_tables.is_empty());
        assert!(result.diff.removed_tables.is_empty());
        assert_eq!(result.diff.modified_tables.len(), 1);
        assert_eq!(result.diff.modified_tables[0].table_name, "users");
        assert_eq!(result.diff.modified_tables[0].column_diffs.len(), 1);
    }

    #[test]
    fn test_diff_added_table() {
        let before = "";
        let after = "CREATE TABLE users (id INT PRIMARY KEY);";

        let request = DiffRequest::from_sql(before, after);
        let result = diff(request).unwrap();

        assert!(!result.diff.is_empty());
        assert_eq!(result.diff.added_tables.len(), 1);
        assert!(result.diff.added_tables.contains(&"users".to_string()));
    }

    #[test]
    fn test_diff_removed_table() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);";
        let after = "";

        let request = DiffRequest::from_sql(before, after);
        let result = diff(request).unwrap();

        assert!(!result.diff.is_empty());
        assert_eq!(result.diff.removed_tables.len(), 1);
    }

    #[test]
    fn test_diff_added_column() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);";
        let after = "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));";

        let request = DiffRequest::from_sql(before, after);
        let result = diff(request).unwrap();

        assert!(!result.diff.is_empty());
        assert_eq!(result.diff.modified_tables.len(), 1);
        assert_eq!(result.diff.modified_tables[0].column_diffs.len(), 1);
        assert_eq!(
            result.diff.modified_tables[0].column_diffs[0].change_kind,
            ChangeKind::Added
        );
    }

    #[test]
    fn test_format_diff_text_no_changes() {
        let result = DiffResult {
            diff: relune_core::SchemaDiff::default(),
            diagnostics: vec![],
            rendered: None,
        };

        let text = format_diff_text(&result);
        assert!(text.contains("No changes detected"));
    }

    #[test]
    fn test_format_diff_text_with_changes() {
        let mut diff_result = relune_core::SchemaDiff::default();
        diff_result.added_tables.push("new_table".to_string());
        diff_result.summary.tables_added = 1;

        let result = DiffResult {
            diff: diff_result,
            diagnostics: vec![],
            rendered: None,
        };

        let text = format_diff_text(&result);
        assert!(text.contains("Added tables"));
        assert!(text.contains("new_table"));
    }

    #[test]
    fn test_format_diff_text_with_view_and_enum_changes() {
        let before = "\
            CREATE TYPE status AS ENUM ('draft', 'published');\n\
            CREATE TABLE users (id INT PRIMARY KEY, status status);\n\
            CREATE VIEW active_users AS SELECT id, status FROM users;\n\
        ";
        let after = "\
            CREATE TYPE status AS ENUM ('published', 'draft');\n\
            CREATE TABLE users (id INT PRIMARY KEY, status TEXT);\n\
            CREATE VIEW active_users AS SELECT id FROM users;\n\
        ";

        let result = diff(DiffRequest::from_sql(before, after)).unwrap();
        let text = format_diff_text(&result);

        assert!(text.contains("Modified views"));
        assert!(text.contains("active_users"));
        assert!(text.contains("Modified enums"));
        assert!(text.contains("status"));
        assert!(text.contains("view internals"));
        assert!(text.contains("enum internals"));
    }

    #[test]
    fn test_format_diff_includes_check_constraint_changes() {
        let before = "CREATE TABLE t (x INT, CONSTRAINT x_pos CHECK (x > 0));";
        let after = "CREATE TABLE t (x INT, CONSTRAINT x_pos CHECK (x > 1));";

        let result = diff(DiffRequest::from_sql(before, after)).unwrap();

        let text = format_diff_text(&result);
        assert!(
            text.contains("Checks:"),
            "text diff should list check changes:\n{text}"
        );
        assert!(text.contains("~ x_pos"));
        assert!(
            text.contains("~ t (1 changes)"),
            "change count must include check changes:\n{text}"
        );
        assert!(
            text.contains("1 check change(s)"),
            "summary must report check changes:\n{text}"
        );

        let markdown = format_diff_markdown(&result);
        assert!(
            markdown.contains("**Checks:**"),
            "markdown diff should list check changes:\n{markdown}"
        );
    }

    #[test]
    fn test_build_diff_schema_includes_all_tables() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);";
        let after = "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));\nCREATE TABLE posts (id INT PRIMARY KEY);";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let merged = build_diff_schema(&before_schema, &after_schema, &diff);
        assert_eq!(merged.tables.len(), 2);
    }

    #[test]
    fn test_build_diff_schema_includes_removed_tables() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);\nCREATE TABLE old_table (id INT PRIMARY KEY);";
        let after = "CREATE TABLE users (id INT PRIMARY KEY);";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let merged = build_diff_schema(&before_schema, &after_schema, &diff);
        assert_eq!(merged.tables.len(), 2);
        assert!(merged.tables.iter().any(|t| t.name == "old_table"));
    }

    #[test]
    fn test_build_diff_schema_restores_removed_columns() {
        let before =
            "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255), email VARCHAR(255));";
        let after = "CREATE TABLE users (id INT PRIMARY KEY, email VARCHAR(255));";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let merged = build_diff_schema(&before_schema, &after_schema, &diff);
        let users = merged.tables.iter().find(|t| t.name == "users").unwrap();
        assert!(
            users.columns.iter().any(|c| c.name == "name"),
            "removed column 'name' should be restored in merged schema"
        );
        assert_eq!(users.columns.len(), 3);
    }

    #[test]
    fn test_build_diff_schema_restores_removed_view_columns_and_enum_values() {
        let before = "\
            CREATE TYPE status AS ENUM ('draft', 'published');\n\
            CREATE TABLE users (id INT PRIMARY KEY, status status);\n\
            CREATE VIEW active_users AS SELECT id, status FROM users;\n\
        ";
        let after = "\
            CREATE TYPE status AS ENUM ('published');\n\
            CREATE TABLE users (id INT PRIMARY KEY, status TEXT);\n\
            CREATE VIEW active_users AS SELECT id FROM users;\n\
        ";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let merged = build_diff_schema(&before_schema, &after_schema, &diff);
        let active_users = merged
            .views
            .iter()
            .find(|view| view.name == "active_users")
            .unwrap();
        assert!(
            active_users
                .columns
                .iter()
                .any(|column| column.name == "status"),
            "removed view column should be restored in merged schema"
        );

        let status = merged
            .enums
            .iter()
            .find(|enum_type| enum_type.name == "status")
            .unwrap();
        assert!(
            status.values.iter().any(|value| value == "draft"),
            "removed enum value should be restored in merged schema"
        );
    }

    #[test]
    fn test_build_diff_schema_restores_unnamed_fk() {
        let before = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE posts (id INT PRIMARY KEY, user_id INT REFERENCES users(id));\n\
        ";
        let after = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE posts (id INT PRIMARY KEY, user_id INT);\n\
        ";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let merged = build_diff_schema(&before_schema, &after_schema, &diff);
        let posts = merged.tables.iter().find(|t| t.name == "posts").unwrap();
        assert_eq!(
            posts.foreign_keys.len(),
            1,
            "removed unnamed FK should be restored"
        );
    }

    #[test]
    fn test_build_diff_overlay_added_table() {
        let before = "";
        let after = "CREATE TABLE users (id INT PRIMARY KEY);";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff, &[]);
        assert!(!overlay.is_empty());
        let node = overlay.node("users").expect("should have users overlay");
        assert_eq!(node.change_kind(), Some(ChangeKind::Added));
        assert!(node.annotations.is_empty());
    }

    #[test]
    fn test_build_diff_overlay_view_and_enum_nodes() {
        let before = "\
            CREATE TYPE status AS ENUM ('draft', 'published');\n\
            CREATE TABLE users (id INT PRIMARY KEY, status status);\n\
            CREATE VIEW active_users AS SELECT id, status FROM users;\n\
        ";
        let after = "\
            CREATE TYPE status AS ENUM ('published', 'draft');\n\
            CREATE TABLE users (id INT PRIMARY KEY, status TEXT);\n\
            CREATE VIEW active_users AS SELECT id FROM users;\n\
        ";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff, &[]);
        let view_overlay = overlay.node("active_users").expect("view overlay");
        assert_eq!(view_overlay.change_kind(), Some(ChangeKind::Modified));

        assert_eq!(
            view_overlay.column_changes,
            std::collections::BTreeMap::from([(
                "status".to_string(),
                ColumnChange::new(ChangeKind::Removed)
            )])
        );

        let enum_overlay = overlay.node("status").expect("enum overlay");
        assert_eq!(enum_overlay.change_kind(), Some(ChangeKind::Modified));
        assert_eq!(
            enum_overlay.column_changes,
            std::collections::BTreeMap::from([
                ("draft".to_string(), ColumnChange::new(ChangeKind::Modified)),
                (
                    "published".to_string(),
                    ColumnChange::new(ChangeKind::Modified)
                ),
            ])
        );
    }

    #[test]
    fn test_build_diff_overlay_removed_table() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);";
        let after = "";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff, &[]);
        assert!(!overlay.is_empty());
        let node = overlay.node("users").expect("should have users overlay");
        assert_eq!(node.change_kind(), Some(ChangeKind::Removed));
    }

    #[test]
    fn test_build_diff_overlay_modified_table() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);";
        let after = "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff, &[]);
        assert!(!overlay.is_empty());
        let node = overlay.node("users").expect("should have users overlay");
        let change = node.change.as_ref().expect("modified table change");
        assert_eq!(change.kind, ChangeKind::Modified);
        assert_eq!(change.details, vec!["+ name".to_string()]);
    }

    #[test]
    fn test_build_diff_overlay_records_column_changes_apart_from_same_named_index() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY, email TEXT);";
        let after = "\
            CREATE TABLE users (id INT PRIMARY KEY, email VARCHAR(255), name TEXT);\n\
            CREATE INDEX id ON users (email);\n\
        ";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff, &[]);
        let node = overlay.node("users").expect("should have users overlay");
        assert_eq!(
            node.column_changes,
            std::collections::BTreeMap::from([
                (
                    "email".to_string(),
                    ColumnChange {
                        kind: ChangeKind::Modified,
                        previous_type: Some("TEXT".to_string()),
                    }
                ),
                ("name".to_string(), ColumnChange::new(ChangeKind::Added)),
            ])
        );
        let details = &node.change.as_ref().expect("change").details;
        assert!(details.contains(&"~ email: TEXT → VARCHAR(255)".to_string()));
    }

    #[test]
    fn test_build_diff_overlay_keeps_risk_apart_from_change_kind() {
        let before = "\
            CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(120), bio VARCHAR(500));\n\
        ";
        // `name` widens (modified, safe); `bio` narrows (modified, breaking);
        // `status` is a NOT NULL column added without a default (added, risky).
        let after = "\
            CREATE TABLE users (\
                id INT PRIMARY KEY, name VARCHAR(500), bio VARCHAR(120), status TEXT NOT NULL\
            );\n\
        ";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);
        let findings = relune_core::run_rules(
            &diff,
            &before_schema,
            &after_schema,
            ReviewRuleId::all_rules(),
            relune_core::EffectiveDialect::Auto,
        );

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff, &findings);
        let node = overlay.node("users").expect("users overlay");
        assert_eq!(node.change_kind(), Some(ChangeKind::Modified));
        assert_eq!(
            node.column_changes["status"],
            ColumnChange::new(ChangeKind::Added)
        );
        assert_eq!(
            node.column_changes["name"].previous_type.as_deref(),
            Some("VARCHAR(120)")
        );
        let rules: Vec<_> = node
            .annotations
            .iter()
            .map(|a| (a.rule_id.as_deref().unwrap_or_default(), a.severity))
            .collect();
        assert!(rules.contains(&("risk/type-narrow", relune_core::ReviewSeverity::Breaking)));
        assert!(rules.contains(&(
            "risk/add-not-null-on-existing",
            relune_core::ReviewSeverity::Warning
        )));
        assert_eq!(
            node.top_risk(),
            Some((relune_core::ReviewSeverity::Breaking, 1))
        );
    }

    #[test]
    fn test_build_diff_overlay_no_changes() {
        let sql = "CREATE TABLE users (id INT PRIMARY KEY);";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(sql)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(sql)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff, &[]);
        assert!(overlay.is_empty());
    }

    #[test]
    fn test_diff_render_svg() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);";
        let after = "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));\nCREATE TABLE posts (id INT PRIMARY KEY, user_id INT REFERENCES users(id));";

        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            ..Default::default()
        };
        let result = diff(request).unwrap();

        assert!(result.rendered.is_some());
        let svg = result.rendered.unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("node-kind-table diff-added"));
        assert!(svg.contains("node-kind-table diff-modified"));
        assert!(svg.contains(r#"class="column-row diff-added" data-column-name="name""#));
    }

    #[test]
    fn test_diff_render_html() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);";
        let after = "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));";

        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Html,
            ..Default::default()
        };
        let result = diff(request).unwrap();

        assert!(result.rendered.is_some());
        let html = result.rendered.unwrap();
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("diff-modified"));
        // Metadata carries the change kind and details apart from risks
        assert!(html.contains(r#""diff_kind":"modified""#));
        assert!(html.contains(r#""diff_details":["+ name"]"#));
    }

    // ---------------------------------------------------------------
    // Filter × overlay interaction tests
    // ---------------------------------------------------------------

    #[test]
    fn test_diff_svg_filter_include_preserves_overlay() {
        let before = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));\n\
        ";
        let after = "\
            CREATE TABLE users (id INT PRIMARY KEY, email VARCHAR(255));\n\
            CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users(id));\n\
            CREATE TABLE products (id INT PRIMARY KEY);\n\
        ";

        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            filter: relune_core::FilterSpec {
                include: vec!["users".to_string(), "products".to_string()],
                exclude: vec![],
            },
            ..Default::default()
        };
        let result = diff(request).unwrap();

        let svg = result.rendered.as_deref().expect("SVG output expected");
        assert!(svg.contains("<svg"), "should produce valid SVG");
        // Modified users → modified marking
        assert!(svg.contains("diff-modified"), "modified table overlay");
        // Added products → added marking
        assert!(svg.contains("diff-added"), "added table overlay");
        // orders should be filtered out
        assert!(
            !svg.contains(">orders<"),
            "excluded table should not appear in SVG"
        );
    }

    #[test]
    fn test_diff_svg_filter_exclude_hides_changed_table() {
        let before = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE logs (id INT PRIMARY KEY, ts TIMESTAMP);\n\
        ";
        let after = "\
            CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));\n\
            CREATE TABLE logs (id INT PRIMARY KEY);\n\
        ";

        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            filter: relune_core::FilterSpec {
                include: vec![],
                exclude: vec!["logs".to_string()],
            },
            ..Default::default()
        };
        let result = diff(request).unwrap();

        // Diff data should still contain logs as modified
        assert!(
            result
                .diff
                .modified_tables
                .iter()
                .any(|t| t.table_name == "logs"),
            "diff data should include logs"
        );

        let svg = result.rendered.as_deref().expect("SVG output expected");
        assert!(svg.contains("diff-modified"), "users should be modified");
        assert!(
            !svg.contains(">logs<"),
            "excluded table should not appear in SVG"
        );
    }

    #[test]
    fn test_diff_svg_filter_include_all_shows_removed_table() {
        let before = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE old_cache (id INT PRIMARY KEY);\n\
        ";
        let after = "CREATE TABLE users (id INT PRIMARY KEY);";

        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            filter: relune_core::FilterSpec {
                include: vec!["*".to_string()],
                exclude: vec![],
            },
            ..Default::default()
        };
        let result = diff(request).unwrap();

        let svg = result.rendered.as_deref().expect("SVG output expected");
        // Removed table → removed marking
        assert!(
            svg.contains("diff-removed"),
            "removed table should have removed marking"
        );
        assert!(
            svg.contains("old_cache"),
            "removed table should appear in SVG"
        );
    }

    #[test]
    fn test_diff_svg_filter_excludes_removed_table() {
        let before = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE old_cache (id INT PRIMARY KEY);\n\
        ";
        let after = "CREATE TABLE users (id INT PRIMARY KEY);";

        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            filter: relune_core::FilterSpec {
                include: vec![],
                exclude: vec!["old_*".to_string()],
            },
            ..Default::default()
        };
        let result = diff(request).unwrap();

        // Diff data still records the removal
        assert!(
            result
                .diff
                .removed_tables
                .contains(&"old_cache".to_string()),
            "diff data should include removed table"
        );

        let svg = result.rendered.as_deref().expect("SVG output expected");
        assert!(
            !svg.contains("old_cache"),
            "filtered-out removed table should not appear"
        );
    }

    #[test]
    fn test_diff_svg_filter_removed_fk_target_excluded() {
        let before = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE posts (id INT PRIMARY KEY, user_id INT REFERENCES users(id));\n\
        ";
        let after = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE posts (id INT PRIMARY KEY, user_id INT);\n\
        ";

        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            filter: relune_core::FilterSpec {
                include: vec!["posts".to_string()],
                exclude: vec![],
            },
            ..Default::default()
        };
        // Should not panic even when FK target is filtered out
        let result = diff(request).unwrap();

        let svg = result.rendered.as_deref().expect("SVG output expected");
        assert!(svg.contains("<svg"), "should produce valid SVG");
        // posts is modified (FK removed)
        assert!(
            svg.contains("diff-modified"),
            "modified table should have modified marking"
        );
    }

    // ---------------------------------------------------------------
    // Grouping × overlay interaction tests
    // ---------------------------------------------------------------

    #[test]
    fn test_diff_svg_grouping_by_schema_preserves_overlay() {
        let before = "\
            CREATE SCHEMA sales;\n\
            CREATE TABLE sales.orders (id INT PRIMARY KEY);\n\
            CREATE SCHEMA hr;\n\
            CREATE TABLE hr.employees (id INT PRIMARY KEY);\n\
        ";
        let after = "\
            CREATE SCHEMA sales;\n\
            CREATE TABLE sales.orders (id INT PRIMARY KEY, total DECIMAL);\n\
            CREATE SCHEMA hr;\n\
            CREATE TABLE hr.employees (id INT PRIMARY KEY);\n\
            CREATE TABLE hr.departments (id INT PRIMARY KEY);\n\
        ";

        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            grouping: relune_core::GroupingSpec {
                strategy: relune_core::GroupingStrategy::BySchema,
            },
            ..Default::default()
        };
        let result = diff(request).unwrap();

        let svg = result.rendered.as_deref().expect("SVG output expected");
        assert!(svg.contains("<svg"), "should produce valid SVG");
        // Modified orders → warning
        assert!(
            svg.contains("diff-modified"),
            "modified table should have modified marking"
        );
        // Added departments → info
        assert!(
            svg.contains("diff-added"),
            "added table should have added marking"
        );
    }

    #[test]
    fn test_diff_svg_grouping_by_prefix_preserves_overlay() {
        let before = "\
            CREATE TABLE app_users (id INT PRIMARY KEY);\n\
            CREATE TABLE app_posts (id INT PRIMARY KEY);\n\
            CREATE TABLE sys_logs (id INT PRIMARY KEY);\n\
        ";
        let after = "\
            CREATE TABLE app_users (id INT PRIMARY KEY, name VARCHAR(255));\n\
            CREATE TABLE app_posts (id INT PRIMARY KEY);\n\
        ";

        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            grouping: relune_core::GroupingSpec {
                strategy: relune_core::GroupingStrategy::ByPrefix,
            },
            ..Default::default()
        };
        let result = diff(request).unwrap();

        let svg = result.rendered.as_deref().expect("SVG output expected");
        assert!(svg.contains("<svg"), "should produce valid SVG");
        // Modified app_users → warning
        assert!(
            svg.contains("diff-modified"),
            "modified table should have modified marking"
        );
        // Removed sys_logs → error
        assert!(
            svg.contains("diff-removed"),
            "removed table should have removed marking"
        );
        // All tables should be present
        assert!(svg.contains("app_users"), "app_users should appear");
        assert!(svg.contains("sys_logs"), "removed table should appear");
    }

    #[test]
    fn test_diff_svg_grouping_does_not_hide_removed_table() {
        let before = "\
            CREATE TABLE core_users (id INT PRIMARY KEY);\n\
            CREATE TABLE core_sessions (id INT PRIMARY KEY);\n\
        ";
        let after = "CREATE TABLE core_users (id INT PRIMARY KEY);";

        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            grouping: relune_core::GroupingSpec {
                strategy: relune_core::GroupingStrategy::ByPrefix,
            },
            ..Default::default()
        };
        let result = diff(request).unwrap();

        assert!(
            !result.diff.removed_tables.is_empty(),
            "should detect removal"
        );
        let svg = result.rendered.as_deref().expect("SVG output expected");
        assert!(
            svg.contains("core_sessions"),
            "removed table must remain visible with grouping active"
        );
        assert!(
            svg.contains("diff-removed"),
            "removed table should have removed marking"
        );
    }

    // ---------------------------------------------------------------
    // Combined filter + grouping + overlay tests
    // ---------------------------------------------------------------

    #[test]
    fn test_diff_svg_filter_and_grouping_combined() {
        let before = "\
            CREATE TABLE app_users (id INT PRIMARY KEY);\n\
            CREATE TABLE app_posts (id INT PRIMARY KEY, user_id INT REFERENCES app_users(id));\n\
            CREATE TABLE sys_logs (id INT PRIMARY KEY);\n\
        ";
        let after = "\
            CREATE TABLE app_users (id INT PRIMARY KEY, email VARCHAR(255));\n\
            CREATE TABLE app_posts (id INT PRIMARY KEY, user_id INT REFERENCES app_users(id));\n\
            CREATE TABLE app_tags (id INT PRIMARY KEY);\n\
        ";

        // Filter: include only app_* tables; grouping: by prefix
        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            filter: relune_core::FilterSpec {
                include: vec!["app_*".to_string()],
                exclude: vec![],
            },
            grouping: relune_core::GroupingSpec {
                strategy: relune_core::GroupingStrategy::ByPrefix,
            },
            ..Default::default()
        };
        let result = diff(request).unwrap();

        let svg = result.rendered.as_deref().expect("SVG output expected");
        assert!(svg.contains("<svg"));
        // Modified app_users → warning
        assert!(svg.contains("diff-modified"), "modified table overlay");
        // Added app_tags → info
        assert!(svg.contains("diff-added"), "added table overlay");
        // sys_logs is removed but also filtered out by app_* include
        assert!(
            !svg.contains("sys_logs"),
            "sys_logs should be excluded by filter"
        );
    }

    #[test]
    fn test_diff_svg_filter_grouping_with_removed_fk_edge() {
        let before = "\
            CREATE TABLE app_users (id INT PRIMARY KEY);\n\
            CREATE TABLE app_orders (id INT PRIMARY KEY, user_id INT REFERENCES app_users(id));\n\
            CREATE TABLE app_items (id INT PRIMARY KEY, order_id INT REFERENCES app_orders(id));\n\
        ";
        let after = "\
            CREATE TABLE app_users (id INT PRIMARY KEY);\n\
            CREATE TABLE app_orders (id INT PRIMARY KEY, user_id INT);\n\
            CREATE TABLE app_items (id INT PRIMARY KEY, order_id INT REFERENCES app_orders(id));\n\
        ";

        // FK from orders→users removed; all tables visible with grouping
        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            grouping: relune_core::GroupingSpec {
                strategy: relune_core::GroupingStrategy::ByPrefix,
            },
            ..Default::default()
        };
        let result = diff(request).unwrap();

        let svg = result.rendered.as_deref().expect("SVG output expected");
        assert!(svg.contains("<svg"));
        // app_orders is modified (FK removed)
        assert!(svg.contains("diff-modified"), "modified table overlay");
        // All three tables should be present
        assert!(svg.contains("app_users"));
        assert!(svg.contains("app_orders"));
        assert!(svg.contains("app_items"));
    }

    // ---------------------------------------------------------------
    // Multi-schema diff with qualified names
    // ---------------------------------------------------------------

    #[test]
    fn test_diff_svg_multi_schema_qualified_names() {
        let before = "\
            CREATE SCHEMA public;\n\
            CREATE TABLE public.users (id INT PRIMARY KEY);\n\
            CREATE SCHEMA audit;\n\
            CREATE TABLE audit.logs (id INT PRIMARY KEY);\n\
        ";
        let after = "\
            CREATE SCHEMA public;\n\
            CREATE TABLE public.users (id INT PRIMARY KEY, name VARCHAR(255));\n\
            CREATE SCHEMA audit;\n\
            CREATE TABLE audit.logs (id INT PRIMARY KEY, action VARCHAR(50));\n\
            CREATE TABLE audit.events (id INT PRIMARY KEY);\n\
        ";

        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            ..Default::default()
        };
        let result = diff(request).unwrap();

        let svg = result.rendered.as_deref().expect("SVG output expected");
        assert!(svg.contains("<svg"));
        // Both modified tables → modified markings
        assert!(svg.contains("diff-modified"), "modified tables overlay");
        // Added events → added marking
        assert!(svg.contains("diff-added"), "added table overlay");
    }

    #[test]
    fn test_diff_svg_multi_schema_filter_by_schema_name() {
        let before = "\
            CREATE SCHEMA sales;\n\
            CREATE TABLE sales.orders (id INT PRIMARY KEY);\n\
            CREATE SCHEMA hr;\n\
            CREATE TABLE hr.employees (id INT PRIMARY KEY);\n\
        ";
        let after = "\
            CREATE SCHEMA sales;\n\
            CREATE TABLE sales.orders (id INT PRIMARY KEY, total DECIMAL);\n\
            CREATE SCHEMA hr;\n\
            CREATE TABLE hr.employees (id INT PRIMARY KEY, name VARCHAR(255));\n\
            CREATE TABLE hr.departments (id INT PRIMARY KEY);\n\
        ";

        // Filter to only show sales schema tables
        let request = DiffRequest {
            before: crate::request::InputSource::sql_text(before),
            after: crate::request::InputSource::sql_text(after),
            format: crate::request::DiffFormat::Svg,
            filter: relune_core::FilterSpec {
                include: vec!["sales.*".to_string()],
                exclude: vec![],
            },
            ..Default::default()
        };
        let result = diff(request).unwrap();

        // Diff data captures all changes regardless of filter
        assert_eq!(result.diff.modified_tables.len(), 2);
        assert_eq!(result.diff.added_tables.len(), 1);

        let svg = result.rendered.as_deref().expect("SVG output expected");
        assert!(svg.contains("<svg"));
        // Modified sales.orders should be visible
        assert!(svg.contains("diff-modified"), "modified orders overlay");
        // hr tables should be filtered out
        assert!(
            !svg.contains("employees"),
            "hr.employees should be excluded"
        );
        assert!(
            !svg.contains("departments"),
            "hr.departments should be excluded"
        );
    }

    // ---------------------------------------------------------------
    // Overlay correctness: verify overlay data matches diff data
    // ---------------------------------------------------------------

    #[test]
    fn test_build_diff_overlay_keeps_parallel_relationships_apart() {
        // Two FKs from posts to users: author_id stays, editor_id becomes
        // reviewer_id. Each line must keep its own change kind.
        let before = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE posts (\
                id INT PRIMARY KEY,\
                author_id INT REFERENCES users(id),\
                editor_id INT REFERENCES users(id)\
            );\n\
        ";
        let after = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE posts (\
                id INT PRIMARY KEY,\
                author_id INT REFERENCES users(id),\
                reviewer_id INT REFERENCES users(id)\
            );\n\
        ";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff, &[]);

        assert!(
            overlay
                .edge("posts", "users", &["author_id"], &["id"])
                .is_none()
        );
        assert_eq!(
            overlay
                .edge("posts", "users", &["editor_id"], &["id"])
                .and_then(|edge| edge.change),
            Some(ChangeKind::Removed)
        );
        assert_eq!(
            overlay
                .edge("posts", "users", &["reviewer_id"], &["id"])
                .and_then(|edge| edge.change),
            Some(ChangeKind::Added)
        );
    }

    #[test]
    fn test_build_diff_overlay_tells_apart_repointed_target_columns() {
        let before = "\
            CREATE TABLE parent (id INT PRIMARY KEY, code INT UNIQUE);\n\
            CREATE TABLE child (id INT PRIMARY KEY, ref INT REFERENCES parent(id));\n\
        ";
        let after = "\
            CREATE TABLE parent (id INT PRIMARY KEY, code INT UNIQUE);\n\
            CREATE TABLE child (id INT PRIMARY KEY, ref INT REFERENCES parent(code));\n\
        ";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff, &[]);

        let change = |to_column: &str| {
            overlay
                .edge("child", "parent", &["ref"], &[to_column])
                .and_then(|edge| edge.change)
        };
        assert_eq!(change("id"), Some(ChangeKind::Removed));
        assert_eq!(change("code"), Some(ChangeKind::Added));
    }

    #[test]
    fn test_build_diff_overlay_fk_edge_annotations() {
        let before = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE posts (id INT PRIMARY KEY, user_id INT REFERENCES users(id));\n\
            CREATE TABLE tags (id INT PRIMARY KEY);\n\
        ";
        let after = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE posts (id INT PRIMARY KEY, user_id INT);\n\
            CREATE TABLE tags (id INT PRIMARY KEY, user_id INT REFERENCES users(id));\n\
        ";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff, &[]);

        // posts→users edge removed
        let post_user_edge = overlay.edge("posts", "users", &["user_id"], &["id"]);
        assert!(
            post_user_edge.is_some(),
            "posts→users removed FK should be annotated"
        );
        assert_eq!(
            post_user_edge.unwrap().change,
            Some(ChangeKind::Removed),
            "posts→users should be marked removed"
        );

        // tags→users edge added (via modified table)
        let tag_user_edge = overlay.edge("tags", "users", &["user_id"], &["id"]);
        assert!(
            tag_user_edge.is_some(),
            "tags→users added FK should be annotated"
        );
    }

    #[test]
    fn test_build_diff_schema_with_added_and_removed_fks() {
        let before = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE categories (id INT PRIMARY KEY);\n\
            CREATE TABLE posts (id INT PRIMARY KEY, user_id INT REFERENCES users(id));\n\
        ";
        let after = "\
            CREATE TABLE users (id INT PRIMARY KEY);\n\
            CREATE TABLE categories (id INT PRIMARY KEY);\n\
            CREATE TABLE posts (id INT PRIMARY KEY, cat_id INT REFERENCES categories(id));\n\
        ";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(before)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(after)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let merged = build_diff_schema(&before_schema, &after_schema, &diff);
        let posts = merged.tables.iter().find(|t| t.name == "posts").unwrap();

        // Should have both the new FK (→categories) and restored old FK (→users)
        assert_eq!(
            posts.foreign_keys.len(),
            2,
            "merged schema should contain both added and removed FKs"
        );
    }

    #[test]
    fn test_format_diff_markdown_no_changes() {
        let result = DiffResult {
            diff: relune_core::SchemaDiff::default(),
            diagnostics: vec![],
            rendered: None,
        };

        let md = format_diff_markdown(&result);
        assert!(md.contains("No schema changes detected"));
    }

    #[test]
    fn test_format_diff_markdown_with_changes() {
        let before = "CREATE TABLE users (id INT PRIMARY KEY);";
        let after = "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));\nCREATE TABLE posts (id INT PRIMARY KEY);";

        let result = diff(DiffRequest::from_sql(before, after)).unwrap();
        let md = format_diff_markdown(&result);

        // Summary header
        assert!(md.contains("## Schema Diff:"));
        // GFM table
        assert!(md.contains("| Category | Added | Removed | Modified |"));
        assert!(md.contains("| Tables |"));
        // Added table
        assert!(md.contains("### Added tables"));
        assert!(md.contains("- `posts`"));
        // Modified table with details
        assert!(md.contains("<details>"));
        assert!(md.contains("<code>users</code>"));
        assert!(md.contains("- `+` `name`\n"));
    }

    #[test]
    fn test_format_diff_markdown_with_view_and_enum_changes() {
        let before = "\
            CREATE TYPE status AS ENUM ('draft', 'published');\n\
            CREATE TABLE users (id INT PRIMARY KEY, status status);\n\
            CREATE VIEW active_users AS SELECT id, status FROM users;\n\
        ";
        let after = "\
            CREATE TYPE status AS ENUM ('published', 'draft');\n\
            CREATE TABLE users (id INT PRIMARY KEY, status TEXT);\n\
            CREATE VIEW active_users AS SELECT id FROM users;\n\
        ";

        let result = diff(DiffRequest::from_sql(before, after)).unwrap();
        let md = format_diff_markdown(&result);

        assert!(md.contains("### Modified views"));
        assert!(md.contains("active_users"));
        assert!(md.contains("### Modified enums"));
        assert!(md.contains("status"));
    }

    #[test]
    fn test_format_diff_markdown_escapes_special_chars() {
        use relune_core::SchemaDiff;
        use relune_core::diff::{ColumnDiff, DiffSummary, TableDiff};

        let mut diff_result = SchemaDiff::default();
        diff_result
            .added_tables
            .push("table|with<pipe&amp".to_string());
        diff_result.modified_tables.push(TableDiff {
            stable_id: "t<able".to_string(),
            table_name: "t<able".to_string(),
            change_kind: ChangeKind::Modified,
            column_diffs: vec![ColumnDiff {
                column_name: "col|name".to_string(),
                change_kind: ChangeKind::Added,
                old_value: None,
                new_value: None,
            }],
            fk_diffs: vec![],
            index_diffs: vec![],
            check_diffs: vec![],
        });
        diff_result.summary = DiffSummary {
            tables_added: 1,
            tables_modified: 1,
            columns_changed: 1,
            ..Default::default()
        };

        let result = DiffResult {
            diff: diff_result,
            diagnostics: vec![],
            rendered: None,
        };

        let md = format_diff_markdown(&result);

        // Added table in bullet list: rendered as a code span, no HTML escaping
        assert!(
            md.contains("- `table|with<pipe&amp`"),
            "added table name should be rendered as a code span"
        );
        // Table name in <summary><code>: HTML-escaped
        assert!(
            md.contains("<code>t&lt;able</code>"),
            "<code> in <summary> requires HTML escaping"
        );
        // Column name inside <details> body: rendered as a code span
        assert!(
            md.contains("- `+` `col|name`\n"),
            "column name in <details> body should be rendered as a code span"
        );
    }
}
