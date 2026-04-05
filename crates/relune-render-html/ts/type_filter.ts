import { syncEdgeDimming } from './edge_filters';
import { parseReluneMetadata, type TableMetadata } from './metadata';
import { emitViewerEvent, getViewerRuntime } from './viewer_api';
import {
  createTypeFilterState,
  tableMatchesAnySelectedType,
  visibleTypesForQuery,
  selectedTypeList,
  activeTypes,
} from './type_filter_state';
import { rebuildFilterList, syncFilterChrome } from './type_filter_dom';

{
  const section = document.getElementById('type-filter-section');
  const listEl = document.getElementById('type-filter-list');
  const svgEl = document.querySelector('.canvas svg');

  if (section === null || listEl === null || svgEl === null) {
    // Type filter markup or SVG not present.
  } else {
    const runtime = getViewerRuntime();
    const listRoot = listEl;
    const svgRoot = svgEl;
    const summaryEl = document.getElementById('type-filter-summary');
    const clearBtn = document.getElementById('type-filter-clear');
    const selectVisibleBtn = document.getElementById('type-filter-select-visible');
    const queryInput = document.getElementById('type-filter-query');
    const resetBar = document.getElementById('filter-reset-bar');
    const resetCopy = document.getElementById('filter-reset-copy');
    const resetButton = document.getElementById('filter-reset-button');

    const metadata = parseReluneMetadata();
    const tables: TableMetadata[] = metadata?.tables ?? [];
    const state = createTypeFilterState(tables);

    if (state.allTypes.length === 0) {
      section.setAttribute('hidden', '');
    } else {
      section.removeAttribute('hidden');
    }

    const getQuery = (): string => (queryInput instanceof HTMLInputElement ? queryInput.value : '');

    function rebuild(): void {
      const visible = visibleTypesForQuery(state.allTypes, getQuery());
      rebuildFilterList(
        visible,
        state.selectedTypes,
        state.typeTableCounts,
        listRoot,
        (dataType, checked) => {
          if (checked) {
            state.selectedTypes.add(dataType);
          } else {
            state.selectedTypes.delete(dataType);
          }
          applyTypeFilter();
        },
      );
    }

    function applyTypeFilter(): void {
      const nodes = svgRoot.querySelectorAll('.node');
      const effective = new Set(activeTypes(state.selectedTypes, state.allTypes, getQuery()));
      if (effective.size === 0) {
        nodes.forEach((node) => {
          node.classList.remove('dimmed-by-type-filter', 'excluded-by-type-filter');
        });
      } else {
        nodes.forEach((node) => {
          const tableId = node.getAttribute('data-id') ?? node.getAttribute('data-table-id') ?? '';
          const table = tables.find((c) => c.id === tableId);
          const matches = table !== undefined && tableMatchesAnySelectedType(table, effective);
          node.classList.toggle('dimmed-by-type-filter', !matches);
          node.classList.toggle('excluded-by-type-filter', !matches);
        });
      }

      const effectiveList = Array.from(effective);
      const selected = selectedTypeList(state.selectedTypes);
      const query = getQuery().trim();
      syncFilterChrome(
        effectiveList.length > 0,
        selected.length > 0,
        selected,
        effectiveList,
        query,
        summaryEl,
        resetBar,
        resetCopy,
      );
      syncEdgeDimming(svgRoot);

      emitViewerEvent('relune:filters-changed', {
        active: effectiveList.length > 0,
        selectedTypes: selected.length > 0 ? selected : effectiveList,
        query: selected.length > 0 ? '' : query,
      });
    }

    function clearSelection(): void {
      state.selectedTypes.clear();
      if (queryInput instanceof HTMLInputElement) {
        queryInput.value = '';
      }
      rebuild();
      applyTypeFilter();
    }

    runtime.filters = {
      reset(): void {
        clearSelection();
      },
      hasActiveFilters(): boolean {
        return activeTypes(state.selectedTypes, state.allTypes, getQuery()).length > 0;
      },
      setSelectedTypes(types: string[]): void {
        state.selectedTypes.clear();
        for (const t of types) {
          if (state.typeSet.has(t)) {
            state.selectedTypes.add(t);
          }
        }
        rebuild();
        applyTypeFilter();
      },
      getSelectedTypes(): string[] {
        return selectedTypeList(state.selectedTypes);
      },
      getAvailableTypes(): string[] {
        return [...state.allTypes];
      },
    };

    queryInput?.addEventListener('input', () => {
      rebuild();
      applyTypeFilter();
    });

    clearBtn?.addEventListener('click', clearSelection);
    resetButton?.addEventListener('click', clearSelection);
    selectVisibleBtn?.addEventListener('click', () => {
      const query = getQuery();
      for (const dataType of visibleTypesForQuery(state.allTypes, query)) {
        state.selectedTypes.add(dataType);
      }
      rebuild();
      applyTypeFilter();
    });

    rebuild();
    applyTypeFilter();
  }
}
