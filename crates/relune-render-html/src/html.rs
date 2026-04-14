//! HTML document generation.
//!
//! Embedded viewer scripts are authored in `ts/` and committed as bundled assets in `src/js/`.

use crate::components::{
    build_collapse_js, build_detail_drawer_html, build_filter_engine_js,
    build_filter_reset_bar_html, build_group_panel_html, build_group_toggle_js, build_highlight_js,
    build_hover_popover_html, build_load_motion_js, build_minimap_js, build_pan_zoom_js,
    build_search_js, build_search_panel_html, build_shortcuts_js, build_url_state_js,
    build_viewer_controls_html,
};
use crate::css::build_css;
use crate::options::HtmlRenderOptions;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use relune_render_theme::escape_xml_text;
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
        js_parts.push(build_filter_engine_js());
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

    let heading = options
        .title
        .as_deref()
        .map(|heading| format!("<h1>{}</h1>", escape_xml_text(heading)));

    let group_panel = if options.enable_group_toggles && !options.enable_search {
        Some(build_group_panel_html())
    } else {
        None
    };

    let search_panel = if options.enable_search {
        Some(build_search_panel_html(options.enable_group_toggles))
    } else {
        None
    };

    let filter_reset_bar = if options.enable_search {
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

    let hover_popover = if options.enable_highlight {
        Some(build_hover_popover_html())
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
{hover_popover}
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
        hover_popover = hover_popover.unwrap_or_default(),
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
    use crate::options::Theme;

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

        assert!(js.contains("function clampPan("));
        assert!(js.contains("function getAvailableViewport("));
        assert!(js.contains("contentX - diagram.x"));
    }

    #[test]
    fn test_css_dark_theme() {
        let css = build_css(Theme::Dark, true, false, false, false);

        assert!(css.contains("--bg-color: #0c0f1a"));
        assert!(css.contains("color-scheme: dark"));
        assert!(css.contains("--text-color: #e2e8f0"));
    }

    #[test]
    fn test_css_light_theme() {
        let css = build_css(Theme::Light, true, false, false, false);

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
        let css = build_css(Theme::Light, true, false, false, false);

        assert!(css.contains(".group-panel"));
        assert!(css.contains(".group-item"));
        assert!(css.contains(".hidden-by-group"));
    }

    #[test]
    fn test_group_panel_css_not_included_when_disabled() {
        let css = build_css(Theme::Light, false, false, false, false);

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
        assert!(html.contains("filter-section"));
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
        let css = build_css(Theme::Light, false, true, false, false);

        assert!(css.contains(".search-panel"));
        assert!(css.contains(".search-input"));
        assert!(css.contains(".search-clear"));
        assert!(css.contains(".search-results"));
        assert!(css.contains(".dimmed-by-search"));
        assert!(css.contains(".highlighted-by-search"));
        assert!(css.contains(".dimmed-by-edge-filter"));
        assert!(css.contains(".filter-section"));
    }

    #[test]
    fn test_search_css_not_included_when_disabled() {
        let css = build_css(Theme::Light, false, false, false, false);

        assert!(!css.contains("/* Explorer sidebar styles */"));
        assert!(!css.contains(".search-container"));
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
        assert!(html.contains("filter-section"));
        assert!(html.contains("filter-facets"));
        assert!(html.contains(r#"id="object-browser-list""#));
        assert!(html.contains(r#"id="object-browser-count""#));
        assert!(html.contains(r#"id="group-panel""#));
    }

    #[test]
    fn test_search_panel_html_without_groups() {
        let html = build_search_panel_html(false);

        assert!(html.contains(r#"class="search-panel""#));
        assert!(html.contains("filter-section"));
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
        assert!(html.contains("filter-section"));
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
        let css = build_css(Theme::Dark, false, true, false, true);

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
    fn test_build_hover_popover_html() {
        let html = build_hover_popover_html();

        assert!(html.contains(r#"id="hover-popover""#));
        assert!(html.contains(r#"id="hover-popover-title""#));
        assert!(html.contains(r#"id="hover-popover-metrics""#));
        assert!(html.contains(r#"id="hover-popover-badges""#));
    }

    #[test]
    fn test_build_filter_reset_bar_html() {
        let html = build_filter_reset_bar_html();

        assert!(html.contains(r#"id="filter-reset-bar""#));
        assert!(html.contains(r#"id="filter-reset-button""#));
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
        let css = build_css(Theme::Light, false, false, false, true);

        assert!(css.contains(".hover-popover"));
        assert!(css.contains(".hover-preview-node"));
        assert!(css.contains(".hover-preview-edge"));
        assert!(css.contains(".highlighted-neighbor"));
        assert!(css.contains(".dimmed-by-highlight"));
        assert!(css.contains(".selected-node"));
        assert!(css.contains(".node.selected-node {"));
        assert!(css.contains("transition: stroke 0.3s"));
    }

    #[test]
    fn test_highlight_css_not_included_when_disabled() {
        let css = build_css(Theme::Light, false, false, false, false);

        assert!(!css.contains("/* Neighbor highlight styles */"));
        assert!(!css.contains(".hover-popover"));
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

        assert!(html.contains("computeNeighborHighlights"));
        assert!(html.contains("computeHoverPreview"));
        assert!(html.contains("clearHighlightClasses"));
        assert!(html.contains("renderHoverPopover"));
        assert!(html.contains("hoveredNode"));
        assert!(html.contains("inboundMap"));
        assert!(html.contains("outboundMap"));
        assert!(html.contains(r#"id="hover-popover""#));
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

        assert!(!html.contains("computeNeighborHighlights"));
        assert!(!html.contains("computeHoverPreview"));
        assert!(!html.contains("clearHighlightClasses"));
        assert!(!html.contains("inboundMap"));
        assert!(!html.contains("outboundMap"));
        assert!(!html.contains(r#"id="hover-popover""#));
    }

    #[test]
    fn test_detail_drawer_pill_css() {
        let css = build_css(Theme::Dark, false, false, false, true);

        assert!(css.contains(".detail-column-pills"));
        assert!(css.contains(".detail-column-pill"));
        assert!(css.contains(".detail-column-pill-pk"));
    }

    #[test]
    fn test_detail_drawer_layout_css() {
        let css = build_css(Theme::Dark, false, false, false, true);

        assert!(css.contains("grid-template-columns: repeat(2, minmax(0, 1fr));"));
        assert!(css.contains(".detail-relations .detail-relation"));
        assert!(css.contains(".detail-relation-meta"));
        assert!(css.contains("line-height: 1.35;"));
        assert!(css.contains("margin-top: 2px;"));
    }

    #[test]
    fn test_panel_radius_consistency() {
        let css = build_css(Theme::Dark, true, true, false, true);

        // All major panels use 22px radius
        assert!(css.contains(".search-panel"));
        assert!(css.contains(".minimap-shell"));
        assert!(css.contains(".detail-drawer"));
        // minimap-shell should use 22px, not 18px
        assert!(!css.contains("border-radius: 18px"));
    }

    #[test]
    fn test_minimap_enhanced_visibility() {
        let css = build_css(Theme::Dark, false, true, false, true);

        assert!(css.contains(".minimap-node.selected"));
        assert!(css.contains("drop-shadow"));
        assert!(css.contains("stroke-dasharray"));
        assert!(css.contains(".minimap-frame"));
    }

    #[test]
    fn test_render_html_title_escapes_special_chars() {
        let special_title = r#"<img src=x onerror=alert(1)> & "quotes" 'apos'"#;
        let svg = "<svg></svg>";
        let metadata = "{}";
        let options = HtmlRenderOptions {
            title: Some(special_title.to_string()),
            ..Default::default()
        };

        let html = build_html_document(svg, metadata, &options);

        // The raw payload must never appear unescaped
        assert!(!html.contains(special_title));
        // <title> must contain the escaped form
        let escaped = "&lt;img src=x onerror=alert(1)&gt; &amp; &quot;quotes&quot; &#39;apos&#39;";
        assert!(
            html.contains(&format!("<title>{escaped}</title>")),
            "title tag should contain escaped special characters"
        );
        // <h1> heading must also be escaped
        assert!(
            html.contains(&format!("<h1>{escaped}</h1>")),
            "h1 heading should contain escaped special characters"
        );
    }
}
