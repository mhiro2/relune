import type { FacetDefinition, FacetId, FacetSummaryItem, FilterMode } from './filter_engine_state';
import { visibleTypesForQuery } from './filter_engine_state';

function buildFacetControlId(facetId: string, value: string, index: number): string {
  const normalized = value
    .trim()
    .toLowerCase()
    .replaceAll(/[^a-z0-9]+/g, '-')
    .replaceAll(/^-+|-+$/g, '');
  const suffix = normalized === '' ? 'value' : normalized;
  return `filter-${facetId}-${suffix}-${index}`;
}

// ── Mode switcher ──────────────────────────────────────────────────────

export function buildFilterModeSwitcher(
  currentMode: FilterMode,
  onChange: (mode: FilterMode) => void,
): HTMLElement {
  const wrapper = document.createElement('div');
  wrapper.className = 'filter-mode-switcher';

  const modes: { id: FilterMode; label: string; title: string }[] = [
    { id: 'dim', label: 'Dim', title: 'Reduce opacity of non-matching objects' },
    { id: 'hide', label: 'Hide', title: 'Hide non-matching objects' },
    { id: 'focus', label: 'Focus', title: 'Hide non-matching objects and zoom to fit' },
  ];

  for (const { id, label, title } of modes) {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'filter-mode-button';
    btn.classList.toggle('active', id === currentMode);
    btn.textContent = label;
    btn.title = title;
    btn.dataset.mode = id;
    btn.addEventListener('click', () => {
      onChange(id);
    });
    wrapper.appendChild(btn);
  }

  return wrapper;
}

export function syncModeSwitcher(container: HTMLElement, activeMode: FilterMode): void {
  for (const btn of container.querySelectorAll('.filter-mode-button')) {
    const mode = (btn as HTMLElement).dataset.mode;
    btn.classList.toggle('active', mode === activeMode);
  }
}

// ── Facet section ──────────────────────────────────────────────────────

export function buildFacetSection(
  facet: FacetDefinition,
  onChange: (value: string, checked: boolean) => void,
  onSearchInput?: (query: string) => void,
): HTMLDetailsElement {
  const details = document.createElement('details');
  details.className = 'filter-facet';
  details.dataset.facetId = facet.id;

  const summary = document.createElement('summary');
  summary.className = 'filter-facet-summary';

  const label = document.createElement('span');
  label.className = 'filter-facet-label';
  label.textContent = facet.label;

  const badge = document.createElement('span');
  badge.className = 'filter-facet-badge';
  badge.hidden = true;

  const actions = document.createElement('span');
  actions.className = 'filter-facet-actions';

  const allBtn = document.createElement('button');
  allBtn.type = 'button';
  allBtn.className = 'filter-facet-action';
  allBtn.textContent = 'Select All';
  allBtn.addEventListener('click', () => {
    const listEl = details.querySelector('.filter-facet-list');
    if (!listEl) return;
    for (const cb of listEl.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')) {
      if (!cb.checked) {
        cb.checked = true;
        onChange(cb.value, true);
      }
    }
  });

  const noneBtn = document.createElement('button');
  noneBtn.type = 'button';
  noneBtn.className = 'filter-facet-action';
  noneBtn.textContent = 'Clear';
  noneBtn.addEventListener('click', () => {
    const listEl = details.querySelector('.filter-facet-list');
    if (!listEl) return;
    for (const cb of listEl.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')) {
      if (cb.checked) {
        cb.checked = false;
        onChange(cb.value, false);
      }
    }
  });

  actions.append(allBtn, noneBtn);
  summary.append(label, badge);
  details.append(summary, actions);

  if (facet.hasSearch === true) {
    const searchInput = document.createElement('input');
    searchInput.type = 'search';
    searchInput.id = `filter-facet-search-${facet.id}`;
    searchInput.name = `filter-facet-search-${facet.id}`;
    searchInput.className = 'filter-facet-search';
    searchInput.placeholder = 'Narrow type list...';
    searchInput.setAttribute('aria-label', `${facet.label} filter search`);
    searchInput.autocomplete = 'off';
    searchInput.addEventListener('input', () => {
      onSearchInput?.(searchInput.value);
    });
    details.appendChild(searchInput);
  }

  const list = document.createElement('div');
  list.className = 'filter-facet-list';
  details.appendChild(list);

  return details;
}

