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
      } catch {
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
    } catch {
    }
    const canvas = document.getElementById("canvas");
    const svg = canvas?.querySelector("svg");
    if (svg) {
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
        });
      }
    }
  }
  var saveState2;
})();
