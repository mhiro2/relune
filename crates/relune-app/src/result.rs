//! Result types for the relune application layer.
//!
//! These types define the output of operations from relune-app.

use relune_core::{Diagnostic, Schema, SchemaStats};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Result of a render operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderResult {
    /// The rendered content (SVG, HTML, or JSON string).
    pub content: String,
    /// Diagnostics collected during processing.
    pub diagnostics: Vec<Diagnostic>,
    /// Statistics about the rendering.
    pub stats: RenderStats,
}

/// Statistics about a render operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderStats {
    /// Number of tables in the output.
    pub table_count: usize,
    /// Number of columns in the output.
    pub column_count: usize,
    /// Number of foreign keys (edges) in the output.
    pub edge_count: usize,
    /// Number of views in the schema.
    pub view_count: usize,
    /// Time spent parsing.
    #[serde(with = "duration_serde")]
    pub parse_time: Duration,
    /// Time spent building the graph.
    #[serde(with = "duration_serde")]
    pub graph_time: Duration,
    /// Time spent laying out.
    #[serde(with = "duration_serde")]
    pub layout_time: Duration,
    /// Time spent rendering.
    #[serde(with = "duration_serde")]
    pub render_time: Duration,
    /// Total time.
    #[serde(with = "duration_serde")]
    pub total_time: Duration,
}

impl Default for RenderStats {
    fn default() -> Self {
        Self {
            table_count: 0,
            column_count: 0,
            edge_count: 0,
            view_count: 0,
            parse_time: Duration::ZERO,
            graph_time: Duration::ZERO,
            layout_time: Duration::ZERO,
            render_time: Duration::ZERO,
            total_time: Duration::ZERO,
        }
    }
}

impl RenderStats {
    /// Create stats from schema stats and timing information.
    #[must_use]
    pub fn from_schema_stats(
        stats: &SchemaStats,
        parse_time: Duration,
        graph_time: Duration,
        layout_time: Duration,
        render_time: Duration,
    ) -> Self {
        Self {
            table_count: stats.table_count,
            column_count: stats.column_count,
            edge_count: stats.foreign_key_count,
            view_count: stats.view_count,
            parse_time,
            graph_time,
            layout_time,
            render_time,
            total_time: parse_time + graph_time + layout_time + render_time,
        }
    }
}

/// Custom serialization for Duration.
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    #[derive(Serialize)]
    struct DurationData {
        millis: u64,
        nanos: u32,
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let millis = duration.as_millis() as u64;
        let nanos = duration.subsec_nanos();
        DurationData { millis, nanos }.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct DurationData {
            millis: u64,
            nanos: u32,
        }
        let data = DurationData::deserialize(deserializer)?;
        Ok(Duration::from_millis(data.millis) + Duration::from_nanos(u64::from(data.nanos)))
    }
}

/// Result of an inspect operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectResult {
    /// Schema summary (always present).
    pub summary: SchemaSummary,
    /// Table details (if a specific table was requested).
    pub table: Option<TableDetails>,
    /// Diagnostics collected during processing.
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

/// Summary of a schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaSummary {
    /// Total number of tables.
    pub table_count: usize,
    /// Total number of columns.
    pub column_count: usize,
    /// Total number of foreign keys.
    pub foreign_key_count: usize,
    /// Total number of views.
    pub view_count: usize,
    /// Total number of enums.
    pub enum_count: usize,
    /// List of table names.
    pub tables: Vec<TableSummary>,
}

impl From<&Schema> for SchemaSummary {
    fn from(schema: &Schema) -> Self {
        Self {
            table_count: schema.tables.len(),
            column_count: schema.tables.iter().map(|t| t.columns.len()).sum(),
            foreign_key_count: schema.tables.iter().map(|t| t.foreign_keys.len()).sum(),
            view_count: schema.views.len(),
            enum_count: schema.enums.len(),
            tables: schema.tables.iter().map(TableSummary::from).collect(),
        }
    }
}

/// Summary of a single table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSummary {
    /// Qualified table name.
    pub name: String,
    /// Number of columns.
    pub column_count: usize,
    /// Number of foreign keys.
    pub foreign_key_count: usize,
    /// Whether the table has a primary key.
    pub has_primary_key: bool,
}

impl From<&relune_core::Table> for TableSummary {
    fn from(table: &relune_core::Table) -> Self {
        Self {
            name: table.qualified_name(),
            column_count: table.columns.len(),
            foreign_key_count: table.foreign_keys.len(),
            has_primary_key: table.columns.iter().any(|c| c.is_primary_key),
        }
    }
}

/// Detailed information about a table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableDetails {
    /// Qualified table name.
    pub name: String,
    /// Table comment (if any).
    pub comment: Option<String>,
    /// Column details.
    pub columns: Vec<ColumnDetails>,
    /// Foreign key details.
    pub foreign_keys: Vec<ForeignKeyDetails>,
    /// Index details.
    pub indexes: Vec<IndexDetails>,
}

