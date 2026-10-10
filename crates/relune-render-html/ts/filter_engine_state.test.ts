import { describe, expect, it } from 'vitest';

import {
  activeFilterSummary,
  columnMatchesSelectedType,
  createFilterEngineState,
  hasActiveFilters,
  tableMatchesAllFacets,
  visibleTypesForQuery,
} from './filter_engine_state';
import { column, table } from './test_fixtures';

const tables = [
  table('public.users', {
    schema_name: 'public',
    columns: [column('id', 'bigint'), column('email', 'varchar(255)')],
  }),
  table('public.posts', {
    schema_name: 'public',
    columns: [column('id', 'bigint'), column('body', 'text'), column('score', ' ')],
    issues: [{ severity: 'warning', message: 'missing index' }],
  }),
  table('audit.events', {
    schema_name: 'audit',
    kind: 'view',
    columns: [column('payload', 'jsonb')],
    issues: [
      { severity: 'breaking', message: 'broken' },
      { severity: 'breaking', message: 'still broken' },
    ],
  }),
];

describe('createFilterEngineState', () => {
  it('builds sorted facet values with per-table counts', () => {
    const state = createFilterEngineState(tables);
    expect([...state.facets.keys()]).toEqual(['schema', 'kind', 'columnType', 'severity']);
    expect(state.mode).toBe('dim');

    const schema = state.facets.get('schema');
    expect(schema?.allValues).toEqual(['audit', 'public']);
    expect(schema?.counts.get('public')).toBe(2);

    const types = state.facets.get('columnType');
    // Blank types are skipped and each table counts a type once.
    expect(types?.allValues).toEqual(['bigint', 'jsonb', 'text', 'varchar(255)']);
    expect(types?.counts.get('bigint')).toBe(2);
    expect(types?.hasSearch).toBe(true);

    const severity = state.facets.get('severity');
    expect(severity?.allValues).toEqual(['breaking', 'none', 'warning']);
    expect(severity?.counts.get('breaking')).toBe(1);
  });

  it('omits facets that cannot narrow the diagram', () => {
    const state = createFilterEngineState([
      table('users', { columns: [column('id', 'int')] }),
      table('posts', { columns: [column('id', 'int')] }),
    ]);
    // One schema, one kind and no issues leave only the column type facet.
    expect([...state.facets.keys()]).toEqual(['columnType']);
    expect(state.facets.get('schema')?.allValues).toBeUndefined();
  });

  it('groups tables without a schema under a default label', () => {
    const state = createFilterEngineState([table('a'), table('b', { schema_name: 'sales' })]);
    expect(state.facets.get('schema')?.allValues).toEqual(['(default)', 'sales']);
  });

  it('adds the diff facet only when diff data is present', () => {
    expect(createFilterEngineState(tables).facets.has('diffKind')).toBe(false);

    const state = createFilterEngineState([table('a', { diff_kind: 'added' }), table('b')]);
    expect(state.facets.get('diffKind')?.allValues).toEqual(['added', 'unchanged']);
  });
});

describe('columnMatchesSelectedType', () => {
  it('matches case-insensitively and ignores parameters', () => {
    expect(columnMatchesSelectedType('VARCHAR(255)', 'varchar')).toBe(true);
    expect(columnMatchesSelectedType('varchar', 'varchar(64)')).toBe(true);
    expect(columnMatchesSelectedType(' numeric(10,2) ', 'NUMERIC(12,4)')).toBe(true);
  });

  it('treats modifiers and array suffixes as the same base type', () => {
    expect(columnMatchesSelectedType('timestamp with time zone', 'timestamp')).toBe(true);
    expect(columnMatchesSelectedType('text[]', 'text')).toBe(true);
  });

  it('does not match types that merely share a prefix', () => {
    expect(columnMatchesSelectedType('integer', 'int')).toBe(false);
    expect(columnMatchesSelectedType('bigint', 'int')).toBe(false);
    expect(columnMatchesSelectedType('textual', 'text')).toBe(false);
  });
});

describe('facet matching', () => {
  it('passes every table while nothing is selected', () => {
    const state = createFilterEngineState(tables);
    expect(hasActiveFilters(state)).toBe(false);
    expect(tables.every((t) => tableMatchesAllFacets(t, state))).toBe(true);
  });

  it('ORs values within a facet and ANDs across facets', () => {
    const state = createFilterEngineState(tables);
    state.facets.get('schema')?.selectedValues.add('public');
    state.facets.get('columnType')?.selectedValues.add('text').add('varchar');

    expect(hasActiveFilters(state)).toBe(true);
    expect(tables.filter((t) => tableMatchesAllFacets(t, state)).map((t) => t.id)).toEqual([
      'public.users',
      'public.posts',
    ]);

    state.facets.get('severity')?.selectedValues.add('none');
    expect(tables.filter((t) => tableMatchesAllFacets(t, state)).map((t) => t.id)).toEqual([
      'public.users',
    ]);
  });

  it('summarises active facets with sorted values', () => {
    const state = createFilterEngineState(tables);
    state.facets.get('severity')?.selectedValues.add('warning').add('breaking');
    expect(activeFilterSummary(state)).toEqual([
      { facetId: 'severity', label: 'Risk', count: 2, values: ['breaking', 'warning'] },
    ]);
  });
});

describe('visibleTypesForQuery', () => {
  it('filters by a trimmed, case-insensitive substring', () => {
    const types = ['bigint', 'integer', 'text'];
    expect(visibleTypesForQuery(types, '  INT ')).toEqual(['bigint', 'integer']);
    expect(visibleTypesForQuery(types, '')).toBe(types);
  });
});
