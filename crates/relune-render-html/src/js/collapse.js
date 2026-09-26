"use strict";
(() => {
  // ts/collapse_dom.ts
  var SVG_NS = "http://www.w3.org/2000/svg";
  var HEADER_SELECTOR = ".table-header, .table-header-fade, .table-name, .table-kind, .collapse-indicator";
  var KIND_LABEL_RESERVE = 44;
  var INDICATOR_WIDTH = 16;
  var INDICATOR_GAP = 4;
  var MIN_NAME_CLIP_WIDTH = 24;
  var FIRST_ROW_BASELINE = 46;
  var EXPANDED_GLYPH = "\u25BE";
  var COLLAPSED_GLYPH = "\u25B8";
  function numericAttribute(el, name) {
    return Number.parseFloat(el.getAttribute(name) ?? "") || 0;
  }
  function createSvgText(className, x, y, text) {
    const el = document.createElementNS(SVG_NS, "text");
    el.setAttribute("class", className);
    el.setAttribute("x", String(x));
    el.setAttribute("y", String(y));
    el.textContent = text;
    return el;
  }
  function shrinkNameClip(node) {
    const clipRef = node.querySelector(".table-name")?.getAttribute("clip-path") ?? "";
    const clipId = /^url\(#(.+)\)$/.exec(clipRef)?.[1];
    if (clipId === void 0) return;
    const clipRect = node.querySelector(`clipPath[id="${CSS.escape(clipId)}"] rect`);
    if (clipRect === null) return;
    const width = numericAttribute(clipRect, "width");
    clipRect.setAttribute(
      "width",
      String(Math.max(width - INDICATOR_WIDTH - INDICATOR_GAP * 2, MIN_NAME_CLIP_WIDTH))
    );
  }
  function decorateTable(node, header, columnCount) {
    const x = numericAttribute(header, "x");
    const y = numericAttribute(header, "y");
    const width = numericAttribute(header, "width");
    const headerHeight = numericAttribute(header, "height");
    const indicator = createSvgText(
      "collapse-indicator",
      x + width - KIND_LABEL_RESERVE - INDICATOR_GAP - INDICATOR_WIDTH / 2,
      y + headerHeight / 2,
      EXPANDED_GLYPH
    );
    indicator.setAttribute("text-anchor", "middle");
    indicator.setAttribute("dominant-baseline", "central");
    node.appendChild(indicator);
    shrinkNameClip(node);
    if (columnCount > 0) {
      const label = `${columnCount} ${columnCount === 1 ? "column" : "columns"} hidden`;
      node.appendChild(createSvgText("column-count-badge", x + 10, y + FIRST_ROW_BASELINE, label));
    }
    return indicator;
  }
  function createCollapseController(svg, options) {
    const tables = /* @__PURE__ */ new Map();
    const collapsed = /* @__PURE__ */ new Set();
    const apply = (tableId, collapse) => {
      const table = tables.get(tableId);
      if (table === void 0) return;
      table.node.classList.toggle("collapsed", collapse);
      table.indicator.textContent = collapse ? COLLAPSED_GLYPH : EXPANDED_GLYPH;
      if (collapse) {
        collapsed.add(tableId);
      } else {
        collapsed.delete(tableId);
      }
    };
    svg.querySelectorAll(".table-node[data-table-id]").forEach((node) => {
      const tableId = node.getAttribute("data-table-id");
      const header = node.querySelector(".table-header");
      if (tableId === null || header === null || tables.has(tableId)) return;
      const indicator = decorateTable(node, header, options.columnCounts.get(tableId) ?? 0);
      tables.set(tableId, { node, indicator });
      const onClick = (event) => {
        event.stopPropagation();
        apply(tableId, !collapsed.has(tableId));
        options.onToggle();
      };
      node.querySelectorAll(HEADER_SELECTOR).forEach((el) => {
        el.addEventListener("click", onClick);
      });
    });
    for (const tableId of options.initiallyCollapsed) {
      apply(tableId, true);
    }
    return {
      getCollapsed() {
        return Array.from(collapsed);
      },
      setCollapsed(tableIds) {
        const target = new Set(tableIds);
        for (const tableId of Array.from(collapsed)) {
          if (!target.has(tableId)) apply(tableId, false);
        }
        for (const tableId of target) {
          apply(tableId, true);
        }
      }
    };
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
  function noticeStack() {
    const existing = document.getElementById("relune-viewer-notices");
    if (existing instanceof HTMLElement) {
      return existing;
    }
    const stack = document.createElement("div");
    stack.id = "relune-viewer-notices";
    stack.className = "viewer-notice-stack";
    document.body.appendChild(stack);
    return stack;
  }
  function showViewerNotice(message, severity = "warning") {
    const item = document.createElement("div");
    item.className = `viewer-notice viewer-notice-${severity}`;
    item.setAttribute("role", severity === "warning" ? "alert" : "status");
    item.textContent = message;
    noticeStack().appendChild(item);
    window.setTimeout(() => {
      item.remove();
    }, 4500);
  }
  function reportSessionStorageError(action, error) {
    const isSecurityError = error instanceof DOMException && error.name === "SecurityError";
    if (isSecurityError) {
      return;
    }
    const isQuotaExceeded = error instanceof DOMException && (error.name === "QuotaExceededError" || error.name === "NS_ERROR_DOM_QUOTA_REACHED");
    if (isQuotaExceeded) {
      showViewerNotice(
        `Session storage is full while ${action}. Viewer state was not saved.`,
        "warning"
      );
      return;
    }
    console.warn(`Session storage error while ${action}`, error);
  }
  function getSessionStorage() {
    try {
      return window.sessionStorage;
    } catch (error) {
      reportSessionStorageError("accessing session storage", error);
      return null;
    }
  }

  // ts/collapse.ts
  var STORAGE_KEY = "relune-collapsed-tables";
  {
    let loadState2 = function() {
      try {
        const saved = sessionStorageRef?.getItem(STORAGE_KEY);
        if (saved) {
          const arr = JSON.parse(saved);
          if (Array.isArray(arr)) {
            return arr.filter((id) => typeof id === "string");
          }
        }
      } catch (error) {
        reportSessionStorageError("restoring collapsed tables", error);
      }
      return [];
    }, saveState2 = function(tableIds) {
      if (sessionStorageRef === null) {
        return;
      }
      try {
        sessionStorageRef.setItem(STORAGE_KEY, JSON.stringify(tableIds));
      } catch (error) {
        reportSessionStorageError("saving collapsed tables", error);
      }
    };
    loadState = loadState2, saveState = saveState2;
    const metadata = parseReluneMetadata();
    const columnCounts = new Map(
      (metadata?.tables ?? []).map((table) => [table.id, table.columns?.length ?? 0])
    );
    const sessionStorageRef = getSessionStorage();
    const svg = document.getElementById("canvas")?.querySelector("svg");
    if (svg) {
      const controller = createCollapseController(svg, {
        columnCounts,
        initiallyCollapsed: loadState2(),
        onToggle: () => {
          saveState2(controller.getCollapsed());
          emitViewerEvent("relune:collapse-changed", void 0);
        }
      });
      const runtime = getViewerRuntime();
      runtime.collapse = {
        getCollapsed() {
          return controller.getCollapsed();
        },
        setCollapsed(tableIds) {
          controller.setCollapsed(tableIds);
          saveState2(controller.getCollapsed());
        }
      };
      markViewerModuleReady("collapse");
    }
  }
  var loadState;
  var saveState;
})();
