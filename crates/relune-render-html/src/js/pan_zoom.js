"use strict";
(() => {
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

  // ts/pan_zoom_state.ts
  function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
  }
  function parseViewBox(svg) {
    const raw = svg.getAttribute("viewBox");
    if (raw === null) {
      return { x: 0, y: 0, width: 0, height: 0 };
    }
    const parts = raw.split(/\s+/).map(Number).filter((value) => Number.isFinite(value));
    return {
      x: parts[0] ?? 0,
      y: parts[1] ?? 0,
      width: parts[2] ?? 0,
      height: parts[3] ?? 0
    };
  }
  function clampAxis(nextPan, contentSize, viewportStart, viewportSize) {
    if (contentSize <= viewportSize) {
      return viewportStart + (viewportSize - contentSize) / 2;
    }
    const padding = clamp(viewportSize * 0.08, 24, 80);
    const minPan = viewportStart + viewportSize - contentSize - padding;
    const maxPan = viewportStart + padding;
    return clamp(nextPan, minPan, maxPan);
  }
  function clampPan(panX, panY, scale, diagram, available) {
    const scaledWidth = diagram.width * scale;
    const scaledHeight = diagram.height * scale;
    return {
      panX: clampAxis(panX, scaledWidth, available.left, available.width),
      panY: clampAxis(panY, scaledHeight, available.top, available.height)
    };
  }
  function computeFit(diagram, available) {
    if (diagram.width <= 0 || diagram.height <= 0 || available.width <= 0 || available.height <= 0) {
      return null;
    }
    const padding = 40;
    const scale = clamp(
      Math.min(
        (available.width - padding * 2) / diagram.width,
        (available.height - padding * 2) / diagram.height
      ),
      0.1,
      2
    );
    const panX = available.left + (available.width - diagram.width * scale) / 2;
    const panY = available.top + (available.height - diagram.height * scale) / 2;
    return { scale, panX, panY };
  }
  function computeZoomAt(currentScale, panX, panY, nextScale, localX, localY) {
    const clampedScale = clamp(nextScale, 0.1, 2);
    const scaleFactor = clampedScale / currentScale;
    return {
      scale: clampedScale,
      panX: localX - (localX - panX) * scaleFactor,
      panY: localY - (localY - panY) * scaleFactor
    };
  }
  function buildViewportState(scale, panX, panY, viewportEl, diagram) {
    const rect = viewportEl.getBoundingClientRect();
    return {
      scale,
      panX,
      panY,
      viewportWidth: rect.width,
      viewportHeight: rect.height,
      contentWidth: diagram.width,
      contentHeight: diagram.height
    };
  }

  // ts/pan_zoom_dom.ts
  function getAvailableViewport(viewportEl) {
    const rect = viewportEl.getBoundingClientRect();
    const leftInset = overlayInset(viewportEl, document.getElementById("search-panel"), "left");
    const rightInset = overlayInset(viewportEl, document.getElementById("detail-drawer"), "right");
    const topInset = Math.max(
      overlayInset(viewportEl, document.querySelector("h1"), "top"),
      overlayInset(viewportEl, document.getElementById("filter-reset-bar"), "top")
    );
    const bottomInset = Math.max(
      overlayInset(viewportEl, document.getElementById("viewer-controls"), "bottom"),
      overlayInset(viewportEl, document.getElementById("minimap-shell"), "bottom")
    );
    return {
      left: leftInset,
      top: topInset,
      width: Math.max(rect.width - leftInset - rightInset, 120),
      height: Math.max(rect.height - topInset - bottomInset, 120)
    };
  }
  function overlayInset(viewportEl, element, side) {
    if (!(element instanceof HTMLElement) || element.hasAttribute("hidden")) {
      return 0;
    }
    const viewportRect = viewportEl.getBoundingClientRect();
    const rect = element.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) {
      return 0;
    }
    switch (side) {
      case "left":
        return Math.max(0, rect.right - viewportRect.left + 16);
      case "right":
        return Math.max(0, viewportRect.right - rect.left + 16);
      case "top":
        return Math.max(0, rect.bottom - viewportRect.top + 16);
      case "bottom":
        return Math.max(0, viewportRect.bottom - rect.top + 16);
    }
  }
  function applyTransform(svg, canvasEl, zoomLevelEl, scale, panX, panY, diagram) {
    const scaledWidth = diagram.width * scale;
    const scaledHeight = diagram.height * scale;
    svg.style.width = `${scaledWidth}px`;
    svg.style.height = `${scaledHeight}px`;
    canvasEl.style.width = `${scaledWidth}px`;
    canvasEl.style.height = `${scaledHeight}px`;
    canvasEl.style.transform = `translate(${panX}px, ${panY}px)`;
    if (zoomLevelEl instanceof HTMLElement) {
      zoomLevelEl.textContent = `${Math.round(scale * 100)}%`;
    }
  }

  // ts/pan_zoom.ts
  {
    const viewportEl = document.getElementById("viewport");
    const canvasEl = document.getElementById("canvas");
    const zoomInBtn = document.getElementById("zoom-in");
    const zoomOutBtn = document.getElementById("zoom-out");
    const zoomFitBtn = document.getElementById("zoom-fit");
    const zoomLevelEl = document.getElementById("zoom-level");
    if (viewportEl instanceof HTMLElement && canvasEl instanceof HTMLElement) {
      const runtime = getViewerRuntime();
      const svg = canvasEl.querySelector("svg");
      if (!(svg instanceof SVGSVGElement)) {
      } else {
        const diagram = parseViewBox(svg);
        let scale = 1;
        let panX = 0;
        let panY = 0;
        let isDragging = false;
        let startX = 0;
        let startY = 0;
        let startPanX = 0;
        let startPanY = 0;
        const updateTransform = () => {
          const available = getAvailableViewport(viewportEl);
          const constrained = clampPan(panX, panY, scale, diagram, available);
          panX = constrained.panX;
          panY = constrained.panY;
          applyTransform(svg, canvasEl, zoomLevelEl, scale, panX, panY, diagram);
          emitViewerEvent(
            "relune:viewport-changed",
            buildViewportState(scale, panX, panY, viewportEl, diagram)
          );
        };
        const zoomAt = (nextScale, localX, localY) => {
          const result = computeZoomAt(scale, panX, panY, nextScale, localX, localY);
          scale = result.scale;
          panX = result.panX;
          panY = result.panY;
          updateTransform();
        };
        const fitToScreen = () => {
          const available = getAvailableViewport(viewportEl);
          const result = computeFit(diagram, available);
          if (result === null) return;
          scale = result.scale;
          panX = result.panX;
          panY = result.panY;
          updateTransform();
        };
        const fitToRect = (rect) => {
          const available = getAvailableViewport(viewportEl);
          const result = computeFit(rect, available);
          if (result === null) return;
          scale = result.scale;
          panX = result.panX;
          panY = result.panY;
          updateTransform();
        };
        const centerOnContent = (contentX, contentY) => {
          const available = getAvailableViewport(viewportEl);
          panX = available.left + available.width / 2 - (contentX - diagram.x) * scale;
          panY = available.top + available.height / 2 - (contentY - diagram.y) * scale;
          updateTransform();
        };
        runtime.viewport = {
          zoomIn() {
            const rect = viewportEl.getBoundingClientRect();
            zoomAt(scale * 1.15, rect.width / 2, rect.height / 2);
          },
          zoomOut() {
            const rect = viewportEl.getBoundingClientRect();
            zoomAt(scale * 0.87, rect.width / 2, rect.height / 2);
          },
          fit() {
            fitToScreen();
          },
          fitToRect(rect) {
            fitToRect(rect);
          },
          center(contentX, contentY) {
            centerOnContent(contentX, contentY);
          },
          getState() {
            return buildViewportState(scale, panX, panY, viewportEl, diagram);
          },
          getDiagramBounds() {
            return diagram;
          },
          setState(nextScale, nextPanX, nextPanY) {
            scale = clamp(nextScale, 0.1, 2);
            panX = nextPanX;
            panY = nextPanY;
            updateTransform();
          }
        };
        markViewerModuleReady("viewport");
        viewportEl.addEventListener("mousedown", (event) => {
          if (event.button !== 0) return;
          isDragging = true;
          startX = event.clientX;
          startY = event.clientY;
          startPanX = panX;
          startPanY = panY;
          viewportEl.style.cursor = "grabbing";
          event.preventDefault();
        });
        document.addEventListener("mousemove", (event) => {
          if (!isDragging) return;
          panX = startPanX + (event.clientX - startX);
          panY = startPanY + (event.clientY - startY);
          updateTransform();
        });
        document.addEventListener("mouseup", () => {
          isDragging = false;
          viewportEl.style.cursor = "grab";
        });
        viewportEl.addEventListener(
          "wheel",
          (event) => {
            event.preventDefault();
            if (!event.ctrlKey && event.deltaMode === 0) {
              panX -= event.deltaX;
              panY -= event.deltaY;
              updateTransform();
              return;
            }
            if (event.deltaY === 0) return;
            const rect = viewportEl.getBoundingClientRect();
            zoomAt(
              scale * (event.deltaY > 0 ? 0.9 : 1.1),
              event.clientX - rect.left,
              event.clientY - rect.top
            );
          },
          { passive: false }
        );
        let touchStartDist = 0;
        let touchStartScale = 1;
        let touchStartX = 0;
        let touchStartY = 0;
        viewportEl.addEventListener(
          "touchstart",
          (event) => {
            const touches = event.touches;
            if (touches.length === 1) {
              isDragging = true;
              touchStartX = touches[0].clientX;
              touchStartY = touches[0].clientY;
              startPanX = panX;
              startPanY = panY;
            } else if (touches.length === 2) {
              isDragging = false;
              const dx = touches[0].clientX - touches[1].clientX;
              const dy = touches[0].clientY - touches[1].clientY;
              touchStartDist = Math.sqrt(dx * dx + dy * dy);
              touchStartScale = scale;
            }
          },
          { passive: true }
        );
        viewportEl.addEventListener(
          "touchmove",
          (event) => {
            const touches = event.touches;
            if (touches.length === 1 && isDragging) {
              panX = startPanX + (touches[0].clientX - touchStartX);
              panY = startPanY + (touches[0].clientY - touchStartY);
              updateTransform();
            } else if (touches.length === 2) {
              event.preventDefault();
              const dx = touches[0].clientX - touches[1].clientX;
              const dy = touches[0].clientY - touches[1].clientY;
              const dist = Math.sqrt(dx * dx + dy * dy);
              const rect = viewportEl.getBoundingClientRect();
              const midX = (touches[0].clientX + touches[1].clientX) / 2 - rect.left;
              const midY = (touches[0].clientY + touches[1].clientY) / 2 - rect.top;
              zoomAt(touchStartScale * (dist / touchStartDist), midX, midY);
            }
          },
          { passive: false }
        );
        viewportEl.addEventListener("touchend", () => {
          isDragging = false;
        });
        zoomInBtn?.addEventListener("click", () => runtime.viewport?.zoomIn());
        zoomOutBtn?.addEventListener("click", () => runtime.viewport?.zoomOut());
        zoomFitBtn?.addEventListener("click", () => fitToScreen());
        window.addEventListener("resize", fitToScreen);
        requestAnimationFrame(fitToScreen);
      }
    }
  }
})();
