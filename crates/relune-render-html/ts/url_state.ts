import { parseReluneMetadata, type TableMetadata } from './metadata';
import { matchesTableQuery } from './search_actions';
import { getViewerRuntime, waitForViewerModules, type ViewerModule } from './viewer_api';

{
  const runtime = getViewerRuntime();
  const metadata = parseReluneMetadata();
  const tables: TableMetadata[] = metadata?.tables ?? [];
  const tableIds = new Set(tables.map((table) => table.id));

  const PARAM_SEARCH = 'q';
  const PARAM_TABLE = 't';
  const PARAM_RELATION = 'r';
  const PARAM_SCALE = 's';
  const PARAM_PAN_X = 'x';
  const PARAM_PAN_Y = 'y';
  const PARAM_FILTER_SCHEMA = 'fs';
  const PARAM_FILTER_KIND = 'fk';
  const PARAM_FILTER_TYPE = 'ft';
  const PARAM_FILTER_SEVERITY = 'fi';
  const PARAM_FILTER_DIFF = 'fd';
  const PARAM_FILTER_MODE = 'fm';
  const PARAM_HIDDEN_GROUPS = 'hg';
  const PARAM_MINIMAP_VISIBLE = 'mv';

  type FacetUrlParam = { param: string; facetId: string };
  const FACET_PARAMS: FacetUrlParam[] = [
    { param: PARAM_FILTER_SCHEMA, facetId: 'schema' },
    { param: PARAM_FILTER_KIND, facetId: 'kind' },
    { param: PARAM_FILTER_TYPE, facetId: 'columnType' },
    { param: PARAM_FILTER_SEVERITY, facetId: 'severity' },
    { param: PARAM_FILTER_DIFF, facetId: 'diffKind' },
  ];
  const MIN_VIEWPORT_SCALE = 0.1;
  const MAX_VIEWPORT_SCALE = 2;
  const MIN_VIEWPORT_PAN_LIMIT = 10_000;

  // ---------------------------------------------------------------------------
  // Read from URL hash
  // ---------------------------------------------------------------------------

  function readHash(): URLSearchParams {
    const raw = location.hash.replace(/^#/, '');
    return new URLSearchParams(raw);
  }

  function maxViewportPanMagnitude(): number {
    const bounds = runtime.viewport?.getDiagramBounds();
    if (bounds === null || bounds === undefined) {
      return MIN_VIEWPORT_PAN_LIMIT;
    }
    const extent = Math.max(Math.abs(bounds.x), Math.abs(bounds.y), bounds.width, bounds.height, 1);
    return Math.max(extent * MAX_VIEWPORT_SCALE * 4, MIN_VIEWPORT_PAN_LIMIT);
  }

  function hasValidViewportState(scale: number, panX: number, panY: number): boolean {
    return (
      Number.isFinite(scale) &&
      Number.isFinite(panX) &&
      Number.isFinite(panY) &&
      scale >= MIN_VIEWPORT_SCALE &&
      scale <= MAX_VIEWPORT_SCALE &&
      Math.abs(panX) <= maxViewportPanMagnitude() &&
      Math.abs(panY) <= maxViewportPanMagnitude()
    );
  }

  // List values are written as repeated parameters (`ft=a&ft=b`) rather than
  // joined with a separator, since column types such as `numeric(10,2)` and
  // quoted table names can contain any character.
  function appendList(params: URLSearchParams, param: string, values: readonly string[]): void {
    for (const value of values) {
      params.append(param, value);
    }
  }

  function readList(params: URLSearchParams, param: string): string[] {
    return params.getAll(param).filter((value) => value !== '');
  }

  function hasMetadataSearchMatch(query: string): boolean {
    return query.trim() !== '' && tables.some((table) => matchesTableQuery(table, query));
  }

  // ---------------------------------------------------------------------------
  // Write to URL hash (debounced)
  // ---------------------------------------------------------------------------

  let writeTimer: ReturnType<typeof setTimeout> | null = null;
  let pendingPush = false;
  let restoringFromPopstate = false;

  function scheduleWrite(): void {
    if (writeTimer !== null) {
      clearTimeout(writeTimer);
    }
    writeTimer = setTimeout(writeHash, 300);
  }

  function scheduleDiscreteWrite(): void {
    pendingPush = true;
    scheduleWrite();
  }

  function buildHashParams(): URLSearchParams {
    const params = new URLSearchParams();

    const query = runtime.search?.getQuery() ?? '';
    if (query !== '') {
      params.set(PARAM_SEARCH, query);
    }

    const selected = runtime.selection?.getSelected() ?? null;
    if (selected !== null) {
      params.set(PARAM_TABLE, selected);
    }

    const relation = runtime.selection?.getSelectedRelation() ?? null;
    if (relation !== null) {
      params.set(PARAM_RELATION, relation);
    }

    const viewport = runtime.viewport?.getState();
    if (viewport !== null && viewport !== undefined) {
      params.set(PARAM_SCALE, viewport.scale.toFixed(4));
      params.set(PARAM_PAN_X, viewport.panX.toFixed(1));
      params.set(PARAM_PAN_Y, viewport.panY.toFixed(1));
    }

    for (const { param, facetId } of FACET_PARAMS) {
      appendList(params, param, runtime.filters?.getFacetSelection(facetId as any) ?? []);
    }

    const filterMode = runtime.filters?.getMode();
    if (filterMode !== undefined && filterMode !== 'dim') {
      params.set(PARAM_FILTER_MODE, filterMode);
    }

    appendList(params, PARAM_HIDDEN_GROUPS, runtime.groups?.getHiddenGroups() ?? []);

    if (runtime.minimap?.isHidden() === false) {
      params.set(PARAM_MINIMAP_VISIBLE, '1');
    }

    return params;
  }

  function writeHash(): void {
    const str = buildHashParams().toString();
    const newHash = str === '' ? '' : `#${str}`;
    if (newHash !== location.hash && newHash !== '#') {
      const url = newHash || location.pathname + location.search;
      try {
        if (pendingPush && !restoringFromPopstate) {
          history.pushState(null, '', url);
        } else {
          history.replaceState(null, '', url);
        }
      } catch {
        // Silently ignore in sandboxed iframes (e.g. srcdoc)
      }
    }
    pendingPush = false;
  }

  // ---------------------------------------------------------------------------
  // Restore state from URL hash on load
  // ---------------------------------------------------------------------------

  function restoreFromHash(): void {
    const params = readHash();
    // Sync minimap visibility unconditionally so popstate back to a clean hash
    // restores the hidden default instead of leaving it stuck visible. Use the
    // silent option to avoid emitting `relune:minimap-toggled` — that event
    // would queue a debounced hash write after restoringFromPopstate has
    // already flipped back to false, causing a spurious pushState.
    runtime.minimap?.setHidden(params.get(PARAM_MINIMAP_VISIBLE) !== '1', { silent: true });
    if (params.toString() === '') {
      // Going back to a clean hash drops whatever is still selected.
      runtime.selection?.clear();
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
      if (hasValidViewportState(scale, panX, panY)) {
        runtime.viewport?.setState(scale, panX, panY);
      }
    }

    // Restore search query
    const query = params.get(PARAM_SEARCH);
    if (query !== null && query !== '' && hasMetadataSearchMatch(query)) {
      runtime.search?.setQuery(query);
    }

    // Restore filter mode
    const fmRaw = params.get(PARAM_FILTER_MODE);
    if (fmRaw === 'hide' || fmRaw === 'focus') {
      runtime.filters?.setMode(fmRaw);
    }

    // Restore facet selections
    for (const { param, facetId } of FACET_PARAMS) {
      const values = readList(params, param);
      if (values.length > 0) {
        runtime.filters?.setFacetSelection(facetId as any, values);
      }
    }

    // Restore hidden groups
    for (const groupId of readList(params, PARAM_HIDDEN_GROUPS)) {
      runtime.groups?.setVisibility(groupId, false);
    }

    // Restore the selected relationship or table last, so it can center on
    // the restored viewport scale. The two are never selected together.
    const relation = params.get(PARAM_RELATION);
    if (relation !== null && relation !== '' && runtime.selection?.selectRelation(relation)) {
      return;
    }
    const table = params.get(PARAM_TABLE);
    if (table !== null && table !== '' && tableIds.has(table)) {
      runtime.selection?.select(table);
    } else {
      runtime.selection?.clear();
    }
  }

  function expectedViewerModules(): ViewerModule[] {
    const modules: ViewerModule[] = [];
    if (document.getElementById('zoom-fit') !== null) {
      modules.push('viewport');
    }
    if (document.getElementById('table-search') instanceof HTMLInputElement) {
      modules.push('search');
    }
    if (document.getElementById('filter-section') !== null) {
      modules.push('filters');
    }
    if (document.getElementById('detail-drawer') !== null) {
      modules.push('selection');
    }
    if ((metadata?.groups?.length ?? 0) > 0) {
      modules.push('groups');
    }
    if (document.getElementById('minimap-shell') !== null) {
      modules.push('minimap');
    }
    return modules;
  }

  // ---------------------------------------------------------------------------
  // Listen for state changes and update URL
  // ---------------------------------------------------------------------------

  document.addEventListener('relune:search-changed', scheduleDiscreteWrite);
  document.addEventListener('relune:node-selected', scheduleDiscreteWrite);
  document.addEventListener('relune:node-cleared', scheduleDiscreteWrite);
  document.addEventListener('relune:relation-selected', scheduleDiscreteWrite);
  document.addEventListener('relune:relation-cleared', scheduleDiscreteWrite);
  document.addEventListener('relune:viewport-changed', scheduleWrite);
  document.addEventListener('relune:filters-changed', scheduleDiscreteWrite);
  document.addEventListener('relune:groups-changed', scheduleDiscreteWrite);
  document.addEventListener('relune:minimap-toggled', scheduleDiscreteWrite);

  // ---------------------------------------------------------------------------
  // popstate: re-apply state when the user navigates back/forward or edits hash
  // ---------------------------------------------------------------------------

  window.addEventListener('popstate', () => {
    restoringFromPopstate = true;
    restoreFromHash();
    restoringFromPopstate = false;
  });

  // ---------------------------------------------------------------------------
  // Init: restore state after all modules have initialised
  // ---------------------------------------------------------------------------

  waitForViewerModules(expectedViewerModules(), restoreFromHash);
}
