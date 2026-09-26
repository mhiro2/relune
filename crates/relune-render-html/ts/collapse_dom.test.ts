import { beforeEach, describe, expect, it, vi } from 'vitest';

import { createCollapseController } from './collapse_dom';

// Mirrors the table node markup emitted by relune-render-svg.
function tableNode(id: string, index: number, x: number, y: number, columns: string[]): string {
  const rows = columns
    .map(
      (name) =>
        `<g class="column-row" data-column-name="${name}"><text class="column-name">${name}</text></g>`,
    )
    .join('');
  return `<g class="table-node node node-kind-table" data-table-id="${id}" data-id="${id}">
    <rect class="table-body" x="${x}" y="${y}" width="200" height="120"/>
    <rect class="table-header" x="${x}" y="${y}" width="200" height="32"/>
    <rect class="table-header-fade" x="${x}" y="${y + 16}" width="200" height="16"/>
    <clipPath id="node-${index}-header-clip"><rect x="${x + 10}" y="${y + 8}" width="146" height="16"/></clipPath>
    <text class="table-name" x="${x + 10}" y="${y + 21}" clip-path="url(#node-${index}-header-clip)">${id}</text>
    <text class="table-kind" x="${x + 190}" y="${y + 21}">TABLE</text>
    ${rows}
    <rect class="type-filter-overlay" x="${x}" y="${y}" width="200" height="120"/>
  </g>`;
}

function setup(initiallyCollapsed: string[] = []) {
  document.body.innerHTML = `<svg>${tableNode('users', 0, 300, 40, ['id', 'email'])}${tableNode(
    'posts',
    1,
    40,
    240,
    ['id'],
  )}</svg>`;
  const svg = document.querySelector('svg');
  if (svg === null) throw new Error('missing svg');
  const onToggle = vi.fn();
  const controller = createCollapseController(svg, {
    columnCounts: new Map([
      ['users', 2],
      ['posts', 1],
    ]),
    initiallyCollapsed,
    onToggle,
  });
  const node = (id: string): Element => {
    const el = svg.querySelector(`[data-table-id="${id}"]`);
    if (el === null) throw new Error(`missing node ${id}`);
    return el;
  };
  return { controller, onToggle, node };
}

function click(el: Element | null): void {
  el?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
}

describe('createCollapseController', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  it('adds exactly one indicator and badge per table', () => {
    const { node } = setup();
    expect(node('users').querySelectorAll('.collapse-indicator')).toHaveLength(1);
    expect(node('users').querySelectorAll('.column-count-badge')).toHaveLength(1);
    expect(node('users').querySelector('.column-count-badge')?.textContent).toBe(
      '2 columns hidden',
    );
    expect(node('posts').querySelector('.column-count-badge')?.textContent).toBe('1 column hidden');
  });

  it('positions the indicator inside the node header in diagram coordinates', () => {
    const { node } = setup();
    const indicator = node('users').querySelector('.collapse-indicator');
    const x = Number(indicator?.getAttribute('x'));
    const y = Number(indicator?.getAttribute('y'));
    expect(x).toBeGreaterThan(300);
    expect(x).toBeLessThan(500);
    expect(y).toBeGreaterThan(40);
    expect(y).toBeLessThan(72);
    expect(Number(node('users').querySelector('.column-count-badge')?.getAttribute('x'))).toBe(310);
  });

  it('narrows the table-name clip to make room for the indicator', () => {
    const { node } = setup();
    const clipWidth = Number(node('users').querySelector('clipPath rect')?.getAttribute('width'));
    expect(clipWidth).toBeLessThan(146);
  });

  it('toggles a table once per header click', () => {
    const { controller, onToggle, node } = setup();
    const users = node('users');

    click(users.querySelector('.table-header'));
    expect(users.classList.contains('collapsed')).toBe(true);
    expect(users.querySelector('.collapse-indicator')?.textContent).toBe('▸');
    expect(controller.getCollapsed()).toEqual(['users']);

    click(users.querySelector('.table-name'));
    expect(users.classList.contains('collapsed')).toBe(false);
    expect(users.querySelector('.collapse-indicator')?.textContent).toBe('▾');
    expect(controller.getCollapsed()).toEqual([]);
    expect(onToggle).toHaveBeenCalledTimes(2);
  });

  it('keeps header clicks from reaching node-level listeners', () => {
    const { node } = setup();
    const users = node('users');
    const nodeClick = vi.fn();
    users.addEventListener('click', nodeClick);

    click(users.querySelector('.collapse-indicator'));
    expect(nodeClick).not.toHaveBeenCalled();

    click(users.querySelector('.column-row'));
    expect(nodeClick).toHaveBeenCalledTimes(1);
    expect(users.classList.contains('collapsed')).toBe(true);
  });

  it('restores the initial state and ignores unknown tables', () => {
    const { controller, node } = setup(['posts', 'missing']);
    expect(node('posts').classList.contains('collapsed')).toBe(true);
    expect(node('users').classList.contains('collapsed')).toBe(false);
    expect(controller.getCollapsed()).toEqual(['posts']);
  });

  it('replaces the collapsed set through the runtime API without notifying', () => {
    const { controller, onToggle, node } = setup(['posts']);
    controller.setCollapsed(['users']);
    expect(node('users').classList.contains('collapsed')).toBe(true);
    expect(node('posts').classList.contains('collapsed')).toBe(false);
    expect(controller.getCollapsed()).toEqual(['users']);
    expect(onToggle).not.toHaveBeenCalled();
  });
});
