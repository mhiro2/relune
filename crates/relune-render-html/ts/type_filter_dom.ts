export function rebuildFilterList(
  visibleTypes: string[],
  selectedTypes: Set<string>,
  typeTableCounts: Map<string, number>,
  container: HTMLElement,
  onChange: (dataType: string, checked: boolean) => void,
): void {
  container.innerHTML = '';

  for (const dataType of visibleTypes) {
    const row = document.createElement('label');
    row.className = 'type-filter-item';

    const checkbox = document.createElement('input');
    checkbox.type = 'checkbox';
    checkbox.value = dataType;
    checkbox.checked = selectedTypes.has(dataType);
    checkbox.addEventListener('change', () => {
      onChange(dataType, checkbox.checked);
    });

    const label = document.createElement('span');
    label.textContent = dataType;

    const count = document.createElement('span');
    count.className = 'type-filter-item-count';
    count.textContent = String(typeTableCounts.get(dataType) ?? 0);

    row.appendChild(checkbox);
    row.appendChild(label);
    row.appendChild(count);
    container.appendChild(row);
  }
}

export function syncFilterChrome(
  hasActiveFilter: boolean,
  hasExplicitSelection: boolean,
  selected: string[],
  activeTypeList: string[],
  query: string,
  summaryEl: HTMLElement | null,
  resetBar: HTMLElement | null,
  resetCopy: HTMLElement | null,
): void {
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
}
