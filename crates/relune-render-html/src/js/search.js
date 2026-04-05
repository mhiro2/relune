"use strict";
(() => {
  // ts/edge_filters.ts
  function nodeId(el) {
    return el.getAttribute("data-id") ?? el.getAttribute("data-table-id") ?? "";
  }
  function syncEdgeDimming(svgRoot) {
    const nodeById = /* @__PURE__ */ new Map();
    svgRoot.querySelectorAll(".node").forEach((node) => {
      const id = nodeId(node);
      if (id !== "") {
        nodeById.set(id, node);
      }
    });
    svgRoot.querySelectorAll(".edge").forEach((edge) => {
      const fromId = edge.getAttribute("data-from") ?? "";
      const toId = edge.getAttribute("data-to") ?? "";
      const fromEl = nodeById.get(fromId);
      const toEl = nodeById.get(toId);
      const endpointDimmed = fromEl?.classList.contains("dimmed-by-search") === true || toEl?.classList.contains("dimmed-by-search") === true || fromEl?.classList.contains("dimmed-by-type-filter") === true || toEl?.classList.contains("dimmed-by-type-filter") === true;
      if (endpointDimmed) {
        edge.classList.add("dimmed-by-edge-filter");
      } else {
        edge.classList.remove("dimmed-by-edge-filter");
      }
    });
  }

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
  function markViewerModuleReady(module) {
    readyModules().add(module);
    flushViewerWaiters();
  }
  function flushViewerWaiters() {
    const ready = readyModules();
    const remaining = [];
    for (const waiter of runtimeWaiters()) {
      if (Array.from(waiter.modules).every((module) => ready.has(module))) {
        waiter.callback();
      } else {
        remaining.push(waiter);
      }
    }
    const viewerWindow = window;
    viewerWindow[VIEWER_WAITERS_KEY] = remaining;
  }
  function emitViewerEvent(name, detail) {
    document.dispatchEvent(new CustomEvent(name, { detail }));
  }

  // ts/search_actions.ts
  function computeSearchMatches(nodes, tableNames, query) {
    const q = query.toLowerCase().trim();
    const results = [];
    let matchCount = 0;
    nodes.forEach((node) => {
      if (q === "") {
        results.push({ node, matches: true });
        matchCount += 1;
        return;
      }
      const tableId = node.getAttribute("data-id") ?? node.getAttribute("data-table-id") ?? "";
      const tableName = tableNames[tableId] ?? tableId;
      const nodeText = node.textContent?.toLowerCase() ?? "";
      const matches = tableName.toLowerCase().includes(q) || tableId.toLowerCase().includes(q) || nodeText.includes(q);
      results.push({ node, matches });
      if (matches) matchCount += 1;
    });
    return { results, matchCount, total: nodes.length };
  }

  // ts/search.ts
  {
    const searchInput = document.getElementById("table-search");
    const searchClear = document.getElementById("search-clear");
    const searchResults = document.getElementById("search-results");
    const svgRoot = document.querySelector(".canvas svg");
    if (searchInput instanceof HTMLInputElement && svgRoot) {
      const runtime = getViewerRuntime();
      const metadata = parseReluneMetadata();
      const tables = metadata?.tables ?? [];
      const tableNames = {};
      for (const table of tables) {
        tableNames[table.id] = tableDisplayName(table);
      }
      const performSearch = (query) => {
        const q = query.toLowerCase().trim();
        const nodes = svgRoot.querySelectorAll(".node");
        if (q === "") {
          nodes.forEach((node) => {
            node.classList.remove("dimmed-by-search", "highlighted-by-search");
          });
          syncEdgeDimming(svgRoot);
          searchClear?.classList.remove("visible");
          searchResults?.classList.remove("visible");
          emitViewerEvent("relune:search-changed", {
            active: false,
            query: "",
            matches: nodes.length,
            total: nodes.length
          });
          return;
        }
        searchClear?.classList.add("visible");
        const { results, matchCount, total } = computeSearchMatches(nodes, tableNames, query);
        for (const { node, matches } of results) {
          if (matches) {
            node.classList.remove("dimmed-by-search");
            node.classList.add("highlighted-by-search");
          } else {
            node.classList.remove("highlighted-by-search");
            node.classList.add("dimmed-by-search");
          }
        }
        syncEdgeDimming(svgRoot);
        if (searchResults) {
          searchResults.textContent = `${matchCount} of ${total} objects`;
          searchResults.classList.add("visible");
        }
        emitViewerEvent("relune:search-changed", {
          active: true,
          query,
          matches: matchCount,
          total
        });
      };
      runtime.search = {
        focus() {
          searchInput.focus();
        },
        clear() {
          searchInput.value = "";
          performSearch("");
        },
        isActive() {
          return searchInput.value.trim() !== "";
        },
        setQuery(query) {
          searchInput.value = query;
          performSearch(query);
        },
        getQuery() {
          return searchInput.value;
        }
      };
      markViewerModuleReady("search");
      let debounceTimer = null;
      const debouncedSearch = (query) => {
        if (debounceTimer !== null) {
          clearTimeout(debounceTimer);
        }
        debounceTimer = setTimeout(() => {
          performSearch(query);
        }, 150);
      };
      searchInput.addEventListener("input", (event) => {
        const target = event.target;
        if (target instanceof HTMLInputElement) {
          debouncedSearch(target.value);
        }
      });
      searchClear?.addEventListener("click", () => {
        runtime.search?.clear();
        searchInput.focus();
      });
      searchInput.addEventListener("keydown", (event) => {
        if (event.key === "Escape") {
          runtime.search?.clear();
          searchInput.blur();
        }
      });
    }
  }
})();
