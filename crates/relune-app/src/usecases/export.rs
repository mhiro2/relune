//! Export use case implementation.

use crate::error::AppError;
use crate::request::{ExportFormat, ExportRequest};
use crate::result::ExportResult;
use crate::schema_input::schema_from_input;
use relune_core::Schema;
use relune_layout::{
    FocusExtractor, LayoutConfig, LayoutGraph, LayoutGraphBuilder, LayoutRequest,
    build_layout_from_graph_with_config, layout_graph_to_d2, layout_graph_to_dot,
    layout_graph_to_mermaid,
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

const fn export_format_uses_graph(format: ExportFormat) -> bool {
    matches!(
        format,
        ExportFormat::GraphJson
            | ExportFormat::LayoutJson
            | ExportFormat::Mermaid
            | ExportFormat::D2
            | ExportFormat::Dot
    )
}

/// Execute an export request.
#[allow(clippy::needless_pass_by_value)] // Owned request matches other usecases and CLI call sites.
pub fn export(request: ExportRequest) -> Result<ExportResult, AppError> {
    // Parse input
    let (schema, diagnostics) = schema_from_input(&request.input)?;
    let stats = schema.stats();
    let graph = export_format_uses_graph(request.format)
        .then(|| graph_for_export(&request, &schema))
        .transpose()?;

    // Build content based on format
    let content = match request.format {
        ExportFormat::SchemaJson => {
            let export = relune_core::export::export_schema(&schema);
            serde_json::to_string_pretty(&export)?
        }
        ExportFormat::GraphJson => {
            serde_json::to_string_pretty(graph.as_ref().expect("graph json requires graph"))?
        }
        ExportFormat::LayoutJson => {
            let config = LayoutConfig::from(&request.layout);
            let positioned = build_layout_from_graph_with_config(
                graph.as_ref().expect("layout json requires graph"),
                &config,
            )?;
            serde_json::to_string_pretty(&positioned)?
        }
        ExportFormat::Mermaid => {
            layout_graph_to_mermaid(graph.as_ref().expect("mermaid requires graph"))
        }
        ExportFormat::D2 => layout_graph_to_d2(graph.as_ref().expect("d2 requires graph")),
        ExportFormat::Dot => layout_graph_to_dot(graph.as_ref().expect("dot requires graph")),
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
    use relune_core::{FilterSpec, FocusSpec, LayoutAlgorithm, LayoutSpec, RouteStyle};

    fn graph_table_names(content: &str) -> std::collections::BTreeSet<String> {
        let graph: serde_json::Value = serde_json::from_str(content).unwrap();
        graph["nodes"]
            .as_array()
            .expect("nodes array")
            .iter()
            .map(|node| node["table_name"].as_str().expect("table_name").to_string())
            .collect()
    }

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
            CREATE TABLE comments (
                id INT PRIMARY KEY,
                post_id INT REFERENCES posts(id)
            );
        ";

        let request = ExportRequest::from_sql(sql)
            .with_format(ExportFormat::GraphJson)
            .with_focus(FocusSpec {
                table: "posts".to_string(),
                depth: 1,
            });

        let result = export(request).unwrap();
        let graph: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        let nodes = graph["nodes"].as_array().expect("nodes array");
        let node_ids = nodes
            .iter()
            .map(|node| node["table_name"].as_str().expect("table_name").to_string())
            .collect::<std::collections::BTreeSet<_>>();
        let edges = graph["edges"].as_array().expect("edges array");
        let edge_ids = edges
            .iter()
            .map(|edge| {
                (
                    edge["from"].as_str().expect("from").to_string(),
                    edge["to"].as_str().expect("to").to_string(),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        let expected_nodes = ["comments", "posts", "users"]
            .into_iter()
            .map(str::to_string)
            .collect::<std::collections::BTreeSet<_>>();
        let expected_edges = [("comments", "posts"), ("posts", "users")]
            .into_iter()
            .map(|(from, to)| (from.to_string(), to.to_string()))
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(node_ids, expected_nodes);
        assert_eq!(edge_ids, expected_edges);
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
    fn test_export_layout_json_includes_routing_debug_metadata() {
        let sql = r"
            CREATE TABLE users (
                id INT PRIMARY KEY
            );
            CREATE TABLE posts (
                id INT PRIMARY KEY,
                author_id INT REFERENCES users(id),
                reviewer_id INT REFERENCES users(id)
            );
        ";

        let request = ExportRequest::from_sql(sql).with_format(ExportFormat::LayoutJson);

        let result = export(request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();

        assert_eq!(
            parsed["routing_debug"]["non_self_loop_detour_activations"],
            0
        );
        let edge_debug = &parsed["edges"][0]["routing_debug"];
        assert!(edge_debug["source_side"].is_string());
        assert!(edge_debug["target_side"].is_string());
        assert!(edge_debug["source_slot_index"].is_number());
        assert!(edge_debug["source_slot_count"].is_number());
        assert!(edge_debug["target_slot_index"].is_number());
        assert!(edge_debug["target_slot_count"].is_number());
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
            .with_format(ExportFormat::GraphJson)
            .with_filter(filter);

        let result = export(request).unwrap();
        let table_names = graph_table_names(&result.content);

        assert_eq!(table_names, std::iter::once("users".to_string()).collect());
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
