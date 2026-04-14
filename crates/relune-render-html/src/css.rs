//! CSS generation for the HTML viewer.

use crate::options::Theme;
use relune_render_theme::get_colors;

/// Build CSS styles based on theme and options.
#[allow(clippy::too_many_lines)]
#[allow(clippy::fn_params_excessive_bools)]
pub(crate) fn build_css(
    theme: Theme,
    enable_group_toggles: bool,
    enable_search: bool,
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

    .filter-section {
      border-top: 1px solid var(--panel-border);
    }

    .filter-section-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      padding: 10px 14px 6px;
      font-size: 12px;
      font-weight: 600;
      opacity: 0.9;
    }

    .filter-section-header > span {
      flex-shrink: 0;
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
      background-color: var(--accent-soft);
    }

    .filter-mode-button.active {
      opacity: 1;
      background-color: var(--accent-soft);
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
      border-color: var(--accent-color);
      background-color: var(--accent-soft);
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
      background: var(--accent-soft);
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
      background: var(--accent-color);
      color: white;
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
      background: var(--accent-color);
      color: white;
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
      border-color: var(--accent-color);
      background-color: var(--accent-soft);
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
      background-color: var(--accent-soft);
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
      background: var(--accent-soft);
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
      color: var(--accent-color);
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

    .hover-popover-badge-error { background: rgba(248, 113, 113, 0.22); color: #f87171; }
    .hover-popover-badge-warning { background: rgba(251, 191, 36, 0.22); color: #fbbf24; }
    .hover-popover-badge-info { background: rgba(56, 189, 248, 0.22); color: #38bdf8; }
    .hover-popover-badge-hint { background: rgba(148, 163, 184, 0.18); color: #94a3b8; }

    .node.hover-preview-node,
    .node.hover-preview-neighbor {
      opacity: 1 !important;
      transition: opacity 0.18s, filter 0.18s;
    }

    .node.hover-preview-node {
      filter: drop-shadow(0 0 8px rgba(245, 158, 11, 0.28));
    }

    .node.hover-preview-node .table-body {
      stroke: rgba(245, 158, 11, 0.72);
      stroke-width: 2.05px;
    }

    .node.hover-preview-neighbor {
      filter: drop-shadow(0 0 8px rgba(245, 158, 11, 0.18));
    }

    .node.hover-preview-neighbor .table-body {
      stroke: rgba(245, 158, 11, 0.6);
      stroke-width: 1.9px;
    }

    .node.hover-preview-neighbor.hover-inbound {
      filter: drop-shadow(0 0 8px rgba(45, 212, 191, 0.18));
    }

    .node.hover-preview-neighbor.hover-inbound .table-body {
      stroke: rgba(45, 212, 191, 0.68);
    }

    .node.hover-preview-neighbor.hover-outbound {
      filter: drop-shadow(0 0 8px rgba(251, 191, 36, 0.2));
    }

    .node.hover-preview-neighbor.hover-outbound .table-body {
      stroke: rgba(251, 191, 36, 0.72);
    }

    .edge.hover-preview-edge {
      opacity: 0.92 !important;
      stroke-width: 2.15px;
      transition: opacity 0.18s, stroke-width 0.18s;
    }

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
    }

    .edge.highlighted-neighbor .edge-glow-path {
      opacity: 0.92;
    }

    .edge.highlighted-neighbor .edge-particles {
      opacity: 0.92;
    }

    .edge.hover-preview-edge .edge-glow-path {
      opacity: 0.76;
    }

    .edge.hover-preview-edge .edge-particles {
      opacity: 0.72;
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

    button.detail-relation {
      width: 100%;
      text-align: left;
      font: inherit;
      color: inherit;
      cursor: pointer;
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

    .detail-column-pill-fk {
      background: rgba(99, 102, 241, 0.2);
      color: #818cf8;
      opacity: 1;
    }

    .detail-column-pill-ix {
      background: rgba(20, 184, 166, 0.2);
      color: #2dd4bf;
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
      cursor: pointer;
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
      border-color: color-mix(in srgb, var(--accent-color) 44%, var(--panel-border));
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
        accent_color = accent_color,
        accent_soft = accent_soft,
        viewer_bg = viewer_bg,
        grid_dot = grid_dot,
        grid_line = grid_line,
        viewer_shell_css = viewer_shell_css,
    )
}
