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
  function getViewerRuntime() {
    if (window.reluneViewer === void 0) {
      window.reluneViewer = {};
    }
    return window.reluneViewer;
  }
  function emitViewerEvent(name, detail) {
    document.dispatchEvent(new CustomEvent(name, { detail }));
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
          if (!groupPanel || !collapseBtn) {
            return;
          }
          groupPanel.classList.toggle("group-panel-collapsed", collapsed);
          collapseBtn.setAttribute("aria-expanded", collapsed ? "false" : "true");
          collapseBtn.textContent = collapsed ? "\u25B8" : "\u25BE";
        }, buildGroupList = function() {
          if (!groupList) {
            return;
          }
          groupList.innerHTML = "";
          for (const group of groups) {
            const item = document.createElement("div");
            item.className = "group-item";
            item.setAttribute("data-group-id", group.id);
            const checkbox = document.createElement("input");
            checkbox.type = "checkbox";
            checkbox.id = `group-${group.id}`;
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
              toggleGroup(group.id, checkbox.checked);
            });
            groupList.appendChild(item);
          }
        }, toggleGroup = function(groupId, visible) {
          visibleGroups[groupId] = visible;
          const tableIds = groupTableMap[groupId] ?? [];
          const svg = document.querySelector(".canvas svg");
          if (!svg) {
            return;
          }
          for (const tableId of tableIds) {
            const node = svg.querySelector(`.node[data-id="${CSS.escape(tableId)}"]`);
            if (node) {
              if (visible) {
                node.classList.remove("hidden-by-group");
              } else {
                node.classList.add("hidden-by-group");
              }
            }
          }
          updateEdgeVisibility();
          const groupItem = document.querySelector(
            `.group-item[data-group-id="${CSS.escape(groupId)}"]`
          );
          if (groupItem) {
            if (visible) {
              groupItem.classList.remove("hidden-group");
            } else {
              groupItem.classList.add("hidden-group");
            }
          }
          emitViewerEvent("relune:groups-changed", {
            visibleGroups: { ...visibleGroups }
          });
        }, updateEdgeVisibility = function() {
          const svg = document.querySelector(".canvas svg");
          if (!svg) {
            return;
          }
          const edges = svg.querySelectorAll(".edge");
          edges.forEach((edge) => {
            const fromId = edge.getAttribute("data-from");
            const toId = edge.getAttribute("data-to");
            const fromHidden = fromId ? isNodeHidden(fromId) : false;
            const toHidden = toId ? isNodeHidden(toId) : false;
            if (fromHidden || toHidden) {
              edge.classList.add("hidden-by-group");
            } else {
              edge.classList.remove("hidden-by-group");
            }
          });
        }, isNodeHidden = function(nodeId) {
          for (const groupId of Object.keys(groupTableMap)) {
            if (!visibleGroups[groupId]) {
              const tableIds = groupTableMap[groupId];
              if (tableIds?.includes(nodeId)) {
                return true;
              }
            }
          }
          return false;
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
        applyPanelCollapsed2 = applyPanelCollapsed, buildGroupList2 = buildGroupList, toggleGroup2 = toggleGroup, updateEdgeVisibility2 = updateEdgeVisibility, isNodeHidden2 = isNodeHidden, showAllGroups2 = showAllGroups, hideAllGroups2 = hideAllGroups;
        const collapseBtn = document.getElementById("group-panel-collapse");
        const COLLAPSE_KEY = "relune-group-panel-collapsed";
        collapseBtn?.addEventListener("click", () => {
          const next = !groupPanel?.classList.contains("group-panel-collapsed");
          applyPanelCollapsed(next);
          try {
            sessionStorage.setItem(COLLAPSE_KEY, next ? "1" : "0");
          } catch {
          }
        });
        try {
          if (sessionStorage.getItem(COLLAPSE_KEY) === "1") {
            applyPanelCollapsed(true);
          }
        } catch {
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
        buildGroupList();
      }
    }
  }
  var applyPanelCollapsed2;
  var buildGroupList2;
  var toggleGroup2;
  var updateEdgeVisibility2;
  var isNodeHidden2;
  var showAllGroups2;
  var hideAllGroups2;
})();
