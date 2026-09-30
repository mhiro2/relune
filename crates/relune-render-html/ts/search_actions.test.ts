import { describe, expect, it } from 'vitest';

import type { TableMetadata } from './metadata';
import { computeSearchMatches, matchesTableQuery } from './search_actions';
import { column, table } from './test_fixtures';

// Node text mirrors relune-render-svg: tooltips, kind captions, and badges
// sit next to the names and must not be searchable.
function nodes(): NodeListOf<Element> {
  document.body.innerHTML = `<svg>
    <g class="node" data-id="public.users"><title>users table
2 columns
1 primary key</title><text>users</text><text>TABLE</text><text>id: bigint</text><text>PK</text></g>
    <g class="node" data-table-id="public.posts"><text>posts</text><text>title: text</text></g>
    <g class="node"><text>orphan</text></g>
  </svg>`;
  return document.querySelectorAll('.node');
}

const tablesById = new Map<string, TableMetadata>([
  [
    'public.users',
    table('public.users', {
      label: 'Accounts',
      table_name: 'users',
      schema_name: 'public',
      columns: [column('id', 'bigint', { is_primary_key: true })],
    }),
  ],
  [
    'public.posts',
    table('public.posts', {
      table_name: 'posts',
      schema_name: 'public',
      columns: [column('title', 'text')],
    }),
  ],
]);

function matchedIds(query: string): (string | null)[] {
  return computeSearchMatches(nodes(), tablesById, query)
    .results.filter((r) => r.matches)
    .map((r) => r.node.getAttribute('data-id') ?? r.node.getAttribute('data-table-id'));
}

describe('computeSearchMatches', () => {
  it('matches every node for a blank query', () => {
    const { results, matchCount, total } = computeSearchMatches(nodes(), tablesById, '   ');
    expect(results.every((r) => r.matches)).toBe(true);
    expect(matchCount).toBe(3);
    expect(total).toBe(3);
  });

  it('matches names, ids, and columns from metadata case-insensitively', () => {
    expect(matchedIds('ACCOUNTS')).toEqual(['public.users']);
    expect(matchedIds('public.')).toEqual(['public.users', 'public.posts']);
    expect(matchedIds(' Title ')).toEqual(['public.posts']);
    expect(matchedIds('BIGINT')).toEqual(['public.users']);
  });

  it('ignores tooltip, kind, and badge text in the SVG', () => {
    for (const query of ['primary key', 'columns', 'pk', 'table']) {
      expect(matchedIds(query)).toEqual([]);
    }
  });

  it('falls back to the node id when a node has no metadata', () => {
    document.body.innerHTML = `<svg><g class="node" data-id="audit_log"><text>x</text></g></svg>`;
    const { matchCount } = computeSearchMatches(
      document.querySelectorAll('.node'),
      new Map(),
      'audit',
    );
    expect(matchCount).toBe(1);
  });
});

describe('matchesTableQuery', () => {
  const users = table('public.users', {
    label: 'Users',
    table_name: 'users',
    schema_name: 'auth',
    columns: [column('email', 'citext')],
  });

  it('matches ids, labels, schemas, table names, columns and column types', () => {
    for (const query of ['public.', 'USERS', 'auth', 'email', 'citext', '  Email  ']) {
      expect(matchesTableQuery(users, query)).toBe(true);
    }
    expect(matchesTableQuery(users, 'orders')).toBe(false);
  });

  it('treats a blank query as matching everything', () => {
    expect(matchesTableQuery(users, '   ')).toBe(true);
  });
});
