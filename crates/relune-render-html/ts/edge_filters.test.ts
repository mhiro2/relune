import { describe, expect, it } from 'vitest';

import { syncEdgeDimming } from './edge_filters';

function setup(): Element {
  document.body.innerHTML = `<svg>
    <g class="node" data-id="users"></g>
    <g class="node" data-table-id="posts"></g>
    <g class="node" data-id="tags"></g>
    <g class="edge" id="posts-users" data-from="posts" data-to="users"></g>
    <g class="edge" id="tags-posts" data-from="tags" data-to="posts"></g>
    <g class="edge" id="dangling" data-from="ghost" data-to="users"></g>
  </svg>`;
  const svg = document.querySelector('svg');
  if (svg === null) throw new Error('missing svg');
  return svg;
}

function edgeClasses(svg: Element, id: string): string[] {
  return [...(svg.querySelector(`#${id}`)?.classList ?? [])].filter((c) => c !== 'edge');
}

function node(svg: Element, id: string): Element {
  const el = svg.querySelector(`[data-id="${id}"], [data-table-id="${id}"]`);
  if (el === null) throw new Error(`missing node ${id}`);
  return el;
}

describe('syncEdgeDimming', () => {
  it('dims edges touching a search- or filter-dimmed node', () => {
    const svg = setup();
    node(svg, 'users').classList.add('dimmed-by-search');
    syncEdgeDimming(svg);
    expect(edgeClasses(svg, 'posts-users')).toEqual(['dimmed-by-edge-filter']);
    expect(edgeClasses(svg, 'tags-posts')).toEqual([]);

    node(svg, 'users').classList.remove('dimmed-by-search');
    node(svg, 'tags').classList.add('dimmed-by-filter');
    syncEdgeDimming(svg);
    expect(edgeClasses(svg, 'posts-users')).toEqual([]);
    expect(edgeClasses(svg, 'tags-posts')).toEqual(['dimmed-by-edge-filter']);
  });

  it('hides rather than dims edges whose endpoint is hidden', () => {
    const svg = setup();
    node(svg, 'posts').classList.add('hidden-by-filter');
    node(svg, 'users').classList.add('dimmed-by-search');
    syncEdgeDimming(svg);
    expect(edgeClasses(svg, 'posts-users')).toEqual(['hidden-by-filter']);
    expect(edgeClasses(svg, 'tags-posts')).toEqual(['hidden-by-filter']);
  });

  it('leaves edges to unknown nodes untouched by the missing endpoint', () => {
    const svg = setup();
    syncEdgeDimming(svg);
    expect(edgeClasses(svg, 'dangling')).toEqual([]);
  });
});
