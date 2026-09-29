import { describe, expect, it, vi } from 'vitest';

import { computeHoverPreview, computeNeighborHighlights } from './highlight_actions';
import { createHighlightPainter, createObjectBrowser } from './highlight_dom';
import { createHighlightState } from './highlight_state';
import { edge, table } from './test_fixtures';

// users <- posts <- comments, plus an unrelated tags table
const tables = ['users', 'posts', 'comments', 'tags'].map((id) => table(id));
const edges = [edge('posts', 'users'), edge('comments', 'posts')];
const state = createHighlightState(tables, edges);

function renderDiagram() {
  document.body.innerHTML = `<svg>
    ${tables.map((t) => `<g class="node" data-id="${t.id}"></g>`).join('')}
    ${edges.map((e) => `<g class="edge" data-from="${e.from}" data-to="${e.to}"></g>`).join('')}
  </svg>`;
  const nodesById = new Map(
    [...document.querySelectorAll('.node')].map((n) => [n.getAttribute('data-id') ?? '', n]),
  );
  const edgeEls = [...document.querySelectorAll('.edge')];
  return { nodesById, edgeEls, painter: createHighlightPainter({ nodesById, edges: edgeEls }) };
}

const classesOf = (element: Element | undefined): string[] =>
  [...(element?.classList ?? [])].filter((c) => c !== 'node' && c !== 'edge');

describe('createHighlightPainter', () => {
  it('marks only the hovered neighborhood and clears it again', () => {
    const { nodesById, edgeEls, painter } = renderDiagram();
    const tags = nodesById.get('tags');
    const spy = vi.spyOn(tags?.classList ?? document.body.classList, 'remove');

    painter.applyHoverPreview(computeHoverPreview('posts', state));
    expect(classesOf(nodesById.get('posts'))).toEqual(['hover-preview-node']);
    expect(classesOf(nodesById.get('users'))).toEqual(['hover-preview-neighbor', 'hover-outbound']);
    expect(classesOf(nodesById.get('comments'))).toEqual([
      'hover-preview-neighbor',
      'hover-inbound',
    ]);
    expect(classesOf(tags)).toEqual([]);
    expect(edgeEls.map(classesOf)).toEqual([['hover-preview-edge'], ['hover-preview-edge']]);

    painter.clear();
    for (const element of [...nodesById.values(), ...edgeEls]) {
      expect(classesOf(element)).toEqual([]);
    }
    // The untouched table is never visited when the preview is cleared.
    expect(spy).not.toHaveBeenCalled();
  });

  it('dims everything outside the selected neighborhood', () => {
    const { nodesById, edgeEls, painter } = renderDiagram();
    painter.applySelected(computeNeighborHighlights('users', state));

    expect(classesOf(nodesById.get('users'))).toEqual(['selected-node']);
    expect(classesOf(nodesById.get('posts'))).toEqual(['highlighted-neighbor', 'inbound']);
    expect(classesOf(nodesById.get('comments'))).toEqual(['dimmed-by-highlight']);
    expect(classesOf(nodesById.get('tags'))).toEqual(['dimmed-by-highlight']);
    expect(edgeEls.map(classesOf)).toEqual([['highlighted-neighbor'], ['dimmed-by-highlight']]);

    painter.clear();
    expect([...nodesById.values(), ...edgeEls].flatMap(classesOf)).toEqual([]);
  });
});

describe('createObjectBrowser', () => {
  it('reuses each table button across renders', () => {
    document.body.innerHTML = '<div id="list"></div><span id="count"></span><p id="empty"></p>';
    const list = document.getElementById('list') as HTMLElement;
    const count = document.getElementById('count') as HTMLElement;
    const empty = document.getElementById('empty') as HTMLElement;
    const onSelect = vi.fn();
    const browser = createObjectBrowser(list, count, empty, onSelect);
    const item = (t: (typeof tables)[number], isSelected = false) => ({
      table: t,
      isSelected,
      isDimmedBySearch: false,
      isExcludedByFilter: false,
      isHiddenByGroup: false,
    });

    browser.render(
      tables.map((t) => item(t)),
      tables.length,
    );
    const first = [...list.children];
    expect(first).toHaveLength(4);
    expect(count.textContent).toBe('4/4');

    browser.render([item(tables[1] as (typeof tables)[number], true)], tables.length);
    expect([...list.children]).toEqual([first[1]]);
    expect(first[1]?.classList.contains('selected')).toBe(true);
    expect(count.textContent).toBe('1/4');

    browser.render([], tables.length);
    expect(list.children).toHaveLength(0);
    expect(empty.hasAttribute('hidden')).toBe(false);

    (first[1] as HTMLButtonElement).click();
    expect(onSelect).toHaveBeenCalledWith('posts');
  });
});
