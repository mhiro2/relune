import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { column, metadataScript, resetViewerRuntime, table } from './test_fixtures';
import {
  getViewerRuntime,
  markViewerModuleReady,
  type FacetId,
  type FilterMode,
  type ViewerModule,
} from './viewer_api';

const ALL_MODULES: ViewerModule[] = [
  'viewport',
  'search',
  'filters',
  'selection',
  'groups',
  'collapse',
  'minimap',
];

function renderViewer(): void {
  document.body.innerHTML = `
    ${metadataScript({
      tables: [
        table('public.users', { columns: [column('email', 'text')] }),
        table('public.posts', { columns: [column('title', 'text')] }),
      ],
      groups: [{ id: 'g-public', label: 'public', table_ids: ['public.users', 'public.posts'] }],
    })}
    <button id="zoom-fit"></button>
    <input id="table-search">
    <div id="filter-section"></div>
    <div id="detail-drawer"></div>
    <div id="canvas"><svg></svg></div>
    <div id="minimap-shell"></div>`;
}

// In-memory stand-ins for the viewer modules url_state talks to.
function installRuntime() {
  const state = {
    query: '',
    selected: null as string | null,
    viewport: { scale: 1, panX: 0, panY: 0 },
    facets: new Map<FacetId, string[]>(),
    mode: 'dim' as FilterMode,
    hiddenGroups: [] as string[],
    collapsed: [] as string[],
    minimapHidden: true,
  };
  const mocks = {
    setQuery: vi.fn((q: string) => {
      state.query = q;
    }),
    select: vi.fn((id: string) => {
      state.selected = id;
    }),
    setState: vi.fn((scale: number, panX: number, panY: number) => {
      state.viewport = { scale, panX, panY };
    }),
    setMode: vi.fn((mode: FilterMode) => {
      state.mode = mode;
    }),
    setHidden: vi.fn((hidden: boolean) => {
      state.minimapHidden = hidden;
    }),
  };
  const runtime = getViewerRuntime();
  runtime.search = {
    focus: vi.fn(),
    clear: vi.fn(),
    isActive: () => state.query !== '',
    setQuery: mocks.setQuery,
    getQuery: () => state.query,
  };
  runtime.selection = {
    clear: vi.fn(),
    select: mocks.select,
    getSelected: () => state.selected,
  };
  runtime.viewport = {
    zoomIn: vi.fn(),
    zoomOut: vi.fn(),
    fit: vi.fn(),
    fitToRect: vi.fn(),
    center: vi.fn(),
    getState: () => ({
      ...state.viewport,
      viewportWidth: 800,
      viewportHeight: 600,
      contentWidth: 1000,
      contentHeight: 800,
    }),
    getDiagramBounds: () => ({ x: 0, y: 0, width: 1000, height: 800 }),
    setState: mocks.setState,
  };
  runtime.filters = {
    reset: vi.fn(),
    hasActiveFilters: () => state.facets.size > 0,
    getMode: () => state.mode,
    setMode: mocks.setMode,
    getFacetSelection: (id: FacetId) => state.facets.get(id) ?? [],
    setFacetSelection: vi.fn((id: FacetId, values: string[]) => {
      state.facets.set(id, values);
    }),
    getAvailableFacets: () => [],
  };
  runtime.groups = {
    setVisibility: vi.fn((id: string, visible: boolean) => {
      if (!visible) state.hiddenGroups.push(id);
    }),
    getHiddenGroups: () => state.hiddenGroups,
  };
  runtime.collapse = {
    getCollapsed: () => state.collapsed,
    setCollapsed: vi.fn((ids: string[]) => {
      state.collapsed = ids;
    }),
  };
  runtime.minimap = {
    isHidden: () => state.minimapHidden,
    setHidden: mocks.setHidden,
  };
  return { mocks, state };
}

function markAllReady(): void {
  for (const module of ALL_MODULES) markViewerModuleReady(module);
}

async function loadUrlState(): Promise<void> {
  vi.resetModules();
  await import('./url_state');
}

// url_state installs document/window listeners on load; drop them after each
// test so modules loaded by earlier tests do not react to later events.
const installedListeners: [EventTarget, string, EventListenerOrEventListenerObject][] = [];

beforeEach(() => {
  resetViewerRuntime();
  renderViewer();
  history.replaceState(null, '', '/diagram.html');
  for (const target of [document, window]) {
    const original = target.addEventListener.bind(target);
    vi.spyOn(target, 'addEventListener').mockImplementation((type, listener, options) => {
      if (listener !== null) installedListeners.push([target, type, listener]);
      original(type, listener, options);
    });
  }
});

afterEach(() => {
  vi.restoreAllMocks();
  for (const [target, type, listener] of installedListeners.splice(0)) {
    target.removeEventListener(type, listener);
  }
  vi.useRealTimers();
});

