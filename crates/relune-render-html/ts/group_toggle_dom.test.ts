import { describe, expect, it } from 'vitest';

import { applyGroupVisibility } from './group_toggle_dom';

describe('group visibility', () => {
  it('hides a group surface and label with its tables', () => {
    document.body.innerHTML = `
      <svg>
        <g class="group" data-group-id="sales"><rect class="group-box"></rect></g>
        <g class="group" data-group-id="public"><rect class="group-box"></rect></g>
        <g class="node" data-id="sales.orders"></g>
        <g class="node" data-id="public.users"></g>
        <text class="group-label" data-group-id="sales">sales</text>
      </svg>`;
    const svg = document.querySelector('svg')!;

    applyGroupVisibility(svg, 'sales', ['sales.orders'], false);

    const hidden = [...svg.querySelectorAll('.hidden-by-group')].map(
      (element) => element.getAttribute('data-group-id') ?? element.getAttribute('data-id'),
    );
    expect(hidden).toEqual(['sales', 'sales.orders', 'sales']);

    applyGroupVisibility(svg, 'sales', ['sales.orders'], true);
    expect(svg.querySelectorAll('.hidden-by-group')).toHaveLength(0);
  });
});
