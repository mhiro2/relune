"use strict";
(() => {
  // ts/viewer_api.ts
  var VIEWER_RUNTIME_KEY = /* @__PURE__ */ Symbol.for("relune.viewer.runtime");
  function getViewerRuntime() {
    const viewerWindow = window;
    if (viewerWindow[VIEWER_RUNTIME_KEY] === void 0) {
      viewerWindow[VIEWER_RUNTIME_KEY] = {};
    }
    return viewerWindow[VIEWER_RUNTIME_KEY];
  }
  function isEditableTarget(target) {
    if (!(target instanceof Element)) {
      return false;
    }
    return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement || target.closest('[contenteditable="true"]') !== null;
  }

  // ts/shortcuts.ts
  {
    const runtime = getViewerRuntime();
    document.addEventListener("keydown", (event) => {
      if (isEditableTarget(event.target)) {
        if (event.key === "Escape") {
          runtime.search?.clear();
        }
        return;
      }
      if (event.ctrlKey || event.metaKey || event.altKey || event.isComposing) {
        return;
      }
      switch (event.key) {
        case "/":
          event.preventDefault();
          runtime.sidebar?.setCollapsed(false);
          runtime.search?.focus();
          break;
        case "Escape":
          runtime.search?.clear();
          runtime.filters?.reset();
          runtime.selection?.clear();
          break;
        case "f":
        case "F":
          event.preventDefault();
          runtime.viewport?.fit();
          break;
        case "g":
        case "G":
          event.preventDefault();
          if (runtime.groups !== void 0) {
            const reveal = runtime.sidebar?.isCollapsed() === true;
            runtime.sidebar?.setCollapsed(false);
            runtime.groups.setPanelOpen(reveal || !runtime.groups.isPanelOpen());
          }
          break;
        case "s":
        case "S":
          event.preventDefault();
          if (runtime.sidebar !== void 0) {
            runtime.sidebar.setCollapsed(!runtime.sidebar.isCollapsed());
          }
          break;
        case "m":
        case "M":
          event.preventDefault();
          if (runtime.minimap !== void 0) {
            runtime.minimap.setHidden(!runtime.minimap.isHidden());
            runtime.viewport?.fit();
          }
          break;
        case "+":
        case "=":
          event.preventDefault();
          runtime.viewport?.zoomIn();
          break;
        case "-":
        case "_":
          event.preventDefault();
          runtime.viewport?.zoomOut();
          break;
        default:
          break;
      }
    });
  }
})();
