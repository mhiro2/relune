import { tableDisplayName, type TableMetadata } from './metadata';

export interface SearchMatch {
  node: Element;
  matches: boolean;
}

/**
 * Whether a table matches a search query by name, schema, or column name / type.
 *
 * This is the single matcher behind the search box, the object browser, and
 * URL restoration, so all three agree on what a query finds. It reads the
 * embedded metadata rather than the SVG text, which also carries tooltips,
 * badge labels, and kind captions.
 */
export function matchesTableQuery(table: TableMetadata, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (needle === '') {
    return true;
  }
  const includes = (value: string | null | undefined): boolean =>
    (value ?? '').toLowerCase().includes(needle);

  return (
    includes(tableDisplayName(table)) ||
    includes(table.id) ||
    includes(table.table_name) ||
    includes(table.schema_name) ||
    table.columns.some((column) => includes(column.name) || includes(column.data_type))
  );
}

export function computeSearchMatches(
  nodes: NodeListOf<Element>,
  tablesById: ReadonlyMap<string, TableMetadata>,
  query: string,
): { results: SearchMatch[]; matchCount: number; total: number } {
  const needle = query.trim().toLowerCase();
  const results: SearchMatch[] = [];
  let matchCount = 0;

  nodes.forEach((node) => {
    const tableId = node.getAttribute('data-id') ?? node.getAttribute('data-table-id') ?? '';
    const table = tablesById.get(tableId);
    const matches =
      table === undefined
        ? tableId.toLowerCase().includes(needle)
        : matchesTableQuery(table, query);

    results.push({ node, matches });
    if (matches) matchCount += 1;
  });

  return { results, matchCount, total: nodes.length };
}
