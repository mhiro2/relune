import { syncEdgeDimming } from './edge_filters';
import { parseReluneMetadata, type TableMetadata } from './metadata';
import { emitViewerEvent, getViewerRuntime, markViewerModuleReady } from './viewer_api';
import {
  createFilterEngineState,
  tableMatchesAllFacets,
  hasActiveFilters,
  activeFilterSummary,
  type FacetId,
  type FilterMode,
} from './filter_engine_state';
import {
  buildFilterModeSwitcher,
  syncModeSwitcher,
  buildFacetSection,
  rebuildFacetCheckboxes,
  rebuildColumnTypeFacet,
  syncFacetBadge,
  renderActiveFilterSummary,
  syncFilterResetBar,
} from './filter_engine_dom';

{
  const sectionEl = document.getElementById('filter-section');
  const headerEl = document.getElementById('filter-section-header');
  const summaryEl = document.getElementById('filter-active-summary');
  const facetsEl = document.getElementById('filter-facets');
  const svgEl = document.querySelector('.canvas svg');
  const resetBar = document.getElementById('filter-reset-bar');
  const resetCopy = document.getElementById('filter-reset-copy');
  const resetButton = document.getElementById('filter-reset-button');

  if (
    sectionEl === null ||
    headerEl === null ||
    summaryEl === null ||
    facetsEl === null ||
    svgEl === null
  ) {
    // Filter section or SVG not present — skip
  } else {
    const runtime = getViewerRuntime();
    const svgRoot = svgEl;
    const summaryRoot = summaryEl;
    const metadata = parseReluneMetadata();
    const tables: TableMetadata[] = metadata?.tables ?? [];
    const state = createFilterEngineState(tables);

    if (state.facets.size === 0) {
      sectionEl.hidden = true;
    } else {
      sectionEl.hidden = false;
    }

    // ── Build header ────────────────────────────────────────────────

    const titleSpan = document.createElement('span');
    titleSpan.textContent = 'Filters';

    const modeSwitcher = buildFilterModeSwitcher(state.mode, (mode) => {
      state.mode = mode;
      syncModeSwitcher(modeSwitcher, mode);
      applyFilter();
    });

    const resetAllBtn = document.createElement('button');
    resetAllBtn.type = 'button';
    resetAllBtn.className = 'filter-section-reset';
    resetAllBtn.textContent = 'Reset';
    resetAllBtn.hidden = true;
    resetAllBtn.addEventListener('click', clearAll);

    headerEl.append(titleSpan, modeSwitcher, resetAllBtn);

    // ── Per-facet search query state (columnType only) ──────────────

    const columnTypeQuery: { value: string } = { value: '' };

    // ── Build facet sections ────────────────────────────────────────

    const facetDetails = new Map<FacetId, HTMLDetailsElement>();

    for (const facet of state.facets.values()) {
      const onChange = (value: string, checked: boolean): void => {
        if (checked) {
          facet.selectedValues.add(value);
        } else {
          facet.selectedValues.delete(value);
        }
        applyFilter();
      };

      const onSearchInput =
        facet.id === 'columnType'
          ? (query: string): void => {
              columnTypeQuery.value = query;
              rebuildColumnTypeFacet(details, facet, query, onChange);
            }
          : undefined;

      const details = buildFacetSection(facet, onChange, onSearchInput);
      if (facet.allValues.length <= 5) {
        details.open = true;
      }
      facetDetails.set(facet.id, details);

      // Initial checkbox build
      if (facet.id === 'columnType') {
        rebuildColumnTypeFacet(details, facet, '', onChange);
      } else {
        rebuildFacetCheckboxes(
          details,
          facet.allValues,
          facet.selectedValues,
          facet.counts,
          onChange,
        );
      }

      facetsEl.appendChild(details);
    }

    // ── Apply filter ────────────────────────────────────────────────

    function applyFilter(): void {
      const nodes = svgRoot.querySelectorAll('.node');
      const active = hasActiveFilters(state);

      if (!active) {
        nodes.forEach((node) => {
          node.classList.remove('dimmed-by-filter', 'hidden-by-filter');
        });
      } else {
        const dimClass = state.mode === 'dim' ? 'dimmed-by-filter' : 'hidden-by-filter';
        const removeClass = state.mode === 'dim' ? 'hidden-by-filter' : 'dimmed-by-filter';

        nodes.forEach((node) => {
          const tableId = node.getAttribute('data-id') ?? node.getAttribute('data-table-id') ?? '';
          const table = tables.find((t) => t.id === tableId);
          const matches = table !== undefined && tableMatchesAllFacets(table, state);
          node.classList.toggle(dimClass, !matches);
          node.classList.remove(removeClass);
        });
      }

      // Sync edge dimming/hiding
      syncEdgeDimming(svgRoot);

      // Focus mode: fit viewport to visible nodes
      if (active && state.mode === 'focus') {
        fitToVisibleNodes();
      }

      // Sync UI
      const summaryItems = activeFilterSummary(state);

      for (const [facetId, details] of facetDetails) {
        const facet = state.facets.get(facetId);
        if (facet) {
          syncFacetBadge(details, facet.selectedValues.size);
        }
      }

      renderActiveFilterSummary(summaryRoot, summaryItems, (facetId) => {
        const details = facetDetails.get(facetId);
        if (details) {
          details.open = true;
          details.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
        }
      });

      resetAllBtn.hidden = !active;

      syncFilterResetBar(active, summaryItems, state.mode, resetBar, resetCopy);

      emitViewerEvent('relune:filters-changed', {
        active,
        mode: state.mode,
        facets: summaryItems,
      });
    }

    function clearAll(): void {
      for (const facet of state.facets.values()) {
        facet.selectedValues.clear();
      }
      columnTypeQuery.value = '';

      // Reset search inputs and rebuild checkboxes
      for (const [facetId, details] of facetDetails) {
        const facet = state.facets.get(facetId);
        if (!facet) continue;

        const searchInput = details.querySelector<HTMLInputElement>('.filter-facet-search');
        if (searchInput) {
          searchInput.value = '';
        }

        const onChange = (value: string, checked: boolean): void => {
          if (checked) {
            facet.selectedValues.add(value);
          } else {
            facet.selectedValues.delete(value);
          }
          applyFilter();
        };

        if (facetId === 'columnType') {
          rebuildColumnTypeFacet(details, facet, '', onChange);
        } else {
          rebuildFacetCheckboxes(
            details,
            facet.allValues,
            facet.selectedValues,
            facet.counts,
            onChange,
          );
        }
      }

      applyFilter();
    }

    function fitToVisibleNodes(): void {
      const nodes = svgRoot.querySelectorAll('.node:not(.hidden-by-filter)');
      if (nodes.length === 0) {
        runtime.viewport?.fit();
        return;
      }

      let minX = Infinity;
      let minY = Infinity;
      let maxX = -Infinity;
      let maxY = -Infinity;

      for (const node of nodes) {
        if (node instanceof SVGGraphicsElement) {
          const bbox = node.getBBox();
          minX = Math.min(minX, bbox.x);
          minY = Math.min(minY, bbox.y);
          maxX = Math.max(maxX, bbox.x + bbox.width);
          maxY = Math.max(maxY, bbox.y + bbox.height);
        }
      }

      if (minX < maxX && minY < maxY) {
        runtime.viewport?.fitToRect({ x: minX, y: minY, width: maxX - minX, height: maxY - minY });
      }
    }

    // ── Reset button ────────────────────────────────────────────────

    resetButton?.addEventListener('click', clearAll);

    // ── Runtime API ─────────────────────────────────────────────────

    runtime.filters = {
      reset(): void {
        clearAll();
      },
      hasActiveFilters(): boolean {
        return hasActiveFilters(state);
      },
      getMode(): FilterMode {
        return state.mode;
      },
      setMode(mode: FilterMode): void {
        state.mode = mode;
        syncModeSwitcher(modeSwitcher, mode);
        applyFilter();
      },
      getFacetSelection(facetId: FacetId): string[] {
        const facet = state.facets.get(facetId);
        return facet ? [...facet.selectedValues].sort() : [];
      },
      setFacetSelection(facetId: FacetId, values: string[]): void {
        const facet = state.facets.get(facetId);
        if (!facet) return;
        facet.selectedValues.clear();
        for (const v of values) {
          if (facet.allValues.includes(v)) {
            facet.selectedValues.add(v);
          }
        }

        // Rebuild checkboxes
        const details = facetDetails.get(facetId);
        if (details) {
          const onChange = (value: string, checked: boolean): void => {
            if (checked) {
              facet.selectedValues.add(value);
            } else {
              facet.selectedValues.delete(value);
            }
            applyFilter();
          };

          if (facetId === 'columnType') {
            rebuildColumnTypeFacet(details, facet, columnTypeQuery.value, onChange);
          } else {
            rebuildFacetCheckboxes(
              details,
              facet.allValues,
              facet.selectedValues,
              facet.counts,
              onChange,
            );
          }
        }

        applyFilter();
      },
      getAvailableFacets(): FacetId[] {
        return [...state.facets.keys()];
      },
    };
    markViewerModuleReady('filters');

    applyFilter();
  }
}
