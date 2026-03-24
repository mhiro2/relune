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
use relune_app::{export, inspect, render};
use request::{WasmExportRequest, WasmInspectRequest, WasmRenderRequest};
use wasm_bindgen::prelude::*;

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
/// - `edgeStyle`: Edge routing style - "straight", "orthogonal", or "curved"
/// - `horizontalSpacing`: Horizontal spacing hint (default: 320)
/// - `verticalSpacing`: Vertical spacing hint (default: 80)
///
/// Returns a JSON result object with:
/// - `content`: The rendered content (SVG, HTML, or JSON string)
/// - `diagnostics`: Array of diagnostic messages
/// - `stats`: Statistics about the rendering
#[wasm_bindgen]
pub fn render_from_sql(input: JsValue) -> Result<JsValue, JsValue> {
    let req: WasmRenderRequest = serde_wasm_bindgen::from_value(input)
        .map_err(|e| WasmError::new(format!("Invalid request: {e}")))?;

    let render_req = req.to_render_request().map_err(WasmError::new)?;

    let result = render(render_req).map_err(WasmError::from)?;

    serde_wasm_bindgen::to_value(&result).map_err(|e| WasmError::from(e).into())
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
        .map_err(|e| WasmError::new(format!("Invalid request: {e}")))?;

    let inspect_req = req.to_inspect_request().map_err(WasmError::new)?;

    let result = inspect(inspect_req).map_err(WasmError::from)?;

    serde_wasm_bindgen::to_value(&result).map_err(|e| WasmError::from(e).into())
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
/// - `edgeStyle`: Edge routing style - "straight", "orthogonal", or "curved"
///
/// Returns a JSON result object with:
/// - `content`: The exported JSON string
/// - `diagnostics`: Array of diagnostic messages
/// - `stats`: Statistics about the exported schema
#[wasm_bindgen]
pub fn export_from_sql(input: JsValue) -> Result<JsValue, JsValue> {
    let req: WasmExportRequest = serde_wasm_bindgen::from_value(input)
        .map_err(|e| WasmError::new(format!("Invalid request: {e}")))?;

    let export_req = req.to_export_request().map_err(WasmError::new)?;

    let result = export(export_req).map_err(WasmError::from)?;

    serde_wasm_bindgen::to_value(&result).map_err(|e| WasmError::from(e).into())
}

/// Export schema or graph data from schema JSON.
///
/// This is an alias for `export_from_sql` that expects `schemaJson` instead of `sql`.
/// See `export_from_sql` for full parameter documentation.
#[wasm_bindgen]
pub fn export_from_schema_json(input: JsValue) -> Result<JsValue, JsValue> {
    export_from_sql(input)
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

    let render_req = req.to_render_request().map_err(|e| JsValue::from_str(&e))?;

    let result = render(render_req).map_err(|e| JsValue::from_str(&e.to_string()))?;

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
        format: Some("html".to_string()),
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

    let render_req = req.to_render_request().map_err(|e| JsValue::from_str(&e))?;

    let result = render(render_req).map_err(|e| JsValue::from_str(&e.to_string()))?;

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
    use wasm_bindgen_test::*;

    use super::version;

    #[wasm_bindgen_test]
    fn wasm_version_is_non_empty() {
        assert!(!version().is_empty());
    }
}
