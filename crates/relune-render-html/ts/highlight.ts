import {
  computeHoverPreview,
  computeNeighborHighlights,
  computeRelationHighlight,
  nextLineIndex,
  relationColumnPairs,
  relationKey,
} from './highlight_actions';
import {
  createHighlightPainter,
  createObjectBrowser,
  hideHoverPopover,
  renderDrawer,
  renderHoverPopover,
  renderRelationCard,
  type DrawerElements,
  type RelationCardElements,
  type ObjectBrowserItem,
  type HoverPopoverElements,
  type PopoverPosition,
} from './highlight_dom';
import { createHighlightState } from './highlight_state';
import { parseReluneMetadata, type EdgeMetadata, type TableMetadata } from './metadata';
import { matchesTableQuery } from './search_actions';
import { emitViewerEvent, getViewerRuntime, markViewerModuleReady } from './viewer_api';

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
  const traversalEl = document.getElementById('detail-traversal');
  const viewport = document.getElementById('viewport');

  const drawerEls: DrawerElements | null = (() => {
    const drawer = document.getElementById('detail-drawer');
    const title = document.getElementById('detail-title');
    const titleBadges = document.getElementById('detail-title-badges');
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
      titleBadges instanceof HTMLElement &&
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
        titleBadges,
        kind,
        subtitle,
        metrics,
        columns,
        columnsEmpty,
        relations,
        relationsEmpty,
        changesSection: document.getElementById('detail-changes-section'),
        changes: document.getElementById('detail-changes'),
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

  const relationEls: RelationCardElements | null = (() => {
    const card = document.getElementById('relation-card');
    const kind = document.getElementById('relation-card-kind');
    const title = document.getElementById('relation-card-title');
    const name = document.getElementById('relation-card-name');
    const pairs = document.getElementById('relation-card-pairs');
    const openFrom = document.getElementById('relation-card-open-from');
    const openTo = document.getElementById('relation-card-open-to');
    if (
      card instanceof HTMLElement &&
      kind instanceof HTMLElement &&
      title instanceof HTMLElement &&
      name instanceof HTMLElement &&
      pairs instanceof HTMLElement &&
      openFrom instanceof HTMLButtonElement &&
      openTo instanceof HTMLButtonElement
    ) {
      return { card, kind, title, name, pairs, openFrom, openTo };
    }
    return null;
  })();

  if (svgRoot && drawerEls && hoverEls) {
    const runtime = getViewerRuntime();

    // Index the diagram once; hover and selection then touch only the
    // elements they change instead of re-querying the whole SVG.
    const nodesById = new Map<string, Element>();
    svgRoot.querySelectorAll('.node[data-id], .table-node[data-table-id]').forEach((node) => {
      const id = node.getAttribute('data-id') ?? node.getAttribute('data-table-id');
      if (id !== null && !nodesById.has(id)) nodesById.set(id, node);
    });
    const edgeEls = Array.from(svgRoot.querySelectorAll('.edge'));
    const painter = createHighlightPainter({ nodesById, edges: edgeEls });

    const findNode = (nodeId: string): Element | undefined => nodesById.get(nodeId);

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

    const navigateToTable = (tableId: string): void => {
      setSelectedNode(tableId);
      centerNodeInViewport(tableId);
    };

    // ── Object browser sync ───────────────────────────────────────────────

    const objectBrowser =
      objectBrowserList instanceof HTMLElement &&
      objectBrowserCount instanceof HTMLElement &&
      objectBrowserEmpty instanceof HTMLElement
        ? createObjectBrowser(
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
          )
        : null;

    const syncObjectBrowser = (): void => {
      if (objectBrowser === null) {
        return;
      }

      const query = searchInput instanceof HTMLInputElement ? searchInput.value : '';
      const visibleTables = tables.filter((table) => matchesTableQuery(table, query));

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

      objectBrowser.render(items, tables.length);
    };

    // ── Traversal depth toggle ─────────────────────────────────────────

    const syncTraversalButtons = (): void => {
      traversalEl?.querySelectorAll<HTMLButtonElement>('.detail-traversal-btn').forEach((btn) => {
        const depth = Number(btn.dataset['depth']);
        btn.classList.toggle('active', depth === state.traversalDepth);
      });
    };

    traversalEl?.addEventListener('click', (event: Event) => {
      const target = event.target;
      if (!(target instanceof HTMLButtonElement) || target.dataset['depth'] === undefined) return;
      const depth = Number(target.dataset['depth']);
      if (depth >= 1 && depth <= 2 && depth !== state.traversalDepth) {
        state.traversalDepth = depth;
        renderHighlight();
      }
    });

    // ── Selection / highlight orchestration ────────────────────────────────

    /** Repaints highlight classes, the drawer, and the hover popover. */
    const renderHighlight = (): void => {
      painter.clear();
      hideHoverPopover(hoverEls);
      const relation =
        state.selectedEdge === null ? null : computeRelationHighlight(state.selectedEdge, state);
      if (relationEls !== null) {
        renderRelationCard(
          relation === null ? undefined : state.edges[relation.edgeIndex],
          state.tableById,
          relationEls,
        );
      }

      if (relation !== null) {
        painter.applyRelation(relation);
        renderDrawer(undefined, state, drawerEls);
        traversalEl?.setAttribute('hidden', '');
      } else if (state.selectedNode !== null) {
        const highlight = computeNeighborHighlights(
          state.selectedNode,
          state,
          state.traversalDepth,
        );
        painter.applySelected(highlight);
        renderDrawer(state.tableById.get(state.selectedNode), state, drawerEls, (edge) => {
          selectRelation(edge, true);
        });
        traversalEl?.removeAttribute('hidden');
        syncTraversalButtons();
      } else {
        renderDrawer(undefined, state, drawerEls);
        traversalEl?.setAttribute('hidden', '');
        if (state.hoveredNode !== null) {
          const hoveredNode = findNode(state.hoveredNode);
          if (hoveredNode !== undefined) {
            const preview = computeHoverPreview(state.hoveredNode, state);
            painter.applyHoverPreview(preview);
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
    };

    /** Full refresh for changes that also affect the object browser. */
    const renderInteraction = (): void => {
      renderHighlight();
      syncObjectBrowser();
    };

    const selectedRelationKey = (): string | null => {
      const edge = state.selectedEdge === null ? undefined : state.edges[state.selectedEdge];
      return edge === undefined ? null : relationKey(edge);
    };

    const setSelectedNode = (tableId: string | null): void => {
      const previous = state.selectedNode;
      const hadRelation = state.selectedEdge !== null;
      state.selectedNode = tableId;
      state.selectedEdge = null;
      state.hoveredNode = null;
      renderInteraction();
      if (hadRelation) emitViewerEvent('relune:relation-cleared', undefined);

      if (previous === tableId) {
        return;
      }

      if (tableId === null) {
        emitViewerEvent('relune:node-cleared', undefined);
      } else {
        emitViewerEvent('relune:node-selected', { nodeId: tableId });
      }
    };

    // Table whose drawer a relationship was opened from, so closing the
    // relation card returns there instead of dropping keyboard focus.
    let relationOrigin: string | null = null;

    /**
     * Selects one relationship, clearing any table selection. Keyboard paths
     * move focus to the relation card, since the drawer they came from closes.
     */
    const setSelectedEdge = (edgeIndex: number | null, focusCard = false): void => {
      const hadTable = state.selectedNode !== null;
      const previous = state.selectedEdge;
      state.selectedNode = null;
      state.selectedEdge = edgeIndex;
      state.hoveredNode = null;
      relationOrigin = null;
      renderInteraction();
      if (hadTable) emitViewerEvent('relune:node-cleared', undefined);
      // A relationship restored behind a hidden group keeps the Tab stop on
      // a line that is still on the diagram.
      if (edgeIndex !== null && isLineFocusable(edgeIndex)) setRovingLine(edgeIndex);
      if (edgeIndex !== previous) {
        const key = selectedRelationKey();
        if (key === null) {
          emitViewerEvent('relune:relation-cleared', undefined);
        } else {
          emitViewerEvent('relune:relation-selected', { key });
        }
      }
      if (focusCard && edgeIndex !== null) relationEls?.card.focus();
    };

    const selectRelation = (edge: EdgeMetadata, focusCard = false): void => {
      const index = state.edges.indexOf(edge);
      if (index < 0) return;
      const origin = state.selectedNode;
      setSelectedEdge(index, focusCard);
      relationOrigin = origin;
    };

    /** Selects a table and moves focus into its drawer. */
    const openTableDrawer = (tableId: string): void => {
      navigateToTable(tableId);
      drawerEls.drawer.focus();
    };

    const clearHoverPreview = (): void => {
      if (state.selectedNode !== null || state.hoveredNode === null) {
        return;
      }
      state.hoveredNode = null;
      renderHighlight();
    };

    // ── Node event listeners ──────────────────────────────────────────────

    // Hovering only changes highlight classes and the popover, so it skips
    // rebuilding the object browser.
    nodesById.forEach((node, nodeId) => {
      node.addEventListener('mouseenter', () => {
        if (state.selectedNode !== null) return;
        state.hoveredNode = nodeId;
        renderHighlight();
      });

      node.addEventListener('mouseleave', () => {
        if (state.selectedNode === null && state.hoveredNode === nodeId) {
          state.hoveredNode = null;
          renderHighlight();
        }
      });

      node.addEventListener('click', (event: Event) => {
        event.stopPropagation();

        if (state.selectedNode === nodeId) {
          setSelectedNode(null);
        } else {
          setSelectedNode(nodeId);
        }
      });
    });

    // ── Edge click listeners ────────────────────────────────────────────

    // Edges render in metadata order, so a DOM index is a metadata index.
    // Each line is also a keyboard button naming the mapping it selects. The
    // lines share one Tab stop and arrow keys move between them, so Tab does
    // not have to walk every line of a large diagram.
    const lineHint = document.createElement('p');
    lineHint.id = 'relation-line-hint';
    lineHint.className = 'visually-hidden';
    lineHint.textContent = 'Use the arrow keys to move between relationships.';
    document.body.appendChild(lineHint);

    const isLineFocusable = (index: number): boolean => {
      const line = edgeEls[index];
      return (
        line !== undefined &&
        !line.classList.contains('hidden-by-group') &&
        !line.classList.contains('hidden-by-filter')
      );
    };
    let rovingLine = -1;
    const setRovingLine = (index: number): void => {
      if (index === rovingLine) return;
      edgeEls[rovingLine]?.setAttribute('tabindex', '-1');
      rovingLine = index;
      edgeEls[index]?.setAttribute('tabindex', '0');
    };
    /** Keeps the Tab stop on a line that is still on the diagram. */
    const syncRovingLine = (): void => {
      if (isLineFocusable(rovingLine)) return;
      const first = nextLineIndex(edgeEls.length, -1, 'Home', isLineFocusable);
      if (first !== null) setRovingLine(first);
    };

    edgeEls.forEach((edgeEl, index) => {
      const toggle = (focusCard: boolean): void => {
        setSelectedEdge(state.selectedEdge === index ? null : index, focusCard);
      };
      const edge = state.edges[index];
      if (edge !== undefined) {
        edgeEl.setAttribute('tabindex', '-1');
        edgeEl.setAttribute('role', 'button');
        edgeEl.setAttribute('aria-label', `Relationship ${relationColumnPairs(edge).join(', ')}`);
        edgeEl.setAttribute('aria-describedby', lineHint.id);
      }
      edgeEl.addEventListener('focus', () => {
        setRovingLine(index);
      });
      edgeEl.addEventListener('click', (event: Event) => {
        event.stopPropagation();
        toggle(false);
      });
      edgeEl.addEventListener('keydown', (event: Event) => {
        const { key } = event as KeyboardEvent;
        if (key === 'Enter' || key === ' ') {
          event.preventDefault();
          event.stopPropagation();
          toggle(true);
          return;
        }
        const next = nextLineIndex(edgeEls.length, index, key, isLineFocusable);
        if (next === null) return;
        event.preventDefault();
        event.stopPropagation();
        setRovingLine(next);
        (edgeEls[next] as HTMLElement | SVGElement | undefined)?.focus();
      });
    });
    syncRovingLine();

    svgRoot.addEventListener('click', () => {
      if (state.selectedNode !== null || state.selectedEdge !== null) {
        setSelectedNode(null);
      }
    });

    const openRelationEnd = (end: 'from' | 'to'): void => {
      const edge = state.selectedEdge === null ? undefined : state.edges[state.selectedEdge];
      if (edge !== undefined) openTableDrawer(edge[end]);
    };
    relationEls?.openFrom.addEventListener('click', () => {
      openRelationEnd('from');
    });
    relationEls?.openTo.addEventListener('click', () => {
      openRelationEnd('to');
    });
    /** Closes the relation card, returning to the drawer or line it came from. */
    const closeRelationCard = (): void => {
      const index = state.selectedEdge;
      if (relationOrigin !== null) {
        openTableDrawer(relationOrigin);
        return;
      }
      setSelectedEdge(null);
      if (index !== null) (edgeEls[index] as HTMLElement | SVGElement | undefined)?.focus();
    };
    document.getElementById('relation-card-close')?.addEventListener('click', closeRelationCard);
    // Escape inside the card steps back instead of clearing the whole view.
    relationEls?.card.addEventListener('keydown', (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopPropagation();
      closeRelationCard();
    });

    drawerClose?.addEventListener('click', () => {
      setSelectedNode(null);
    });

    const handleVisibilityStateChange = (): void => {
      syncRovingLine();
      if (state.selectedNode === null && state.hoveredNode !== null) {
        state.hoveredNode = null;
        renderInteraction();
        return;
      }
      syncObjectBrowser();
    };

    // Search reports through the debounced `relune:search-changed` event
    // rather than raw input, so typing does not rebuild the browser per key.
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
      selectRelation(key: string): boolean {
        const index = state.edges.findIndex((edge) => relationKey(edge) === key);
        if (index < 0) return false;
        setSelectedEdge(index);
        return true;
      },
      getSelectedRelation: selectedRelationKey,
    };
    markViewerModuleReady('selection');

    renderInteraction();
  }
}
