import type { EdgeMetadata, TableMetadata } from './metadata';

export interface HighlightState {
  hoveredNode: string | null;
  selectedNode: string | null;
  tableById: Map<string, TableMetadata>;
  inboundMap: Record<string, { node: string; edge: EdgeMetadata }[]>;
  outboundMap: Record<string, { node: string; edge: EdgeMetadata }[]>;
  edges: EdgeMetadata[];
}

export function createHighlightState(
  tables: TableMetadata[],
  edges: EdgeMetadata[],
): HighlightState {
  const tableById = new Map(tables.map((table) => [table.id, table]));

  const inboundMap: Record<string, { node: string; edge: EdgeMetadata }[]> = {};
  const outboundMap: Record<string, { node: string; edge: EdgeMetadata }[]> = {};

  for (const edge of edges) {
    (outboundMap[edge.from] ??= []).push({ node: edge.to, edge });
    (inboundMap[edge.to] ??= []).push({ node: edge.from, edge });
  }

  return { hoveredNode: null, selectedNode: null, tableById, inboundMap, outboundMap, edges };
}
