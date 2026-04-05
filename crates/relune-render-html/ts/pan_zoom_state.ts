import type { DiagramBounds, ViewportState } from './viewer_api';

export function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

export function parseViewBox(svg: SVGSVGElement): DiagramBounds {
  const raw = svg.getAttribute('viewBox');
  if (raw === null) {
    return { x: 0, y: 0, width: 0, height: 0 };
  }

  const parts = raw
    .split(/\s+/)
    .map(Number)
    .filter((value) => Number.isFinite(value));

  return {
    x: parts[0] ?? 0,
    y: parts[1] ?? 0,
    width: parts[2] ?? 0,
    height: parts[3] ?? 0,
  };
}

export interface AvailableViewport {
  left: number;
  top: number;
  width: number;
  height: number;
}

export function clampAxis(
  nextPan: number,
  contentSize: number,
  viewportStart: number,
  viewportSize: number,
): number {
  if (contentSize <= viewportSize) {
    return viewportStart + (viewportSize - contentSize) / 2;
  }

  const padding = clamp(viewportSize * 0.08, 24, 80);
  const minPan = viewportStart + viewportSize - contentSize - padding;
  const maxPan = viewportStart + padding;
  return clamp(nextPan, minPan, maxPan);
}

export function clampPan(
  panX: number,
  panY: number,
  scale: number,
  diagram: DiagramBounds,
  available: AvailableViewport,
): { panX: number; panY: number } {
  const scaledWidth = diagram.width * scale;
  const scaledHeight = diagram.height * scale;
  return {
    panX: clampAxis(panX, scaledWidth, available.left, available.width),
    panY: clampAxis(panY, scaledHeight, available.top, available.height),
  };
}

export function computeFit(
  diagram: DiagramBounds,
  available: AvailableViewport,
): { scale: number; panX: number; panY: number } | null {
  if (diagram.width <= 0 || diagram.height <= 0 || available.width <= 0 || available.height <= 0) {
    return null;
  }

  const padding = 40;
  const scale = clamp(
    Math.min(
      (available.width - padding * 2) / diagram.width,
      (available.height - padding * 2) / diagram.height,
    ),
    0.1,
    2,
  );
  const panX = available.left + (available.width - diagram.width * scale) / 2;
  const panY = available.top + (available.height - diagram.height * scale) / 2;
  return { scale, panX, panY };
}

export function computeZoomAt(
  currentScale: number,
  panX: number,
  panY: number,
  nextScale: number,
  localX: number,
  localY: number,
): { scale: number; panX: number; panY: number } {
  const clampedScale = clamp(nextScale, 0.1, 2);
  const scaleFactor = clampedScale / currentScale;
  return {
    scale: clampedScale,
    panX: localX - (localX - panX) * scaleFactor,
    panY: localY - (localY - panY) * scaleFactor,
  };
}

export function buildViewportState(
  scale: number,
  panX: number,
  panY: number,
  viewportEl: HTMLElement,
  diagram: DiagramBounds,
): ViewportState {
  const rect = viewportEl.getBoundingClientRect();
  return {
    scale,
    panX,
    panY,
    viewportWidth: rect.width,
    viewportHeight: rect.height,
    contentWidth: diagram.width,
    contentHeight: diagram.height,
  };
}
