import { describe, expect, it } from 'vitest';

import {
  arraysEqual,
  buildQueryString,
  DEFAULT_STATE,
  parsePositiveInteger,
  parseQueryState,
  parseStoredState,
  sanitizeState,
  serializePatterns,
  splitPatterns,
  toBuiltinExampleId,
} from './state';
import type { PersistedState } from './types';

describe('splitPatterns / serializePatterns', () => {
  it('trims entries and drops empty ones', () => {
    expect(splitPatterns(' users , posts,, sales.* ,')).toEqual(['users', 'posts', 'sales.*']);
    expect(splitPatterns('')).toEqual([]);
  });

  it('round-trips through the serialized form', () => {
    const patterns = ['users', 'sales.*'];
    expect(serializePatterns(patterns)).toBe('users, sales.*');
    expect(splitPatterns(serializePatterns(patterns))).toEqual(patterns);
  });
});

describe('arraysEqual', () => {
  it('compares element-wise and by length', () => {
    expect(arraysEqual(['a', 'b'], ['a', 'b'])).toBe(true);
    expect(arraysEqual(['a', 'b'], ['b', 'a'])).toBe(false);
    expect(arraysEqual(['a'], ['a', 'b'])).toBe(false);
    expect(arraysEqual([], [])).toBe(true);
  });
});

describe('parsePositiveInteger', () => {
  it('accepts integers of at least one', () => {
    expect(parsePositiveInteger('1')).toBe(1);
    expect(parsePositiveInteger('12abc')).toBe(12);
  });

  it('rejects zero, negatives and non-numbers', () => {
    expect(parsePositiveInteger('0')).toBeUndefined();
    expect(parsePositiveInteger('-3')).toBeUndefined();
    expect(parsePositiveInteger('')).toBeUndefined();
    expect(parsePositiveInteger('abc')).toBeUndefined();
  });
});

describe('sanitizeState', () => {
  it('keeps valid enum values and strings', () => {
    expect(
      sanitizeState({
        example: 'ecommerce',
        mode: 'compare',
        theme: 'dark',
        layout: 'force-directed',
        direction: 'left-to-right',
        edgeStyle: 'orthogonal',
        groupBy: 'schema',
        exportFormat: 'd2',
        compareView: 'review',
        compareReviewDialect: 'mysql',
        viewpoint: '  billing  ',
        sql: 'CREATE TABLE t (id int);',
      }),
    ).toEqual({
      example: 'ecommerce',
      mode: 'compare',
      theme: 'dark',
      layout: 'force-directed',
      direction: 'left-to-right',
      edgeStyle: 'orthogonal',
      groupBy: 'schema',
      exportFormat: 'd2',
      compareView: 'review',
      compareReviewDialect: 'mysql',
      viewpoint: 'billing',
      sql: 'CREATE TABLE t (id int);',
    });
  });

  it('drops unknown enum values and non-string fields', () => {
    const tampered = {
      example: 'unknown',
      mode: 'draw',
      theme: 'sepia',
      layout: 'circular',
      direction: 'diagonal',
      edgeStyle: 'wavy',
      groupBy: 'owner',
      exportFormat: 'png',
      compareView: 'html',
      compareReviewDialect: 'oracle',
      depth: 3,
      sql: null,
    } as unknown as Partial<PersistedState>;
    expect(sanitizeState(tampered)).toEqual({});
  });
});

describe('parseStoredState', () => {
  it('returns an empty state for missing or malformed payloads', () => {
    expect(parseStoredState(null)).toEqual({});
    expect(parseStoredState('')).toEqual({});
    expect(parseStoredState('{not json')).toEqual({});
  });

  it('sanitizes the stored payload', () => {
    expect(parseStoredState(JSON.stringify({ mode: 'lint', theme: 'neon', sql: 'x' }))).toEqual({
      mode: 'lint',
      sql: 'x',
    });
  });
});

