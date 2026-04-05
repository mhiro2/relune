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

  // ts/highlight_state.ts
  function createHighlightState(tables, edges) {
    const tableById = new Map(tables.map((table) => [table.id, table]));
    const inboundMap = {};
    const outboundMap = {};
    for (const edge of edges) {
      (outboundMap[edge.from] ??= []).push({ node: edge.to, edge });
      (inboundMap[edge.to] ??= []).push({ node: edge.from, edge });
    }
    return { selectedNode: null, tableById, inboundMap, outboundMap, edges };
  }

  // ts/highlight_actions.ts
  function computeNeighborHighlights(nodeId, state) {
    const inbound = state.inboundMap[nodeId] ?? [];
    const outbound = state.outboundMap[nodeId] ?? [];
    const neighborIds = /* @__PURE__ */ new Set();
    const inboundNodeIds = /* @__PURE__ */ new Set();
    const outboundNodeIds = /* @__PURE__ */ new Set();
    for (const relation of inbound) {
      neighborIds.add(relation.node);
      inboundNodeIds.add(relation.node);
    }
    for (const relation of outbound) {
      neighborIds.add(relation.node);
      outboundNodeIds.add(relation.node);
    }
    const connectedEdgeIndices = /* @__PURE__ */ new Set();
    state.edges.forEach((edge, index) => {
      if (edge.from === nodeId || edge.to === nodeId) {
        connectedEdgeIndices.add(index);
      }
    });
    return { selectedId: nodeId, neighborIds, connectedEdgeIndices, inboundNodeIds, outboundNodeIds };
  }
  function matchesBrowserQuery(table, query) {
    const needle = query.trim().toLowerCase();
    if (needle === "") {
      return true;
    }
    return table.id.toLowerCase().includes(needle) || table.label.toLowerCase().includes(needle) || table.table_name.toLowerCase().includes(needle) || table.columns.some(
      (column) => column.name.toLowerCase().includes(needle) || column.data_type.toLowerCase().includes(needle)
    );
  }

  // ts/highlight_dom.ts
  var ALLOWED_DIFF_KINDS = /* @__PURE__ */ new Set(["added", "removed", "modified"]);
  var ALLOWED_SEVERITIES = /* @__PURE__ */ new Set(["error", "warning", "info", "hint"]);
  function safeCssToken(value, allowlist) {
    return allowlist.has(value) ? value : "";
  }
  function clearChildren(element) {
    element.replaceChildren();
  }
  function diffBadge(kind) {
    const badge = document.createElement("div");
    const safe = safeCssToken(kind, ALLOWED_DIFF_KINDS);
    badge.className = safe !== "" ? `detail-diff-badge detail-diff-badge-${safe}` : "detail-diff-badge";
    badge.textContent = kind;
    return badge;
  }
  function metricCard(label, value) {
    const card = document.createElement("div");
    card.className = "detail-metric";
    const labelEl = document.createElement("span");
    labelEl.className = "detail-metric-label";
    labelEl.textContent = label;
    const valueEl = document.createElement("span");
    valueEl.className = "detail-metric-value";
    valueEl.textContent = value;
    card.append(labelEl, valueEl);
    return card;
  }
  function clearHighlightClasses(svgRoot, getNodes) {
    getNodes().forEach((node) => {
      node.classList.remove(
        "highlighted-neighbor",
        "dimmed-by-highlight",
        "selected-node",
        "inbound",
        "outbound"
      );
    });
    svgRoot.querySelectorAll(".edge").forEach((edge) => {
      edge.classList.remove("highlighted-neighbor", "dimmed-by-highlight");
    });
  }
  function applyHighlightClasses(svgRoot, getNodes, getNodeId, highlight) {
    getNodes().forEach((node) => {
      const id = getNodeId(node);
      if (id === highlight.selectedId) {
        node.classList.add("selected-node");
        node.classList.remove("dimmed-by-highlight");
      } else if (id !== null && highlight.neighborIds.has(id)) {
        node.classList.add("highlighted-neighbor");
        const isInbound = highlight.inboundNodeIds.has(id);
        const isOutbound = highlight.outboundNodeIds.has(id);
        node.classList.toggle("inbound", isInbound && !isOutbound);
        node.classList.toggle("outbound", isOutbound && !isInbound);
        node.classList.remove("dimmed-by-highlight");
      } else {
        node.classList.add("dimmed-by-highlight");
        node.classList.remove("highlighted-neighbor", "selected-node", "inbound", "outbound");
      }
    });
    svgRoot.querySelectorAll(".edge").forEach((edgeElement, index) => {
      edgeElement.classList.toggle("highlighted-neighbor", highlight.connectedEdgeIndices.has(index));
      edgeElement.classList.toggle("dimmed-by-highlight", !highlight.connectedEdgeIndices.has(index));
    });
  }
  function renderDrawer(table, state, elements) {
    if (table === void 0) {
      elements.drawer.setAttribute("hidden", "");
      clearChildren(elements.metrics);
      clearChildren(elements.columns);
      clearChildren(elements.relations);
      if (elements.issues) clearChildren(elements.issues);
      elements.columnsEmpty.removeAttribute("hidden");
      elements.relationsEmpty.removeAttribute("hidden");
      if (elements.issuesEmpty) elements.issuesEmpty.removeAttribute("hidden");
      return;
    }
    const tableId = table.id;
    elements.drawer.removeAttribute("hidden");
    elements.kind.textContent = table.kind;
    elements.title.textContent = table.label || table.table_name || table.id;
    elements.subtitle.textContent = table.schema_name ? `${table.schema_name}.${table.table_name}` : table.table_name;
    clearChildren(elements.metrics);
    if (table.diff_kind) {
      elements.metrics.append(diffBadge(table.diff_kind));
    }
    elements.metrics.append(
      metricCard("Columns", String(table.columns.length)),
      metricCard("Inbound", String(table.inbound_count)),
      metricCard("Outbound", String(table.outbound_count))
    );
    clearChildren(elements.columns);
    if (table.columns.length === 0) {
      elements.columnsEmpty.removeAttribute("hidden");
    } else {
      elements.columnsEmpty.setAttribute("hidden", "");
      for (const column of table.columns) {
        elements.columns.appendChild(buildColumnElement(column));
      }
    }
    clearChildren(elements.relations);
    const relations = [...state.inboundMap[tableId] ?? [], ...state.outboundMap[tableId] ?? []];
    if (relations.length === 0) {
      elements.relationsEmpty.removeAttribute("hidden");
    } else {
      elements.relationsEmpty.setAttribute("hidden", "");
      for (const relation of relations) {
        elements.relations.appendChild(
          buildRelationElement(relation.edge, relation.node, state.tableById)
        );
      }
    }
    if (elements.issues instanceof HTMLElement && elements.issuesEmpty instanceof HTMLElement) {
      clearChildren(elements.issues);
      const issues = table.issues ?? [];
      if (issues.length === 0) {
        elements.issuesEmpty.removeAttribute("hidden");
      } else {
        elements.issuesEmpty.setAttribute("hidden", "");
        for (const issue of issues) {
          elements.issues.appendChild(buildIssueElement(issue));
        }
      }
    }
  }
  function buildColumnElement(column) {
    const columnEl = document.createElement("div");
    columnEl.className = "detail-column";
    const name = document.createElement("span");
    name.className = "detail-column-name";
    name.textContent = column.name;
    const pills = document.createElement("span");
    pills.className = "detail-column-pills";
    if (column.is_primary_key) {
      const pk = document.createElement("span");
      pk.className = "detail-column-pill detail-column-pill-pk";
      pk.textContent = "PK";
      pills.appendChild(pk);
    }
    const typePill = document.createElement("span");
    typePill.className = "detail-column-pill";
    typePill.textContent = column.data_type || "unknown";
    pills.appendChild(typePill);
    const nullPill = document.createElement("span");
    nullPill.className = `detail-column-pill ${column.nullable ? "detail-column-pill-nullable" : "detail-column-pill-required"}`;
    nullPill.textContent = column.nullable ? "nullable" : "required";
    pills.appendChild(nullPill);
    if (column.diff_kind) {
      const diffPill = document.createElement("span");
      const safeDiff = safeCssToken(column.diff_kind, ALLOWED_DIFF_KINDS);
      diffPill.className = safeDiff !== "" ? `detail-column-pill detail-column-pill-diff detail-column-pill-diff-${safeDiff}` : "detail-column-pill detail-column-pill-diff";
      diffPill.textContent = column.diff_kind;
      pills.appendChild(diffPill);
    }
    columnEl.append(name, pills);
    return columnEl;
  }
  function buildRelationElement(edge, targetNodeId, tableById) {
    const relationEl = document.createElement("div");
    relationEl.className = "detail-relation";
    const targetTable = tableById.get(targetNodeId);
    const label = document.createElement("span");
    label.className = "detail-relation-label";
    label.textContent = edge.name ?? `${edge.from} \u2192 ${edge.to}`;
    const meta = document.createElement("span");
    meta.className = "detail-relation-meta";
    const targetName = targetTable?.label ?? targetNodeId;
    const columnMap = edge.from_columns.length > 0 && edge.to_columns.length > 0 ? ` \xB7 ${edge.from_columns.join(", ")} \u2192 ${edge.to_columns.join(", ")}` : "";
    meta.textContent = `${edge.kind} \xB7 ${targetName}${columnMap}`;
    relationEl.append(label, meta);
    return relationEl;
  }
  function buildIssueElement(issue) {
    const issueEl = document.createElement("div");
    const safeSev = safeCssToken(issue.severity, ALLOWED_SEVERITIES);
    issueEl.className = safeSev !== "" ? `detail-issue detail-issue-${safeSev}` : "detail-issue";
    const header = document.createElement("div");
    header.className = "detail-issue-header";
    const badge = document.createElement("span");
    badge.className = safeSev !== "" ? `detail-issue-badge detail-issue-badge-${safeSev}` : "detail-issue-badge";
    badge.textContent = issue.severity;
    const msg = document.createElement("span");
    msg.className = "detail-issue-message";
    msg.textContent = issue.message;
    header.append(badge, msg);
    issueEl.appendChild(header);
    if (issue.hint) {
      const hintEl = document.createElement("span");
      hintEl.className = "detail-issue-hint";
      hintEl.textContent = `\u2192 ${issue.hint}`;
      issueEl.appendChild(hintEl);
    }
    return issueEl;
  }
  function renderObjectBrowser(items, totalCount, listEl, countEl, emptyEl, onSelect) {
    listEl.replaceChildren();
    countEl.textContent = `${items.length}/${totalCount}`;
    emptyEl.toggleAttribute("hidden", items.length > 0);
    for (const item of items) {
      const button = buildObjectBrowserButton(item, onSelect);
      listEl.appendChild(button);
    }
  }
  function buildObjectBrowserButton(item, onSelect) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "object-browser-item";
    button.classList.toggle("selected", item.isSelected);
    button.classList.toggle("filtered-out", item.isDimmedBySearch || item.isDimmedByTypeFilter);
    button.classList.toggle("hidden-item", item.isHiddenByGroup);
    const header = document.createElement("div");
    header.className = "object-browser-item-header";
    const name = document.createElement("span");
    name.className = "object-browser-item-name";
    name.textContent = item.table.label || item.table.table_name || item.table.id;
    const kind = document.createElement("span");
    kind.className = "object-browser-kind";
    kind.textContent = item.table.kind;
    const tableIssues = item.table.issues ?? [];
    if (tableIssues.length > 0) {
      const severityRank = { error: 3, warning: 2, info: 1, hint: 0 };
      const maxSeverity = tableIssues.reduce((max, issue) => {
        return (severityRank[issue.severity] ?? 0) > (severityRank[max] ?? 0) ? issue.severity : max;
      }, "hint");
      const issueBadge = document.createElement("span");
      const safeSev = safeCssToken(maxSeverity, ALLOWED_SEVERITIES);
      issueBadge.className = safeSev !== "" ? `object-browser-issue-badge object-browser-issue-badge-${safeSev}` : "object-browser-issue-badge";
      issueBadge.textContent = String(tableIssues.length);
      issueBadge.title = `${tableIssues.length} issue${tableIssues.length === 1 ? "" : "s"}`;
      header.append(name, issueBadge, kind);
    } else {
      header.append(name, kind);
    }
    const meta = document.createElement("div");
    meta.className = "object-browser-item-meta";
    const counts = document.createElement("span");
    counts.textContent = `${item.table.columns.length} cols`;
    const relations = document.createElement("span");
    relations.textContent = `${item.table.inbound_count} in / ${item.table.outbound_count} out`;
    meta.append(counts, relations);
    button.append(header, meta);
    button.addEventListener("click", () => {
      onSelect(item.table.id);
    });
    return button;
  }

  // ts/highlight.ts
  {
    const metadata = parseReluneMetadata();
    const tables = metadata?.tables ?? [];
    const state = createHighlightState(tables, metadata?.edges ?? []);
    const canvas = document.getElementById("canvas");
    const svgRoot = canvas?.querySelector("svg");
    const searchInput = document.getElementById("table-search");
    const objectBrowserList = document.getElementById("object-browser-list");
    const objectBrowserCount = document.getElementById("object-browser-count");
    const objectBrowserEmpty = document.getElementById("object-browser-empty");
    const drawerClose = document.getElementById("detail-close");
    const drawerEls = (() => {
      const drawer = document.getElementById("detail-drawer");
      const title = document.getElementById("detail-title");
      const kind = document.getElementById("detail-kind");
      const subtitle = document.getElementById("detail-subtitle");
      const metrics = document.getElementById("detail-metrics");
      const columns = document.getElementById("detail-columns");
      const columnsEmpty = document.getElementById("detail-columns-empty");
      const relations = document.getElementById("detail-relations");
      const relationsEmpty = document.getElementById("detail-relationships-empty");
      if (drawer instanceof HTMLElement && title instanceof HTMLElement && kind instanceof HTMLElement && subtitle instanceof HTMLElement && metrics instanceof HTMLElement && columns instanceof HTMLElement && columnsEmpty instanceof HTMLElement && relations instanceof HTMLElement && relationsEmpty instanceof HTMLElement) {
        return {
          drawer,
          title,
          kind,
          subtitle,
          metrics,
          columns,
          columnsEmpty,
          relations,
          relationsEmpty,
          issues: document.getElementById("detail-issues"),
          issuesEmpty: document.getElementById("detail-issues-empty")
        };
      }
      return null;
    })();
    if (svgRoot && drawerEls) {
      const runtime = getViewerRuntime();
      const getNodes = () => svgRoot.querySelectorAll(".node[data-id], .table-node[data-table-id]");
      const getNodeId = (node) => node.getAttribute("data-id") ?? node.getAttribute("data-table-id");
      const centerNodeInViewport = (nodeId) => {
        const node = Array.from(getNodes()).find((c) => getNodeId(c) === nodeId);
        const rect = node?.querySelector(".table-body");
        if (rect === void 0 || rect === null) return;
        const x = Number.parseFloat(rect.getAttribute("x") ?? "0");
        const y = Number.parseFloat(rect.getAttribute("y") ?? "0");
        const width = Number.parseFloat(rect.getAttribute("width") ?? "0");
        const height = Number.parseFloat(rect.getAttribute("height") ?? "0");
        runtime.viewport?.center(x + width / 2, y + height / 2);
      };
      const syncObjectBrowser = () => {
        if (!(objectBrowserList instanceof HTMLElement) || !(objectBrowserCount instanceof HTMLElement) || !(objectBrowserEmpty instanceof HTMLElement)) {
          return;
        }
        const query = searchInput instanceof HTMLInputElement ? searchInput.value : "";
        const visibleTables = tables.filter((table) => matchesBrowserQuery(table, query));
        const items = visibleTables.map((table) => {
          const node = Array.from(getNodes()).find((c) => getNodeId(c) === table.id);
          return {
            table,
            isSelected: state.selectedNode === table.id,
            isDimmedBySearch: node?.classList.contains("dimmed-by-search") === true,
            isDimmedByTypeFilter: node?.classList.contains("dimmed-by-type-filter") === true,
            isHiddenByGroup: node?.classList.contains("hidden-by-group") === true
          };
        });
        renderObjectBrowser(
          items,
          tables.length,
          objectBrowserList,
          objectBrowserCount,
          objectBrowserEmpty,
          (tableId) => {
            if (state.selectedNode === tableId) {
              runtime.selection?.clear();
            } else {
              runtime.selection?.select(tableId);
              centerNodeInViewport(tableId);
            }
          }
        );
      };
      const applySelection = (tableId) => {
        if (tableId === null) {
          clearHighlightClasses(svgRoot, getNodes);
          renderDrawer(void 0, state, drawerEls);
          emitViewerEvent("relune:node-cleared", void 0);
        } else {
          const highlight = computeNeighborHighlights(tableId, state);
          applyHighlightClasses(svgRoot, getNodes, getNodeId, highlight);
          renderDrawer(state.tableById.get(tableId), state, drawerEls);
          emitViewerEvent("relune:node-selected", { nodeId: tableId });
        }
        syncObjectBrowser();
      };
      getNodes().forEach((node) => {
        node.addEventListener("mouseenter", () => {
          if (state.selectedNode !== null) return;
          const nodeId = getNodeId(node);
          if (nodeId !== null) {
            const highlight = computeNeighborHighlights(nodeId, state);
            applyHighlightClasses(svgRoot, getNodes, getNodeId, highlight);
          }
        });
        node.addEventListener("mouseleave", () => {
          if (state.selectedNode === null) {
            clearHighlightClasses(svgRoot, getNodes);
          }
        });
        node.addEventListener("click", (event) => {
          event.stopPropagation();
          const nodeId = getNodeId(node);
          if (nodeId === null) return;
          if (state.selectedNode === nodeId) {
            state.selectedNode = null;
            applySelection(null);
          } else {
            state.selectedNode = nodeId;
            applySelection(nodeId);
          }
        });
      });
      svgRoot.addEventListener("click", () => {
        if (state.selectedNode !== null) {
          state.selectedNode = null;
          applySelection(null);
        }
      });
      drawerClose?.addEventListener("click", () => {
        state.selectedNode = null;
        applySelection(null);
      });
      searchInput?.addEventListener("input", () => syncObjectBrowser());
      document.addEventListener("relune:filters-changed", syncObjectBrowser);
      document.addEventListener("relune:search-changed", syncObjectBrowser);
      document.addEventListener("relune:groups-changed", syncObjectBrowser);
      runtime.selection = {
        clear() {
          state.selectedNode = null;
          applySelection(null);
        },
        select(nodeId) {
          const node = Array.from(getNodes()).find((c) => getNodeId(c) === nodeId);
          if (node === void 0) return;
          state.selectedNode = nodeId;
          applySelection(nodeId);
        },
        getSelected() {
          return state.selectedNode;
        }
      };
      markViewerModuleReady("selection");
      syncObjectBrowser();
    }
  }
})();
