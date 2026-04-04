import { syncEdgeDimming } from './edge_filters';
import { parseReluneMetadata, type TableMetadata } from './metadata';
import { emitViewerEvent, getViewerRuntime } from './viewer_api';

function columnMatchesSelectedType(columnType: string, selectedType: string): boolean {
  const column = columnType.trim().toLowerCase();
  const selected = selectedType.trim().toLowerCase();
  if (column === selected) {
    return true;
  }

  const base = (raw: string): string => {
    const index = raw.indexOf('(');
    return (index === -1 ? raw : raw.slice(0, index)).trim();
  };

  const baseColumn = base(column);
  const baseSelected = base(selected);
  return baseColumn === baseSelected || column.includes(selected) || selected.includes(column);
}

function tableMatchesAnySelectedType(table: TableMetadata, selectedTypes: Set<string>): boolean {
  return (table.columns ?? []).some((column) =>
    Array.from(selectedTypes).some((selectedType) =>
      columnMatchesSelectedType(column.data_type ?? '', selectedType),
    ),
  );
}

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
    const typeSet = new Set<string>();
    const selectedTypes = new Set<string>();
    const typeTableCounts = new Map<string, number>();

    for (const table of tables) {
      const tableTypes = new Set<string>();
      for (const column of table.columns ?? []) {
        const dataType = (column.data_type ?? '').trim();
        if (dataType !== '') {
          typeSet.add(dataType);
          tableTypes.add(dataType);
        }
      }
      tableTypes.forEach((dataType) => {
        typeTableCounts.set(dataType, (typeTableCounts.get(dataType) ?? 0) + 1);
      });
    }

    const allTypes = Array.from(typeSet).sort((left, right) =>
      left.localeCompare(right, undefined, { sensitivity: 'base' }),
    );

    if (allTypes.length === 0) {
      section.setAttribute('hidden', '');
    } else {
      section.removeAttribute('hidden');
    }

    function visibleTypesForQuery(query: string): string[] {
      const needle = query.trim().toLowerCase();
      if (needle === '') {
        return allTypes;
      }
      return allTypes.filter((dataType) => dataType.toLowerCase().includes(needle));
    }

    function selectedTypeList(): string[] {
      return Array.from(selectedTypes).sort((left, right) =>
        left.localeCompare(right, undefined, { sensitivity: 'base' }),
      );
    }

    function activeTypes(): string[] {
      if (selectedTypes.size > 0) {
        return selectedTypeList();
      }

      const query = queryInput instanceof HTMLInputElement ? queryInput.value : '';
      return query.trim() === '' ? [] : visibleTypesForQuery(query);
    }

    function syncFilterChrome(activeTypeList: string[]): void {
      const selected = selectedTypeList();
      const query = queryInput instanceof HTMLInputElement ? queryInput.value.trim() : '';
      const hasExplicitSelection = selected.length > 0;
      const hasActiveFilter = activeTypeList.length > 0;

      if (summaryEl) {
        if (!hasActiveFilter) {
          summaryEl.textContent = '';
          summaryEl.classList.remove('visible');
        } else {
          summaryEl.textContent = hasExplicitSelection
            ? `${selected.length} type(s) selected across the schema`
            : `${activeTypeList.length} matching type(s) for "${query}"`;
          summaryEl.classList.add('visible');
        }
      }

      if (resetBar && resetCopy) {
        resetBar.toggleAttribute('hidden', !hasActiveFilter);
        if (hasExplicitSelection) {
          const preview = selected.slice(0, 3).join(', ');
          const suffix = selected.length > 3 ? ` +${selected.length - 3} more` : '';
          resetCopy.textContent = `${selected.length} type filter(s): ${preview}${suffix}`;
        } else if (hasActiveFilter) {
          const preview = activeTypeList.slice(0, 3).join(', ');
          const suffix = activeTypeList.length > 3 ? ` +${activeTypeList.length - 3} more` : '';
          resetCopy.textContent = `Type query "${query}": ${preview}${suffix}`;
        } else {
          resetCopy.textContent = '';
        }
      }

      emitViewerEvent('relune:filters-changed', {
        active: hasActiveFilter,
        selectedTypes: hasExplicitSelection ? selected : activeTypeList,
        query: hasExplicitSelection ? '' : query,
      });
    }

    function rebuildList(): void {
      listRoot.innerHTML = '';
      const query = queryInput instanceof HTMLInputElement ? queryInput.value : '';
      const visibleTypes = visibleTypesForQuery(query);

      for (const dataType of visibleTypes) {
        const row = document.createElement('label');
        row.className = 'type-filter-item';

        const checkbox = document.createElement('input');
        checkbox.type = 'checkbox';
        checkbox.value = dataType;
        checkbox.checked = selectedTypes.has(dataType);
        checkbox.addEventListener('change', () => {
          if (checkbox.checked) {
            selectedTypes.add(dataType);
          } else {
            selectedTypes.delete(dataType);
          }
          applyTypeFilter();
        });

        const label = document.createElement('span');
        label.textContent = dataType;

        const count = document.createElement('span');
        count.className = 'type-filter-item-count';
        count.textContent = String(typeTableCounts.get(dataType) ?? 0);

        row.appendChild(checkbox);
        row.appendChild(label);
        row.appendChild(count);
        listRoot.appendChild(row);
      }
    }

    function applyTypeFilter(): void {
      const nodes = svgRoot.querySelectorAll('.node');
      const effectiveTypes = new Set(activeTypes());
      if (effectiveTypes.size === 0) {
        nodes.forEach((node) => {
          node.classList.remove('dimmed-by-type-filter', 'excluded-by-type-filter');
        });
      } else {
        nodes.forEach((node) => {
          const tableId = node.getAttribute('data-id') ?? node.getAttribute('data-table-id') ?? '';
          const table = tables.find((candidate) => candidate.id === tableId);
          const matches = table !== undefined && tableMatchesAnySelectedType(table, effectiveTypes);
          node.classList.toggle('dimmed-by-type-filter', !matches);
          node.classList.toggle('excluded-by-type-filter', !matches);
        });
      }

      syncFilterChrome(Array.from(effectiveTypes));
      syncEdgeDimming(svgRoot);
    }

    function clearSelection(): void {
      selectedTypes.clear();
      if (queryInput instanceof HTMLInputElement) {
        queryInput.value = '';
      }
      rebuildList();
      applyTypeFilter();
    }

    runtime.filters = {
      reset(): void {
        clearSelection();
      },
      hasActiveFilters(): boolean {
        return activeTypes().length > 0;
      },
      setSelectedTypes(types: string[]): void {
        selectedTypes.clear();
        for (const t of types) {
          if (typeSet.has(t)) {
            selectedTypes.add(t);
          }
        }
        rebuildList();
        applyTypeFilter();
      },
      getSelectedTypes(): string[] {
        return selectedTypeList();
      },
    };

    queryInput?.addEventListener('input', () => {
      rebuildList();
      applyTypeFilter();
    });

    clearBtn?.addEventListener('click', clearSelection);
    resetButton?.addEventListener('click', clearSelection);
    selectVisibleBtn?.addEventListener('click', () => {
      const query = queryInput instanceof HTMLInputElement ? queryInput.value : '';
      for (const dataType of visibleTypesForQuery(query)) {
        selectedTypes.add(dataType);
      }
      rebuildList();
      applyTypeFilter();
    });

    rebuildList();
    applyTypeFilter();
  }
}
