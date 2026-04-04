//! Diff use case implementation.

use std::collections::HashMap;
use std::fmt::Write;

use relune_core::diff::TableDiff;
use relune_core::{ChangeKind, Schema, Table, diff_schemas};
use relune_layout::{Annotation, DiagramOverlay, OverlaySeverity};

use crate::error::AppError;
use crate::request::DiffRequest;
use crate::result::DiffResult;
use crate::schema_input::schema_from_input;

/// Execute a diff request.
#[allow(clippy::needless_pass_by_value)]
pub fn diff(request: DiffRequest) -> Result<DiffResult, AppError> {
    use crate::request::DiffFormat;

    // Step 1: Resolve schemas
    let (before_schema, mut diagnostics) = schema_from_input(&request.before)?;
    let (after_schema, after_diagnostics) = schema_from_input(&request.after)?;
    diagnostics.extend(after_diagnostics);

    // Step 2: Compute diff
    let schema_diff = diff_schemas(&before_schema, &after_schema);

    // Step 3: Render visual output if requested
    let rendered = match request.format {
        DiffFormat::Svg | DiffFormat::Html => {
            let content =
                render_diff_visual(&before_schema, &after_schema, &schema_diff, &request)?;
            Some(content)
        }
        DiffFormat::Text | DiffFormat::Json => None,
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
    request: &DiffRequest,
) -> Result<String, AppError> {
    use crate::request::{DiffFormat, OutputFormat, RenderRequest};

    let merged = build_diff_schema(before, after, schema_diff);
    let overlay = build_diff_overlay(before, after, schema_diff);

    let output_format = match request.format {
        DiffFormat::Svg => OutputFormat::Svg,
        DiffFormat::Html => OutputFormat::Html,
        _ => unreachable!(),
    };

    let render_request = RenderRequest {
        input: crate::request::InputSource::default(),
        output_format,
        filter: request.filter.clone(),
        focus: None,
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
    use relune_layout::{
        FocusExtractor, LayoutConfig, LayoutGraphBuilder, build_layout_from_graph_with_config,
    };
    use relune_render_html::{HtmlRenderOptions, Theme as HtmlTheme};
    use relune_render_svg::{SvgRenderOptions, Theme as SvgTheme, render_svg_with_overlay};

    use crate::request::{OutputFormat, RenderTheme};

    let layout_config = LayoutConfig::from(&request.layout);
    let mut graph = LayoutGraphBuilder::new()
        .filter(request.filter.clone())
        .focus(request.focus.clone())
        .grouping(request.grouping)
        .build(schema);
    if let Some(ref focus) = request.focus {
        graph = FocusExtractor
            .extract(&graph, focus)
            .map_err(relune_layout::LayoutError::from)?;
    }

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
        compact: false,
        show_tooltips: true,
    };
    let svg = render_svg_with_overlay(&positioned, svg_options, request.overlay.as_ref());

    match request.output_format {
        OutputFormat::Svg => Ok(svg),
        OutputFormat::Html => {
            let html_theme = match request.options.theme {
                RenderTheme::Light => HtmlTheme::Light,
                RenderTheme::Dark => HtmlTheme::Dark,
            };
            let html_options = HtmlRenderOptions {
                theme: html_theme,
                include_legend: request.options.show_legend || request.options.show_stats,
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
            let change_count = table_diff.column_diffs.len()
                + table_diff.fk_diffs.len()
                + table_diff.index_diffs.len();
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
        }
    }

    // Summary
    let _ = writeln!(
        output,
        "\nSummary: {} table(s) added, {} removed, {} modified",
        summary.tables_added, summary.tables_removed, summary.tables_modified
    );
    let _ = writeln!(
        output,
        "         {} column change(s), {} FK change(s), {} index change(s)",
        summary.columns_changed, summary.foreign_keys_changed, summary.indexes_changed
    );

    output
}

/// Build a merged schema that contains the union of both schemas for diff visualization.
///
/// The merged schema includes:
/// - All tables from `after` (current state)
/// - Removed tables from `before` (so they appear ghosted in the diagram)
/// - For modified tables with removed FKs, the removed FKs are added back so that
///   removed edges are visible in the diagram.
#[must_use]
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

    // Start with all after tables
    let mut tables: Vec<Table> = after.tables.clone();

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

    Schema {
        tables,
        views: after.views.clone(),
        enums: after.enums.clone(),
    }
}

/// Build a [`DiagramOverlay`] from a [`SchemaDiff`] for diff visualization.
///
/// Annotates nodes and edges with their diff status:
/// - Added tables/edges → `Info` severity, `rule_id = "diff-added"`
/// - Removed tables/edges → `Error` severity, `rule_id = "diff-removed"`
/// - Modified tables → `Warning` severity, `rule_id = "diff-modified"`
#[must_use]
pub fn build_diff_overlay(
    before: &Schema,
    after: &Schema,
    diff: &relune_core::SchemaDiff,
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

    // Annotate added tables
    for table_name in &diff.added_tables {
        if let Some(table) = after_by_name.get(table_name) {
            annotate_table_node(
                &mut overlay,
                table,
                OverlaySeverity::Info,
                "diff-added",
                "Added",
            );
            annotate_table_edges(
                &mut overlay,
                table,
                after,
                OverlaySeverity::Info,
                "diff-added",
            );
        }
    }

    // Annotate removed tables
    for table_name in &diff.removed_tables {
        if let Some(table) = before_by_name.get(table_name) {
            annotate_table_node(
                &mut overlay,
                table,
                OverlaySeverity::Error,
                "diff-removed",
                "Removed",
            );
            annotate_table_edges(
                &mut overlay,
                table,
                before,
                OverlaySeverity::Error,
                "diff-removed",
            );
        }
    }

    // Annotate modified tables
    for table_diff in &diff.modified_tables {
        let stable_id = after_by_name
            .get(&table_diff.table_name)
            .or_else(|| before_by_name.get(&table_diff.table_name))
            .map(|t| t.stable_id.as_str());

        if let Some(stable_id) = stable_id {
            annotate_modified_table(&mut overlay, stable_id, table_diff, before, after);
        }
    }

    overlay
}

fn annotate_table_node(
    overlay: &mut DiagramOverlay,
    table: &Table,
    severity: OverlaySeverity,
    rule_id: &str,
    label: &str,
) {
    let col_count = table.columns.len();
    overlay.add_node_annotation(
        &table.stable_id,
        Annotation {
            severity,
            message: format!("{label} table ({col_count} columns)"),
            hint: None,
            rule_id: Some(rule_id.to_string()),
        },
    );
}

fn annotate_table_edges(
    overlay: &mut DiagramOverlay,
    table: &Table,
    schema: &Schema,
    severity: OverlaySeverity,
    rule_id: &str,
) {
    let label = match severity {
        OverlaySeverity::Info => "Added",
        OverlaySeverity::Error => "Removed",
        _ => "Changed",
    };
    for fk in &table.foreign_keys {
        let target_id = resolve_fk_target_stable_id(schema, fk.to_schema.as_deref(), &fk.to_table);
        let to_id = target_id.as_deref().unwrap_or(&fk.to_table);
        overlay.add_edge_annotation(
            &table.stable_id,
            to_id,
            Annotation {
                severity,
                message: format!("{label} relationship"),
                hint: None,
                rule_id: Some(rule_id.to_string()),
            },
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

fn annotate_modified_table(
    overlay: &mut DiagramOverlay,
    stable_id: &str,
    table_diff: &TableDiff,
    before: &Schema,
    after: &Schema,
) {
    let mut details = Vec::new();
    for col in &table_diff.column_diffs {
        details.push(format!(
            "{} {}",
            change_indicator(col.change_kind),
            col.column_name
        ));
    }
    for fk in &table_diff.fk_diffs {
        let name = fk.name.as_deref().unwrap_or("unnamed FK");
        details.push(format!("{} {name}", change_indicator(fk.change_kind)));
    }
    for idx in &table_diff.index_diffs {
        let name = idx.name.as_deref().unwrap_or("unnamed index");
        details.push(format!("{} {name}", change_indicator(idx.change_kind)));
    }

    let change_count =
        table_diff.column_diffs.len() + table_diff.fk_diffs.len() + table_diff.index_diffs.len();
    overlay.add_node_annotation(
        stable_id,
        Annotation {
            severity: OverlaySeverity::Warning,
            message: format!("Modified ({change_count} changes)"),
            hint: if details.is_empty() {
                None
            } else {
                Some(details.join(", "))
            },
            rule_id: Some("diff-modified".to_string()),
        },
    );

    // Annotate FK edges based on diff
    for fk_diff in &table_diff.fk_diffs {
        let (severity, msg, rule_id) = match fk_diff.change_kind {
            ChangeKind::Added => (OverlaySeverity::Info, "Added relationship", "diff-added"),
            ChangeKind::Removed => (
                OverlaySeverity::Error,
                "Removed relationship",
                "diff-removed",
            ),
            ChangeKind::Modified => (
                OverlaySeverity::Warning,
                "Modified relationship",
                "diff-modified",
            ),
        };
        let fk_ref = fk_diff.new_value.as_ref().or(fk_diff.old_value.as_ref());
        let to_table = fk_ref.map_or("", |v| v.to_table.as_str());
        let to_schema = fk_ref.and_then(|v| v.to_schema.as_deref());
        if !to_table.is_empty() {
            let target_id = resolve_fk_target_stable_id(after, to_schema, to_table)
                .or_else(|| resolve_fk_target_stable_id(before, to_schema, to_table))
                .unwrap_or_else(|| to_table.to_string());
            overlay.add_edge_annotation(
                stable_id,
                &target_id,
                Annotation {
                    severity,
                    message: msg.to_string(),
                    hint: None,
                    rule_id: Some(rule_id.to_string()),
                },
            );
        }
    }
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
        let qualified = format!("{schema_name}.{to_table}");
        if let Some(t) = schema
            .tables
            .iter()
            .find(|t| t.qualified_name() == qualified)
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

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff);
        assert!(!overlay.is_empty());
        let node = overlay.node("users").expect("should have users overlay");
        assert_eq!(node.annotations[0].rule_id.as_deref(), Some("diff-added"));
        assert_eq!(node.annotations[0].severity, OverlaySeverity::Info);
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

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff);
        assert!(!overlay.is_empty());
        let node = overlay.node("users").expect("should have users overlay");
        assert_eq!(node.annotations[0].rule_id.as_deref(), Some("diff-removed"));
        assert_eq!(node.annotations[0].severity, OverlaySeverity::Error);
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

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff);
        assert!(!overlay.is_empty());
        let node = overlay.node("users").expect("should have users overlay");
        assert_eq!(
            node.annotations[0].rule_id.as_deref(),
            Some("diff-modified")
        );
        assert_eq!(node.annotations[0].severity, OverlaySeverity::Warning);
    }

    #[test]
    fn test_build_diff_overlay_no_changes() {
        let sql = "CREATE TABLE users (id INT PRIMARY KEY);";

        let (before_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(sql)).unwrap();
        let (after_schema, _) =
            schema_from_input(&crate::request::InputSource::sql_text(sql)).unwrap();
        let diff = relune_core::diff_schemas(&before_schema, &after_schema);

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff);
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
        // Added table should have overlay-info class
        assert!(svg.contains("overlay-info"));
        // Modified table should have overlay-warning class
        assert!(svg.contains("overlay-warning"));
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
        assert!(html.contains("overlay-warning"));
        // Metadata should include diff issues
        assert!(html.contains("diff-modified"));
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
        // Modified users → warning overlay
        assert!(svg.contains("overlay-warning"), "modified table overlay");
        // Added products → info overlay
        assert!(svg.contains("overlay-info"), "added table overlay");
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
        assert!(svg.contains("overlay-warning"), "users should be modified");
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
        // Removed table → error overlay
        assert!(
            svg.contains("overlay-error"),
            "removed table should have error overlay"
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
            svg.contains("overlay-warning"),
            "modified table should have warning overlay"
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
            svg.contains("overlay-warning"),
            "modified table should have warning overlay"
        );
        // Added departments → info
        assert!(
            svg.contains("overlay-info"),
            "added table should have info overlay"
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
            svg.contains("overlay-warning"),
            "modified table should have warning overlay"
        );
        // Removed sys_logs → error
        assert!(
            svg.contains("overlay-error"),
            "removed table should have error overlay"
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
            svg.contains("overlay-error"),
            "removed table should have error overlay"
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
        assert!(svg.contains("overlay-warning"), "modified table overlay");
        // Added app_tags → info
        assert!(svg.contains("overlay-info"), "added table overlay");
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
        assert!(svg.contains("overlay-warning"), "modified table overlay");
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
        // Both modified tables → warning overlays
        assert!(svg.contains("overlay-warning"), "modified tables overlay");
        // Added events → info overlay
        assert!(svg.contains("overlay-info"), "added table overlay");
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
        assert!(svg.contains("overlay-warning"), "modified orders overlay");
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

        let overlay = build_diff_overlay(&before_schema, &after_schema, &diff);

        // posts→users edge removed
        let post_user_edge = overlay.edge("posts", "users");
        assert!(
            post_user_edge.is_some(),
            "posts→users removed FK should be annotated"
        );
        assert!(
            post_user_edge
                .unwrap()
                .annotations
                .iter()
                .any(|a| a.rule_id.as_deref() == Some("diff-removed")),
            "posts→users should have diff-removed annotation"
        );

        // tags→users edge added (via modified table)
        let tag_user_edge = overlay.edge("tags", "users");
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
}
