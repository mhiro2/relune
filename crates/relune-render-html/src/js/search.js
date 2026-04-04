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
  function getViewerRuntime() {
    if (window.reluneViewer === void 0) {
      window.reluneViewer = {};
    }
    return window.reluneViewer;
  }
  function emitViewerEvent(name, detail) {
    document.dispatchEvent(new CustomEvent(name, { detail }));
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
        let matchCount = 0;
        const totalCount = nodes.length;
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
            matches: totalCount,
            total: totalCount
          });
          return;
        }
        searchClear?.classList.add("visible");
        nodes.forEach((node) => {
          const tableId = node.getAttribute("data-id") ?? node.getAttribute("data-table-id") ?? "";
          const tableName = tableNames[tableId] ?? tableId;
          const nodeText = node.textContent?.toLowerCase() ?? "";
          const matches = tableName.toLowerCase().includes(q) || tableId.toLowerCase().includes(q) || nodeText.includes(q);
          if (matches) {
            node.classList.remove("dimmed-by-search");
            node.classList.add("highlighted-by-search");
            matchCount += 1;
          } else {
            node.classList.remove("highlighted-by-search");
            node.classList.add("dimmed-by-search");
          }
        });
        syncEdgeDimming(svgRoot);
        if (searchResults) {
          searchResults.textContent = `${matchCount} of ${totalCount} objects`;
          searchResults.classList.add("visible");
        }
        emitViewerEvent("relune:search-changed", {
          active: true,
          query,
          matches: matchCount,
          total: totalCount
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
