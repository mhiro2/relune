//! HTML component builders for the viewer shell.

/// Build the pan/zoom JavaScript.
pub(crate) const fn build_pan_zoom_js() -> &'static str {
    include_str!("js/pan_zoom.js")
}

/// Build the group panel HTML structure.
#[allow(clippy::needless_raw_string_hashes)]
pub(crate) fn build_group_panel_html() -> String {
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
pub(crate) const fn build_group_toggle_js() -> &'static str {
    include_str!("js/group_toggle.js")
}

/// Build the search JavaScript.
pub(crate) const fn build_search_js() -> &'static str {
    include_str!("js/search.js")
}

/// Build the filter engine JavaScript.
pub(crate) const fn build_filter_engine_js() -> &'static str {
    include_str!("js/filter_engine.js")
}

/// Build the collapse JavaScript.
pub(crate) const fn build_collapse_js() -> &'static str {
    include_str!("js/collapse.js")
}

/// Build the highlight neighbors JavaScript.
pub(crate) const fn build_highlight_js() -> &'static str {
    include_str!("js/highlight.js")
}

/// Build the minimap JavaScript.
pub(crate) const fn build_minimap_js() -> &'static str {
    include_str!("js/minimap.js")
}

/// Build the keyboard shortcuts JavaScript.
pub(crate) const fn build_shortcuts_js() -> &'static str {
    include_str!("js/shortcuts.js")
}

/// Build the load animation JavaScript.
pub(crate) const fn build_load_motion_js() -> &'static str {
    include_str!("js/load_motion.js")
}

/// Build the URL state synchronisation JavaScript.
pub(crate) const fn build_url_state_js() -> &'static str {
    include_str!("js/url_state.js")
}

#[allow(clippy::needless_raw_string_hashes)]
pub(crate) fn build_filter_reset_bar_html() -> String {
    r#"  <div class="filter-reset-bar" id="filter-reset-bar" hidden>
    <span class="filter-reset-copy" id="filter-reset-copy"></span>
    <button type="button" class="filter-reset-button" id="filter-reset-button">Reset filters</button>
  </div>
"#
    .to_string()
}

#[allow(clippy::needless_raw_string_hashes)]
pub(crate) fn build_viewer_controls_html() -> String {
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
pub(crate) fn build_detail_drawer_html() -> String {
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
    <div class="detail-traversal" id="detail-traversal" hidden>
      <span class="detail-traversal-label">Highlight</span>
      <div class="detail-traversal-buttons">
        <button type="button" class="detail-traversal-btn active" data-depth="1">1-hop</button>
        <button type="button" class="detail-traversal-btn" data-depth="2">2-hop</button>
      </div>
    </div>
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

#[allow(clippy::needless_raw_string_hashes)]
pub(crate) fn build_hover_popover_html() -> String {
    r#"  <aside class="hover-popover" id="hover-popover" hidden>
    <p class="hover-popover-kicker" id="hover-popover-kind">Preview</p>
    <h2 class="hover-popover-title" id="hover-popover-title">Object preview</h2>
    <p class="hover-popover-subtitle" id="hover-popover-subtitle"></p>
    <div class="hover-popover-metrics" id="hover-popover-metrics"></div>
    <div class="hover-popover-badges" id="hover-popover-badges"></div>
  </aside>
"#
    .to_string()
}

/// Build the search panel HTML structure.
#[allow(clippy::needless_raw_string_hashes)]
pub(crate) fn build_search_panel_html(enable_group_toggles: bool) -> String {
    let filter_block = r#"    <section class="filter-section" id="filter-section" aria-label="Filters">
      <div class="filter-section-header" id="filter-section-header"></div>
      <div class="filter-active-summary" id="filter-active-summary"></div>
      <div class="filter-facets" id="filter-facets"></div>
    </section>
"#;

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
{filter_block}    <section class="object-browser-section" aria-label="Schema objects">
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
