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
    let readHash = function() {
      const raw = location.hash.replace(/^#/, "");
      return new URLSearchParams(raw);
    }, parseAllowedTypes = function(typesRaw, allowedTypes) {
      const selected = /* @__PURE__ */ new Set();
      for (const type of typesRaw.split(",")) {
        const candidate = type.trim();
        if (candidate !== "" && allowedTypes.has(candidate)) {
          selected.add(candidate);
        }
      }
      return [...selected];
    }, maxViewportPanMagnitude = function() {
      const bounds = runtime.viewport?.getDiagramBounds();
      if (bounds === null || bounds === void 0) {
        return MIN_VIEWPORT_PAN_LIMIT;
      }
      const extent = Math.max(Math.abs(bounds.x), Math.abs(bounds.y), bounds.width, bounds.height, 1);
      return Math.max(extent * MAX_VIEWPORT_SCALE * 4, MIN_VIEWPORT_PAN_LIMIT);
    }, hasValidViewportState = function(scale, panX, panY) {
      return Number.isFinite(scale) && Number.isFinite(panX) && Number.isFinite(panY) && scale >= MIN_VIEWPORT_SCALE && scale <= MAX_VIEWPORT_SCALE && Math.abs(panX) <= maxViewportPanMagnitude() && Math.abs(panY) <= maxViewportPanMagnitude();
    }, matchesMetadataSearch = function(table, query) {
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
    }, hasMetadataSearchMatch = function(query) {
      return tables.some((table) => matchesMetadataSearch(table, query));
    }, scheduleWrite = function() {
      if (writeTimer !== null) {
        clearTimeout(writeTimer);
      }
      writeTimer = setTimeout(writeHash, 300);
    }, scheduleDiscreteWrite = function() {
      pendingPush = true;
      scheduleWrite();
    }, buildHashParams = function() {
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
      const types = runtime.filters?.getSelectedTypes() ?? [];
      if (types.length > 0) {
        params.set(PARAM_TYPES, types.join(","));
      }
      const hiddenGroups = runtime.groups?.getHiddenGroups() ?? [];
      if (hiddenGroups.length > 0) {
        params.set(PARAM_HIDDEN_GROUPS, hiddenGroups.join(","));
      }
      const collapsed = runtime.collapse?.getCollapsed() ?? [];
      if (collapsed.length > 0) {
        params.set(PARAM_COLLAPSED, collapsed.join(","));
      }
      return params;
    }, writeHash = function() {
      const str = buildHashParams().toString();
      const newHash = str === "" ? "" : `#${str}`;
      if (newHash !== location.hash && newHash !== "#") {
        const url = newHash || location.pathname + location.search;
        if (pendingPush && !restoringFromPopstate) {
          history.pushState(null, "", url);
        } else {
          history.replaceState(null, "", url);
        }
      }
      pendingPush = false;
    }, restoreFromHash = function() {
      const params = readHash();
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
        if (hasValidViewportState(scale, panX, panY)) {
          runtime.viewport?.setState(scale, panX, panY);
        }
      }
      const query = params.get(PARAM_SEARCH);
      if (query !== null && query !== "" && hasMetadataSearchMatch(query)) {
        runtime.search?.setQuery(query);
      }
      const typesRaw = params.get(PARAM_TYPES);
      if (typesRaw !== null && typesRaw !== "") {
        const allowedTypes = new Set(runtime.filters?.getAvailableTypes() ?? []);
        const types = parseAllowedTypes(typesRaw, allowedTypes);
        if (types.length > 0) {
          runtime.filters?.setSelectedTypes(types);
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
    }, expectedViewerModules = function() {
      const modules = [];
      if (document.getElementById("zoom-fit") !== null) {
        modules.push("viewport");
      }
      if (document.getElementById("table-search") instanceof HTMLInputElement) {
        modules.push("search");
      }
      if (document.getElementById("type-filter-section") !== null) {
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
      return modules;
    };
    readHash2 = readHash, parseAllowedTypes2 = parseAllowedTypes, maxViewportPanMagnitude2 = maxViewportPanMagnitude, hasValidViewportState2 = hasValidViewportState, matchesMetadataSearch2 = matchesMetadataSearch, hasMetadataSearchMatch2 = hasMetadataSearchMatch, scheduleWrite2 = scheduleWrite, scheduleDiscreteWrite2 = scheduleDiscreteWrite, buildHashParams2 = buildHashParams, writeHash2 = writeHash, restoreFromHash2 = restoreFromHash, expectedViewerModules2 = expectedViewerModules;
    const runtime = getViewerRuntime();
    const metadata = parseReluneMetadata();
    const tables = metadata?.tables ?? [];
    const tableIds = new Set(tables.map((table) => table.id));
    const PARAM_SEARCH = "q";
    const PARAM_TABLE = "t";
    const PARAM_SCALE = "s";
    const PARAM_PAN_X = "x";
    const PARAM_PAN_Y = "y";
    const PARAM_TYPES = "types";
    const PARAM_HIDDEN_GROUPS = "hg";
    const PARAM_COLLAPSED = "c";
    const MIN_VIEWPORT_SCALE = 0.1;
    const MAX_VIEWPORT_SCALE = 2;
    const MIN_VIEWPORT_PAN_LIMIT = 1e4;
    let writeTimer = null;
    let pendingPush = false;
    let restoringFromPopstate = false;
    document.addEventListener("relune:search-changed", scheduleDiscreteWrite);
    document.addEventListener("relune:node-selected", scheduleDiscreteWrite);
    document.addEventListener("relune:node-cleared", scheduleDiscreteWrite);
    document.addEventListener("relune:viewport-changed", scheduleWrite);
    document.addEventListener("relune:filters-changed", scheduleDiscreteWrite);
    document.addEventListener("relune:groups-changed", scheduleDiscreteWrite);
    document.addEventListener("relune:collapse-changed", scheduleDiscreteWrite);
    window.addEventListener("popstate", () => {
      restoringFromPopstate = true;
      restoreFromHash();
      restoringFromPopstate = false;
    });
    waitForViewerModules(expectedViewerModules(), restoreFromHash);
  }
  var readHash2;
  var parseAllowedTypes2;
  var maxViewportPanMagnitude2;
  var hasValidViewportState2;
  var matchesMetadataSearch2;
  var hasMetadataSearchMatch2;
  var scheduleWrite2;
  var scheduleDiscreteWrite2;
  var buildHashParams2;
  var writeHash2;
  var restoreFromHash2;
  var expectedViewerModules2;
})();
