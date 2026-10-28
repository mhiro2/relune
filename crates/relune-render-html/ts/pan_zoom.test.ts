import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { resetViewerRuntime } from './test_fixtures';
import { getViewerRuntime, type ViewerViewportApi } from './viewer_api';

// The diagram's viewBox starts away from the origin, as relune-render-svg
// emits it when content sits at negative or offset coordinates.
function renderViewer(): void {
  document.body.innerHTML = `
    <div id="search-panel"></div>
    <div id="viewport">
      <div id="canvas">
        <svg viewBox="100 50 1000 1000"></svg>
      </div>
    </div>
    <span id="zoom-level"></span>`;
}

function stubRect(id: string, rect: { left: number; top: number; width: number; height: number }) {
  const element = document.getElementById(id);
  if (!(element instanceof HTMLElement)) throw new Error(`missing #${id}`);
  vi.spyOn(element, 'getBoundingClientRect').mockReturnValue({
    ...rect,
    x: rect.left,
    y: rect.top,
    right: rect.left + rect.width,
    bottom: rect.top + rect.height,
    toJSON: () => rect,
  });
}

async function loadPanZoom(): Promise<ViewerViewportApi> {
  vi.resetModules();
  await import('./pan_zoom');
  const viewport = getViewerRuntime().viewport;
  if (viewport === undefined) throw new Error('viewport API not registered');
  return viewport;
}

function canvasTransform(): string {
  return document.getElementById('canvas')?.style.transform ?? '';
}

function viewportEl(): HTMLElement {
  const element = document.getElementById('viewport');
  if (!(element instanceof HTMLElement)) throw new Error('missing #viewport');
  return element;
}

describe('pan and zoom', () => {
  beforeEach(() => {
    resetViewerRuntime();
    renderViewer();
    stubRect('viewport', { left: 0, top: 0, width: 800, height: 600 });
    // A 200px sidebar covers the left edge, so 216px (with the 16px gap)
    // of the viewport is unavailable to the diagram.
    stubRect('search-panel', { left: 0, top: 0, width: 200, height: 600 });
    // Keep the initial fit from running so each test sets its own view.
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation(() => 0);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('pans the canvas on wheel and drag', async () => {
    const viewport = await loadPanZoom();
    viewport.setState(1, 0, 0);

    viewportEl().dispatchEvent(
      new WheelEvent('wheel', { deltaX: 30, deltaY: 40, cancelable: true }),
    );
    expect(canvasTransform()).toBe('translate(-30px, -40px)');

    viewportEl().dispatchEvent(
      new MouseEvent('mousedown', { button: 0, clientX: 100, clientY: 100 }),
    );
    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 150, clientY: 130 }));
    document.dispatchEvent(new MouseEvent('mouseup'));
    expect(canvasTransform()).toBe('translate(20px, -10px)');
    expect(viewport.getState()).toMatchObject({ scale: 1, panX: 20, panY: -10 });
  });

  it('zooms around the pointer on ctrl+wheel', async () => {
    const viewport = await loadPanZoom();
    viewport.setState(1, 0, 0);

    const wheel = new WheelEvent('wheel', { deltaY: 100 });
    // happy-dom drops the inherited MouseEvent fields from the WheelEvent init.
    Object.defineProperties(wheel, {
      ctrlKey: { value: true },
      clientX: { value: 400 },
      clientY: { value: 300 },
    });
    viewportEl().dispatchEvent(wheel);

    expect(document.getElementById('zoom-level')?.textContent).toBe('90%');
    // The point under the cursor stays put: 400 - (400 - 0) * 0.9.
    expect(viewport.getState()).toMatchObject({ scale: 0.9, panX: 40, panY: 30 });
    expect(document.querySelector('svg')?.style.width).toBe('900px');
  });

  it('keeps the diagram within reach of the area beside the sidebar', async () => {
    const viewport = await loadPanZoom();

    // Padding is 8% of the free size: 584 * 0.08 horizontally, 48 vertically.
    viewport.setState(1, 5000, 5000);
    const state = viewport.getState();
    expect(state?.panX).toBeCloseTo(216 + 584 * 0.08);
    expect(state?.panY).toBe(48);

    viewport.setState(1, -5000, -5000);
    expect(viewport.getState()?.panX).toBeCloseTo(216 + 584 - 1000 - 584 * 0.08);
    expect(viewport.getState()?.panY).toBe(600 - 1000 - 48);
  });

  it('centers on content coordinates relative to the viewBox origin', async () => {
    const viewport = await loadPanZoom();
    viewport.setState(1, 0, 0);

    // (600, 550) is the middle of the 100,50 1000x1000 viewBox; the free
    // area's middle is 216 + 584 / 2 = 508 across and 300 down.
    viewport.center(600, 550);

    expect(viewport.getState()).toMatchObject({ panX: 8, panY: -200 });
    expect(canvasTransform()).toBe('translate(8px, -200px)');
  });
});
