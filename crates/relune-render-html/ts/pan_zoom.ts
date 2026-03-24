{
  const viewportEl = document.getElementById('viewport');
  const canvasEl = document.getElementById('canvas');
  if (viewportEl && canvasEl && canvasEl instanceof HTMLElement) {
    let scale = 1;
    let panX = 0;
    let panY = 0;
    let isDragging = false;
    let startX = 0;
    let startY = 0;
    let startPanX = 0;
    let startPanY = 0;

    const updateTransform = (): void => {
      canvasEl.style.transform = `translate(${panX}px, ${panY}px) scale(${scale})`;
    };

    const clamp = (value: number, min: number, max: number): number =>
      Math.max(min, Math.min(max, value));

    const centerContent = (): void => {
      const rect = viewportEl.getBoundingClientRect();
      const svg = canvasEl.querySelector('svg');
      if (svg) {
        const vb = svg.getAttribute('viewBox');
        if (vb) {
          const parts = vb.split(/\s+/).map(Number);
          const width = parts[2] ?? 0;
          const height = parts[3] ?? 0;
          panX = (rect.width - width * scale) / 2;
          panY = (rect.height - height * scale) / 2;
          updateTransform();
        }
      }
    };

    viewportEl.addEventListener('mousedown', (e: MouseEvent) => {
      if (e.button !== 0) {
        return;
      }
      isDragging = true;
      startX = e.clientX;
      startY = e.clientY;
      startPanX = panX;
      startPanY = panY;
      viewportEl.style.cursor = 'grabbing';
      e.preventDefault();
    });

    document.addEventListener('mousemove', (e: MouseEvent) => {
      if (!isDragging) {
        return;
      }
      const dx = e.clientX - startX;
      const dy = e.clientY - startY;
      panX = startPanX + dx;
      panY = startPanY + dy;
      updateTransform();
    });

    document.addEventListener('mouseup', () => {
      isDragging = false;
      viewportEl.style.cursor = 'grab';
    });

    viewportEl.addEventListener(
      'wheel',
      (e: WheelEvent) => {
        e.preventDefault();

        const rect = viewportEl.getBoundingClientRect();
        const mouseX = e.clientX - rect.left;
        const mouseY = e.clientY - rect.top;

        const oldScale = scale;
        const delta = e.deltaY > 0 ? 0.9 : 1.1;
        scale = clamp(scale * delta, 0.1, 5);

        const scaleFactor = scale / oldScale;
        panX = mouseX - (mouseX - panX) * scaleFactor;
        panY = mouseY - (mouseY - panY) * scaleFactor;

        updateTransform();
      },
      { passive: false },
    );

    let touchStartDist = 0;
    let touchStartScale = 1;
    let touchStartPanX = 0;
    let touchStartPanY = 0;
    let touchStartX = 0;
    let touchStartY = 0;

    viewportEl.addEventListener(
      'touchstart',
      (e: TouchEvent) => {
        if (e.touches.length === 1) {
          isDragging = true;
          touchStartX = e.touches[0].clientX;
          touchStartY = e.touches[0].clientY;
          startPanX = panX;
          startPanY = panY;
        } else if (e.touches.length === 2) {
          isDragging = false;
          const dx = e.touches[0].clientX - e.touches[1].clientX;
          const dy = e.touches[0].clientY - e.touches[1].clientY;
          touchStartDist = Math.sqrt(dx * dx + dy * dy);
          touchStartScale = scale;
          touchStartPanX = panX;
          touchStartPanY = panY;
        }
      },
      { passive: true },
    );

    viewportEl.addEventListener(
      'touchmove',
      (e: TouchEvent) => {
        if (e.touches.length === 1 && isDragging) {
          const dx = e.touches[0].clientX - touchStartX;
          const dy = e.touches[0].clientY - touchStartY;
          panX = startPanX + dx;
          panY = startPanY + dy;
          updateTransform();
        } else if (e.touches.length === 2) {
          e.preventDefault();
          const dx = e.touches[0].clientX - e.touches[1].clientX;
          const dy = e.touches[0].clientY - e.touches[1].clientY;
          const dist = Math.sqrt(dx * dx + dy * dy);
          const midX = (e.touches[0].clientX + e.touches[1].clientX) / 2;
          const midY = (e.touches[0].clientY + e.touches[1].clientY) / 2;

          const rect = viewportEl.getBoundingClientRect();
          const localX = midX - rect.left;
          const localY = midY - rect.top;

          const oldScale = scale;
          scale = clamp(touchStartScale * (dist / touchStartDist), 0.1, 5);
          const scaleFactor = scale / oldScale;

          panX = localX - (localX - touchStartPanX) * scaleFactor;
          panY = localY - (localY - touchStartPanY) * scaleFactor;

          updateTransform();
        }
      },
      { passive: false },
    );

    viewportEl.addEventListener('touchend', () => {
      isDragging = false;
    });

    setTimeout(centerContent, 0);
    window.addEventListener('resize', centerContent);
  }
}
