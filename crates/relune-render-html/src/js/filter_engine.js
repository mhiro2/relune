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
      const endpointHidden = fromEl?.classList.contains("hidden-by-filter") === true || toEl?.classList.contains("hidden-by-filter") === true;
      const endpointDimmed = fromEl?.classList.contains("dimmed-by-search") === true || toEl?.classList.contains("dimmed-by-search") === true || fromEl?.classList.contains("dimmed-by-filter") === true || toEl?.classList.contains("dimmed-by-filter") === true;
      edge.classList.toggle("hidden-by-filter", endpointHidden);
      edge.classList.toggle("dimmed-by-edge-filter", !endpointHidden && endpointDimmed);
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

  // ts/filter_engine_state.ts
  var DEFAULT_SCHEMA = "(default)";
  function extractSchemaValues(table) {
    return [table.schema_name ?? DEFAULT_SCHEMA];
  }
  function extractKindValues(table) {
    return [table.kind];
  }
  function extractColumnTypeValues(table) {
    const types = /* @__PURE__ */ new Set();
    for (const col of table.columns ?? []) {
      const dt = (col.data_type ?? "").trim();
      if (dt !== "") {
        types.add(dt);
      }
    }
    return [...types];
  }
  function extractSeverityValues(table) {
    const issues = table.issues ?? [];
    if (issues.length === 0) return ["none"];
    const severities = /* @__PURE__ */ new Set();
    for (const issue of issues) {
      severities.add(issue.severity);
    }
    return [...severities];
  }
  function extractDiffKindValues(table) {
    return [table.diff_kind ?? "unchanged"];
  }
  function buildFacet(id, label, tables, extractValues, hasSearch) {
    const valueSet = /* @__PURE__ */ new Set();
    const counts = /* @__PURE__ */ new Map();
    for (const table of tables) {
      const tableValues = new Set(extractValues(table));
      for (const v of tableValues) {
        valueSet.add(v);
        counts.set(v, (counts.get(v) ?? 0) + 1);
      }
    }
    const allValues = [...valueSet].sort(
      (a, b) => a.localeCompare(b, void 0, { sensitivity: "base" })
    );
    return {
      id,
      label,
      allValues,
      selectedValues: /* @__PURE__ */ new Set(),
      counts,
      extractValues,
      hasSearch
    };
  }
  function createFilterEngineState(tables) {
    const facets = /* @__PURE__ */ new Map();
    const schemaFacet = buildFacet("schema", "Schema", tables, extractSchemaValues);
    if (schemaFacet.allValues.length > 1) {
      facets.set("schema", schemaFacet);
    }
    const kindFacet = buildFacet("kind", "Kind", tables, extractKindValues);
    if (kindFacet.allValues.length > 1) {
      facets.set("kind", kindFacet);
    }
    const typeFacet = buildFacet("columnType", "Column Type", tables, extractColumnTypeValues, true);
    if (typeFacet.allValues.length > 0) {
      facets.set("columnType", typeFacet);
    }
    const severityFacet = buildFacet("severity", "Issues", tables, extractSeverityValues);
    if (severityFacet.allValues.length > 1) {
      facets.set("severity", severityFacet);
    }
    const diffFacet = buildFacet("diffKind", "Changes", tables, extractDiffKindValues);
    const hasDiffData = tables.some((t) => t.diff_kind != null);
    if (hasDiffData) {
      facets.set("diffKind", diffFacet);
    }
    return { facets, mode: "dim" };
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
    const startsWithTypeToken = (value, token) => value === token || value.startsWith(`${token}(`) || value.startsWith(`${token} `) || value.startsWith(`${token}[`) || value.startsWith(`${token},`);
    return baseColumn === baseSelected || startsWithTypeToken(column, selected) || startsWithTypeToken(selected, column);
  }
  function tableMatchesFacet(table, facet) {
    if (facet.selectedValues.size === 0) return true;
    if (facet.id === "columnType") {
      return (table.columns ?? []).some(
        (col) => [...facet.selectedValues].some((sel) => columnMatchesSelectedType(col.data_type ?? "", sel))
      );
    }
    const tableValues = facet.extractValues(table);
    return tableValues.some((v) => facet.selectedValues.has(v));
  }
  function tableMatchesAllFacets(table, state) {
    for (const facet of state.facets.values()) {
      if (!tableMatchesFacet(table, facet)) return false;
    }
    return true;
  }
  function hasActiveFilters(state) {
    for (const facet of state.facets.values()) {
      if (facet.selectedValues.size > 0) return true;
    }
    return false;
  }
  function activeFilterSummary(state) {
    const items = [];
    for (const facet of state.facets.values()) {
      if (facet.selectedValues.size > 0) {
        items.push({
          facetId: facet.id,
          label: facet.label,
          count: facet.selectedValues.size,
          values: [...facet.selectedValues].sort()
        });
      }
    }
    return items;
  }
  function visibleTypesForQuery(allTypes, query) {
    const needle = query.trim().toLowerCase();
    if (needle === "") return allTypes;
    return allTypes.filter((t) => t.toLowerCase().includes(needle));
  }

  // ts/filter_engine_dom.ts
  function buildFilterModeSwitcher(currentMode, onChange) {
    const wrapper = document.createElement("div");
    wrapper.className = "filter-mode-switcher";
    const modes = [
      { id: "dim", label: "Dim", title: "Reduce opacity of non-matching objects" },
      { id: "hide", label: "Hide", title: "Hide non-matching objects" },
      { id: "focus", label: "Focus", title: "Hide non-matching objects and zoom to fit" }
    ];
    for (const { id, label, title } of modes) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "filter-mode-button";
      btn.classList.toggle("active", id === currentMode);
      btn.textContent = label;
      btn.title = title;
      btn.dataset.mode = id;
      btn.addEventListener("click", () => {
        onChange(id);
      });
      wrapper.appendChild(btn);
    }
    return wrapper;
  }
  function syncModeSwitcher(container, activeMode) {
    for (const btn of container.querySelectorAll(".filter-mode-button")) {
      const mode = btn.dataset.mode;
      btn.classList.toggle("active", mode === activeMode);
    }
  }
  function buildFacetSection(facet, onChange, onSearchInput) {
    const details = document.createElement("details");
    details.className = "filter-facet";
    details.dataset.facetId = facet.id;
    const summary = document.createElement("summary");
    summary.className = "filter-facet-summary";
    const label = document.createElement("span");
    label.className = "filter-facet-label";
    label.textContent = facet.label;
    const badge = document.createElement("span");
    badge.className = "filter-facet-badge";
    badge.hidden = true;
    const actions = document.createElement("span");
    actions.className = "filter-facet-actions";
    const allBtn = document.createElement("button");
    allBtn.type = "button";
    allBtn.className = "filter-facet-action";
    allBtn.textContent = "Select All";
    allBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      const listEl = details.querySelector(".filter-facet-list");
      if (!listEl) return;
      for (const cb of listEl.querySelectorAll('input[type="checkbox"]')) {
        if (!cb.checked) {
          cb.checked = true;
          onChange(cb.value, true);
        }
      }
    });
    const noneBtn = document.createElement("button");
    noneBtn.type = "button";
    noneBtn.className = "filter-facet-action";
    noneBtn.textContent = "Clear";
    noneBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      const listEl = details.querySelector(".filter-facet-list");
      if (!listEl) return;
      for (const cb of listEl.querySelectorAll('input[type="checkbox"]')) {
        if (cb.checked) {
          cb.checked = false;
          onChange(cb.value, false);
        }
      }
    });
    actions.append(allBtn, noneBtn);
    summary.append(label, badge, actions);
    details.appendChild(summary);
    if (facet.hasSearch === true) {
      const searchInput = document.createElement("input");
      searchInput.type = "search";
      searchInput.className = "filter-facet-search";
      searchInput.placeholder = "Narrow type list...";
      searchInput.autocomplete = "off";
      searchInput.addEventListener("input", () => {
        onSearchInput?.(searchInput.value);
      });
      details.appendChild(searchInput);
    }
    const list = document.createElement("div");
    list.className = "filter-facet-list";
    details.appendChild(list);
    return details;
  }
  function rebuildFacetCheckboxes(details, values, selectedValues, counts, onChange) {
    const list = details.querySelector(".filter-facet-list");
    if (!list) return;
    list.replaceChildren();
    for (const value of values) {
      const row = document.createElement("label");
      row.className = "filter-facet-item";
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.value = value;
      checkbox.checked = selectedValues.has(value);
      checkbox.addEventListener("change", () => {
        onChange(value, checkbox.checked);
      });
      const text = document.createElement("span");
      text.textContent = value;
      const count = document.createElement("span");
      count.className = "filter-facet-item-count";
      count.textContent = String(counts.get(value) ?? 0);
      row.append(checkbox, text, count);
      list.appendChild(row);
    }
  }
  function syncFacetBadge(details, selectedCount) {
    const badge = details.querySelector(".filter-facet-badge");
    if (badge instanceof HTMLElement) {
      badge.hidden = selectedCount === 0;
      badge.textContent = String(selectedCount);
    }
  }
  function renderActiveFilterSummary(container, items, onClickFacet) {
    container.replaceChildren();
    if (items.length === 0) {
      container.hidden = true;
      return;
    }
    container.hidden = false;
    for (const item of items) {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = "filter-summary-chip";
      const preview = item.values.slice(0, 2).join(", ");
      const suffix = item.count > 2 ? ` +${item.count - 2}` : "";
      chip.textContent = `${item.label}: ${preview}${suffix}`;
      chip.title = item.values.join(", ");
      chip.addEventListener("click", () => {
        onClickFacet(item.facetId);
      });
      container.appendChild(chip);
    }
  }
  function syncFilterResetBar(active, items, mode, resetBar, resetCopy) {
    if (!resetBar || !resetCopy) return;
    resetBar.toggleAttribute("hidden", !active);
    if (!active) {
      resetCopy.textContent = "";
      return;
    }
    const parts = items.map((item) => {
      const preview = item.values.slice(0, 2).join(", ");
      const suffix = item.count > 2 ? ` +${item.count - 2}` : "";
      return `${item.label}: ${preview}${suffix}`;
    });
    const modeLabel = mode !== "dim" ? ` [${mode}]` : "";
    resetCopy.textContent = parts.join(" / ") + modeLabel;
  }
  function rebuildColumnTypeFacet(details, facet, query, onChange) {
    const visible = visibleTypesForQuery(facet.allValues, query);
    rebuildFacetCheckboxes(details, visible, facet.selectedValues, facet.counts, onChange);
  }

  // ts/filter_engine.ts
  {
    const sectionEl = document.getElementById("filter-section");
    const headerEl = document.getElementById("filter-section-header");
    const summaryEl = document.getElementById("filter-active-summary");
    const facetsEl = document.getElementById("filter-facets");
    const svgEl = document.querySelector(".canvas svg");
    const resetBar = document.getElementById("filter-reset-bar");
    const resetCopy = document.getElementById("filter-reset-copy");
    const resetButton = document.getElementById("filter-reset-button");
    if (sectionEl === null || headerEl === null || summaryEl === null || facetsEl === null || svgEl === null) {
    } else {
      let applyFilter = function() {
        const nodes = svgRoot.querySelectorAll(".node");
        const active = hasActiveFilters(state);
        if (!active) {
          nodes.forEach((node) => {
            node.classList.remove("dimmed-by-filter", "hidden-by-filter");
          });
        } else {
          const dimClass = state.mode === "dim" ? "dimmed-by-filter" : "hidden-by-filter";
          const removeClass = state.mode === "dim" ? "hidden-by-filter" : "dimmed-by-filter";
          nodes.forEach((node) => {
            const tableId = node.getAttribute("data-id") ?? node.getAttribute("data-table-id") ?? "";
            const table = tables.find((t) => t.id === tableId);
            const matches = table !== void 0 && tableMatchesAllFacets(table, state);
            node.classList.toggle(dimClass, !matches);
            node.classList.remove(removeClass);
          });
        }
        syncEdgeDimming(svgRoot);
        if (active && state.mode === "focus") {
          fitToVisibleNodes();
        }
        const summaryItems = activeFilterSummary(state);
        for (const [facetId, details] of facetDetails) {
          const facet = state.facets.get(facetId);
          if (facet) {
            syncFacetBadge(details, facet.selectedValues.size);
          }
        }
        renderActiveFilterSummary(summaryRoot, summaryItems, (facetId) => {
          const details = facetDetails.get(facetId);
          if (details) {
            details.open = true;
            details.scrollIntoView({ behavior: "smooth", block: "nearest" });
          }
        });
        resetAllBtn.hidden = !active;
        syncFilterResetBar(active, summaryItems, state.mode, resetBar, resetCopy);
        emitViewerEvent("relune:filters-changed", {
          active,
          mode: state.mode,
          facets: summaryItems
        });
      }, clearAll = function() {
        for (const facet of state.facets.values()) {
          facet.selectedValues.clear();
        }
        columnTypeQuery.value = "";
        for (const [facetId, details] of facetDetails) {
          const facet = state.facets.get(facetId);
          if (!facet) continue;
          const searchInput = details.querySelector(".filter-facet-search");
          if (searchInput) {
            searchInput.value = "";
          }
          const onChange = (value, checked) => {
            if (checked) {
              facet.selectedValues.add(value);
            } else {
              facet.selectedValues.delete(value);
            }
            applyFilter();
          };
          if (facetId === "columnType") {
            rebuildColumnTypeFacet(details, facet, "", onChange);
          } else {
            rebuildFacetCheckboxes(
              details,
              facet.allValues,
              facet.selectedValues,
              facet.counts,
              onChange
            );
          }
        }
        applyFilter();
      }, fitToVisibleNodes = function() {
        const nodes = svgRoot.querySelectorAll(".node:not(.hidden-by-filter)");
        if (nodes.length === 0) {
          runtime.viewport?.fit();
          return;
        }
        let minX = Infinity;
        let minY = Infinity;
        let maxX = -Infinity;
        let maxY = -Infinity;
        for (const node of nodes) {
          if (node instanceof SVGGraphicsElement) {
            const bbox = node.getBBox();
            minX = Math.min(minX, bbox.x);
            minY = Math.min(minY, bbox.y);
            maxX = Math.max(maxX, bbox.x + bbox.width);
            maxY = Math.max(maxY, bbox.y + bbox.height);
          }
        }
        if (minX < maxX && minY < maxY) {
          runtime.viewport?.fitToRect({ x: minX, y: minY, width: maxX - minX, height: maxY - minY });
        }
      };
      applyFilter2 = applyFilter, clearAll2 = clearAll, fitToVisibleNodes2 = fitToVisibleNodes;
      const runtime = getViewerRuntime();
      const svgRoot = svgEl;
      const summaryRoot = summaryEl;
      const metadata = parseReluneMetadata();
      const tables = metadata?.tables ?? [];
      const state = createFilterEngineState(tables);
      if (state.facets.size === 0) {
        sectionEl.hidden = true;
      } else {
        sectionEl.hidden = false;
      }
      const titleSpan = document.createElement("span");
      titleSpan.textContent = "Filters";
      const modeSwitcher = buildFilterModeSwitcher(state.mode, (mode) => {
        state.mode = mode;
        syncModeSwitcher(modeSwitcher, mode);
        applyFilter();
      });
      const resetAllBtn = document.createElement("button");
      resetAllBtn.type = "button";
      resetAllBtn.className = "filter-section-reset";
      resetAllBtn.textContent = "Reset";
      resetAllBtn.hidden = true;
      resetAllBtn.addEventListener("click", clearAll);
      headerEl.append(titleSpan, modeSwitcher, resetAllBtn);
      const columnTypeQuery = { value: "" };
      const facetDetails = /* @__PURE__ */ new Map();
      for (const facet of state.facets.values()) {
        const onChange = (value, checked) => {
          if (checked) {
            facet.selectedValues.add(value);
          } else {
            facet.selectedValues.delete(value);
          }
          applyFilter();
        };
        const onSearchInput = facet.id === "columnType" ? (query) => {
          columnTypeQuery.value = query;
          rebuildColumnTypeFacet(details, facet, query, onChange);
        } : void 0;
        const details = buildFacetSection(facet, onChange, onSearchInput);
        if (facet.allValues.length <= 5) {
          details.open = true;
        }
        facetDetails.set(facet.id, details);
        if (facet.id === "columnType") {
          rebuildColumnTypeFacet(details, facet, "", onChange);
        } else {
          rebuildFacetCheckboxes(
            details,
            facet.allValues,
            facet.selectedValues,
            facet.counts,
            onChange
          );
        }
        facetsEl.appendChild(details);
      }
      resetButton?.addEventListener("click", clearAll);
      runtime.filters = {
        reset() {
          clearAll();
        },
        hasActiveFilters() {
          return hasActiveFilters(state);
        },
        getMode() {
          return state.mode;
        },
        setMode(mode) {
          state.mode = mode;
          syncModeSwitcher(modeSwitcher, mode);
          applyFilter();
        },
        getFacetSelection(facetId) {
          const facet = state.facets.get(facetId);
          return facet ? [...facet.selectedValues].sort() : [];
        },
        setFacetSelection(facetId, values) {
          const facet = state.facets.get(facetId);
          if (!facet) return;
          facet.selectedValues.clear();
          for (const v of values) {
            if (facet.allValues.includes(v)) {
              facet.selectedValues.add(v);
            }
          }
          const details = facetDetails.get(facetId);
          if (details) {
            const onChange = (value, checked) => {
              if (checked) {
                facet.selectedValues.add(value);
              } else {
                facet.selectedValues.delete(value);
              }
              applyFilter();
            };
            if (facetId === "columnType") {
              rebuildColumnTypeFacet(details, facet, columnTypeQuery.value, onChange);
            } else {
              rebuildFacetCheckboxes(
                details,
                facet.allValues,
                facet.selectedValues,
                facet.counts,
                onChange
              );
            }
          }
          applyFilter();
        },
        getAvailableFacets() {
          return [...state.facets.keys()];
        }
      };
      markViewerModuleReady("filters");
      applyFilter();
    }
  }
  var applyFilter2;
  var clearAll2;
  var fitToVisibleNodes2;
})();
