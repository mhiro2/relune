import { beforeEach, describe, expect, it, vi } from 'vitest';

import { resetViewerRuntime } from './test_fixtures';
import { getViewerRuntime } from './viewer_api';

const SIDEBAR_HTML = `
  <aside id="search-panel">
    <button type="button" id="sidebar-collapse" aria-expanded="true"></button>
  </aside>
  <button type="button" id="sidebar-open" aria-expanded="false" hidden></button>
`;

async function loadSidebar(): Promise<void> {
  vi.resetModules();
  await import('./sidebar');
}

beforeEach(() => {
  resetViewerRuntime();
  window.sessionStorage.clear();
  document.body.innerHTML = SIDEBAR_HTML;
});

describe('explorer sidebar', () => {
  it('swaps the sidebar for the open button and back', async () => {
    await loadSidebar();
    const panel = document.getElementById('search-panel')!;
    const openButton = document.getElementById('sidebar-open') as HTMLButtonElement;

    document.getElementById('sidebar-collapse')!.click();
    expect(panel.hasAttribute('hidden')).toBe(true);
    expect(openButton.hidden).toBe(false);
    expect(openButton.getAttribute('aria-expanded')).toBe('false');
    expect(document.activeElement).toBe(openButton);

    openButton.click();
    expect(panel.hasAttribute('hidden')).toBe(false);
    expect(openButton.hidden).toBe(true);
    expect(getViewerRuntime().sidebar?.isCollapsed()).toBe(false);
  });

  it('remembers a hidden sidebar for the tab', async () => {
    await loadSidebar();
    getViewerRuntime().sidebar?.setCollapsed(true);
    expect(window.sessionStorage.getItem('relune-sidebar-collapsed')).toBe('1');

    resetViewerRuntime();
    document.body.innerHTML = SIDEBAR_HTML;
    await loadSidebar();
    expect(document.getElementById('search-panel')!.hasAttribute('hidden')).toBe(true);
    expect(getViewerRuntime().sidebar?.isCollapsed()).toBe(true);
  });
});