describe('parseQueryState', () => {
  it('maps short query keys onto state fields', () => {
    expect(
      parseQueryState(
        '?example=multi-schema&mode=export&edges=straight&group=prefix&focus=orders&depth=2&include=a,b&exclude=c&export=dot&table=users&rules=r1&compare=json&reviewDialect=sqlite&viewpoint=sales',
      ),
    ).toEqual({
      example: 'multi-schema',
      mode: 'export',
      edgeStyle: 'straight',
      groupBy: 'prefix',
      focusTable: 'orders',
      depth: '2',
      includeTables: 'a,b',
      excludeTables: 'c',
      exportFormat: 'dot',
      inspectTable: 'users',
      lintRules: 'r1',
      compareView: 'json',
      compareReviewDialect: 'sqlite',
      viewpoint: 'sales',
    });
  });

  it('ignores invalid values and never reads SQL from the URL', () => {
    expect(parseQueryState('?mode=bogus&sql=DROP%20TABLE%20users')).toEqual({});
  });
});

describe('buildQueryString', () => {
  const state = (overrides: Partial<PersistedState>): PersistedState => ({
    ...DEFAULT_STATE,
    ...overrides,
  });

  it('always writes the core view settings and omits defaults', () => {
    expect(buildQueryString(DEFAULT_STATE)).toBe(
      'example=simple-blog&mode=render&theme=light&layout=hierarchical&direction=top-to-bottom&edges=curved&group=none',
    );
  });

  it('writes scope settings and non-default depth', () => {
    const params = new URLSearchParams(
      buildQueryString(
        state({
          viewpoint: 'authoring',
          focusTable: 'posts',
          depth: '3',
          includeTables: 'users',
          excludeTables: 'logs',
        }),
      ),
    );
    expect(params.get('viewpoint')).toBe('authoring');
    expect(params.get('focus')).toBe('posts');
    expect(params.get('depth')).toBe('3');
    expect(params.get('include')).toBe('users');
    expect(params.get('exclude')).toBe('logs');
  });

  it('only writes mode-specific settings for the active mode', () => {
    const renderParams = new URLSearchParams(
      buildQueryString(
        state({ exportFormat: 'd2', inspectTable: 'users', lintRules: 'r', compareView: 'json' }),
      ),
    );
    expect(renderParams.has('export')).toBe(false);
    expect(renderParams.has('table')).toBe(false);
    expect(renderParams.has('rules')).toBe(false);
    expect(renderParams.has('compare')).toBe(false);

    expect(new URLSearchParams(buildQueryString(state({ mode: 'export' }))).get('export')).toBe(
      'schema-json',
    );
    expect(
      new URLSearchParams(buildQueryString(state({ mode: 'inspect', inspectTable: 'users' }))).get(
        'table',
      ),
    ).toBe('users');
    expect(
      new URLSearchParams(buildQueryString(state({ mode: 'lint', lintRules: 'r1' }))).get('rules'),
    ).toBe('r1');
  });

  it('writes the review dialect only for a non-default review comparison', () => {
    const compare = (overrides: Partial<PersistedState>) =>
      new URLSearchParams(buildQueryString(state({ mode: 'compare', ...overrides })));
    expect(compare({ compareView: 'text' }).get('compare')).toBe('text');
    expect(compare({ compareView: 'review' }).has('reviewDialect')).toBe(false);
    expect(
      compare({ compareView: 'text', compareReviewDialect: 'mysql' }).has('reviewDialect'),
    ).toBe(false);
    expect(
      compare({ compareView: 'review', compareReviewDialect: 'mysql' }).get('reviewDialect'),
    ).toBe('mysql');
  });

  it('round-trips through parseQueryState', () => {
    const original = state({
      example: 'ecommerce',
      mode: 'compare',
      theme: 'dark',
      compareView: 'review',
      compareReviewDialect: 'postgres',
      focusTable: 'orders',
      depth: '2',
    });
    const restored = { ...DEFAULT_STATE, ...parseQueryState(`?${buildQueryString(original)}`) };
    expect(restored).toEqual(original);
  });
});

describe('toBuiltinExampleId', () => {
  it('maps the custom example onto the default built-in one', () => {
    expect(toBuiltinExampleId('custom')).toBe('simple-blog');
    expect(toBuiltinExampleId('ecommerce')).toBe('ecommerce');
  });
});
