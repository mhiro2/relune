import { syncEdgeDimming } from './edge_filters';
import { parseReluneMetadata, tableDisplayName, type TableMetadata } from './metadata';
import { emitViewerEvent, getViewerRuntime } from './viewer_api';

{
  const searchInput = document.getElementById('table-search');
  const searchClear = document.getElementById('search-clear');
  const searchResults = document.getElementById('search-results');
  const svgRoot = document.querySelector('.canvas svg');
  if (searchInput instanceof HTMLInputElement && svgRoot) {
    const runtime = getViewerRuntime();
    const metadata = parseReluneMetadata();
    const tables: TableMetadata[] = metadata?.tables ?? [];

    const tableNames: Record<string, string> = {};
    for (const table of tables) {
      tableNames[table.id] = tableDisplayName(table);
    }

    const performSearch = (query: string): void => {
      const q = query.toLowerCase().trim();
      const nodes = svgRoot.querySelectorAll('.node');
      let matchCount = 0;
      const totalCount = nodes.length;

      if (q === '') {
        nodes.forEach((node) => {
          node.classList.remove('dimmed-by-search', 'highlighted-by-search');
        });
        syncEdgeDimming(svgRoot);
        searchClear?.classList.remove('visible');
        searchResults?.classList.remove('visible');
        emitViewerEvent('relune:search-changed', {
          active: false,
          query: '',
          matches: totalCount,
          total: totalCount,
        });
        return;
      }

      searchClear?.classList.add('visible');

      nodes.forEach((node) => {
        const tableId = node.getAttribute('data-id') ?? node.getAttribute('data-table-id') ?? '';
        const tableName = tableNames[tableId] ?? tableId;
        const nodeText = node.textContent?.toLowerCase() ?? '';

        const matches =
          tableName.toLowerCase().includes(q) ||
          tableId.toLowerCase().includes(q) ||
          nodeText.includes(q);

        if (matches) {
          node.classList.remove('dimmed-by-search');
          node.classList.add('highlighted-by-search');
          matchCount += 1;
        } else {
          node.classList.remove('highlighted-by-search');
          node.classList.add('dimmed-by-search');
        }
      });

      syncEdgeDimming(svgRoot);

      if (searchResults) {
        searchResults.textContent = `${matchCount} of ${totalCount} objects`;
        searchResults.classList.add('visible');
      }

      emitViewerEvent('relune:search-changed', {
        active: true,
        query,
        matches: matchCount,
        total: totalCount,
      });
    };

    runtime.search = {
      focus(): void {
        searchInput.focus();
      },
      clear(): void {
        searchInput.value = '';
        performSearch('');
      },
      isActive(): boolean {
        return searchInput.value.trim() !== '';
      },
      setQuery(query: string): void {
        searchInput.value = query;
        performSearch(query);
      },
      getQuery(): string {
        return searchInput.value;
      },
    };

    let debounceTimer: ReturnType<typeof setTimeout> | null = null;
    const debouncedSearch = (query: string): void => {
      if (debounceTimer !== null) {
        clearTimeout(debounceTimer);
      }
      debounceTimer = setTimeout(() => {
        performSearch(query);
      }, 150);
    };

    searchInput.addEventListener('input', (event: Event) => {
      const target = event.target;
      if (target instanceof HTMLInputElement) {
        debouncedSearch(target.value);
      }
    });

    searchClear?.addEventListener('click', () => {
      runtime.search?.clear();
      searchInput.focus();
    });

    searchInput.addEventListener('keydown', (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        runtime.search?.clear();
        searchInput.blur();
      }
    });
  }
}
