import type { TableMetadata } from './metadata';
import type { HighlightState } from './highlight_state';

interface HighlightNeighborhood {
  neighborIds: Set<string>;
  connectedEdgeIndices: Set<number>;
  inboundNodeIds: Set<string>;
  outboundNodeIds: Set<string>;
}

export interface NeighborHighlight extends HighlightNeighborhood {
  selectedId: string;
}

export interface HoverPreview extends HighlightNeighborhood {
  hoveredId: string;
}

function collectNeighborhood(
  nodeId: string,
  state: HighlightState,
  depth: number = 1,
): HighlightNeighborhood {
  const neighborIds = new Set<string>();
  const inboundNodeIds = new Set<string>();
  const outboundNodeIds = new Set<string>();

  // Track edges traversed during BFS so only reachable edges are highlighted
  const traversedEdgeKeys = new Set<string>();

  // BFS traversal up to `depth` hops
  const visited = new Set<string>([nodeId]);
  let frontier = [nodeId];

  for (let hop = 0; hop < depth && frontier.length > 0; hop++) {
    const nextFrontier: string[] = [];
    for (const current of frontier) {
      for (const relation of state.inboundMap[current] ?? []) {
        neighborIds.add(relation.node);
        traversedEdgeKeys.add(edgeKey(relation.edge));
        if (current === nodeId) inboundNodeIds.add(relation.node);
        if (!visited.has(relation.node)) {
          visited.add(relation.node);
          nextFrontier.push(relation.node);
        }
      }
      for (const relation of state.outboundMap[current] ?? []) {
        neighborIds.add(relation.node);
        traversedEdgeKeys.add(edgeKey(relation.edge));
        if (current === nodeId) outboundNodeIds.add(relation.node);
        if (!visited.has(relation.node)) {
          visited.add(relation.node);
          nextFrontier.push(relation.node);
        }
      }
    }
    frontier = nextFrontier;
  }

  // Only highlight edges that were actually traversed
  const connectedEdgeIndices = new Set<number>();
  state.edges.forEach((edge, index) => {
    if (traversedEdgeKeys.has(edgeKey(edge))) {
      connectedEdgeIndices.add(index);
    }
  });

  return { neighborIds, connectedEdgeIndices, inboundNodeIds, outboundNodeIds };
}

function edgeKey(edge: { from: string; to: string; name?: string | null }): string {
  return `${edge.from}\0${edge.to}\0${edge.name ?? ''}`;
}

export function computeNeighborHighlights(
  nodeId: string,
  state: HighlightState,
  depth: number = 1,
): NeighborHighlight {
  return { selectedId: nodeId, ...collectNeighborhood(nodeId, state, depth) };
}

export function computeHoverPreview(nodeId: string, state: HighlightState): HoverPreview {
  return { hoveredId: nodeId, ...collectNeighborhood(nodeId, state) };
}

export function matchesBrowserQuery(table: TableMetadata, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (needle === '') {
    return true;
  }

  return (
    table.id.toLowerCase().includes(needle) ||
    table.label.toLowerCase().includes(needle) ||
    table.table_name.toLowerCase().includes(needle) ||
    table.columns.some(
      (column) =>
        column.name.toLowerCase().includes(needle) ||
        column.data_type.toLowerCase().includes(needle),
    )
  );
}
