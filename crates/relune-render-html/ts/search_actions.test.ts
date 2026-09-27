import { describe, expect, it } from 'vitest';

import { computeSearchMatches } from './search_actions';

function nodes(): NodeListOf<Element> {
  document.body.innerHTML = `<svg>
    <g class="node" data-id="public.users"><text>users</text><text>email</text></g>
    <g class="node" data-table-id="public.posts"><text>posts</text><text>title</text></g>
    <g class="node"><text>orphan</text></g>
  </svg>`;
  return document.querySelectorAll('.node');
}

function matchedIds(results: { node: Element; matches: boolean }[]): (string | null)[] {
  return results
    .filter((r) => r.matches)
    .map((r) => r.node.getAttribute('data-id') ?? r.node.getAttribute('data-table-id'));
}

describe('computeSearchMatches', () => {
  it('matches every node for a blank query', () => {
    const { results, matchCount, total } = computeSearchMatches(nodes(), {}, '   ');
    expect(results.every((r) => r.matches)).toBe(true);
    expect(matchCount).toBe(3);
    expect(total).toBe(3);
  });

  it('matches the display name, the id and the node text case-insensitively', () => {
    const names = { 'public.users': 'Accounts' };
    expect(matchedIds(computeSearchMatches(nodes(), names, 'ACCOUNTS').results)).toEqual([
      'public.users',
    ]);
    expect(matchedIds(computeSearchMatches(nodes(), names, 'public.').results)).toEqual([
      'public.users',
      'public.posts',
    ]);
    const byColumn = computeSearchMatches(nodes(), names, ' Title ');
    expect(matchedIds(byColumn.results)).toEqual(['public.posts']);
    expect(byColumn.matchCount).toBe(1);
  });

  it('reports no matches for an unknown query', () => {
    const { matchCount, total } = computeSearchMatches(nodes(), {}, 'invoices');
    expect(matchCount).toBe(0);
    expect(total).toBe(3);
  });
});
