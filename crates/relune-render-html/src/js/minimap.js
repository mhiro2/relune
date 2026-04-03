"use strict";
(() => {
  // ts/viewer_api.ts
  function getViewerRuntime() {
    if (window.reluneViewer === void 0) {
      window.reluneViewer = {};
    }
    return window.reluneViewer;
  }

  // ts/minimap.ts
  {
    const host = document.getElementById("minimap");
    const svgRoot = document.querySelector(".canvas svg");
    if (!(host instanceof SVGSVGElement) || svgRoot === null) {
    } else {
      let updateFrame = function(state) {
        const viewX = (-state.panX / state.scale - bounds.x) / state.contentWidth * 100;
        const viewY = (-state.panY / state.scale - bounds.y) / state.contentHeight * 100;
        const viewWidth = state.viewportWidth / state.scale / state.contentWidth * 100;
        const viewHeight = state.viewportHeight / state.scale / state.contentHeight * 100;
        frame.setAttribute("x", String(viewX));
        frame.setAttribute("y", String(viewY));
        frame.setAttribute("width", String(viewWidth));
        frame.setAttribute("height", String(viewHeight));
      }, focusPoint = function(event) {
        const rect = hostSvg.getBoundingClientRect();
        if (rect.width <= 0 || rect.height <= 0) {
          return;
        }
        const percentX = (event.clientX - rect.left) / rect.width;
        const percentY = (event.clientY - rect.top) / rect.height;
        const contentX = bounds.x + percentX * bounds.width;
        const contentY = bounds.y + percentY * bounds.height;
        runtime.viewport?.center(contentX, contentY);
      };
      updateFrame2 = updateFrame, focusPoint2 = focusPoint;
      const runtime = getViewerRuntime();
      const hostSvg = host;
      hostSvg.innerHTML = "";
      const frame = document.createElementNS("http://www.w3.org/2000/svg", "rect");
      frame.setAttribute("class", "minimap-frame");
      hostSvg.appendChild(frame);
      const bounds = runtime.viewport?.getDiagramBounds() ?? (() => {
        const viewBox = svgRoot.getAttribute("viewBox")?.split(/\s+/).map(Number) ?? [];
        return {
          x: viewBox[0] ?? 0,
          y: viewBox[1] ?? 0,
          width: Math.max(viewBox[2] ?? 0, 1),
          height: Math.max(viewBox[3] ?? 0, 1)
        };
      })();
      const nodes = [];
      svgRoot.querySelectorAll(".node").forEach((node) => {
        const id = node.getAttribute("data-id") ?? node.getAttribute("data-table-id");
        const rect = node.querySelector(".table-body");
        if (id === null || rect === null) {
          return;
        }
        nodes.push({
          id,
          x: Number.parseFloat(rect.getAttribute("x") ?? "0"),
          y: Number.parseFloat(rect.getAttribute("y") ?? "0"),
          width: Number.parseFloat(rect.getAttribute("width") ?? "0"),
          height: Number.parseFloat(rect.getAttribute("height") ?? "0")
        });
      });
      const nodeEls = /* @__PURE__ */ new Map();
      for (const node of nodes) {
        const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
        rect.setAttribute("class", "minimap-node");
        rect.setAttribute("x", String((node.x - bounds.x) / bounds.width * 100));
        rect.setAttribute("y", String((node.y - bounds.y) / bounds.height * 100));
        rect.setAttribute("width", String(node.width / bounds.width * 100));
        rect.setAttribute("height", String(node.height / bounds.height * 100));
        hostSvg.appendChild(rect);
        nodeEls.set(node.id, rect);
      }
      document.addEventListener("relune:viewport-changed", (event) => {
        const customEvent = event;
        updateFrame(customEvent.detail);
      });
      document.addEventListener("relune:node-selected", (event) => {
        const customEvent = event;
        nodeEls.forEach((element, id) => {
          element.classList.toggle("selected", id === customEvent.detail.nodeId);
        });
      });
      document.addEventListener("relune:node-cleared", () => {
        nodeEls.forEach((element) => {
          element.classList.remove("selected");
        });
      });
      let dragging = false;
      hostSvg.addEventListener("mousedown", (event) => {
        dragging = true;
        focusPoint(event);
        event.preventDefault();
      });
      document.addEventListener("mousemove", (event) => {
        if (dragging) {
          focusPoint(event);
        }
      });
      document.addEventListener("mouseup", () => {
        dragging = false;
      });
      const initialState = runtime.viewport?.getState();
      if (initialState !== null && initialState !== void 0) {
        updateFrame(initialState);
      }
      hostSvg.addEventListener("click", (event) => {
        const bounds2 = runtime.viewport?.getDiagramBounds();
        if (bounds2 === null || bounds2 === void 0) {
          return;
        }
        const rect = hostSvg.getBoundingClientRect();
        if (rect.width <= 0 || rect.height <= 0) {
          return;
        }
        const percentX = (event.clientX - rect.left) / rect.width;
        const percentY = (event.clientY - rect.top) / rect.height;
        runtime.viewport?.center(
          bounds2.x + bounds2.width * percentX,
          bounds2.y + bounds2.height * percentY
        );
      });
    }
  }
  var updateFrame2;
  var focusPoint2;
})();
