//! Inspect use case implementation.

use std::fmt::Write;

use crate::error::AppError;
use crate::request::InspectRequest;
use crate::result::{InspectResult, SchemaSummary, TableDetails};
use crate::schema_input::schema_from_input;
use relune_core::Schema;

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
         Views: {}\n\
         Enums: {}\n",
        result.summary.table_count,
        result.summary.column_count,
        result.summary.foreign_key_count,
        result.summary.view_count,
        result.summary.enum_count
    );

    // Format table list
    if !result.summary.tables.is_empty() {
        output.push_str("Tables:\n");
        output.push_str("-------\n");
        for table in &result.summary.tables {
            let pk_indicator = if table.has_primary_key { " [PK]" } else { "" };
            let _ = writeln!(
                output,
                "  {} ({} columns, {} FKs){pk_indicator}",
                table.name, table.column_count, table.foreign_key_count
            );
        }
        output.push('\n');
    }

    // Format table details if present
    if let Some(ref table) = result.table {
        let _ = write!(output, "{}", format_table_details(table));
    }

    output
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
