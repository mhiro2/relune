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
    let (schema, mut diagnostics) = schema_from_input(&request.input)?;
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
            let g = graph
                .as_ref()
                .ok_or_else(|| AppError::other("export", "graph json requires graph"))?;
            serde_json::to_string_pretty(g)?
        }
        ExportFormat::LayoutJson => {
            let g = graph
                .as_ref()
                .ok_or_else(|| AppError::other("export", "layout json requires graph"))?;
            let config = LayoutConfig::from(&request.layout);
            diagnostics.extend(config.density_fallback_notice(g.nodes.len()));
            let positioned = build_layout_from_graph_with_config(g, &config)?;
            serde_json::to_string_pretty(&positioned)?
        }
        ExportFormat::Mermaid => {
            let g = graph
                .as_ref()
                .ok_or_else(|| AppError::other("export", "mermaid requires graph"))?;
            layout_graph_to_mermaid(g)
        }
        ExportFormat::D2 => {
            let g = graph
                .as_ref()
                .ok_or_else(|| AppError::other("export", "d2 requires graph"))?;
            layout_graph_to_d2(g)
        }
        ExportFormat::Dot => {
            let g = graph
                .as_ref()
                .ok_or_else(|| AppError::other("export", "dot requires graph"))?;
            layout_graph_to_dot(g)
        }
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
            CREATE TABLE reactions (
                id INT PRIMARY KEY,
                comment_id INT REFERENCES comments(id)
            );
            CREATE TABLE settings (
                id INT PRIMARY KEY
            );
        ";
        let edge_ids = |content: &str| {
            let graph: serde_json::Value = serde_json::from_str(content).unwrap();
            graph["edges"]
                .as_array()
                .expect("edges array")
                .iter()
                .map(|edge| {
                    (
                        edge["from"].as_str().expect("from").to_string(),
                        edge["to"].as_str().expect("to").to_string(),
                    )
                })
                .collect::<std::collections::BTreeSet<_>>()
        };
        let set = |names: &[&str]| {
            names
                .iter()
                .map(|name| (*name).to_string())
                .collect::<std::collections::BTreeSet<_>>()
        };
        let edge_set = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(from, to)| ((*from).to_string(), (*to).to_string()))
                .collect::<std::collections::BTreeSet<_>>()
        };

        // Without focus the whole graph is exported.
        let full =
            export(ExportRequest::from_sql(sql).with_format(ExportFormat::GraphJson)).unwrap();
        assert_eq!(
            graph_table_names(&full.content),
            set(&["comments", "posts", "reactions", "settings", "users"])
        );
        assert_eq!(
            edge_ids(&full.content),
            edge_set(&[
                ("comments", "posts"),
                ("posts", "users"),
                ("reactions", "comments")
            ])
        );

        // A depth-1 focus on posts drops the two-hop and unconnected tables.
        let focused = export(
            ExportRequest::from_sql(sql)
                .with_format(ExportFormat::GraphJson)
                .with_focus(FocusSpec::new("posts", 1)),
        )
        .unwrap();
        assert_eq!(
            graph_table_names(&focused.content),
            set(&["comments", "posts", "users"])
        );
        assert_eq!(
            edge_ids(&focused.content),
            edge_set(&[("comments", "posts"), ("posts", "users")])
        );
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
        let graph = relune_testkit::parse_layout_json(&result.content);

        assert_eq!(graph.nodes.len(), 1);
        let users = &graph.nodes[0];
        assert_eq!(users.id, "users");
        assert!(users.x.is_finite() && users.y.is_finite());
        assert!(users.width > 0.0 && users.height > 0.0);
        let columns = users
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(columns, ["id", "name"]);
        assert!(graph.edges.is_empty());
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
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("schema.sql");
        std::fs::write(
            &path,
            "CREATE TABLE accounts (id INT PRIMARY KEY, email TEXT NOT NULL);",
        )
        .unwrap();

        let request = ExportRequest {
            input: InputSource::sql_file(&path),
            format: ExportFormat::SchemaJson,
            ..Default::default()
        };

        let result = export(request).unwrap();
        let exported: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        let tables = exported["tables"].as_array().expect("tables array");
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0]["name"], "accounts");
        let columns = tables[0]["columns"]
            .as_array()
            .expect("columns array")
            .iter()
            .map(|column| column["name"].as_str().expect("column name"))
            .collect::<Vec<_>>();
        assert_eq!(columns, ["id", "email"]);
    }
}
