//! Inspect use case implementation.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write;

use crate::error::AppError;
use crate::request::InspectRequest;
use crate::result::{InspectResult, SchemaSummary, TableDetails};
use crate::schema_input::schema_from_input;
use relune_core::Schema;
use relune_core::diagnostic::codes;

/// Execute an inspect request.
#[allow(clippy::needless_pass_by_value)]
pub fn inspect(request: InspectRequest) -> Result<InspectResult, AppError> {
    // Parse input
    let (schema, diagnostics) = schema_from_input(&request.input)?;

    // Build summary
    let summary = SchemaSummary::from(&schema);

    // Get table details if requested
    let table = if let Some(table_name) = &request.table {
        Some(find_table_details(&schema, table_name)?)
    } else {
        None
    };

    Ok(InspectResult {
        summary,
        table,
        diagnostics,
    })
}

/// Format inspect result as text.
#[must_use]
pub fn format_inspect_text(result: &InspectResult) -> String {
    let mut output = String::new();

    // Format summary
    let _ = writeln!(
        output,
        "Schema Summary:\n\
         ==============\n\
         Tables: {}\n\
         Columns: {}\n\
         Foreign Keys: {}\n\
         Indexes: {}\n\
         Views: {}\n\
         Enums: {}\n",
        result.summary.table_count,
        result.summary.column_count,
        result.summary.foreign_key_count,
        result.summary.index_count,
        result.summary.view_count,
        result.summary.enum_count
    );

    // Format table list
    if !result.summary.tables.is_empty() {
        output.push_str("Tables:\n");
        output.push_str("-------\n");
        for table in &result.summary.tables {
            let mut badges = Vec::new();
            if table.has_primary_key {
                badges.push("PK");
            }
            let fk_info = format!(
                "{} out, {} in",
                table.foreign_key_count, table.incoming_fk_count
            );
            let _ = writeln!(
                output,
                "  {:30} {:>3} cols  FKs({})  {:>2} idx{}",
                table.name,
                table.column_count,
                fk_info,
                table.index_count,
                if badges.is_empty() {
                    String::new()
                } else {
                    format!("  [{}]", badges.join(", "))
                },
            );
        }
        output.push('\n');
    }

    // Exploration hints for large schemas
    format_exploration_hints(&result.summary, &mut output);

    // Diagnostics summary
    format_diagnostics_summary(&result.diagnostics, &mut output);

    // Format table details if present
    if let Some(ref table) = result.table {
        let _ = write!(output, "{}", format_table_details(table));
    }

    // Navigation hints
    format_navigation_hints(result, &mut output);

    output
}

/// Format exploration hints (hub tables, orphans, missing PKs).
fn format_exploration_hints(summary: &SchemaSummary, output: &mut String) {
    // Only show hints when there are enough tables.
    if summary.tables.len() < 3 {
        return;
    }

    let mut has_section = false;
    let ensure_section = |out: &mut String, started: &mut bool| {
        if !*started {
            out.push_str("Highlights:\n");
            out.push_str("----------\n");
            *started = true;
        }
    };

    // Hub tables: top tables by total connections, show up to 5.
    let mut by_connections: Vec<_> = summary
        .tables
        .iter()
        .filter(|t| t.total_connections() > 0)
        .collect();
    by_connections.sort_by_key(|t| Reverse(t.total_connections()));
    let hubs: Vec<_> = by_connections
        .iter()
        .take(5)
        .filter(|t| t.total_connections() >= 2)
        .collect();
    if !hubs.is_empty() {
        ensure_section(output, &mut has_section);
        let _ = writeln!(output, "  Hub tables (most FK connections):");
        for t in &hubs {
            let _ = writeln!(
                output,
                "    {} ({} out, {} in = {} total)",
                t.name,
                t.foreign_key_count,
                t.incoming_fk_count,
                t.total_connections()
            );
        }
    }

    // Tables without PK.
    if summary.tables_without_pk > 0 {
        ensure_section(output, &mut has_section);
        let names: Vec<_> = summary
            .tables
            .iter()
            .filter(|t| !t.has_primary_key)
            .map(|t| t.name.as_str())
            .collect();
        let _ = writeln!(
            output,
            "  Tables without primary key ({}): {}",
            summary.tables_without_pk,
            names.join(", ")
        );
    }

    // Orphan tables.
    if summary.orphan_table_count > 0 {
        ensure_section(output, &mut has_section);
        let names: Vec<_> = summary
            .tables
            .iter()
            .filter(|t| t.foreign_key_count == 0 && t.incoming_fk_count == 0)
            .map(|t| t.name.as_str())
            .collect();
        let _ = writeln!(
            output,
            "  Isolated tables (no FK connections, {}): {}",
            summary.orphan_table_count,
            names.join(", ")
        );
    }

    if has_section {
        output.push('\n');
    }
}

