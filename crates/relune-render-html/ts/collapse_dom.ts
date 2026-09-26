const SVG_NS = 'http://www.w3.org/2000/svg';

// Elements that make up the table header; clicking any of them toggles the table.
const HEADER_SELECTOR =
  '.table-header, .table-header-fade, .table-name, .table-kind, .collapse-indicator';

// The SVG renderer ends the table-name clip 44px before the node's right edge,
// leaving that space for the right-aligned kind label.
const KIND_LABEL_RESERVE = 44;
const INDICATOR_WIDTH = 16;
const INDICATOR_GAP = 4;
const MIN_NAME_CLIP_WIDTH = 24;
// Baseline of the first column row, relative to the node top.
const FIRST_ROW_BASELINE = 46;

const EXPANDED_GLYPH = '▾';
const COLLAPSED_GLYPH = '▸';

interface CollapsibleTable {
  node: Element;
  indicator: SVGTextElement;
}

export interface CollapseControllerOptions {
  columnCounts: ReadonlyMap<string, number>;
  initiallyCollapsed: Iterable<string>;
  /** Called after the user toggles a table from the diagram. */
  onToggle: () => void;
}

export interface CollapseController {
  getCollapsed(): string[];
  setCollapsed(tableIds: Iterable<string>): void;
}

function numericAttribute(el: Element, name: string): number {
  return Number.parseFloat(el.getAttribute(name) ?? '') || 0;
}

function createSvgText(className: string, x: number, y: number, text: string): SVGTextElement {
  const el = document.createElementNS(SVG_NS, 'text');
  el.setAttribute('class', className);
  el.setAttribute('x', String(x));
  el.setAttribute('y', String(y));
  el.textContent = text;
  return el;
}

/** Narrows the table-name clip so the name never runs under the indicator. */
function shrinkNameClip(node: Element): void {
  const clipRef = node.querySelector('.table-name')?.getAttribute('clip-path') ?? '';
  const clipId = /^url\(#(.+)\)$/.exec(clipRef)?.[1];
  if (clipId === undefined) return;
  const clipRect = node.querySelector(`clipPath[id="${CSS.escape(clipId)}"] rect`);
  if (clipRect === null) return;
  const width = numericAttribute(clipRect, 'width');
  clipRect.setAttribute(
    'width',
    String(Math.max(width - INDICATOR_WIDTH - INDICATOR_GAP * 2, MIN_NAME_CLIP_WIDTH)),
  );
}

function decorateTable(node: Element, header: Element, columnCount: number): SVGTextElement {
  const x = numericAttribute(header, 'x');
  const y = numericAttribute(header, 'y');
  const width = numericAttribute(header, 'width');
  const headerHeight = numericAttribute(header, 'height');

  const indicator = createSvgText(
    'collapse-indicator',
    x + width - KIND_LABEL_RESERVE - INDICATOR_GAP - INDICATOR_WIDTH / 2,
    y + headerHeight / 2,
    EXPANDED_GLYPH,
  );
  indicator.setAttribute('text-anchor', 'middle');
  indicator.setAttribute('dominant-baseline', 'central');
  node.appendChild(indicator);
  shrinkNameClip(node);

  if (columnCount > 0) {
    const label = `${columnCount} ${columnCount === 1 ? 'column' : 'columns'} hidden`;
    node.appendChild(createSvgText('column-count-badge', x + 10, y + FIRST_ROW_BASELINE, label));
  }

  return indicator;
}

/**
 * Wires collapse/expand behaviour onto every table node in the rendered SVG.
 *
 * Visibility of column rows and the hidden-column badge is driven by the
 * `.collapsed` class so the stylesheet stays the single source of truth.
 */
export function createCollapseController(
  svg: Element,
  options: CollapseControllerOptions,
): CollapseController {
  const tables = new Map<string, CollapsibleTable>();
  const collapsed = new Set<string>();

  const apply = (tableId: string, collapse: boolean): void => {
    const table = tables.get(tableId);
    if (table === undefined) return;
    table.node.classList.toggle('collapsed', collapse);
    table.indicator.textContent = collapse ? COLLAPSED_GLYPH : EXPANDED_GLYPH;
    if (collapse) {
      collapsed.add(tableId);
    } else {
      collapsed.delete(tableId);
    }
  };

  svg.querySelectorAll('.table-node[data-table-id]').forEach((node) => {
    const tableId = node.getAttribute('data-table-id');
    const header = node.querySelector('.table-header');
    if (tableId === null || header === null || tables.has(tableId)) return;

    const indicator = decorateTable(node, header, options.columnCounts.get(tableId) ?? 0);
    tables.set(tableId, { node, indicator });

    const onClick = (event: Event): void => {
      // Keep header clicks from also selecting the table.
      event.stopPropagation();
      apply(tableId, !collapsed.has(tableId));
      options.onToggle();
    };
    node.querySelectorAll(HEADER_SELECTOR).forEach((el) => {
      el.addEventListener('click', onClick);
    });
  });

  for (const tableId of options.initiallyCollapsed) {
    apply(tableId, true);
  }

  return {
    getCollapsed(): string[] {
      return Array.from(collapsed);
    },
    setCollapsed(tableIds: Iterable<string>): void {
      const target = new Set(tableIds);
      for (const tableId of Array.from(collapsed)) {
        if (!target.has(tableId)) apply(tableId, false);
      }
      for (const tableId of target) {
        apply(tableId, true);
      }
    },
  };
}
