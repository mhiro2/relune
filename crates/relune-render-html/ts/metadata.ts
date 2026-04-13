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
  diff_kind?: 'added' | 'removed' | 'modified' | null;
}

export interface IssueMetadata {
  severity: 'error' | 'warning' | 'info' | 'hint';
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
  diff_kind?: 'added' | 'removed' | 'modified' | null;
}

export interface EdgeMetadata {
  from: string;
  to: string;
  name?: string | null;
  from_columns: string[];
  to_columns: string[];
  kind: 'foreign_key' | 'enum_reference' | 'view_dependency';
  issues?: IssueMetadata[];
  diff_kind?: 'added' | 'removed' | 'modified' | null;
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

/** Display name for search / UI (matches previous JS: label or id). */
export function tableDisplayName(table: TableMetadata): string {
  return table.label || table.table_name || table.id;
}
