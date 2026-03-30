import {
  emitViewerEvent,
  getViewerRuntime,
  type DiagramBounds,
  type ViewportState,
} from './viewer_api';

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function parseViewBox(svg: SVGSVGElement): DiagramBounds {
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

interface AvailableViewportRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

{
  const viewportEl = document.getElementById('viewport');
  const canvasEl = document.getElementById('canvas');
  const zoomInBtn = document.getElementById('zoom-in');
  const zoomOutBtn = document.getElementById('zoom-out');
  const zoomFitBtn = document.getElementById('zoom-fit');
  const zoomLevelEl = document.getElementById('zoom-level');

  if (viewportEl instanceof HTMLElement && canvasEl instanceof HTMLElement) {
    const runtime = getViewerRuntime();
    const svg = canvasEl.querySelector('svg');
    if (!(svg instanceof SVGSVGElement)) {
      // Embedded SVG missing.
    } else {
      const diagram = parseViewBox(svg);
      let scale = 1;
      let panX = 0;
      let panY = 0;
      let isDragging = false;
      let startX = 0;
      let startY = 0;
      let startPanX = 0;
      let startPanY = 0;

      const getAvailableViewport = (): AvailableViewportRect => {
        const rect = viewportEl.getBoundingClientRect();
        const leftInset = overlayInset(document.getElementById('search-panel'), 'left');
        const rightInset = overlayInset(document.getElementById('detail-drawer'), 'right');
        const topInset = Math.max(
          overlayInset(document.querySelector('h1'), 'top'),
          overlayInset(document.getElementById('filter-reset-bar'), 'top'),
        );
        const bottomInset = Math.max(
          overlayInset(document.getElementById('viewer-controls'), 'bottom'),
          overlayInset(document.getElementById('minimap-shell'), 'bottom'),
        );

        return {
          left: leftInset,
          top: topInset,
          width: Math.max(rect.width - leftInset - rightInset, 120),
          height: Math.max(rect.height - topInset - bottomInset, 120),
        };
      };

      const clampAxis = (
        nextPan: number,
        contentSize: number,
        viewportStart: number,
        viewportSize: number,
      ): number => {
        if (contentSize <= viewportSize) {
          return viewportStart + (viewportSize - contentSize) / 2;
        }

        const padding = clamp(viewportSize * 0.08, 24, 80);
        const minPan = viewportStart + viewportSize - contentSize - padding;
        const maxPan = viewportStart + padding;
        return clamp(nextPan, minPan, maxPan);
      };

      const clampPan = (nextPanX: number, nextPanY: number): { panX: number; panY: number } => {
        const availableViewport = getAvailableViewport();
        const scaledWidth = diagram.width * scale;
        const scaledHeight = diagram.height * scale;

        return {
          panX: clampAxis(nextPanX, scaledWidth, availableViewport.left, availableViewport.width),
          panY: clampAxis(nextPanY, scaledHeight, availableViewport.top, availableViewport.height),
        };
      };

      const currentState = (): ViewportState => {
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
      };

      const overlayInset = (
        element: Element | null,
        side: 'left' | 'right' | 'top' | 'bottom',
      ): number => {
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
      };

      const updateTransform = (): void => {
        const constrainedPan = clampPan(panX, panY);
        panX = constrainedPan.panX;
        panY = constrainedPan.panY;

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
        emitViewerEvent('relune:viewport-changed', currentState());
      };

      const zoomAt = (nextScale: number, localX: number, localY: number): void => {
        const clampedScale = clamp(nextScale, 0.1, 2);
        const scaleFactor = clampedScale / scale;
        panX = localX - (localX - panX) * scaleFactor;
        panY = localY - (localY - panY) * scaleFactor;
        scale = clampedScale;
        updateTransform();
      };

      const fitToScreen = (): void => {
        const availableViewport = getAvailableViewport();
        if (
          diagram.width <= 0 ||
          diagram.height <= 0 ||
          availableViewport.width <= 0 ||
          availableViewport.height <= 0
        ) {
          return;
        }

        const padding = 40;
        scale = clamp(
          Math.min(
            (availableViewport.width - padding * 2) / diagram.width,
            (availableViewport.height - padding * 2) / diagram.height,
          ),
          0.1,
          2,
        );
        panX = availableViewport.left + (availableViewport.width - diagram.width * scale) / 2;
        panY = availableViewport.top + (availableViewport.height - diagram.height * scale) / 2;
        updateTransform();
      };

      const centerOnContent = (contentX: number, contentY: number): void => {
        const availableViewport = getAvailableViewport();
        panX =
          availableViewport.left + availableViewport.width / 2 - (contentX - diagram.x) * scale;
        panY =
          availableViewport.top + availableViewport.height / 2 - (contentY - diagram.y) * scale;
        updateTransform();
      };

      runtime.viewport = {
        zoomIn(): void {
          const rect = viewportEl.getBoundingClientRect();
          zoomAt(scale * 1.15, rect.width / 2, rect.height / 2);
        },
        zoomOut(): void {
          const rect = viewportEl.getBoundingClientRect();
          zoomAt(scale * 0.87, rect.width / 2, rect.height / 2);
        },
        fit(): void {
          fitToScreen();
        },
        center(contentX: number, contentY: number): void {
          centerOnContent(contentX, contentY);
        },
        getState(): ViewportState {
          return currentState();
        },
        getDiagramBounds(): DiagramBounds {
          return diagram;
        },
      };

      viewportEl.addEventListener('mousedown', (event: MouseEvent) => {
        if (event.button !== 0) {
          return;
        }
        isDragging = true;
        startX = event.clientX;
        startY = event.clientY;
        startPanX = panX;
        startPanY = panY;
        viewportEl.style.cursor = 'grabbing';
        event.preventDefault();
      });

      document.addEventListener('mousemove', (event: MouseEvent) => {
        if (!isDragging) {
          return;
        }
        panX = startPanX + (event.clientX - startX);
        panY = startPanY + (event.clientY - startY);
        updateTransform();
      });

      document.addEventListener('mouseup', () => {
        isDragging = false;
        viewportEl.style.cursor = 'grab';
      });

      viewportEl.addEventListener(
        'wheel',
        (event: WheelEvent) => {
          event.preventDefault();
          if (!event.ctrlKey && event.deltaMode === 0) {
            panX -= event.deltaX;
            panY -= event.deltaY;
            updateTransform();
            return;
          }
          if (event.deltaY === 0) {
            return;
          }
          const rect = viewportEl.getBoundingClientRect();
          zoomAt(
            scale * (event.deltaY > 0 ? 0.9 : 1.1),
            event.clientX - rect.left,
            event.clientY - rect.top,
          );
        },
        { passive: false },
      );

      let touchStartDist = 0;
      let touchStartScale = 1;
      let touchStartX = 0;
      let touchStartY = 0;

      viewportEl.addEventListener(
        'touchstart',
        (event: TouchEvent) => {
          const touches = event.touches;
          if (touches.length === 1) {
            isDragging = true;
            touchStartX = touches[0]!.clientX;
            touchStartY = touches[0]!.clientY;
            startPanX = panX;
            startPanY = panY;
          } else if (touches.length === 2) {
            isDragging = false;
            const dx = touches[0]!.clientX - touches[1]!.clientX;
            const dy = touches[0]!.clientY - touches[1]!.clientY;
            touchStartDist = Math.sqrt(dx * dx + dy * dy);
            touchStartScale = scale;
          }
        },
        { passive: true },
      );

      viewportEl.addEventListener(
        'touchmove',
        (event: TouchEvent) => {
          const touches = event.touches;
          if (touches.length === 1 && isDragging) {
            panX = startPanX + (touches[0]!.clientX - touchStartX);
            panY = startPanY + (touches[0]!.clientY - touchStartY);
            updateTransform();
          } else if (touches.length === 2) {
            event.preventDefault();
            const dx = touches[0]!.clientX - touches[1]!.clientX;
            const dy = touches[0]!.clientY - touches[1]!.clientY;
            const dist = Math.sqrt(dx * dx + dy * dy);
            const rect = viewportEl.getBoundingClientRect();
            const midX = (touches[0]!.clientX + touches[1]!.clientX) / 2 - rect.left;
            const midY = (touches[0]!.clientY + touches[1]!.clientY) / 2 - rect.top;
            zoomAt(touchStartScale * (dist / touchStartDist), midX, midY);
          }
        },
        { passive: false },
      );

      viewportEl.addEventListener('touchend', () => {
        isDragging = false;
      });

      zoomInBtn?.addEventListener('click', () => {
        runtime.viewport?.zoomIn();
      });

      zoomOutBtn?.addEventListener('click', () => {
        runtime.viewport?.zoomOut();
      });

      zoomFitBtn?.addEventListener('click', () => {
        fitToScreen();
      });

      window.addEventListener('resize', fitToScreen);
      requestAnimationFrame(fitToScreen);
    }
  }
}
