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
    return { hoveredNode: null, selectedNode: null, tableById, inboundMap, outboundMap, edges };
  }

  // ts/highlight_actions.ts
  function collectNeighborhood(nodeId, state) {
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
    return { neighborIds, connectedEdgeIndices, inboundNodeIds, outboundNodeIds };
  }
  function computeNeighborHighlights(nodeId, state) {
    return { selectedId: nodeId, ...collectNeighborhood(nodeId, state) };
  }
  function computeHoverPreview(nodeId, state) {
    return { hoveredId: nodeId, ...collectNeighborhood(nodeId, state) };
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
  var SELECTED_NODE_CLASSES = [
    "highlighted-neighbor",
    "dimmed-by-highlight",
    "selected-node",
    "inbound",
    "outbound"
  ];
  var HOVER_NODE_CLASSES = [
    "hover-preview-node",
    "hover-preview-neighbor",
    "hover-inbound",
    "hover-outbound"
  ];
  function clearHighlightClasses(svgRoot, getNodes) {
    getNodes().forEach((node) => {
      node.classList.remove(...SELECTED_NODE_CLASSES, ...HOVER_NODE_CLASSES);
    });
    svgRoot.querySelectorAll(".edge").forEach((edge) => {
      edge.classList.remove("highlighted-neighbor", "dimmed-by-highlight", "hover-preview-edge");
    });
  }
  function applySelectedHighlightClasses(svgRoot, getNodes, getNodeId, highlight) {
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
  function applyHoverPreviewClasses(svgRoot, getNodes, getNodeId, preview) {
    getNodes().forEach((node) => {
      const id = getNodeId(node);
      if (id === preview.hoveredId) {
        node.classList.add("hover-preview-node");
      } else if (id !== null && preview.neighborIds.has(id)) {
        node.classList.add("hover-preview-neighbor");
        const isInbound = preview.inboundNodeIds.has(id);
        const isOutbound = preview.outboundNodeIds.has(id);
        node.classList.toggle("hover-inbound", isInbound && !isOutbound);
        node.classList.toggle("hover-outbound", isOutbound && !isInbound);
      }
    });
    svgRoot.querySelectorAll(".edge").forEach((edgeElement, index) => {
      edgeElement.classList.toggle("hover-preview-edge", preview.connectedEdgeIndices.has(index));
    });
  }
  function renderDrawer(table, state, elements, onNavigate) {
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
          buildRelationElement(relation.edge, relation.node, state.tableById, onNavigate)
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
  function hideHoverPopover(elements) {
    elements.popover.setAttribute("hidden", "");
    elements.popover.style.removeProperty("left");
    elements.popover.style.removeProperty("top");
    elements.popover.style.removeProperty("visibility");
    clearChildren(elements.metrics);
    clearChildren(elements.badges);
  }
  function renderHoverPopover(table, elements, position) {
    if (table === void 0 || position === void 0) {
      hideHoverPopover(elements);
      return;
    }
    elements.kind.textContent = table.kind;
    elements.title.textContent = tableDisplayName(table);
    elements.subtitle.textContent = table.schema_name ? `${table.schema_name}.${table.table_name}` : table.table_name;
    clearChildren(elements.metrics);
    elements.metrics.append(
      summaryMetric("Cols", String(table.columns.length)),
      summaryMetric("In", String(table.inbound_count)),
      summaryMetric("Out", String(table.outbound_count))
    );
    clearChildren(elements.badges);
    if (table.diff_kind) {
      elements.badges.appendChild(diffBadge(table.diff_kind));
    }
    const issues = table.issues ?? [];
    if (issues.length > 0) {
      elements.badges.appendChild(issueCountBadge(issues));
    }
    placePopover(elements.popover, position);
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
    if (column.is_foreign_key) {
      const fk = document.createElement("span");
      fk.className = "detail-column-pill detail-column-pill-fk";
      fk.textContent = "FK";
      pills.appendChild(fk);
    }
    if (column.is_indexed) {
      const ix = document.createElement("span");
      ix.className = "detail-column-pill detail-column-pill-ix";
      ix.textContent = "IX";
      pills.appendChild(ix);
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
  function buildRelationElement(edge, targetNodeId, tableById, onNavigate) {
    const relationEl = document.createElement("div");
    relationEl.className = "detail-relation";
    if (onNavigate) {
      relationEl.classList.add("detail-relation-navigable");
      relationEl.addEventListener("click", () => {
        onNavigate(targetNodeId);
      });
    }
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
  function summaryMetric(label, value) {
    const metric = document.createElement("span");
    metric.className = "hover-popover-metric";
    const labelEl = document.createElement("span");
    labelEl.className = "hover-popover-metric-label";
    labelEl.textContent = label;
    const valueEl = document.createElement("span");
    valueEl.className = "hover-popover-metric-value";
    valueEl.textContent = value;
    metric.append(labelEl, valueEl);
    return metric;
  }
  function issueCountBadge(issues) {
    const badge = document.createElement("span");
    const safeSeverity = safeCssToken(highestIssueSeverity(issues), ALLOWED_SEVERITIES);
    badge.className = safeSeverity !== "" ? `hover-popover-badge hover-popover-badge-${safeSeverity}` : "hover-popover-badge";
    badge.textContent = `${issues.length} issue${issues.length === 1 ? "" : "s"}`;
    return badge;
  }
  function highestIssueSeverity(issues) {
    const severityRank = { error: 3, warning: 2, info: 1, hint: 0 };
    return issues.reduce((max, issue) => {
      return (severityRank[issue.severity] ?? 0) > (severityRank[max] ?? 0) ? issue.severity : max;
    }, "hint");
  }
  function placePopover(popover, position) {
    const margin = 12;
    popover.removeAttribute("hidden");
    popover.style.left = `${Math.round(position.left)}px`;
    popover.style.top = `${Math.round(position.top)}px`;
    popover.style.visibility = "hidden";
    const rect = popover.getBoundingClientRect();
    const left = Math.max(margin, Math.min(position.left, window.innerWidth - rect.width - margin));
    const top = Math.max(margin, Math.min(position.top, window.innerHeight - rect.height - margin));
    popover.style.left = `${Math.round(left)}px`;
    popover.style.top = `${Math.round(top)}px`;
    popover.style.visibility = "visible";
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
    button.classList.toggle("filtered-out", item.isDimmedBySearch || item.isExcludedByFilter);
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
    const viewport = document.getElementById("viewport");
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
    const hoverEls = (() => {
      const popover = document.getElementById("hover-popover");
      const kind = document.getElementById("hover-popover-kind");
      const title = document.getElementById("hover-popover-title");
      const subtitle = document.getElementById("hover-popover-subtitle");
      const metrics = document.getElementById("hover-popover-metrics");
      const badges = document.getElementById("hover-popover-badges");
      if (popover instanceof HTMLElement && kind instanceof HTMLElement && title instanceof HTMLElement && subtitle instanceof HTMLElement && metrics instanceof HTMLElement && badges instanceof HTMLElement) {
        return { popover, kind, title, subtitle, metrics, badges };
      }
      return null;
    })();
    if (svgRoot && drawerEls && hoverEls) {
      const runtime = getViewerRuntime();
      const getNodes = () => svgRoot.querySelectorAll(".node[data-id], .table-node[data-table-id]");
      const getNodeId = (node) => node.getAttribute("data-id") ?? node.getAttribute("data-table-id");
      const findNode = (nodeId) => Array.from(getNodes()).find((candidate) => getNodeId(candidate) === nodeId);
      const hoverPopoverPosition = (node) => {
        const anchor = node.querySelector(".table-body") ?? node;
        const rect = anchor.getBoundingClientRect();
        const viewportRect = viewport?.getBoundingClientRect();
        const top = Math.max(rect.top - 8, (viewportRect?.top ?? 0) + 12);
        return {
          left: rect.right + 14,
          top
        };
      };
      const centerNodeInViewport = (nodeId) => {
        const node = findNode(nodeId);
        const rect = node?.querySelector(".table-body");
        if (rect === void 0 || rect === null) return;
        const x = Number.parseFloat(rect.getAttribute("x") ?? "0");
        const y = Number.parseFloat(rect.getAttribute("y") ?? "0");
        const width = Number.parseFloat(rect.getAttribute("width") ?? "0");
        const height = Number.parseFloat(rect.getAttribute("height") ?? "0");
        runtime.viewport?.center(x + width / 2, y + height / 2);
      };
      const navigateToTable = (tableId) => {
        setSelectedNode(tableId);
        centerNodeInViewport(tableId);
      };
      const syncObjectBrowser = () => {
        if (!(objectBrowserList instanceof HTMLElement) || !(objectBrowserCount instanceof HTMLElement) || !(objectBrowserEmpty instanceof HTMLElement)) {
          return;
        }
        const query = searchInput instanceof HTMLInputElement ? searchInput.value : "";
        const visibleTables = tables.filter((table) => matchesBrowserQuery(table, query));
        const filterMode = runtime.filters?.getMode() ?? "dim";
        const isHideOrFocus = filterMode === "hide" || filterMode === "focus";
        const items = visibleTables.filter((table) => {
          if (!isHideOrFocus) return true;
          const node = findNode(table.id);
          return node?.classList.contains("hidden-by-filter") !== true;
        }).map((table) => {
          const node = findNode(table.id);
          return {
            table,
            isSelected: state.selectedNode === table.id,
            isDimmedBySearch: node?.classList.contains("dimmed-by-search") === true,
            isExcludedByFilter: node?.classList.contains("dimmed-by-filter") === true,
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
      const renderInteraction = () => {
        clearHighlightClasses(svgRoot, getNodes);
        hideHoverPopover(hoverEls);
        if (state.selectedNode !== null) {
          const highlight = computeNeighborHighlights(state.selectedNode, state);
          applySelectedHighlightClasses(svgRoot, getNodes, getNodeId, highlight);
          renderDrawer(state.tableById.get(state.selectedNode), state, drawerEls, navigateToTable);
        } else {
          renderDrawer(void 0, state, drawerEls);
          if (state.hoveredNode !== null) {
            const hoveredNode = findNode(state.hoveredNode);
            if (hoveredNode !== void 0) {
              const preview = computeHoverPreview(state.hoveredNode, state);
              applyHoverPreviewClasses(svgRoot, getNodes, getNodeId, preview);
              renderHoverPopover(
                state.tableById.get(state.hoveredNode),
                hoverEls,
                hoverPopoverPosition(hoveredNode)
              );
            } else {
              state.hoveredNode = null;
            }
          }
        }
        syncObjectBrowser();
      };
      const setSelectedNode = (tableId) => {
        const previous = state.selectedNode;
        state.selectedNode = tableId;
        state.hoveredNode = null;
        renderInteraction();
        if (previous === tableId) {
          return;
        }
        if (tableId === null) {
          emitViewerEvent("relune:node-cleared", void 0);
        } else {
          emitViewerEvent("relune:node-selected", { nodeId: tableId });
        }
      };
      const clearHoverPreview = () => {
        if (state.selectedNode !== null || state.hoveredNode === null) {
          return;
        }
        state.hoveredNode = null;
        renderInteraction();
      };
      getNodes().forEach((node) => {
        const nodeId = getNodeId(node);
        node.addEventListener("mouseenter", () => {
          if (state.selectedNode !== null) return;
          if (nodeId !== null) {
            state.hoveredNode = nodeId;
            renderInteraction();
          }
        });
        node.addEventListener("mouseleave", () => {
          if (state.selectedNode === null && state.hoveredNode === nodeId) {
            state.hoveredNode = null;
            renderInteraction();
          }
        });
        node.addEventListener("click", (event) => {
          event.stopPropagation();
          if (nodeId === null) return;
          if (state.selectedNode === nodeId) {
            setSelectedNode(null);
          } else {
            setSelectedNode(nodeId);
          }
        });
      });
      svgRoot.querySelectorAll(".edge").forEach((edgeEl) => {
        edgeEl.addEventListener("click", (event) => {
          event.stopPropagation();
          const fromId = edgeEl.getAttribute("data-from");
          if (fromId === null) return;
          navigateToTable(fromId);
        });
      });
      svgRoot.addEventListener("click", () => {
        if (state.selectedNode !== null) {
          setSelectedNode(null);
        }
      });
      drawerClose?.addEventListener("click", () => {
        setSelectedNode(null);
      });
      const handleVisibilityStateChange = () => {
        if (state.selectedNode === null && state.hoveredNode !== null) {
          state.hoveredNode = null;
          renderInteraction();
          return;
        }
        syncObjectBrowser();
      };
      searchInput?.addEventListener("input", handleVisibilityStateChange);
      document.addEventListener("relune:filters-changed", handleVisibilityStateChange);
      document.addEventListener("relune:search-changed", handleVisibilityStateChange);
      document.addEventListener("relune:groups-changed", handleVisibilityStateChange);
      document.addEventListener("relune:viewport-changed", clearHoverPreview);
      runtime.selection = {
        clear() {
          setSelectedNode(null);
        },
        select(nodeId) {
          const node = findNode(nodeId);
          if (node === void 0) return;
          setSelectedNode(nodeId);
        },
        getSelected() {
          return state.selectedNode;
        }
      };
      markViewerModuleReady("selection");
      renderInteraction();
    }
  }
})();
