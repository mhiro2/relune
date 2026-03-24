//! Diff use case implementation.

use std::fmt::Write;

use crate::error::AppError;
use crate::request::DiffRequest;
use crate::result::DiffResult;
use crate::schema_input::schema_from_input;
use relune_core::{ChangeKind, diff_schemas};

/// Execute a diff request.
#[allow(clippy::needless_pass_by_value)]
pub fn diff(request: DiffRequest) -> Result<DiffResult, AppError> {
    // Step 1: Resolve schemas
    let (before_schema, mut diagnostics) = schema_from_input(&request.before)?;
    let (after_schema, after_diagnostics) = schema_from_input(&request.after)?;
    diagnostics.extend(after_diagnostics);

    // Step 2: Compute diff
    let diff = diff_schemas(&before_schema, &after_schema);

    Ok(DiffResult { diff, diagnostics })
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
        };

        let text = format_diff_text(&result);
        assert!(text.contains("Added tables"));
        assert!(text.contains("new_table"));
    }
}
