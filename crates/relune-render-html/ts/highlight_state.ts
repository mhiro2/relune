import type { EdgeMetadata, TableMetadata } from './metadata';

export interface HighlightState {
  hoveredNode: string | null;
  selectedNode: string | null;
  /** Index into `edges` of the selected relationship, if any. */
  selectedEdge: number | null;
  /** Columns marked on the selected table, e.g. by a review finding. */
  emphasizedColumns: string[];
  traversalDepth: number;
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

  return {
    hoveredNode: null,
    selectedNode: null,
    selectedEdge: null,
    emphasizedColumns: [],
    traversalDepth: 1,
    tableById,
    inboundMap,
    outboundMap,
    edges,
  };
}
