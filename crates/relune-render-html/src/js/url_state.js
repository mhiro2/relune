"use strict";
(() => {
  // ts/metadata.ts
  var METADATA_ELEMENT_ID = "relune-metadata";
  function parseReluneMetadata() {
    const el = document.getElementById(METADATA_ELEMENT_ID);
    const raw = el?.textContent;
    if (raw == null || raw === "") {
      return null;
    }
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }
  function tableDisplayName(table) {
    return table.label || table.table_name || table.id;
  }

  // ts/viewer_api.ts
  var VIEWER_RUNTIME_KEY = /* @__PURE__ */ Symbol.for("relune.viewer.runtime");
  var VIEWER_READY_MODULES_KEY = /* @__PURE__ */ Symbol.for("relune.viewer.ready_modules");
  var VIEWER_WAITERS_KEY = /* @__PURE__ */ Symbol.for("relune.viewer.waiters");
  function getViewerRuntime() {
    const viewerWindow = window;
    if (viewerWindow[VIEWER_RUNTIME_KEY] === void 0) {
      viewerWindow[VIEWER_RUNTIME_KEY] = {};
    }
    return viewerWindow[VIEWER_RUNTIME_KEY];
  }
  function readyModules() {
    const viewerWindow = window;
    if (viewerWindow[VIEWER_READY_MODULES_KEY] === void 0) {
      viewerWindow[VIEWER_READY_MODULES_KEY] = /* @__PURE__ */ new Set();
    }
    return viewerWindow[VIEWER_READY_MODULES_KEY];
  }
  function runtimeWaiters() {
    const viewerWindow = window;
    if (viewerWindow[VIEWER_WAITERS_KEY] === void 0) {
      viewerWindow[VIEWER_WAITERS_KEY] = [];
    }
    return viewerWindow[VIEWER_WAITERS_KEY];
  }
  function waitForViewerModules(modules, callback) {
    const pending = new Set(modules);
    if (pending.size === 0 || Array.from(pending).every((module) => readyModules().has(module))) {
      callback();
      return;
    }
    runtimeWaiters().push({ modules: pending, callback });
  }

  // ts/url_state.ts
  {
    let readHash2 = function() {
      const raw = location.hash.replace(/^#/, "");
      return new URLSearchParams(raw);
    }, maxViewportPanMagnitude2 = function() {
      const bounds = runtime.viewport?.getDiagramBounds();
      if (bounds === null || bounds === void 0) {
        return MIN_VIEWPORT_PAN_LIMIT;
      }
      const extent = Math.max(Math.abs(bounds.x), Math.abs(bounds.y), bounds.width, bounds.height, 1);
      return Math.max(extent * MAX_VIEWPORT_SCALE * 4, MIN_VIEWPORT_PAN_LIMIT);
    }, hasValidViewportState2 = function(scale, panX, panY) {
      return Number.isFinite(scale) && Number.isFinite(panX) && Number.isFinite(panY) && scale >= MIN_VIEWPORT_SCALE && scale <= MAX_VIEWPORT_SCALE && Math.abs(panX) <= maxViewportPanMagnitude2() && Math.abs(panY) <= maxViewportPanMagnitude2();
    }, matchesMetadataSearch2 = function(table, query) {
      const normalizedQuery = query.trim().toLowerCase();
      if (normalizedQuery === "") {
        return false;
      }
      const searchable = [
        tableDisplayName(table),
        table.id,
        table.table_name,
        table.schema_name ?? "",
        table.kind,
        ...(table.columns ?? []).flatMap((column) => [column.name, column.data_type ?? ""])
      ].join("\n").toLowerCase();
      return searchable.includes(normalizedQuery);
    }, hasMetadataSearchMatch2 = function(query) {
      return tables.some((table) => matchesMetadataSearch2(table, query));
    }, scheduleWrite2 = function() {
      if (writeTimer !== null) {
        clearTimeout(writeTimer);
      }
      writeTimer = setTimeout(writeHash2, 300);
    }, scheduleDiscreteWrite2 = function() {
      pendingPush = true;
      scheduleWrite2();
    }, buildHashParams2 = function() {
      const params = new URLSearchParams();
      const query = runtime.search?.getQuery() ?? "";
      if (query !== "") {
        params.set(PARAM_SEARCH, query);
      }
      const selected = runtime.selection?.getSelected() ?? null;
      if (selected !== null) {
        params.set(PARAM_TABLE, selected);
      }
      const viewport = runtime.viewport?.getState();
      if (viewport !== null && viewport !== void 0) {
        params.set(PARAM_SCALE, viewport.scale.toFixed(4));
        params.set(PARAM_PAN_X, viewport.panX.toFixed(1));
        params.set(PARAM_PAN_Y, viewport.panY.toFixed(1));
      }
      for (const { param, facetId } of FACET_PARAMS) {
        const selection = runtime.filters?.getFacetSelection(facetId) ?? [];
        if (selection.length > 0) {
          params.set(param, selection.join(","));
        }
      }
      const filterMode = runtime.filters?.getMode();
      if (filterMode !== void 0 && filterMode !== "dim") {
        params.set(PARAM_FILTER_MODE, filterMode);
      }
      const hiddenGroups = runtime.groups?.getHiddenGroups() ?? [];
      if (hiddenGroups.length > 0) {
        params.set(PARAM_HIDDEN_GROUPS, hiddenGroups.join(","));
      }
      const collapsed = runtime.collapse?.getCollapsed() ?? [];
      if (collapsed.length > 0) {
        params.set(PARAM_COLLAPSED, collapsed.join(","));
      }
      if (runtime.minimap?.isHidden() === false) {
        params.set(PARAM_MINIMAP_VISIBLE, "1");
      }
      return params;
    }, writeHash2 = function() {
      const str = buildHashParams2().toString();
      const newHash = str === "" ? "" : `#${str}`;
      if (newHash !== location.hash && newHash !== "#") {
        const url = newHash || location.pathname + location.search;
        try {
          if (pendingPush && !restoringFromPopstate) {
            history.pushState(null, "", url);
          } else {
            history.replaceState(null, "", url);
          }
        } catch {
        }
      }
      pendingPush = false;
    }, restoreFromHash2 = function() {
      const params = readHash2();
      runtime.minimap?.setHidden(params.get(PARAM_MINIMAP_VISIBLE) !== "1", { silent: true });
      if (params.toString() === "") {
        return;
      }
      const s = params.get(PARAM_SCALE);
      const x = params.get(PARAM_PAN_X);
      const y = params.get(PARAM_PAN_Y);
      if (s !== null && x !== null && y !== null) {
        const scale = Number.parseFloat(s);
        const panX = Number.parseFloat(x);
        const panY = Number.parseFloat(y);
        if (hasValidViewportState2(scale, panX, panY)) {
          runtime.viewport?.setState(scale, panX, panY);
        }
      }
      const query = params.get(PARAM_SEARCH);
      if (query !== null && query !== "" && hasMetadataSearchMatch2(query)) {
        runtime.search?.setQuery(query);
      }
      const fmRaw = params.get(PARAM_FILTER_MODE);
      if (fmRaw === "hide" || fmRaw === "focus") {
        runtime.filters?.setMode(fmRaw);
      }
      for (const { param, facetId } of FACET_PARAMS) {
        const raw = params.get(param);
        if (raw !== null && raw !== "") {
          const values = raw.split(",").filter((v) => v !== "");
          if (values.length > 0) {
            runtime.filters?.setFacetSelection(facetId, values);
          }
        }
      }
      const hgRaw = params.get(PARAM_HIDDEN_GROUPS);
      if (hgRaw !== null && hgRaw !== "") {
        const hiddenGroups = hgRaw.split(",").filter((g) => g !== "");
        for (const groupId of hiddenGroups) {
          runtime.groups?.setVisibility(groupId, false);
        }
      }
      const collapsedRaw = params.get(PARAM_COLLAPSED);
      if (collapsedRaw !== null && collapsedRaw !== "") {
        const collapsed = collapsedRaw.split(",").filter((id) => id !== "" && tableIds.has(id));
        if (collapsed.length > 0) {
          runtime.collapse?.setCollapsed(collapsed);
        }
      }
      const table = params.get(PARAM_TABLE);
      if (table !== null && table !== "" && tableIds.has(table)) {
        runtime.selection?.select(table);
      }
    }, expectedViewerModules2 = function() {
      const modules = [];
      if (document.getElementById("zoom-fit") !== null) {
        modules.push("viewport");
      }
      if (document.getElementById("table-search") instanceof HTMLInputElement) {
        modules.push("search");
      }
      if (document.getElementById("filter-section") !== null) {
        modules.push("filters");
      }
      if (document.getElementById("detail-drawer") !== null) {
        modules.push("selection");
      }
      if ((metadata?.groups?.length ?? 0) > 0) {
        modules.push("groups");
      }
      if (document.getElementById("canvas")?.querySelector("svg") !== null) {
        modules.push("collapse");
      }
      if (document.getElementById("minimap-shell") !== null) {
        modules.push("minimap");
      }
      return modules;
    };
    readHash = readHash2, maxViewportPanMagnitude = maxViewportPanMagnitude2, hasValidViewportState = hasValidViewportState2, matchesMetadataSearch = matchesMetadataSearch2, hasMetadataSearchMatch = hasMetadataSearchMatch2, scheduleWrite = scheduleWrite2, scheduleDiscreteWrite = scheduleDiscreteWrite2, buildHashParams = buildHashParams2, writeHash = writeHash2, restoreFromHash = restoreFromHash2, expectedViewerModules = expectedViewerModules2;
    const runtime = getViewerRuntime();
    const metadata = parseReluneMetadata();
    const tables = metadata?.tables ?? [];
    const tableIds = new Set(tables.map((table) => table.id));
    const PARAM_SEARCH = "q";
    const PARAM_TABLE = "t";
    const PARAM_SCALE = "s";
    const PARAM_PAN_X = "x";
    const PARAM_PAN_Y = "y";
    const PARAM_FILTER_SCHEMA = "fs";
    const PARAM_FILTER_KIND = "fk";
    const PARAM_FILTER_TYPE = "ft";
    const PARAM_FILTER_SEVERITY = "fi";
    const PARAM_FILTER_DIFF = "fd";
    const PARAM_FILTER_MODE = "fm";
    const PARAM_HIDDEN_GROUPS = "hg";
    const PARAM_COLLAPSED = "c";
    const PARAM_MINIMAP_VISIBLE = "mv";
    const FACET_PARAMS = [
      { param: PARAM_FILTER_SCHEMA, facetId: "schema" },
      { param: PARAM_FILTER_KIND, facetId: "kind" },
      { param: PARAM_FILTER_TYPE, facetId: "columnType" },
      { param: PARAM_FILTER_SEVERITY, facetId: "severity" },
      { param: PARAM_FILTER_DIFF, facetId: "diffKind" }
    ];
    const MIN_VIEWPORT_SCALE = 0.1;
    const MAX_VIEWPORT_SCALE = 2;
    const MIN_VIEWPORT_PAN_LIMIT = 1e4;
    let writeTimer = null;
    let pendingPush = false;
    let restoringFromPopstate = false;
    document.addEventListener("relune:search-changed", scheduleDiscreteWrite2);
    document.addEventListener("relune:node-selected", scheduleDiscreteWrite2);
    document.addEventListener("relune:node-cleared", scheduleDiscreteWrite2);
    document.addEventListener("relune:viewport-changed", scheduleWrite2);
    document.addEventListener("relune:filters-changed", scheduleDiscreteWrite2);
    document.addEventListener("relune:groups-changed", scheduleDiscreteWrite2);
    document.addEventListener("relune:collapse-changed", scheduleDiscreteWrite2);
    document.addEventListener("relune:minimap-toggled", scheduleDiscreteWrite2);
    window.addEventListener("popstate", () => {
      restoringFromPopstate = true;
      restoreFromHash2();
      restoringFromPopstate = false;
    });
    waitForViewerModules(expectedViewerModules2(), restoreFromHash2);
  }
  var readHash;
  var maxViewportPanMagnitude;
  var hasValidViewportState;
  var matchesMetadataSearch;
  var hasMetadataSearchMatch;
  var scheduleWrite;
  var scheduleDiscreteWrite;
  var buildHashParams;
  var writeHash;
  var restoreFromHash;
  var expectedViewerModules;
})();