describe('restoring state from the URL hash', () => {
  it('applies every supported parameter once all modules are ready', async () => {
    const { mocks, state } = installRuntime();
    history.replaceState(
      null,
      '',
      '/diagram.html#q=email&t=public.posts&s=1.5000&x=-20.0&y=10.0&fs=public&ft=text,varchar&fm=focus&hg=g-public&c=public.users&mv=1',
    );
    await loadUrlState();
    // Nothing is restored until the modules the page provides report ready.
    expect(mocks.setQuery).not.toHaveBeenCalled();

    markAllReady();
    expect(state.viewport).toEqual({ scale: 1.5, panX: -20, panY: 10 });
    expect(state.query).toBe('email');
    expect(state.mode).toBe('focus');
    expect(Object.fromEntries(state.facets)).toEqual({
      schema: ['public'],
      columnType: ['text', 'varchar'],
    });
    expect(state.hiddenGroups).toEqual(['g-public']);
    expect(state.collapsed).toEqual(['public.users']);
    expect(state.selected).toBe('public.posts');
    expect(mocks.setHidden).toHaveBeenCalledWith(false, { silent: true });
  });

  it('ignores values that do not fit the current diagram', async () => {
    const { mocks, state } = installRuntime();
    markAllReady();
    history.replaceState(
      null,
      '',
      '/diagram.html#q=invoices&t=public.missing&s=9&x=0&y=0&fm=bogus&c=public.missing,public.posts',
    );
    await loadUrlState();

    expect(mocks.setState).not.toHaveBeenCalled();
    expect(mocks.setQuery).not.toHaveBeenCalled();
    expect(mocks.setMode).not.toHaveBeenCalled();
    expect(mocks.select).not.toHaveBeenCalled();
    expect(state.collapsed).toEqual(['public.posts']);
    expect(mocks.setHidden).toHaveBeenCalledWith(true, { silent: true });
  });

  it('only waits for the modules the page provides', async () => {
    const { state } = installRuntime();
    document.getElementById('canvas')?.remove();
    for (const module of ALL_MODULES.filter((m) => m !== 'collapse')) {
      markViewerModuleReady(module);
    }
    history.replaceState(null, '', '/diagram.html#t=public.users');
    await loadUrlState();
    expect(state.selected).toBe('public.users');
  });

  it('rejects pans far outside the diagram', async () => {
    const { mocks } = installRuntime();
    markAllReady();
    history.replaceState(null, '', '/diagram.html#s=1&x=100000&y=0');
    await loadUrlState();
    expect(mocks.setState).not.toHaveBeenCalled();
  });

  it('re-applies the hash on popstate', async () => {
    const { state } = installRuntime();
    markAllReady();
    await loadUrlState();
    expect(state.selected).toBeNull();

    history.replaceState(null, '', '/diagram.html#t=public.users');
    window.dispatchEvent(new PopStateEvent('popstate'));
    expect(state.selected).toBe('public.users');
  });
});

describe('writing state to the URL hash', () => {
  it('pushes a history entry for discrete changes after the debounce', async () => {
    const { state } = installRuntime();
    markAllReady();
    await loadUrlState();
    vi.useFakeTimers();
    const push = vi.spyOn(history, 'pushState');

    state.query = 'user';
    state.selected = 'public.users';
    state.viewport = { scale: 1.25, panX: 12.34, panY: -5 };
    state.facets.set('kind', ['table', 'view']);
    state.mode = 'hide';
    state.hiddenGroups = ['g-public'];
    state.collapsed = ['public.posts'];
    state.minimapHidden = false;
    document.dispatchEvent(new CustomEvent('relune:search-changed'));
    document.dispatchEvent(new CustomEvent('relune:node-selected'));
    expect(push).not.toHaveBeenCalled();

    vi.advanceTimersByTime(300);
    expect(push).toHaveBeenCalledOnce();
    expect(location.hash).toBe(
      '#q=user&t=public.users&s=1.2500&x=12.3&y=-5.0&fk=table%2Cview&fm=hide&hg=g-public&c=public.posts&mv=1',
    );
  });

  it('replaces the entry for continuous viewport changes', async () => {
    const { state } = installRuntime();
    markAllReady();
    await loadUrlState();
    vi.useFakeTimers();
    const push = vi.spyOn(history, 'pushState');
    const replace = vi.spyOn(history, 'replaceState');

    state.viewport = { scale: 2, panX: 1, panY: 2 };
    document.dispatchEvent(new CustomEvent('relune:viewport-changed'));
    vi.advanceTimersByTime(300);
    expect(push).not.toHaveBeenCalled();
    expect(replace).toHaveBeenCalledOnce();
    expect(location.hash).toBe('#s=2.0000&x=1.0&y=2.0');
  });
});
