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

    switch (event.key) {
      case '/':
        event.preventDefault();
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
        document.getElementById('group-panel-collapse')?.dispatchEvent(new MouseEvent('click'));
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
