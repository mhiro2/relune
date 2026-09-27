import { describe, expect, it } from 'vitest';

import {
  buildViewportState,
  clamp,
  clampAxis,
  clampPan,
  computeFit,
  computeZoomAt,
  parseViewBox,
} from './pan_zoom_state';

function svgWithViewBox(viewBox: string | null): SVGSVGElement {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  if (viewBox !== null) svg.setAttribute('viewBox', viewBox);
  return svg;
}

describe('clamp', () => {
  it('limits values to the inclusive range', () => {
    expect(clamp(5, 0, 10)).toBe(5);
    expect(clamp(-1, 0, 10)).toBe(0);
    expect(clamp(11, 0, 10)).toBe(10);
  });
});

describe('parseViewBox', () => {
  it('reads the four viewBox numbers', () => {
    expect(parseViewBox(svgWithViewBox('-20 -10  800 600'))).toEqual({
      x: -20,
      y: -10,
      width: 800,
      height: 600,
    });
  });

  it('falls back to zero for missing or partial values', () => {
    expect(parseViewBox(svgWithViewBox(null))).toEqual({ x: 0, y: 0, width: 0, height: 0 });
    expect(parseViewBox(svgWithViewBox('0 0 400'))).toEqual({ x: 0, y: 0, width: 400, height: 0 });
  });
});

describe('clampAxis', () => {
  it('centres content that fits in the viewport', () => {
    expect(clampAxis(-500, 200, 100, 1000)).toBe(500);
  });

  it('keeps oversized content within a padded range', () => {
    // padding = clamp(1000 * 0.08, 24, 80) = 80
    expect(clampAxis(500, 3000, 0, 1000)).toBe(80);
    expect(clampAxis(-5000, 3000, 0, 1000)).toBe(1000 - 3000 - 80);
    expect(clampAxis(-900, 3000, 0, 1000)).toBe(-900);
  });

  it('uses the minimum padding for small viewports', () => {
    expect(clampAxis(1000, 1000, 0, 100)).toBe(24);
  });
});

describe('clampPan', () => {
  it('clamps each axis against the scaled diagram', () => {
    const result = clampPan(
      9999,
      9999,
      2,
      { x: 0, y: 0, width: 400, height: 100 },
      { left: 0, top: 50, width: 500, height: 500 },
    );
    // width 800 > 500 clamps to padding 40; height 200 < 500 is centred.
    expect(result).toEqual({ panX: 40, panY: 50 + 150 });
  });
});

describe('computeFit', () => {
  it('fits and centres the diagram with padding', () => {
    const fit = computeFit(
      { x: 0, y: 0, width: 960, height: 480 },
      { left: 0, top: 0, width: 1000, height: 1000 },
    );
    expect(fit).toEqual({ scale: 1, panX: 20, panY: 260 });
  });

  it('respects the zoom bounds', () => {
    const available = { left: 0, top: 0, width: 1000, height: 1000 };
    expect(computeFit({ x: 0, y: 0, width: 10, height: 10 }, available)?.scale).toBe(2);
    expect(computeFit({ x: 0, y: 0, width: 100_000, height: 10 }, available)?.scale).toBe(0.1);
  });

  it('returns null for empty diagrams or viewports', () => {
    const available = { left: 0, top: 0, width: 1000, height: 1000 };
    expect(computeFit({ x: 0, y: 0, width: 0, height: 10 }, available)).toBeNull();
    expect(
      computeFit({ x: 0, y: 0, width: 10, height: 10 }, { ...available, height: 0 }),
    ).toBeNull();
  });
});

describe('computeZoomAt', () => {
  it('keeps the point under the cursor fixed', () => {
    const result = computeZoomAt(1, 100, 50, 2, 300, 250);
    expect(result).toEqual({ scale: 2, panX: -100, panY: -150 });
    // Content point under the cursor stays the same before and after.
    expect((300 - 100) / 1).toBe((300 - result.panX) / result.scale);
  });

  it('clamps the requested scale', () => {
    expect(computeZoomAt(1, 0, 0, 10, 0, 0).scale).toBe(2);
    expect(computeZoomAt(1, 0, 0, 0.01, 0, 0).scale).toBe(0.1);
  });
});

describe('buildViewportState', () => {
  it('combines the transform with viewport and content sizes', () => {
    const viewport = document.createElement('div');
    viewport.getBoundingClientRect = () => ({ width: 640, height: 480 }) as DOMRect;
    expect(
      buildViewportState(1.5, 10, 20, viewport, { x: 0, y: 0, width: 800, height: 600 }),
    ).toEqual({
      scale: 1.5,
      panX: 10,
      panY: 20,
      viewportWidth: 640,
      viewportHeight: 480,
      contentWidth: 800,
      contentHeight: 600,
    });
  });
});
