import type { TableMetadata } from './metadata';

export type FacetId = 'schema' | 'kind' | 'columnType' | 'severity' | 'diffKind';
export type FilterMode = 'dim' | 'hide' | 'focus';

export interface FacetDefinition {
  id: FacetId;
  label: string;
  allValues: string[];
  selectedValues: Set<string>;
  counts: Map<string, number>;
  extractValues: (table: TableMetadata) => string[];
  hasSearch?: boolean | undefined;
}

export interface FilterEngineState {
  facets: Map<FacetId, FacetDefinition>;
  mode: FilterMode;
}

const DEFAULT_SCHEMA = '(default)';

function extractSchemaValues(table: TableMetadata): string[] {
  return [table.schema_name ?? DEFAULT_SCHEMA];
}

function extractKindValues(table: TableMetadata): string[] {
  return [table.kind];
}

function extractColumnTypeValues(table: TableMetadata): string[] {
  const types = new Set<string>();
  for (const col of table.columns ?? []) {
    const dt = (col.data_type ?? '').trim();
    if (dt !== '') {
      types.add(dt);
    }
  }
  return [...types];
}

function extractSeverityValues(table: TableMetadata): string[] {
  const issues = table.issues ?? [];
  if (issues.length === 0) return ['none'];
  const severities = new Set<string>();
  for (const issue of issues) {
    severities.add(issue.severity);
  }
  return [...severities];
}

function extractDiffKindValues(table: TableMetadata): string[] {
  return [table.diff_kind ?? 'unchanged'];
}

function buildFacet(
  id: FacetId,
  label: string,
  tables: TableMetadata[],
  extractValues: (table: TableMetadata) => string[],
  hasSearch?: boolean,
): FacetDefinition {
  const valueSet = new Set<string>();
  const counts = new Map<string, number>();

  for (const table of tables) {
    const tableValues = new Set(extractValues(table));
    for (const v of tableValues) {
      valueSet.add(v);
      counts.set(v, (counts.get(v) ?? 0) + 1);
    }
  }

  const allValues = [...valueSet].sort((a, b) =>
    a.localeCompare(b, undefined, { sensitivity: 'base' }),
  );

  return {
    id,
    label,
    allValues,
    selectedValues: new Set<string>(),
    counts,
    extractValues,
    hasSearch,
  };
}

export function createFilterEngineState(tables: TableMetadata[]): FilterEngineState {
  const facets = new Map<FacetId, FacetDefinition>();

  const schemaFacet = buildFacet('schema', 'Schema', tables, extractSchemaValues);
  if (schemaFacet.allValues.length > 1) {
    facets.set('schema', schemaFacet);
  }

  const kindFacet = buildFacet('kind', 'Kind', tables, extractKindValues);
  if (kindFacet.allValues.length > 1) {
    facets.set('kind', kindFacet);
  }

  const typeFacet = buildFacet('columnType', 'Column Type', tables, extractColumnTypeValues, true);
  if (typeFacet.allValues.length > 0) {
    facets.set('columnType', typeFacet);
  }

  const severityFacet = buildFacet('severity', 'Issues', tables, extractSeverityValues);
  if (severityFacet.allValues.length > 1) {
    facets.set('severity', severityFacet);
  }

  const diffFacet = buildFacet('diffKind', 'Changes', tables, extractDiffKindValues);
  const hasDiffData = tables.some((t) => t.diff_kind != null);
  if (hasDiffData) {
    facets.set('diffKind', diffFacet);
  }

  return { facets, mode: 'dim' };
}

// Column type fuzzy matching (ported from type_filter_state.ts)

export function columnMatchesSelectedType(columnType: string, selectedType: string): boolean {
  const column = columnType.trim().toLowerCase();
  const selected = selectedType.trim().toLowerCase();
  if (column === selected) return true;

  const base = (raw: string): string => {
    const index = raw.indexOf('(');
    return (index === -1 ? raw : raw.slice(0, index)).trim();
  };

  const baseColumn = base(column);
  const baseSelected = base(selected);

  const startsWithTypeToken = (value: string, token: string): boolean =>
    value === token ||
    value.startsWith(`${token}(`) ||
    value.startsWith(`${token} `) ||
    value.startsWith(`${token}[`) ||
    value.startsWith(`${token},`);

  return (
    baseColumn === baseSelected ||
    startsWithTypeToken(column, selected) ||
    startsWithTypeToken(selected, column)
  );
}

function tableMatchesFacet(table: TableMetadata, facet: FacetDefinition): boolean {
  if (facet.selectedValues.size === 0) return true;

  if (facet.id === 'columnType') {
    return (table.columns ?? []).some((col) =>
      [...facet.selectedValues].some((sel) => columnMatchesSelectedType(col.data_type ?? '', sel)),
    );
  }

  const tableValues = facet.extractValues(table);
  return tableValues.some((v) => facet.selectedValues.has(v));
}

export function tableMatchesAllFacets(table: TableMetadata, state: FilterEngineState): boolean {
  for (const facet of state.facets.values()) {
    if (!tableMatchesFacet(table, facet)) return false;
  }
  return true;
}

export function hasActiveFilters(state: FilterEngineState): boolean {
  for (const facet of state.facets.values()) {
    if (facet.selectedValues.size > 0) return true;
  }
  return false;
}

export interface FacetSummaryItem {
  facetId: FacetId;
  label: string;
  count: number;
  values: string[];
}

export function activeFilterSummary(state: FilterEngineState): FacetSummaryItem[] {
  const items: FacetSummaryItem[] = [];
  for (const facet of state.facets.values()) {
    if (facet.selectedValues.size > 0) {
      items.push({
        facetId: facet.id,
        label: facet.label,
        count: facet.selectedValues.size,
        values: [...facet.selectedValues].sort(),
      });
    }
  }
  return items;
}

export function visibleTypesForQuery(allTypes: string[], query: string): string[] {
  const needle = query.trim().toLowerCase();
  if (needle === '') return allTypes;
  return allTypes.filter((t) => t.toLowerCase().includes(needle));
}
