import { describe, expect, it } from 'vitest';

import { computeRelationHighlight, relationColumnPairs } from './highlight_actions';
import { createHighlightPainter, pathEndpoints, renderRelationCard } from './highlight_dom';
import { createHighlightState } from './highlight_state';
import { edge, table } from './test_fixtures';

// stock_movements references warehouse_bins through a three-column composite key.
const compositeKey = ['region_code', 'warehouse_number', 'bin_label'];
const tables = [table('stock_movements'), table('warehouse_bins'), table('tags')];
const edges = [
  edge('stock_movements', 'warehouse_bins', {
    name: 'fk_stock_bin',
    from_columns: compositeKey,
    to_columns: compositeKey,
  }),
  edge('tags', 'warehouse_bins'),
];
const state = createHighlightState(tables, edges);

function tableNode(id: string, y: number, columns: string[]): string {
  const rows = columns
    .map(
      (name, index) =>
        `<g class="column-row" data-column-name="${name}"><text class="column-name" y="${y + 53 + index * 22}">${name}</text></g>`,
    )
    .join('');
  return `<g class="node" data-id="${id}"><rect class="table-body" x="10" y="${y}" width="200" height="120"/>${rows}</g>`;
}

function renderDiagram() {
  document.body.innerHTML = `<svg>
    ${tableNode('stock_movements', 0, ['id', ...compositeKey, 'quantity_delta'])}
    ${tableNode('warehouse_bins', 300, [...compositeKey, 'capacity_units'])}
    ${tableNode('tags', 600, ['id'])}
    <g class="edge"><path class="edge-path" d="M 210.0 70.5 L 260.0 70.5 L 260.0 360.0 L 210.0 360.0"/></g>
    <g class="edge"><path class="edge-path" d="M 10 600 L 10 400"/></g>
  </svg>`;
  const nodesById = new Map(
    [...document.querySelectorAll('.node')].map((n) => [n.getAttribute('data-id') ?? '', n]),
  );
  const edgeEls = [...document.querySelectorAll('.edge')];
  return { nodesById, edgeEls, painter: createHighlightPainter({ nodesById, edges: edgeEls }) };
}

const highlightedColumns = (node: Element | undefined): string[] =>
  [...(node?.querySelectorAll('.column-row.relation-column') ?? [])].map(
    (row) => row.getAttribute('data-column-name') ?? '',
  );

describe('relationColumnPairs', () => {
  it('lists one mapping per column pair of a composite key', () => {
    expect(relationColumnPairs(edges[0] as (typeof edges)[number])).toEqual([
      'stock_movements.region_code → warehouse_bins.region_code',
      'stock_movements.warehouse_number → warehouse_bins.warehouse_number',
      'stock_movements.bin_label → warehouse_bins.bin_label',
    ]);
  });

  it('keeps the source column of an enum reference', () => {
    const enumRef = edge('orders', 'order_status', {
      kind: 'enum_reference',
      from_columns: ['status'],
    });
    expect(relationColumnPairs(enumRef)).toEqual(['orders.status → order_status']);
  });

  it('falls back to the table pair when columns are unknown', () => {
    expect(relationColumnPairs(edges[1] as (typeof edges)[number])).toEqual([
      'tags → warehouse_bins',
    ]);
  });
});

describe('pathEndpoints', () => {
  it('reads the first and last points of a path', () => {
    expect(pathEndpoints('M 1.5 2 L 3 4 Q 5 6 -7.25 8')).toEqual([
      [1.5, 2],
      [-7.25, 8],
    ]);
    expect(pathEndpoints('')).toBeNull();
  });
});

describe('applyRelation', () => {
  it('emphasizes every column of a composite key on both ends', () => {
    const { nodesById, edgeEls, painter } = renderDiagram();
    const relation = computeRelationHighlight(0, state);
    expect(relation).not.toBeNull();
    if (relation === null) return;

    painter.applyRelation(relation);

    expect(highlightedColumns(nodesById.get('stock_movements'))).toEqual(compositeKey);
    expect(highlightedColumns(nodesById.get('warehouse_bins'))).toEqual(compositeKey);
    expect(nodesById.get('stock_movements')?.classList.contains('relation-endpoint')).toBe(true);
    expect(nodesById.get('tags')?.classList.contains('dimmed-by-highlight')).toBe(true);
    expect(edgeEls[0]?.classList.contains('selected-edge')).toBe(true);
    expect(edgeEls[1]?.classList.contains('dimmed-by-highlight')).toBe(true);

    // Bands sit behind each matched row; ports mark both ends of the path.
    const band = nodesById.get('stock_movements')?.querySelector('.relation-column-band');
    expect(band?.getAttribute('y')).toBe(String(22 + 38));
    expect(band?.getAttribute('height')).toBe('22');
    const ports = [...(edgeEls[0]?.querySelectorAll('.relation-port') ?? [])];
    expect(ports.map((port) => [port.getAttribute('cx'), port.getAttribute('cy')])).toEqual([
      ['210', '70.5'],
      ['210', '360'],
    ]);

    painter.clear();
    expect(document.querySelectorAll('.relation-column-band, .relation-port')).toHaveLength(0);
    expect(document.querySelectorAll('.relation-column, .selected-edge')).toHaveLength(0);
  });
});

describe('renderRelationCard', () => {
  it('shows the mapping for the selected relationship and hides when cleared', () => {
    document.body.innerHTML = `<aside id="card" hidden>
      <p id="kind"></p><h2 id="title"></h2><p id="name" hidden></p><ul id="pairs"></ul>
      <button id="from"></button><button id="to"></button></aside>`;
    const byId = (id: string) => document.getElementById(id) as HTMLElement;
    const elements = {
      card: byId('card'),
      kind: byId('kind'),
      title: byId('title'),
      name: byId('name'),
      pairs: byId('pairs'),
      openFrom: byId('from') as HTMLButtonElement,
      openTo: byId('to') as HTMLButtonElement,
    };

    renderRelationCard(edges[0], state.tableById, elements);
    expect(elements.card.hasAttribute('hidden')).toBe(false);
    expect(elements.kind.textContent).toBe('Foreign key');
    expect(elements.title.textContent).toBe('stock_movements → warehouse_bins');
    expect(elements.name.textContent).toBe('fk_stock_bin');
    expect(elements.pairs.children).toHaveLength(3);
    expect(elements.openTo.textContent).toBe('Open warehouse_bins');

    renderRelationCard(undefined, state.tableById, elements);
    expect(elements.card.hasAttribute('hidden')).toBe(true);
    expect(elements.pairs.children).toHaveLength(0);
  });
});
