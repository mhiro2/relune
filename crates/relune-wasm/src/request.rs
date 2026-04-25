//! WASM-specific request types.
//!
//! These types are designed for easy JSON serialization/deserialization
//! from JavaScript.

use relune_app::{
    DiffFormat, DiffRequest, ExportFormat, ExportRequest, FilterSpec, FocusSpec, GroupingSpec,
    GroupingStrategy, InspectFormat, InspectRequest, LayoutAlgorithm, LayoutCompactionSpec,
    LayoutDirection, LayoutSpec, LintFormat, LintRequest, OutputFormat, RenderOptions,
    RenderRequest, RenderTheme, RouteStyle,
};
use relune_core::{LintProfile, LintRuleCategory, Severity};
use serde::{Deserialize, Serialize};

/// Grouping strategy as exposed to the WASM/JS API.
///
/// Uses short kebab-case names ("none", "schema", "prefix") rather than the
/// internal `GroupingStrategy` `snake_case` variants (`by_schema`, `by_prefix`).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WasmGroupBy {
    #[default]
    None,
    Schema,
    Prefix,
}

impl From<WasmGroupBy> for GroupingStrategy {
    fn from(value: WasmGroupBy) -> Self {
        match value {
            WasmGroupBy::None => Self::None,
            WasmGroupBy::Schema => Self::BySchema,
            WasmGroupBy::Prefix => Self::ByPrefix,
        }
    }
}

/// Layout direction as exposed to the WASM/JS API.
///
/// Uses kebab-case names ("top-to-bottom") matching the JS convention, while
/// the internal `LayoutDirection` uses `snake_case` (`top_to_bottom`).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WasmLayoutDirection {
    #[default]
    TopToBottom,
    LeftToRight,
    RightToLeft,
    BottomToTop,
}

impl From<WasmLayoutDirection> for LayoutDirection {
    fn from(value: WasmLayoutDirection) -> Self {
        match value {
            WasmLayoutDirection::TopToBottom => Self::TopToBottom,
            WasmLayoutDirection::LeftToRight => Self::LeftToRight,
            WasmLayoutDirection::RightToLeft => Self::RightToLeft,
            WasmLayoutDirection::BottomToTop => Self::BottomToTop,
        }
    }
}

/// WASM-friendly render request that can be deserialized from JavaScript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmRenderRequest {
    /// SQL DDL text (optional, mutually exclusive with `schema_json`).
    pub sql: Option<String>,
    /// Pre-normalized schema JSON (optional, mutually exclusive with sql).
    pub schema_json: Option<String>,
    /// Output format.
    #[serde(default)]
    pub format: Option<OutputFormat>,
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
    /// Grouping strategy.
    #[serde(default)]
    pub group_by: Option<WasmGroupBy>,
    /// Layout direction.
    #[serde(default)]
    pub layout_direction: Option<WasmLayoutDirection>,
    /// Layout algorithm.
    #[serde(default)]
    pub layout_algorithm: Option<LayoutAlgorithm>,
    /// Edge rendering style.
    #[serde(default)]
    pub edge_style: Option<RouteStyle>,
    /// Horizontal spacing hint.
    #[serde(default)]
    pub horizontal_spacing: Option<f32>,
    /// Vertical spacing hint.
    #[serde(default)]
    pub vertical_spacing: Option<f32>,
    /// Render theme.
    #[serde(default)]
    pub theme: Option<RenderTheme>,
    /// Whether to show a legend in rendered outputs.
    #[serde(default)]
    pub show_legend: Option<bool>,
    /// Whether to show render statistics in rendered outputs.
    #[serde(default)]
    pub show_stats: Option<bool>,
}

