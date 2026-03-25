//! WASM-specific request types.
//!
//! These types are designed for easy JSON serialization/deserialization
//! from JavaScript.

use relune_app::{
    ExportFormat, ExportRequest, FilterSpec, FocusSpec, GroupingSpec, GroupingStrategy,
    InspectFormat, InspectRequest, LayoutAlgorithm, LayoutDirection, LayoutSpec, OutputFormat,
    RenderOptions, RenderRequest, RouteStyle,
};
use serde::{Deserialize, Serialize};

/// WASM-friendly render request that can be deserialized from JavaScript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmRenderRequest {
    /// SQL DDL text (optional, mutually exclusive with `schema_json`).
    pub sql: Option<String>,
    /// Pre-normalized schema JSON (optional, mutually exclusive with sql).
    pub schema_json: Option<String>,
    /// Output format: "svg", "html", "graph-json", "schema-json".
    #[serde(default)]
    pub format: Option<String>,
    /// Focus table name (optional).
    pub focus_table: Option<String>,
    /// Focus depth (optional, default 1).
    #[serde(default)]
    pub depth: Option<u32>,
    /// Tables to include (glob patterns).
    #[serde(default)]
    pub include_tables: Vec<String>,
    /// Tables to exclude (glob patterns).
    #[serde(default)]
    pub exclude_tables: Vec<String>,
    /// Grouping strategy: "none", "schema", "prefix".
    #[serde(default)]
    pub group_by: Option<String>,
    /// Layout direction: "top-to-bottom", "left-to-right", "right-to-left", "bottom-to-top".
    #[serde(default)]
    pub layout_direction: Option<String>,
    /// Layout algorithm: "hierarchical" or "force-directed".
    #[serde(default)]
    pub layout_algorithm: Option<String>,
    /// Edge routing style: "straight", "orthogonal", or "curved".
    #[serde(default)]
    pub edge_style: Option<String>,
    /// Horizontal spacing hint.
    #[serde(default)]
    pub horizontal_spacing: Option<f32>,
    /// Vertical spacing hint.
    #[serde(default)]
    pub vertical_spacing: Option<f32>,
}

impl WasmRenderRequest {
    /// Convert to a `RenderRequest` for the app layer.
    pub fn to_render_request(&self) -> Result<RenderRequest, String> {
        // Validate input source
        let input = match (&self.sql, &self.schema_json) {
            (Some(sql), None) => relune_app::InputSource::sql_text(sql),
            (None, Some(json)) => relune_app::InputSource::schema_json(json),
            (Some(_), Some(_)) => {
                return Err("Cannot specify both 'sql' and 'schemaJson'".to_string());
            }
            (None, None) => return Err("Must specify either 'sql' or 'schemaJson'".to_string()),
        };

        // Parse output format
        let output_format = match self.format.as_deref() {
            None | Some("svg") => OutputFormat::Svg,
            Some("html") => OutputFormat::Html,
            Some("graph-json") => OutputFormat::GraphJson,
            Some("schema-json") => OutputFormat::SchemaJson,
            Some(other) => return Err(format!("Unknown output format: {other}")),
        };

        // Build focus spec (use FocusSpec::new to clamp depth to MAX_FOCUS_DEPTH)
        let focus = self
            .focus_table
            .as_ref()
            .map(|table| FocusSpec::new(table.clone(), self.depth.unwrap_or(1)));

        // Build filter spec
        let filter = FilterSpec {
            include: self.include_tables.clone(),
            exclude: self.exclude_tables.clone(),
        };

        // Parse grouping strategy
        let grouping_strategy = match self.group_by.as_deref() {
            None | Some("none") => GroupingStrategy::None,
            Some("schema") => GroupingStrategy::BySchema,
            Some("prefix") => GroupingStrategy::ByPrefix,
            Some(other) => return Err(format!("Unknown grouping strategy: {other}")),
        };
        let grouping = GroupingSpec {
            strategy: grouping_strategy,
        };

        let horizontal_spacing =
            validated_spacing(self.horizontal_spacing, 320.0, "horizontalSpacing")?;
        let vertical_spacing = validated_spacing(self.vertical_spacing, 80.0, "verticalSpacing")?;

        // Parse layout direction
        let layout_direction = match self.layout_direction.as_deref() {
            None | Some("top-to-bottom") => LayoutDirection::TopToBottom,
            Some("left-to-right") => LayoutDirection::LeftToRight,
            Some("right-to-left") => LayoutDirection::RightToLeft,
            Some("bottom-to-top") => LayoutDirection::BottomToTop,
            Some(other) => return Err(format!("Unknown layout direction: {other}")),
        };
        let layout_algorithm = match self.layout_algorithm.as_deref() {
            None | Some("hierarchical") => LayoutAlgorithm::Hierarchical,
            Some("force-directed") => LayoutAlgorithm::ForceDirected,
            Some(other) => return Err(format!("Unknown layout algorithm: {other}")),
        };
        let edge_style = match self.edge_style.as_deref() {
            None | Some("straight") => RouteStyle::Straight,
            Some("orthogonal") => RouteStyle::Orthogonal,
            Some("curved") => RouteStyle::Curved,
            Some(other) => return Err(format!("Unknown edge style: {other}")),
        };
        let layout = LayoutSpec {
            algorithm: layout_algorithm,
            direction: layout_direction,
            edge_style,
            horizontal_spacing,
            vertical_spacing,
            force_iterations: 150,
        };

        Ok(RenderRequest {
            input,
            output_format,
            filter,
            focus,
            grouping,
            layout,
            options: RenderOptions::default(),
            output_path: None, // Not applicable in WASM
        })
    }
}

