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
  function readSessionFlag(key) {
    const storage = getSessionStorage();
    if (storage === null) return null;
    try {
      const value = storage.getItem(key);
      return value === null ? null : value === "1";
    } catch (error) {
      reportSessionStorageError(`restoring ${key}`, error);
      return null;
    }
  }
  function writeSessionFlag(key, value) {
    const storage = getSessionStorage();
    if (storage === null) return;
    try {
      storage.setItem(key, value ? "1" : "0");
    } catch (error) {
      reportSessionStorageError(`saving ${key}`, error);
    }
  }

  // ts/sidebar.ts
  {
    const panel = document.getElementById("search-panel");
    const collapseButton = document.getElementById("sidebar-collapse");
    const openButton = document.getElementById("sidebar-open");
    if (panel instanceof HTMLElement && openButton instanceof HTMLButtonElement) {
      const COLLAPSED_KEY = "relune-sidebar-collapsed";
      const isCollapsed = () => panel.hasAttribute("hidden");
      const setCollapsed = (collapsed) => {
        if (collapsed === isCollapsed()) return;
        panel.toggleAttribute("hidden", collapsed);
        openButton.toggleAttribute("hidden", !collapsed);
        collapseButton?.setAttribute("aria-expanded", String(!collapsed));
        openButton.setAttribute("aria-expanded", String(!collapsed));
        writeSessionFlag(COLLAPSED_KEY, collapsed);
      };
      collapseButton?.addEventListener("click", () => {
        setCollapsed(true);
        openButton.focus();
      });
      openButton.addEventListener("click", () => {
        setCollapsed(false);
        collapseButton?.focus();
      });
      if (readSessionFlag(COLLAPSED_KEY) === true) {
        setCollapsed(true);
      }
      getViewerRuntime().sidebar = { isCollapsed, setCollapsed };
    }
  }
})();