/// Format diagnostics grouped by category and statement kind.
fn format_diagnostics_summary(diagnostics: &[relune_core::Diagnostic], output: &mut String) {
    if diagnostics.is_empty() {
        return;
    }

    // Separate unsupported/skipped (aggregate by kind) from other diagnostics.
    let unsupported_code = codes::parse_unsupported();
    let skipped_code = codes::parse_skipped();

    let mut unsupported_kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut skipped_kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut other: Vec<&relune_core::Diagnostic> = Vec::new();

    for d in diagnostics {
        if d.code == unsupported_code {
            let kind = extract_construct_name(&d.message);
            *unsupported_kinds.entry(kind).or_insert(0) += 1;
        } else if d.code == skipped_code {
            let kind = extract_construct_name(&d.message);
            *skipped_kinds.entry(kind).or_insert(0) += 1;
        } else {
            other.push(d);
        }
    }

    output.push_str("Diagnostics:\n");
    output.push_str("-----------\n");

    // Unsupported constructs — grouped by statement kind.
    if !unsupported_kinds.is_empty() {
        let total: usize = unsupported_kinds.values().sum();
        let _ = writeln!(
            output,
            "  Unsupported SQL constructs ({total} total, skipped):"
        );
        for (kind, count) in &unsupported_kinds {
            if *count == 1 {
                let _ = writeln!(output, "    {kind}");
            } else {
                let _ = writeln!(output, "    {kind} ({count})");
            }
        }
    }

    // Skipped DML — grouped by statement kind.
    if !skipped_kinds.is_empty() {
        let total: usize = skipped_kinds.values().sum();
        let _ = writeln!(output, "  Skipped DML statements ({total} total, non-DDL):");
        for (kind, count) in &skipped_kinds {
            if *count == 1 {
                let _ = writeln!(output, "    {kind}");
            } else {
                let _ = writeln!(output, "    {kind} ({count})");
            }
        }
    }

    // Other diagnostics listed individually.
    if !other.is_empty() {
        // Sort by severity descending.
        let mut other = other;
        other.sort_by(|a, b| b.severity.cmp(&a.severity));
        for d in &other {
            let _ = writeln!(output, "  [{}] {}: {}", d.severity, d.code, d.message);
        }
    }

    output.push('\n');
}

