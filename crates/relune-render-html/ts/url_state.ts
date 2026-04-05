import { getViewerRuntime } from './viewer_api';

{
  const runtime = getViewerRuntime();

  const PARAM_SEARCH = 'q';
  const PARAM_TABLE = 't';
  const PARAM_SCALE = 's';
  const PARAM_PAN_X = 'x';
  const PARAM_PAN_Y = 'y';
  const PARAM_TYPES = 'types';
  const PARAM_HIDDEN_GROUPS = 'hg';

  // ---------------------------------------------------------------------------
  // Read from URL hash
  // ---------------------------------------------------------------------------

  function readHash(): URLSearchParams {
    const raw = location.hash.replace(/^#/, '');
    return new URLSearchParams(raw);
  }

  function parseAllowedTypes(typesRaw: string, allowedTypes: ReadonlySet<string>): string[] {
    const selected = new Set<string>();
    for (const type of typesRaw.split(',')) {
      const candidate = type.trim();
      if (candidate !== '' && allowedTypes.has(candidate)) {
        selected.add(candidate);
      }
    }
    return [...selected];
  }

  // ---------------------------------------------------------------------------
  // Write to URL hash (debounced)
  // ---------------------------------------------------------------------------

  let writeTimer: ReturnType<typeof setTimeout> | null = null;

  function scheduleWrite(): void {
    if (writeTimer !== null) {
      clearTimeout(writeTimer);
    }
    writeTimer = setTimeout(writeHash, 300);
  }

  function writeHash(): void {
    const params = new URLSearchParams();

    const query = runtime.search?.getQuery() ?? '';
    if (query !== '') {
      params.set(PARAM_SEARCH, query);
    }

    const selected = runtime.selection?.getSelected() ?? null;
    if (selected !== null) {
      params.set(PARAM_TABLE, selected);
    }

    const viewport = runtime.viewport?.getState();
    if (viewport !== null && viewport !== undefined) {
      params.set(PARAM_SCALE, viewport.scale.toFixed(4));
      params.set(PARAM_PAN_X, viewport.panX.toFixed(1));
      params.set(PARAM_PAN_Y, viewport.panY.toFixed(1));
    }

    const types = runtime.filters?.getSelectedTypes() ?? [];
    if (types.length > 0) {
      params.set(PARAM_TYPES, types.join(','));
    }

    const hiddenGroups = runtime.groups?.getHiddenGroups() ?? [];
    if (hiddenGroups.length > 0) {
      params.set(PARAM_HIDDEN_GROUPS, hiddenGroups.join(','));
    }

    const str = params.toString();
    const newHash = str === '' ? '' : `#${str}`;
    if (newHash !== location.hash && newHash !== '#') {
      history.replaceState(null, '', newHash || location.pathname + location.search);
    }
  }

  // ---------------------------------------------------------------------------
  // Restore state from URL hash on load
  // ---------------------------------------------------------------------------

  function restoreFromHash(): void {
    const params = readHash();
    if (params.toString() === '') {
      return;
    }

    // Restore viewport first (before selection centering overrides it)
    const s = params.get(PARAM_SCALE);
    const x = params.get(PARAM_PAN_X);
    const y = params.get(PARAM_PAN_Y);
    if (s !== null && x !== null && y !== null) {
      const scale = Number.parseFloat(s);
      const panX = Number.parseFloat(x);
      const panY = Number.parseFloat(y);
      if (Number.isFinite(scale) && Number.isFinite(panX) && Number.isFinite(panY)) {
        runtime.viewport?.setState(scale, panX, panY);
      }
    }

    // Restore search query
    const query = params.get(PARAM_SEARCH);
    if (query !== null && query !== '') {
      runtime.search?.setQuery(query);
    }

    // Restore type filters
    const typesRaw = params.get(PARAM_TYPES);
    if (typesRaw !== null && typesRaw !== '') {
      const allowedTypes = new Set(runtime.filters?.getAvailableTypes() ?? []);
      const types = parseAllowedTypes(typesRaw, allowedTypes);
      if (types.length > 0) {
        runtime.filters?.setSelectedTypes(types);
      }
    }

    // Restore hidden groups
    const hgRaw = params.get(PARAM_HIDDEN_GROUPS);
    if (hgRaw !== null && hgRaw !== '') {
      const hiddenGroups = hgRaw.split(',').filter((g) => g !== '');
      for (const groupId of hiddenGroups) {
        runtime.groups?.setVisibility(groupId, false);
      }
    }

    // Restore selected table (last, so it can center on restored viewport scale)
    const table = params.get(PARAM_TABLE);
    if (table !== null && table !== '') {
      runtime.selection?.select(table);
    }
  }

  // ---------------------------------------------------------------------------
  // Listen for state changes and update URL
  // ---------------------------------------------------------------------------

  document.addEventListener('relune:search-changed', scheduleWrite);
  document.addEventListener('relune:node-selected', scheduleWrite);
  document.addEventListener('relune:node-cleared', scheduleWrite);
  document.addEventListener('relune:viewport-changed', scheduleWrite);
  document.addEventListener('relune:filters-changed', scheduleWrite);
  document.addEventListener('relune:groups-changed', scheduleWrite);

  // ---------------------------------------------------------------------------
  // Init: restore state after all modules have initialised
  // ---------------------------------------------------------------------------

  requestAnimationFrame(() => {
    restoreFromHash();
  });
}
