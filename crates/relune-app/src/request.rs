//! Request types for the relune application layer.
//!
//! These types define the public API for requesting operations from relune-app.

use std::path::PathBuf;

use relune_core::{FilterSpec, FocusSpec, GroupingSpec, LayoutSpec, SqlDialect};
use serde::{Deserialize, Serialize};

/// Source of input schema data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputSource {
    /// SQL DDL text provided directly.
    SqlText {
        /// The SQL DDL text content.
        sql: String,
        /// SQL dialect to use for parsing.
        #[serde(default)]
        dialect: SqlDialect,
    },
    /// SQL DDL file path.
    SqlFile {
        /// Path to the SQL file.
        path: PathBuf,
        /// SQL dialect to use for parsing.
        #[serde(default)]
        dialect: SqlDialect,
    },
    /// Database connection URL for live introspection (`postgres://`, `mysql://`, `mariadb://`, `sqlite:`).
    #[cfg(feature = "introspect")]
    DbUrl {
        /// The database connection URL.
        url: String,
    },
    /// Pre-normalized schema JSON.
    SchemaJson {
        /// The JSON string content.
        json: String,
    },
    /// Pre-normalized schema JSON file.
    SchemaJsonFile {
        /// Path to the JSON file.
        path: PathBuf,
    },
}

impl Default for InputSource {
    fn default() -> Self {
        Self::SqlText {
            sql: String::new(),
            dialect: SqlDialect::default(),
        }
    }
}

impl InputSource {
    /// Create a new SQL text input.
    pub fn sql_text(sql: impl Into<String>) -> Self {
        Self::SqlText {
            sql: sql.into(),
            dialect: SqlDialect::default(),
        }
    }

    /// Create a new SQL text input with explicit dialect.
    pub fn sql_text_with_dialect(sql: impl Into<String>, dialect: SqlDialect) -> Self {
        Self::SqlText {
            sql: sql.into(),
            dialect,
        }
    }

    /// Create a new SQL file input.
    pub fn sql_file(path: impl Into<PathBuf>) -> Self {
        Self::SqlFile {
            path: path.into(),
            dialect: SqlDialect::default(),
        }
    }

    /// Create a new SQL file input with explicit dialect.
    pub fn sql_file_with_dialect(path: impl Into<PathBuf>, dialect: SqlDialect) -> Self {
        Self::SqlFile {
            path: path.into(),
            dialect,
        }
    }

    /// Create a new schema JSON input.
    pub fn schema_json(json: impl Into<String>) -> Self {
        Self::SchemaJson { json: json.into() }
    }

    /// Create a new schema JSON file input.
    pub fn schema_json_file(path: impl Into<PathBuf>) -> Self {
        Self::SchemaJsonFile { path: path.into() }
    }

    /// Create a database URL input (requires the `introspect` feature).
    #[cfg(feature = "introspect")]
    pub fn db_url(url: impl Into<String>) -> Self {
        Self::DbUrl { url: url.into() }
    }
}

/// Output format for rendering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFormat {
    /// SVG output.
    #[default]
    Svg,
    /// Self-contained HTML with embedded SVG.
    Html,
    /// Graph JSON (intermediate representation).
    GraphJson,
    /// Schema JSON (normalized export).
    SchemaJson,
}

/// Request to render an ERD diagram.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RenderRequest {
    /// Input source for the schema.
    pub input: InputSource,
    /// Output format.
    #[serde(default)]
    pub output_format: OutputFormat,
    /// Filter specification for tables.
    #[serde(default)]
    pub filter: FilterSpec,
    /// Optional focus specification.
    pub focus: Option<FocusSpec>,
    /// Grouping specification.
    #[serde(default)]
    pub grouping: GroupingSpec,
    /// Layout specification.
    #[serde(default)]
    pub layout: LayoutSpec,
    /// Optional output file path. If None, output goes to stdout.
    pub output_path: Option<PathBuf>,
}

impl RenderRequest {
    /// Create a new render request from SQL text.
    pub fn from_sql(sql: impl Into<String>) -> Self {
        Self {
            input: InputSource::sql_text(sql),
            ..Default::default()
        }
    }

    /// Set the output format.
    #[must_use]
    pub const fn with_output_format(mut self, format: OutputFormat) -> Self {
        self.output_format = format;
        self
    }

