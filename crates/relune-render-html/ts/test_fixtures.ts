import type { ColumnMetadata, EdgeMetadata, GraphMetadata, TableMetadata } from './metadata';

// Shared builders for viewer tests. Not bundled: esbuild only follows imports
// from the entry points listed in esbuild.config.mjs.

export function column(
  name: string,
  dataType: string,
  overrides: Partial<ColumnMetadata> = {},
): ColumnMetadata {
  return {
    name,
    data_type: dataType,
    nullable: false,
    is_primary_key: false,
    is_foreign_key: false,
    is_indexed: false,
    ...overrides,
  };
}

export function table(id: string, overrides: Partial<TableMetadata> = {}): TableMetadata {
  return {
    id,
    label: id,
    table_name: id,
    kind: 'table',
    columns: [],
    inbound_count: 0,
    outbound_count: 0,
    is_join_table_candidate: false,
    ...overrides,
  };
}

export function edge(
  from: string,
  to: string,
  overrides: Partial<EdgeMetadata> = {},
): EdgeMetadata {
  return {
    from,
    to,
    from_columns: [],
    to_columns: [],
    kind: 'foreign_key',
    ...overrides,
  };
}

/** Writes the embedded metadata script the viewer modules read on load. */
export function metadataScript(metadata: Partial<GraphMetadata>): string {
  const full: GraphMetadata = { tables: [], edges: [], groups: [], ...metadata };
  return `<script type="application/json" id="relune-metadata">${JSON.stringify(full)}</script>`;
}

/** Drops the viewer runtime shared through `window` so each test starts clean. */
export function resetViewerRuntime(): void {
  const viewerWindow = window as unknown as Record<symbol, unknown>;
  for (const key of ['runtime', 'ready_modules', 'waiters']) {
    delete viewerWindow[Symbol.for(`relune.viewer.${key}`)];
  }
}
