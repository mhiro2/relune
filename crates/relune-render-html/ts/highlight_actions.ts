import type { HighlightState } from './highlight_state';
import type { EdgeMetadata } from './metadata';

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

  // Cycles and self-references lead back to the root; it is not its own neighbor.
  neighborIds.delete(nodeId);
  inboundNodeIds.delete(nodeId);
  outboundNodeIds.delete(nodeId);

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

/** One selected relationship: its edge and the columns on each end. */
export interface RelationHighlight {
  edgeIndex: number;
  fromId: string;
  toId: string;
  fromColumns: readonly string[];
  toColumns: readonly string[];
}

export function computeRelationHighlight(
  edgeIndex: number,
  state: HighlightState,
): RelationHighlight | null {
  const edge = state.edges[edgeIndex];
  if (edge === undefined) return null;
  return {
    edgeIndex,
    fromId: edge.from,
    toId: edge.to,
    fromColumns: edge.from_columns,
    toColumns: edge.to_columns,
  };
}

/**
 * Column correspondences of a relationship, one `from.col → to.col` line per
 * column pair, so a composite key reads as its full set of pairs.
 */
/** Percent-escapes the characters a relationship key uses as delimiters. */
function escapeKeyPart(part: string): string {
  return part.replaceAll(/[%(),>]/g, (char) => `%${char.charCodeAt(0).toString(16).toUpperCase()}`);
}

/**
 * Readable key naming one relationship in a shared link, e.g.
 * `posts(user_id)>users(id)`. It names both ends and their columns, so it
 * stays valid while unrelated relationships come and go; delimiters inside
 * quoted identifiers are escaped so no two relationships share a key.
 */
export function relationKey(edge: EdgeMetadata): string {
  const end = (table: string, columns: readonly string[]): string =>
    `${escapeKeyPart(table)}(${columns.map(escapeKeyPart).join(',')})`;
  return `${end(edge.from, edge.from_columns)}>${end(edge.to, edge.to_columns)}`;
}

export function relationColumnPairs(edge: EdgeMetadata): string[] {
  const pairCount = Math.min(edge.from_columns.length, edge.to_columns.length);
  if (pairCount === 0) {
    // Enum references know only the source column; keep it so two columns
    // using the same enum stay distinguishable.
    if (edge.from_columns.length > 0) {
      return edge.from_columns.map((column) => `${edge.from}.${column} → ${edge.to}`);
    }
    return [`${edge.from} → ${edge.to}`];
  }
  return edge.from_columns
    .slice(0, pairCount)
    .map((column, index) => `${edge.from}.${column} → ${edge.to}.${edge.to_columns[index]}`);
}

/**
 * Index the arrow, Home, or End `key` moves to among `count` relationship
 * lines, skipping lines `isFocusable` rejects. Returns `null` for other keys
 * or when there is nowhere to go.
 */
export function nextLineIndex(
  count: number,
  current: number,
  key: string,
  isFocusable: (index: number) => boolean,
): number | null {
  const scan = (start: number, step: number): number | null => {
    for (let index = start; index >= 0 && index < count; index += step) {
      if (isFocusable(index)) return index;
    }
    return null;
  };
  switch (key) {
    case 'ArrowRight':
    case 'ArrowDown':
      return scan(current + 1, 1);
    case 'ArrowLeft':
    case 'ArrowUp':
      return scan(current - 1, -1);
    case 'Home':
      return scan(0, 1);
    case 'End':
      return scan(count - 1, -1);
    default:
      return null;
  }
}
