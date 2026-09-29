import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { resetViewerRuntime } from './test_fixtures';
import { getViewerRuntime } from './viewer_api';

function installRuntime() {
  const mocks = {
    focusSearch: vi.fn(),
    fit: vi.fn(),
    zoomIn: vi.fn(),
    zoomOut: vi.fn(),
    setMinimapHidden: vi.fn(),
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

  it.each([
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
