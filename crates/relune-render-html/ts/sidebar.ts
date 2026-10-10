import { getViewerRuntime, readSessionFlag, writeSessionFlag } from './viewer_api';

// The explorer sidebar can be put away to give the diagram the whole
// window; a small button in its place brings it back.
{
  const panel = document.getElementById('search-panel');
  const collapseButton = document.getElementById('sidebar-collapse');
  const openButton = document.getElementById('sidebar-open');

  if (panel instanceof HTMLElement && openButton instanceof HTMLButtonElement) {
    const COLLAPSED_KEY = 'relune-sidebar-collapsed';

    const isCollapsed = (): boolean => panel.hasAttribute('hidden');
    const setCollapsed = (collapsed: boolean): void => {
      if (collapsed === isCollapsed()) return;
      panel.toggleAttribute('hidden', collapsed);
      openButton.toggleAttribute('hidden', !collapsed);
      collapseButton?.setAttribute('aria-expanded', String(!collapsed));
      openButton.setAttribute('aria-expanded', String(!collapsed));
      writeSessionFlag(COLLAPSED_KEY, collapsed);
    };

    collapseButton?.addEventListener('click', () => {
      setCollapsed(true);
      openButton.focus();
    });
    openButton.addEventListener('click', () => {
      setCollapsed(false);
      collapseButton?.focus();
    });

    if (readSessionFlag(COLLAPSED_KEY) === true) {
      setCollapsed(true);
    }

    getViewerRuntime().sidebar = { isCollapsed, setCollapsed };
  }
}
