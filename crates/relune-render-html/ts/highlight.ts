import { parseReluneMetadata, type EdgeMetadata, type TableMetadata } from './metadata';
import { emitViewerEvent, getViewerRuntime } from './viewer_api';

function clearChildren(element: HTMLElement): void {
  element.replaceChildren();
}

function metricCard(label: string, value: string): HTMLDivElement {
  const card = document.createElement('div');
  card.className = 'detail-metric';

  const labelEl = document.createElement('span');
  labelEl.className = 'detail-metric-label';
  labelEl.textContent = label;

  const valueEl = document.createElement('span');
  valueEl.className = 'detail-metric-value';
  valueEl.textContent = value;

  card.append(labelEl, valueEl);
  return card;
}

{
  const metadata = parseReluneMetadata();
  const tables: TableMetadata[] = metadata?.tables ?? [];
  const edges: EdgeMetadata[] = metadata?.edges ?? [];
  const tableById = new Map(tables.map((table) => [table.id, table]));

  const inboundMap: Record<string, { node: string; edge: EdgeMetadata }[]> = {};
  const outboundMap: Record<string, { node: string; edge: EdgeMetadata }[]> = {};

  for (const edge of edges) {
    (outboundMap[edge.from] ??= []).push({ node: edge.to, edge });
    (inboundMap[edge.to] ??= []).push({ node: edge.from, edge });
  }

  const canvas = document.getElementById('canvas');
  const svgRoot = canvas?.querySelector('svg');
  const drawer = document.getElementById('detail-drawer');
  const drawerTitle = document.getElementById('detail-title');
  const drawerKind = document.getElementById('detail-kind');
  const drawerSubtitle = document.getElementById('detail-subtitle');
  const drawerMetrics = document.getElementById('detail-metrics');
  const drawerColumns = document.getElementById('detail-columns');
  const drawerColumnsEmpty = document.getElementById('detail-columns-empty');
  const drawerRelations = document.getElementById('detail-relations');
  const drawerRelationsEmpty = document.getElementById('detail-relationships-empty');
  const drawerClose = document.getElementById('detail-close');
  const searchInput = document.getElementById('table-search');
  const objectBrowserList = document.getElementById('object-browser-list');
  const objectBrowserCount = document.getElementById('object-browser-count');
  const objectBrowserEmpty = document.getElementById('object-browser-empty');

  if (
    svgRoot &&
    drawer instanceof HTMLElement &&
    drawerTitle instanceof HTMLElement &&
    drawerKind instanceof HTMLElement &&
    drawerSubtitle instanceof HTMLElement &&
    drawerMetrics instanceof HTMLElement &&
    drawerColumns instanceof HTMLElement &&
    drawerColumnsEmpty instanceof HTMLElement &&
    drawerRelations instanceof HTMLElement &&
    drawerRelationsEmpty instanceof HTMLElement
  ) {
    const runtime = getViewerRuntime();
    let selectedNode: string | null = null;

    const getNodes = (): NodeListOf<Element> =>
      svgRoot.querySelectorAll('.node[data-id], .table-node[data-table-id]');

    const getNodeId = (node: Element): string | null =>
      node.getAttribute('data-id') ?? node.getAttribute('data-table-id');

    const matchesBrowserQuery = (table: TableMetadata, query: string): boolean => {
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
    };

    const centerNodeInViewport = (nodeId: string): void => {
      const node = Array.from(getNodes()).find((candidate) => getNodeId(candidate) === nodeId);
      const rect = node?.querySelector<SVGRectElement>('.table-body');
      if (rect === undefined || rect === null) {
        return;
      }

      const x = Number.parseFloat(rect.getAttribute('x') ?? '0');
      const y = Number.parseFloat(rect.getAttribute('y') ?? '0');
      const width = Number.parseFloat(rect.getAttribute('width') ?? '0');
      const height = Number.parseFloat(rect.getAttribute('height') ?? '0');
      runtime.viewport?.center(x + width / 2, y + height / 2);
    };

    const clearHighlights = (): void => {
      getNodes().forEach((node) => {
        node.classList.remove(
          'highlighted-neighbor',
          'dimmed-by-highlight',
          'selected-node',
          'inbound',
          'outbound',
        );
      });
      svgRoot.querySelectorAll('.edge').forEach((edge) => {
        edge.classList.remove('highlighted-neighbor', 'dimmed-by-highlight');
      });
    };

    const syncObjectBrowser = (): void => {
      if (
        !(objectBrowserList instanceof HTMLElement) ||
        !(objectBrowserCount instanceof HTMLElement) ||
        !(objectBrowserEmpty instanceof HTMLElement)
      ) {
        return;
      }

      clearChildren(objectBrowserList);
      const query = searchInput instanceof HTMLInputElement ? searchInput.value : '';
      const visibleTables = tables.filter((table) => matchesBrowserQuery(table, query));
      objectBrowserCount.textContent = `${visibleTables.length}/${tables.length}`;
      objectBrowserEmpty.toggleAttribute('hidden', visibleTables.length > 0);

      for (const table of visibleTables) {
        const item = document.createElement('button');
        item.type = 'button';
        item.className = 'object-browser-item';
        item.classList.toggle('selected', selectedNode === table.id);

        const node = Array.from(getNodes()).find((candidate) => getNodeId(candidate) === table.id);
        item.classList.toggle(
          'filtered-out',
          node?.classList.contains('dimmed-by-search') === true ||
            node?.classList.contains('dimmed-by-type-filter') === true,
        );
        item.classList.toggle('hidden-item', node?.classList.contains('hidden-by-group') === true);

        const header = document.createElement('div');
        header.className = 'object-browser-item-header';

        const name = document.createElement('span');
        name.className = 'object-browser-item-name';
        name.textContent = table.label || table.table_name || table.id;

        const kind = document.createElement('span');
        kind.className = 'object-browser-kind';
        kind.textContent = table.kind;

        header.append(name, kind);

        const meta = document.createElement('div');
        meta.className = 'object-browser-item-meta';

        const counts = document.createElement('span');
        counts.textContent = `${table.columns.length} cols`;

        const relations = document.createElement('span');
        relations.textContent = `${table.inbound_count} in / ${table.outbound_count} out`;

        meta.append(counts, relations);
        item.append(header, meta);

        item.addEventListener('click', () => {
          if (selectedNode === table.id) {
            runtime.selection?.clear();
            return;
          }

          runtime.selection?.select(table.id);
          centerNodeInViewport(table.id);
        });

        objectBrowserList.appendChild(item);
      }
    };

    const renderDrawer = (tableId: string | null): void => {
      if (tableId === null) {
        drawer.setAttribute('hidden', '');
        clearChildren(drawerMetrics);
        clearChildren(drawerColumns);
        clearChildren(drawerRelations);
        drawerColumnsEmpty.removeAttribute('hidden');
        drawerRelationsEmpty.removeAttribute('hidden');
        emitViewerEvent('relune:node-cleared', undefined);
        syncObjectBrowser();
        return;
      }

      const table = tableById.get(tableId);
      if (table === undefined) {
        return;
      }

      drawer.removeAttribute('hidden');
      drawerKind.textContent = table.kind;
      drawerTitle.textContent = table.label || table.table_name || table.id;
      drawerSubtitle.textContent = table.schema_name
        ? `${table.schema_name}.${table.table_name}`
        : table.table_name;

      clearChildren(drawerMetrics);
      drawerMetrics.append(
        metricCard('Columns', String(table.columns.length)),
        metricCard('Inbound', String(table.inbound_count)),
        metricCard('Outbound', String(table.outbound_count)),
      );

      clearChildren(drawerColumns);
      if (table.columns.length === 0) {
        drawerColumnsEmpty.removeAttribute('hidden');
      } else {
        drawerColumnsEmpty.setAttribute('hidden', '');
        for (const column of table.columns) {
          const columnEl = document.createElement('div');
          columnEl.className = 'detail-column';

          const name = document.createElement('span');
          name.className = 'detail-column-name';
          name.textContent = column.name;

          const meta = document.createElement('span');
          meta.className = 'detail-column-meta';
          const flags = [];
          if (column.is_primary_key) {
            flags.push('PK');
          }
          flags.push(column.nullable ? 'nullable' : 'required');
          meta.textContent = `${column.data_type || 'type unknown'} · ${flags.join(' · ')}`;

          columnEl.append(name, meta);
          drawerColumns.appendChild(columnEl);
        }
      }

      clearChildren(drawerRelations);
      const relations = [...(inboundMap[tableId] ?? []), ...(outboundMap[tableId] ?? [])];
      if (relations.length === 0) {
        drawerRelationsEmpty.removeAttribute('hidden');
      } else {
        drawerRelationsEmpty.setAttribute('hidden', '');
        for (const relation of relations) {
          const relationEl = document.createElement('div');
          relationEl.className = 'detail-relation';

          const targetTable = tableById.get(relation.node);
          const label = document.createElement('span');
          label.className = 'detail-relation-label';
          label.textContent = relation.edge.name ?? `${relation.edge.from} → ${relation.edge.to}`;

          const meta = document.createElement('span');
          meta.className = 'detail-relation-meta';
          const targetName = targetTable?.label ?? relation.node;
          const columnMap =
            relation.edge.from_columns.length > 0 && relation.edge.to_columns.length > 0
              ? ` · ${relation.edge.from_columns.join(', ')} → ${relation.edge.to_columns.join(', ')}`
              : '';
          meta.textContent = `${relation.edge.kind} · ${targetName}${columnMap}`;

          relationEl.append(label, meta);
          drawerRelations.appendChild(relationEl);
        }
      }
      emitViewerEvent('relune:node-selected', { nodeId: tableId });
      syncObjectBrowser();
    };

    const highlightNeighbors = (nodeId: string): void => {
      const inbound = inboundMap[nodeId] ?? [];
      const outbound = outboundMap[nodeId] ?? [];
      const neighborIds = new Set<string>();

      for (const relation of inbound) {
        neighborIds.add(relation.node);
      }
      for (const relation of outbound) {
        neighborIds.add(relation.node);
      }

      const connectedEdges = new Set<number>();
      edges.forEach((edge, index) => {
        if (edge.from === nodeId || edge.to === nodeId) {
          connectedEdges.add(index);
        }
      });

      getNodes().forEach((node) => {
        const id = getNodeId(node);
        if (id === nodeId) {
          node.classList.add('selected-node');
          node.classList.remove('dimmed-by-highlight');
        } else if (id !== null && neighborIds.has(id)) {
          node.classList.add('highlighted-neighbor');
          const isInbound = inbound.some((relation) => relation.node === id);
          const isOutbound = outbound.some((relation) => relation.node === id);
          node.classList.toggle('inbound', isInbound && !isOutbound);
          node.classList.toggle('outbound', isOutbound && !isInbound);
          node.classList.remove('dimmed-by-highlight');
        } else {
          node.classList.add('dimmed-by-highlight');
          node.classList.remove('highlighted-neighbor', 'selected-node', 'inbound', 'outbound');
        }
      });

      svgRoot.querySelectorAll('.edge').forEach((edgeElement, index) => {
        edgeElement.classList.toggle('highlighted-neighbor', connectedEdges.has(index));
        edgeElement.classList.toggle('dimmed-by-highlight', !connectedEdges.has(index));
      });
    };

    getNodes().forEach((node) => {
      node.addEventListener('mouseenter', () => {
        if (selectedNode !== null) {
          return;
        }
        const nodeId = getNodeId(node);
        if (nodeId !== null) {
          highlightNeighbors(nodeId);
        }
      });

      node.addEventListener('mouseleave', () => {
        if (selectedNode === null) {
          clearHighlights();
        }
      });

      node.addEventListener('click', (event: Event) => {
        event.stopPropagation();
        const nodeId = getNodeId(node);
        if (nodeId === null) {
          return;
        }

        if (selectedNode === nodeId) {
          selectedNode = null;
          clearHighlights();
          renderDrawer(null);
        } else {
          selectedNode = nodeId;
          highlightNeighbors(nodeId);
          renderDrawer(nodeId);
        }
      });
    });

    svgRoot.addEventListener('click', (event: Event) => {
      const target = event.target;
      if (
        target === svgRoot ||
        (target instanceof Element && target.tagName.toLowerCase() === 'svg')
      ) {
        selectedNode = null;
        clearHighlights();
        renderDrawer(null);
      }
    });

    drawerClose?.addEventListener('click', () => {
      selectedNode = null;
      clearHighlights();
      renderDrawer(null);
    });

    searchInput?.addEventListener('input', () => {
      syncObjectBrowser();
    });
    document.addEventListener('relune:filters-changed', syncObjectBrowser);
    document.addEventListener('relune:search-changed', syncObjectBrowser);
    document.addEventListener('relune:groups-changed', syncObjectBrowser);

    runtime.selection = {
      clear(): void {
        selectedNode = null;
        clearHighlights();
        renderDrawer(null);
      },
      select(nodeId: string): void {
        const node = Array.from(getNodes()).find((candidate) => getNodeId(candidate) === nodeId);
        if (node === undefined) {
          return;
        }
        selectedNode = nodeId;
        highlightNeighbors(nodeId);
        renderDrawer(nodeId);
      },
      getSelected(): string | null {
        return selectedNode;
      },
    };

    syncObjectBrowser();
  }
}