impl WasmRenderRequest {
    /// Convert to a `RenderRequest` for the app layer.
    pub fn to_render_request(&self) -> Result<RenderRequest, String> {
        let input = wasm_input_source(self.sql.as_deref(), self.schema_json.as_deref())?;

        let output_format = self.format.unwrap_or(OutputFormat::Svg);

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

        let grouping = GroupingSpec {
            strategy: self.group_by.unwrap_or_default().into(),
        };

        let horizontal_spacing =
            validated_spacing(self.horizontal_spacing, 320.0, "horizontalSpacing")?;
        let vertical_spacing = validated_spacing(self.vertical_spacing, 80.0, "verticalSpacing")?;

        let layout = LayoutSpec {
            algorithm: self.layout_algorithm.unwrap_or_default(),
            direction: self.layout_direction.unwrap_or_default().into(),
            edge_style: self.edge_style.unwrap_or_default(),
            horizontal_spacing,
            vertical_spacing,
            force_iterations: 150,
            compaction: LayoutCompactionSpec::default(),
            ..Default::default()
        };
        let default_options = RenderOptions::default();

        Ok(RenderRequest {
            input,
            output_format,
            filter,
            focus,
            grouping,
            layout,
            options: RenderOptions {
                theme: self.theme.unwrap_or(default_options.theme),
                show_legend: self.show_legend.unwrap_or(default_options.show_legend),
                show_stats: self.show_stats.unwrap_or(default_options.show_stats),
            },
            output_path: None, // Not applicable in WASM
            overlay: None,
        })
    }
}

