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
  center(contentX: number, contentY: number): void;
  getState(): ViewportState | null;
  getDiagramBounds(): DiagramBounds | null;
}

export interface ViewerFiltersApi {
  reset(): void;
  hasActiveFilters(): boolean;
}

export interface ViewerSearchApi {
  focus(): void;
  clear(): void;
  isActive(): boolean;
}

export interface ViewerSelectionApi {
  clear(): void;
  select(nodeId: string): void;
  getSelected(): string | null;
}

export interface ViewerRuntime {
  viewport?: ViewerViewportApi;
  filters?: ViewerFiltersApi;
  search?: ViewerSearchApi;
  selection?: ViewerSelectionApi;
}

declare global {
  interface Window {
    reluneViewer?: ViewerRuntime;
  }
}

export function getViewerRuntime(): ViewerRuntime {
  if (window.reluneViewer === undefined) {
    window.reluneViewer = {};
  }
  return window.reluneViewer;
}

export function emitViewerEvent<T>(name: string, detail: T): void {
  document.dispatchEvent(new CustomEvent<T>(name, { detail }));
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