/// Extract the construct/statement name from a diagnostic message.
///
/// Handles patterns like:
///   "Unsupported SQL construct: CREATE FUNCTION. This statement will be skipped."
///   "Skipped DML statement: INSERT. Only DDL statements are processed."
///   `"Unsupported SQL construct: CreateSchema { schema_name... This statement..."`
fn extract_construct_name(message: &str) -> String {
    // Try "construct: <NAME>." or "statement: <NAME>."
    if let Some(after_colon) = message.split(": ").nth(1)
        && let Some(name) = after_colon.split('.').next()
    {
        // Trim debug-format noise: cut at first '{' or '(' from Rust Debug output.
        let trimmed = name.split(['{', '(']).next().unwrap_or(name).trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    // Fallback: use whole message truncated.
    message.chars().take(60).collect()
}

/// Format navigation hints at the end of output.
fn format_navigation_hints(result: &InspectResult, output: &mut String) {
    // Only show when we printed the summary (not table detail mode).
    if result.table.is_some() {
        return;
    }
    if result.summary.tables.is_empty() {
        return;
    }

    output.push_str("Next steps:\n");
    output.push_str("----------\n");

    // Suggest inspecting a hub table.
    let mut by_conn: Vec<_> = result
        .summary
        .tables
        .iter()
        .filter(|t| t.total_connections() > 0)
        .collect();
    by_conn.sort_by_key(|t| Reverse(t.total_connections()));
    if let Some(hub) = by_conn.first() {
        let _ = writeln!(
            output,
            "  Drill into a table:  relune inspect --table {}  ...",
            hub.name
        );
    } else {
        let _ = writeln!(
            output,
            "  Drill into a table:  relune inspect --table <name>  ..."
        );
    }
    let _ = writeln!(
        output,
        "  Visualize schema:    relune render ...  or  relune viewer ..."
    );
    let _ = writeln!(output, "  Export schema data:   relune export ...");
    output.push('\n');
}

/// Format table details as text.
fn format_table_details(table: &TableDetails) -> String {
    let mut output = String::new();

    let _ = writeln!(output, "Table: {}", table.name);
    let _ = writeln!(output, "{}", "=".repeat(60));

    if let Some(ref comment) = table.comment {
        let _ = writeln!(output, "Comment: {comment}\n");
    }

    output.push_str("Columns:\n");
    for col in &table.columns {
        let mut attrs = Vec::new();
        if col.is_primary_key {
            attrs.push("PK");
        }
        if col.nullable {
            attrs.push("NULL");
        } else {
            attrs.push("NOT NULL");
        }
        let attr_str = attrs.join(", ");
        let _ = writeln!(
            output,
            "  {:30} {:20} [{}]",
            col.name, col.data_type, attr_str
        );
        if let Some(ref comment) = col.comment {
            let _ = writeln!(output, "    └─ {comment}");
        }
    }

    if !table.foreign_keys.is_empty() {
        output.push_str("\nForeign Keys:\n");
        for fk in &table.foreign_keys {
            let name = fk.name.as_deref().unwrap_or("(unnamed)");
            let target = match &fk.to_schema {
                Some(schema) => format!("{}.{}", schema, fk.to_table),
                None => fk.to_table.clone(),
            };
            let _ = writeln!(
                output,
                "  {name} -> {target} ({})",
                fk.from_columns.join(", ")
            );
        }
    }

    if !table.indexes.is_empty() {
        output.push_str("\nIndexes:\n");
        for idx in &table.indexes {
            let name = idx.name.as_deref().unwrap_or("(unnamed)");
            let unique = if idx.is_unique { " [UNIQUE]" } else { "" };
            let _ = writeln!(output, "  {name} ({}){unique}", idx.columns.join(", "));
        }
    }

    output
}

/// Find and return details for a specific table.
fn find_table_details(schema: &Schema, table_name: &str) -> Result<TableDetails, AppError> {
    // Try to find by exact match first
    let table = schema.tables.iter().find(|t| {
        t.qualified_name() == table_name || t.name == table_name || t.stable_id == table_name
    });

    match table {
        Some(t) => Ok(TableDetails::from(t)),
        None => Err(AppError::table_not_found(table_name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inspect_summary() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255) NOT NULL
            );
            CREATE TABLE posts (
                id INT PRIMARY KEY,
                user_id INT REFERENCES users(id),
                title VARCHAR(255)
            );
        ";

        let request = InspectRequest::from_sql(sql);
        let result = inspect(request).unwrap();

        assert_eq!(result.summary.table_count, 2);
        assert_eq!(result.summary.column_count, 5);
        assert_eq!(result.summary.foreign_key_count, 1);
    }

    #[test]
    fn test_inspect_multi_schema_incoming_fk() {
        let sql = r"
            CREATE SCHEMA public;
            CREATE SCHEMA auth;
            CREATE TABLE public.users (
                id INT PRIMARY KEY,
                name VARCHAR(255)
            );
            CREATE TABLE auth.users (
                id INT PRIMARY KEY,
                email VARCHAR(255)
            );
            CREATE TABLE auth.sessions (
                id INT PRIMARY KEY,
                user_id INT REFERENCES auth.users(id)
            );
        ";

        let request = InspectRequest::from_sql(sql);
        let result = inspect(request).unwrap();

        // auth.sessions -> auth.users: only auth.users should get 1 incoming FK.
        let pub_users = result
            .summary
            .tables
            .iter()
            .find(|t| t.name == "public.users")
            .unwrap();
        assert_eq!(
            pub_users.incoming_fk_count, 0,
            "public.users should have 0 incoming FKs"
        );

        let auth_users = result
            .summary
            .tables
            .iter()
            .find(|t| t.name == "auth.users")
            .unwrap();
        assert_eq!(
            auth_users.incoming_fk_count, 1,
            "auth.users should have 1 incoming FK"
        );
    }

    #[test]
    fn test_inspect_specific_table() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255) NOT NULL
            );
        ";

        let request = InspectRequest::from_sql(sql).with_table("users");
        let result = inspect(request).unwrap();

        assert!(result.table.is_some());
        let table = result.table.unwrap();
        assert_eq!(table.name, "users");
        assert_eq!(table.columns.len(), 2);
        assert!(table.columns[0].is_primary_key);
        assert!(!table.columns[1].nullable);
    }

    #[test]
    fn test_inspect_table_not_found() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
        ";

        let request = InspectRequest::from_sql(sql).with_table("nonexistent");
        let result = inspect(request);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_format_inspect_text() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255)
            );
        ";

        let request = InspectRequest::from_sql(sql);
        let result = inspect(request).unwrap();
        let text = format_inspect_text(&result);

        assert!(text.contains("Schema Summary"));
        assert!(text.contains("Tables: 1"));
        assert!(text.contains("users"));
    }

    #[test]
    fn test_format_table_details() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255) NOT NULL,
                email VARCHAR(255)
            );
        ";

        let request = InspectRequest::from_sql(sql).with_table("users");
        let result = inspect(request).unwrap();
        let text = format_inspect_text(&result);

        assert!(text.contains("Table: users"));
        assert!(text.contains("id"));
        assert!(text.contains("PK"));
        assert!(text.contains("name"));
        assert!(text.contains("email"));
    }
}