impl From<&relune_core::Table> for TableDetails {
    fn from(table: &relune_core::Table) -> Self {
        Self {
            name: table.qualified_name(),
            comment: table.comment.clone(),
            columns: table.columns.iter().map(ColumnDetails::from).collect(),
            foreign_keys: table
                .foreign_keys
                .iter()
                .map(ForeignKeyDetails::from)
                .collect(),
            indexes: table.indexes.iter().map(IndexDetails::from).collect(),
        }
    }
}

/// Details about a column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDetails {
    /// Column name.
    pub name: String,
    /// Data type.
    pub data_type: String,
    /// Whether the column is nullable.
    pub nullable: bool,
    /// Whether the column is a primary key.
    pub is_primary_key: bool,
    /// Column comment (if any).
    pub comment: Option<String>,
}

impl From<&relune_core::Column> for ColumnDetails {
    fn from(column: &relune_core::Column) -> Self {
        Self {
            name: column.name.clone(),
            data_type: column.data_type.clone(),
            nullable: column.nullable,
            is_primary_key: column.is_primary_key,
            comment: column.comment.clone(),
        }
    }
}

/// Details about a foreign key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeyDetails {
    /// Foreign key name (if any).
    pub name: Option<String>,
    /// Source columns.
    pub from_columns: Vec<String>,
    /// Target schema, if cross-schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_schema: Option<String>,
    /// Target table.
    pub to_table: String,
    /// Target columns.
    pub to_columns: Vec<String>,
}

impl From<&relune_core::ForeignKey> for ForeignKeyDetails {
    fn from(fk: &relune_core::ForeignKey) -> Self {
        Self {
            name: fk.name.clone(),
            from_columns: fk.from_columns.clone(),
            to_schema: fk.to_schema.clone(),
            to_table: fk.to_table.clone(),
            to_columns: fk.to_columns.clone(),
        }
    }
}

/// Details about an index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDetails {
    /// Index name, if named.
    pub name: Option<String>,
    /// Indexed columns.
    pub columns: Vec<String>,
    /// Whether the index is unique.
    pub is_unique: bool,
}

impl From<&relune_core::Index> for IndexDetails {
    fn from(index: &relune_core::Index) -> Self {
        Self {
            name: index.name.clone(),
            columns: index.columns.clone(),
            is_unique: index.is_unique,
        }
    }
}

/// Result of an export operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResult {
    /// The exported content (JSON string).
    pub content: String,
    /// Diagnostics collected during processing.
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    /// Statistics about the exported schema.
    pub stats: SchemaStats,
}

/// Result of a lint operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintResult {
    /// Lint issues found.
    pub issues: Vec<relune_core::LintIssue>,
    /// Statistics.
    pub stats: relune_core::LintStats,
    /// Diagnostics collected during processing.
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

impl LintResult {
    /// Returns true if there are any issues matching the `fail_on` severity.
    #[must_use]
    pub fn has_failures(&self, fail_on: Option<relune_core::Severity>) -> bool {
        if let Some(min_severity) = fail_on {
            self.issues
                .iter()
                .any(|issue| issue.severity >= min_severity)
        } else {
            false
        }
    }
}

/// Result of a diff operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    /// The schema diff.
    pub diff: relune_core::SchemaDiff,
    /// Diagnostics collected during processing.
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

impl DiffResult {
    /// Returns true if there are any changes between the schemas.
    #[must_use]
    pub const fn has_changes(&self) -> bool {
        !self.diff.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_stats_default() {
        let stats = RenderStats::default();
        assert_eq!(stats.table_count, 0);
        assert_eq!(stats.total_time, Duration::ZERO);
    }

    #[test]
    fn test_render_result_serialization() {
        let result = RenderResult {
            content: "<svg></svg>".to_string(),
            diagnostics: vec![],
            stats: RenderStats::default(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"content\""));
        assert!(json.contains("\"diagnostics\""));
        assert!(json.contains("\"stats\""));
    }

    #[test]
    fn test_schema_summary_from_schema() {
        let schema = Schema::default();
        let summary = SchemaSummary::from(&schema);
        assert_eq!(summary.table_count, 0);
        assert_eq!(summary.tables.len(), 0);
    }

    #[test]
    fn test_duration_serialization() {
        let stats = RenderStats {
            parse_time: Duration::from_millis(100),
            ..Default::default()
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"parse_time\""));
        assert!(json.contains("\"millis\":100"));
    }

    #[test]
    fn test_duration_deserialization() {
        let json = r#"{"table_count":0,"column_count":0,"edge_count":0,"view_count":0,"parse_time":{"millis":50,"nanos":0},"graph_time":{"millis":0,"nanos":0},"layout_time":{"millis":0,"nanos":0},"render_time":{"millis":0,"nanos":0},"total_time":{"millis":50,"nanos":0}}"#;
        let stats: RenderStats = serde_json::from_str(json).unwrap();
        assert_eq!(stats.parse_time, Duration::from_millis(50));
    }
}
