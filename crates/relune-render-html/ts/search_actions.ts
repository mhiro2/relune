export interface SearchMatch {
  node: Element;
  matches: boolean;
}

export function computeSearchMatches(
  nodes: NodeListOf<Element>,
  tableNames: Record<string, string>,
  query: string,
): { results: SearchMatch[]; matchCount: number; total: number } {
  const q = query.toLowerCase().trim();
  const results: SearchMatch[] = [];
  let matchCount = 0;

  nodes.forEach((node) => {
    if (q === '') {
      results.push({ node, matches: true });
      matchCount += 1;
      return;
    }

    const tableId = node.getAttribute('data-id') ?? node.getAttribute('data-table-id') ?? '';
    const tableName = tableNames[tableId] ?? tableId;
    const nodeText = node.textContent?.toLowerCase() ?? '';

    const matches =
      tableName.toLowerCase().includes(q) ||
      tableId.toLowerCase().includes(q) ||
      nodeText.includes(q);

    results.push({ node, matches });
    if (matches) matchCount += 1;
  });

  return { results, matchCount, total: nodes.length };
}