fn wasm_input_source(
    sql: Option<&str>,
    schema_json: Option<&str>,
) -> Result<relune_app::InputSource, String> {
    match (sql, schema_json) {
        (Some(sql), None) => Ok(relune_app::InputSource::sql_text(sql)),
        (None, Some(json)) => Ok(relune_app::InputSource::schema_json(json)),
        (Some(_), Some(_)) => Err("Cannot specify both 'sql' and 'schemaJson'".to_string()),
        (None, None) => Err("Must specify either 'sql' or 'schemaJson'".to_string()),
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
    /// Output format.
    #[serde(default)]
    pub format: Option<InspectFormat>,
}

impl WasmInspectRequest {
    /// Convert to an `InspectRequest` for the app layer.
    pub fn to_inspect_request(&self) -> Result<InspectRequest, String> {
        Ok(InspectRequest {
            input: wasm_input_source(self.sql.as_deref(), self.schema_json.as_deref())?,
            table: self.table.clone(),
            format: self.format.unwrap_or(InspectFormat::Json),
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
    /// Export format.
    #[serde(default)]
    pub format: Option<ExportFormat>,
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
    /// Grouping strategy.
    #[serde(default)]
    pub group_by: Option<WasmGroupBy>,
    /// Layout direction.
    #[serde(default)]
    pub layout_direction: Option<WasmLayoutDirection>,
    /// Layout algorithm.
    #[serde(default)]
    pub layout_algorithm: Option<LayoutAlgorithm>,
    /// Edge rendering style.
    #[serde(default)]
    pub edge_style: Option<RouteStyle>,
}

impl WasmExportRequest {
    /// Convert to an `ExportRequest` for the app layer.
    pub fn to_export_request(&self) -> Result<ExportRequest, String> {
        let format = self.format.unwrap_or(ExportFormat::SchemaJson);

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

        let grouping = GroupingSpec {
            strategy: self.group_by.unwrap_or_default().into(),
        };

        Ok(ExportRequest {
            input: wasm_input_source(self.sql.as_deref(), self.schema_json.as_deref())?,
            format,
            filter,
            focus,
            grouping,
            layout: LayoutSpec {
                direction: self.layout_direction.unwrap_or_default().into(),
                algorithm: self.layout_algorithm.unwrap_or_default(),
                edge_style: self.edge_style.unwrap_or_default(),
                ..Default::default()
            },
            output_path: None, // Not applicable in WASM
        })
    }
}

/// WASM-friendly lint request that can be deserialized from JavaScript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmLintRequest {
    /// SQL DDL text (optional, mutually exclusive with `schema_json`).
    pub sql: Option<String>,
    /// Pre-normalized schema JSON (optional, mutually exclusive with sql).
    pub schema_json: Option<String>,
    /// Lint profile used to seed the active rule set.
    #[serde(default)]
    pub profile: Option<LintProfile>,
    /// Output format.
    #[serde(default)]
    pub format: Option<LintFormat>,
    /// Optional specific rules to run.
    #[serde(default)]
    pub rules: Vec<String>,
    /// Optional rule IDs to exclude from the active rule set.
    #[serde(default)]
    pub exclude_rules: Vec<String>,
    /// Optional rule categories to keep.
    #[serde(default)]
    pub categories: Vec<LintRuleCategory>,
    /// Table patterns to suppress from the final report.
    #[serde(default)]
    pub except_tables: Vec<String>,
    /// Minimum severity that should be treated as failure.
    #[serde(default)]
    pub fail_on: Option<Severity>,
}

impl WasmLintRequest {
    /// Convert to a `LintRequest` for the app layer.
    pub fn to_lint_request(&self) -> Result<LintRequest, String> {
        Ok(LintRequest {
            input: wasm_input_source(self.sql.as_deref(), self.schema_json.as_deref())?,
            profile: self.profile.unwrap_or_default(),
            format: self.format.unwrap_or(LintFormat::Json),
            rules: self.rules.clone(),
            exclude_rules: self.exclude_rules.clone(),
            categories: self.categories.clone(),
            except_tables: self.except_tables.clone(),
            fail_on: self.fail_on,
        })
    }
}

/// WASM-friendly diff request that can be deserialized from JavaScript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmDiffRequest {
    /// Baseline SQL DDL text.
    pub before_sql: Option<String>,
    /// Baseline schema JSON.
    pub before_schema_json: Option<String>,
    /// Updated SQL DDL text.
    pub after_sql: Option<String>,
    /// Updated schema JSON.
    pub after_schema_json: Option<String>,
    /// Output format.
    #[serde(default)]
    pub format: Option<DiffFormat>,
    /// Tables to include (glob patterns).
    #[serde(default)]
    pub include_tables: Vec<String>,
    /// Tables to exclude (glob patterns).
    #[serde(default)]
    pub exclude_tables: Vec<String>,
    /// Grouping strategy.
    #[serde(default)]
    pub group_by: Option<WasmGroupBy>,
    /// Layout direction.
    #[serde(default)]
    pub layout_direction: Option<WasmLayoutDirection>,
    /// Layout algorithm.
    #[serde(default)]
    pub layout_algorithm: Option<LayoutAlgorithm>,
    /// Edge rendering style.
    #[serde(default)]
    pub edge_style: Option<RouteStyle>,
    /// Render theme.
    #[serde(default)]
    pub theme: Option<RenderTheme>,
    /// Whether to show a legend in rendered outputs.
    #[serde(default)]
    pub show_legend: Option<bool>,
    /// Whether to show render statistics in rendered outputs.
    #[serde(default)]
    pub show_stats: Option<bool>,
}

impl WasmDiffRequest {
    /// Convert to a `DiffRequest` for the app layer.
    pub fn to_diff_request(&self) -> Result<DiffRequest, String> {
        let filter = FilterSpec {
            include: self.include_tables.clone(),
            exclude: self.exclude_tables.clone(),
        };
        let grouping = GroupingSpec {
            strategy: self.group_by.unwrap_or_default().into(),
        };
        let default_options = RenderOptions::default();

        Ok(DiffRequest {
            before: wasm_input_source(
                self.before_sql.as_deref(),
                self.before_schema_json.as_deref(),
            )?,
            after: wasm_input_source(self.after_sql.as_deref(), self.after_schema_json.as_deref())?,
            format: self.format.unwrap_or(DiffFormat::Json),
            output_path: None,
            options: RenderOptions {
                theme: self.theme.unwrap_or(default_options.theme),
                show_legend: self.show_legend.unwrap_or(default_options.show_legend),
                show_stats: self.show_stats.unwrap_or(default_options.show_stats),
            },
            filter,
            grouping,
            layout: LayoutSpec {
                algorithm: self.layout_algorithm.unwrap_or_default(),
                direction: self.layout_direction.unwrap_or_default().into(),
                edge_style: self.edge_style.unwrap_or_default(),
                force_iterations: 150,
                compaction: LayoutCompactionSpec::default(),
                ..Default::default()
            },
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
            format: Some(OutputFormat::Svg),
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
            theme: None,
            show_legend: None,
            show_stats: None,
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
            layout_algorithm: Some(LayoutAlgorithm::ForceDirected),
            edge_style: Some(RouteStyle::Orthogonal),
            horizontal_spacing: None,
            vertical_spacing: None,
            theme: Some(RenderTheme::Light),
            show_legend: Some(false),
            show_stats: Some(false),
        };

        let render_req = req.to_render_request().unwrap();
        assert!(render_req.focus.is_some());
        let focus = render_req.focus.unwrap();
        assert_eq!(focus.table, "users");
        assert_eq!(focus.depth, 2);
        assert_eq!(render_req.layout.algorithm, LayoutAlgorithm::ForceDirected);
        assert_eq!(render_req.layout.edge_style, RouteStyle::Orthogonal);
        assert_eq!(render_req.options.theme, RenderTheme::Light);
        assert!(!render_req.options.show_legend);
        assert!(!render_req.options.show_stats);
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
            theme: None,
            show_legend: None,
            show_stats: None,
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
            theme: None,
            show_legend: None,
            show_stats: None,
        };

        assert!(req.to_render_request().is_err());
    }

    #[test]
    fn test_wasm_inspect_request() {
        let req = WasmInspectRequest {
            sql: Some("CREATE TABLE users (id INT);".to_string()),
            schema_json: None,
            table: Some("users".to_string()),
            format: Some(InspectFormat::Json),
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
            format: Some(ExportFormat::GraphJson),
            focus_table: None,
            depth: None,
            include_tables: vec![],
            exclude_tables: vec![],
            group_by: None,
            layout_direction: Some(WasmLayoutDirection::LeftToRight),
            layout_algorithm: Some(LayoutAlgorithm::ForceDirected),
            edge_style: Some(RouteStyle::Curved),
        };

        let export_req = req.to_export_request().unwrap();
        assert_eq!(export_req.format, ExportFormat::GraphJson);
        assert_eq!(export_req.layout.direction, LayoutDirection::LeftToRight);
        assert_eq!(export_req.layout.algorithm, LayoutAlgorithm::ForceDirected);
        assert_eq!(export_req.layout.edge_style, RouteStyle::Curved);
    }

    #[test]
    fn test_wasm_lint_request() {
        let req = WasmLintRequest {
            sql: Some("CREATE TABLE users (name TEXT);".to_string()),
            schema_json: None,
            profile: None,
            format: None,
            rules: vec!["no-primary-key".to_string()],
            exclude_rules: vec![],
            categories: vec![],
            except_tables: vec![],
            fail_on: Some(Severity::Warning),
        };

        let lint_req = req.to_lint_request().unwrap();
        assert_eq!(lint_req.format, LintFormat::Json);
        assert_eq!(lint_req.rules, vec!["no-primary-key"]);
        assert_eq!(lint_req.fail_on, Some(Severity::Warning));
    }

    #[test]
    fn test_wasm_diff_request() {
        let req = WasmDiffRequest {
            before_sql: Some("CREATE TABLE users (id INT PRIMARY KEY);".to_string()),
            before_schema_json: None,
            after_sql: Some(
                "CREATE TABLE users (id INT PRIMARY KEY, email TEXT NOT NULL);".to_string(),
            ),
            after_schema_json: None,
            format: Some(DiffFormat::Html),
            include_tables: vec!["users".to_string()],
            exclude_tables: vec![],
            group_by: Some(WasmGroupBy::Schema),
            layout_direction: Some(WasmLayoutDirection::RightToLeft),
            layout_algorithm: Some(LayoutAlgorithm::ForceDirected),
            edge_style: Some(RouteStyle::Straight),
            theme: Some(RenderTheme::Light),
            show_legend: Some(false),
            show_stats: Some(false),
        };

        let diff_req = req.to_diff_request().unwrap();
        assert_eq!(diff_req.format, DiffFormat::Html);
        assert_eq!(diff_req.filter.include, vec!["users"]);
        assert_eq!(diff_req.layout.direction, LayoutDirection::RightToLeft);
        assert_eq!(diff_req.layout.algorithm, LayoutAlgorithm::ForceDirected);
        assert_eq!(diff_req.layout.edge_style, RouteStyle::Straight);
        assert_eq!(diff_req.options.theme, RenderTheme::Light);
        assert!(!diff_req.options.show_legend);
        assert!(!diff_req.options.show_stats);
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
            theme: None,
            show_legend: None,
            show_stats: None,
        };

        let err = req
            .to_render_request()
            .expect_err("spacing should be rejected");
        assert!(err.contains("horizontalSpacing"));
    }
}
