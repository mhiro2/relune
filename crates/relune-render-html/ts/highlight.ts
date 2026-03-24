import { parseReluneMetadata, type EdgeMetadata } from './metadata';

{
  const metadata = parseReluneMetadata();
  const edges: EdgeMetadata[] = metadata?.edges ?? [];

  const inboundMap: Record<string, { node: string; edge: EdgeMetadata }[]> = {};
  const outboundMap: Record<string, { node: string; edge: EdgeMetadata }[]> = {};

  for (const edge of edges) {
    const from = edge.from;
    const to = edge.to;

    if (!outboundMap[from]) {
      outboundMap[from] = [];
    }
    outboundMap[from].push({ node: to, edge });

    if (!inboundMap[to]) {
      inboundMap[to] = [];
    }
    inboundMap[to].push({ node: from, edge });
  }

  const canvas = document.getElementById('canvas');
  const svgRoot = canvas?.querySelector('svg');
  if (svgRoot) {
    let selectedNode: string | null = null;

    const getNodes = (): NodeListOf<Element> =>
      svgRoot.querySelectorAll('.node[data-id], .table-node[data-table-id]');

    const getNodeId = (node: Element): string | null =>
      node.getAttribute('data-id') ?? node.getAttribute('data-table-id');

    const clearHighlights = (): void => {
      getNodes().forEach((n) => {
        n.classList.remove(
          'highlighted-neighbor',
          'dimmed-by-highlight',
          'selected-node',
          'inbound',
          'outbound',
        );
      });
      svgRoot.querySelectorAll('.edge').forEach((e) => {
        e.classList.remove('highlighted-neighbor', 'dimmed-by-highlight');
      });
    };

    const highlightNeighbors = (nodeId: string): void => {
      const inbound = inboundMap[nodeId] ?? [];
      const outbound = outboundMap[nodeId] ?? [];
      const neighborIds = new Set<string>();

      for (const item of inbound) {
        neighborIds.add(item.node);
      }
      for (const item of outbound) {
        neighborIds.add(item.node);
      }

      const connectedEdges = new Set<number>();
      edges.forEach((edge, idx) => {
        if (edge.from === nodeId || edge.to === nodeId) {
          connectedEdges.add(idx);
        }
      });

      getNodes().forEach((node) => {
        const id = getNodeId(node);
        if (id === nodeId) {
          node.classList.add('selected-node');
          node.classList.remove('dimmed-by-highlight');
        } else if (id != null && neighborIds.has(id)) {
          node.classList.add('highlighted-neighbor');
          const isInbound = inbound.some((item) => item.node === id);
          const isOutbound = outbound.some((item) => item.node === id);
          if (isInbound && !isOutbound) {
            node.classList.add('inbound');
          } else if (isOutbound && !isInbound) {
            node.classList.add('outbound');
          }
          node.classList.remove('dimmed-by-highlight');
        } else {
          node.classList.add('dimmed-by-highlight');
          node.classList.remove('highlighted-neighbor', 'selected-node');
        }
      });

      svgRoot.querySelectorAll('.edge').forEach((edgeEl, idx) => {
        if (connectedEdges.has(idx)) {
          edgeEl.classList.add('highlighted-neighbor');
          edgeEl.classList.remove('dimmed-by-highlight');
        } else {
          edgeEl.classList.add('dimmed-by-highlight');
          edgeEl.classList.remove('highlighted-neighbor');
        }
      });
    };

    getNodes().forEach((node) => {
      node.addEventListener('mouseenter', () => {
        if (selectedNode) {
          return;
        }
        const nodeId = getNodeId(node);
        if (nodeId) {
          highlightNeighbors(nodeId);
        }
      });

      node.addEventListener('mouseleave', () => {
        if (selectedNode) {
          return;
        }
        clearHighlights();
      });

      node.addEventListener('click', (e: Event) => {
        e.stopPropagation();
        const nodeId = getNodeId(node);

        if (nodeId == null) {
          return;
        }

        if (selectedNode === nodeId) {
          selectedNode = null;
          clearHighlights();
        } else {
          selectedNode = nodeId;
          highlightNeighbors(nodeId);
        }
      });
    });

    svgRoot.addEventListener('click', (e: Event) => {
      const target = e.target;
      if (target === svgRoot || (target instanceof Element && target.tagName === 'svg')) {
        selectedNode = null;
        clearHighlights();
      }
    });
  }
}
