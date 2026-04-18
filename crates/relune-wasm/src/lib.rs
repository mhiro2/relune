//! WASM bindings for relune.
//!
//! This crate provides WebAssembly bindings for rendering ERD diagrams
//! from SQL or schema JSON in browser environments.
//!
//! # Example (JavaScript)
//!
//! ```javascript
//! import init, { render_from_sql, set_panic_hook } from 'relune-wasm';
//!
//! await init();
//! set_panic_hook();
//!
//! const result = render_from_sql({
//!     sql: 'CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(255));',
//!     format: 'svg'
//! });
//!
//! console.log(result.content);
//! ```

mod error;
mod request;

use error::WasmError;
use relune_app::{diff, export, format_diff_markdown, format_diff_text, inspect, lint, render};
use request::{
    WasmDiffRequest, WasmExportRequest, WasmInspectRequest, WasmLintRequest, WasmRenderRequest,
};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
struct WasmDiffResponse {
    diff: relune_core::SchemaDiff,
    diagnostics: Vec<relune_core::Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rendered: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

/// Set the panic hook for better error messages in the browser.
///
/// This function should be called once during initialization.
/// It provides better panic messages with stack traces in the
/// browser console.
#[wasm_bindgen]
pub fn set_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// Initialize the WASM module.
///
/// This is an optional convenience function that sets up the panic hook.
/// You can also call `set_panic_hook()` directly.
#[wasm_bindgen]
pub fn init() {
    set_panic_hook();
}

/// Render an ERD from SQL text.
///
/// Accepts a JSON request object with the following fields:
/// - `sql`: SQL DDL text (required if schemaJson not provided)
/// - `schemaJson`: Pre-normalized schema JSON (required if sql not provided)
/// - `format`: Output format - "svg", "html", "graph-json", "schema-json" (default: "svg")
/// - `focusTable`: Table name to focus on (optional)
/// - `depth`: Focus depth (default: 1)
/// - `includeTables`: Tables to include (glob patterns)
/// - `excludeTables`: Tables to exclude (glob patterns)
/// - `groupBy`: Grouping strategy - "none", "schema", "prefix" (default: "none")
/// - `layoutDirection`: Layout direction - "top-to-bottom", "left-to-right", etc.
/// - `layoutAlgorithm`: Layout algorithm - "hierarchical" or "force-directed"
/// - `edgeStyle`: Edge rendering style - "straight", "orthogonal", or "curved"
/// - `horizontalSpacing`: Horizontal spacing hint (default: 320)
/// - `verticalSpacing`: Vertical spacing hint (default: 80)
/// - `theme`: Render theme - "light" or "dark" (default: "dark")
/// - `showLegend`: Whether to show the legend (default: true)
/// - `showStats`: Whether to show render statistics inside the output (default: true)
///
/// Returns a JSON result object with:
/// - `content`: The rendered content (SVG, HTML, or JSON string)
/// - `diagnostics`: Array of diagnostic messages
/// - `stats`: Statistics about the rendering
#[wasm_bindgen]
pub fn render_from_sql(input: JsValue) -> Result<JsValue, JsValue> {
    let req: WasmRenderRequest = serde_wasm_bindgen::from_value(input)
        .map_err(|e| WasmError::input(format!("Invalid request: {e}")))?;

    let render_req = req.to_render_request().map_err(WasmError::input)?;

    let result = render(render_req).map_err(WasmError::from)?;

    Ok(serde_wasm_bindgen::to_value(&result).map_err(WasmError::from)?)
}

/// Render an ERD from schema JSON.
///
/// This is an alias for `render_from_sql` that expects `schemaJson` instead of `sql`.
/// See `render_from_sql` for full parameter documentation.
#[wasm_bindgen]
pub fn render_from_schema_json(input: JsValue) -> Result<JsValue, JsValue> {
    render_from_sql(input)
}

/// Inspect schema metadata from SQL text.
///
/// Accepts a JSON request object with the following fields:
/// - `sql`: SQL DDL text (required if schemaJson not provided)
/// - `schemaJson`: Pre-normalized schema JSON (required if sql not provided)
/// - `table`: Table name to inspect (optional, returns schema summary if not specified)
/// - `format`: Output format - "json" or "text" (default: "json")
///
/// Returns a JSON result object with:
/// - `summary`: Schema summary (table count, column count, etc.)
/// - `table`: Table details (if a specific table was requested)
/// - `diagnostics`: Array of diagnostic messages
#[wasm_bindgen]
pub fn inspect_from_sql(input: JsValue) -> Result<JsValue, JsValue> {
    let req: WasmInspectRequest = serde_wasm_bindgen::from_value(input)
        .map_err(|e| WasmError::input(format!("Invalid request: {e}")))?;

    let inspect_req = req.to_inspect_request().map_err(WasmError::input)?;

    let result = inspect(inspect_req).map_err(WasmError::from)?;

    Ok(serde_wasm_bindgen::to_value(&result).map_err(WasmError::from)?)
}

/// Inspect schema metadata from schema JSON.
///
/// This is an alias for `inspect_from_sql` that expects `schemaJson` instead of `sql`.
/// See `inspect_from_sql` for full parameter documentation.
#[wasm_bindgen]
pub fn inspect_from_schema_json(input: JsValue) -> Result<JsValue, JsValue> {
    inspect_from_sql(input)
}

/// Export schema or graph data from SQL text.
///
/// Accepts a JSON request object with the following fields:
/// - `sql`: SQL DDL text (required if schemaJson not provided)
/// - `schemaJson`: Pre-normalized schema JSON (required if sql not provided)
/// - `format`: Export format - "schema-json", "graph-json", "layout-json", "mermaid", "d2", "dot" (default: "schema-json")
/// - `focusTable`: Table name to focus on (optional)
/// - `depth`: Focus depth (default: 1)
/// - `includeTables`: Tables to include (glob patterns)
/// - `excludeTables`: Tables to exclude (glob patterns)
/// - `groupBy`: Grouping strategy - "none", "schema", "prefix" (default: "none")
/// - `layoutAlgorithm`: Layout algorithm - "hierarchical" or "force-directed"
/// - `edgeStyle`: Edge rendering style - "straight", "orthogonal", or "curved"
///
/// Returns a JSON result object with:
/// - `content`: The exported JSON string
/// - `diagnostics`: Array of diagnostic messages
/// - `stats`: Statistics about the exported schema
#[wasm_bindgen]
pub fn export_from_sql(input: JsValue) -> Result<JsValue, JsValue> {
    let req: WasmExportRequest = serde_wasm_bindgen::from_value(input)
        .map_err(|e| WasmError::input(format!("Invalid request: {e}")))?;

    let export_req = req.to_export_request().map_err(WasmError::input)?;

    let result = export(export_req).map_err(WasmError::from)?;

    Ok(serde_wasm_bindgen::to_value(&result).map_err(WasmError::from)?)
}

/// Export schema or graph data from schema JSON.
///
/// This is an alias for `export_from_sql` that expects `schemaJson` instead of `sql`.
/// See `export_from_sql` for full parameter documentation.
#[wasm_bindgen]
pub fn export_from_schema_json(input: JsValue) -> Result<JsValue, JsValue> {
    export_from_sql(input)
}

/// Run lint diagnostics from SQL text.
///
/// Accepts a JSON request object with the following fields:
/// - `sql`: SQL DDL text (required if schemaJson not provided)
/// - `schemaJson`: Pre-normalized schema JSON (required if sql not provided)
/// - `format`: Output format - "json" or "text" (default: "json")
/// - `rules`: Optional list of lint rule ids to run
/// - `failOn`: Optional minimum severity that should be treated as failure
///
/// Returns a JSON result object with:
/// - `issues`: Array of lint issues
/// - `stats`: Lint summary counts
/// - `diagnostics`: Array of parser / schema diagnostics
#[wasm_bindgen]
pub fn lint_from_sql(input: JsValue) -> Result<JsValue, JsValue> {
    let req: WasmLintRequest = serde_wasm_bindgen::from_value(input)
        .map_err(|e| WasmError::input(format!("Invalid request: {e}")))?;

    let lint_req = req.to_lint_request().map_err(WasmError::input)?;
    let result = lint(lint_req).map_err(WasmError::from)?;

    Ok(serde_wasm_bindgen::to_value(&result).map_err(WasmError::from)?)
}

/// Run lint diagnostics from schema JSON.
///
/// This is an alias for `lint_from_sql` that expects `schemaJson` instead of `sql`.
/// See `lint_from_sql` for full parameter documentation.
#[wasm_bindgen]
pub fn lint_from_schema_json(input: JsValue) -> Result<JsValue, JsValue> {
    lint_from_sql(input)
}

/// Compare two schemas from SQL text.
///
/// Accepts a JSON request object with the following fields:
/// - `beforeSql`: Baseline SQL DDL text (required if beforeSchemaJson not provided)
/// - `beforeSchemaJson`: Baseline schema JSON (required if beforeSql not provided)
/// - `afterSql`: Updated SQL DDL text (required if afterSchemaJson not provided)
/// - `afterSchemaJson`: Updated schema JSON (required if afterSql not provided)
/// - `format`: Output format - "json", "text", "markdown", "svg", or "html" (default: "json")
/// - `includeTables`: Tables to include (glob patterns)
/// - `excludeTables`: Tables to exclude (glob patterns)
/// - `groupBy`: Grouping strategy - "none", "schema", "prefix" (default: "none")
/// - `layoutDirection`: Layout direction for visual diff output
/// - `layoutAlgorithm`: Layout algorithm for visual diff output
/// - `edgeStyle`: Edge rendering style for visual diff output
/// - `theme`: Render theme for visual diff output
/// - `showLegend`: Whether to include the legend in visual diff output
/// - `showStats`: Whether to include stats in visual diff output
///
/// Returns a JSON result object with:
/// - `diff`: Structured schema diff
/// - `diagnostics`: Array of parser / schema diagnostics
/// - `rendered`: Visual diff output when `format` is "svg" or "html"
/// - `content`: Formatted text / markdown / json output for non-visual formats
#[wasm_bindgen]
pub fn diff_from_sql(input: JsValue) -> Result<JsValue, JsValue> {
    let req: WasmDiffRequest = serde_wasm_bindgen::from_value(input)
        .map_err(|e| WasmError::input(format!("Invalid request: {e}")))?;

    let diff_req = req.to_diff_request().map_err(WasmError::input)?;
    let format = diff_req.format;
    let result = diff(diff_req).map_err(WasmError::from)?;

    let content = match format {
        relune_app::DiffFormat::Text => Some(format_diff_text(&result)),
        relune_app::DiffFormat::Markdown => Some(format_diff_markdown(&result)),
        relune_app::DiffFormat::Json => Some(
            serde_json::to_string_pretty(&result)
                .map_err(|error| WasmError::with_code(error.to_string(), "SERIALIZATION_ERROR"))?,
        ),
        relune_app::DiffFormat::Svg | relune_app::DiffFormat::Html => None,
    };

    let response = WasmDiffResponse {
        diff: result.diff,
        diagnostics: result.diagnostics,
        rendered: result.rendered,
        content,
    };

    Ok(serde_wasm_bindgen::to_value(&response).map_err(WasmError::from)?)
}

/// Compare two schemas from schema JSON.
///
/// This is an alias for `diff_from_sql` that expects schema JSON inputs.
/// See `diff_from_sql` for full parameter documentation.
#[wasm_bindgen]
pub fn diff_from_schema_json(input: JsValue) -> Result<JsValue, JsValue> {
    diff_from_sql(input)
}

// ============================================================================
// Convenience functions for simpler use cases
// ============================================================================

/// Simple render from SQL - returns just the SVG string.
///
/// This is a convenience function for the common case of rendering
/// a simple SVG from SQL without any options.
#[wasm_bindgen(js_name = renderSvgFromSql)]
pub fn render_svg_from_sql(sql: &str) -> Result<String, JsValue> {
    let req = WasmRenderRequest {
        sql: Some(sql.to_string()),
        schema_json: None,
        format: Some(relune_app::OutputFormat::Svg),
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

    let render_req = req.to_render_request().map_err(WasmError::input)?;

    let result = render(render_req).map_err(WasmError::from)?;

    Ok(result.content)
}

/// Simple render HTML from SQL - returns just the HTML string.
///
/// This is a convenience function for rendering a self-contained HTML
/// document with embedded SVG.
#[wasm_bindgen(js_name = renderHtmlFromSql)]
pub fn render_html_from_sql(sql: &str) -> Result<String, JsValue> {
    let req = WasmRenderRequest {
        sql: Some(sql.to_string()),
        schema_json: None,
        format: Some(relune_app::OutputFormat::Html),
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

    let render_req = req.to_render_request().map_err(WasmError::input)?;

    let result = render(render_req).map_err(WasmError::from)?;

    Ok(result.content)
}

/// Get version info.
#[wasm_bindgen]
#[must_use]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        let v = version();
        assert!(!v.is_empty());
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_bindgen_tests {
    use crate::request::{WasmDiffRequest, WasmLintRequest};
    use relune_app::{DiffFormat, LintFormat};
    use wasm_bindgen_test::*;

    use super::{diff_from_sql, lint_from_sql, version};

    #[wasm_bindgen_test]
    fn wasm_version_is_non_empty() {
        assert!(!version().is_empty());
    }

    #[wasm_bindgen_test]
    fn wasm_lint_from_sql() {
        let input = serde_wasm_bindgen::to_value(&WasmLintRequest {
            sql: Some("CREATE TABLE users (name TEXT);".to_string()),
            schema_json: None,
            format: Some(LintFormat::Json),
            rules: vec![],
            fail_on: None,
        })
        .expect("serialize lint request");

        let result = lint_from_sql(input).expect("lint request should succeed");
        let value: serde_json::Value =
            serde_wasm_bindgen::from_value(result).expect("deserialize lint result");

        assert_eq!(value["stats"]["total"], 3);
    }

    #[wasm_bindgen_test]
    fn wasm_diff_from_sql_visual() {
        let input = serde_wasm_bindgen::to_value(&WasmDiffRequest {
            before_sql: Some("CREATE TABLE users (id INT PRIMARY KEY);".to_string()),
            before_schema_json: None,
            after_sql: Some(
                "CREATE TABLE users (id INT PRIMARY KEY, email TEXT NOT NULL);".to_string(),
            ),
            after_schema_json: None,
            format: Some(DiffFormat::Html),
            include_tables: vec![],
            exclude_tables: vec![],
            group_by: None,
            layout_direction: None,
            layout_algorithm: None,
            edge_style: None,
            theme: None,
            show_legend: None,
            show_stats: None,
        })
        .expect("serialize diff request");

        let result = diff_from_sql(input).expect("diff request should succeed");
        let value: serde_json::Value =
            serde_wasm_bindgen::from_value(result).expect("deserialize diff result");

        assert_eq!(value["diff"]["summary"]["tables_modified"], 1);
        assert!(
            value["rendered"]
                .as_str()
                .unwrap_or_default()
                .contains("<html")
        );
    }

    #[wasm_bindgen_test]
    fn wasm_diff_from_sql_markdown() {
        let input = serde_wasm_bindgen::to_value(&WasmDiffRequest {
            before_sql: Some("CREATE TABLE users (id INT PRIMARY KEY);".to_string()),
            before_schema_json: None,
            after_sql: Some(
                "CREATE TABLE users (id INT PRIMARY KEY, email TEXT NOT NULL);".to_string(),
            ),
            after_schema_json: None,
            format: Some(DiffFormat::Markdown),
            include_tables: vec![],
            exclude_tables: vec![],
            group_by: None,
            layout_direction: None,
            layout_algorithm: None,
            edge_style: None,
            theme: None,
            show_legend: None,
            show_stats: None,
        })
        .expect("serialize diff request");

        let result = diff_from_sql(input).expect("diff request should succeed");
        let value: serde_json::Value =
            serde_wasm_bindgen::from_value(result).expect("deserialize diff result");

        assert!(
            value["content"]
                .as_str()
                .unwrap_or_default()
                .contains("## Schema Diff")
        );
    }
}
