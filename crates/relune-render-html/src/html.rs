//! HTML document generation.
//!
//! Embedded viewer scripts are generated from TypeScript in `ts/` into `src/js/` by `build.rs` (`pnpm run build`; outputs are gitignored).

use crate::options::{HtmlRenderOptions, Theme};
use relune_render_theme::get_colors;

/// Escape JSON content for safe embedding in a script tag.
pub fn escape_json_for_script(json: &str) -> String {
    // For JSON inside a script tag, we need to escape </script> to prevent
    // premature script termination. We also escape <!-- and --> to prevent
    // HTML comment issues in older browsers.
    json.replace("</script>", "<\\/script>")
        .replace("<!--", "<\\!--")
        .replace("-->", "-\\->")
}

/// Build the complete HTML document.
#[allow(clippy::needless_raw_string_hashes)]
#[allow(clippy::too_many_lines)]
pub fn build_html_document(svg: &str, metadata_json: &str, options: &HtmlRenderOptions) -> String {
    let title = options.title.as_deref().unwrap_or("Relune ERD");

    let css = build_css(
        options.theme,
        options.enable_group_toggles,
        options.enable_search,
        options.enable_column_type_filter,
        options.enable_collapse,
        options.enable_highlight,
    );
    // Combine all enabled JS modules
    let mut js_parts: Vec<&str> = Vec::new();
    if options.enable_pan_zoom {
        js_parts.push(build_pan_zoom_js());
    }
    if options.enable_group_toggles {
        js_parts.push(build_group_toggle_js());
    }
    if options.enable_search {
        js_parts.push(build_search_js());
        if options.enable_column_type_filter {
            js_parts.push(build_type_filter_js());
        }
    }
    if options.enable_collapse {
        js_parts.push(build_collapse_js());
    }
    if options.enable_highlight {
        js_parts.push(build_highlight_js());
    }
    let js = if js_parts.is_empty() {
        None
    } else {
        Some(js_parts.join("\n\n"))
    };

    let heading = if options.title.is_some() {
        Some(format!(
            "<h1>{}</h1>",
            html_escape(options.title.as_deref().unwrap())
        ))
    } else {
        None
    };

    let group_panel = if options.enable_group_toggles {
        Some(build_group_panel_html())
    } else {
        None
    };

    let search_panel = if options.enable_search {
        Some(build_search_panel_html(options.enable_column_type_filter))
    } else {
        None
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{title}</title>
  <style>
{css}
  </style>
</head>
<body>
{heading}
{search_panel}
{group_panel}
  <div class="container">
    <div class="viewport" id="viewport">
      <div class="canvas" id="canvas">
{svg}
      </div>
    </div>
  </div>
  <script type="application/json" id="relune-metadata" data-relune-metadata>
{metadata_json}
  </script>
{js}
</body>
</html>"#,
        title = html_escape(title),
        heading = heading.unwrap_or_default(),
        search_panel = search_panel.unwrap_or_default(),
        group_panel = group_panel.unwrap_or_default(),
        css = css,
        svg = indent_svg(svg),
        metadata_json = metadata_json,
        js = js
            .map(|j| format!("\n  <script>\n{j}\n  </script>"))
            .unwrap_or_default()
    )
}

