//! HTML document generation.
//!
//! Embedded viewer scripts are authored in `ts/` and committed as bundled assets in `src/js/`.

use crate::options::{HtmlRenderOptions, Theme};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use relune_render_theme::{escape_xml_text, get_colors};
use std::sync::LazyLock;

static FAVICON_DATA_URI: LazyLock<String> = LazyLock::new(|| {
    let encoded = STANDARD.encode(include_bytes!("../assets/favicon.png"));
    format!("data:image/png;base64,{encoded}")
});

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
        js_parts.push(build_minimap_js());
        js_parts.push(build_shortcuts_js());
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
    js_parts.push(build_load_motion_js());
    js_parts.push(build_url_state_js());
    let js = if js_parts.is_empty() {
        None
    } else {
        Some(js_parts.join("\n\n"))
    };

    let heading = if options.title.is_some() {
        Some(format!(
            "<h1>{}</h1>",
            escape_xml_text(options.title.as_deref().unwrap())
        ))
    } else {
        None
    };

    let group_panel = if options.enable_group_toggles && !options.enable_search {
        Some(build_group_panel_html())
    } else {
        None
    };

    let search_panel = if options.enable_search {
        Some(build_search_panel_html(
            options.enable_column_type_filter,
            options.enable_group_toggles,
        ))
    } else {
        None
    };

    let filter_reset_bar = if options.enable_search && options.enable_column_type_filter {
        Some(build_filter_reset_bar_html())
    } else {
        None
    };

    let viewer_controls = if options.enable_pan_zoom {
        Some(build_viewer_controls_html())
    } else {
        None
    };

    let detail_drawer = if options.enable_highlight {
        Some(build_detail_drawer_html())
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
  <link rel="icon" href="{favicon_data_uri}">
  <style>
{css}
  </style>
</head>
<body>
{heading}
{search_panel}
{filter_reset_bar}
{group_panel}
{detail_drawer}
{viewer_controls}
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
        title = escape_xml_text(title),
        favicon_data_uri = FAVICON_DATA_URI.as_str(),
        heading = heading.unwrap_or_default(),
        search_panel = search_panel.unwrap_or_default(),
        filter_reset_bar = filter_reset_bar.unwrap_or_default(),
        group_panel = group_panel.unwrap_or_default(),
        detail_drawer = detail_drawer.unwrap_or_default(),
        viewer_controls = viewer_controls.unwrap_or_default(),
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
    let accent_color = colors.glow_color;
    let (viewer_bg, panel_bg, panel_border, panel_shadow, accent_soft, grid_dot, grid_line) =
        match theme {
            Theme::Dark => (
                "radial-gradient(circle at top, rgba(245, 158, 11, 0.16), transparent 34%), linear-gradient(180deg, #0b1020 0%, #111827 52%, #0a0f1c 100%)",
                "rgba(10, 15, 28, 0.9)",
                "rgba(148, 163, 184, 0.18)",
                "0 18px 48px rgba(2, 6, 23, 0.52)",
                "rgba(245, 158, 11, 0.16)",
                "rgba(148, 163, 184, 0.12)",
                "rgba(148, 163, 184, 0.05)",
            ),
            Theme::Light => (
                "radial-gradient(circle at top, rgba(217, 119, 6, 0.12), transparent 32%), linear-gradient(180deg, #f8fafc 0%, #eef2ff 42%, #f8fafc 100%)",
                "rgba(255, 255, 255, 0.86)",
                "rgba(71, 85, 105, 0.16)",
                "0 16px 36px rgba(15, 23, 42, 0.12)",
                "rgba(194, 65, 12, 0.12)",
                "rgba(71, 85, 105, 0.12)",
                "rgba(71, 85, 105, 0.04)",
            ),
        };

    let search_css = if enable_search {
        r"
    /* Explorer sidebar styles */
    .search-panel {
      position: fixed;
      top: 12px;
      left: 12px;
      bottom: 12px;
      width: min(340px, calc(100vw - 24px));
      display: flex;
      flex-direction: column;
      min-height: 0;
      background: var(--panel-bg);
      border: 1px solid var(--panel-border);
      border-radius: 22px;
      z-index: 240;
      overflow: hidden;
      box-shadow: var(--panel-shadow);
      backdrop-filter: blur(16px);
    }

    body:has(h1) .search-panel {
      top: 61px;
    }

    .search-panel-header {
      display: flex;
      align-items: baseline;
      justify-content: space-between;
      gap: 10px;
      padding: 16px 16px 10px;
      border-bottom: 1px solid var(--panel-border);
    }

    .search-panel-title {
      font-size: 15px;
      font-weight: 700;
      letter-spacing: 0.01em;
    }

    .search-panel-meta,
    .object-browser-count {
      font-size: 11px;
      opacity: 0.65;
      white-space: nowrap;
    }

    .search-container {
      display: flex;
      align-items: center;
      padding: 12px 14px;
      gap: 10px;
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
      font-family: var(--ui-font);
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
      background-color: var(--accent-soft);
    }

    .search-results {
      padding: 0 16px 12px;
      font-size: 12px;
      opacity: 0.7;
      display: none;
    }

    .search-results.visible {
      display: block;
    }

    .object-browser-section {
      display: flex;
      flex: 1;
      flex-direction: column;
      min-height: 180px;
      border-top: 1px solid var(--panel-border);
    }

    .object-browser-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
      padding: 12px 16px 8px;
      font-size: 12px;
      font-weight: 600;
      opacity: 0.9;
    }

    .object-browser-list {
      flex: 1;
      min-height: 0;
      overflow-y: auto;
      padding: 4px 0 12px;
    }

    .object-browser-empty {
      padding: 0 16px 16px;
      font-size: 12px;
      opacity: 0.62;
    }

    .object-browser-empty[hidden] {
      display: none;
    }

    .object-browser-item {
      width: 100%;
      border: none;
      border-left: 2px solid transparent;
      background: transparent;
      color: inherit;
      text-align: left;
      padding: 12px 16px 11px;
      cursor: pointer;
      transition: background-color 0.16s, border-color 0.16s, opacity 0.16s;
    }

    .object-browser-item:hover {
      background: var(--accent-soft);
    }

    .object-browser-item.selected {
      background: color-mix(in srgb, var(--accent-soft) 76%, transparent);
      border-left-color: var(--accent-color);
    }

    .object-browser-item.filtered-out {
      opacity: 0.46;
    }

    .object-browser-item.hidden-item {
      opacity: 0.24;
    }

    .object-browser-item-header,
    .object-browser-item-meta {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
    }

    .object-browser-item-header {
      margin-bottom: 6px;
    }

    .object-browser-item-name {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: 13px;
      font-weight: 600;
    }

    .object-browser-kind {
      flex-shrink: 0;
      padding: 2px 8px;
      border-radius: 999px;
      background: rgba(148, 163, 184, 0.14);
      font-size: 10px;
      font-weight: 700;
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }

    .object-browser-item-meta {
      font-size: 11px;
      opacity: 0.7;
      font-family: var(--mono-font);
    }

    .node.dimmed-by-search {
      opacity: 0.25;
      transition: opacity 0.2s;
    }

    .node.highlighted-by-search {
      opacity: 1;
      transition: opacity 0.2s;
    }

    .node.dimmed-by-type-filter {
      opacity: 0.2;
      transition: opacity 0.2s;
    }

    .node.dimmed-by-search.dimmed-by-type-filter {
      opacity: 0.08;
    }

    .edge.dimmed-by-edge-filter {
      opacity: 0.12;
      transition: opacity 0.2s;
    }"
    } else {
        ""
    };

    let type_filter_css = if enable_search && enable_column_type_filter {
        r"
    .type-filter-section {
      border-top: 1px solid var(--panel-border);
    }

    .type-filter-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 10px 14px 6px;
      font-size: 12px;
      font-weight: 600;
      opacity: 0.9;
    }

    .type-filter-actions {
      display: flex;
      gap: 6px;
    }

    .type-filter-action {
      background: transparent;
      border: 1px solid var(--panel-border);
      color: var(--text-color);
      font-size: 11px;
      cursor: pointer;
      padding: 4px 8px;
      border-radius: 999px;
      opacity: 0.84;
      transition: background-color 0.16s, border-color 0.16s, opacity 0.16s;
    }

    .type-filter-action:hover {
      opacity: 1;
      border-color: var(--accent-color);
      background-color: var(--accent-soft);
    }

    .type-filter-query {
      display: block;
      width: calc(100% - 28px);
      margin: 0 14px 10px;
      padding: 8px 10px;
      font-size: 12px;
      border: 1px solid var(--panel-border);
      border-radius: 10px;
      background: rgba(15, 23, 42, 0.02);
      color: var(--text-color);
    }

    .type-filter-list {
      max-height: min(220px, 28vh);
      overflow-y: auto;
      padding: 4px 0 10px;
    }

    .type-filter-item {
      display: flex;
      align-items: center;
      gap: 10px;
      padding: 7px 14px;
      font-size: 12px;
      cursor: pointer;
      transition: background-color 0.16s;
    }

    .type-filter-item:hover {
      background-color: var(--accent-soft);
    }

    .type-filter-item span {
      word-break: break-word;
      font-family: var(--mono-font);
    }

    .type-filter-item-count {
      margin-left: auto;
      min-width: 28px;
      padding: 2px 8px;
      border-radius: 999px;
      background: var(--accent-soft);
      text-align: center;
      font-size: 11px;
      font-weight: 700;
      font-family: var(--ui-font);
    }

    .type-filter-summary {
      display: none;
      padding: 8px 14px 12px;
      font-size: 11px;
      opacity: 0.7;
      border-top: 1px solid var(--panel-border);
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
      display: flex;
      flex-direction: column;
      min-height: 0;
    }

    body > .group-panel {
      position: fixed;
      top: 12px;
      left: 12px;
      width: min(340px, calc(100vw - 24px));
      max-height: calc(100vh - 24px);
      background: var(--panel-bg);
      border: 1px solid var(--panel-border);
      border-radius: 22px;
      z-index: 220;
      overflow: hidden;
      box-shadow: var(--panel-shadow);
      backdrop-filter: blur(16px);
    }

    body:has(h1) > .group-panel {
      top: 61px;
    }

    .search-panel .group-panel {
      border-top: 1px solid var(--panel-border);
      background: transparent;
    }

    .group-panel-header {
      padding: 12px 14px;
      font-size: 13px;
      font-weight: 600;
      border-bottom: 1px solid var(--panel-border);
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
      background-color: var(--accent-soft);
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
      border: 1px solid transparent;
      color: var(--text-color);
      font-size: 11px;
      cursor: pointer;
      padding: 4px 8px;
      border-radius: 999px;
      opacity: 0.7;
      transition: opacity 0.2s, background-color 0.2s, border-color 0.2s;
    }

    .group-panel-actions button:hover {
      opacity: 1;
      border-color: var(--accent-color);
      background-color: var(--accent-soft);
    }

    .group-list {
      padding: 8px 0 12px;
      max-height: min(220px, 28vh);
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
      background-color: var(--accent-soft);
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
      filter: drop-shadow(0 0 10px rgba(245, 158, 11, 0.35));
      transition: opacity 0.2s, filter 0.2s;
    }

    .node .table-body {
      transition: stroke 0.3s, stroke-width 0.3s, filter 0.3s, opacity 0.3s;
    }

    .node.highlighted-neighbor .table-body {
      stroke: rgba(245, 158, 11, 0.78);
      stroke-width: 2.2px;
    }

    .node.highlighted-neighbor.inbound {
      filter: drop-shadow(0 0 10px rgba(45, 212, 191, 0.35));
    }

    .node.highlighted-neighbor.inbound .table-body {
      stroke: rgba(45, 212, 191, 0.78);
    }

    .node.highlighted-neighbor.outbound {
      filter: drop-shadow(0 0 10px rgba(251, 191, 36, 0.4));
    }

    .node.highlighted-neighbor.outbound .table-body {
      stroke: rgba(251, 191, 36, 0.84);
    }

    .node.dimmed-by-highlight {
      opacity: 0.12 !important;
      transition: opacity 0.2s;
    }

    .edge.highlighted-neighbor {
      opacity: 1 !important;
      stroke-width: 2.6px;
      transition: opacity 0.2s, stroke-width 0.2s;
    }

    .edge.dimmed-by-highlight {
      opacity: 0.08 !important;
      transition: opacity 0.2s;
    }

    .node.selected-node {
      filter: drop-shadow(0 0 14px rgba(245, 158, 11, 0.48));
    }"
    } else {
        ""
    };

    let viewer_shell_css = r"
    .filter-reset-bar {
      position: fixed;
      top: 12px;
      left: 50%;
      transform: translateX(-50%);
      display: flex;
      align-items: center;
      gap: 12px;
      min-width: min(520px, calc(100vw - 24px));
      max-width: calc(100vw - 24px);
      padding: 10px 14px;
      border: 1px solid var(--panel-border);
      border-radius: 999px;
      background: var(--panel-bg);
      box-shadow: var(--panel-shadow);
      backdrop-filter: blur(16px);
      z-index: 260;
    }

    .filter-reset-bar[hidden] {
      display: none;
    }

    body:has(h1) .filter-reset-bar {
      top: 61px;
    }

    .filter-reset-copy {
      flex: 1;
      min-width: 0;
      font-size: 12px;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .filter-reset-button {
      border: 1px solid var(--accent-color);
      background: var(--accent-soft);
      color: var(--text-color);
      border-radius: 999px;
      padding: 6px 12px;
      cursor: pointer;
      font: inherit;
      transition: filter 0.16s, transform 0.16s;
    }

    .filter-reset-button:hover {
      filter: brightness(1.05);
      transform: translateY(-1px);
    }

    .viewer-controls {
      position: fixed;
      left: 50%;
      bottom: 16px;
      transform: translateX(-50%);
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 6px;
      border: 1px solid var(--panel-border);
      border-radius: 999px;
      background: color-mix(in srgb, var(--panel-bg) 92%, transparent);
      box-shadow: var(--panel-shadow);
      backdrop-filter: blur(16px);
      z-index: 230;
    }

    .viewer-control-button {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-width: 36px;
      height: 34px;
      border: 1px solid var(--panel-border);
      background: transparent;
      color: var(--text-color);
      border-radius: 999px;
      font: 600 12px var(--ui-font);
      cursor: pointer;
      transition: transform 0.16s, border-color 0.16s, background-color 0.16s;
    }

    .viewer-control-button svg {
      width: 16px;
      height: 16px;
      pointer-events: none;
    }

    .viewer-control-fit {
      min-width: 36px;
    }

    .viewer-control-status {
      min-width: 52px;
      text-align: center;
      font: 600 11px var(--mono-font);
      opacity: 0.7;
    }

    .viewer-control-button:hover {
      transform: translateY(-1px);
      border-color: var(--accent-color);
      background: color-mix(in srgb, var(--panel-bg) 82%, var(--accent-soft));
    }

    .minimap-shell {
      position: fixed;
      right: 12px;
      bottom: 88px;
      width: min(240px, calc(100vw - 24px));
      border: 1px solid var(--panel-border);
      border-radius: 22px;
      background: var(--panel-bg);
      box-shadow: var(--panel-shadow);
      backdrop-filter: blur(16px);
      overflow: hidden;
      z-index: 210;
    }

    .minimap-header {
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 12px 14px;
      border-bottom: 1px solid var(--panel-border);
      font-size: 11px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      opacity: 0.8;
    }

    .minimap-hint {
      opacity: 0.65;
      text-transform: none;
      letter-spacing: normal;
    }

    .minimap {
      display: block;
      width: 100%;
      height: 150px;
      cursor: pointer;
      background: rgba(148, 163, 184, 0.04);
    }

    .minimap-node {
      fill: rgba(148, 163, 184, 0.58);
      stroke: rgba(148, 163, 184, 0.82);
      stroke-width: 0.6;
      rx: 2;
      transition: fill 0.15s, stroke 0.15s;
    }

    .minimap-node.selected {
      fill: var(--accent-color);
      stroke: var(--accent-color);
      filter: drop-shadow(0 0 3px var(--accent-soft));
    }

    .minimap-frame {
      fill: rgba(245, 158, 11, 0.1);
      stroke: var(--accent-color);
      stroke-width: 1.8;
      stroke-dasharray: 4 2;
      rx: 2;
    }

    .detail-drawer {
      position: fixed;
      top: 12px;
      right: 12px;
      width: min(340px, calc(100vw - 24px));
      bottom: 12px;
      overflow: auto;
      padding: 16px;
      border: 1px solid var(--panel-border);
      border-radius: 22px;
      background: var(--panel-bg);
      box-shadow: var(--panel-shadow);
      backdrop-filter: blur(16px);
      z-index: 250;
    }

    .detail-drawer[hidden] {
      display: none;
    }

    body:has(h1) .detail-drawer {
      top: 61px;
    }

    .detail-drawer-header {
      display: flex;
      align-items: flex-start;
      justify-content: space-between;
      gap: 12px;
    }

    .detail-kicker {
      font-size: 11px;
      letter-spacing: 0.08em;
      text-transform: uppercase;
      color: var(--accent-color);
      margin-bottom: 6px;
    }

    .detail-title {
      font-size: 20px;
      line-height: 1.15;
      margin: 0;
    }

    .detail-subtitle {
      margin-top: 10px;
      font-size: 13px;
      opacity: 0.72;
    }

    .detail-close {
      width: 36px;
      height: 36px;
      border: 1px solid var(--panel-border);
      background: transparent;
      color: var(--text-color);
      border-radius: 50%;
      font-size: 20px;
      line-height: 1;
      cursor: pointer;
    }

    .detail-metrics {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 10px;
      margin: 16px 0;
    }

    .detail-metric {
      padding: 10px 12px;
      border-radius: 12px;
      background: rgba(148, 163, 184, 0.08);
    }

    .detail-metric-label {
      display: block;
      font-size: 11px;
      opacity: 0.65;
      margin-bottom: 4px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
    }

    .detail-metric-value {
      font-family: var(--mono-font);
      font-size: 14px;
    }

    .detail-section + .detail-section {
      margin-top: 16px;
    }

    .detail-section h3 {
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      opacity: 0.74;
      margin-bottom: 10px;
    }

    .detail-columns,
    .detail-relations {
      display: flex;
      flex-wrap: wrap;
      gap: 6px;
    }

    .detail-columns .detail-column {
      flex: 1 1 100%;
    }

    .detail-column,
    .detail-relation {
      border: 1px solid rgba(148, 163, 184, 0.12);
      border-radius: 12px;
      padding: 8px 12px;
      background: rgba(148, 163, 184, 0.05);
      transition: border-color 0.15s, background-color 0.15s;
    }

    .detail-relation:hover {
      border-color: var(--accent-color);
      background: color-mix(in srgb, rgba(148, 163, 184, 0.05) 72%, var(--accent-soft));
    }

    .detail-column-name,
    .detail-relation-label {
      display: block;
      font-family: var(--mono-font);
      font-size: 13px;
      margin-bottom: 2px;
    }

    .detail-column-pills {
      display: flex;
      flex-wrap: wrap;
      gap: 4px;
    }

    .detail-column-pill {
      display: inline-block;
      padding: 1px 7px;
      border-radius: 999px;
      font-size: 10px;
      font-weight: 600;
      letter-spacing: 0.02em;
      background: rgba(148, 163, 184, 0.12);
      opacity: 0.78;
    }

    .detail-column-pill-pk {
      background: rgba(245, 158, 11, 0.2);
      color: var(--accent-color);
      opacity: 1;
    }

    .detail-column-pill-required {
      opacity: 0.56;
    }

    .detail-column-pill-nullable {
      opacity: 0.56;
    }

    .detail-column-pill-diff {
      font-weight: 700;
      letter-spacing: 0.04em;
      opacity: 1;
    }

    .detail-column-pill-diff-added {
      background: rgba(34, 197, 94, 0.2);
      color: #22c55e;
    }

    .detail-column-pill-diff-removed {
      background: rgba(239, 68, 68, 0.2);
      color: #ef4444;
    }

    .detail-column-pill-diff-modified {
      background: rgba(245, 158, 11, 0.2);
      color: #f59e0b;
    }

    .detail-diff-badge {
      display: inline-block;
      padding: 2px 10px;
      border-radius: 999px;
      font-size: 11px;
      font-weight: 700;
      letter-spacing: 0.04em;
      text-transform: uppercase;
    }

    .detail-diff-badge-added {
      background: rgba(34, 197, 94, 0.18);
      color: #22c55e;
    }

    .detail-diff-badge-removed {
      background: rgba(239, 68, 68, 0.18);
      color: #ef4444;
    }

    .detail-diff-badge-modified {
      background: rgba(245, 158, 11, 0.18);
      color: #f59e0b;
    }

    .detail-column-meta,
    .detail-relation-meta {
      font-size: 11px;
      opacity: 0.65;
      letter-spacing: 0.01em;
    }

    .detail-relation-meta {
      border: none;
      background: transparent;
      color: inherit;
      padding: 0;
      text-align: left;
      cursor: pointer;
      font: inherit;
    }

    .detail-empty {
      font-size: 12px;
      opacity: 0.62;
    }

    .detail-issue {
      border: 1px solid rgba(148, 163, 184, 0.12);
      border-radius: 12px;
      padding: 10px 12px;
      margin-bottom: 6px;
    }

    .detail-issue-error { border-color: rgba(248, 113, 113, 0.4); }
    .detail-issue-warning { border-color: rgba(251, 191, 36, 0.4); }
    .detail-issue-info { border-color: rgba(56, 189, 248, 0.4); }

    .detail-issue-header {
      display: flex;
      align-items: center;
      gap: 8px;
    }

    .detail-issue-badge {
      display: inline-block;
      padding: 1px 7px;
      border-radius: 8px;
      font-size: 10px;
      font-weight: 700;
      text-transform: uppercase;
      letter-spacing: 0.04em;
      white-space: nowrap;
    }

    .detail-issue-badge-error { background: rgba(248, 113, 113, 0.22); color: #f87171; }
    .detail-issue-badge-warning { background: rgba(251, 191, 36, 0.22); color: #fbbf24; }
    .detail-issue-badge-info { background: rgba(56, 189, 248, 0.22); color: #38bdf8; }
    .detail-issue-badge-hint { background: rgba(148, 163, 184, 0.18); color: #94a3b8; }

    .detail-issue-message {
      font-size: 13px;
    }

    .detail-issue-hint {
      display: block;
      font-size: 12px;
      opacity: 0.72;
      margin-top: 4px;
      padding-left: 4px;
    }

    .object-browser-issue-badge {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-width: 18px;
      height: 18px;
      padding: 0 5px;
      border-radius: 9px;
      font-size: 10px;
      font-weight: 700;
      flex-shrink: 0;
    }

    .object-browser-issue-badge-error { background: rgba(248, 113, 113, 0.22); color: #f87171; }
    .object-browser-issue-badge-warning { background: rgba(251, 191, 36, 0.22); color: #fbbf24; }
    .object-browser-issue-badge-info { background: rgba(56, 189, 248, 0.22); color: #38bdf8; }
    .object-browser-issue-badge-hint { background: rgba(148, 163, 184, 0.18); color: #94a3b8; }

    .canvas svg .node,
    .canvas svg .edge {
      opacity: 0;
      animation-duration: 440ms;
      animation-timing-function: cubic-bezier(0.2, 0.9, 0.2, 1);
      animation-fill-mode: forwards;
      animation-delay: var(--enter-delay, calc(var(--enter-index, 0) * 20ms));
    }

    .canvas svg .node {
      animation-name: relune-node-enter;
      transform-box: fill-box;
      transform-origin: center;
    }

    .canvas svg .edge {
      animation-name: relune-edge-enter;
    }

    .node.dimmed-by-type-filter .type-filter-overlay {
      opacity: 0.34;
    }

    .edge.highlighted-neighbor .edge-glow-path {
      opacity: 0.92;
    }

    .edge.highlighted-neighbor .edge-particles {
      opacity: 0.92;
    }

    @keyframes relune-node-enter {
      from {
        opacity: 0;
        transform: translateY(10px) scale(0.985);
      }
      to {
        opacity: 1;
        transform: translateY(0) scale(1);
      }
    }

    @keyframes relune-edge-enter {
      from {
        opacity: 0;
      }
      to {
        opacity: 1;
      }
    }

    @media (max-width: 960px) {
      .detail-drawer,
      .search-panel,
      .minimap-shell {
        width: calc(100vw - 24px);
      }

      body > .group-panel {
        width: calc(100vw - 24px);
      }

      .detail-drawer {
        top: auto;
        bottom: 16px;
        max-height: 42vh;
      }

      .viewer-controls {
        bottom: 12px;
      }

      .search-panel {
        top: 12px;
        bottom: auto;
        max-height: min(58vh, 720px);
      }

      body:has(h1) .search-panel {
        top: 61px;
      }

      body > .group-panel {
        top: auto;
        bottom: 16px;
        max-height: 38vh;
      }

      .minimap-shell {
        right: 12px;
        bottom: 74px;
      }
    }";

    format!(
        r"    :root {{
      color-scheme: {color_scheme};
      --bg-color: {bg_color};
      --text-color: {text_color};
      --border-color: {border_color};
      --node-bg: {node_bg};
      --node-header-bg: {node_header_bg};
      --edge-color: {edge_color};
      --panel-bg: {panel_bg};
      --panel-border: {panel_border};
      --panel-shadow: {panel_shadow};
      --accent-color: {accent_color};
      --accent-soft: {accent_soft};
      --viewer-bg: {viewer_bg};
      --grid-dot: {grid_dot};
      --grid-line: {grid_line};
      --ui-font: 'Inter', 'Segoe UI', system-ui, sans-serif;
      --mono-font: 'JetBrains Mono', 'Fira Code', 'SFMono-Regular', ui-monospace, monospace;
    }}

    @font-face {{
      font-family: 'Inter';
      src: local('Inter'), local('Inter Regular');
      font-display: swap;
    }}

    @font-face {{
      font-family: 'JetBrains Mono';
      src: local('JetBrains Mono'), local('JetBrainsMono Nerd Font Mono'), local('JetBrains Mono Regular');
      font-display: swap;
    }}

    * {{
      box-sizing: border-box;
      margin: 0;
      padding: 0;
    }}

    body {{
      font-family: var(--ui-font);
      background: var(--viewer-bg);
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
      background: color-mix(in srgb, var(--panel-bg) 92%, transparent);
      border-bottom: 1px solid var(--panel-border);
      backdrop-filter: blur(16px);
      z-index: 180;
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
      position: relative;
      background-image:
        radial-gradient(circle at 1px 1px, var(--grid-dot) 1.2px, transparent 0),
        linear-gradient(var(--grid-line) 1px, transparent 1px),
        linear-gradient(90deg, var(--grid-line) 1px, transparent 1px);
      background-size: 24px 24px, 96px 96px, 96px 96px;
      background-position: 0 0, -1px -1px, -1px -1px;
    }}

    .viewport:active {{
      cursor: grabbing;
    }}

    .canvas {{
      position: absolute;
      top: 0;
      left: 0;
      transform-origin: 0 0;
      will-change: transform;
    }}

    .viewport svg {{
      display: block;
      overflow: visible;
    }}

    /* Controls hint */
    .viewport::after {{
      content: 'Drag to pan, scroll to zoom, F to fit';
      position: absolute;
      bottom: 16px;
      left: 16px;
      font-size: 12px;
      color: var(--text-color);
      opacity: 0.5;
      pointer-events: none;
      transition: opacity 0.3s;
      z-index: 20;
    }}

    .viewport:hover::after {{
      opacity: 0.8;
    }}
{search_css}{type_filter_css}{group_panel_css}{highlight_css}{viewer_shell_css}",
        bg_color = colors.background,
        color_scheme = if matches!(theme, Theme::Dark) {
            "dark"
        } else {
            "light"
        },
        text_color = colors.text_primary,
        border_color = colors.node_stroke,
        node_bg = colors.node_fill,
        node_header_bg = colors.header_fill,
        edge_color = colors.edge_stroke,
        panel_bg = panel_bg,
        panel_border = panel_border,
        panel_shadow = panel_shadow,
        accent_color = accent_color,
        accent_soft = accent_soft,
        viewer_bg = viewer_bg,
        grid_dot = grid_dot,
        grid_line = grid_line,
        viewer_shell_css = viewer_shell_css,
    )
}

/// Build the pan/zoom JavaScript.
const fn build_pan_zoom_js() -> &'static str {
    include_str!("js/pan_zoom.js")
}

/// Build the group panel HTML structure.
#[allow(clippy::needless_raw_string_hashes)]
fn build_group_panel_html() -> String {
    r#"  <section class="group-panel" id="group-panel">
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
  </section>
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

/// Build the minimap JavaScript.
const fn build_minimap_js() -> &'static str {
    include_str!("js/minimap.js")
}

/// Build the keyboard shortcuts JavaScript.
const fn build_shortcuts_js() -> &'static str {
    include_str!("js/shortcuts.js")
}

/// Build the load animation JavaScript.
const fn build_load_motion_js() -> &'static str {
    include_str!("js/load_motion.js")
}

/// Build the URL state synchronisation JavaScript.
const fn build_url_state_js() -> &'static str {
    include_str!("js/url_state.js")
}

#[allow(clippy::needless_raw_string_hashes)]
fn build_filter_reset_bar_html() -> String {
    r#"  <div class="filter-reset-bar" id="filter-reset-bar" hidden>
    <span class="filter-reset-copy" id="filter-reset-copy"></span>
    <button type="button" class="filter-reset-button" id="filter-reset-button">Reset filters</button>
  </div>
"#
    .to_string()
}

#[allow(clippy::needless_raw_string_hashes)]
fn build_viewer_controls_html() -> String {
    r#"  <div class="viewer-controls" id="viewer-controls" aria-label="Diagram controls">
    <button type="button" class="viewer-control-button" id="zoom-in" title="Zoom in"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg></button>
    <button type="button" class="viewer-control-button" id="zoom-out" title="Zoom out"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M5 12h14"/></svg></button>
    <span class="viewer-control-status" id="zoom-level">100%</span>
    <button type="button" class="viewer-control-button viewer-control-fit" id="zoom-fit" title="Fit to screen"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7"/></svg></button>
  </div>
  <div class="minimap-shell" id="minimap-shell" aria-label="Diagram minimap">
    <div class="minimap-header">
      <span>Minimap</span>
      <span class="minimap-hint">Viewport</span>
    </div>
    <svg class="minimap" id="minimap" viewBox="0 0 100 100" aria-hidden="true"></svg>
  </div>
"#
    .to_string()
}

#[allow(clippy::needless_raw_string_hashes)]
fn build_detail_drawer_html() -> String {
    r#"  <aside class="detail-drawer" id="detail-drawer" hidden>
    <div class="detail-drawer-header">
      <div>
        <p class="detail-kicker" id="detail-kind">Inspector</p>
        <h2 class="detail-title" id="detail-title">Object details</h2>
      </div>
      <button type="button" class="detail-close" id="detail-close" aria-label="Close details">&times;</button>
    </div>
    <p class="detail-subtitle" id="detail-subtitle"></p>
    <div class="detail-metrics" id="detail-metrics"></div>
    <section class="detail-section">
      <h3>Columns</h3>
      <div class="detail-empty" id="detail-columns-empty">No column details available.</div>
      <div class="detail-columns" id="detail-columns"></div>
    </section>
    <section class="detail-section">
      <h3>Relationships</h3>
      <div class="detail-empty" id="detail-relationships-empty">No relationships for this object.</div>
      <div class="detail-relations" id="detail-relations"></div>
    </section>
    <section class="detail-section">
      <h3>Health</h3>
      <div class="detail-empty" id="detail-issues-empty">No issues detected.</div>
      <div class="detail-issues" id="detail-issues"></div>
    </section>
  </aside>
"#
    .to_string()
}

/// Build the search panel HTML structure.
#[allow(clippy::needless_raw_string_hashes)]
fn build_search_panel_html(enable_column_type_filter: bool, enable_group_toggles: bool) -> String {
    let type_block = if enable_column_type_filter {
        r#"    <section class="type-filter-section" id="type-filter-section" hidden aria-label="Column type filter">
      <div class="type-filter-header">
        <span>Filter by type</span>
        <div class="type-filter-actions">
          <button type="button" class="type-filter-action" id="type-filter-select-visible">All</button>
          <button type="button" class="type-filter-action" id="type-filter-clear">None</button>
        </div>
      </div>
      <input type="search" id="type-filter-query" class="type-filter-query" placeholder="Narrow type list..." autocomplete="off">
      <div class="type-filter-list" id="type-filter-list"></div>
      <div class="type-filter-summary" id="type-filter-summary"></div>
    </section>
"#
    } else {
        ""
    };

    let group_block = if enable_group_toggles {
        r#"    <section class="group-panel" id="group-panel">
      <div class="group-panel-header">
        <button type="button" id="group-panel-collapse" class="group-panel-collapse-btn" aria-expanded="true" title="Collapse or expand groups">&#9662;</button>
        <span class="group-panel-title">Groups</span>
        <div class="group-panel-actions">
          <button type="button" id="show-all-groups">Show All</button>
          <button type="button" id="hide-all-groups">Hide All</button>
        </div>
      </div>
      <div class="group-panel-body" id="group-panel-body">
        <div class="group-list" id="group-list"></div>
      </div>
    </section>
"#
    } else {
        ""
    };

    format!(
        r#"  <aside class="search-panel" id="search-panel">
    <div class="search-panel-header">
      <span class="search-panel-title">Explore</span>
      <span class="search-panel-meta">Press / to focus</span>
    </div>
    <div class="search-container">
      <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="11" cy="11" r="8"></circle>
        <path d="m21 21l-4.35-4.35"></path>
      </svg>
      <input type="text" class="search-input" id="table-search" placeholder="Search tables, views, or columns" autocomplete="off">
      <button type="button" class="search-clear" id="search-clear" title="Clear search">&times;</button>
    </div>
    <div class="search-results" id="search-results"></div>
{type_block}    <section class="object-browser-section" aria-label="Schema objects">
      <div class="object-browser-header">
        <span>Objects</span>
        <span class="object-browser-count" id="object-browser-count"></span>
      </div>
      <div class="object-browser-list" id="object-browser-list"></div>
      <p class="object-browser-empty" id="object-browser-empty" hidden>No matching objects.</p>
    </section>
{group_block}  </aside>
"#,
    )
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
    fn test_escape_xml_text_for_html() {
        assert_eq!(escape_xml_text("<script>"), "&lt;script&gt;");
        assert_eq!(escape_xml_text("a & b"), "a &amp; b");
        assert_eq!(escape_xml_text(r#""quoted""#), "&quot;quoted&quot;");
        assert_eq!(escape_xml_text("'quoted'"), "&#39;quoted&#39;");
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
        assert!(html.contains(r#"<link rel="icon" href="data:image/png;base64,"#));
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
    fn test_pan_zoom_js_clamps_panning_to_viewport_bounds() {
        let js = build_pan_zoom_js();

        assert!(js.contains("const clampPan ="));
        assert!(js.contains("const getAvailableViewport ="));
        assert!(js.contains("contentX - diagram.x"));
    }

    #[test]
    fn test_css_dark_theme() {
        let css = build_css(Theme::Dark, true, false, false, false, false);

        assert!(css.contains("--bg-color: #0c0f1a"));
        assert!(css.contains("color-scheme: dark"));
        assert!(css.contains("--text-color: #e2e8f0"));
    }

    #[test]
    fn test_css_light_theme() {
        let css = build_css(Theme::Light, true, false, false, false, false);

        assert!(css.contains("--bg-color: #f7f8fc"));
        assert!(css.contains("color-scheme: light"));
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

        assert!(!css.contains(".group-item input[type=\"checkbox\"]"));
        assert!(!css.contains(".hidden-by-group"));
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
        assert!(html.contains(r#"id="object-browser-list""#));
        assert!(html.contains("type-filter-section"));
        assert!(html.contains("filter-reset-bar"));
        assert!(html.contains("viewer-controls"));
        assert!(html.contains("detail-drawer"));
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

        assert!(!css.contains("/* Explorer sidebar styles */"));
        assert!(!css.contains(".search-container"));
        assert!(!css.contains(".dimmed-by-search"));
    }

    #[test]
    fn test_search_panel_html_structure() {
        let html = build_search_panel_html(true, true);

        assert!(html.contains(r#"class="search-panel""#));
        assert!(html.contains(r#"class="search-icon""#));
        assert!(html.contains(r#"class="search-input""#));
        assert!(html.contains(r#"id="table-search""#));
        assert!(html.contains(r#"class="search-clear""#));
        assert!(html.contains(r#"id="search-clear""#));
        assert!(html.contains(r#"class="search-results""#));
        assert!(html.contains(r#"id="search-results""#));
        assert!(html.contains("type-filter-section"));
        assert!(html.contains("Filter by type"));
        assert!(html.contains("type-filter-select-visible"));
        assert!(html.contains("type-filter-clear"));
        assert!(html.contains(">All<"));
        assert!(html.contains(">None<"));
        assert!(html.contains(r#"id="object-browser-list""#));
        assert!(html.contains(r#"id="object-browser-count""#));
        assert!(html.contains(r#"id="group-panel""#));
    }

    #[test]
    fn test_search_panel_html_without_column_type_filter() {
        let html = build_search_panel_html(false, false);

        assert!(html.contains(r#"class="search-panel""#));
        assert!(!html.contains("type-filter-section"));
        assert!(!html.contains(r#"id="group-panel""#));
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
        assert!(html.contains("detail-drawer"));
        assert!(html.contains("minimap"));
        assert!(html.contains("zoom-fit"));
    }

    #[test]
    fn test_build_viewer_controls_html() {
        let html = build_viewer_controls_html();

        assert!(html.contains(r#"id="zoom-in""#));
        assert!(html.contains(r#"id="zoom-out""#));
        assert!(html.contains(r#"id="zoom-level""#));
        assert!(html.contains(r#"id="zoom-fit""#));
        assert!(html.contains(r#"id="minimap""#));
        // Verify SVG icons replaced text labels
        assert!(html.contains("<svg"));
        assert!(!html.contains(">+</button>"));
        assert!(!html.contains(">-</button>"));
        assert!(!html.contains(">Fit</button>"));
    }

    #[test]
    fn test_viewer_controls_svg_icons_in_css() {
        let css = build_css(Theme::Dark, false, true, false, false, true);

        assert!(css.contains(".viewer-control-button svg"));
        assert!(css.contains("width: 16px"));
    }

    #[test]
    fn test_build_detail_drawer_html() {
        let html = build_detail_drawer_html();

        assert!(html.contains(r#"id="detail-drawer""#));
        assert!(html.contains(r#"id="detail-title""#));
        assert!(html.contains(r#"id="detail-columns""#));
        assert!(html.contains(r#"id="detail-relations""#));
    }

    #[test]
    fn test_build_filter_reset_bar_html() {
        let html = build_filter_reset_bar_html();

        assert!(html.contains(r#"id="filter-reset-bar""#));
        assert!(html.contains(r#"id="filter-reset-button""#));
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
        assert!(css.contains(".node.selected-node {"));
        assert!(css.contains("transition: stroke 0.3s"));
    }

    #[test]
    fn test_highlight_css_not_included_when_disabled() {
        let css = build_css(Theme::Light, false, false, false, false, false);

        assert!(!css.contains("/* Neighbor highlight styles */"));
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

    #[test]
    fn test_detail_drawer_pill_css() {
        let css = build_css(Theme::Dark, false, false, false, false, true);

        assert!(css.contains(".detail-column-pills"));
        assert!(css.contains(".detail-column-pill"));
        assert!(css.contains(".detail-column-pill-pk"));
    }

    #[test]
    fn test_panel_radius_consistency() {
        let css = build_css(Theme::Dark, true, true, false, false, true);

        // All major panels use 22px radius
        assert!(css.contains(".search-panel"));
        assert!(css.contains(".minimap-shell"));
        assert!(css.contains(".detail-drawer"));
        // minimap-shell should use 22px, not 18px
        assert!(!css.contains("border-radius: 18px"));
    }

    #[test]
    fn test_minimap_enhanced_visibility() {
        let css = build_css(Theme::Dark, false, true, false, false, true);

        assert!(css.contains(".minimap-node.selected"));
        assert!(css.contains("drop-shadow"));
        assert!(css.contains("stroke-dasharray"));
        assert!(css.contains(".minimap-frame"));
    }
}
