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

  // ts/type_filter.ts
  function columnMatchesSelectedType(columnType, selectedType) {
    const column = columnType.trim().toLowerCase();
    const selected = selectedType.trim().toLowerCase();
    if (column === selected) {
      return true;
    }
    const base = (raw) => {
      const index = raw.indexOf("(");
      return (index === -1 ? raw : raw.slice(0, index)).trim();
    };
    const baseColumn = base(column);
    const baseSelected = base(selected);
    return baseColumn === baseSelected || column.includes(selected) || selected.includes(column);
  }
  function tableMatchesAnySelectedType(table, selectedTypes) {
    return (table.columns ?? []).some(
      (column) => Array.from(selectedTypes).some(
        (selectedType) => columnMatchesSelectedType(column.data_type ?? "", selectedType)
      )
    );
  }
  {
    const section = document.getElementById("type-filter-section");
    const listEl = document.getElementById("type-filter-list");
    const svgEl = document.querySelector(".canvas svg");
    if (section === null || listEl === null || svgEl === null) {
    } else {
      let visibleTypesForQuery = function(query) {
        const needle = query.trim().toLowerCase();
        if (needle === "") {
          return allTypes;
        }
        return allTypes.filter((dataType) => dataType.toLowerCase().includes(needle));
      }, selectedTypeList = function() {
        return Array.from(selectedTypes).sort(
          (left, right) => left.localeCompare(right, void 0, { sensitivity: "base" })
        );
      }, activeTypes = function() {
        if (selectedTypes.size > 0) {
          return selectedTypeList();
        }
        const query = queryInput instanceof HTMLInputElement ? queryInput.value : "";
        return query.trim() === "" ? [] : visibleTypesForQuery(query);
      }, syncFilterChrome = function(activeTypeList) {
        const selected = selectedTypeList();
        const query = queryInput instanceof HTMLInputElement ? queryInput.value.trim() : "";
        const hasExplicitSelection = selected.length > 0;
        const hasActiveFilter = activeTypeList.length > 0;
        if (summaryEl) {
          if (!hasActiveFilter) {
            summaryEl.textContent = "";
            summaryEl.classList.remove("visible");
          } else {
            summaryEl.textContent = hasExplicitSelection ? `${selected.length} type(s) selected across the schema` : `${activeTypeList.length} matching type(s) for "${query}"`;
            summaryEl.classList.add("visible");
          }
        }
        if (resetBar && resetCopy) {
          resetBar.toggleAttribute("hidden", !hasActiveFilter);
          if (hasExplicitSelection) {
            const preview = selected.slice(0, 3).join(", ");
            const suffix = selected.length > 3 ? ` +${selected.length - 3} more` : "";
            resetCopy.textContent = `${selected.length} type filter(s): ${preview}${suffix}`;
          } else if (hasActiveFilter) {
            const preview = activeTypeList.slice(0, 3).join(", ");
            const suffix = activeTypeList.length > 3 ? ` +${activeTypeList.length - 3} more` : "";
            resetCopy.textContent = `Type query "${query}": ${preview}${suffix}`;
          } else {
            resetCopy.textContent = "";
          }
        }
        emitViewerEvent("relune:filters-changed", {
          active: hasActiveFilter,
          selectedTypes: hasExplicitSelection ? selected : activeTypeList,
          query: hasExplicitSelection ? "" : query
        });
      }, rebuildList = function() {
        listRoot.innerHTML = "";
        const query = queryInput instanceof HTMLInputElement ? queryInput.value : "";
        const visibleTypes = visibleTypesForQuery(query);
        for (const dataType of visibleTypes) {
          const row = document.createElement("label");
          row.className = "type-filter-item";
          const checkbox = document.createElement("input");
          checkbox.type = "checkbox";
          checkbox.value = dataType;
          checkbox.checked = selectedTypes.has(dataType);
          checkbox.addEventListener("change", () => {
            if (checkbox.checked) {
              selectedTypes.add(dataType);
            } else {
              selectedTypes.delete(dataType);
            }
            applyTypeFilter();
          });
          const label = document.createElement("span");
          label.textContent = dataType;
          const count = document.createElement("span");
          count.className = "type-filter-item-count";
          count.textContent = String(typeTableCounts.get(dataType) ?? 0);
          row.appendChild(checkbox);
          row.appendChild(label);
          row.appendChild(count);
          listRoot.appendChild(row);
        }
      }, applyTypeFilter = function() {
        const nodes = svgRoot.querySelectorAll(".node");
        const effectiveTypes = new Set(activeTypes());
        if (effectiveTypes.size === 0) {
          nodes.forEach((node) => {
            node.classList.remove("dimmed-by-type-filter", "excluded-by-type-filter");
          });
        } else {
          nodes.forEach((node) => {
            const tableId = node.getAttribute("data-id") ?? node.getAttribute("data-table-id") ?? "";
            const table = tables.find((candidate) => candidate.id === tableId);
            const matches = table !== void 0 && tableMatchesAnySelectedType(table, effectiveTypes);
            node.classList.toggle("dimmed-by-type-filter", !matches);
            node.classList.toggle("excluded-by-type-filter", !matches);
          });
        }
        syncFilterChrome(Array.from(effectiveTypes));
        syncEdgeDimming(svgRoot);
      }, clearSelection = function() {
        selectedTypes.clear();
        if (queryInput instanceof HTMLInputElement) {
          queryInput.value = "";
        }
        rebuildList();
        applyTypeFilter();
      };
      visibleTypesForQuery2 = visibleTypesForQuery, selectedTypeList2 = selectedTypeList, activeTypes2 = activeTypes, syncFilterChrome2 = syncFilterChrome, rebuildList2 = rebuildList, applyTypeFilter2 = applyTypeFilter, clearSelection2 = clearSelection;
      const runtime = getViewerRuntime();
      const listRoot = listEl;
      const svgRoot = svgEl;
      const summaryEl = document.getElementById("type-filter-summary");
      const clearBtn = document.getElementById("type-filter-clear");
      const selectVisibleBtn = document.getElementById("type-filter-select-visible");
      const queryInput = document.getElementById("type-filter-query");
      const resetBar = document.getElementById("filter-reset-bar");
      const resetCopy = document.getElementById("filter-reset-copy");
      const resetButton = document.getElementById("filter-reset-button");
      const metadata = parseReluneMetadata();
      const tables = metadata?.tables ?? [];
      const typeSet = /* @__PURE__ */ new Set();
      const selectedTypes = /* @__PURE__ */ new Set();
      const typeTableCounts = /* @__PURE__ */ new Map();
      for (const table of tables) {
        const tableTypes = /* @__PURE__ */ new Set();
        for (const column of table.columns ?? []) {
          const dataType = (column.data_type ?? "").trim();
          if (dataType !== "") {
            typeSet.add(dataType);
            tableTypes.add(dataType);
          }
        }
        tableTypes.forEach((dataType) => {
          typeTableCounts.set(dataType, (typeTableCounts.get(dataType) ?? 0) + 1);
        });
      }
      const allTypes = Array.from(typeSet).sort(
        (left, right) => left.localeCompare(right, void 0, { sensitivity: "base" })
      );
      if (allTypes.length === 0) {
        section.setAttribute("hidden", "");
      } else {
        section.removeAttribute("hidden");
      }
      runtime.filters = {
        reset() {
          clearSelection();
        },
        hasActiveFilters() {
          return activeTypes().length > 0;
        },
        setSelectedTypes(types) {
          selectedTypes.clear();
          for (const t of types) {
            if (typeSet.has(t)) {
              selectedTypes.add(t);
            }
          }
          rebuildList();
          applyTypeFilter();
        },
        getSelectedTypes() {
          return selectedTypeList();
        }
      };
      queryInput?.addEventListener("input", () => {
        rebuildList();
        applyTypeFilter();
      });
      clearBtn?.addEventListener("click", clearSelection);
      resetButton?.addEventListener("click", clearSelection);
      selectVisibleBtn?.addEventListener("click", () => {
        const query = queryInput instanceof HTMLInputElement ? queryInput.value : "";
        for (const dataType of visibleTypesForQuery(query)) {
          selectedTypes.add(dataType);
        }
        rebuildList();
        applyTypeFilter();
      });
      rebuildList();
      applyTypeFilter();
    }
  }
  var visibleTypesForQuery2;
  var selectedTypeList2;
  var activeTypes2;
  var syncFilterChrome2;
  var rebuildList2;
  var applyTypeFilter2;
  var clearSelection2;
})();
