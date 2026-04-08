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

function collectNeighborhood(nodeId: string, state: HighlightState): HighlightNeighborhood {
  const inbound = state.inboundMap[nodeId] ?? [];
  const outbound = state.outboundMap[nodeId] ?? [];
  const neighborIds = new Set<string>();
  const inboundNodeIds = new Set<string>();
  const outboundNodeIds = new Set<string>();

  for (const relation of inbound) {
    neighborIds.add(relation.node);
    inboundNodeIds.add(relation.node);
  }
  for (const relation of outbound) {
    neighborIds.add(relation.node);
    outboundNodeIds.add(relation.node);
  }

  const connectedEdgeIndices = new Set<number>();
  state.edges.forEach((edge, index) => {
    if (edge.from === nodeId || edge.to === nodeId) {
      connectedEdgeIndices.add(index);
    }
  });

  return { neighborIds, connectedEdgeIndices, inboundNodeIds, outboundNodeIds };
}

export function computeNeighborHighlights(
  nodeId: string,
  state: HighlightState,
): NeighborHighlight {
  return { selectedId: nodeId, ...collectNeighborhood(nodeId, state) };
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
