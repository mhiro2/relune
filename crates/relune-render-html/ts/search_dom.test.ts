import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { column, metadataScript, resetViewerRuntime, table } from './test_fixtures';
import { getViewerRuntime } from './viewer_api';

// Mirrors the viewer shell and the node/edge markup emitted by relune-render-svg.
function renderViewer(): void {
  document.body.innerHTML = `
    ${metadataScript({
      tables: [
        table('public.users', { label: 'Users', columns: [column('email', 'text')] }),
        table('public.posts', { label: 'Posts', columns: [column('title', 'text')] }),
        table('public.tags', { label: 'Tags', columns: [column('name', 'text')] }),
      ],
    })}
    <input id="table-search" type="search">
    <button id="search-clear"></button>
    <div id="search-results"></div>
    <div class="canvas"><svg>
      <g class="node" data-id="public.users"><text>Users</text><text>email</text></g>
      <g class="node" data-id="public.posts"><text>Posts</text><text>title</text></g>
      <g class="node" data-id="public.tags"><text>Tags</text><text>name</text></g>
      <g class="edge" data-from="public.posts" data-to="public.users"></g>
    </svg></div>`;
}

async function loadSearch(): Promise<HTMLInputElement> {
  vi.resetModules();
  await import('./search');
  const input = document.getElementById('table-search');
  if (!(input instanceof HTMLInputElement)) throw new Error('missing search input');
  return input;
}

function classesOf(id: string): string[] {
  return [...(document.querySelector(`[data-id="${id}"]`)?.classList ?? [])].filter(
    (c) => c !== 'node',
  );
}

function type(input: HTMLInputElement, value: string): void {
  input.value = value;
  input.dispatchEvent(new Event('input', { bubbles: true }));
}

describe('table search', () => {
  // Scopes the document listeners each test adds.
  let listeners: AbortController;

  beforeEach(() => {
    listeners = new AbortController();
    resetViewerRuntime();
    renderViewer();
    vi.useFakeTimers();
  });

  afterEach(() => {
    listeners.abort();
    vi.useRealTimers();
  });

  it('highlights matches, dims the rest and reports the count after the debounce', async () => {
    const input = await loadSearch();
    const events: unknown[] = [];
    document.addEventListener(
      'relune:search-changed',
      (e) => {
        events.push((e as CustomEvent).detail);
      },
      { signal: listeners.signal },
    );

    type(input, 'pos');
    expect(classesOf('public.posts')).toEqual([]);
    vi.advanceTimersByTime(150);

    expect(classesOf('public.posts')).toEqual(['highlighted-by-search']);
    expect(classesOf('public.users')).toEqual(['dimmed-by-search']);
    expect(classesOf('public.tags')).toEqual(['dimmed-by-search']);
    expect(document.querySelector('.edge')?.classList.contains('dimmed-by-edge-filter')).toBe(true);
    expect(document.getElementById('search-results')?.textContent).toBe('1 of 3 objects');
    expect(document.getElementById('search-results')?.classList.contains('visible')).toBe(true);
    expect(document.getElementById('search-clear')?.classList.contains('visible')).toBe(true);
    expect(events).toEqual([{ active: true, query: 'pos', matches: 1, total: 3 }]);
  });

  it('only runs the latest query while typing', async () => {
    const input = await loadSearch();
    const listener = vi.fn();
    document.addEventListener('relune:search-changed', listener, { signal: listeners.signal });

    type(input, 'u');
    vi.advanceTimersByTime(100);
    type(input, 'us');
    vi.advanceTimersByTime(100);
    expect(listener).not.toHaveBeenCalled();
    vi.advanceTimersByTime(50);
    expect(listener).toHaveBeenCalledOnce();
    expect(classesOf('public.users')).toEqual(['highlighted-by-search']);
  });

  it('matches metadata display names', async () => {
    await loadSearch();
    getViewerRuntime().search?.setQuery('TAGS');
    expect(classesOf('public.tags')).toEqual(['highlighted-by-search']);
    expect(classesOf('public.users')).toEqual(['dimmed-by-search']);
  });

  it('clears highlighting through the clear button and Escape', async () => {
    const input = await loadSearch();
    const runtime = getViewerRuntime();

    runtime.search?.setQuery('users');
    expect(runtime.search?.isActive()).toBe(true);
    document.getElementById('search-clear')?.dispatchEvent(new MouseEvent('click'));
    expect(input.value).toBe('');
    expect(runtime.search?.isActive()).toBe(false);
    expect(classesOf('public.posts')).toEqual([]);
    expect(document.querySelector('.edge')?.classList.contains('dimmed-by-edge-filter')).toBe(
      false,
    );
    expect(document.getElementById('search-results')?.classList.contains('visible')).toBe(false);

    runtime.search?.setQuery('users');
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(runtime.search?.getQuery()).toBe('');
    expect(classesOf('public.posts')).toEqual([]);
  });

  it('emits an inactive event with the full count when cleared', async () => {
    await loadSearch();
    const events: unknown[] = [];
    document.addEventListener(
      'relune:search-changed',
      (e) => {
        events.push((e as CustomEvent).detail);
      },
      { signal: listeners.signal },
    );
    getViewerRuntime().search?.clear();
    expect(events).toEqual([{ active: false, query: '', matches: 3, total: 3 }]);
  });
});
