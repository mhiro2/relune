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
      switch (event.key) {
        case "/":
          event.preventDefault();
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
          document.getElementById("group-panel-collapse")?.dispatchEvent(new MouseEvent("click"));
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