/// Build CSS styles based on theme and options.
#[allow(clippy::too_many_lines)]
#[allow(clippy::fn_params_excessive_bools)]
fn build_css(
    theme: Theme,
    enable_group_toggles: bool,
    enable_search: bool,
    enable_column_type_filter: bool,
    _enable_collapse: bool,
    enable_highlight: bool,
) -> String {
    let colors = get_colors(theme);

    let search_css = if enable_search {
        r"
    /* Search panel styles */
    .search-panel {
      position: fixed;
      top: 12px;
      left: 12px;
      width: 300px;
      background-color: var(--bg-color);
      border: 1px solid var(--border-color);
      border-radius: 8px;
      z-index: 200;
      overflow: hidden;
      box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
    }

    body:has(h1) .search-panel {
      top: 61px;
    }

    .search-container {
      display: flex;
      align-items: center;
      padding: 10px 12px;
      gap: 8px;
    }

    .search-icon {
      flex-shrink: 0;
      width: 16px;
      height: 16px;
      opacity: 0.5;
    }

    .search-input {
      flex: 1;
      border: none;
      background: transparent;
      font-size: 14px;
      color: var(--text-color);
      outline: none;
    }

    .search-input::placeholder {
      opacity: 0.6;
    }

    .search-clear {
      flex-shrink: 0;
      width: 20px;
      height: 20px;
      border: none;
      background: transparent;
      color: var(--text-color);
      cursor: pointer;
      opacity: 0;
      border-radius: 50%;
      display: flex;
      align-items: center;
      justify-content: center;
      transition: opacity 0.15s, background-color 0.15s;
      font-size: 16px;
      line-height: 1;
    }

    .search-clear.visible {
      opacity: 0.5;
    }

    .search-clear:hover {
      opacity: 1;
      background-color: var(--border-color);
    }

    .search-results {
      padding: 6px 12px;
      font-size: 12px;
      opacity: 0.7;
      border-top: 1px solid var(--border-color);
      display: none;
    }

    .search-results.visible {
      display: block;
    }

    /* Search highlight/dim styles */
    .node.dimmed-by-search {
      opacity: 0.25;
      transition: opacity 0.2s;
    }

    .node.highlighted-by-search {
      opacity: 1;
      transition: opacity 0.2s;
    }

    .node.dimmed-by-type-filter {
      opacity: 0.25;
      transition: opacity 0.2s;
    }

    .node.dimmed-by-search.dimmed-by-type-filter {
      opacity: 0.12;
    }

    .edge.dimmed-by-edge-filter {
      opacity: 0.15;
      transition: opacity 0.2s;
    }"
    } else {
        ""
    };

    let type_filter_css = if enable_search && enable_column_type_filter {
        r"
    .type-filter-section {
      border-top: 1px solid var(--border-color);
    }

    .type-filter-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 8px 12px 4px;
      font-size: 12px;
      font-weight: 600;
      opacity: 0.9;
    }

    .type-filter-clear {
      background: none;
      border: none;
      color: var(--text-color);
      font-size: 11px;
      cursor: pointer;
      padding: 2px 6px;
      border-radius: 4px;
      opacity: 0.7;
    }

    .type-filter-clear:hover {
      opacity: 1;
      background-color: var(--border-color);
    }

    .type-filter-query {
      display: block;
      width: calc(100% - 24px);
      margin: 0 12px 8px;
      padding: 6px 8px;
      font-size: 12px;
      border: 1px solid var(--border-color);
      border-radius: 6px;
      background: var(--bg-color);
      color: var(--text-color);
    }

    .type-filter-list {
      max-height: 200px;
      overflow-y: auto;
      padding: 4px 0 8px;
    }

    .type-filter-item {
      display: flex;
      align-items: flex-start;
      gap: 8px;
      padding: 4px 12px;
      font-size: 12px;
      cursor: pointer;
    }

    .type-filter-item:hover {
      background-color: var(--border-color);
    }

    .type-filter-item span {
      word-break: break-word;
    }

    .type-filter-summary {
      display: none;
      padding: 6px 12px;
      font-size: 11px;
      opacity: 0.7;
      border-top: 1px solid var(--border-color);
    }

    .type-filter-summary.visible {
      display: block;
    }"
    } else {
        ""
    };

    let group_panel_css = if enable_group_toggles {
        r#"
    /* Group panel styles */
    .group-panel {
      position: fixed;
      top: 12px;
      right: 12px;
      width: 220px;
      max-height: calc(100vh - 24px);
      background-color: var(--bg-color);
      border: 1px solid var(--border-color);
      border-radius: 8px;
      z-index: 200;
      overflow: hidden;
      box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
    }

    body:has(h1) .group-panel {
      top: 61px;
    }

    body:has(.search-panel):not(:has(h1)) .group-panel {
      top: 61px;
    }

    body:has(.search-panel):has(h1) .group-panel {
      top: 110px;
    }

    .group-panel-header {
      padding: 10px 12px;
      font-size: 13px;
      font-weight: 600;
      border-bottom: 1px solid var(--border-color);
      display: flex;
      align-items: center;
      gap: 8px;
    }

    .group-panel-title {
      flex: 1;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .group-panel-collapse-btn {
      flex-shrink: 0;
      width: 28px;
      height: 28px;
      padding: 0;
      border: none;
      border-radius: 6px;
      background: transparent;
      color: var(--text-color);
      font-size: 14px;
      line-height: 1;
      cursor: pointer;
      opacity: 0.75;
      transition: opacity 0.15s, background-color 0.15s;
    }

    .group-panel-collapse-btn:hover {
      opacity: 1;
      background-color: var(--border-color);
    }

    .group-panel.group-panel-collapsed .group-panel-body {
      display: none;
    }

    .group-panel-actions {
      display: flex;
      gap: 8px;
    }

    .group-panel-actions button {
      background: none;
      border: none;
      color: var(--text-color);
      font-size: 11px;
      cursor: pointer;
      padding: 2px 6px;
      border-radius: 4px;
      opacity: 0.7;
      transition: opacity 0.2s, background-color 0.2s;
    }

    .group-panel-actions button:hover {
      opacity: 1;
      background-color: var(--border-color);
    }

    .group-list {
      padding: 8px 0;
      max-height: calc(100vh - 120px);
      overflow-y: auto;
    }

    .group-item {
      display: flex;
      align-items: center;
      padding: 8px 14px;
      cursor: pointer;
      transition: background-color 0.15s;
    }

    .group-item:hover {
      background-color: var(--border-color);
    }

    .group-item input[type="checkbox"] {
      margin-right: 10px;
      cursor: pointer;
      accent-color: var(--text-color);
    }

    .group-item label {
      flex: 1;
      font-size: 13px;
      cursor: pointer;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .group-item .count {
      font-size: 11px;
      opacity: 0.6;
      margin-left: 8px;
    }

    .group-item.hidden-group {
      opacity: 0.5;
    }

    /* Hidden nodes/edges */
    .node.hidden-by-group,
    .edge.hidden-by-group {
      display: none !important;
    }"#
    } else {
        ""
    };

    let highlight_css = if enable_highlight {
        r"
    /* Neighbor highlight styles */
    .node.highlighted-neighbor {
      opacity: 1 !important;
      filter: drop-shadow(0 0 6px rgba(59, 130, 246, 0.5));
      transition: opacity 0.2s, filter 0.2s;
    }

    .node.highlighted-neighbor.inbound {
      filter: drop-shadow(0 0 6px rgba(34, 197, 94, 0.5));
    }

    .node.highlighted-neighbor.outbound {
      filter: drop-shadow(0 0 6px rgba(249, 115, 22, 0.5));
    }

    .node.dimmed-by-highlight {
      opacity: 0.2 !important;
      transition: opacity 0.2s;
    }

    .edge.highlighted-neighbor {
      opacity: 1 !important;
      stroke-width: 2.5px;
      transition: opacity 0.2s, stroke-width 0.2s;
    }

    .edge.dimmed-by-highlight {
      opacity: 0.1 !important;
      transition: opacity 0.2s;
    }

    .node.selected-node {
      filter: drop-shadow(0 0 8px rgba(59, 130, 246, 0.8));
    }"
    } else {
        ""
    };

    format!(
        r"    :root {{
      --bg-color: {bg_color};
      --text-color: {text_color};
      --border-color: {border_color};
      --node-bg: {node_bg};
      --node-header-bg: {node_header_bg};
      --edge-color: {edge_color};
    }}

    * {{
      box-sizing: border-box;
      margin: 0;
      padding: 0;
    }}

    body {{
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
      background-color: var(--bg-color);
      color: var(--text-color);
      min-height: 100vh;
      overflow: hidden;
    }}

    h1 {{
      position: fixed;
      top: 0;
      left: 0;
      right: 0;
      padding: 12px 20px;
      font-size: 18px;
      font-weight: 600;
      background-color: var(--bg-color);
      border-bottom: 1px solid var(--border-color);
      z-index: 100;
      margin: 0;
    }}

    .container {{
      position: fixed;
      top: 0;
      left: 0;
      right: 0;
      bottom: 0;
    }}

    /* Add padding for heading if present */
    body:has(h1) .container {{
      top: 49px;
    }}

    .viewport {{
      width: 100%;
      height: 100%;
      overflow: hidden;
      cursor: grab;
    }}

    .viewport:active {{
      cursor: grabbing;
    }}

    .canvas {{
      transform-origin: 0 0;
    }}

    .viewport svg {{
      display: block;
    }}

    /* Controls hint */
    .viewport::after {{
      content: 'Drag to pan, scroll to zoom';
      position: absolute;
      bottom: 16px;
      right: 16px;
      font-size: 12px;
      color: var(--text-color);
      opacity: 0.5;
      pointer-events: none;
      transition: opacity 0.3s;
    }}

    .viewport:hover::after {{
      opacity: 0.8;
    }}
{search_css}{type_filter_css}{group_panel_css}{highlight_css}",
        bg_color = colors.background,
        text_color = colors.text_primary,
        border_color = colors.node_stroke,
        node_bg = colors.node_fill,
        node_header_bg = colors.header_fill,
        edge_color = colors.edge_stroke,
    )
}

