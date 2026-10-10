"use strict";
(() => {
  // ts/highlight_actions.ts
  function collectNeighborhood(nodeId, state, depth = 1) {
    const neighborIds = /* @__PURE__ */ new Set();
    const inboundNodeIds = /* @__PURE__ */ new Set();
    const outboundNodeIds = /* @__PURE__ */ new Set();
    const traversedEdgeKeys = /* @__PURE__ */ new Set();
    const visited = /* @__PURE__ */ new Set([nodeId]);
    let frontier = [nodeId];
    for (let hop = 0; hop < depth && frontier.length > 0; hop++) {
      const nextFrontier = [];
      for (const current of frontier) {
        for (const relation of state.inboundMap[current] ?? []) {
          neighborIds.add(relation.node);
          traversedEdgeKeys.add(edgeKey(relation.edge));
          if (current === nodeId) inboundNodeIds.add(relation.node);
          if (!visited.has(relation.node)) {
            visited.add(relation.node);
            nextFrontier.push(relation.node);
          }
        }
        for (const relation of state.outboundMap[current] ?? []) {
          neighborIds.add(relation.node);
          traversedEdgeKeys.add(edgeKey(relation.edge));
          if (current === nodeId) outboundNodeIds.add(relation.node);
          if (!visited.has(relation.node)) {
            visited.add(relation.node);
            nextFrontier.push(relation.node);
          }
        }
      }
      frontier = nextFrontier;
    }
    neighborIds.delete(nodeId);
    inboundNodeIds.delete(nodeId);
    outboundNodeIds.delete(nodeId);
    const connectedEdgeIndices = /* @__PURE__ */ new Set();
    state.edges.forEach((edge, index) => {
      if (traversedEdgeKeys.has(edgeKey(edge))) {
        connectedEdgeIndices.add(index);
      }
    });
    return { neighborIds, connectedEdgeIndices, inboundNodeIds, outboundNodeIds };
  }
  function edgeKey(edge) {
    return `${edge.from}\0${edge.to}\0${edge.name ?? ""}`;
  }
  function computeNeighborHighlights(nodeId, state, depth = 1) {
    return { selectedId: nodeId, ...collectNeighborhood(nodeId, state, depth) };
  }
  function computeHoverPreview(nodeId, state) {
    return { hoveredId: nodeId, ...collectNeighborhood(nodeId, state) };
  }
  function computeRelationHighlight(edgeIndex, state) {
    const edge = state.edges[edgeIndex];
    if (edge === void 0) return null;
    return {
      edgeIndex,
      fromId: edge.from,
      toId: edge.to,
      fromColumns: edge.from_columns,
      toColumns: edge.to_columns
    };
  }
  function relationColumnPairs(edge) {
    const pairCount = Math.min(edge.from_columns.length, edge.to_columns.length);
    if (pairCount === 0) {
      if (edge.from_columns.length > 0) {
        return edge.from_columns.map((column) => `${edge.from}.${column} \u2192 ${edge.to}`);
      }
      return [`${edge.from} \u2192 ${edge.to}`];
    }
    return edge.from_columns.slice(0, pairCount).map((column, index) => `${edge.from}.${column} \u2192 ${edge.to}.${edge.to_columns[index]}`);
  }

  // ts/metadata.ts
  var RISK_SEVERITY_ORDER = ["info", "warning", "caution", "breaking"];
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
  function topRisk(issues) {
    let top;
    for (const issue of issues) {
      if (top === void 0 || RISK_SEVERITY_ORDER.indexOf(issue.severity) > RISK_SEVERITY_ORDER.indexOf(top)) {
        top = issue.severity;
      }
    }
    if (top === void 0) return void 0;
    const severity = top;
    return { severity, count: issues.filter((issue) => issue.severity === severity).length };
  }
  function riskLabel(issues) {
    const top = topRisk(issues);
    return top === void 0 ? void 0 : `${top.severity} ${top.count}`;
  }
  var DIFF_MARKERS = { added: "+", removed: "\u2212", modified: "~" };
  function diffMarker(kind) {
    return DIFF_MARKERS[kind];
  }
  function tableDisplayName(table) {
    return table.label || table.table_name || table.id;
  }

  // ts/highlight_dom.ts
  var ALLOWED_DIFF_KINDS = /* @__PURE__ */ new Set(["added", "removed", "modified"]);
  var ALLOWED_SEVERITIES = /* @__PURE__ */ new Set(["breaking", "caution", "warning", "info"]);
  function safeCssToken(value, allowlist) {
    return allowlist.has(value) ? value : "";
  }
  function clearChildren(element) {
    element.replaceChildren();
  }
  function joinTableBadge() {
    const badge = document.createElement("div");
    badge.className = "detail-badge detail-badge-join";
    badge.textContent = "Join Table";
    return badge;
  }
  function diffBadge(kind) {
    const badge = document.createElement("div");
    const safe = safeCssToken(kind, ALLOWED_DIFF_KINDS);
    badge.className = safe !== "" ? `detail-diff-badge detail-diff-badge-${safe}` : "detail-diff-badge";
    badge.textContent = `${diffMarker(kind)} ${kind}`;
    return badge;
  }
  function riskBadge(issues, baseClass) {
    const top = topRisk(issues);
    const label = riskLabel(issues);
    if (top === void 0 || label === void 0) return void 0;
    const badge = document.createElement("span");
    const safe = safeCssToken(top.severity, ALLOWED_SEVERITIES);
    badge.className = safe !== "" ? `${baseClass} ${baseClass}-${safe}` : baseClass;
    badge.textContent = label;
    badge.title = `${issues.length} risk${issues.length === 1 ? "" : "s"}`;
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
  var SVG_NS = "http://www.w3.org/2000/svg";
  var ROW_HEIGHT = 22;
  var ROW_BASELINE = 15;
  var PORT_RADIUS = 3.5;
  var HIGHLIGHT_CLASSES = [
    "highlighted-neighbor",
    "dimmed-by-highlight",
    "selected-node",
    "inbound",
    "outbound",
    "hover-preview-node",
    "hover-preview-neighbor",
    "hover-inbound",
    "hover-outbound",
    "hover-preview-edge",
    "selected-edge",
    "relation-endpoint",
    "relation-column"
  ];
  function svgElement(name, className, attributes) {
    const element = document.createElementNS(SVG_NS, name);
    element.setAttribute("class", className);
    for (const [key, value] of Object.entries(attributes)) {
      element.setAttribute(key, String(value));
    }
    return element;
  }
  function numericAttribute(element, name) {
    return Number.parseFloat(element?.getAttribute(name) ?? "") || 0;
  }
  function pathEndpoints(d) {
    const numbers = (d.match(/-?\d+(?:\.\d+)?/g) ?? []).map(Number);
    if (numbers.length < 4) return null;
    return [
      [numbers[0] ?? 0, numbers[1] ?? 0],
      [numbers[numbers.length - 2] ?? 0, numbers[numbers.length - 1] ?? 0]
    ];
  }
  function createHighlightPainter(targets) {
    const touched = /* @__PURE__ */ new Set();
    const decorations = [];
    const mark = (element, ...classes) => {
      element.classList.add(...classes);
      touched.add(element);
    };
    const markColumns = (nodeId, columns) => {
      const node = targets.nodesById.get(nodeId);
      if (node === void 0 || columns.length === 0) return;
      const body = node.querySelector(".table-body");
      node.querySelectorAll(".column-row").forEach((row) => {
        if (!columns.includes(row.getAttribute("data-column-name") ?? "")) return;
        mark(row, "relation-column");
        const band = svgElement("rect", "relation-column-band", {
          x: numericAttribute(body, "x") + 1,
          y: numericAttribute(row.querySelector(".column-name"), "y") - ROW_BASELINE,
          width: Math.max(numericAttribute(body, "width") - 2, 0),
          height: ROW_HEIGHT
        });
        row.prepend(band);
        decorations.push(band);
      });
    };
    const markPorts = (edge) => {
      const endpoints = pathEndpoints(edge.querySelector(".edge-path")?.getAttribute("d") ?? "");
      if (endpoints === null) return;
      for (const [cx, cy] of endpoints) {
        const port = svgElement("circle", "relation-port", { cx, cy, r: PORT_RADIUS });
        edge.append(port);
        decorations.push(port);
      }
    };
    const directionClasses = (id, inbound, outbound, [inboundClass, outboundClass]) => {
      const isInbound = inbound.has(id);
      const isOutbound = outbound.has(id);
      if (isInbound && !isOutbound) return [inboundClass];
      if (isOutbound && !isInbound) return [outboundClass];
      return [];
    };
    return {
      clear() {
        for (const element of touched) {
          element.classList.remove(...HIGHLIGHT_CLASSES);
        }
        touched.clear();
        for (const decoration of decorations) {
          decoration.remove();
        }
        decorations.length = 0;
      },
      applyRelation(relation) {
        targets.nodesById.forEach((node, id) => {
          const isEndpoint = id === relation.fromId || id === relation.toId;
          mark(node, isEndpoint ? "relation-endpoint" : "dimmed-by-highlight");
        });
        targets.edges.forEach((edge, index) => {
          mark(edge, index === relation.edgeIndex ? "selected-edge" : "dimmed-by-highlight");
        });
        markColumns(relation.fromId, relation.fromColumns);
        markColumns(relation.toId, relation.toColumns);
        const selected = targets.edges[relation.edgeIndex];
        if (selected !== void 0) markPorts(selected);
      },
      applySelected(highlight) {
        targets.nodesById.forEach((node, id) => {
          if (id === highlight.selectedId) {
            mark(node, "selected-node");
          } else if (highlight.neighborIds.has(id)) {
            mark(
              node,
              "highlighted-neighbor",
              ...directionClasses(id, highlight.inboundNodeIds, highlight.outboundNodeIds, [
                "inbound",
                "outbound"
              ])
            );
          } else {
            mark(node, "dimmed-by-highlight");
          }
        });
        targets.edges.forEach((edge, index) => {
          mark(
            edge,
            highlight.connectedEdgeIndices.has(index) ? "highlighted-neighbor" : "dimmed-by-highlight"
          );
        });
      },
      applyHoverPreview(preview) {
        const hovered = targets.nodesById.get(preview.hoveredId);
        if (hovered !== void 0) mark(hovered, "hover-preview-node");
        for (const id of preview.neighborIds) {
          const node = targets.nodesById.get(id);
          if (node === void 0) continue;
          mark(
            node,
            "hover-preview-neighbor",
            ...directionClasses(id, preview.inboundNodeIds, preview.outboundNodeIds, [
              "hover-inbound",
              "hover-outbound"
            ])
          );
        }
        for (const index of preview.connectedEdgeIndices) {
          const edge = targets.edges[index];
          if (edge !== void 0) mark(edge, "hover-preview-edge");
        }
      }
    };
  }
  var RELATION_KIND_LABELS = {
    foreign_key: "Foreign key",
    enum_reference: "Enum reference",
    view_dependency: "View dependency"
  };
  function renderRelationCard(edge, tableById, elements) {
    if (edge === void 0) {
      elements.card.setAttribute("hidden", "");
      clearChildren(elements.pairs);
      return;
    }
    const label = (id) => {
      const table = tableById.get(id);
      return table === void 0 ? id : tableDisplayName(table);
    };
    elements.card.removeAttribute("hidden");
    elements.kind.textContent = RELATION_KIND_LABELS[edge.kind] ?? edge.kind;
    elements.title.textContent = `${label(edge.from)} \u2192 ${label(edge.to)}`;
    elements.name.textContent = edge.name ?? "";
    elements.name.toggleAttribute("hidden", edge.name == null || edge.name === "");
    clearChildren(elements.pairs);
    for (const pair of relationColumnPairs(edge)) {
      const item = document.createElement("li");
      item.textContent = pair;
      elements.pairs.appendChild(item);
    }
    elements.openFrom.textContent = `Open ${label(edge.from)}`;
    elements.openTo.textContent = `Open ${label(edge.to)}`;
  }
  function renderDrawer(table, state, elements, onSelectRelation) {
    if (table === void 0) {
      elements.drawer.setAttribute("hidden", "");
      clearChildren(elements.titleBadges);
      clearChildren(elements.metrics);
      clearChildren(elements.columns);
      clearChildren(elements.relations);
      if (elements.changes) clearChildren(elements.changes);
      if (elements.changesSection) elements.changesSection.setAttribute("hidden", "");
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
    clearChildren(elements.titleBadges);
    if (table.diff_kind) {
      elements.titleBadges.append(diffBadge(table.diff_kind));
    }
    if (table.is_join_table_candidate) {
      elements.titleBadges.append(joinTableBadge());
    }
    clearChildren(elements.metrics);
    const totalRelations = table.inbound_count + table.outbound_count;
    elements.metrics.append(
      metricCard("Columns", String(table.columns.length)),
      metricCard("Relations", String(totalRelations)),
      metricCard("\u2190 In", String(table.inbound_count)),
      metricCard("Out \u2192", String(table.outbound_count))
    );
    if (elements.changes instanceof HTMLElement && elements.changesSection instanceof HTMLElement) {
      clearChildren(elements.changes);
      const details = table.diff_details ?? [];
      if (details.length === 0) {
        elements.changesSection.setAttribute("hidden", "");
      } else {
        elements.changesSection.removeAttribute("hidden");
        for (const detail of details) {
          const item = document.createElement("li");
          item.className = "detail-change";
          item.textContent = detail;
          elements.changes.appendChild(item);
        }
      }
    }
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
          buildRelationElement(relation.edge, relation.node, state.tableById, onSelectRelation)
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
    const badge = riskBadge(table.issues ?? [], "hover-popover-badge");
    if (badge !== void 0) {
      elements.badges.appendChild(badge);
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
    const dataType = column.data_type || "unknown";
    typePill.textContent = column.previous_data_type != null ? `${column.previous_data_type} \u2192 ${dataType}` : dataType;
    pills.appendChild(typePill);
    const nullPill = document.createElement("span");
    nullPill.className = `detail-column-pill ${column.nullable ? "detail-column-pill-nullable" : "detail-column-pill-required"}`;
    nullPill.textContent = column.nullable ? "nullable" : "required";
    pills.appendChild(nullPill);
    if (column.diff_kind) {
      const diffPill = document.createElement("span");
      const safeDiff = safeCssToken(column.diff_kind, ALLOWED_DIFF_KINDS);
      diffPill.className = safeDiff !== "" ? `detail-column-pill detail-column-pill-diff detail-column-pill-diff-${safeDiff}` : "detail-column-pill detail-column-pill-diff";
      diffPill.textContent = `${diffMarker(column.diff_kind)} ${column.diff_kind}`;
      pills.appendChild(diffPill);
    }
    columnEl.append(name, pills);
    return columnEl;
  }
  function buildRelationElement(edge, targetNodeId, tableById, onSelectRelation) {
    const targetTable = tableById.get(targetNodeId);
    const targetName = targetTable?.label ?? targetNodeId;
    const label = document.createElement("span");
    label.className = "detail-relation-label";
    label.textContent = edge.name ?? `${edge.from} \u2192 ${edge.to}`;
    const meta = document.createElement("span");
    meta.className = "detail-relation-meta";
    const columnMap = edge.from_columns.length > 0 && edge.to_columns.length > 0 ? ` \xB7 ${edge.from_columns.join(", ")} \u2192 ${edge.to_columns.join(", ")}` : "";
    meta.textContent = `${edge.kind} \xB7 ${targetName}${columnMap}`;
    if (onSelectRelation) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "detail-relation detail-relation-navigable";
      btn.setAttribute("aria-label", `Show relationship ${relationColumnPairs(edge).join(", ")}`);
      btn.addEventListener("click", () => {
        onSelectRelation(edge);
      });
      btn.append(label, meta);
      return btn;
    }
    const div = document.createElement("div");
    div.className = "detail-relation";
    div.append(label, meta);
    return div;
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
  function createObjectBrowser(listEl, countEl, emptyEl, onSelect) {
    const buttons = /* @__PURE__ */ new Map();
    return {
      render(items, totalCount) {
        countEl.textContent = `${items.length}/${totalCount}`;
        emptyEl.toggleAttribute("hidden", items.length > 0);
        listEl.replaceChildren(
          ...items.map((item) => {
            let button = buttons.get(item.table.id);
            if (button === void 0) {
              button = buildObjectBrowserButton(item.table, onSelect);
              buttons.set(item.table.id, button);
            }
            button.classList.toggle("selected", item.isSelected);
            button.classList.toggle("filtered-out", item.isDimmedBySearch || item.isExcludedByFilter);
            button.classList.toggle("hidden-item", item.isHiddenByGroup);
            return button;
          })
        );
      }
    };
  }
  function buildObjectBrowserButton(table, onSelect) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "object-browser-item";
    const header = document.createElement("div");
    header.className = "object-browser-item-header";
    const name = document.createElement("span");
    name.className = "object-browser-item-name";
    name.textContent = table.label || table.table_name || table.id;
    const kind = document.createElement("span");
    kind.className = "object-browser-kind";
    kind.textContent = table.kind;
    const issueBadge = riskBadge(table.issues ?? [], "object-browser-issue-badge");
    if (issueBadge !== void 0) {
      header.append(name, issueBadge, kind);
    } else {
      header.append(name, kind);
    }
    const meta = document.createElement("div");
    meta.className = "object-browser-item-meta";
    const counts = document.createElement("span");
    counts.textContent = `${table.columns.length} cols`;
    const relations = document.createElement("span");
    relations.textContent = `${table.inbound_count} in / ${table.outbound_count} out`;
    meta.append(counts, relations);
    button.append(header, meta);
    button.addEventListener("click", () => {
      onSelect(table.id);
    });
    return button;
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
    return {
      hoveredNode: null,
      selectedNode: null,
      selectedEdge: null,
      traversalDepth: 1,
      tableById,
      inboundMap,
      outboundMap,
      edges
    };
  }

  // ts/search_actions.ts
  function matchesTableQuery(table, query) {
    const needle = query.trim().toLowerCase();
    if (needle === "") {
      return true;
    }
    const includes = (value) => (value ?? "").toLowerCase().includes(needle);
    return includes(tableDisplayName(table)) || includes(table.id) || includes(table.table_name) || includes(table.schema_name) || table.columns.some((column) => includes(column.name) || includes(column.data_type));
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
    const traversalEl = document.getElementById("detail-traversal");
    const viewport = document.getElementById("viewport");
    const drawerEls = (() => {
      const drawer = document.getElementById("detail-drawer");
      const title = document.getElementById("detail-title");
      const titleBadges = document.getElementById("detail-title-badges");
      const kind = document.getElementById("detail-kind");
      const subtitle = document.getElementById("detail-subtitle");
      const metrics = document.getElementById("detail-metrics");
      const columns = document.getElementById("detail-columns");
      const columnsEmpty = document.getElementById("detail-columns-empty");
      const relations = document.getElementById("detail-relations");
      const relationsEmpty = document.getElementById("detail-relationships-empty");
      if (drawer instanceof HTMLElement && title instanceof HTMLElement && titleBadges instanceof HTMLElement && kind instanceof HTMLElement && subtitle instanceof HTMLElement && metrics instanceof HTMLElement && columns instanceof HTMLElement && columnsEmpty instanceof HTMLElement && relations instanceof HTMLElement && relationsEmpty instanceof HTMLElement) {
        return {
          drawer,
          title,
          titleBadges,
          kind,
          subtitle,
          metrics,
          columns,
          columnsEmpty,
          relations,
          relationsEmpty,
          changesSection: document.getElementById("detail-changes-section"),
          changes: document.getElementById("detail-changes"),
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
    const relationEls = (() => {
      const card = document.getElementById("relation-card");
      const kind = document.getElementById("relation-card-kind");
      const title = document.getElementById("relation-card-title");
      const name = document.getElementById("relation-card-name");
      const pairs = document.getElementById("relation-card-pairs");
      const openFrom = document.getElementById("relation-card-open-from");
      const openTo = document.getElementById("relation-card-open-to");
      if (card instanceof HTMLElement && kind instanceof HTMLElement && title instanceof HTMLElement && name instanceof HTMLElement && pairs instanceof HTMLElement && openFrom instanceof HTMLButtonElement && openTo instanceof HTMLButtonElement) {
        return { card, kind, title, name, pairs, openFrom, openTo };
      }
      return null;
    })();
    if (svgRoot && drawerEls && hoverEls) {
      const runtime = getViewerRuntime();
      const nodesById = /* @__PURE__ */ new Map();
      svgRoot.querySelectorAll(".node[data-id], .table-node[data-table-id]").forEach((node) => {
        const id = node.getAttribute("data-id") ?? node.getAttribute("data-table-id");
        if (id !== null && !nodesById.has(id)) nodesById.set(id, node);
      });
      const edgeEls = Array.from(svgRoot.querySelectorAll(".edge"));
      const painter = createHighlightPainter({ nodesById, edges: edgeEls });
      const findNode = (nodeId) => nodesById.get(nodeId);
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
      const objectBrowser = objectBrowserList instanceof HTMLElement && objectBrowserCount instanceof HTMLElement && objectBrowserEmpty instanceof HTMLElement ? createObjectBrowser(
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
      ) : null;
      const syncObjectBrowser = () => {
        if (objectBrowser === null) {
          return;
        }
        const query = searchInput instanceof HTMLInputElement ? searchInput.value : "";
        const visibleTables = tables.filter((table) => matchesTableQuery(table, query));
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
        objectBrowser.render(items, tables.length);
      };
      const syncTraversalButtons = () => {
        traversalEl?.querySelectorAll(".detail-traversal-btn").forEach((btn) => {
          const depth = Number(btn.dataset["depth"]);
          btn.classList.toggle("active", depth === state.traversalDepth);
        });
      };
      traversalEl?.addEventListener("click", (event) => {
        const target = event.target;
        if (!(target instanceof HTMLButtonElement) || target.dataset["depth"] === void 0) return;
        const depth = Number(target.dataset["depth"]);
        if (depth >= 1 && depth <= 2 && depth !== state.traversalDepth) {
          state.traversalDepth = depth;
          renderHighlight();
        }
      });
      const renderHighlight = () => {
        painter.clear();
        hideHoverPopover(hoverEls);
        const relation = state.selectedEdge === null ? null : computeRelationHighlight(state.selectedEdge, state);
        if (relationEls !== null) {
          renderRelationCard(
            relation === null ? void 0 : state.edges[relation.edgeIndex],
            state.tableById,
            relationEls
          );
        }
        if (relation !== null) {
          painter.applyRelation(relation);
          renderDrawer(void 0, state, drawerEls);
          traversalEl?.setAttribute("hidden", "");
        } else if (state.selectedNode !== null) {
          const highlight = computeNeighborHighlights(
            state.selectedNode,
            state,
            state.traversalDepth
          );
          painter.applySelected(highlight);
          renderDrawer(state.tableById.get(state.selectedNode), state, drawerEls, (edge) => {
            selectRelation(edge, true);
          });
          traversalEl?.removeAttribute("hidden");
          syncTraversalButtons();
        } else {
          renderDrawer(void 0, state, drawerEls);
          traversalEl?.setAttribute("hidden", "");
          if (state.hoveredNode !== null) {
            const hoveredNode = findNode(state.hoveredNode);
            if (hoveredNode !== void 0) {
              const preview = computeHoverPreview(state.hoveredNode, state);
              painter.applyHoverPreview(preview);
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
      };
      const renderInteraction = () => {
        renderHighlight();
        syncObjectBrowser();
      };
      const setSelectedNode = (tableId) => {
        const previous = state.selectedNode;
        state.selectedNode = tableId;
        state.selectedEdge = null;
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
      let relationOrigin = null;
      const setSelectedEdge = (edgeIndex, focusCard = false) => {
        const hadTable = state.selectedNode !== null;
        state.selectedNode = null;
        state.selectedEdge = edgeIndex;
        state.hoveredNode = null;
        relationOrigin = null;
        renderInteraction();
        if (hadTable) emitViewerEvent("relune:node-cleared", void 0);
        if (focusCard && edgeIndex !== null) relationEls?.card.focus();
      };
      const selectRelation = (edge, focusCard = false) => {
        const index = state.edges.indexOf(edge);
        if (index < 0) return;
        const origin = state.selectedNode;
        setSelectedEdge(index, focusCard);
        relationOrigin = origin;
      };
      const openTableDrawer = (tableId) => {
        navigateToTable(tableId);
        drawerEls.drawer.focus();
      };
      const clearHoverPreview = () => {
        if (state.selectedNode !== null || state.hoveredNode === null) {
          return;
        }
        state.hoveredNode = null;
        renderHighlight();
      };
      nodesById.forEach((node, nodeId) => {
        node.addEventListener("mouseenter", () => {
          if (state.selectedNode !== null) return;
          state.hoveredNode = nodeId;
          renderHighlight();
        });
        node.addEventListener("mouseleave", () => {
          if (state.selectedNode === null && state.hoveredNode === nodeId) {
            state.hoveredNode = null;
            renderHighlight();
          }
        });
        node.addEventListener("click", (event) => {
          event.stopPropagation();
          if (state.selectedNode === nodeId) {
            setSelectedNode(null);
          } else {
            setSelectedNode(nodeId);
          }
        });
      });
      edgeEls.forEach((edgeEl, index) => {
        const toggle = (focusCard) => {
          setSelectedEdge(state.selectedEdge === index ? null : index, focusCard);
        };
        const edge = state.edges[index];
        if (edge !== void 0) {
          edgeEl.setAttribute("tabindex", "0");
          edgeEl.setAttribute("role", "button");
          edgeEl.setAttribute("aria-label", `Relationship ${relationColumnPairs(edge).join(", ")}`);
        }
        edgeEl.addEventListener("click", (event) => {
          event.stopPropagation();
          toggle(false);
        });
        edgeEl.addEventListener("keydown", (event) => {
          const { key } = event;
          if (key !== "Enter" && key !== " ") return;
          event.preventDefault();
          event.stopPropagation();
          toggle(true);
        });
      });
      svgRoot.addEventListener("click", () => {
        if (state.selectedNode !== null || state.selectedEdge !== null) {
          setSelectedNode(null);
        }
      });
      const openRelationEnd = (end) => {
        const edge = state.selectedEdge === null ? void 0 : state.edges[state.selectedEdge];
        if (edge !== void 0) openTableDrawer(edge[end]);
      };
      relationEls?.openFrom.addEventListener("click", () => {
        openRelationEnd("from");
      });
      relationEls?.openTo.addEventListener("click", () => {
        openRelationEnd("to");
      });
      const closeRelationCard = () => {
        const index = state.selectedEdge;
        if (relationOrigin !== null) {
          openTableDrawer(relationOrigin);
          return;
        }
        setSelectedEdge(null);
        if (index !== null) edgeEls[index]?.focus();
      };
      document.getElementById("relation-card-close")?.addEventListener("click", closeRelationCard);
      relationEls?.card.addEventListener("keydown", (event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        closeRelationCard();
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
