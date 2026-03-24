//! Export use case implementation.

use crate::error::AppError;
use crate::request::{ExportFormat, ExportRequest};
use crate::result::ExportResult;
use crate::schema_input::schema_from_input;
use relune_core::Schema;
use relune_layout::{
    FocusExtractor, LayoutConfig, LayoutGraph, LayoutGraphBuilder, LayoutRequest,
    build_layout_with_config, layout_graph_to_d2, layout_graph_to_dot, layout_graph_to_mermaid,
};

fn graph_for_export(request: &ExportRequest, schema: &Schema) -> Result<LayoutGraph, AppError> {
    let layout_request = LayoutRequest {
        filter: request.filter.clone(),
        focus: request.focus.clone(),
        grouping: request.grouping,
        collapse_join_tables: false,
    };
    let mut graph = LayoutGraphBuilder::new()
        .request(layout_request)
        .build(schema);
    if let Some(ref focus) = request.focus {
        graph = FocusExtractor
            .extract(&graph, focus)
            .map_err(relune_layout::LayoutError::from)?;
    }
    Ok(graph)
}

/// Execute an export request.
#[allow(clippy::needless_pass_by_value)] // Owned request matches other usecases and CLI call sites.
pub fn export(request: ExportRequest) -> Result<ExportResult, AppError> {
    // Parse input
    let (schema, diagnostics) = schema_from_input(&request.input)?;
    let stats = schema.stats();

    // Build content based on format
    let content = match request.format {
        ExportFormat::SchemaJson => {
            let export = relune_core::export::export_schema(&schema);
            serde_json::to_string_pretty(&export)?
        }
        ExportFormat::GraphJson => {
            let graph = graph_for_export(&request, &schema)?;
            serde_json::to_string_pretty(&graph)?
        }
        ExportFormat::LayoutJson => {
            let layout_request = LayoutRequest {
                filter: request.filter.clone(),
                focus: request.focus.clone(),
                grouping: request.grouping,
                collapse_join_tables: false,
            };
            let config = LayoutConfig::from(&request.layout);
            let positioned = build_layout_with_config(&schema, &layout_request, &config)?;
            serde_json::to_string_pretty(&positioned)?
        }
        ExportFormat::Mermaid => layout_graph_to_mermaid(&graph_for_export(&request, &schema)?),
        ExportFormat::D2 => layout_graph_to_d2(&graph_for_export(&request, &schema)?),
        ExportFormat::Dot => layout_graph_to_dot(&graph_for_export(&request, &schema)?),
    };

    Ok(ExportResult {
        content,
        diagnostics,
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::InputSource;
    use relune_core::{FilterSpec, LayoutAlgorithm, LayoutSpec, RouteStyle};

    #[test]
    fn test_export_schema_json() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255)
            );
        ";

        let request = ExportRequest::from_sql(sql).with_format(ExportFormat::SchemaJson);

        let result = export(request).unwrap();

        assert!(result.content.contains("\"tables\""));
        assert!(result.content.contains("\"users\""));
        assert_eq!(result.stats.table_count, 1);
    }

    #[test]
    fn test_export_graph_json() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
            CREATE TABLE posts (
                id INT PRIMARY KEY,
                user_id INT REFERENCES users(id)
            );
        ";

        let request = ExportRequest::from_sql(sql).with_format(ExportFormat::GraphJson);

        let result = export(request).unwrap();

        assert!(result.content.contains("\"nodes\""));
        assert!(result.content.contains("\"edges\""));
        assert_eq!(result.stats.table_count, 2);
    }

    #[test]
    fn test_export_layout_json() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY,
                name VARCHAR(255)
            );
        ";

        let request = ExportRequest::from_sql(sql).with_format(ExportFormat::LayoutJson);

        let result = export(request).unwrap();

        assert!(result.content.contains("\"nodes\""));
        assert!(result.content.contains("\"x\""));
        assert!(result.content.contains("\"y\""));
        assert!(result.content.contains("\"width\""));
        assert!(result.content.contains("\"height\""));
    }

    #[test]
    fn test_export_layout_json_with_force_directed_orthogonal_edges() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
            CREATE TABLE posts (
                id INT PRIMARY KEY,
                user_id INT REFERENCES users(id)
            );
        ";

        let request = ExportRequest::from_sql(sql)
            .with_format(ExportFormat::LayoutJson)
            .with_layout(LayoutSpec {
                algorithm: LayoutAlgorithm::ForceDirected,
                edge_style: RouteStyle::Orthogonal,
                ..Default::default()
            });

        let result = export(request).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        let style = &parsed["edges"][0]["route"]["style"];
        assert_eq!(style, "orthogonal");
    }

    #[test]
    fn test_export_mermaid_d2_dot() {
        let sql = r"
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE posts (
                id INT PRIMARY KEY,
                user_id INT REFERENCES users(id)
            );
        ";

        let m = export(ExportRequest::from_sql(sql).with_format(ExportFormat::Mermaid)).unwrap();
        assert!(m.content.starts_with("erDiagram\n"));
        assert!(m.content.contains("||--o{"));

        let d2 = export(ExportRequest::from_sql(sql).with_format(ExportFormat::D2)).unwrap();
        assert!(d2.content.contains("->"));

        let dot = export(ExportRequest::from_sql(sql).with_format(ExportFormat::Dot)).unwrap();
        assert!(dot.content.starts_with("digraph erd"));
    }

    #[test]
    fn test_export_with_filter() {
        // Note: Filter is applied during graph building, not during schema export
        // For schema export, all tables are exported; filter affects graph-based exports
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
            include: vec!["users".to_string()],
            exclude: vec![],
        };

        let request = ExportRequest::from_sql(sql)
            .with_format(ExportFormat::SchemaJson)
            .with_filter(filter);

        let result = export(request).unwrap();

        // Export should succeed
        assert!(result.content.contains("users"));
        // Schema export includes all tables; filter affects graph exports
        assert!(result.stats.table_count >= 1);
    }

    #[test]
    fn test_export_from_sql_file() {
        // Create a temporary SQL file
        let sql = "CREATE TABLE test (id INT PRIMARY KEY);";
        let temp_path = std::env::temp_dir().join("relune_test_export.sql");
        std::fs::write(&temp_path, sql).unwrap();

        let request = ExportRequest {
            input: InputSource::sql_file(&temp_path),
            format: ExportFormat::SchemaJson,
            ..Default::default()
        };

        let result = export(request);
        assert!(result.is_ok());

        // Cleanup
        std::fs::remove_file(&temp_path).ok();
    }
}