/// Build the pan/zoom JavaScript.
const fn build_pan_zoom_js() -> &'static str {
    include_str!("js/pan_zoom.js")
}

/// Build the group panel HTML structure.
#[allow(clippy::needless_raw_string_hashes)]
fn build_group_panel_html() -> String {
    r#"  <div class="group-panel" id="group-panel">
    <div class="group-panel-header">
      <button type="button" id="group-panel-collapse" class="group-panel-collapse-btn" aria-expanded="true" title="Collapse or expand panel">&#9662;</button>
      <span class="group-panel-title">Groups</span>
      <div class="group-panel-actions">
        <button type="button" id="show-all-groups">Show All</button>
        <button type="button" id="hide-all-groups">Hide All</button>
      </div>
    </div>
    <div class="group-panel-body" id="group-panel-body">
      <div class="group-list" id="group-list"></div>
    </div>
  </div>
"#
    .to_string()
}

/// Build the group toggle JavaScript.
const fn build_group_toggle_js() -> &'static str {
    include_str!("js/group_toggle.js")
}

/// Build the search JavaScript.
const fn build_search_js() -> &'static str {
    include_str!("js/search.js")
}

/// Build the column type filter JavaScript.
const fn build_type_filter_js() -> &'static str {
    include_str!("js/type_filter.js")
}