/// WASM-friendly inspect request that can be deserialized from JavaScript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmInspectRequest {
    /// SQL DDL text (optional, mutually exclusive with `schema_json`).
    pub sql: Option<String>,
    /// Pre-normalized schema JSON (optional, mutually exclusive with sql).
    pub schema_json: Option<String>,
    /// Table name to inspect (optional, returns schema summary if not specified).
    pub table: Option<String>,
    /// Output format: "json" or "text".
    #[serde(default)]
    pub format: Option<String>,
}

impl WasmInspectRequest {
    /// Convert to an `InspectRequest` for the app layer.
    pub fn to_inspect_request(&self) -> Result<InspectRequest, String> {
        // Validate input source
        let input = match (&self.sql, &self.schema_json) {
            (Some(sql), None) => relune_app::InputSource::sql_text(sql),
            (None, Some(json)) => relune_app::InputSource::schema_json(json),
            (Some(_), Some(_)) => {
                return Err("Cannot specify both 'sql' and 'schemaJson'".to_string());
            }
            (None, None) => return Err("Must specify either 'sql' or 'schemaJson'".to_string()),
        };

        // Parse output format
        let format = match self.format.as_deref() {
            None | Some("json") => InspectFormat::Json,
            Some("text") => InspectFormat::Text,
            Some(other) => return Err(format!("Unknown inspect format: {other}")),
        };

        Ok(InspectRequest {
            input,
            table: self.table.clone(),
            format,
        })
    }
}

/// WASM-friendly export request that can be deserialized from JavaScript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmExportRequest {
    /// SQL DDL text (optional, mutually exclusive with `schema_json`).
    pub sql: Option<String>,
    /// Pre-normalized schema JSON (optional, mutually exclusive with sql).
    pub schema_json: Option<String>,
    /// Export format: "schema-json", "graph-json", "layout-json", "mermaid", "d2", "dot".
    #[serde(default)]
    pub format: Option<String>,
    /// Focus table name (optional).
    pub focus_table: Option<String>,
    /// Focus depth (optional, default 1).
    #[serde(default)]
    pub depth: Option<u32>,
    /// Tables to include (glob patterns).
    #[serde(default)]
    pub include_tables: Vec<String>,
    /// Tables to exclude (glob patterns).
    #[serde(default)]
    pub exclude_tables: Vec<String>,
    /// Grouping strategy: "none", "schema", "prefix".
    #[serde(default)]
    pub group_by: Option<String>,
    /// Layout algorithm: "hierarchical" or "force-directed".
    #[serde(default)]
    pub layout_algorithm: Option<String>,
    /// Edge routing style: "straight", "orthogonal", or "curved".
    #[serde(default)]
    pub edge_style: Option<String>,
}

