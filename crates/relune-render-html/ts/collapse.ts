import { parseReluneMetadata } from './metadata';
import {
  emitViewerEvent,
  getViewerRuntime,
  getSessionStorage,
  markViewerModuleReady,
  reportSessionStorageError,
} from './viewer_api';

function setStyleCursor(el: Element, cursor: string): void {
  const styled = el as HTMLElement | SVGGraphicsElement;
  styled.style.cursor = cursor;
}

function setStyleDisplay(el: Element, display: string): void {
  const styled = el as HTMLElement | SVGGraphicsElement;
  styled.style.display = display;
}

interface TableNodeEntry {
  node: Element;
  id: string;
}

{
  const metadata = parseReluneMetadata();

  const columnCounts: Record<string, number> = {};
  if (metadata?.tables) {
    for (const table of metadata.tables) {
      columnCounts[table.id] = table.columns?.length ?? 0;
    }
  }

  const collapsedTables = new Set<string>();
  const sessionStorageRef = getSessionStorage();

  try {
    const saved = sessionStorageRef?.getItem('relune-collapsed-tables');
    if (saved) {
      const arr: unknown = JSON.parse(saved);
      if (Array.isArray(arr)) {
        for (const id of arr) {
          if (typeof id === 'string') {
            collapsedTables.add(id);
          }
        }
      }
    }
  } catch (error: unknown) {
    reportSessionStorageError('restoring collapsed tables', error);
  }

  function saveState(): void {
    if (sessionStorageRef === null) {
      return;
    }

    try {
      sessionStorageRef.setItem(
        'relune-collapsed-tables',
        JSON.stringify(Array.from(collapsedTables)),
      );
    } catch (error: unknown) {
      reportSessionStorageError('saving collapsed tables', error);
    }
  }

  const canvas = document.getElementById('canvas');
  const svg = canvas?.querySelector('svg');
  if (svg) {
    const tableNodes: TableNodeEntry[] = [];

    svg.querySelectorAll('.table-node[data-table-id]').forEach((node) => {
      const id = node.getAttribute('data-table-id');
      if (id) {
        tableNodes.push({ node, id });
      }
    });

    svg.querySelectorAll('g.node[data-id]').forEach((node) => {
      const id = node.getAttribute('data-id');
      if (id) {
        tableNodes.push({ node, id });
      }
    });

    for (const entry of tableNodes) {
      const tableNode = entry.node;
      const tableId = entry.id;
      const columnCount = columnCounts[tableId] ?? 0;

      const header = tableNode.querySelector('.table-header') ?? tableNode.querySelector('rect');

      if (!header) {
        continue;
      }

      const tableNameText =
        tableNode.querySelector('.table-name') ?? tableNode.querySelector('text');

      setStyleCursor(header, 'pointer');

      let collapseIndicator: SVGTextElement | null = null;
      let countBadge: SVGTextElement | null = null;

      if (tableNameText) {
        const headerY = parseFloat(tableNameText.getAttribute('y') ?? '') || 0;

        const tableRect = tableNode.querySelector('rect');
        const tableWidth = tableRect
          ? parseFloat(tableRect.getAttribute('width') ?? '') || 200
          : 200;

        collapseIndicator = document.createElementNS('http://www.w3.org/2000/svg', 'text');
        collapseIndicator.setAttribute('class', 'collapse-indicator');
        collapseIndicator.setAttribute('x', String(tableWidth - 20));
        collapseIndicator.setAttribute('y', String(headerY));
        collapseIndicator.setAttribute('text-anchor', 'middle');
        collapseIndicator.setAttribute('fill', '#64748b');
        collapseIndicator.textContent = '-';
        tableNode.appendChild(collapseIndicator);

        if (columnCount > 0) {
          countBadge = document.createElementNS('http://www.w3.org/2000/svg', 'text');
          countBadge.setAttribute('class', 'column-count-badge');
          countBadge.setAttribute('x', String(tableWidth - 40));
          countBadge.setAttribute('y', String(headerY));
          countBadge.setAttribute('text-anchor', 'end');
          countBadge.setAttribute('fill', '#64748b');
          countBadge.textContent = `(${columnCount})`;
          countBadge.style.display = 'none';
          tableNode.appendChild(countBadge);
        }
      }

      let columnRows: NodeListOf<Element> = tableNode.querySelectorAll('.column-row, .column-name');

      if (columnRows.length === 0 && tableNameText) {
        tableNode.querySelectorAll('text').forEach((text) => {
          if (text === tableNameText) {
            return;
          }
          if (
            text.classList.contains('collapse-indicator') ||
            text.classList.contains('column-count-badge')
          ) {
            return;
          }
          text.classList.add('column-text');
        });
        columnRows = tableNode.querySelectorAll('.column-text');
      }

      if (collapsedTables.has(tableId)) {
        tableNode.classList.add('collapsed');
        Array.from(columnRows).forEach((row) => {
          setStyleDisplay(row, 'none');
        });
        if (collapseIndicator) {
          collapseIndicator.textContent = '+';
        }
        if (countBadge) {
          countBadge.style.display = '';
        }
      }

      header.addEventListener('click', (e: Event) => {
        e.stopPropagation();
        const isCollapsed = tableNode.classList.toggle('collapsed');

        if (isCollapsed) {
          collapsedTables.add(tableId);
          Array.from(columnRows).forEach((row) => {
            setStyleDisplay(row, 'none');
          });
          if (collapseIndicator) {
            collapseIndicator.textContent = '+';
          }
          if (countBadge) {
            countBadge.style.display = '';
          }
        } else {
          collapsedTables.delete(tableId);
          Array.from(columnRows).forEach((row) => {
            setStyleDisplay(row, '');
          });
          if (collapseIndicator) {
            collapseIndicator.textContent = '-';
          }
          if (countBadge) {
            countBadge.style.display = 'none';
          }
        }

        saveState();
        emitViewerEvent('relune:collapse-changed', undefined);
      });
    }

    // ── Runtime API ────────────────────────────────────────────────────

    const tableNodeMap = new Map(tableNodes.map((entry) => [entry.id, entry]));

    function applyCollapseState(tableId: string, collapse: boolean): void {
      const entry = tableNodeMap.get(tableId);
      if (entry === undefined) return;
      const tableNode = entry.node;
      const isCurrentlyCollapsed = tableNode.classList.contains('collapsed');
      if (isCurrentlyCollapsed === collapse) return;

      const collapseInd = tableNode.querySelector('.collapse-indicator') as SVGTextElement | null;
      const badge = tableNode.querySelector('.column-count-badge') as SVGTextElement | null;
      const rows = tableNode.querySelectorAll('.column-row, .column-name, .column-text');

      if (collapse) {
        tableNode.classList.add('collapsed');
        collapsedTables.add(tableId);
        rows.forEach((row) => setStyleDisplay(row, 'none'));
        if (collapseInd) collapseInd.textContent = '+';
        if (badge) badge.style.display = '';
      } else {
        tableNode.classList.remove('collapsed');
        collapsedTables.delete(tableId);
        rows.forEach((row) => setStyleDisplay(row, ''));
        if (collapseInd) collapseInd.textContent = '-';
        if (badge) badge.style.display = 'none';
      }
    }

    const runtime = getViewerRuntime();
    runtime.collapse = {
      getCollapsed(): string[] {
        return Array.from(collapsedTables);
      },
      setCollapsed(tableIds: string[]): void {
        const target = new Set(tableIds);
        // Expand tables that should no longer be collapsed
        for (const id of collapsedTables) {
          if (!target.has(id)) {
            applyCollapseState(id, false);
          }
        }
        // Collapse tables that should be collapsed
        for (const id of target) {
          applyCollapseState(id, true);
        }
        saveState();
      },
    };
    markViewerModuleReady('collapse');
  }
}
