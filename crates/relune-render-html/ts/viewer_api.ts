export interface ViewportState {
  scale: number;
  panX: number;
  panY: number;
  viewportWidth: number;
  viewportHeight: number;
  contentWidth: number;
  contentHeight: number;
}

export interface DiagramBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface ViewerViewportApi {
  zoomIn(): void;
  zoomOut(): void;
  fit(): void;
  fitToRect(rect: DiagramBounds): void;
  center(contentX: number, contentY: number): void;
  getState(): ViewportState | null;
  getDiagramBounds(): DiagramBounds | null;
  setState(scale: number, panX: number, panY: number): void;
}

export type FacetId = 'schema' | 'kind' | 'columnType' | 'severity' | 'diffKind';
export type FilterMode = 'dim' | 'hide' | 'focus';

export interface ViewerFiltersApi {
  reset(): void;
  hasActiveFilters(): boolean;
  getMode(): FilterMode;
  setMode(mode: FilterMode): void;
  getFacetSelection(facetId: FacetId): string[];
  setFacetSelection(facetId: FacetId, values: string[]): void;
  getAvailableFacets(): FacetId[];
}

export interface ViewerSearchApi {
  focus(): void;
  clear(): void;
  isActive(): boolean;
  setQuery(query: string): void;
  getQuery(): string;
}

export interface ViewerSelectionApi {
  clear(): void;
  select(nodeId: string): void;
  getSelected(): string | null;
}

export interface ViewerGroupsApi {
  setVisibility(groupId: string, visible: boolean): void;
  getHiddenGroups(): string[];
}

export interface ViewerCollapseApi {
  getCollapsed(): string[];
  setCollapsed(tableIds: string[]): void;
}

export interface ViewerRuntime {
  viewport?: ViewerViewportApi;
  filters?: ViewerFiltersApi;
  search?: ViewerSearchApi;
  selection?: ViewerSelectionApi;
  groups?: ViewerGroupsApi;
  collapse?: ViewerCollapseApi;
}

export type ViewerModule = 'viewport' | 'filters' | 'search' | 'selection' | 'groups' | 'collapse';

type RuntimeWaiter = {
  modules: Set<ViewerModule>;
  callback: () => void;
};

const VIEWER_RUNTIME_KEY = Symbol.for('relune.viewer.runtime');
const VIEWER_READY_MODULES_KEY = Symbol.for('relune.viewer.ready_modules');
const VIEWER_WAITERS_KEY = Symbol.for('relune.viewer.waiters');

type ViewerWindow = Window & {
  [VIEWER_RUNTIME_KEY]?: ViewerRuntime;
  [VIEWER_READY_MODULES_KEY]?: Set<ViewerModule>;
  [VIEWER_WAITERS_KEY]?: RuntimeWaiter[];
};

export function getViewerRuntime(): ViewerRuntime {
  const viewerWindow = window as ViewerWindow;
  if (viewerWindow[VIEWER_RUNTIME_KEY] === undefined) {
    viewerWindow[VIEWER_RUNTIME_KEY] = {};
  }
  return viewerWindow[VIEWER_RUNTIME_KEY];
}

function readyModules(): Set<ViewerModule> {
  const viewerWindow = window as ViewerWindow;
  if (viewerWindow[VIEWER_READY_MODULES_KEY] === undefined) {
    viewerWindow[VIEWER_READY_MODULES_KEY] = new Set<ViewerModule>();
  }
  return viewerWindow[VIEWER_READY_MODULES_KEY];
}

function runtimeWaiters(): RuntimeWaiter[] {
  const viewerWindow = window as ViewerWindow;
  if (viewerWindow[VIEWER_WAITERS_KEY] === undefined) {
    viewerWindow[VIEWER_WAITERS_KEY] = [];
  }
  return viewerWindow[VIEWER_WAITERS_KEY];
}

export function markViewerModuleReady(module: ViewerModule): void {
  readyModules().add(module);
  flushViewerWaiters();
}

export function waitForViewerModules(modules: Iterable<ViewerModule>, callback: () => void): void {
  const pending = new Set(modules);
  if (pending.size === 0 || Array.from(pending).every((module) => readyModules().has(module))) {
    callback();
    return;
  }
  runtimeWaiters().push({ modules: pending, callback });
}

function flushViewerWaiters(): void {
  const ready = readyModules();
  const remaining: RuntimeWaiter[] = [];
  for (const waiter of runtimeWaiters()) {
    if (Array.from(waiter.modules).every((module) => ready.has(module))) {
      waiter.callback();
    } else {
      remaining.push(waiter);
    }
  }
  const viewerWindow = window as ViewerWindow;
  viewerWindow[VIEWER_WAITERS_KEY] = remaining;
}

export function emitViewerEvent<T>(name: string, detail: T): void {
  document.dispatchEvent(new CustomEvent<T>(name, { detail }));
}

function noticeStack(): HTMLElement {
  const existing = document.getElementById('relune-viewer-notices');
  if (existing instanceof HTMLElement) {
    return existing;
  }
  const stack = document.createElement('div');
  stack.id = 'relune-viewer-notices';
  stack.className = 'viewer-notice-stack';
  document.body.appendChild(stack);
  return stack;
}

export function showViewerNotice(message: string, severity: 'warning' | 'info' = 'warning'): void {
  const item = document.createElement('div');
  item.className = `viewer-notice viewer-notice-${severity}`;
  item.setAttribute('role', severity === 'warning' ? 'alert' : 'status');
  item.textContent = message;
  noticeStack().appendChild(item);
  window.setTimeout(() => {
    item.remove();
  }, 4500);
}

export function reportSessionStorageError(action: string, error: unknown): void {
  const isQuotaExceeded =
    error instanceof DOMException &&
    (error.name === 'QuotaExceededError' || error.name === 'NS_ERROR_DOM_QUOTA_REACHED');
  if (isQuotaExceeded) {
    showViewerNotice(
      `Session storage is full while ${action}. Viewer state was not saved.`,
      'warning',
    );
    return;
  }
  console.warn(`Session storage error while ${action}`, error);
}

export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) {
    return false;
  }
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
    target.closest('[contenteditable="true"]') !== null
  );
}