impl WasmExportRequest {
    /// Convert to an `ExportRequest` for the app layer.
    pub fn to_export_request(&self) -> Result<ExportRequest, String> {
        // Validate input source
        let input = match (&self.sql, &self.schema_json) {
            (Some(sql), None) => relune_app::InputSource::sql_text(sql),
            (None, Some(json)) => relune_app::InputSource::schema_json(json),
            (Some(_), Some(_)) => {
                return Err("Cannot specify both 'sql' and 'schemaJson'".to_string());
            }
            (None, None) => return Err("Must specify either 'sql' or 'schemaJson'".to_string()),
        };

        // Parse export format
        let format = match self.format.as_deref() {
            None | Some("schema-json") => ExportFormat::SchemaJson,
            Some("graph-json") => ExportFormat::GraphJson,
            Some("layout-json") => ExportFormat::LayoutJson,
            Some("mermaid") => ExportFormat::Mermaid,
            Some("d2") => ExportFormat::D2,
            Some("dot") => ExportFormat::Dot,
            Some(other) => return Err(format!("Unknown export format: {other}")),
        };

        // Build focus spec (use FocusSpec::new to clamp depth to MAX_FOCUS_DEPTH)
        let focus = self
            .focus_table
            .as_ref()
            .map(|table| FocusSpec::new(table.clone(), self.depth.unwrap_or(1)));

        // Build filter spec
        let filter = FilterSpec {
            include: self.include_tables.clone(),
            exclude: self.exclude_tables.clone(),
        };

        // Parse grouping strategy
        let grouping_strategy = match self.group_by.as_deref() {
            None | Some("none") => GroupingStrategy::None,
            Some("schema") => GroupingStrategy::BySchema,
            Some("prefix") => GroupingStrategy::ByPrefix,
            Some(other) => return Err(format!("Unknown grouping strategy: {other}")),
        };
        let grouping = GroupingSpec {
            strategy: grouping_strategy,
        };

        let layout_algorithm = match self.layout_algorithm.as_deref() {
            None | Some("hierarchical") => LayoutAlgorithm::Hierarchical,
            Some("force-directed") => LayoutAlgorithm::ForceDirected,
            Some(other) => return Err(format!("Unknown layout algorithm: {other}")),
        };
        let edge_style = match self.edge_style.as_deref() {
            None | Some("straight") => RouteStyle::Straight,
            Some("orthogonal") => RouteStyle::Orthogonal,
            Some("curved") => RouteStyle::Curved,
            Some(other) => return Err(format!("Unknown edge style: {other}")),
        };

        Ok(ExportRequest {
            input,
            format,
            filter,
            focus,
            grouping,
            layout: LayoutSpec {
                algorithm: layout_algorithm,
                edge_style,
                ..Default::default()
            },
            output_path: None, // Not applicable in WASM
        })
    }
}