/// Build the collapse JavaScript.
const fn build_collapse_js() -> &'static str {
    include_str!("js/collapse.js")
}

/// Build the highlight neighbors JavaScript.
const fn build_highlight_js() -> &'static str {
    include_str!("js/highlight.js")
}

/// Build the search panel HTML structure.
#[allow(clippy::needless_raw_string_hashes)]
fn build_search_panel_html(enable_column_type_filter: bool) -> String {
    let type_block = if enable_column_type_filter {
        r#"    <section class="type-filter-section" id="type-filter-section" hidden aria-label="Column type filter">
      <div class="type-filter-header">
        <span>Column types</span>
        <button type="button" class="type-filter-clear" id="type-filter-clear">Clear</button>
      </div>
      <input type="search" id="type-filter-query" class="type-filter-query" placeholder="Narrow type list..." autocomplete="off">
      <div class="type-filter-list" id="type-filter-list"></div>
      <div class="type-filter-summary" id="type-filter-summary"></div>
    </section>
"#
    } else {
        ""
    };

    format!(
        r#"  <div class="search-panel">
    <div class="search-container">
      <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="11" cy="11" r="8"></circle>
        <path d="m21 21l-4.35-4.35"></path>
      </svg>
      <input type="text" class="search-input" id="table-search" placeholder="Search tables (press / to focus)" autocomplete="off">
      <button type="button" class="search-clear" id="search-clear" title="Clear search">&times;</button>
    </div>
    <div class="search-results" id="search-results"></div>
{type_block}  </div>
"#,
    )
}

