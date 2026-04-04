"use strict";
(() => {
  // ts/viewer_api.ts
  function getViewerRuntime() {
    if (window.reluneViewer === void 0) {
      window.reluneViewer = {};
    }
    return window.reluneViewer;
  }

  // ts/url_state.ts
  {
    let readHash = function() {
      const raw = location.hash.replace(/^#/, "");
      return new URLSearchParams(raw);
    }, scheduleWrite = function() {
      if (writeTimer !== null) {
        clearTimeout(writeTimer);
      }
      writeTimer = setTimeout(writeHash, 300);
    }, writeHash = function() {
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
      const str = params.toString();
      const newHash = str === "" ? "" : `#${str}`;
      if (newHash !== location.hash && newHash !== "#") {
        history.replaceState(null, "", newHash || location.pathname + location.search);
      }
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
        if (Number.isFinite(scale) && Number.isFinite(panX) && Number.isFinite(panY)) {
          runtime.viewport?.setState(scale, panX, panY);
        }
      }
      const query = params.get(PARAM_SEARCH);
      if (query !== null && query !== "") {
        runtime.search?.setQuery(query);
      }
      const typesRaw = params.get(PARAM_TYPES);
      if (typesRaw !== null && typesRaw !== "") {
        const types = typesRaw.split(",").filter((t) => t !== "");
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
      const table = params.get(PARAM_TABLE);
      if (table !== null && table !== "") {
        runtime.selection?.select(table);
      }
    };
    readHash2 = readHash, scheduleWrite2 = scheduleWrite, writeHash2 = writeHash, restoreFromHash2 = restoreFromHash;
    const runtime = getViewerRuntime();
    const PARAM_SEARCH = "q";
    const PARAM_TABLE = "t";
    const PARAM_SCALE = "s";
    const PARAM_PAN_X = "x";
    const PARAM_PAN_Y = "y";
    const PARAM_TYPES = "types";
    const PARAM_HIDDEN_GROUPS = "hg";
    let writeTimer = null;
    document.addEventListener("relune:search-changed", scheduleWrite);
    document.addEventListener("relune:node-selected", scheduleWrite);
    document.addEventListener("relune:node-cleared", scheduleWrite);
    document.addEventListener("relune:viewport-changed", scheduleWrite);
    document.addEventListener("relune:filters-changed", scheduleWrite);
    document.addEventListener("relune:groups-changed", scheduleWrite);
    requestAnimationFrame(() => {
      restoreFromHash();
    });
  }
  var readHash2;
  var scheduleWrite2;
  var writeHash2;
  var restoreFromHash2;
})();
