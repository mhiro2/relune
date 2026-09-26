import { createCollapseController } from './collapse_dom';
import { parseReluneMetadata } from './metadata';
import {
  emitViewerEvent,
  getViewerRuntime,
  getSessionStorage,
  markViewerModuleReady,
  reportSessionStorageError,
} from './viewer_api';

// Scope saved state to the document so different diagrams opened in the same
// origin (e.g. several exports under one file:// or static host) do not share it.
// The hash is excluded because it carries the viewer's own URL state.
const STORAGE_KEY = `relune-collapsed-tables:${location.pathname}${location.search}`;

{
  const metadata = parseReluneMetadata();
  const columnCounts = new Map(
    (metadata?.tables ?? []).map((table) => [table.id, table.columns?.length ?? 0]),
  );

  const sessionStorageRef = getSessionStorage();

  function loadState(): string[] {
    try {
      const saved = sessionStorageRef?.getItem(STORAGE_KEY);
      if (saved) {
        const arr: unknown = JSON.parse(saved);
        if (Array.isArray(arr)) {
          return arr.filter((id): id is string => typeof id === 'string');
        }
      }
    } catch (error: unknown) {
      reportSessionStorageError('restoring collapsed tables', error);
    }
    return [];
  }

  function saveState(tableIds: string[]): void {
    if (sessionStorageRef === null) {
      return;
    }

    try {
      sessionStorageRef.setItem(STORAGE_KEY, JSON.stringify(tableIds));
    } catch (error: unknown) {
      reportSessionStorageError('saving collapsed tables', error);
    }
  }

  const svg = document.getElementById('canvas')?.querySelector('svg');
  if (svg) {
    const controller = createCollapseController(svg, {
      columnCounts,
      initiallyCollapsed: loadState(),
      onToggle: () => {
        saveState(controller.getCollapsed());
        emitViewerEvent('relune:collapse-changed', undefined);
      },
    });

    const runtime = getViewerRuntime();
    runtime.collapse = {
      getCollapsed(): string[] {
        return controller.getCollapsed();
      },
      setCollapsed(tableIds: string[]): void {
        controller.setCollapsed(tableIds);
        saveState(controller.getCollapsed());
      },
    };
    markViewerModuleReady('collapse');
  }
}
