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

  // ts/type_filter_state.ts
  function createTypeFilterState(tables) {
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
    return { typeSet, allTypes, selectedTypes, typeTableCounts };
  }
  function columnMatchesSelectedType(columnType, selectedType) {
    const column = columnType.trim().toLowerCase();
    const selected = selectedType.trim().toLowerCase();
    if (column === selected) return true;
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
  function visibleTypesForQuery(allTypes, query) {
    const needle = query.trim().toLowerCase();
    if (needle === "") return allTypes;
    return allTypes.filter((dataType) => dataType.toLowerCase().includes(needle));
  }
  function selectedTypeList(selectedTypes) {
    return Array.from(selectedTypes).sort(
      (left, right) => left.localeCompare(right, void 0, { sensitivity: "base" })
    );
  }
  function activeTypes(selectedTypes, allTypes, query) {
    if (selectedTypes.size > 0) return selectedTypeList(selectedTypes);
    return query.trim() === "" ? [] : visibleTypesForQuery(allTypes, query);
  }

  // ts/type_filter_dom.ts
  function rebuildFilterList(visibleTypes, selectedTypes, typeTableCounts, container, onChange) {
    container.innerHTML = "";
    for (const dataType of visibleTypes) {
      const row = document.createElement("label");
      row.className = "type-filter-item";
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.value = dataType;
      checkbox.checked = selectedTypes.has(dataType);
      checkbox.addEventListener("change", () => {
        onChange(dataType, checkbox.checked);
      });
      const label = document.createElement("span");
      label.textContent = dataType;
      const count = document.createElement("span");
      count.className = "type-filter-item-count";
      count.textContent = String(typeTableCounts.get(dataType) ?? 0);
      row.appendChild(checkbox);
      row.appendChild(label);
      row.appendChild(count);
      container.appendChild(row);
    }
  }
  function syncFilterChrome(hasActiveFilter, hasExplicitSelection, selected, activeTypeList, query, summaryEl, resetBar, resetCopy) {
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
  }

  // ts/type_filter.ts
  {
    const section = document.getElementById("type-filter-section");
    const listEl = document.getElementById("type-filter-list");
    const svgEl = document.querySelector(".canvas svg");
    if (section === null || listEl === null || svgEl === null) {
    } else {
      let rebuild = function() {
        const visible = visibleTypesForQuery(state.allTypes, getQuery());
        rebuildFilterList(
          visible,
          state.selectedTypes,
          state.typeTableCounts,
          listRoot,
          (dataType, checked) => {
            if (checked) {
              state.selectedTypes.add(dataType);
            } else {
              state.selectedTypes.delete(dataType);
            }
            applyTypeFilter();
          }
        );
      }, applyTypeFilter = function() {
        const nodes = svgRoot.querySelectorAll(".node");
        const effective = new Set(activeTypes(state.selectedTypes, state.allTypes, getQuery()));
        if (effective.size === 0) {
          nodes.forEach((node) => {
            node.classList.remove("dimmed-by-type-filter", "excluded-by-type-filter");
          });
        } else {
          nodes.forEach((node) => {
            const tableId = node.getAttribute("data-id") ?? node.getAttribute("data-table-id") ?? "";
            const table = tables.find((c) => c.id === tableId);
            const matches = table !== void 0 && tableMatchesAnySelectedType(table, effective);
            node.classList.toggle("dimmed-by-type-filter", !matches);
            node.classList.toggle("excluded-by-type-filter", !matches);
          });
        }
        const effectiveList = Array.from(effective);
        const selected = selectedTypeList(state.selectedTypes);
        const query = getQuery().trim();
        syncFilterChrome(
          effectiveList.length > 0,
          selected.length > 0,
          selected,
          effectiveList,
          query,
          summaryEl,
          resetBar,
          resetCopy
        );
        syncEdgeDimming(svgRoot);
        emitViewerEvent("relune:filters-changed", {
          active: effectiveList.length > 0,
          selectedTypes: selected.length > 0 ? selected : effectiveList,
          query: selected.length > 0 ? "" : query
        });
      }, clearSelection = function() {
        state.selectedTypes.clear();
        if (queryInput instanceof HTMLInputElement) {
          queryInput.value = "";
        }
        rebuild();
        applyTypeFilter();
      };
      rebuild2 = rebuild, applyTypeFilter2 = applyTypeFilter, clearSelection2 = clearSelection;
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
      const state = createTypeFilterState(tables);
      if (state.allTypes.length === 0) {
        section.setAttribute("hidden", "");
      } else {
        section.removeAttribute("hidden");
      }
      const getQuery = () => queryInput instanceof HTMLInputElement ? queryInput.value : "";
      runtime.filters = {
        reset() {
          clearSelection();
        },
        hasActiveFilters() {
          return activeTypes(state.selectedTypes, state.allTypes, getQuery()).length > 0;
        },
        setSelectedTypes(types) {
          state.selectedTypes.clear();
          for (const t of types) {
            if (state.typeSet.has(t)) {
              state.selectedTypes.add(t);
            }
          }
          rebuild();
          applyTypeFilter();
        },
        getSelectedTypes() {
          return selectedTypeList(state.selectedTypes);
        },
        getAvailableTypes() {
          return [...state.allTypes];
        }
      };
      queryInput?.addEventListener("input", () => {
        rebuild();
        applyTypeFilter();
      });
      clearBtn?.addEventListener("click", clearSelection);
      resetButton?.addEventListener("click", clearSelection);
      selectVisibleBtn?.addEventListener("click", () => {
        const query = getQuery();
        for (const dataType of visibleTypesForQuery(state.allTypes, query)) {
          state.selectedTypes.add(dataType);
        }
        rebuild();
        applyTypeFilter();
      });
      rebuild();
      applyTypeFilter();
    }
  }
  var rebuild2;
  var applyTypeFilter2;
  var clearSelection2;
})();