/// Escape HTML special characters.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Indent SVG content for proper nesting in HTML.
fn indent_svg(svg: &str) -> String {
    let mut indented = String::with_capacity(svg.len().saturating_add(svg.lines().count() * 8));
    for (index, line) in svg.lines().enumerate() {
        if index > 0 {
            indented.push('\n');
        }
        indented.push_str("        ");
        indented.push_str(line.trim_start());
    }
    indented
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_json_for_script() {
        let input = r#"{"test": "</script>"}"#;
        let escaped = escape_json_for_script(input);
        assert_eq!(escaped, r#"{"test": "<\/script>"}"#);
        assert!(!escaped.contains("</script>"));
    }

    #[test]
    fn test_escape_json_preserves_content() {
        let input = r#"{"key": "value", "number": 42}"#;
        let escaped = escape_json_for_script(input);
        assert_eq!(escaped, input);
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape(r#""quoted""#), "&quot;quoted&quot;");
        assert_eq!(html_escape("'quoted'"), "&#39;quoted&#39;");
    }

    #[test]
    fn test_build_html_document_structure() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions::default();

        let html = build_html_document(svg, metadata, &options);

        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<html lang=\"en\">"));
        assert!(html.contains("</html>"));
        assert!(html.contains("<style>"));
        assert!(html.contains("</style>"));
    }

    #[test]
    fn test_build_html_with_title() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            title: Some("Test Title".to_string()),
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        assert!(html.contains("<title>Test Title</title>"));
        assert!(html.contains("<h1>Test Title</h1>"));
    }

    #[test]
    fn test_build_html_escapes_xss_payload_in_embedded_svg_and_title() {
        let payload = "<script>alert('xss')</script>";
        let escaped = "&lt;script&gt;alert(&#39;xss&#39;)&lt;/script&gt;";
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g data-table-id="{escaped}"><title>{escaped}</title><text>{escaped}</text></g></svg>"#
        );
        let metadata = "{}";
        let options = HtmlRenderOptions {
            title: Some(payload.to_string()),
            ..Default::default()
        };

        let html = build_html_document(&svg, metadata, &options);

        assert!(html.contains(escaped));
        assert!(html.contains(&format!("<title>{escaped}</title>")));
        assert!(html.contains(&format!("<h1>{escaped}</h1>")));
        assert!(!html.contains(payload));
    }

    #[test]
    fn test_build_html_without_pan_zoom() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            enable_pan_zoom: false,
            enable_group_toggles: false,
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        // Should not contain the pan/zoom script
        assert!(!html.contains("function initPanZoom"));
        assert!(!html.contains("viewport.addEventListener"));
    }

    #[test]
    fn test_css_dark_theme() {
        let css = build_css(Theme::Dark, true, false, false, false, false);

        assert!(css.contains("--bg-color: #0f172a"));
        assert!(css.contains("--text-color: #e2e8f0"));
    }

    #[test]
    fn test_css_light_theme() {
        let css = build_css(Theme::Light, true, false, false, false, false);

        assert!(css.contains("--bg-color: #ffffff"));
        assert!(css.contains("--text-color: #1e293b"));
    }

    #[test]
    fn test_group_panel_included_when_enabled() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            enable_group_toggles: true,
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        assert!(html.contains(r#"class="group-panel""#));
        assert!(html.contains(r#"id="group-panel""#));
        assert!(html.contains(r#"id="show-all-groups""#));
        assert!(html.contains(r#"id="hide-all-groups""#));
        assert!(html.contains(r#"id="group-panel-collapse""#));
        assert!(html.contains("buildGroupList"));
    }

    #[test]
    fn test_group_panel_not_included_when_disabled() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            enable_group_toggles: false,
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        assert!(!html.contains(r#"class="group-panel""#));
        assert!(!html.contains("buildGroupList"));
        assert!(!html.contains("toggleGroup"));
    }

    #[test]
    fn test_group_panel_css_included_when_enabled() {
        let css = build_css(Theme::Light, true, false, false, false, false);

        assert!(css.contains(".group-panel"));
        assert!(css.contains(".group-item"));
        assert!(css.contains(".hidden-by-group"));
    }

    #[test]
    fn test_group_panel_css_not_included_when_disabled() {
        let css = build_css(Theme::Light, false, false, false, false, false);

        assert!(!css.contains(".group-panel"));
        assert!(!css.contains(".group-item"));
    }

    #[test]
    fn test_collapse_js_included_when_enabled() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            enable_collapse: true,
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        assert!(html.contains("collapsedTables"));
        assert!(html.contains("columnCounts"));
        assert!(html.contains("collapse-indicator"));
        assert!(html.contains("column-count-badge"));
    }

    #[test]
    fn test_collapse_js_not_included_when_disabled() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            enable_collapse: false,
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        assert!(!html.contains("collapsedTables"));
    }

    #[test]
    fn test_search_panel_included_when_enabled() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            enable_search: true,
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        assert!(html.contains(r#"class="search-panel""#));
        assert!(html.contains(r#"id="table-search""#));
        assert!(html.contains("type-filter-section"));
    }

    #[test]
    fn test_search_panel_not_included_when_disabled() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            enable_search: false,
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        assert!(!html.contains(r#"class="search-panel""#));
        assert!(!html.contains(r#"id="table-search""#));
    }

    #[test]
    fn test_search_css_included_when_enabled() {
        let css = build_css(Theme::Light, false, true, true, false, false);

        assert!(css.contains(".search-panel"));
        assert!(css.contains(".search-input"));
        assert!(css.contains(".search-clear"));
        assert!(css.contains(".search-results"));
        assert!(css.contains(".dimmed-by-search"));
        assert!(css.contains(".highlighted-by-search"));
        assert!(css.contains(".dimmed-by-edge-filter"));
        assert!(css.contains(".type-filter-section"));
    }

    #[test]
    fn test_search_css_without_column_type_filter() {
        let css = build_css(Theme::Light, false, true, false, false, false);

        assert!(css.contains(".search-panel"));
        assert!(!css.contains(".type-filter-section"));
    }

    #[test]
    fn test_search_css_not_included_when_disabled() {
        let css = build_css(Theme::Light, false, false, false, false, false);

        assert!(!css.contains(".search-panel"));
        assert!(!css.contains(".search-input"));
        assert!(!css.contains(".dimmed-by-search"));
    }

    #[test]
    fn test_search_panel_html_structure() {
        let html = build_search_panel_html(true);

        assert!(html.contains(r#"class="search-panel""#));
        assert!(html.contains(r#"class="search-icon""#));
        assert!(html.contains(r#"class="search-input""#));
        assert!(html.contains(r#"id="table-search""#));
        assert!(html.contains(r#"class="search-clear""#));
        assert!(html.contains(r#"id="search-clear""#));
        assert!(html.contains(r#"class="search-results""#));
        assert!(html.contains(r#"id="search-results""#));
        assert!(html.contains("type-filter-section"));
    }

    #[test]
    fn test_search_panel_html_without_column_type_filter() {
        let html = build_search_panel_html(false);

        assert!(html.contains(r#"class="search-panel""#));
        assert!(!html.contains("type-filter-section"));
    }

    #[test]
    fn test_search_js_included_when_enabled() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            enable_search: true,
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        assert!(html.contains("performSearch"));
        assert!(html.contains("debouncedSearch"));
        assert!(html.contains("table-search"));
        assert!(html.contains("search-clear"));
        assert!(html.contains("search-results"));
        assert!(html.contains("tableMatchesAnySelectedType"));
    }

    #[test]
    fn test_column_type_filter_js_omitted_when_disabled() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            enable_search: true,
            enable_column_type_filter: false,
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        assert!(html.contains("performSearch"));
        assert!(!html.contains("tableMatchesAnySelectedType"));
    }

    #[test]
    fn test_search_js_not_included_when_disabled() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            enable_search: false,
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        assert!(!html.contains("performSearch"));
        assert!(!html.contains("debouncedSearch"));
    }

    #[test]
    fn test_highlight_css_included_when_enabled() {
        let css = build_css(Theme::Light, false, false, false, false, true);

        assert!(css.contains(".highlighted-neighbor"));
        assert!(css.contains(".dimmed-by-highlight"));
        assert!(css.contains(".selected-node"));
    }

    #[test]
    fn test_highlight_css_not_included_when_disabled() {
        let css = build_css(Theme::Light, false, false, false, false, false);

        assert!(!css.contains(".highlighted-neighbor"));
        assert!(!css.contains(".dimmed-by-highlight"));
        assert!(!css.contains(".selected-node"));
    }

    #[test]
    fn test_highlight_js_included_when_enabled() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            enable_highlight: true,
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        assert!(html.contains("highlightNeighbors"));
        assert!(html.contains("clearHighlights"));
        assert!(html.contains("inboundMap"));
        assert!(html.contains("outboundMap"));
    }

    #[test]
    fn test_highlight_js_not_included_when_disabled() {
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            enable_highlight: false,
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        assert!(!html.contains("highlightNeighbors"));
        assert!(!html.contains("clearHighlights"));
        assert!(!html.contains("inboundMap"));
        assert!(!html.contains("outboundMap"));
    }
}