    /// Set the output file path.
    #[must_use]
    pub fn with_output_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.output_path = Some(path.into());
        self
    }

    /// Set the focus specification.
    #[must_use]
    pub fn with_focus(mut self, focus: FocusSpec) -> Self {
        self.focus = Some(focus);
        self
    }

    /// Set the filter specification.
    #[must_use]
    pub fn with_filter(mut self, filter: FilterSpec) -> Self {
        self.filter = filter;
        self
    }

    /// Set the grouping specification.
    #[must_use]
    pub const fn with_grouping(mut self, grouping: GroupingSpec) -> Self {
        self.grouping = grouping;
        self
    }

    /// Set the layout specification.
    #[must_use]
    pub const fn with_layout(mut self, layout: LayoutSpec) -> Self {
        self.layout = layout;
        self
    }
}

/// Request to inspect schema metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectRequest {
    /// Input source for the schema.
    pub input: InputSource,
    /// Optional table name to inspect. If None, returns schema summary.
    pub table: Option<String>,
    /// Output format.
    #[serde(default)]
    pub format: InspectFormat,
}

impl InspectRequest {
    /// Create a new inspect request from SQL text.
    pub fn from_sql(sql: impl Into<String>) -> Self {
        Self {
            input: InputSource::sql_text(sql),
            ..Default::default()
        }
    }

    /// Set the table to inspect.
    #[must_use]
    pub fn with_table(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }

    /// Set the output format.
    #[must_use]
    pub const fn with_format(mut self, format: InspectFormat) -> Self {
        self.format = format;
        self
    }

    /// Request a schema summary instead of table details.
    #[must_use]
    pub fn summary(mut self) -> Self {
        self.table = None;
        self
    }
}

/// Output format for inspect command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InspectFormat {
    /// Human-readable text output.
    #[default]
    Text,
    /// JSON output.
    Json,
}

/// Request to export schema or graph data.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExportRequest {
    /// Input source for the schema.
    pub input: InputSource,
    /// Export format.
    #[serde(default)]
    pub format: ExportFormat,
    /// Optional filter specification.
    #[serde(default)]
    pub filter: FilterSpec,
    /// Optional focus specification.
    pub focus: Option<FocusSpec>,
    /// Optional grouping specification.
    #[serde(default)]
    pub grouping: GroupingSpec,
    /// Layout specification for positioned exports.
    #[serde(default)]
    pub layout: LayoutSpec,
    /// Optional output file path. If None, output goes to stdout.
    pub output_path: Option<PathBuf>,
}

impl ExportRequest {
    /// Create a new export request from SQL text.
    pub fn from_sql(sql: impl Into<String>) -> Self {
        Self {
            input: InputSource::sql_text(sql),
            ..Default::default()
        }
    }

    /// Set the export format.
    #[must_use]
    pub const fn with_format(mut self, format: ExportFormat) -> Self {
        self.format = format;
        self
    }

    /// Set the output file path.
    #[must_use]
    pub fn with_output_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.output_path = Some(path.into());
        self
    }

    /// Set the focus specification.
    #[must_use]
    pub fn with_focus(mut self, focus: FocusSpec) -> Self {
        self.focus = Some(focus);
        self
    }

    /// Set the filter specification.
    #[must_use]
    pub fn with_filter(mut self, filter: FilterSpec) -> Self {
        self.filter = filter;
        self
    }

    /// Set the grouping specification.
    #[must_use]
    pub const fn with_grouping(mut self, grouping: GroupingSpec) -> Self {
        self.grouping = grouping;
        self
    }

    /// Set the layout specification.
    #[must_use]
    pub const fn with_layout(mut self, layout: LayoutSpec) -> Self {
        self.layout = layout;
        self
    }
}

/// Export format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExportFormat {
    /// Normalized schema JSON.
    #[default]
    SchemaJson,
    /// Graph JSON (intermediate representation).
    GraphJson,
    /// Layout JSON (positioned graph).
    LayoutJson,
    /// Mermaid `erDiagram` source (review / documentation).
    Mermaid,
    /// D2 diagram source.
    D2,
    /// Graphviz DOT `digraph` source.
    Dot,
}

/// Request to run lint diagnostics on a schema.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LintRequest {
    /// Input source for the schema.
    pub input: InputSource,
    /// Output format.
    #[serde(default)]
    pub format: LintFormat,
    /// Optional specific rules to run. If empty, runs all rules.
    #[serde(default)]
    pub rules: Vec<String>,
    /// Minimum severity that causes non-zero exit.
    #[serde(default)]
    pub fail_on: Option<relune_core::Severity>,
}

impl LintRequest {
    /// Create a new lint request from SQL text.
    pub fn from_sql(sql: impl Into<String>) -> Self {
        Self {
            input: InputSource::sql_text(sql),
            ..Default::default()
        }
    }

