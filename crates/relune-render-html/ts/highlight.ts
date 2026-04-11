import { parseReluneMetadata, type TableMetadata } from './metadata';
import { emitViewerEvent, getViewerRuntime, markViewerModuleReady } from './viewer_api';
import { createHighlightState } from './highlight_state';
import {
  computeHoverPreview,
  computeNeighborHighlights,
  matchesBrowserQuery,
} from './highlight_actions';
import {
  applyHoverPreviewClasses,
  applySelectedHighlightClasses,
  clearHighlightClasses,
  hideHoverPopover,
  renderDrawer,
  renderHoverPopover,
  renderObjectBrowser,
  type DrawerElements,
  type ObjectBrowserItem,
  type HoverPopoverElements,
  type PopoverPosition,
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
  const viewport = document.getElementById('viewport');

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

  const hoverEls: HoverPopoverElements | null = (() => {
    const popover = document.getElementById('hover-popover');
    const kind = document.getElementById('hover-popover-kind');
    const title = document.getElementById('hover-popover-title');
    const subtitle = document.getElementById('hover-popover-subtitle');
    const metrics = document.getElementById('hover-popover-metrics');
    const badges = document.getElementById('hover-popover-badges');
    if (
      popover instanceof HTMLElement &&
      kind instanceof HTMLElement &&
      title instanceof HTMLElement &&
      subtitle instanceof HTMLElement &&
      metrics instanceof HTMLElement &&
      badges instanceof HTMLElement
    ) {
      return { popover, kind, title, subtitle, metrics, badges };
    }
    return null;
  })();

  if (svgRoot && drawerEls && hoverEls) {
    const runtime = getViewerRuntime();

    const getNodes = (): NodeListOf<Element> =>
      svgRoot.querySelectorAll('.node[data-id], .table-node[data-table-id]');

    const getNodeId = (node: Element): string | null =>
      node.getAttribute('data-id') ?? node.getAttribute('data-table-id');

    const findNode = (nodeId: string): Element | undefined =>
      Array.from(getNodes()).find((candidate) => getNodeId(candidate) === nodeId);

    const hoverPopoverPosition = (node: Element): PopoverPosition => {
      const anchor = node.querySelector('.table-body') ?? node;
      const rect = anchor.getBoundingClientRect();
      const viewportRect = viewport?.getBoundingClientRect();
      const top = Math.max(rect.top - 8, (viewportRect?.top ?? 0) + 12);
      return {
        left: rect.right + 14,
        top,
      };
    };

    const centerNodeInViewport = (nodeId: string): void => {
      const node = findNode(nodeId);
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

      const filterMode = runtime.filters?.getMode() ?? 'dim';
      const isHideOrFocus = filterMode === 'hide' || filterMode === 'focus';

      const items: ObjectBrowserItem[] = visibleTables
        .filter((table) => {
          if (!isHideOrFocus) return true;
          const node = findNode(table.id);
          return node?.classList.contains('hidden-by-filter') !== true;
        })
        .map((table) => {
          const node = findNode(table.id);
          return {
            table,
            isSelected: state.selectedNode === table.id,
            isDimmedBySearch: node?.classList.contains('dimmed-by-search') === true,
            isExcludedByFilter: node?.classList.contains('dimmed-by-filter') === true,
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

    const renderInteraction = (): void => {
      clearHighlightClasses(svgRoot, getNodes);
      hideHoverPopover(hoverEls);

      if (state.selectedNode !== null) {
        const highlight = computeNeighborHighlights(state.selectedNode, state);
        applySelectedHighlightClasses(svgRoot, getNodes, getNodeId, highlight);
        renderDrawer(state.tableById.get(state.selectedNode), state, drawerEls);
      } else {
        renderDrawer(undefined, state, drawerEls);
        if (state.hoveredNode !== null) {
          const hoveredNode = findNode(state.hoveredNode);
          if (hoveredNode !== undefined) {
            const preview = computeHoverPreview(state.hoveredNode, state);
            applyHoverPreviewClasses(svgRoot, getNodes, getNodeId, preview);
            renderHoverPopover(
              state.tableById.get(state.hoveredNode),
              hoverEls,
              hoverPopoverPosition(hoveredNode),
            );
          } else {
            state.hoveredNode = null;
          }
        }
      }
      syncObjectBrowser();
    };

    const setSelectedNode = (tableId: string | null): void => {
      const previous = state.selectedNode;
      state.selectedNode = tableId;
      state.hoveredNode = null;
      renderInteraction();

      if (previous === tableId) {
        return;
      }

      if (tableId === null) {
        emitViewerEvent('relune:node-cleared', undefined);
      } else {
        emitViewerEvent('relune:node-selected', { nodeId: tableId });
      }
    };

    const clearHoverPreview = (): void => {
      if (state.selectedNode !== null || state.hoveredNode === null) {
        return;
      }
      state.hoveredNode = null;
      renderInteraction();
    };

    // ── Node event listeners ──────────────────────────────────────────────

    getNodes().forEach((node) => {
      const nodeId = getNodeId(node);

      node.addEventListener('mouseenter', () => {
        if (state.selectedNode !== null) return;
        if (nodeId !== null) {
          state.hoveredNode = nodeId;
          renderInteraction();
        }
      });

      node.addEventListener('mouseleave', () => {
        if (state.selectedNode === null && state.hoveredNode === nodeId) {
          state.hoveredNode = null;
          renderInteraction();
        }
      });

      node.addEventListener('click', (event: Event) => {
        event.stopPropagation();
        if (nodeId === null) return;

        if (state.selectedNode === nodeId) {
          setSelectedNode(null);
        } else {
          setSelectedNode(nodeId);
        }
      });
    });

    svgRoot.addEventListener('click', () => {
      if (state.selectedNode !== null) {
        setSelectedNode(null);
      }
    });

    drawerClose?.addEventListener('click', () => {
      setSelectedNode(null);
    });

    const handleVisibilityStateChange = (): void => {
      if (state.selectedNode === null && state.hoveredNode !== null) {
        state.hoveredNode = null;
        renderInteraction();
        return;
      }
      syncObjectBrowser();
    };

    searchInput?.addEventListener('input', handleVisibilityStateChange);
    document.addEventListener('relune:filters-changed', handleVisibilityStateChange);
    document.addEventListener('relune:search-changed', handleVisibilityStateChange);
    document.addEventListener('relune:groups-changed', handleVisibilityStateChange);
    document.addEventListener('relune:viewport-changed', clearHoverPreview);

    // ── Runtime API ───────────────────────────────────────────────────────

    runtime.selection = {
      clear(): void {
        setSelectedNode(null);
      },
      select(nodeId: string): void {
        const node = findNode(nodeId);
        if (node === undefined) return;
        setSelectedNode(nodeId);
      },
      getSelected(): string | null {
        return state.selectedNode;
      },
    };
    markViewerModuleReady('selection');

    renderInteraction();
  }
}
