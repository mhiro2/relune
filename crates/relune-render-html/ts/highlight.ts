import { parseReluneMetadata, type TableMetadata } from './metadata';
import { emitViewerEvent, getViewerRuntime, markViewerModuleReady } from './viewer_api';
import { createHighlightState } from './highlight_state';
import { computeNeighborHighlights, matchesBrowserQuery } from './highlight_actions';
import {
  clearHighlightClasses,
  applyHighlightClasses,
  renderDrawer,
  renderObjectBrowser,
  type DrawerElements,
  type ObjectBrowserItem,
} from './highlight_dom';

{
  const metadata = parseReluneMetadata();
  const tables: TableMetadata[] = metadata?.tables ?? [];
  const state = createHighlightState(tables, metadata?.edges ?? []);

  const canvas = document.getElementById('canvas');
  const svgRoot = canvas?.querySelector('svg');
  const searchInput = document.getElementById('table-search');
  const objectBrowserList = document.getElementById('object-browser-list');
  const objectBrowserCount = document.getElementById('object-browser-count');
  const objectBrowserEmpty = document.getElementById('object-browser-empty');
  const drawerClose = document.getElementById('detail-close');

  const drawerEls: DrawerElements | null = (() => {
    const drawer = document.getElementById('detail-drawer');
    const title = document.getElementById('detail-title');
    const kind = document.getElementById('detail-kind');
    const subtitle = document.getElementById('detail-subtitle');
    const metrics = document.getElementById('detail-metrics');
    const columns = document.getElementById('detail-columns');
    const columnsEmpty = document.getElementById('detail-columns-empty');
    const relations = document.getElementById('detail-relations');
    const relationsEmpty = document.getElementById('detail-relationships-empty');
    if (
      drawer instanceof HTMLElement &&
      title instanceof HTMLElement &&
      kind instanceof HTMLElement &&
      subtitle instanceof HTMLElement &&
      metrics instanceof HTMLElement &&
      columns instanceof HTMLElement &&
      columnsEmpty instanceof HTMLElement &&
      relations instanceof HTMLElement &&
      relationsEmpty instanceof HTMLElement
    ) {
      return {
        drawer,
        title,
        kind,
        subtitle,
        metrics,
        columns,
        columnsEmpty,
        relations,
        relationsEmpty,
        issues: document.getElementById('detail-issues'),
        issuesEmpty: document.getElementById('detail-issues-empty'),
      };
    }
    return null;
  })();

  if (svgRoot && drawerEls) {
    const runtime = getViewerRuntime();

    const getNodes = (): NodeListOf<Element> =>
      svgRoot.querySelectorAll('.node[data-id], .table-node[data-table-id]');

    const getNodeId = (node: Element): string | null =>
      node.getAttribute('data-id') ?? node.getAttribute('data-table-id');

    const centerNodeInViewport = (nodeId: string): void => {
      const node = Array.from(getNodes()).find((c) => getNodeId(c) === nodeId);
      const rect = node?.querySelector<SVGRectElement>('.table-body');
      if (rect === undefined || rect === null) return;
      const x = Number.parseFloat(rect.getAttribute('x') ?? '0');
      const y = Number.parseFloat(rect.getAttribute('y') ?? '0');
      const width = Number.parseFloat(rect.getAttribute('width') ?? '0');
      const height = Number.parseFloat(rect.getAttribute('height') ?? '0');
      runtime.viewport?.center(x + width / 2, y + height / 2);
    };

    // ── Object browser sync ───────────────────────────────────────────────

    const syncObjectBrowser = (): void => {
      if (
        !(objectBrowserList instanceof HTMLElement) ||
        !(objectBrowserCount instanceof HTMLElement) ||
        !(objectBrowserEmpty instanceof HTMLElement)
      ) {
        return;
      }

      const query = searchInput instanceof HTMLInputElement ? searchInput.value : '';
      const visibleTables = tables.filter((table) => matchesBrowserQuery(table, query));

      const items: ObjectBrowserItem[] = visibleTables.map((table) => {
        const node = Array.from(getNodes()).find((c) => getNodeId(c) === table.id);
        return {
          table,
          isSelected: state.selectedNode === table.id,
          isDimmedBySearch: node?.classList.contains('dimmed-by-search') === true,
          isDimmedByTypeFilter: node?.classList.contains('dimmed-by-type-filter') === true,
          isHiddenByGroup: node?.classList.contains('hidden-by-group') === true,
        };
      });

      renderObjectBrowser(
        items,
        tables.length,
        objectBrowserList,
        objectBrowserCount,
        objectBrowserEmpty,
        (tableId) => {
          if (state.selectedNode === tableId) {
            runtime.selection?.clear();
          } else {
            runtime.selection?.select(tableId);
            centerNodeInViewport(tableId);
          }
        },
      );
    };

    // ── Selection / highlight orchestration ────────────────────────────────

    const applySelection = (tableId: string | null): void => {
      if (tableId === null) {
        clearHighlightClasses(svgRoot, getNodes);
        renderDrawer(undefined, state, drawerEls);
        emitViewerEvent('relune:node-cleared', undefined);
      } else {
        const highlight = computeNeighborHighlights(tableId, state);
        applyHighlightClasses(svgRoot, getNodes, getNodeId, highlight);
        renderDrawer(state.tableById.get(tableId), state, drawerEls);
        emitViewerEvent('relune:node-selected', { nodeId: tableId });
      }
      syncObjectBrowser();
    };

    // ── Node event listeners ──────────────────────────────────────────────

    getNodes().forEach((node) => {
      node.addEventListener('mouseenter', () => {
        if (state.selectedNode !== null) return;
        const nodeId = getNodeId(node);
        if (nodeId !== null) {
          const highlight = computeNeighborHighlights(nodeId, state);
          applyHighlightClasses(svgRoot, getNodes, getNodeId, highlight);
        }
      });

      node.addEventListener('mouseleave', () => {
        if (state.selectedNode === null) {
          clearHighlightClasses(svgRoot, getNodes);
        }
      });

      node.addEventListener('click', (event: Event) => {
        event.stopPropagation();
        const nodeId = getNodeId(node);
        if (nodeId === null) return;

        if (state.selectedNode === nodeId) {
          state.selectedNode = null;
          applySelection(null);
        } else {
          state.selectedNode = nodeId;
          applySelection(nodeId);
        }
      });
    });

    svgRoot.addEventListener('click', () => {
      if (state.selectedNode !== null) {
        state.selectedNode = null;
        applySelection(null);
      }
    });

    drawerClose?.addEventListener('click', () => {
      state.selectedNode = null;
      applySelection(null);
    });

    searchInput?.addEventListener('input', () => syncObjectBrowser());
    document.addEventListener('relune:filters-changed', syncObjectBrowser);
    document.addEventListener('relune:search-changed', syncObjectBrowser);
    document.addEventListener('relune:groups-changed', syncObjectBrowser);

    // ── Runtime API ───────────────────────────────────────────────────────

    runtime.selection = {
      clear(): void {
        state.selectedNode = null;
        applySelection(null);
      },
      select(nodeId: string): void {
        const node = Array.from(getNodes()).find((c) => getNodeId(c) === nodeId);
        if (node === undefined) return;
        state.selectedNode = nodeId;
        applySelection(nodeId);
      },
      getSelected(): string | null {
        return state.selectedNode;
      },
    };
    markViewerModuleReady('selection');

    syncObjectBrowser();
  }
}
