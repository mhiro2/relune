import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { resetViewerRuntime } from './test_fixtures';
import { getViewerRuntime } from './viewer_api';

let sidebarCollapsed = false;
let groupPanelOpen = false;

function installRuntime() {
  const mocks = {
    focusSearch: vi.fn(),
    fit: vi.fn(),
    zoomIn: vi.fn(),
    zoomOut: vi.fn(),
    setMinimapHidden: vi.fn(),
    setSidebarCollapsed: vi.fn(),
    setGroupPanelOpen: vi.fn(),
  };
  const runtime = getViewerRuntime();
  runtime.search = {
    focus: mocks.focusSearch,
    clear: vi.fn(),
    isActive: () => false,
    setQuery: vi.fn(),
    getQuery: () => '',
  };
  runtime.viewport = {
    fit: mocks.fit,
    zoomIn: mocks.zoomIn,
    zoomOut: mocks.zoomOut,
  } as unknown as NonNullable<typeof runtime.viewport>;
  runtime.minimap = {
    isHidden: () => true,
    setHidden: mocks.setMinimapHidden,
  };
  runtime.sidebar = {
    isCollapsed: () => sidebarCollapsed,
    setCollapsed: mocks.setSidebarCollapsed,
  };
  runtime.groups = {
    setVisibility: vi.fn(),
    getHiddenGroups: () => [],
    isPanelOpen: () => groupPanelOpen,
    setPanelOpen: mocks.setGroupPanelOpen,
  };
  return mocks;
}

function press(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, cancelable: true, bubbles: true, ...init });
  document.body.dispatchEvent(event);
  return event;
}

// Each test loads a fresh copy of the module against a fresh runtime and
// removes the document listener it installed afterwards.
let mocks: ReturnType<typeof installRuntime>;
const installedListeners: EventListenerOrEventListenerObject[] = [];
beforeEach(async () => {
  sidebarCollapsed = false;
  groupPanelOpen = false;
  resetViewerRuntime();
  mocks = installRuntime();
  const original = document.addEventListener.bind(document);
  vi.spyOn(document, 'addEventListener').mockImplementation((type, listener, options) => {
    if (listener !== null) installedListeners.push(listener);
    original(type, listener, options);
  });
  vi.resetModules();
  await import('./shortcuts');
});

afterEach(() => {
  vi.restoreAllMocks();
  for (const listener of installedListeners.splice(0)) {
    document.removeEventListener('keydown', listener);
  }
});

describe('viewer shortcuts', () => {
  it('handles the plain keys', () => {
    expect(press('f').defaultPrevented).toBe(true);
    expect(mocks.fit).toHaveBeenCalledTimes(1);
    press('/');
    expect(mocks.focusSearch).toHaveBeenCalledTimes(1);
    press('+', { shiftKey: true });
    press('-');
    expect(mocks.zoomIn).toHaveBeenCalledTimes(1);
    expect(mocks.zoomOut).toHaveBeenCalledTimes(1);
    press('m');
    expect(mocks.setMinimapHidden).toHaveBeenCalledWith(false);
  });

  it('brings back a hidden sidebar before focusing search', () => {
    sidebarCollapsed = true;
    press('/');
    expect(mocks.setSidebarCollapsed).toHaveBeenCalledWith(false);
    expect(mocks.focusSearch).toHaveBeenCalledTimes(1);
  });

  it('toggles the sidebar with S', () => {
    press('s');
    expect(mocks.setSidebarCollapsed).toHaveBeenLastCalledWith(true);
    sidebarCollapsed = true;
    press('S', { shiftKey: true });
    expect(mocks.setSidebarCollapsed).toHaveBeenLastCalledWith(false);
  });

  it('toggles the groups section with G, opening it when the sidebar was hidden', () => {
    press('g');
    expect(mocks.setGroupPanelOpen).toHaveBeenLastCalledWith(true);
    groupPanelOpen = true;
    press('g');
    expect(mocks.setGroupPanelOpen).toHaveBeenLastCalledWith(false);
    sidebarCollapsed = true;
    press('g');
    expect(mocks.setSidebarCollapsed).toHaveBeenLastCalledWith(false);
    expect(mocks.setGroupPanelOpen).toHaveBeenLastCalledWith(true);
  });

  it.each([
    ['s', { metaKey: true }],
    ['f', { metaKey: true }],
    ['f', { ctrlKey: true }],
    ['g', { metaKey: true }],
    ['=', { ctrlKey: true }],
    ['-', { metaKey: true }],
    ['m', { altKey: true }],
    ['/', { ctrlKey: true }],
  ])('leaves %s with modifiers %o to the browser', (key, modifiers) => {
    const event = press(key, modifiers);
    expect(event.defaultPrevented).toBe(false);
    expect(mocks.fit).not.toHaveBeenCalled();
    expect(mocks.zoomIn).not.toHaveBeenCalled();
    expect(mocks.zoomOut).not.toHaveBeenCalled();
    expect(mocks.focusSearch).not.toHaveBeenCalled();
    expect(mocks.setMinimapHidden).not.toHaveBeenCalled();
  });
});