fn validated_spacing(value: Option<f32>, default: f32, field: &str) -> Result<f32, String> {
    let spacing = value.unwrap_or(default);
    if spacing.is_finite() && spacing > 0.0 {
        Ok(spacing)
    } else {
        Err(format!("{field} must be a positive finite number"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_render_request_sql() {
        let req = WasmRenderRequest {
            sql: Some("CREATE TABLE test (id INT);".to_string()),
            schema_json: None,
            format: Some("svg".to_string()),
            focus_table: None,
            depth: None,
            include_tables: vec![],
            exclude_tables: vec![],
            group_by: None,
            layout_direction: None,
            layout_algorithm: None,
            edge_style: None,
            horizontal_spacing: None,
            vertical_spacing: None,
        };

        let render_req = req.to_render_request().unwrap();
        assert_eq!(render_req.output_format, OutputFormat::Svg);
    }

    #[test]
    fn test_wasm_render_request_with_focus() {
        let req = WasmRenderRequest {
            sql: Some("SELECT 1".to_string()),
            schema_json: None,
            format: None,
            focus_table: Some("users".to_string()),
            depth: Some(2),
            include_tables: vec![],
            exclude_tables: vec![],
            group_by: None,
            layout_direction: None,
            layout_algorithm: Some("force-directed".to_string()),
            edge_style: Some("orthogonal".to_string()),
            horizontal_spacing: None,
            vertical_spacing: None,
        };

        let render_req = req.to_render_request().unwrap();
        assert!(render_req.focus.is_some());
        let focus = render_req.focus.unwrap();
        assert_eq!(focus.table, "users");
        assert_eq!(focus.depth, 2);
        assert_eq!(render_req.layout.algorithm, LayoutAlgorithm::ForceDirected);
        assert_eq!(render_req.layout.edge_style, RouteStyle::Orthogonal);
    }

    #[test]
    fn test_wasm_render_request_invalid() {
        let req = WasmRenderRequest {
            sql: None,
            schema_json: None,
            format: None,
            focus_table: None,
            depth: None,
            include_tables: vec![],
            exclude_tables: vec![],
            group_by: None,
            layout_direction: None,
            layout_algorithm: None,
            edge_style: None,
            horizontal_spacing: None,
            vertical_spacing: None,
        };

        assert!(req.to_render_request().is_err());
    }

    #[test]
    fn test_wasm_render_request_both_inputs() {
        let req = WasmRenderRequest {
            sql: Some("SELECT 1".to_string()),
            schema_json: Some("{}".to_string()),
            format: None,
            focus_table: None,
            depth: None,
            include_tables: vec![],
            exclude_tables: vec![],
            group_by: None,
            layout_direction: None,
            layout_algorithm: None,
            edge_style: None,
            horizontal_spacing: None,
            vertical_spacing: None,
        };

        assert!(req.to_render_request().is_err());
    }

    #[test]
    fn test_wasm_inspect_request() {
        let req = WasmInspectRequest {
            sql: Some("CREATE TABLE users (id INT);".to_string()),
            schema_json: None,
            table: Some("users".to_string()),
            format: Some("json".to_string()),
        };

        let inspect_req = req.to_inspect_request().unwrap();
        assert_eq!(inspect_req.table, Some("users".to_string()));
        assert_eq!(inspect_req.format, InspectFormat::Json);
    }

    #[test]
    fn test_wasm_export_request() {
        let req = WasmExportRequest {
            sql: Some("CREATE TABLE test (id INT);".to_string()),
            schema_json: None,
            format: Some("graph-json".to_string()),
            focus_table: None,
            depth: None,
            include_tables: vec![],
            exclude_tables: vec![],
            group_by: None,
            layout_algorithm: Some("force-directed".to_string()),
            edge_style: Some("curved".to_string()),
        };

        let export_req = req.to_export_request().unwrap();
        assert_eq!(export_req.format, ExportFormat::GraphJson);
        assert_eq!(export_req.layout.algorithm, LayoutAlgorithm::ForceDirected);
        assert_eq!(export_req.layout.edge_style, RouteStyle::Curved);
    }

    #[test]
    fn test_wasm_render_request_rejects_non_positive_spacing() {
        let req = WasmRenderRequest {
            sql: Some("CREATE TABLE test (id INT);".to_string()),
            schema_json: None,
            format: None,
            focus_table: None,
            depth: None,
            include_tables: vec![],
            exclude_tables: vec![],
            group_by: None,
            layout_direction: None,
            layout_algorithm: None,
            edge_style: None,
            horizontal_spacing: Some(0.0),
            vertical_spacing: Some(80.0),
        };

        let err = req
            .to_render_request()
            .expect_err("spacing should be rejected");
        assert!(err.contains("horizontalSpacing"));
    }
}
