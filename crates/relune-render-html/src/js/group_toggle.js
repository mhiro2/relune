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

  // ts/group_toggle_dom.ts
  function buildGroupListDOM(groups, container, onChange) {
    container.innerHTML = "";
    for (const group of groups) {
      const item = document.createElement("div");
      item.className = "group-item";
      item.setAttribute("data-group-id", group.id);
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.id = `group-${group.id}`;
      checkbox.name = "relune-group-visibility";
      checkbox.checked = true;
      const label = document.createElement("label");
      label.setAttribute("for", `group-${group.id}`);
      label.textContent = group.label || group.id;
      const count = document.createElement("span");
      count.className = "count";
      count.textContent = `(${group.table_ids?.length ?? 0})`;
      item.appendChild(checkbox);
      item.appendChild(label);
      item.appendChild(count);
      checkbox.addEventListener("change", () => {
        onChange(group.id, checkbox.checked);
      });
      container.appendChild(item);
    }
  }
  function applyGroupVisibility(svg, tableIds, visible) {
    for (const tableId of tableIds) {
      const node = svg.querySelector(`.node[data-id="${CSS.escape(tableId)}"]`);
      if (node) {
        node.classList.toggle("hidden-by-group", !visible);
      }
    }
  }
  function updateEdgeVisibility(svg, isNodeHidden) {
    svg.querySelectorAll(".edge").forEach((edge) => {
      const fromId = edge.getAttribute("data-from");
      const toId = edge.getAttribute("data-to");
      const fromHidden = fromId ? isNodeHidden(fromId) : false;
      const toHidden = toId ? isNodeHidden(toId) : false;
      edge.classList.toggle("hidden-by-group", fromHidden || toHidden);
    });
  }
  function syncGroupItemClass(groupId, visible) {
    const groupItem = document.querySelector(`.group-item[data-group-id="${CSS.escape(groupId)}"]`);
    if (groupItem) {
      groupItem.classList.toggle("hidden-group", !visible);
    }
  }

  // ts/group_toggle.ts
  {
    const metadata = parseReluneMetadata();
    if (metadata) {
      const groups = metadata.groups ?? [];
      const groupPanel = document.getElementById("group-panel");
      const groupList = document.getElementById("group-list");
      if (groups.length === 0) {
        if (groupPanel) {
          groupPanel.style.display = "none";
        }
      } else {
        let applyPanelCollapsed = function(collapsed) {
          if (!groupPanel || !collapseBtn) return;
          groupPanel.classList.toggle("group-panel-collapsed", collapsed);
          collapseBtn.setAttribute("aria-expanded", collapsed ? "false" : "true");
          collapseBtn.textContent = collapsed ? "\u25B8" : "\u25BE";
        }, isNodeHidden = function(nodeId) {
          for (const groupId of Object.keys(groupTableMap)) {
            if (!visibleGroups[groupId]) {
              const tableIds = groupTableMap[groupId];
              if (tableIds?.includes(nodeId)) return true;
            }
          }
          return false;
        }, toggleGroup = function(groupId, visible) {
          visibleGroups[groupId] = visible;
          const svg = document.querySelector(".canvas svg");
          if (!svg) return;
          applyGroupVisibility(svg, groupTableMap[groupId] ?? [], visible);
          updateEdgeVisibility(svg, isNodeHidden);
          syncGroupItemClass(groupId, visible);
          emitViewerEvent("relune:groups-changed", {
            visibleGroups: { ...visibleGroups }
          });
        }, showAllGroups = function() {
          for (const group of groups) {
            const checkbox = document.getElementById(`group-${group.id}`);
            if (checkbox instanceof HTMLInputElement && !checkbox.checked) {
              checkbox.checked = true;
              toggleGroup(group.id, true);
            }
          }
        }, hideAllGroups = function() {
          for (const group of groups) {
            const checkbox = document.getElementById(`group-${group.id}`);
            if (checkbox instanceof HTMLInputElement && checkbox.checked) {
              checkbox.checked = false;
              toggleGroup(group.id, false);
            }
          }
        };
        applyPanelCollapsed2 = applyPanelCollapsed, isNodeHidden2 = isNodeHidden, toggleGroup2 = toggleGroup, showAllGroups2 = showAllGroups, hideAllGroups2 = hideAllGroups;
        const collapseBtn = document.getElementById("group-panel-collapse");
        const COLLAPSE_KEY = "relune-group-panel-collapsed";
        const sessionStorageRef = getSessionStorage();
        collapseBtn?.addEventListener("click", () => {
          const next = !groupPanel?.classList.contains("group-panel-collapsed");
          applyPanelCollapsed(next);
          if (sessionStorageRef === null) {
            return;
          }
          try {
            sessionStorageRef.setItem(COLLAPSE_KEY, next ? "1" : "0");
          } catch (error) {
            reportSessionStorageError("saving the group panel state", error);
          }
        });
        try {
          if (sessionStorageRef?.getItem(COLLAPSE_KEY) === "1") {
            applyPanelCollapsed(true);
          }
        } catch (error) {
          reportSessionStorageError("restoring the group panel state", error);
        }
        const groupTableMap = {};
        for (const group of groups) {
          groupTableMap[group.id] = group.table_ids ?? [];
        }
        const visibleGroups = {};
        for (const group of groups) {
          visibleGroups[group.id] = true;
        }
        const showAllBtn = document.getElementById("show-all-groups");
        const hideAllBtn = document.getElementById("hide-all-groups");
        showAllBtn?.addEventListener("click", showAllGroups);
        hideAllBtn?.addEventListener("click", hideAllGroups);
        const runtime = getViewerRuntime();
        runtime.groups = {
          setVisibility(groupId, visible) {
            const checkbox = document.getElementById(`group-${groupId}`);
            if (checkbox instanceof HTMLInputElement && checkbox.checked !== visible) {
              checkbox.checked = visible;
              toggleGroup(groupId, visible);
            }
          },
          getHiddenGroups() {
            return groups.filter((group) => visibleGroups[group.id] === false).map((group) => group.id);
          }
        };
        markViewerModuleReady("groups");
        if (groupList) {
          buildGroupListDOM(groups, groupList, toggleGroup);
        }
      }
    }
  }
  var applyPanelCollapsed2;
  var isNodeHidden2;
  var toggleGroup2;
  var showAllGroups2;
  var hideAllGroups2;
})();
