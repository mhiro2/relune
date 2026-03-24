import { syncEdgeDimming } from './edge_filters';
import { parseReluneMetadata, type TableMetadata } from './metadata';

function columnMatchesSelectedType(columnType: string, selectedType: string): boolean {
  const c = columnType.trim().toLowerCase();
  const s = selectedType.trim().toLowerCase();
  if (c === s) {
    return true;
  }
  const base = (raw: string): string => {
    const i = raw.indexOf('(');
    return (i === -1 ? raw : raw.slice(0, i)).trim();
  };
  const baseC = base(c);
  const baseS = base(s);
  if (baseC === baseS) {
    return true;
  }
  return c.includes(s) || s.includes(c);
}

function tableMatchesAnySelectedType(table: TableMetadata, selectedTypes: string[]): boolean {
  const columns = table.columns ?? [];
  for (const col of columns) {
    const dt = col.data_type ?? '';
    for (const sel of selectedTypes) {
      if (columnMatchesSelectedType(dt, sel)) {
        return true;
      }
    }
  }
  return false;
}

{
  const section = document.getElementById('type-filter-section');
  const listEl = document.getElementById('type-filter-list');
  const svgEl = document.querySelector('.canvas svg');

  if (section === null || listEl === null || svgEl === null) {
    // Type filter markup or SVG not present.
  } else {
    const listRoot = listEl;
    const svgRoot = svgEl;
    const summaryEl = document.getElementById('type-filter-summary');
    const clearBtn = document.getElementById('type-filter-clear');
    const queryInput = document.getElementById('type-filter-query');

    const metadata = parseReluneMetadata();
    const tables: TableMetadata[] = metadata?.tables ?? [];

    const typeSet = new Set<string>();
    for (const table of tables) {
      for (const col of table.columns ?? []) {
        const dt = (col.data_type ?? '').trim();
        if (dt !== '') {
          typeSet.add(dt);
        }
      }
    }

    const allTypes = Array.from(typeSet).sort((a, b) =>
      a.localeCompare(b, undefined, { sensitivity: 'base' }),
    );

    if (allTypes.length === 0) {
      section.setAttribute('hidden', '');
    } else {
      section.removeAttribute('hidden');
    }

    const checkboxes: HTMLInputElement[] = [];

    function visibleTypesForQuery(q: string): string[] {
      const needle = q.trim().toLowerCase();
      if (needle === '') {
        return allTypes;
      }
      return allTypes.filter((t) => t.toLowerCase().includes(needle));
    }

    function rebuildList(): void {
      listRoot.innerHTML = '';
      checkboxes.length = 0;
      const q = queryInput instanceof HTMLInputElement ? queryInput.value : '';
      const visible = visibleTypesForQuery(q);

      for (const dtype of visible) {
        const row = document.createElement('label');
        row.className = 'type-filter-item';

        const cb = document.createElement('input');
        cb.type = 'checkbox';
        cb.value = dtype;
        cb.addEventListener('change', applyTypeFilter);

        const span = document.createElement('span');
        span.textContent = dtype;

        row.appendChild(cb);
        row.appendChild(span);
        listRoot.appendChild(row);
        checkboxes.push(cb);
      }
    }

    function selectedTypes(): string[] {
      return checkboxes.filter((cb) => cb.checked).map((cb) => cb.value);
    }

    function applyTypeFilter(): void {
      const selected = selectedTypes();
      const nodes = svgRoot.querySelectorAll('.node');

      if (selected.length === 0) {
        nodes.forEach((node) => {
          node.classList.remove('dimmed-by-type-filter');
        });
      } else {
        nodes.forEach((node) => {
          const tableId = node.getAttribute('data-id') ?? node.getAttribute('data-table-id') ?? '';
          const table = tables.find((t) => t.id === tableId);
          const matches = table !== undefined && tableMatchesAnySelectedType(table, selected);

          if (matches) {
            node.classList.remove('dimmed-by-type-filter');
          } else {
            node.classList.add('dimmed-by-type-filter');
          }
        });
      }

      if (summaryEl) {
        if (selected.length === 0) {
          summaryEl.textContent = '';
          summaryEl.classList.remove('visible');
        } else {
          summaryEl.textContent = `${selected.length} type(s) · objects with any matching column`;
          summaryEl.classList.add('visible');
        }
      }

      syncEdgeDimming(svgRoot);
    }

    rebuildList();
    queryInput?.addEventListener('input', () => {
      const had = new Set(selectedTypes());
      rebuildList();
      for (const cb of checkboxes) {
        if (had.has(cb.value)) {
          cb.checked = true;
        }
      }
      applyTypeFilter();
    });

    clearBtn?.addEventListener('click', () => {
      for (const cb of checkboxes) {
        cb.checked = false;
      }
      applyTypeFilter();
    });

    applyTypeFilter();
  }
}
