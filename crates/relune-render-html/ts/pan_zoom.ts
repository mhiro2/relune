import { emitViewerEvent, getViewerRuntime } from './viewer_api';
import {
  clamp,
  parseViewBox,
  clampPan,
  computeFit,
  computeZoomAt,
  buildViewportState,
} from './pan_zoom_state';
import { getAvailableViewport, applyTransform } from './pan_zoom_dom';

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

      const updateTransform = (): void => {
        const available = getAvailableViewport(viewportEl);
        const constrained = clampPan(panX, panY, scale, diagram, available);
        panX = constrained.panX;
        panY = constrained.panY;
        applyTransform(svg, canvasEl, zoomLevelEl, scale, panX, panY, diagram);
        emitViewerEvent(
          'relune:viewport-changed',
          buildViewportState(scale, panX, panY, viewportEl, diagram),
        );
      };

      const zoomAt = (nextScale: number, localX: number, localY: number): void => {
        const result = computeZoomAt(scale, panX, panY, nextScale, localX, localY);
        scale = result.scale;
        panX = result.panX;
        panY = result.panY;
        updateTransform();
      };

      const fitToScreen = (): void => {
        const available = getAvailableViewport(viewportEl);
        const result = computeFit(diagram, available);
        if (result === null) return;
        scale = result.scale;
        panX = result.panX;
        panY = result.panY;
        updateTransform();
      };

      const centerOnContent = (contentX: number, contentY: number): void => {
        const available = getAvailableViewport(viewportEl);
        panX = available.left + available.width / 2 - (contentX - diagram.x) * scale;
        panY = available.top + available.height / 2 - (contentY - diagram.y) * scale;
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
        getState() {
          return buildViewportState(scale, panX, panY, viewportEl, diagram);
        },
        getDiagramBounds() {
          return diagram;
        },
        setState(nextScale: number, nextPanX: number, nextPanY: number): void {
          scale = clamp(nextScale, 0.1, 2);
          panX = nextPanX;
          panY = nextPanY;
          updateTransform();
        },
      };

      // ── Mouse events ──────────────────────────────────────────────────

      viewportEl.addEventListener('mousedown', (event: MouseEvent) => {
        if (event.button !== 0) return;
        isDragging = true;
        startX = event.clientX;
        startY = event.clientY;
        startPanX = panX;
        startPanY = panY;
        viewportEl.style.cursor = 'grabbing';
        event.preventDefault();
      });

      document.addEventListener('mousemove', (event: MouseEvent) => {
        if (!isDragging) return;
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
          if (event.deltaY === 0) return;
          const rect = viewportEl.getBoundingClientRect();
          zoomAt(
            scale * (event.deltaY > 0 ? 0.9 : 1.1),
            event.clientX - rect.left,
            event.clientY - rect.top,
          );
        },
        { passive: false },
      );

      // ── Touch events ──────────────────────────────────────────────────

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

      // ── Buttons ───────────────────────────────────────────────────────

      zoomInBtn?.addEventListener('click', () => runtime.viewport?.zoomIn());
      zoomOutBtn?.addEventListener('click', () => runtime.viewport?.zoomOut());
      zoomFitBtn?.addEventListener('click', () => fitToScreen());

      window.addEventListener('resize', fitToScreen);
      requestAnimationFrame(fitToScreen);
    }
  }
}
