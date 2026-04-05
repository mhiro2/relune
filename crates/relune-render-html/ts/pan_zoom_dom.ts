import type { DiagramBounds } from './viewer_api';

export interface AvailableViewportRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export function getAvailableViewport(viewportEl: HTMLElement): AvailableViewportRect {
  const rect = viewportEl.getBoundingClientRect();
  const leftInset = overlayInset(viewportEl, document.getElementById('search-panel'), 'left');
  const rightInset = overlayInset(viewportEl, document.getElementById('detail-drawer'), 'right');
  const topInset = Math.max(
    overlayInset(viewportEl, document.querySelector('h1'), 'top'),
    overlayInset(viewportEl, document.getElementById('filter-reset-bar'), 'top'),
  );
  const bottomInset = Math.max(
    overlayInset(viewportEl, document.getElementById('viewer-controls'), 'bottom'),
    overlayInset(viewportEl, document.getElementById('minimap-shell'), 'bottom'),
  );

  return {
    left: leftInset,
    top: topInset,
    width: Math.max(rect.width - leftInset - rightInset, 120),
    height: Math.max(rect.height - topInset - bottomInset, 120),
  };
}

function overlayInset(
  viewportEl: HTMLElement,
  element: Element | null,
  side: 'left' | 'right' | 'top' | 'bottom',
): number {
  if (!(element instanceof HTMLElement) || element.hasAttribute('hidden')) {
    return 0;
  }

  const viewportRect = viewportEl.getBoundingClientRect();
  const rect = element.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) {
    return 0;
  }

  switch (side) {
    case 'left':
      return Math.max(0, rect.right - viewportRect.left + 16);
    case 'right':
      return Math.max(0, viewportRect.right - rect.left + 16);
    case 'top':
      return Math.max(0, rect.bottom - viewportRect.top + 16);
    case 'bottom':
      return Math.max(0, viewportRect.bottom - rect.top + 16);
  }
}

export function applyTransform(
  svg: SVGSVGElement,
  canvasEl: HTMLElement,
  zoomLevelEl: HTMLElement | null,
  scale: number,
  panX: number,
  panY: number,
  diagram: DiagramBounds,
): void {
  const scaledWidth = diagram.width * scale;
  const scaledHeight = diagram.height * scale;
  svg.style.width = `${scaledWidth}px`;
  svg.style.height = `${scaledHeight}px`;
  canvasEl.style.width = `${scaledWidth}px`;
  canvasEl.style.height = `${scaledHeight}px`;
  canvasEl.style.transform = `translate(${panX}px, ${panY}px)`;
  if (zoomLevelEl instanceof HTMLElement) {
    zoomLevelEl.textContent = `${Math.round(scale * 100)}%`;
  }
}
