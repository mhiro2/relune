import { getViewerRuntime, isEditableTarget } from './viewer_api';

{
  const runtime = getViewerRuntime();

  document.addEventListener('keydown', (event: KeyboardEvent) => {
    if (isEditableTarget(event.target)) {
      if (event.key === 'Escape') {
        runtime.search?.clear();
      }
      return;
    }
    // Leave Cmd/Ctrl/Alt chords (find, zoom, window management) to the
    // browser and OS; Shift stays allowed because `+` and `F` need it.
    if (event.ctrlKey || event.metaKey || event.altKey || event.isComposing) {
      return;
    }

    switch (event.key) {
      case '/':
        event.preventDefault();
        runtime.sidebar?.setCollapsed(false);
        runtime.search?.focus();
        break;
      case 'Escape':
        runtime.search?.clear();
        runtime.filters?.reset();
        runtime.selection?.clear();
        break;
      case 'f':
      case 'F':
        event.preventDefault();
        runtime.viewport?.fit();
        break;
      case 'g':
      case 'G':
        event.preventDefault();
        if (runtime.groups !== undefined) {
          // Bringing back a hidden sidebar always shows the groups.
          const reveal = runtime.sidebar?.isCollapsed() === true;
          runtime.sidebar?.setCollapsed(false);
          runtime.groups.setPanelOpen(reveal || !runtime.groups.isPanelOpen());
        }
        break;
      case 's':
      case 'S':
        event.preventDefault();
        if (runtime.sidebar !== undefined) {
          runtime.sidebar.setCollapsed(!runtime.sidebar.isCollapsed());
        }
        break;
      case 'm':
      case 'M':
        event.preventDefault();
        if (runtime.minimap !== undefined) {
          runtime.minimap.setHidden(!runtime.minimap.isHidden());
          runtime.viewport?.fit();
        }
        break;
      case '+':
      case '=':
        event.preventDefault();
        runtime.viewport?.zoomIn();
        break;
      case '-':
      case '_':
        event.preventDefault();
        runtime.viewport?.zoomOut();
        break;
      default:
        break;
    }
  });
}