    /// Set the output format.
    #[must_use]
    pub const fn with_format(mut self, format: LintFormat) -> Self {
        self.format = format;
        self
    }

    /// Set specific rules to run.
    #[must_use]
    pub fn with_rules(mut self, rules: Vec<String>) -> Self {
        self.rules = rules;
        self
    }

    /// Set the minimum severity that causes non-zero exit.
    #[must_use]
    pub const fn with_fail_on(mut self, severity: relune_core::Severity) -> Self {
        self.fail_on = Some(severity);
        self
    }
}

/// Output format for lint results.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LintFormat {
    /// Human-readable text output.
    #[default]
    Text,
    /// JSON output.
    Json,
}

/// Request to compare two schemas.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiffRequest {
    /// Input source for the baseline schema.
    pub before: InputSource,
    /// Input source for the updated schema.
    pub after: InputSource,
    /// Output format.
    #[serde(default)]
    pub format: DiffFormat,
    /// Optional output file path.
    pub output_path: Option<PathBuf>,
}

impl DiffRequest {
    /// Create a new diff request from two SQL texts.
    pub fn from_sql(before: impl Into<String>, after: impl Into<String>) -> Self {
        Self {
            before: InputSource::sql_text(before),
            after: InputSource::sql_text(after),
            ..Default::default()
        }
    }

    /// Set the output format.
    #[must_use]
    pub const fn with_format(mut self, format: DiffFormat) -> Self {
        self.format = format;
        self
    }

    /// Set the output file path.
    #[must_use]
    pub fn with_output_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.output_path = Some(path.into());
        self
    }
}

/// Output format for diff command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiffFormat {
    /// Human-readable text output.
    #[default]
    Text,
    /// JSON output.
    Json,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_source_sql_text() {
        let source = InputSource::sql_text("CREATE TABLE test (id INT);");
        match source {
            InputSource::SqlText { sql, .. } => assert!(sql.contains("CREATE TABLE")),
            _ => panic!("Expected SqlText variant"),
        }
    }

    #[test]
    fn test_input_source_default() {
        let source = InputSource::default();
        match source {
            InputSource::SqlText { sql, .. } => assert!(sql.is_empty()),
            _ => panic!("Expected SqlText variant"),
        }
    }

    #[test]
    fn test_output_format_default() {
        let format = OutputFormat::default();
        assert_eq!(format, OutputFormat::Svg);
    }

    #[test]
    fn test_render_request_default() {
        let req = RenderRequest::default();
        assert_eq!(req.output_format, OutputFormat::Svg);
        assert!(req.focus.is_none());
        assert!(req.output_path.is_none());
    }

    #[test]
    fn test_render_request_builder() {
        let req = RenderRequest::from_sql("CREATE TABLE test (id INT);")
            .with_output_format(OutputFormat::Html)
            .with_output_path("output.html");

        assert_eq!(req.output_format, OutputFormat::Html);
        assert_eq!(req.output_path, Some(PathBuf::from("output.html")));
    }

    #[test]
    fn test_inspect_request_default() {
        let req = InspectRequest::default();
        assert!(req.table.is_none());
        assert_eq!(req.format, InspectFormat::Text);
    }

    #[test]
    fn test_inspect_request_builder() {
        let req = InspectRequest::from_sql("CREATE TABLE test (id INT);")
            .with_table("users")
            .with_format(InspectFormat::Json);

        assert_eq!(req.table, Some("users".to_string()));
        assert_eq!(req.format, InspectFormat::Json);
    }

    #[test]
    fn test_export_request_default() {
        let req = ExportRequest::default();
        assert_eq!(req.format, ExportFormat::SchemaJson);
        assert!(req.focus.is_none());
    }

    #[test]
    fn test_export_request_builder() {
        let req = ExportRequest::from_sql("CREATE TABLE test (id INT);")
            .with_format(ExportFormat::GraphJson)
            .with_output_path("graph.json");

        assert_eq!(req.format, ExportFormat::GraphJson);
        assert_eq!(req.output_path, Some(PathBuf::from("graph.json")));
    }

    #[test]
    fn test_request_serialization() {
        let req = RenderRequest::from_sql("SELECT 1");
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"input\""));
        assert!(json.contains("\"output_format\""));
    }

    #[test]
    fn test_request_deserialization() {
        let json = r#"{"input":{"type":"sql_text","sql":"SELECT 1"}}"#;
        let req: RenderRequest = serde_json::from_str(json).unwrap();
        match req.input {
            InputSource::SqlText { sql, .. } => assert_eq!(sql, "SELECT 1"),
            _ => panic!("Expected SqlText variant"),
        }
    }
}
