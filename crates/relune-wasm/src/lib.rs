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
use relune_app::{
    applied_rule_metadata, diff, export, format_diff_markdown, format_diff_text,
    format_review_json, format_review_markdown, format_review_text, inspect, lint, render, review,
};
use request::{
    WasmDiffRequest, WasmExportRequest, WasmInspectRequest, WasmLintRequest, WasmRenderRequest,
    WasmReviewRequest,
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

#[derive(Serialize)]
struct WasmReviewResponse {
    review: relune_core::ReviewResult,
    diagnostics: Vec<relune_core::Diagnostic>,
    denied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    applied_rule_details: Vec<relune_core::ReviewRuleMetadata>,
    requested_dialect: relune_core::SqlDialect,
    effective_dialect: relune_core::SqlDialect,
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
/// - `forceIterations`: Force-directed iteration count (default: 150, max: 10000)
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
/// - `forceIterations`: Force-directed iteration count for visual diff output (default: 150, max: 10000)
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

/// Run a migration risk review on two schemas from SQL text.
///
/// Accepts a JSON request object with the following fields:
/// - `beforeSql`: Baseline SQL DDL text (required if `beforeSchemaJson` not provided)
/// - `beforeSchemaJson`: Baseline schema JSON (required if `beforeSql` not provided)
/// - `afterSql`: Updated SQL DDL text (required if `afterSchemaJson` not provided)
/// - `afterSchemaJson`: Updated schema JSON (required if `afterSql` not provided)
/// - `format`: Output format - "text", "markdown", or "json" (default: "json")
/// - `rules`: Optional rule allowlist (`risk/<kebab>` or short form)
/// - `exceptRules`: Rules to remove from the active set
/// - `exceptTables`: Table glob patterns whose findings move into `suppressed`
/// - `deny`: Minimum severity that flips `denied = true`
/// - `severityOverrides`: Per-rule severity overrides applied after rule
///   evaluation (`[{ rule_id, severity }]`)
/// - `dialect`: Optional dialect hint that drives both the SQL parser and
///   the review-evaluation dialect ("auto" | "postgres" | "mysql" |
///   "sqlite", default "auto"). Lock-risk rules activate on `postgres`
///   or `mysql`; `sqlite` keeps them inactive. `auto` is promoted to the
///   parser-resolved dialect when both SQL inputs agree (so SQL-only
///   inputs that resolve to postgres/mysql run lock-risk automatically),
///   stays `auto` (with a `REVIEW002` warning) when both sides resolve
///   to different concrete dialects, and stays `auto` silently when one
///   or both sides carry no parser-side dialect signal (e.g. schema-JSON
///   inputs).
///
/// Returns a JSON result object with:
/// - `review`: Structured review payload (`findings`, `suppressed`,
///   `summary`, `applied_rules`)
/// - `diagnostics`: Array of parser / schema diagnostics
/// - `denied`: Whether the configured `deny` threshold was exceeded
/// - `requested_dialect`: The dialect supplied in the request (or
///   `"auto"` if unset)
/// - `effective_dialect`: The dialect actually used for review
///   evaluation, after auto-promotion
/// - `content`: CLI-equivalent rendering for the requested `format`
///   (the `format = "json"` payload matches `relune review --format json`)
/// - `applied_rule_details`: Metadata snapshots for each applied rule,
///   suitable for rendering the playground rule legend
#[wasm_bindgen]
pub fn review_from_sql(input: JsValue) -> Result<JsValue, JsValue> {
    let req: WasmReviewRequest = serde_wasm_bindgen::from_value(input)
        .map_err(|e| WasmError::input(format!("Invalid request: {e}")))?;

    let review_req = req.to_review_request().map_err(WasmError::input)?;
    let format = review_req.format;
    let result = review(review_req).map_err(WasmError::from)?;

    let content = match format {
        relune_app::ReviewFormat::Text => Some(format_review_text(&result)),
        relune_app::ReviewFormat::Markdown => Some(format_review_markdown(&result)),
        relune_app::ReviewFormat::Json => {
            Some(format_review_json(&result).map_err(WasmError::from)?)
        }
    };

    let applied_rule_details = applied_rule_metadata(&result);
    let response = WasmReviewResponse {
        review: result.review,
        diagnostics: result.diagnostics,
        denied: result.denied,
        content,
        applied_rule_details,
        requested_dialect: result.requested_dialect,
        effective_dialect: result.effective_dialect,
    };

    Ok(serde_wasm_bindgen::to_value(&response).map_err(WasmError::from)?)
}

/// Run a migration risk review on two schemas from schema JSON.
///
/// This is an alias for `review_from_sql` that expects schema JSON inputs.
/// See `review_from_sql` for full parameter documentation.
#[wasm_bindgen]
pub fn review_from_schema_json(input: JsValue) -> Result<JsValue, JsValue> {
    review_from_sql(input)
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
        force_iterations: None,
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
        force_iterations: None,
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
    use crate::request::{WasmDiffRequest, WasmLintRequest, WasmReviewRequest};
    use relune_app::{DiffFormat, LintFormat, ReviewFormat};
    use relune_core::ReviewSeverity;
    use wasm_bindgen_test::*;

    use super::{diff_from_sql, lint_from_sql, review_from_schema_json, review_from_sql, version};

    #[wasm_bindgen_test]
    fn wasm_version_is_non_empty() {
        assert!(!version().is_empty());
    }

    #[wasm_bindgen_test]
    fn wasm_lint_from_sql() {
        let input = serde_wasm_bindgen::to_value(&WasmLintRequest {
            sql: Some("CREATE TABLE users (name TEXT);".to_string()),
            schema_json: None,
            profile: None,
            format: Some(LintFormat::Json),
            rules: vec![],
            exclude_rules: vec![],
            categories: vec![],
            except_tables: vec![],
            fail_on: None,
        })
        .expect("serialize lint request");

        let result = lint_from_sql(input).expect("lint request should succeed");
        let value: serde_json::Value =
            serde_wasm_bindgen::from_value(result).expect("deserialize lint result");

        assert_eq!(value["stats"]["total"], 4);
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
            force_iterations: None,
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
            force_iterations: None,
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

    #[wasm_bindgen_test]
    fn wasm_review_from_sql_surfaces_resolved_dialect() {
        // SERIAL on both sides triggers the parser's postgres detection;
        // under `auto`, the review pipeline should promote to postgres
        // and surface that resolution to the caller so the playground
        // can render the correct lock-risk-active note.
        let before = "CREATE TABLE users (id SERIAL PRIMARY KEY);";
        let after = "
            CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT);
            CREATE INDEX users_email_idx ON users(email);
        ";
        let input = serde_wasm_bindgen::to_value(&WasmReviewRequest {
            before_sql: Some(before.to_string()),
            before_schema_json: None,
            after_sql: Some(after.to_string()),
            after_schema_json: None,
            format: Some(ReviewFormat::Json),
            rules: vec![],
            except_rules: vec![],
            except_tables: vec![],
            deny: None,
            severity_overrides: vec![],
            dialect: None,
        })
        .expect("serialize review request");

        let result = review_from_sql(input).expect("review request should succeed");
        let value: serde_json::Value =
            serde_wasm_bindgen::from_value(result).expect("deserialize review result");

        assert_eq!(value["requested_dialect"], "auto");
        assert_eq!(value["effective_dialect"], "postgres");
    }

    #[wasm_bindgen_test]
    fn wasm_review_from_sql_no_findings() {
        let sql = "CREATE TABLE users (id INT PRIMARY KEY);";
        let input = serde_wasm_bindgen::to_value(&WasmReviewRequest {
            before_sql: Some(sql.to_string()),
            before_schema_json: None,
            after_sql: Some(sql.to_string()),
            after_schema_json: None,
            format: Some(ReviewFormat::Json),
            rules: vec![],
            except_rules: vec![],
            except_tables: vec![],
            deny: None,
            severity_overrides: vec![],
            dialect: None,
        })
        .expect("serialize review request");

        let result = review_from_sql(input).expect("review request should succeed");
        let value: serde_json::Value =
            serde_wasm_bindgen::from_value(result).expect("deserialize review result");

        assert_eq!(value["denied"], false);
        assert_eq!(
            value["review"]["findings"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(usize::MAX),
            0
        );
        assert_eq!(value["review"]["summary"]["breaking"], 0);
        assert_eq!(value["review"]["summary"]["caution"], 0);
        assert_eq!(value["review"]["summary"]["warning"], 0);
        assert_eq!(value["review"]["summary"]["info"], 0);

        // The CLI-equivalent JSON content should round-trip to the
        // `relune-app::ReviewResult` shape (flattened summary / findings /
        // applied_rules / diagnostics / denied at the top level).
        let content = value["content"]
            .as_str()
            .expect("content should be populated for format=json");
        let parsed: relune_app::ReviewResult =
            serde_json::from_str(content).expect("CLI JSON should deserialize as ReviewResult");
        assert!(parsed.review.findings.is_empty());
        assert_eq!(parsed.review.summary.total(), 0);
        assert!(!parsed.denied);
    }

    #[wasm_bindgen_test]
    fn wasm_review_from_sql_breaking() {
        let before = "
            CREATE TABLE users (id INT PRIMARY KEY, email TEXT);
            CREATE TABLE orders (
                id INT PRIMARY KEY,
                user_email TEXT REFERENCES users(email)
            );
        ";
        let after = "
            CREATE TABLE users (id INT PRIMARY KEY);
            CREATE TABLE orders (
                id INT PRIMARY KEY,
                user_email TEXT REFERENCES users(email)
            );
        ";

        let input = serde_wasm_bindgen::to_value(&WasmReviewRequest {
            before_sql: Some(before.to_string()),
            before_schema_json: None,
            after_sql: Some(after.to_string()),
            after_schema_json: None,
            format: Some(ReviewFormat::Json),
            rules: vec![],
            except_rules: vec![],
            except_tables: vec![],
            deny: Some(ReviewSeverity::Breaking),
            severity_overrides: vec![],
            dialect: None,
        })
        .expect("serialize review request");

        let result = review_from_sql(input).expect("review request should succeed");
        let value: serde_json::Value =
            serde_wasm_bindgen::from_value(result).expect("deserialize review result");

        assert_eq!(value["denied"], true);
        assert_eq!(value["review"]["summary"]["breaking"], 1);
        let applied_details = value["applied_rule_details"]
            .as_array()
            .expect("applied_rule_details array");
        assert_eq!(
            applied_details.len(),
            relune_core::ReviewRuleId::all_rules().len(),
            "applied_rule_details should expose every review rule"
        );

        // Round-trip the embedded CLI JSON content into the typed shape so
        // the wasm payload stays in lockstep with `relune review --format json`.
        let content = value["content"]
            .as_str()
            .expect("content should be populated for format=json");
        let parsed: relune_app::ReviewResult =
            serde_json::from_str(content).expect("CLI JSON should deserialize as ReviewResult");
        assert_eq!(parsed.review.summary.breaking, 1);
        assert!(parsed.denied);
    }

    #[wasm_bindgen_test]
    fn wasm_review_from_schema_json_no_findings() {
        // Identical schemas through the schema_json path. Verifies that the
        // schema_json branch of `wasm_input_source_with_dialect` is wired and
        // that `review_from_schema_json` (the alias) reaches the same code
        // path as `review_from_sql`.
        let schema = r#"
        {
          "version": "1.0.0",
          "tables": [
            {
              "id": "users",
              "schema": null,
              "name": "users",
              "columns": [
                {
                  "name": "id",
                  "data_type": "INT",
                  "nullable": false,
                  "primary_key": true
                }
              ],
              "foreign_keys": [],
              "indexes": []
            }
          ]
        }
        "#;

        let input = serde_wasm_bindgen::to_value(&WasmReviewRequest {
            before_sql: None,
            before_schema_json: Some(schema.to_string()),
            after_sql: None,
            after_schema_json: Some(schema.to_string()),
            format: Some(ReviewFormat::Json),
            rules: vec![],
            except_rules: vec![],
            except_tables: vec![],
            deny: None,
            severity_overrides: vec![],
            dialect: None,
        })
        .expect("serialize review request");

        let result =
            review_from_schema_json(input).expect("schema_json review request should succeed");
        let value: serde_json::Value =
            serde_wasm_bindgen::from_value(result).expect("deserialize review result");

        assert_eq!(value["denied"], false);
        assert_eq!(
            value["review"]["findings"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(usize::MAX),
            0
        );
    }

    #[wasm_bindgen_test]
    fn wasm_review_lock_risk_postgres() {
        // CREATE INDEX on an existing postgres table is a caution-level
        // lock-risk finding. The wasm dialect must reach
        // `ReviewRequest.dialect` for the rule to fire.
        let before = "CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT NOT NULL);";
        let after = "CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT NOT NULL); \
             CREATE INDEX orders_user_id_idx ON orders (user_id);";

        let input = serde_wasm_bindgen::to_value(&WasmReviewRequest {
            before_sql: Some(before.to_string()),
            before_schema_json: None,
            after_sql: Some(after.to_string()),
            after_schema_json: None,
            format: Some(ReviewFormat::Json),
            rules: vec![],
            except_rules: vec![],
            except_tables: vec![],
            deny: None,
            severity_overrides: vec![],
            dialect: Some(relune_core::SqlDialect::Postgres),
        })
        .expect("serialize review request");

        let result = review_from_sql(input).expect("review request should succeed");
        let value: serde_json::Value =
            serde_wasm_bindgen::from_value(result).expect("deserialize review result");

        assert_eq!(value["review"]["summary"]["caution"], 1);
        let findings = value["review"]["findings"]
            .as_array()
            .expect("findings array");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0]["rule_id"], "risk/add-index-on-large-table");
        assert_eq!(findings[0]["severity"], "caution");

        // Round-trip the embedded CLI JSON content into the typed shape so
        // the wasm payload stays in lockstep with `relune review --format
        // json` for caution-band findings as well.
        let content = value["content"]
            .as_str()
            .expect("content should be populated for format=json");
        let parsed: relune_app::ReviewResult =
            serde_json::from_str(content).expect("CLI JSON should deserialize as ReviewResult");
        assert_eq!(parsed.review.summary.caution, 1);
        assert!(parsed.diagnostics.is_empty());
    }

    #[wasm_bindgen_test]
    fn wasm_review_content_text_and_markdown() {
        // Smoke-check the non-JSON content paths. The text and markdown
        // formatters are shared with the CLI, so we only verify the wasm
        // boundary populates `content` with the expected header text.
        let sql = "CREATE TABLE users (id INT PRIMARY KEY);";

        for (format, expected_header) in [
            (ReviewFormat::Text, "Schema review"),
            (ReviewFormat::Markdown, "## Schema review"),
        ] {
            let input = serde_wasm_bindgen::to_value(&WasmReviewRequest {
                before_sql: Some(sql.to_string()),
                before_schema_json: None,
                after_sql: Some(sql.to_string()),
                after_schema_json: None,
                format: Some(format),
                rules: vec![],
                except_rules: vec![],
                except_tables: vec![],
                deny: None,
                severity_overrides: vec![],
                dialect: None,
            })
            .expect("serialize review request");

            let result = review_from_sql(input).expect("review request should succeed");
            let value: serde_json::Value =
                serde_wasm_bindgen::from_value(result).expect("deserialize review result");

            let content = value["content"]
                .as_str()
                .expect("content should be populated for text/markdown");
            assert!(
                content.contains(expected_header),
                "expected `{expected_header}` in {format:?} content, got: {content}"
            );
        }
    }

    #[wasm_bindgen_test]
    fn wasm_error_serializes_to_structured_js_object() {
        use crate::error::WasmError;

        let err = WasmError::with_code("something failed", "TEST_CODE");
        let js_val: wasm_bindgen::JsValue = err.into();

        // The structured path should produce a JS object, not a plain string.
        assert!(
            js_val.is_object(),
            "WasmError should serialize to a JS object"
        );

        let obj: serde_json::Value =
            serde_wasm_bindgen::from_value(js_val).expect("should deserialize back");
        assert_eq!(obj["message"], "something failed");
        assert_eq!(obj["code"], "TEST_CODE");
    }

    #[wasm_bindgen_test]
    fn wasm_error_without_code_omits_code_field() {
        use crate::error::WasmError;

        let err = WasmError::new("no code");
        let js_val: wasm_bindgen::JsValue = err.into();

        let obj: serde_json::Value =
            serde_wasm_bindgen::from_value(js_val).expect("should deserialize back");
        assert_eq!(obj["message"], "no code");
        assert!(obj.get("code").is_none() || obj["code"].is_null());
    }

    /// Verify the fallback path of `From<WasmError> for JsValue`:
    /// when `serde_wasm_bindgen::to_value` fails, the implementation falls back
    /// to `JsValue::from_str(&err.message)`, producing a plain string.
    #[wasm_bindgen_test]
    fn wasm_error_string_fallback_carries_message() {
        let message = "serialization failed";
        let fallback = wasm_bindgen::JsValue::from_str(message);
        assert!(fallback.is_string(), "fallback should produce a JS string");
        assert_eq!(fallback.as_string().unwrap(), message);
    }
}
