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
mod markdown;
pub mod request;
pub mod result;
mod schema_input;
pub mod usecases;

// Re-export request types
pub use request::{
    DiffFormat, DiffRequest, DocFormat, DocRequest, ExportFormat, ExportRequest, InputSource,
    InspectFormat, InspectRequest, LintFormat, LintRequest, OutputFormat, RenderOptions,
    RenderRequest, RenderTheme, ReviewFormat, ReviewRequest, ReviewRuleSettings,
};

// Re-export result types
pub use result::{
    ColumnDetails, DiffResult, DocResult, ExportResult, ForeignKeyDetails, IndexDetails,
    InspectResult, LintResult, LintReview, RenderResult, RenderStats, ReviewInputCoverage,
    ReviewInputs, ReviewResult, SchemaSummary, TableDetails, TableSummary,
};

// Re-export error type
pub use error::AppError;

// Re-export use case functions
#[cfg(feature = "introspect")]
pub use schema_input::schema_from_db_url_async;
pub use schema_input::{MAX_INPUT_FILE_SIZE_BYTES, read_input_file};
pub use usecases::{diff, doc, export, inspect, lint, render, review};

// Re-export from usecases for convenience
pub use usecases::diff::{
    build_diff_overlay, build_diff_schema, format_diff_markdown, format_diff_text,
};
pub use usecases::doc::format_doc_markdown;
pub use usecases::inspect::format_inspect_text;
pub use usecases::lint::{format_lint_json, format_lint_text};
pub use usecases::review::{
    applied_rule_metadata, format_review_json, format_review_markdown, format_review_markdown_with,
    format_review_text, format_review_text_with,
};

// Re-export commonly used types from relune-core for convenience
pub use relune_core::{
    FilterSpec, FocusSpec, GroupingSpec, GroupingStrategy, LayoutAlgorithm, LayoutCompactionSpec,
    LayoutDirection, LayoutSpec, ReviewRuleMetadata, ReviewSeverityOverride, RouteStyle,
};

/// Returns whether live database introspection support is compiled in.
#[must_use]
pub const fn introspection_enabled() -> bool {
    cfg!(feature = "introspect")
}

/// Returns the live database backends supported by the current build.
#[must_use]
pub const fn supported_introspection_backends() -> &'static [&'static str] {
    if cfg!(feature = "introspect") {
        &["postgres", "mysql", "mariadb", "sqlite"]
    } else {
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            CREATE TABLE reactions (
                id INT PRIMARY KEY,
                comment_id INT REFERENCES comments(id)
            );
            CREATE TABLE settings (
                id INT PRIMARY KEY
            );
        ";

        // reactions is two hops away and settings is unconnected, so both
        // fall outside a depth-1 focus on posts.
        let focus = FocusSpec::new("posts", 1);

        let request = RenderRequest::from_sql(sql)
            .with_output_format(OutputFormat::GraphJson)
            .with_focus(focus);

        let result = render(request).unwrap();
        let graph: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        let nodes = graph["nodes"].as_array().expect("nodes array");
        let node_ids = nodes
            .iter()
            .map(|node| node["table_name"].as_str().expect("table_name"))
            .collect::<std::collections::BTreeSet<_>>();
        let edges = graph["edges"].as_array().expect("edges array");
        let edge_ids = edges
            .iter()
            .map(|edge| {
                (
                    edge["from"].as_str().expect("from"),
                    edge["to"].as_str().expect("to"),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();

        let expected_nodes = ["comments", "posts", "users"]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let expected_edges = [("comments", "posts"), ("posts", "users")]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(node_ids, expected_nodes);
        assert_eq!(edge_ids, expected_edges);
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
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
            CREATE TABLE posts (
                id INT PRIMARY KEY
                , user_id INT REFERENCES users(id)
            );
            CREATE TABLE comments (
                id INT PRIMARY KEY
            );
        ";

        let filter = FilterSpec {
            include: vec!["users".to_string(), "posts".to_string()],
            exclude: vec![],
        };

        let request = RenderRequest::from_sql(sql)
            .with_output_format(OutputFormat::GraphJson)
            .with_filter(filter);
        let result = render(request).unwrap();

        let graph: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        let nodes = graph["nodes"].as_array().expect("nodes array");
        let node_ids = nodes
            .iter()
            .map(|node| node["table_name"].as_str().expect("table_name"))
            .collect::<std::collections::BTreeSet<_>>();
        let edges = graph["edges"].as_array().expect("edges array");
        let edge_ids = edges
            .iter()
            .map(|edge| {
                (
                    edge["from"].as_str().expect("from"),
                    edge["to"].as_str().expect("to"),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();

        let expected_nodes = ["posts", "users"]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let expected_edges =
            std::iter::once(("posts", "users")).collect::<std::collections::BTreeSet<_>>();

        assert_eq!(node_ids, expected_nodes);
        assert_eq!(edge_ids, expected_edges);
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
            CREATE TABLE settings (
                id INT PRIMARY KEY
            );
        ";

        let grouping = GroupingSpec {
            strategy: GroupingStrategy::ByPrefix,
        };

        let request = RenderRequest::from_sql(sql).with_grouping(grouping);
        let result = render(request).unwrap();

        assert_eq!(
            relune_testkit::svg::group_members(&result.content),
            std::collections::BTreeMap::from([(
                "app_".to_string(),
                ["app_posts", "app_users"]
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            )])
        );
        assert!(
            relune_testkit::svg::table_ids(&result.content).contains("settings"),
            "ungrouped tables still render"
        );
    }
}
