/**
 * Client-side view of the review report embedded in an HTML review.
 * Keep in sync with `crates/relune-render-html/src/review.rs` and
 * `relune_core::RiskFinding` (serde JSON keys).
 */

import type { RiskSeverity } from './metadata';

export type LineChange = 'unchanged' | 'removed' | 'added';

export interface DefinitionLine {
  change: LineChange;
  text: string;
}

export interface DefinitionDiff {
  lines: DefinitionLine[];
}

export interface ReviewFinding {
  rule_id: string;
  severity: RiskSeverity;
  message: string;
  mitigation?: string | null;
  table_id?: string | null;
  table_name?: string | null;
  column_name?: string | null;
  fk_name?: string | null;
  constraint_name?: string | null;
  columns?: string[];
  related_table_id?: string | null;
  /** Migration statements that make the change. */
  sources?: SourceLocation[];
  /** How the migration SQL carries the change out. */
  execution?: ExecutionNote | null;
  rule_description: string;
  focus_lines?: number[];
}

export interface SourceLocation {
  file: string;
  line: number;
}

export type ExecutionVerdict = 'mitigated' | 'confirmed' | 'unverified';

export interface ExecutionNote {
  verdict: ExecutionVerdict;
  detail: string;
}

export interface ReportStatement extends SourceLocation {
  sql: string;
  in_transaction: boolean;
}

export interface ReviewSummary {
  breaking: number;
  caution: number;
  warning: number;
  info: number;
}

export interface ReviewReport {
  summary: ReviewSummary;
  findings: ReviewFinding[];
  suppressed: number;
  dialect_note?: string | null;
  coverage_notes?: string[];
  definitions?: Record<string, DefinitionDiff>;
  migration_note?: string | null;
  statements?: ReportStatement[];
}

/** `file:line`, as text reports print a source location. */
export function sourceLabel(location: SourceLocation): string {
  return `${location.file}:${location.line}`;
}

/** Severities from most to least severe, the order findings are listed in. */
export const REVIEW_SEVERITIES: readonly RiskSeverity[] = [
  'breaking',
  'caution',
  'warning',
  'info',
];

const REVIEW_ELEMENT_ID = 'relune-review';

/** Parse the embedded review report, or `null` if missing or invalid. */
export function parseReviewReport(): ReviewReport | null {
  const raw = document.getElementById(REVIEW_ELEMENT_ID)?.textContent;
  if (raw == null || raw.trim() === '') return null;
  try {
    return JSON.parse(raw) as ReviewReport;
  } catch {
    return null;
  }
}

/** What a finding is about, e.g. `users.email`, matching the text report. */
export function findingTarget(finding: ReviewFinding): string {
  const table = finding.table_name ?? null;
  if (table !== null) {
    if (finding.column_name != null) return `${table}.${finding.column_name}`;
    if (finding.fk_name != null) return `${table}.${finding.fk_name}`;
    return table;
  }
  return finding.fk_name ?? '(schema)';
}

/** Columns to mark on the diagram: every key column, or the one column. */
export function findingColumns(finding: ReviewFinding): string[] {
  if (finding.columns !== undefined && finding.columns.length > 0) return finding.columns;
  return finding.column_name == null ? [] : [finding.column_name];
}

export type ExcerptItem = { kind: 'line'; index: number } | { kind: 'gap'; count: number };

/**
 * Lines worth reading in a definition diff: every changed or focused line
 * plus `context` lines around each. Runs of other lines collapse into gaps.
 */
export function definitionExcerpt(
  lines: readonly DefinitionLine[],
  focus: readonly number[],
  context = 2,
): ExcerptItem[] {
  const keep = new Set<number>();
  const anchors = new Set(focus);
  lines.forEach((line, index) => {
    if (line.change !== 'unchanged') anchors.add(index);
  });
  // The heading line names the table, so it always stays.
  if (lines.length > 0) keep.add(0);
  for (const anchor of anchors) {
    for (let index = anchor - context; index <= anchor + context; index++) {
      if (index >= 0 && index < lines.length) keep.add(index);
    }
  }

  const items: ExcerptItem[] = [];
  let gap = 0;
  lines.forEach((_line, index) => {
    if (keep.has(index)) {
      if (gap > 0) items.push({ kind: 'gap', count: gap });
      gap = 0;
      items.push({ kind: 'line', index });
    } else {
      gap += 1;
    }
  });
  if (gap > 0) items.push({ kind: 'gap', count: gap });
  return items;
}

/**
 * Index of the next visible finding after `current` in `delta` direction,
 * wrapping around, or `null` when no finding is visible.
 */
export function stepFinding(
  visible: readonly boolean[],
  current: number | null,
  delta: 1 | -1,
): number | null {
  const count = visible.length;
  if (!visible.some(Boolean)) return null;
  let index = current ?? (delta === 1 ? -1 : count);
  for (let step = 0; step < count; step++) {
    index = (index + delta + count) % count;
    if (visible[index] === true) return index;
  }
  return null;
}
