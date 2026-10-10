//! CSS generation for the HTML viewer.

use crate::options::Theme;
use relune_render_theme::{BadgeColor, get_colors};

/// CSS background of a column badge: its fill at the badge's opacity.
fn badge_background(badge: BadgeColor) -> String {
    format!(
        "color-mix(in srgb, {} {}%, transparent)",
        badge.fill, badge.fill_opacity_percent
    )
}

/// Build CSS styles based on theme and options.
#[allow(clippy::too_many_lines)]
#[allow(clippy::fn_params_excessive_bools)]
pub(crate) fn build_css(
    theme: Theme,
    enable_group_toggles: bool,
    enable_search: bool,
    enable_highlight: bool,
) -> String {
    let colors = get_colors(theme);
    let selection_color = colors.selection_color;
    let (panel_bg, panel_border, panel_shadow, grid_dot, grid_line) = match theme {
        Theme::Dark => (
            "rgba(14, 18, 30, 0.94)",
            "rgba(148, 163, 184, 0.18)",
            "0 8px 24px rgba(2, 6, 23, 0.4)",
            "rgba(148, 163, 184, 0.12)",
            "rgba(148, 163, 184, 0.05)",
        ),
        Theme::Light => (
            "rgba(255, 255, 255, 0.94)",
            "rgba(71, 85, 105, 0.16)",
            "0 6px 20px rgba(15, 23, 42, 0.08)",
            "rgba(71, 85, 105, 0.12)",
            "rgba(71, 85, 105, 0.04)",
        ),
    };

    let search_css = if enable_search {
        r"
    /* Explorer sidebar: search and the object list first, filters and
       groups as collapsed sections underneath. */
    .search-panel {
      position: fixed;
      top: 12px;
      left: 12px;
      bottom: 12px;
      width: min(300px, calc(100vw - 24px));
      display: flex;
      flex-direction: column;
      min-height: 0;
      background: var(--panel-bg);
      border: 1px solid var(--panel-border);
      border-radius: 14px;
      z-index: 240;
      overflow: hidden;
      box-shadow: var(--panel-shadow);
      backdrop-filter: blur(16px);
    }

    .search-panel[hidden],
    .sidebar-open[hidden] {
      display: none;
    }

    body:has(h1) .search-panel {
      top: 61px;
    }

    .sidebar-open {
      position: fixed;
      top: 12px;
      left: 12px;
      display: inline-flex;
      align-items: center;
      gap: 6px;
      height: 34px;
      padding: 0 12px;
      border: 1px solid var(--panel-border);
      border-radius: 999px;
      background: var(--panel-bg);
      box-shadow: var(--panel-shadow);
      backdrop-filter: blur(16px);
      color: var(--text-color);
      font: 600 12px var(--ui-font);
      cursor: pointer;
      z-index: 240;
    }

    body:has(h1) .sidebar-open {
      top: 61px;
    }

    .sidebar-open svg,
    .sidebar-collapse svg {
      width: 16px;
      height: 16px;
    }

    .sidebar-open:hover,
    .sidebar-open:focus-visible {
      border-color: var(--selection-color);
      outline: none;
    }

    .search-container {
      display: flex;
      align-items: center;
      padding: 8px 8px 8px 14px;
      gap: 8px;
      border-bottom: 1px solid var(--panel-border);
    }

    .search-icon {
      flex-shrink: 0;
      width: 15px;
      height: 15px;
      opacity: 0.6;
    }

    .search-input {
      flex: 1;
      min-width: 0;
      height: 28px;
      border: none;
      background: transparent;
      font-family: var(--ui-font);
      font-size: 13px;
      color: var(--text-color);
      outline: none;
    }

    .search-input::placeholder {
      color: var(--text-color);
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
      border-radius: 50%;
      display: none;
      align-items: center;
      justify-content: center;
      opacity: 0.6;
      font-size: 16px;
      line-height: 1;
    }

    .search-clear.visible {
      display: flex;
    }

    .search-clear:hover {
      opacity: 1;
      background-color: var(--selection-faint);
    }

    .search-shortcut {
      flex-shrink: 0;
      min-width: 18px;
      padding: 1px 5px;
      border: 1px solid var(--panel-border);
      border-radius: 4px;
      font: 600 11px var(--mono-font);
      text-align: center;
      opacity: 0.7;
    }

    .search-input:focus ~ .search-shortcut,
    .search-clear.visible ~ .search-shortcut {
      display: none;
    }

    .sidebar-collapse {
      flex-shrink: 0;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      width: 28px;
      height: 28px;
      border: none;
      border-radius: 8px;
      background: transparent;
      color: var(--text-color);
      cursor: pointer;
      opacity: 0.7;
    }

    .sidebar-collapse:hover,
    .sidebar-collapse:focus-visible {
      opacity: 1;
      background: var(--selection-faint);
      outline: none;
    }

    .search-results {
      padding: 6px 14px 0;
      font-size: 12px;
      opacity: 0.75;
      display: none;
    }

    .search-results.visible {
      display: block;
    }

    .object-browser-section {
      display: flex;
      flex: 1;
      flex-direction: column;
      min-height: 120px;
    }

    .object-browser-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
      padding: 10px 14px 4px;
      font-size: 11px;
      font-weight: 600;
      letter-spacing: 0.06em;
      text-transform: uppercase;
      opacity: 0.75;
    }

    .search-panel-meta,
    .object-browser-count {
      font-size: 11px;
      letter-spacing: normal;
      text-transform: none;
      white-space: nowrap;
    }

    .object-browser-list {
      flex: 1;
      min-height: 0;
      overflow-y: auto;
      padding: 2px 0 8px;
    }

    .object-browser-empty {
      padding: 0 14px 12px;
      font-size: 12px;
      opacity: 0.7;
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
      padding: 6px 14px 6px 12px;
      cursor: pointer;
      transition: background-color 0.16s, border-color 0.16s, opacity 0.16s;
    }

    .object-browser-item:hover {
      background: var(--selection-faint);
    }

    .object-browser-item.selected {
      background: var(--selection-faint);
      border-left-color: var(--selection-color);
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
      margin-bottom: 1px;
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
      font-size: 10px;
      font-weight: 600;
      letter-spacing: 0.06em;
      text-transform: uppercase;
      opacity: 0.7;
    }

    .object-browser-item-meta {
      font-size: 11px;
      opacity: 0.7;
      font-family: var(--mono-font);
    }

    /* Collapsed sections under the object list. */
    .sidebar-section {
      flex-shrink: 0;
      border-top: 1px solid var(--panel-border);
    }

    .sidebar-section-summary {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 9px 14px;
      font-size: 12px;
      font-weight: 600;
      cursor: pointer;
      user-select: none;
      list-style: none;
    }

    .sidebar-section-summary::-webkit-details-marker {
      display: none;
    }

    .sidebar-section-summary::before {
      content: '\25B6';
      font-size: 8px;
      opacity: 0.7;
      transition: transform 0.16s;
    }

    .sidebar-section[open] > .sidebar-section-summary::before {
      transform: rotate(90deg);
    }

    .sidebar-section-summary:hover {
      background: var(--selection-faint);
    }

    .sidebar-section-summary:focus-visible {
      outline: 2px solid var(--selection-color);
      outline-offset: -2px;
    }

    .sidebar-section-title {
      flex: 1;
      min-width: 0;
    }

    .sidebar-section-meta {
      font-size: 11px;
      font-weight: 500;
      opacity: 0.7;
    }

    .sidebar-section-badge {
      min-width: 18px;
      padding: 1px 6px;
      border-radius: 999px;
      background: var(--selection-color);
      color: var(--bg-color);
      text-align: center;
      font-size: 10px;
      font-weight: 700;
    }

    .sidebar-section-badge[hidden] {
      display: none;
    }

    .sidebar-section-body {
      max-height: 40vh;
      overflow-y: auto;
      padding-bottom: 6px;
    }

    .node.dimmed-by-search {
      opacity: 0.25;
      transition: opacity 0.2s;
    }

    .node.highlighted-by-search {
      opacity: 1;
      transition: opacity 0.2s;
    }

    .node.dimmed-by-filter {
      opacity: 0.2;
      transition: opacity 0.2s;
    }

    .node.dimmed-by-search.dimmed-by-filter {
      opacity: 0.08;
    }

    .node.hidden-by-filter,
    .edge.hidden-by-filter {
      display: none !important;
    }

    .edge.dimmed-by-edge-filter {
      opacity: 0.12;
      transition: opacity 0.2s;
    }"
    } else {
        ""
    };

    let filter_section_css = if enable_search {
        r"
    /* ── Filter section ─────────────────────────────────────────────── */

    .filter-section-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      padding: 2px 14px 8px;
    }

    .filter-mode-switcher {
      display: flex;
      gap: 0;
      border: 1px solid var(--panel-border);
      border-radius: 999px;
      overflow: hidden;
    }

    .filter-mode-button {
      background: transparent;
      border: none;
      color: var(--text-color);
      font-size: 10px;
      font-weight: 600;
      cursor: pointer;
      padding: 3px 10px;
      opacity: 0.6;
      transition: background-color 0.16s, opacity 0.16s;
    }

    .filter-mode-button:hover {
      opacity: 0.9;
      background-color: var(--selection-faint);
    }

    .filter-mode-button.active {
      opacity: 1;
      background-color: var(--selection-faint);
    }

    .filter-section-reset {
      background: transparent;
      border: 1px solid var(--panel-border);
      color: var(--text-color);
      font-size: 11px;
      cursor: pointer;
      padding: 3px 8px;
      border-radius: 999px;
      opacity: 0.7;
      transition: background-color 0.16s, border-color 0.16s, opacity 0.16s;
    }

    .filter-section-reset:hover {
      opacity: 1;
      border-color: var(--selection-color);
      background-color: var(--selection-faint);
    }

    .filter-section-reset[hidden] {
      display: none;
    }

    .filter-active-summary {
      display: flex;
      flex-wrap: wrap;
      gap: 4px;
      padding: 0 14px 8px;
    }

    .filter-active-summary[hidden] {
      display: none;
    }

    .filter-summary-chip {
      background: var(--selection-faint);
      border: none;
      color: var(--text-color);
      font-size: 10px;
      font-weight: 600;
      padding: 2px 8px;
      border-radius: 999px;
      cursor: pointer;
      transition: background-color 0.16s;
    }

    .filter-summary-chip:hover {
      outline: 1px solid var(--selection-color);
    }

    /* ── Facet sections ─────────────────────────────────────────────── */

    .filter-facet {
      position: relative;
      border-top: 1px solid var(--panel-border);
    }

    .filter-facet-summary {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 8px 128px 8px 14px;
      font-size: 12px;
      font-weight: 600;
      cursor: pointer;
      user-select: none;
      list-style: none;
      opacity: 0.9;
    }

    .filter-facet-summary::-webkit-details-marker {
      display: none;
    }

    .filter-facet-summary::before {
      content: '\25B6';
      font-size: 8px;
      transition: transform 0.16s;
    }

    .filter-facet[open] > .filter-facet-summary::before {
      transform: rotate(90deg);
    }

    .filter-facet-label {
      flex: 1;
      min-width: 0;
    }

    .filter-facet-badge {
      min-width: 18px;
      flex-shrink: 0;
      padding: 1px 6px;
      border-radius: 999px;
      background: var(--selection-color);
      color: var(--bg-color);
      text-align: center;
      font-size: 10px;
      font-weight: 700;
    }

    .filter-facet-badge[hidden] {
      display: none;
    }

    .filter-facet-actions {
      position: absolute;
      top: 8px;
      right: 14px;
      display: flex;
      gap: 4px;
      align-items: center;
      z-index: 1;
    }

    .filter-facet-action {
      background: transparent;
      border: 1px solid var(--panel-border);
      color: var(--text-color);
      font-size: 10px;
      cursor: pointer;
      padding: 2px 6px;
      border-radius: 999px;
      opacity: 0.7;
      transition: background-color 0.16s, border-color 0.16s, opacity 0.16s;
    }

    .filter-facet-action:hover {
      opacity: 1;
      border-color: var(--selection-color);
      background-color: var(--selection-faint);
    }

    .filter-facet-search {
      display: block;
      width: calc(100% - 28px);
      margin: 0 14px 6px;
      padding: 6px 10px;
      font-size: 12px;
      border: 1px solid var(--panel-border);
      border-radius: 10px;
      background: rgba(15, 23, 42, 0.02);
      color: var(--text-color);
    }

    .filter-facet-list {
      max-height: min(180px, 24vh);
      overflow-y: auto;
      padding: 2px 0 8px;
    }

    .filter-facet-item {
      display: flex;
      align-items: center;
      gap: 10px;
      padding: 5px 14px;
      font-size: 12px;
      cursor: pointer;
      transition: background-color 0.16s;
    }

    .filter-facet-item:hover {
      background-color: var(--selection-faint);
    }

    .filter-facet-item span {
      word-break: break-word;
      font-family: var(--mono-font);
    }

    .filter-facet-item-count {
      margin-left: auto;
      min-width: 24px;
      padding: 1px 6px;
      border-radius: 999px;
      background: var(--selection-faint);
      text-align: center;
      font-size: 10px;
      font-weight: 700;
      font-family: var(--ui-font);
    }"
    } else {
        ""
    };

    let group_panel_css = if enable_group_toggles {
        r#"
    /* Group panel styles */
    body > .group-panel {
      position: fixed;
      top: 12px;
      left: 12px;
      width: min(300px, calc(100vw - 24px));
      max-height: calc(100vh - 24px);
      background: var(--panel-bg);
      border: 1px solid var(--panel-border);
      border-radius: 14px;
      z-index: 220;
      overflow: hidden;
      box-shadow: var(--panel-shadow);
      backdrop-filter: blur(16px);
    }

    body:has(h1) > .group-panel {
      top: 61px;
    }

    .group-panel-actions {
      display: flex;
      gap: 6px;
      padding: 0 10px 4px;
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
      border-color: var(--selection-color);
      background-color: var(--selection-faint);
    }

    .group-item {
      display: flex;
      align-items: center;
      padding: 5px 14px;
      cursor: pointer;
      transition: background-color 0.15s;
    }

    .group-item:hover {
      background-color: var(--selection-faint);
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

    /* Hidden groups take their tables, lines, surface, and label along. */
    .hidden-by-group {
      display: none !important;
    }"#
    } else {
        ""
    };

    let highlight_css = if enable_highlight {
        r"
    /* Neighbor highlight styles */
    .hover-popover {
      position: fixed;
      min-width: 220px;
      max-width: min(280px, calc(100vw - 24px));
      padding: 12px 14px;
      border: 1px solid var(--panel-border);
      border-radius: 16px;
      background: color-mix(in srgb, var(--panel-bg) 96%, transparent);
      box-shadow: var(--panel-shadow);
      backdrop-filter: blur(16px);
      z-index: 245;
      pointer-events: none;
    }

    .hover-popover[hidden] {
      display: none;
    }

    .hover-popover-kicker {
      margin: 0 0 4px;
      font-size: 10px;
      letter-spacing: 0.08em;
      text-transform: uppercase;
      opacity: 0.72;
    }

    .hover-popover-title {
      margin: 0;
      font-size: 15px;
      line-height: 1.2;
    }

    .hover-popover-subtitle {
      margin: 6px 0 0;
      font-size: 12px;
      opacity: 0.72;
    }

    .hover-popover-metrics,
    .hover-popover-badges {
      display: flex;
      flex-wrap: wrap;
      gap: 6px;
      margin-top: 10px;
    }

    .hover-popover-metric,
    .hover-popover-badge {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 4px 8px;
      border-radius: 999px;
      background: rgba(148, 163, 184, 0.1);
      font-size: 11px;
      line-height: 1;
      white-space: nowrap;
    }

    .hover-popover-metric-label {
      opacity: 0.65;
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }

    .hover-popover-metric-value {
      font-family: var(--mono-font);
      font-weight: 700;
    }

    .hover-popover-badge-breaking { background: var(--risk-breaking); color: var(--risk-text); }
    .hover-popover-badge-caution { background: var(--risk-caution); color: var(--risk-text); }
    .hover-popover-badge-warning { background: var(--risk-warning); color: var(--risk-text); }
    .hover-popover-badge-info { background: var(--risk-info); color: var(--risk-text); }

    /* Table highlights are outlines in the selection colour, never glows, so
       they stay distinct from kind marks and review severities. */
    .node.hover-preview-node,
    .node.hover-preview-neighbor {
      opacity: 1 !important;
      transition: opacity 0.18s;
    }

    .node.hover-preview-node .table-body {
      stroke: var(--selection-color);
      stroke-width: 1.8px;
    }

    .node.hover-preview-neighbor .table-body {
      stroke: var(--selection-soft);
      stroke-width: 1.5px;
    }

    .edge.hover-preview-edge {
      opacity: 0.92 !important;
      stroke-width: 2.15px;
      transition: opacity 0.18s, stroke-width 0.18s;
    }

    .node.highlighted-neighbor {
      opacity: 1 !important;
      transition: opacity 0.2s;
    }

    .node .table-body {
      transition: stroke 0.3s, stroke-width 0.3s, opacity 0.3s;
    }

    .node.highlighted-neighbor .table-body {
      stroke: var(--selection-soft);
      stroke-width: 1.5px;
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

    .node.selected-node .table-body {
      stroke: var(--selection-color);
      stroke-width: 2px;
    }

    /* A selected relationship: its line, ports, endpoint tables, and the
       columns on both ends (every column of a composite key). */
    .node.relation-endpoint {
      opacity: 1 !important;
    }

    .node.relation-endpoint .table-body {
      stroke: var(--selection-soft);
      stroke-width: 1.5px;
    }

    .relation-column-band {
      fill: var(--selection-color);
      fill-opacity: 0.16;
      pointer-events: none;
    }

    .column-row.relation-column .column-name {
      fill: var(--text-color);
      font-weight: 700;
    }

    .edge.selected-edge {
      opacity: 1 !important;
    }

    .edge.selected-edge .edge-path,
    .edge.selected-edge .crow-inline {
      stroke: var(--selection-color);
      stroke-width: 2.4px;
    }

    .visually-hidden {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }

    .relation-port {
      fill: var(--selection-color);
      pointer-events: none;
    }

    /* Takes the detail drawer's place, which is closed while a
       relationship is selected. */
    .relation-card {
      position: fixed;
      top: 12px;
      right: 12px;
      width: min(320px, calc(100vw - 24px));
      max-height: calc(100vh - 24px);
      overflow: auto;
      padding: 12px 14px;
      border: 1px solid var(--panel-border);
      border-radius: 14px;
      background: var(--panel-bg);
      box-shadow: var(--panel-shadow);
      backdrop-filter: blur(16px);
      z-index: 246;
    }

    .relation-card[hidden] {
      display: none;
    }

    .relation-card:focus-visible {
      outline: 2px solid var(--selection-color);
      outline-offset: 2px;
    }

    .relation-card-header {
      display: flex;
      align-items: flex-start;
      justify-content: space-between;
      gap: 12px;
    }

    .relation-card-kicker {
      margin: 0 0 2px;
      font-size: 10px;
      letter-spacing: 0.08em;
      text-transform: uppercase;
      opacity: 0.72;
    }

    .relation-card-title {
      margin: 0;
      font-size: 14px;
      line-height: 1.3;
    }

    .relation-card-name {
      margin: 4px 0 0;
      font-family: var(--mono-font);
      font-size: 11px;
      opacity: 0.72;
    }

    .relation-card-pairs {
      margin: 10px 0 0;
      padding: 0;
      list-style: none;
      font-family: var(--mono-font);
      font-size: 12px;
      line-height: 1.6;
      overflow-wrap: anywhere;
    }

    .relation-card-actions {
      display: flex;
      flex-wrap: wrap;
      gap: 6px;
      margin-top: 10px;
    }

    .relation-card-open,
    .relation-card-close {
      font: inherit;
      color: inherit;
      background: transparent;
      border: 1px solid var(--panel-border);
      border-radius: 999px;
      cursor: pointer;
    }

    .relation-card-open {
      padding: 4px 10px;
      font-size: 12px;
    }

    .relation-card-close {
      width: 26px;
      height: 26px;
      line-height: 1;
      font-size: 16px;
    }

    .relation-card-open:hover,
    .relation-card-open:focus-visible,
    .relation-card-close:hover,
    .relation-card-close:focus-visible {
      border-color: var(--selection-color);
      outline: none;
    }

    .edge.highlighted-neighbor .edge-path,
    .edge.highlighted-neighbor .crow-inline,
    .edge.hover-preview-edge .edge-path,
    .edge.hover-preview-edge .crow-inline {
      stroke: var(--selection-color);
    }

    @media (max-width: 960px) {
      .hover-popover {
        width: calc(100vw - 24px);
      }
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

    /* The sidebar shows active filters itself; the bar stands in for it
       while the sidebar is put away. */
    .search-panel:not([hidden]) ~ .filter-reset-bar {
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
      border: 1px solid var(--selection-color);
      background: var(--selection-faint);
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
      border-color: var(--selection-color);
      background: color-mix(in srgb, var(--panel-bg) 82%, var(--selection-faint));
    }

    .viewer-control-button[aria-pressed=false] {
      opacity: 0.55;
    }

    .viewer-control-button[aria-pressed=false]:hover {
      opacity: 1;
    }

    /* The minimap sits in the bottom-right corner and steps left of the
       detail drawer while one is open. */
    .minimap-shell {
      position: fixed;
      right: 12px;
      bottom: 16px;
      width: min(220px, calc(100vw - 24px));
      border: 1px solid var(--panel-border);
      border-radius: 14px;
      background: var(--panel-bg);
      box-shadow: var(--panel-shadow);
      backdrop-filter: blur(16px);
      overflow: hidden;
      z-index: 210;
    }

    .minimap-shell[hidden] {
      display: none;
    }

    body:has(#detail-drawer:not([hidden])) .minimap-shell {
      right: calc(min(320px, 100vw - 24px) + 24px);
    }

    .minimap {
      display: block;
      width: 100%;
      height: 140px;
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

    .minimap-node.hidden {
      display: none;
    }

    .minimap-node.selected {
      fill: var(--selection-color);
      stroke: var(--selection-color);
    }

    .minimap-frame {
      fill: var(--selection-faint);
      stroke: var(--selection-color);
      stroke-width: 1.8;
      stroke-dasharray: 4 2;
      rx: 2;
    }

    /* The drawer is only as tall as its content, so short tables leave
       the rest of the right edge to the diagram. */
    .detail-drawer {
      position: fixed;
      top: 12px;
      right: 12px;
      width: min(320px, calc(100vw - 24px));
      max-height: calc(100vh - 24px);
      overflow: auto;
      padding: 16px;
      border: 1px solid var(--panel-border);
      border-radius: 14px;
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
      max-height: calc(100vh - 73px);
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
      opacity: 0.72;
      margin-bottom: 6px;
    }

    .detail-title-row {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: 8px;
    }

    .detail-title {
      font-size: 20px;
      line-height: 1.15;
      margin: 0;
      min-width: 0;
      overflow-wrap: anywhere;
    }

    .detail-title-badges {
      display: inline-flex;
      flex-wrap: wrap;
      gap: 6px;
    }

    .detail-title-badges,
    .detail-title-badges * {
      user-select: none;
      -webkit-user-select: none;
    }

    .detail-title-badges:empty {
      display: none;
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

    .detail-traversal {
      display: flex;
      align-items: center;
      gap: 8px;
      margin: 12px 0 4px;
    }

    .detail-traversal-label {
      font-size: 11px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
      opacity: 0.55;
      font-weight: 600;
    }

    .detail-traversal-buttons {
      display: flex;
      gap: 4px;
    }

    .detail-traversal-btn {
      padding: 3px 10px;
      border: 1px solid var(--panel-border);
      border-radius: 6px;
      background: transparent;
      color: var(--text-color);
      font-size: 12px;
      font-weight: 500;
      cursor: pointer;
      transition: background 0.15s, border-color 0.15s;
    }

    .detail-traversal-btn:hover {
      border-color: var(--selection-color);
      background: color-mix(in srgb, transparent 80%, var(--selection-faint));
    }

    .detail-traversal-btn.active {
      border-color: var(--selection-color);
      background: var(--selection-faint);
      font-weight: 600;
    }

    .detail-metrics {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 10px;
      margin: 16px 0;
    }

    .detail-badge {
      display: inline-block;
      padding: 3px 10px;
      border-radius: 6px;
      font-size: 11px;
      font-weight: 600;
      letter-spacing: 0.02em;
    }

    .detail-badge-join {
      background: var(--badge-ix-bg);
      color: var(--badge-ix-text);
    }

    .detail-metric {
      padding: 10px 12px;
      border-radius: 12px;
      background: rgba(148, 163, 184, 0.08);
    }

    .detail-metric-label {
      display: block;
      font-size: 11px;
      line-height: 1.3;
      opacity: 0.65;
      margin-bottom: 4px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
      overflow-wrap: anywhere;
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

    .detail-relations .detail-relation {
      flex: 1 1 100%;
    }

    .detail-column,
    .detail-relation {
      border: 1px solid rgba(148, 163, 184, 0.12);
      border-radius: 12px;
      padding: 8px 12px;
      background: rgba(148, 163, 184, 0.05);
      transition: border-color 0.15s, background-color 0.15s;
      min-width: 0;
      overflow-wrap: anywhere;
    }

    button.detail-relation {
      width: 100%;
      text-align: left;
      font: inherit;
      color: inherit;
      cursor: pointer;
    }

    .detail-relation:hover {
      border-color: var(--selection-color);
      background: color-mix(in srgb, rgba(148, 163, 184, 0.05) 72%, var(--selection-faint));
    }

    .detail-column-name,
    .detail-relation-label {
      display: block;
      font-family: var(--mono-font);
      font-size: 13px;
      margin-bottom: 2px;
      overflow-wrap: anywhere;
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

    /* Key pills reuse the card badge colors so a column reads the same in
       the diagram and in the drawer. */
    .detail-column-pill-pk {
      background: var(--badge-pk-bg);
      color: var(--badge-pk-text);
      opacity: 1;
    }

    .detail-column-pill-fk {
      background: var(--badge-fk-bg);
      color: var(--badge-fk-text);
      opacity: 1;
    }

    .detail-column-pill-ix {
      background: var(--badge-ix-bg);
      color: var(--badge-ix-text);
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
      background: color-mix(in srgb, var(--diff-added) 14%, transparent);
      color: var(--diff-added);
    }

    .detail-column-pill-diff-removed {
      background: color-mix(in srgb, var(--diff-removed) 14%, transparent);
      color: var(--diff-removed);
    }

    .detail-column-pill-diff-modified {
      background: color-mix(in srgb, var(--diff-modified) 14%, transparent);
      color: var(--diff-modified);
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
      background: color-mix(in srgb, var(--diff-added) 14%, transparent);
      color: var(--diff-added);
    }

    .detail-diff-badge-removed {
      background: color-mix(in srgb, var(--diff-removed) 14%, transparent);
      color: var(--diff-removed);
    }

    .detail-diff-badge-modified {
      background: color-mix(in srgb, var(--diff-modified) 14%, transparent);
      color: var(--diff-modified);
    }

    .detail-column-meta,
    .detail-relation-meta {
      display: block;
      font-size: 11px;
      line-height: 1.35;
      opacity: 0.65;
      letter-spacing: 0.01em;
      overflow-wrap: anywhere;
    }

    .detail-relation-meta {
      margin-top: 2px;
    }

    .detail-empty {
      font-size: 12px;
      opacity: 0.62;
    }

    .detail-changes {
      margin: 0;
      padding: 0;
      list-style: none;
      font-family: var(--mono-font);
      font-size: 12px;
      line-height: 1.6;
      overflow-wrap: anywhere;
    }

    .detail-issue {
      border: 1px solid rgba(148, 163, 184, 0.12);
      border-radius: 12px;
      padding: 10px 12px;
      margin-bottom: 6px;
    }

    .detail-issue-breaking { border-color: var(--risk-breaking); }
    .detail-issue-caution { border-color: var(--risk-caution); }
    .detail-issue-warning { border-color: var(--risk-warning); }
    .detail-issue-info { border-color: var(--risk-info); }

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

    .detail-issue-badge-breaking { background: var(--risk-breaking); color: var(--risk-text); }
    .detail-issue-badge-caution { background: var(--risk-caution); color: var(--risk-text); }
    .detail-issue-badge-warning { background: var(--risk-warning); color: var(--risk-text); }
    .detail-issue-badge-info { background: var(--risk-info); color: var(--risk-text); }

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

    .object-browser-issue-badge-breaking { background: var(--risk-breaking); color: var(--risk-text); }
    .object-browser-issue-badge-caution { background: var(--risk-caution); color: var(--risk-text); }
    .object-browser-issue-badge-warning { background: var(--risk-warning); color: var(--risk-text); }
    .object-browser-issue-badge-info { background: var(--risk-info); color: var(--risk-text); }

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
      cursor: pointer;
    }

    /* Line labels stay out of the overview and appear only for the lines a
       reader is looking at; the relation card carries the full mapping. */
    .canvas svg .edge-label,
    .canvas svg .edge-label-pill {
      opacity: 0;
      transition: opacity 0.15s;
      /* Invisible pills must not catch clicks meant for the canvas. */
      pointer-events: none;
    }

    .canvas svg .edge:focus-visible {
      outline: none;
    }

    .canvas svg .edge:focus-visible .edge-path {
      stroke: var(--selection-color);
      stroke-width: 2.4px;
    }

    .canvas svg .edge:hover .edge-label,
    .canvas svg .edge:hover .edge-label-pill,
    .canvas svg .edge:focus-visible .edge-label,
    .canvas svg .edge:focus-visible .edge-label-pill,
    .canvas svg .edge.hover-preview-edge .edge-label,
    .canvas svg .edge.hover-preview-edge .edge-label-pill,
    .canvas svg .edge.highlighted-neighbor .edge-label,
    .canvas svg .edge.highlighted-neighbor .edge-label-pill,
    .canvas svg .edge.selected-edge .edge-label,
    .canvas svg .edge.selected-edge .edge-label-pill {
      opacity: 1;
    }

    .type-filter-overlay {
      pointer-events: none;
    }

    .node.dimmed-by-filter .type-filter-overlay {
      opacity: 0.34;
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
      .search-panel {
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

      .minimap-shell,
      body:has(#detail-drawer:not([hidden])) .minimap-shell {
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
      --selection-color: {selection_color};
      --selection-soft: color-mix(in srgb, var(--selection-color) 62%, transparent);
      --selection-faint: color-mix(in srgb, var(--selection-color) 14%, transparent);
      --badge-pk-bg: {badge_pk_bg};
      --badge-pk-text: {badge_pk_text};
      --badge-fk-bg: {badge_fk_bg};
      --badge-fk-text: {badge_fk_text};
      --badge-ix-bg: {badge_ix_bg};
      --badge-ix-text: {badge_ix_text};
      --diff-added: {diff_added};
      --diff-removed: {diff_removed};
      --diff-modified: {diff_modified};
      --risk-breaking: {risk_breaking};
      --risk-caution: {risk_caution};
      --risk-warning: {risk_warning};
      --risk-info: {risk_info};
      --risk-text: {risk_text};
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
      background: var(--bg-color);
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

    .viewer-notice-stack {{
      position: fixed;
      top: 12px;
      right: 12px;
      display: flex;
      flex-direction: column;
      gap: 8px;
      z-index: 320;
      pointer-events: none;
    }}

    body:has(h1) .viewer-notice-stack {{
      top: 61px;
    }}

    .viewer-notice {{
      max-width: min(360px, calc(100vw - 24px));
      padding: 10px 12px;
      border-radius: 14px;
      border: 1px solid var(--panel-border);
      background: color-mix(in srgb, var(--panel-bg) 92%, transparent);
      box-shadow: var(--panel-shadow);
      color: var(--text-color);
      backdrop-filter: blur(16px);
      font-size: 13px;
      line-height: 1.4;
    }}

    .viewer-notice-warning {{
      border-color: color-mix(in srgb, var(--risk-warning) 44%, var(--panel-border));
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
{search_css}{filter_section_css}{group_panel_css}{highlight_css}{viewer_shell_css}",
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
        selection_color = selection_color,
        badge_pk_bg = badge_background(colors.badges.primary_key),
        badge_pk_text = colors.badges.primary_key.text,
        badge_fk_bg = badge_background(colors.badges.foreign_key),
        badge_fk_text = colors.badges.foreign_key.text,
        badge_ix_bg = badge_background(colors.badges.index),
        badge_ix_text = colors.badges.index.text,
        diff_added = colors.diff.added,
        diff_removed = colors.diff.removed,
        diff_modified = colors.diff.modified,
        risk_breaking = colors.risk.breaking,
        risk_caution = colors.risk.caution,
        risk_warning = colors.risk.warning,
        risk_info = colors.risk.info,
        risk_text = colors.risk.text,
        grid_dot = grid_dot,
        grid_line = grid_line,
        viewer_shell_css = viewer_shell_css,
    )
}
