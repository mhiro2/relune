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

  // ts/collapse.ts
  function setStyleCursor(el, cursor) {
    const styled = el;
    styled.style.cursor = cursor;
  }
  function setStyleDisplay(el, display) {
    const styled = el;
    styled.style.display = display;
  }
  {
    let saveState = function() {
      try {
        sessionStorage.setItem(
          "relune-collapsed-tables",
          JSON.stringify(Array.from(collapsedTables))
        );
      } catch (error) {
        reportSessionStorageError("saving collapsed tables", error);
      }
    };
    saveState2 = saveState;
    const metadata = parseReluneMetadata();
    const columnCounts = {};
    if (metadata?.tables) {
      for (const table of metadata.tables) {
        columnCounts[table.id] = table.columns?.length ?? 0;
      }
    }
    const collapsedTables = /* @__PURE__ */ new Set();
    try {
      const saved = sessionStorage.getItem("relune-collapsed-tables");
      if (saved) {
        const arr = JSON.parse(saved);
        if (Array.isArray(arr)) {
          for (const id of arr) {
            if (typeof id === "string") {
              collapsedTables.add(id);
            }
          }
        }
      }
    } catch (error) {
      reportSessionStorageError("restoring collapsed tables", error);
    }
    const canvas = document.getElementById("canvas");
    const svg = canvas?.querySelector("svg");
    if (svg) {
      let applyCollapseState = function(tableId, collapse) {
        const entry = tableNodeMap.get(tableId);
        if (entry === void 0) return;
        const tableNode = entry.node;
        const isCurrentlyCollapsed = tableNode.classList.contains("collapsed");
        if (isCurrentlyCollapsed === collapse) return;
        const collapseInd = tableNode.querySelector(".collapse-indicator");
        const badge = tableNode.querySelector(".column-count-badge");
        const rows = tableNode.querySelectorAll(".column-row, .column-name, .column-text");
        if (collapse) {
          tableNode.classList.add("collapsed");
          collapsedTables.add(tableId);
          rows.forEach((row) => setStyleDisplay(row, "none"));
          if (collapseInd) collapseInd.textContent = "+";
          if (badge) badge.style.display = "";
        } else {
          tableNode.classList.remove("collapsed");
          collapsedTables.delete(tableId);
          rows.forEach((row) => setStyleDisplay(row, ""));
          if (collapseInd) collapseInd.textContent = "-";
          if (badge) badge.style.display = "none";
        }
      };
      applyCollapseState2 = applyCollapseState;
      const tableNodes = [];
      svg.querySelectorAll(".table-node[data-table-id]").forEach((node) => {
        const id = node.getAttribute("data-table-id");
        if (id) {
          tableNodes.push({ node, id });
        }
      });
      svg.querySelectorAll("g.node[data-id]").forEach((node) => {
        const id = node.getAttribute("data-id");
        if (id) {
          tableNodes.push({ node, id });
        }
      });
      for (const entry of tableNodes) {
        const tableNode = entry.node;
        const tableId = entry.id;
        const columnCount = columnCounts[tableId] ?? 0;
        const header = tableNode.querySelector(".table-header") ?? tableNode.querySelector("rect");
        if (!header) {
          continue;
        }
        const tableNameText = tableNode.querySelector(".table-name") ?? tableNode.querySelector("text");
        setStyleCursor(header, "pointer");
        let collapseIndicator = null;
        let countBadge = null;
        if (tableNameText) {
          const headerY = parseFloat(tableNameText.getAttribute("y") ?? "") || 0;
          const tableRect = tableNode.querySelector("rect");
          const tableWidth = tableRect ? parseFloat(tableRect.getAttribute("width") ?? "") || 200 : 200;
          collapseIndicator = document.createElementNS("http://www.w3.org/2000/svg", "text");
          collapseIndicator.setAttribute("class", "collapse-indicator");
          collapseIndicator.setAttribute("x", String(tableWidth - 20));
          collapseIndicator.setAttribute("y", String(headerY));
          collapseIndicator.setAttribute("text-anchor", "middle");
          collapseIndicator.setAttribute("fill", "#64748b");
          collapseIndicator.textContent = "-";
          tableNode.appendChild(collapseIndicator);
          if (columnCount > 0) {
            countBadge = document.createElementNS("http://www.w3.org/2000/svg", "text");
            countBadge.setAttribute("class", "column-count-badge");
            countBadge.setAttribute("x", String(tableWidth - 40));
            countBadge.setAttribute("y", String(headerY));
            countBadge.setAttribute("text-anchor", "end");
            countBadge.setAttribute("fill", "#64748b");
            countBadge.textContent = `(${columnCount})`;
            countBadge.style.display = "none";
            tableNode.appendChild(countBadge);
          }
        }
        let columnRows = tableNode.querySelectorAll(".column-row, .column-name");
        if (columnRows.length === 0 && tableNameText) {
          tableNode.querySelectorAll("text").forEach((text) => {
            if (text === tableNameText) {
              return;
            }
            if (text.classList.contains("collapse-indicator") || text.classList.contains("column-count-badge")) {
              return;
            }
            text.classList.add("column-text");
          });
          columnRows = tableNode.querySelectorAll(".column-text");
        }
        if (collapsedTables.has(tableId)) {
          tableNode.classList.add("collapsed");
          Array.from(columnRows).forEach((row) => {
            setStyleDisplay(row, "none");
          });
          if (collapseIndicator) {
            collapseIndicator.textContent = "+";
          }
          if (countBadge) {
            countBadge.style.display = "";
          }
        }
        header.addEventListener("click", (e) => {
          e.stopPropagation();
          const isCollapsed = tableNode.classList.toggle("collapsed");
          if (isCollapsed) {
            collapsedTables.add(tableId);
            Array.from(columnRows).forEach((row) => {
              setStyleDisplay(row, "none");
            });
            if (collapseIndicator) {
              collapseIndicator.textContent = "+";
            }
            if (countBadge) {
              countBadge.style.display = "";
            }
          } else {
            collapsedTables.delete(tableId);
            Array.from(columnRows).forEach((row) => {
              setStyleDisplay(row, "");
            });
            if (collapseIndicator) {
              collapseIndicator.textContent = "-";
            }
            if (countBadge) {
              countBadge.style.display = "none";
            }
          }
          saveState();
          emitViewerEvent("relune:collapse-changed", void 0);
        });
      }
      const tableNodeMap = new Map(tableNodes.map((entry) => [entry.id, entry]));
      const runtime = getViewerRuntime();
      runtime.collapse = {
        getCollapsed() {
          return Array.from(collapsedTables);
        },
        setCollapsed(tableIds) {
          const target = new Set(tableIds);
          for (const id of collapsedTables) {
            if (!target.has(id)) {
              applyCollapseState(id, false);
            }
          }
          for (const id of target) {
            applyCollapseState(id, true);
          }
          saveState();
        }
      };
      markViewerModuleReady("collapse");
    }
  }
  var applyCollapseState2;
  var saveState2;
})();
