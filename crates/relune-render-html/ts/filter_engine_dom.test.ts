import { describe, expect, it, vi } from 'vitest';

import { buildFacetSection, rebuildFacetCheckboxes } from './filter_engine_dom';
import type { FacetDefinition } from './filter_engine_state';

function schemaFacet(selected: string[]): FacetDefinition {
  return {
    id: 'schema',
    label: 'Schema',
    allValues: ['public', 'auth', 'billing'],
    selectedValues: new Set(selected),
    counts: new Map([
      ['public', 3],
      ['auth', 1],
      ['billing', 2],
    ]),
    extractValues: () => [],
  };
}

function setup(selected: string[]) {
  const facet = schemaFacet(selected);
  const onBulkChange = vi.fn<(values: string[], checked: boolean) => void>();
  const details = buildFacetSection(facet, onBulkChange);
  rebuildFacetCheckboxes(details, facet.allValues, facet.selectedValues, facet.counts, () => {});
  document.body.replaceChildren(details);
  const [selectAll, clear] = details.querySelectorAll<HTMLButtonElement>('.filter-facet-action');
  const checkboxes = [...details.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')];
  return { onBulkChange, selectAll: selectAll!, clear: clear!, checkboxes };
}

describe('buildFacetSection bulk actions', () => {
  it('reports every newly checked value in a single call on Select All', () => {
    const { onBulkChange, selectAll, checkboxes } = setup(['auth']);

    selectAll.click();

    expect(onBulkChange).toHaveBeenCalledTimes(1);
    expect(onBulkChange).toHaveBeenCalledWith(['public', 'billing'], true);
    expect(checkboxes.every((cb) => cb.checked)).toBe(true);
  });

  it('reports every unchecked value in a single call on Clear', () => {
    const { onBulkChange, clear, checkboxes } = setup(['public', 'billing']);

    clear.click();

    expect(onBulkChange).toHaveBeenCalledTimes(1);
    expect(onBulkChange).toHaveBeenCalledWith(['public', 'billing'], false);
    expect(checkboxes.some((cb) => cb.checked)).toBe(false);
  });

  it('skips the callback when nothing changes', () => {
    const { onBulkChange, clear } = setup([]);

    clear.click();

    expect(onBulkChange).not.toHaveBeenCalled();
  });
});
