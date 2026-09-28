import { describe, expect, it } from 'vitest';

import {
  effectiveDialectNote,
  escapeHtml,
  exportFilename,
  exportFormatLabel,
  exportMimeType,
  formatDiagnosticCode,
  formatDuration,
  formatFindingTarget,
  inputCoverageNote,
  severityEmoji,
  severityRank,
  totalDiffChanges,
} from './format';
import type { DiffSummary, ExportFormat, ReviewFinding, ReviewSeverity } from './types';

describe('escapeHtml', () => {
  it('escapes markup-significant characters', () => {
    expect(escapeHtml(`<a href="x" title='y'>&</a>`)).toBe(
      '&lt;a href=&quot;x&quot; title=&#39;y&#39;&gt;&amp;&lt;/a&gt;',
    );
  });

  it('escapes ampersands first so entities are not double-decoded', () => {
    expect(escapeHtml('&lt;')).toBe('&amp;lt;');
  });
});

describe('formatDuration', () => {
  it('uses one decimal below 10 ms', () => {
    expect(formatDuration({ secs: 0, nanos: 1_250_000 })).toBe('1.3 ms');
  });

  it('uses whole milliseconds below one second', () => {
    expect(formatDuration({ secs: 0, nanos: 123_400_000 })).toBe('123 ms');
  });

  it('uses seconds from one second up', () => {
    expect(formatDuration({ secs: 2, nanos: 345_000_000 })).toBe('2.35 s');
  });
});

describe('formatDiagnosticCode', () => {
  it('pads the numeric part to three digits', () => {
    expect(
      formatDiagnosticCode({
        severity: 'warning',
        code: { prefix: 'PARSE', number: 7 },
        message: '',
      }),
    ).toBe('PARSE007');
    expect(
      formatDiagnosticCode({
        severity: 'error',
        code: { prefix: 'GRAPH', number: 1234 },
        message: '',
      }),
    ).toBe('GRAPH1234');
  });
});

describe('totalDiffChanges', () => {
  it('sums every counter in the summary', () => {
    const keys: (keyof DiffSummary)[] = [
      'tables_added',
      'tables_removed',
      'tables_modified',
      'columns_changed',
      'foreign_keys_changed',
      'indexes_changed',
      'views_added',
      'views_removed',
      'views_modified',
      'view_columns_changed',
      'view_definitions_changed',
      'enums_added',
      'enums_removed',
      'enums_modified',
      'enum_values_changed',
    ];
    const summary = Object.fromEntries(keys.map((key) => [key, 1])) as DiffSummary;
    expect(totalDiffChanges(summary)).toBe(keys.length);
  });
});

describe('review severity helpers', () => {
  it('orders severities from breaking to info', () => {
    const severities: ReviewSeverity[] = ['info', 'breaking', 'warning', 'caution'];
    expect(severities.toSorted((a, b) => severityRank(a) - severityRank(b))).toEqual([
      'breaking',
      'caution',
      'warning',
      'info',
    ]);
  });

  it('assigns a distinct emoji to each severity', () => {
    const emojis = (['breaking', 'caution', 'warning', 'info'] as const).map(severityEmoji);
    expect(new Set(emojis).size).toBe(4);
    expect(emojis.every((emoji) => emoji !== '')).toBe(true);
  });
});

describe('formatFindingTarget', () => {
  const finding = (overrides: Partial<ReviewFinding>): ReviewFinding => ({
    rule_id: 'risk/test',
    severity: 'warning',
    message: '',
    ...overrides,
  });

  it('prefers table.column, then table.fk, then the table alone', () => {
    expect(formatFindingTarget(finding({ table_name: 'users', column_name: 'email' }))).toBe(
      'users.email',
    );
    expect(formatFindingTarget(finding({ table_name: 'posts', fk_name: 'posts_user_fk' }))).toBe(
      'posts.posts_user_fk',
    );
    expect(formatFindingTarget(finding({ table_name: 'users' }))).toBe('users');
  });

  it('falls back to the fk name or the schema', () => {
    expect(formatFindingTarget(finding({ fk_name: 'fk' }))).toBe('fk');
    expect(formatFindingTarget(finding({ column_name: 'orphan' }))).toBe('(schema)');
    expect(formatFindingTarget(finding({}))).toBe('(schema)');
  });
});

describe('effectiveDialectNote', () => {
  it('stays silent when a dialect was chosen explicitly', () => {
    expect(effectiveDialectNote('postgres', 'postgres')).toBeNull();
  });

  it('explains how auto resolved', () => {
    expect(effectiveDialectNote('auto', 'mysql')).toContain('lock-risk review active');
    expect(effectiveDialectNote('auto', 'sqlite')).toContain('inactive on this dialect');
    expect(effectiveDialectNote('auto', 'auto')).toContain('could not infer');
  });
});

describe('inputCoverageNote', () => {
  it('returns null when both inputs are fully covered', () => {
    expect(
      inputCoverageNote({
        before: { empty: false, unsupported_constructs: 0 },
        after: { empty: false, unsupported_constructs: 0 },
      }),
    ).toBeNull();
  });

  it('lists empty inputs and skipped constructs with correct plurals', () => {
    const note = inputCoverageNote({
      before: { empty: true, unsupported_constructs: 1 },
      after: { empty: false, unsupported_constructs: 3 },
    });
    expect(note).toBe(
      'Review coverage is incomplete: the before input produced no schema objects; ' +
        '1 unsupported SQL construct in the before input was skipped; ' +
        '3 unsupported SQL constructs in the after input were skipped. ' +
        'Missing findings do not mean the migration is safe.',
    );
  });
});

describe('export format helpers', () => {
  const formats: ExportFormat[] = [
    'schema-json',
    'graph-json',
    'layout-json',
    'mermaid',
    'd2',
    'dot',
  ];

  it('gives every format a label and a distinct filename', () => {
    expect(formats.every((format) => exportFormatLabel(format) !== '')).toBe(true);
    expect(new Set(formats.map(exportFilename)).size).toBe(formats.length);
  });

  it('serves JSON formats as JSON and the rest as plain text', () => {
    expect(exportMimeType('graph-json')).toBe('application/json;charset=utf-8');
    expect(exportMimeType('mermaid')).toBe('text/plain;charset=utf-8');
    expect(exportFilename('mermaid')).toBe('relune-diagram.mmd');
  });
});
