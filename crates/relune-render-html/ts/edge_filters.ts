/**
 * Dim edges when either endpoint is dimmed by table search or column-type filter.
 */

function nodeId(el: Element): string {
  return el.getAttribute('data-id') ?? el.getAttribute('data-table-id') ?? '';
}

/** Sync `.dimmed-by-edge-filter` on edges from node classes. */
export function syncEdgeDimming(svgRoot: Element): void {
  const nodeById = new Map<string, Element>();
  svgRoot.querySelectorAll('.node').forEach((node) => {
    const id = nodeId(node);
    if (id !== '') {
      nodeById.set(id, node);
    }
  });

  svgRoot.querySelectorAll('.edge').forEach((edge) => {
    const fromId = edge.getAttribute('data-from') ?? '';
    const toId = edge.getAttribute('data-to') ?? '';
    const fromEl = nodeById.get(fromId);
    const toEl = nodeById.get(toId);

    const endpointHidden =
      fromEl?.classList.contains('hidden-by-filter') === true ||
      toEl?.classList.contains('hidden-by-filter') === true;

    const endpointDimmed =
      fromEl?.classList.contains('dimmed-by-search') === true ||
      toEl?.classList.contains('dimmed-by-search') === true ||
      fromEl?.classList.contains('dimmed-by-filter') === true ||
      toEl?.classList.contains('dimmed-by-filter') === true;

    edge.classList.toggle('hidden-by-filter', endpointHidden);
    edge.classList.toggle('dimmed-by-edge-filter', !endpointHidden && endpointDimmed);
  });
}
