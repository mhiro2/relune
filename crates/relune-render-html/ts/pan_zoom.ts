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

{
  const viewportEl = document.getElementById('viewport');
  const canvasEl = document.getElementById('canvas');
  const zoomInBtn = document.getElementById('zoom-in');
  const zoomOutBtn = document.getElementById('zoom-out');
  const zoomFitBtn = document.getElementById('zoom-fit');

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

      const updateTransform = (): void => {
        canvasEl.style.transform = `translate(${panX}px, ${panY}px) scale(${scale})`;
        emitViewerEvent('relune:viewport-changed', currentState());
      };

      const zoomAt = (nextScale: number, localX: number, localY: number): void => {
        const clampedScale = clamp(nextScale, 0.1, 5);
        const scaleFactor = clampedScale / scale;
        panX = localX - (localX - panX) * scaleFactor;
        panY = localY - (localY - panY) * scaleFactor;
        scale = clampedScale;
        updateTransform();
      };

      const fitToScreen = (): void => {
        const rect = viewportEl.getBoundingClientRect();
        if (diagram.width <= 0 || diagram.height <= 0 || rect.width <= 0 || rect.height <= 0) {
          return;
        }

        const padding = 64;
        scale = clamp(
          Math.min(
            (rect.width - padding) / diagram.width,
            (rect.height - padding) / diagram.height,
          ),
          0.1,
          5,
        );
        panX = rect.width / 2 - (diagram.x + diagram.width / 2) * scale;
        panY = rect.height / 2 - (diagram.y + diagram.height / 2) * scale;
        updateTransform();
      };

      const centerOnContent = (contentX: number, contentY: number): void => {
        const rect = viewportEl.getBoundingClientRect();
        panX = rect.width / 2 - contentX * scale;
        panY = rect.height / 2 - contentY * scale;
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
          if (event.touches.length === 1) {
            isDragging = true;
            touchStartX = event.touches[0].clientX;
            touchStartY = event.touches[0].clientY;
            startPanX = panX;
            startPanY = panY;
          } else if (event.touches.length === 2) {
            isDragging = false;
            const dx = event.touches[0].clientX - event.touches[1].clientX;
            const dy = event.touches[0].clientY - event.touches[1].clientY;
            touchStartDist = Math.sqrt(dx * dx + dy * dy);
            touchStartScale = scale;
          }
        },
        { passive: true },
      );

      viewportEl.addEventListener(
        'touchmove',
        (event: TouchEvent) => {
          if (event.touches.length === 1 && isDragging) {
            panX = startPanX + (event.touches[0].clientX - touchStartX);
            panY = startPanY + (event.touches[0].clientY - touchStartY);
            updateTransform();
          } else if (event.touches.length === 2) {
            event.preventDefault();
            const dx = event.touches[0].clientX - event.touches[1].clientX;
            const dy = event.touches[0].clientY - event.touches[1].clientY;
            const dist = Math.sqrt(dx * dx + dy * dy);
            const rect = viewportEl.getBoundingClientRect();
            const midX = (event.touches[0].clientX + event.touches[1].clientX) / 2 - rect.left;
            const midY = (event.touches[0].clientY + event.touches[1].clientY) / 2 - rect.top;
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
