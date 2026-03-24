//! Relune application orchestration layer.
//!
//! This crate provides the main integration API for relune, coordinating
//! parsing, layout, and rendering operations.

//! # Overview
//!
//! The main entry points are:
//! - [`render`] - Render an ERD from SQL or schema JSON
//! - [`inspect`] - Inspect schema metadata or specific table details
//! - [`export`] - Export schema or graph data as JSON
//!
//! # Example
//!
//! ```rust,no_run
//! use relune_app::{render, RenderRequest, OutputFormat};
//!
//! let sql = r#"
//!     CREATE TABLE users (
//!         id INT PRIMARY KEY,
//!         name VARCHAR(255)
//!     );
//! "#;
//!
//! let request = RenderRequest::from_sql(sql)
//!     .with_output_format(OutputFormat::Svg);
//!
//! let result = render(request).unwrap();
//! println!("{}", result.content);
//! ```

pub mod error;
pub mod request;
pub mod result;
mod schema_input;
pub mod usecases;

// Re-export request types
pub use request::{
    DiffFormat, DiffRequest, ExportFormat, ExportRequest, InputSource, InspectFormat,
    InspectRequest, LintFormat, LintRequest, OutputFormat, RenderOptions, RenderRequest,
    RenderTheme,
};

// Re-export result types
pub use result::{
    ColumnDetails, DiffResult, ExportResult, ForeignKeyDetails, IndexDetails, InspectResult,
    LintResult, RenderResult, RenderStats, SchemaSummary, TableDetails, TableSummary,
};

// Re-export error type
pub use error::AppError;

// Re-export use case functions
pub use usecases::{diff, export, inspect, lint, render};

// Re-export from usecases for convenience
pub use usecases::diff::format_diff_text;
pub use usecases::inspect::format_inspect_text;
pub use usecases::lint::{format_lint_json, format_lint_text};

// Re-export commonly used types from relune-core for convenience
pub use relune_core::{
    FilterSpec, FocusSpec, GroupingSpec, GroupingStrategy, LayoutAlgorithm, LayoutDirection,
    LayoutSpec, RouteStyle,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_from_sql_basic() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255) NOT NULL
            );
        ";

        let request = RenderRequest::from_sql(sql);
        let result = render(request);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.content.contains("<svg"));
        assert_eq!(result.stats.table_count, 1);
        assert_eq!(result.stats.column_count, 2);
    }

    #[test]
    fn test_render_with_focus() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
            CREATE TABLE posts (
                id INT PRIMARY KEY,
                user_id INT REFERENCES users(id)
            );
            CREATE TABLE comments (
                id INT PRIMARY KEY,
                post_id INT REFERENCES posts(id)
            );
        ";

        let focus = FocusSpec {
            table: "posts".to_string(),
            depth: 1,
        };

        let request = RenderRequest::from_sql(sql).with_focus(focus);

        let result = render(request).unwrap();
        // Focus should include posts, users (referenced), and comments (references posts)
        assert!(result.stats.table_count >= 2);
    }

    #[test]
    fn test_inspect_schema_summary() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255)
            );
            CREATE TABLE posts (
                id INT PRIMARY KEY,
                user_id INT REFERENCES users(id)
            );
        ";

        let request = InspectRequest::from_sql(sql);
        let result = inspect(request).unwrap();

        assert_eq!(result.summary.table_count, 2);
        assert_eq!(result.summary.column_count, 4);
        assert_eq!(result.summary.foreign_key_count, 1);
    }

    #[test]
    fn test_inspect_table_details() {
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
    fn test_export_schema_json() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
        ";

        let request = ExportRequest::from_sql(sql).with_format(ExportFormat::SchemaJson);
        let result = export(request).unwrap();

        assert!(result.content.contains("\"tables\""));
        assert!(result.content.contains("users"));
    }

    #[test]
    fn test_export_graph_json() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
        ";

        let request = ExportRequest::from_sql(sql).with_format(ExportFormat::GraphJson);
        let result = export(request).unwrap();

        assert!(result.content.contains("\"nodes\""));
        assert!(result.content.contains("\"edges\""));
    }

    #[test]
    fn test_format_inspect_output() {
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
    fn test_output_formats() {
        let sql = "CREATE TABLE test (id INT PRIMARY KEY);";

        // Test SVG
        let request = RenderRequest::from_sql(sql).with_output_format(OutputFormat::Svg);
        let result = render(request).unwrap();
        assert!(result.content.contains("<svg"));

        // Test HTML
        let request = RenderRequest::from_sql(sql).with_output_format(OutputFormat::Html);
        let result = render(request).unwrap();
        assert!(result.content.contains("<!DOCTYPE html>"));

        // Test GraphJson
        let request = RenderRequest::from_sql(sql).with_output_format(OutputFormat::GraphJson);
        let result = render(request).unwrap();
        assert!(result.content.contains("\"nodes\""));

        // Test SchemaJson
        let request = RenderRequest::from_sql(sql).with_output_format(OutputFormat::SchemaJson);
        let result = render(request).unwrap();
        assert!(result.content.contains("\"tables\""));
    }

    #[test]
    fn test_render_with_filter() {
        // Note: Filter functionality affects graph building, not the full schema stats
        // The filter is applied during layout graph construction
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
            CREATE TABLE posts (
                id INT PRIMARY KEY
            );
            CREATE TABLE comments (
                id INT PRIMARY KEY
            );
        ";

        let filter = FilterSpec {
            include: vec!["users".to_string(), "posts".to_string()],
            exclude: vec![],
        };

        let request = RenderRequest::from_sql(sql).with_filter(filter);
        let result = render(request).unwrap();

        // Render should succeed
        assert!(result.content.contains("<svg"));
    }

    #[test]
    fn test_render_with_grouping() {
        let sql = r"
            CREATE TABLE app_users (
                id INT PRIMARY KEY
            );
            CREATE TABLE app_posts (
                id INT PRIMARY KEY
            );
        ";

        let grouping = GroupingSpec {
            strategy: GroupingStrategy::ByPrefix,
        };

        let request = RenderRequest::from_sql(sql).with_grouping(grouping);
        let result = render(request).unwrap();

        assert!(result.content.contains("<svg"));
    }
}
