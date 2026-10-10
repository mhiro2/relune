/**
 * Client-side view of graph metadata embedded in HTML.
 * Keep in sync with `crates/relune-render-html/src/metadata.rs` (serde JSON keys).
 */

export interface ColumnMetadata {
  name: string;
  data_type: string;
  nullable: boolean;
  is_primary_key: boolean;
  is_foreign_key: boolean;
  is_indexed: boolean;
  diff_kind?: DiffKind | null;
  /** Type before a diff changed it; shown as `previous → data_type`. */
  previous_data_type?: string | null;
}

export type DiffKind = 'added' | 'removed' | 'modified';

/** Review severity of a risk, mildest first. */
export type RiskSeverity = 'info' | 'warning' | 'caution' | 'breaking';

const RISK_SEVERITY_ORDER: readonly RiskSeverity[] = ['info', 'warning', 'caution', 'breaking'];

/** A review risk on a table, kept apart from its diff change kind. */
export interface IssueMetadata {
  severity: RiskSeverity;
  message: string;
  hint?: string | null;
  rule_id?: string | null;
}

export interface TableMetadata {
  id: string;
  label: string;
  schema_name?: string | null;
  table_name: string;
  kind: 'table' | 'view' | 'enum';
  columns: ColumnMetadata[];
  inbound_count: number;
  outbound_count: number;
  is_join_table_candidate: boolean;
  issues?: IssueMetadata[];
  diff_kind?: DiffKind | null;
  /** Individual changes under the diff change, e.g. `+ email`. */
  diff_details?: string[];
}

export interface EdgeMetadata {
  from: string;
  to: string;
  name?: string | null;
  from_columns: string[];
  to_columns: string[];
  kind: 'foreign_key' | 'enum_reference' | 'view_dependency';
  diff_kind?: DiffKind | null;
}

export interface GroupMetadata {
  id: string;
  label: string;
  table_ids: string[];
}

export interface GraphMetadata {
  tables: TableMetadata[];
  edges: EdgeMetadata[];
  groups: GroupMetadata[];
}

const METADATA_ELEMENT_ID = 'relune-metadata';

/** Parse embedded JSON metadata, or `null` if missing or invalid. */
export function parseReluneMetadata(): GraphMetadata | null {
  const el = document.getElementById(METADATA_ELEMENT_ID);
  const raw = el?.textContent;
  if (raw == null || raw === '') {
    return null;
  }
  try {
    return JSON.parse(raw) as GraphMetadata;
  } catch {
    return null;
  }
}

/** Highest risk severity and how many risks carry it, or `undefined` without risks. */
export function topRisk(
  issues: readonly IssueMetadata[],
): { severity: RiskSeverity; count: number } | undefined {
  let top: RiskSeverity | undefined;
  for (const issue of issues) {
    if (
      top === undefined ||
      RISK_SEVERITY_ORDER.indexOf(issue.severity) > RISK_SEVERITY_ORDER.indexOf(top)
    ) {
      top = issue.severity;
    }
  }
  if (top === undefined) return undefined;
  const severity = top;
  return { severity, count: issues.filter((issue) => issue.severity === severity).length };
}

/** Risk label naming the highest severity and its count, e.g. `breaking 1`. */
export function riskLabel(issues: readonly IssueMetadata[]): string | undefined {
  const top = topRisk(issues);
  return top === undefined ? undefined : `${top.severity} ${top.count}`;
}

const DIFF_MARKERS: Record<DiffKind, string> = { added: '+', removed: '\u2212', modified: '~' };

/** Marker of a diff change kind, matching the SVG cards. */
export function diffMarker(kind: DiffKind): string {
  return DIFF_MARKERS[kind];
}

/** Display name for search / UI (matches previous JS: label or id). */
export function tableDisplayName(table: TableMetadata): string {
  return table.label || table.table_name || table.id;
}