export function rebuildFacetCheckboxes(
  details: HTMLDetailsElement,
  values: string[],
  selectedValues: Set<string>,
  counts: Map<string, number>,
  onChange: (value: string, checked: boolean) => void,
): void {
  const list = details.querySelector('.filter-facet-list');
  if (!list) return;

  list.replaceChildren();
  const facetId = details.dataset.facetId ?? 'facet';

  for (const [index, value] of values.entries()) {
    const row = document.createElement('label');
    row.className = 'filter-facet-item';

    const checkbox = document.createElement('input');
    checkbox.type = 'checkbox';
    checkbox.id = buildFacetControlId(facetId, value, index);
    checkbox.name = `filter-${facetId}`;
    checkbox.value = value;
    checkbox.checked = selectedValues.has(value);
    checkbox.addEventListener('change', () => {
      onChange(value, checkbox.checked);
    });

    const text = document.createElement('span');
    text.textContent = value;

    const count = document.createElement('span');
    count.className = 'filter-facet-item-count';
    count.textContent = String(counts.get(value) ?? 0);

    row.append(checkbox, text, count);
    list.appendChild(row);
  }
}

export function syncFacetBadge(details: HTMLDetailsElement, selectedCount: number): void {
  const badge = details.querySelector('.filter-facet-badge');
  if (badge instanceof HTMLElement) {
    badge.hidden = selectedCount === 0;
    badge.textContent = String(selectedCount);
  }
}

// ── Active filter summary ──────────────────────────────────────────────

export function renderActiveFilterSummary(
  container: HTMLElement,
  items: FacetSummaryItem[],
  onClickFacet: (facetId: FacetId) => void,
): void {
  container.replaceChildren();

  if (items.length === 0) {
    container.hidden = true;
    return;
  }

  container.hidden = false;

  for (const item of items) {
    const chip = document.createElement('button');
    chip.type = 'button';
    chip.className = 'filter-summary-chip';

    const preview = item.values.slice(0, 2).join(', ');
    const suffix = item.count > 2 ? ` +${item.count - 2}` : '';
    chip.textContent = `${item.label}: ${preview}${suffix}`;
    chip.title = item.values.join(', ');

    chip.addEventListener('click', () => {
      onClickFacet(item.facetId);
    });

    container.appendChild(chip);
  }
}

// ── Filter reset bar ───────────────────────────────────────────────────

export function syncFilterResetBar(
  active: boolean,
  items: FacetSummaryItem[],
  mode: FilterMode,
  resetBar: HTMLElement | null,
  resetCopy: HTMLElement | null,
): void {
  if (!resetBar || !resetCopy) return;

  resetBar.toggleAttribute('hidden', !active);

  if (!active) {
    resetCopy.textContent = '';
    return;
  }

  const parts = items.map((item) => {
    const preview = item.values.slice(0, 2).join(', ');
    const suffix = item.count > 2 ? ` +${item.count - 2}` : '';
    return `${item.label}: ${preview}${suffix}`;
  });

  const modeLabel = mode !== 'dim' ? ` [${mode}]` : '';
  resetCopy.textContent = parts.join(' / ') + modeLabel;
}

// ── Column type facet helpers ──────────────────────────────────────────

export function rebuildColumnTypeFacet(
  details: HTMLDetailsElement,
  facet: FacetDefinition,
  query: string,
  onChange: (value: string, checked: boolean) => void,
): void {
  const visible = visibleTypesForQuery(facet.allValues, query);
  rebuildFacetCheckboxes(details, visible, facet.selectedValues, facet.counts, onChange);
}
