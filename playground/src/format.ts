import type {
  DiffSummary,
  ExportFormat,
  ReviewDialect,
  ReviewFinding,
  ReviewSeverity,
  WasmDiagnostic,
  WasmDuration,
  WasmReviewResult,
} from './types.js';

export function escapeHtml(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

export function formatDuration(duration: WasmDuration): string {
  const millis = duration.secs * 1_000 + duration.nanos / 1_000_000;
  if (millis >= 1_000) {
    return `${(millis / 1_000).toFixed(2)} s`;
  }
  if (millis >= 10) {
    return `${millis.toFixed(0)} ms`;
  }
  return `${millis.toFixed(1)} ms`;
}

export function formatDiagnosticCode(diagnostic: WasmDiagnostic): string {
  return `${diagnostic.code.prefix}${diagnostic.code.number.toString().padStart(3, '0')}`;
}

export function totalDiffChanges(summary: DiffSummary): number {
  return (
    summary.tables_added +
    summary.tables_removed +
    summary.tables_modified +
    summary.columns_changed +
    summary.foreign_keys_changed +
    summary.indexes_changed +
    summary.views_added +
    summary.views_removed +
    summary.views_modified +
    summary.view_columns_changed +
    summary.view_definitions_changed +
    summary.enums_added +
    summary.enums_removed +
    summary.enums_modified +
    summary.enum_values_changed
  );
}

export function severityRank(severity: ReviewSeverity): number {
  switch (severity) {
    case 'breaking':
      return 0;
    case 'caution':
      return 1;
    case 'warning':
      return 2;
    case 'info':
      return 3;
    default:
      return Number.MAX_SAFE_INTEGER;
  }
}

export function severityEmoji(severity: ReviewSeverity): string {
  switch (severity) {
    case 'breaking':
      return '🔴';
    case 'caution':
      return '🟡';
    case 'warning':
      return '🟠';
    case 'info':
      return '⚪';
    default:
      return '';
  }
}

export function formatFindingTarget(finding: ReviewFinding): string {
  const table = finding.table_name ?? '';
  const column = finding.column_name ?? '';
  const fk = finding.fk_name ?? '';
  if (table && column) {
    return `${table}.${column}`;
  }
  if (table && fk) {
    return `${table}.${fk}`;
  }
  if (table) {
    return table;
  }
  if (fk) {
    return fk;
  }
  return '(schema)';
}

export function effectiveDialectNote(
  requested: ReviewDialect,
  effective: ReviewDialect,
): string | null {
  if (requested !== 'auto') {
    return null;
  }
  if (effective === 'postgres' || effective === 'mysql') {
    return `auto resolved to ${effective}; lock-risk review active.`;
  }
  if (effective === 'sqlite') {
    return 'auto resolved to sqlite; lock-risk rules are inactive on this dialect.';
  }
  return 'Auto could not infer a single dialect; lock-risk rules are inactive.';
}

export function inputCoverageNote(inputs: WasmReviewResult['inputs']): string | null {
  const notes: string[] = [];
  for (const [label, coverage] of [
    ['before', inputs.before],
    ['after', inputs.after],
  ] as const) {
    if (coverage.empty) {
      notes.push(`the ${label} input produced no schema objects`);
    }
    const count = coverage.unsupported_constructs;
    if (count > 0) {
      notes.push(
        `${count} unsupported SQL construct${count === 1 ? '' : 's'} in the ${label} input ${count === 1 ? 'was' : 'were'} skipped`,
      );
    }
  }
  if (notes.length === 0) {
    return null;
  }
  return `Review coverage is incomplete: ${notes.join('; ')}. Missing findings do not mean the migration is safe.`;
}

export function exportFormatLabel(format: ExportFormat): string {
  switch (format) {
    case 'schema-json':
      return 'Schema JSON';
    case 'graph-json':
      return 'Graph JSON';
    case 'layout-json':
      return 'Layout JSON';
    case 'mermaid':
      return 'Mermaid';
    case 'd2':
      return 'D2';
    case 'dot':
      return 'DOT';
    default:
      return '';
  }
}

export function exportFilename(format: ExportFormat): string {
  switch (format) {
    case 'schema-json':
      return 'relune-schema.json';
    case 'graph-json':
      return 'relune-graph.json';
    case 'layout-json':
      return 'relune-layout.json';
    case 'mermaid':
      return 'relune-diagram.mmd';
    case 'd2':
      return 'relune-diagram.d2';
    case 'dot':
      return 'relune-diagram.dot';
    default:
      return 'relune-export';
  }
}

export function exportMimeType(format: ExportFormat): string {
  switch (format) {
    case 'schema-json':
    case 'graph-json':
    case 'layout-json':
      return 'application/json;charset=utf-8';
    default:
      return 'text/plain;charset=utf-8';
  }
}
