import {
  relationColumnPairs,
  type HoverPreview,
  type NeighborHighlight,
  type RelationHighlight,
} from './highlight_actions';
import type { HighlightState } from './highlight_state';
import {
  diffMarker,
  riskLabel,
  tableDisplayName,
  topRisk,
  type ColumnMetadata,
  type DiffKind,
  type EdgeMetadata,
  type IssueMetadata,
  type TableMetadata,
} from './metadata';

// ── Shared helpers ──────────────────────────────────────────────────────────

const ALLOWED_DIFF_KINDS = new Set(['added', 'removed', 'modified']);
const ALLOWED_SEVERITIES = new Set(['breaking', 'caution', 'warning', 'info']);
/** Sanitize a value for use in CSS class names. Returns empty string for unknown values. */
function safeCssToken(value: string, allowlist: ReadonlySet<string>): string {
  return allowlist.has(value) ? value : '';
}

function clearChildren(element: HTMLElement): void {
  element.replaceChildren();
}

function joinTableBadge(): HTMLDivElement {
  const badge = document.createElement('div');
  badge.className = 'detail-badge detail-badge-join';
  badge.textContent = 'Join Table';
  return badge;
}

function diffBadge(kind: DiffKind): HTMLDivElement {
  const badge = document.createElement('div');
  const safe = safeCssToken(kind, ALLOWED_DIFF_KINDS);
  badge.className =
    safe !== '' ? `detail-diff-badge detail-diff-badge-${safe}` : 'detail-diff-badge';
  badge.textContent = `${diffMarker(kind)} ${kind}`;
  return badge;
}

/** Badge naming the highest risk and its count, e.g. `breaking 1`. */
function riskBadge(
  issues: readonly IssueMetadata[],
  baseClass: string,
): HTMLSpanElement | undefined {
  const top = topRisk(issues);
  const label = riskLabel(issues);
  if (top === undefined || label === undefined) return undefined;
  const badge = document.createElement('span');
  const safe = safeCssToken(top.severity, ALLOWED_SEVERITIES);
  badge.className = safe !== '' ? `${baseClass} ${baseClass}-${safe}` : baseClass;
  badge.textContent = label;
  badge.title = `${issues.length} risk${issues.length === 1 ? '' : 's'}`;
  return badge;
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

// ── SVG highlight classes ───────────────────────────────────────────────────

const SVG_NS = 'http://www.w3.org/2000/svg';
// Node geometry mirrored from `relune_layout::metrics`; a Rust test in
// relune-render-html keeps these values in sync.
// Height of one column row.
const ROW_HEIGHT = 22;
// Baseline of column text, measured from the top of its row.
const ROW_BASELINE = 15;
const PORT_RADIUS = 3.5;

/** Diagram elements indexed once, so highlighting never re-queries the SVG. */
export interface HighlightTargets {
  nodesById: ReadonlyMap<string, Element>;
  /** Edge elements in metadata order, matching `connectedEdgeIndices`. */
  edges: readonly Element[];
}

export interface HighlightPainter {
  clear(): void;
  applySelected(highlight: NeighborHighlight): void;
  applyHoverPreview(preview: HoverPreview): void;
  /** Emphasizes one relationship: its path, ports, endpoint tables, and columns. */
  applyRelation(relation: RelationHighlight): void;
}

const HIGHLIGHT_CLASSES = [
  'highlighted-neighbor',
  'dimmed-by-highlight',
  'selected-node',
  'inbound',
  'outbound',
  'hover-preview-node',
  'hover-preview-neighbor',
  'hover-inbound',
  'hover-outbound',
  'hover-preview-edge',
  'selected-edge',
  'relation-endpoint',
  'relation-column',
];

function svgElement(name: string, className: string, attributes: Record<string, number>): Element {
  const element = document.createElementNS(SVG_NS, name);
  element.setAttribute('class', className);
  for (const [key, value] of Object.entries(attributes)) {
    element.setAttribute(key, String(value));
  }
  return element;
}

function numericAttribute(element: Element | null | undefined, name: string): number {
  return Number.parseFloat(element?.getAttribute(name) ?? '') || 0;
}

/** First and last points of an SVG path's `d` attribute. */
export function pathEndpoints(d: string): [[number, number], [number, number]] | null {
  const numbers = (d.match(/-?\d+(?:\.\d+)?/g) ?? []).map(Number);
  if (numbers.length < 4) return null;
  return [
    [numbers[0] ?? 0, numbers[1] ?? 0],
    [numbers[numbers.length - 2] ?? 0, numbers[numbers.length - 1] ?? 0],
  ];
}

/**
 * Applies highlight classes and remembers which elements it touched, so
 * clearing a hover preview only visits the hovered neighborhood instead of
 * every node and edge in the diagram.
 */
export function createHighlightPainter(targets: HighlightTargets): HighlightPainter {
  const touched = new Set<Element>();
  // Bands and ports drawn for a selected relationship, removed on clear.
  const decorations: Element[] = [];
  const mark = (element: Element, ...classes: string[]): void => {
    element.classList.add(...classes);
    touched.add(element);
  };
  const markColumns = (nodeId: string, columns: readonly string[]): void => {
    const node = targets.nodesById.get(nodeId);
    if (node === undefined || columns.length === 0) return;
    const body = node.querySelector('.table-body');
    node.querySelectorAll('.column-row').forEach((row) => {
      if (!columns.includes(row.getAttribute('data-column-name') ?? '')) return;
      mark(row, 'relation-column');
      const band = svgElement('rect', 'relation-column-band', {
        x: numericAttribute(body, 'x') + 1,
        y: numericAttribute(row.querySelector('.column-name'), 'y') - ROW_BASELINE,
        width: Math.max(numericAttribute(body, 'width') - 2, 0),
        height: ROW_HEIGHT,
      });
      row.prepend(band);
      decorations.push(band);
    });
  };
  const markPorts = (edge: Element): void => {
    const endpoints = pathEndpoints(edge.querySelector('.edge-path')?.getAttribute('d') ?? '');
    if (endpoints === null) return;
    for (const [cx, cy] of endpoints) {
      const port = svgElement('circle', 'relation-port', { cx, cy, r: PORT_RADIUS });
      edge.append(port);
      decorations.push(port);
    }
  };
  const directionClasses = (
    id: string,
    inbound: ReadonlySet<string>,
    outbound: ReadonlySet<string>,
    [inboundClass, outboundClass]: [string, string],
  ): string[] => {
    const isInbound = inbound.has(id);
    const isOutbound = outbound.has(id);
    if (isInbound && !isOutbound) return [inboundClass];
    if (isOutbound && !isInbound) return [outboundClass];
    return [];
  };

  return {
    clear(): void {
      for (const element of touched) {
        element.classList.remove(...HIGHLIGHT_CLASSES);
      }
      touched.clear();
      for (const decoration of decorations) {
        decoration.remove();
      }
      decorations.length = 0;
    },

    applyRelation(relation: RelationHighlight): void {
      targets.nodesById.forEach((node, id) => {
        const isEndpoint = id === relation.fromId || id === relation.toId;
        mark(node, isEndpoint ? 'relation-endpoint' : 'dimmed-by-highlight');
      });
      targets.edges.forEach((edge, index) => {
        mark(edge, index === relation.edgeIndex ? 'selected-edge' : 'dimmed-by-highlight');
      });
      markColumns(relation.fromId, relation.fromColumns);
      markColumns(relation.toId, relation.toColumns);
      const selected = targets.edges[relation.edgeIndex];
      if (selected !== undefined) markPorts(selected);
    },

    applySelected(highlight: NeighborHighlight): void {
      targets.nodesById.forEach((node, id) => {
        if (id === highlight.selectedId) {
          mark(node, 'selected-node');
        } else if (highlight.neighborIds.has(id)) {
          mark(
            node,
            'highlighted-neighbor',
            ...directionClasses(id, highlight.inboundNodeIds, highlight.outboundNodeIds, [
              'inbound',
              'outbound',
            ]),
          );
        } else {
          mark(node, 'dimmed-by-highlight');
        }
      });
      targets.edges.forEach((edge, index) => {
        mark(
          edge,
          highlight.connectedEdgeIndices.has(index)
            ? 'highlighted-neighbor'
            : 'dimmed-by-highlight',
        );
      });
    },

    applyHoverPreview(preview: HoverPreview): void {
      const hovered = targets.nodesById.get(preview.hoveredId);
      if (hovered !== undefined) mark(hovered, 'hover-preview-node');
      for (const id of preview.neighborIds) {
        const node = targets.nodesById.get(id);
        if (node === undefined) continue;
        mark(
          node,
          'hover-preview-neighbor',
          ...directionClasses(id, preview.inboundNodeIds, preview.outboundNodeIds, [
            'hover-inbound',
            'hover-outbound',
          ]),
        );
      }
      for (const index of preview.connectedEdgeIndices) {
        const edge = targets.edges[index];
        if (edge !== undefined) mark(edge, 'hover-preview-edge');
      }
    },
  };
}

// ── Relation card ───────────────────────────────────────────────────────────

export interface RelationCardElements {
  card: HTMLElement;
  kind: HTMLElement;
  title: HTMLElement;
  name: HTMLElement;
  pairs: HTMLElement;
  openFrom: HTMLButtonElement;
  openTo: HTMLButtonElement;
}

const RELATION_KIND_LABELS: Record<EdgeMetadata['kind'], string> = {
  foreign_key: 'Foreign key',
  enum_reference: 'Enum reference',
  view_dependency: 'View dependency',
};

/** Shows the selected relationship's column mapping, or hides the card. */
export function renderRelationCard(
  edge: EdgeMetadata | undefined,
  tableById: Map<string, TableMetadata>,
  elements: RelationCardElements,
): void {
  if (edge === undefined) {
    elements.card.setAttribute('hidden', '');
    clearChildren(elements.pairs);
    return;
  }
  const label = (id: string): string => {
    const table = tableById.get(id);
    return table === undefined ? id : tableDisplayName(table);
  };
  elements.card.removeAttribute('hidden');
  elements.kind.textContent = RELATION_KIND_LABELS[edge.kind] ?? edge.kind;
  elements.title.textContent = `${label(edge.from)} → ${label(edge.to)}`;
  elements.name.textContent = edge.name ?? '';
  elements.name.toggleAttribute('hidden', edge.name == null || edge.name === '');
  clearChildren(elements.pairs);
  for (const pair of relationColumnPairs(edge)) {
    const item = document.createElement('li');
    item.textContent = pair;
    elements.pairs.appendChild(item);
  }
  elements.openFrom.textContent = `Open ${label(edge.from)}`;
  elements.openTo.textContent = `Open ${label(edge.to)}`;
}

// ── Detail drawer ───────────────────────────────────────────────────────────

export interface DrawerElements {
  drawer: HTMLElement;
  title: HTMLElement;
  titleBadges: HTMLElement;
  kind: HTMLElement;
  subtitle: HTMLElement;
  metrics: HTMLElement;
  columns: HTMLElement;
  columnsEmpty: HTMLElement;
  relations: HTMLElement;
  relationsEmpty: HTMLElement;
  changesSection: HTMLElement | null;
  changes: HTMLElement | null;
  issues: HTMLElement | null;
  issuesEmpty: HTMLElement | null;
}

export interface HoverPopoverElements {
  popover: HTMLElement;
  kind: HTMLElement;
  title: HTMLElement;
  subtitle: HTMLElement;
  metrics: HTMLElement;
  badges: HTMLElement;
}

export interface PopoverPosition {
  left: number;
  top: number;
}

export function renderDrawer(
  table: TableMetadata | undefined,
  state: HighlightState,
  elements: DrawerElements,
  onSelectRelation?: (edge: EdgeMetadata) => void,
): void {
  if (table === undefined) {
    elements.drawer.setAttribute('hidden', '');
    clearChildren(elements.titleBadges);
    clearChildren(elements.metrics);
    clearChildren(elements.columns);
    clearChildren(elements.relations);
    if (elements.changes) clearChildren(elements.changes);
    if (elements.changesSection) elements.changesSection.setAttribute('hidden', '');
    if (elements.issues) clearChildren(elements.issues);
    elements.columnsEmpty.removeAttribute('hidden');
    elements.relationsEmpty.removeAttribute('hidden');
    if (elements.issuesEmpty) elements.issuesEmpty.removeAttribute('hidden');
    return;
  }

  const tableId = table.id;
  elements.drawer.removeAttribute('hidden');
  elements.kind.textContent = table.kind;
  elements.title.textContent = table.label || table.table_name || table.id;
  elements.subtitle.textContent = table.schema_name
    ? `${table.schema_name}.${table.table_name}`
    : table.table_name;

  // Title-row badges (diff status, join table candidate)
  clearChildren(elements.titleBadges);
  if (table.diff_kind) {
    elements.titleBadges.append(diffBadge(table.diff_kind));
  }
  if (table.is_join_table_candidate) {
    elements.titleBadges.append(joinTableBadge());
  }

  // Metrics
  clearChildren(elements.metrics);
  const totalRelations = table.inbound_count + table.outbound_count;
  elements.metrics.append(
    metricCard('Columns', String(table.columns.length)),
    metricCard('Relations', String(totalRelations)),
    metricCard('\u2190 In', String(table.inbound_count)),
    metricCard('Out \u2192', String(table.outbound_count)),
  );

  // Diff changes (FK, index and check changes are listed only here)
  if (elements.changes instanceof HTMLElement && elements.changesSection instanceof HTMLElement) {
    clearChildren(elements.changes);
    const details = table.diff_details ?? [];
    if (details.length === 0) {
      elements.changesSection.setAttribute('hidden', '');
    } else {
      elements.changesSection.removeAttribute('hidden');
      for (const detail of details) {
        const item = document.createElement('li');
        item.className = 'detail-change';
        item.textContent = detail;
        elements.changes.appendChild(item);
      }
    }
  }

  // Columns
  clearChildren(elements.columns);
  if (table.columns.length === 0) {
    elements.columnsEmpty.removeAttribute('hidden');
  } else {
    elements.columnsEmpty.setAttribute('hidden', '');
    for (const column of table.columns) {
      elements.columns.appendChild(buildColumnElement(column));
    }
  }

  // Relations
  clearChildren(elements.relations);
  const relations = [...(state.inboundMap[tableId] ?? []), ...(state.outboundMap[tableId] ?? [])];
  if (relations.length === 0) {
    elements.relationsEmpty.removeAttribute('hidden');
  } else {
    elements.relationsEmpty.setAttribute('hidden', '');
    for (const relation of relations) {
      elements.relations.appendChild(
        buildRelationElement(relation.edge, relation.node, state.tableById, onSelectRelation),
      );
    }
  }

  // Risks
  if (elements.issues instanceof HTMLElement && elements.issuesEmpty instanceof HTMLElement) {
    clearChildren(elements.issues);
    const issues: IssueMetadata[] = table.issues ?? [];
    if (issues.length === 0) {
      elements.issuesEmpty.removeAttribute('hidden');
    } else {
      elements.issuesEmpty.setAttribute('hidden', '');
      for (const issue of issues) {
        elements.issues.appendChild(buildIssueElement(issue));
      }
    }
  }
}

export function hideHoverPopover(elements: HoverPopoverElements): void {
  elements.popover.setAttribute('hidden', '');
  elements.popover.style.removeProperty('left');
  elements.popover.style.removeProperty('top');
  elements.popover.style.removeProperty('visibility');
  clearChildren(elements.metrics);
  clearChildren(elements.badges);
}

export function renderHoverPopover(
  table: TableMetadata | undefined,
  elements: HoverPopoverElements,
  position: PopoverPosition | undefined,
): void {
  if (table === undefined || position === undefined) {
    hideHoverPopover(elements);
    return;
  }

  elements.kind.textContent = table.kind;
  elements.title.textContent = tableDisplayName(table);
  elements.subtitle.textContent = table.schema_name
    ? `${table.schema_name}.${table.table_name}`
    : table.table_name;

  clearChildren(elements.metrics);
  elements.metrics.append(
    summaryMetric('Cols', String(table.columns.length)),
    summaryMetric('In', String(table.inbound_count)),
    summaryMetric('Out', String(table.outbound_count)),
  );

  clearChildren(elements.badges);
  if (table.diff_kind) {
    elements.badges.appendChild(diffBadge(table.diff_kind));
  }

  const badge = riskBadge(table.issues ?? [], 'hover-popover-badge');
  if (badge !== undefined) {
    elements.badges.appendChild(badge);
  }

  placePopover(elements.popover, position);
}

function buildColumnElement(column: ColumnMetadata): HTMLDivElement {
  const columnEl = document.createElement('div');
  columnEl.className = 'detail-column';

  const name = document.createElement('span');
  name.className = 'detail-column-name';
  name.textContent = column.name;

  const pills = document.createElement('span');
  pills.className = 'detail-column-pills';

  if (column.is_primary_key) {
    const pk = document.createElement('span');
    pk.className = 'detail-column-pill detail-column-pill-pk';
    pk.textContent = 'PK';
    pills.appendChild(pk);
  }

  if (column.is_foreign_key) {
    const fk = document.createElement('span');
    fk.className = 'detail-column-pill detail-column-pill-fk';
    fk.textContent = 'FK';
    pills.appendChild(fk);
  }

  if (column.is_indexed) {
    const ix = document.createElement('span');
    ix.className = 'detail-column-pill detail-column-pill-ix';
    ix.textContent = 'IX';
    pills.appendChild(ix);
  }

  const typePill = document.createElement('span');
  typePill.className = 'detail-column-pill';
  const dataType = column.data_type || 'unknown';
  typePill.textContent =
    column.previous_data_type != null ? `${column.previous_data_type} → ${dataType}` : dataType;
  pills.appendChild(typePill);

  const nullPill = document.createElement('span');
  nullPill.className = `detail-column-pill ${column.nullable ? 'detail-column-pill-nullable' : 'detail-column-pill-required'}`;
  nullPill.textContent = column.nullable ? 'nullable' : 'required';
  pills.appendChild(nullPill);

  if (column.diff_kind) {
    const diffPill = document.createElement('span');
    const safeDiff = safeCssToken(column.diff_kind, ALLOWED_DIFF_KINDS);
    diffPill.className =
      safeDiff !== ''
        ? `detail-column-pill detail-column-pill-diff detail-column-pill-diff-${safeDiff}`
        : 'detail-column-pill detail-column-pill-diff';
    diffPill.textContent = `${diffMarker(column.diff_kind)} ${column.diff_kind}`;
    pills.appendChild(diffPill);
  }

  columnEl.append(name, pills);
  return columnEl;
}

function buildRelationElement(
  edge: EdgeMetadata,
  targetNodeId: string,
  tableById: Map<string, TableMetadata>,
  onSelectRelation?: (edge: EdgeMetadata) => void,
): HTMLElement {
  const targetTable = tableById.get(targetNodeId);
  const targetName = targetTable?.label ?? targetNodeId;

  const label = document.createElement('span');
  label.className = 'detail-relation-label';
  label.textContent = edge.name ?? `${edge.from} → ${edge.to}`;

  const meta = document.createElement('span');
  meta.className = 'detail-relation-meta';
  const columnMap =
    edge.from_columns.length > 0 && edge.to_columns.length > 0
      ? ` · ${edge.from_columns.join(', ')} → ${edge.to_columns.join(', ')}`
      : '';
  meta.textContent = `${edge.kind} · ${targetName}${columnMap}`;

  if (onSelectRelation) {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'detail-relation detail-relation-navigable';
    btn.setAttribute('aria-label', `Show relationship ${relationColumnPairs(edge).join(', ')}`);
    btn.addEventListener('click', () => {
      onSelectRelation(edge);
    });
    btn.append(label, meta);
    return btn;
  }

  const div = document.createElement('div');
  div.className = 'detail-relation';
  div.append(label, meta);
  return div;
}

function buildIssueElement(issue: IssueMetadata): HTMLDivElement {
  const issueEl = document.createElement('div');
  const safeSev = safeCssToken(issue.severity, ALLOWED_SEVERITIES);
  issueEl.className = safeSev !== '' ? `detail-issue detail-issue-${safeSev}` : 'detail-issue';

  const header = document.createElement('div');
  header.className = 'detail-issue-header';

  const badge = document.createElement('span');
  badge.className =
    safeSev !== '' ? `detail-issue-badge detail-issue-badge-${safeSev}` : 'detail-issue-badge';
  badge.textContent = issue.severity;

  const msg = document.createElement('span');
  msg.className = 'detail-issue-message';
  msg.textContent = issue.message;

  header.append(badge, msg);
  issueEl.appendChild(header);

  if (issue.hint) {
    const hintEl = document.createElement('span');
    hintEl.className = 'detail-issue-hint';
    hintEl.textContent = `→ ${issue.hint}`;
    issueEl.appendChild(hintEl);
  }

  return issueEl;
}

function summaryMetric(label: string, value: string): HTMLSpanElement {
  const metric = document.createElement('span');
  metric.className = 'hover-popover-metric';

  const labelEl = document.createElement('span');
  labelEl.className = 'hover-popover-metric-label';
  labelEl.textContent = label;

  const valueEl = document.createElement('span');
  valueEl.className = 'hover-popover-metric-value';
  valueEl.textContent = value;

  metric.append(labelEl, valueEl);
  return metric;
}

function placePopover(popover: HTMLElement, position: PopoverPosition): void {
  const margin = 12;
  popover.removeAttribute('hidden');
  popover.style.left = `${Math.round(position.left)}px`;
  popover.style.top = `${Math.round(position.top)}px`;
  popover.style.visibility = 'hidden';

  const rect = popover.getBoundingClientRect();
  const left = Math.max(margin, Math.min(position.left, window.innerWidth - rect.width - margin));
  const top = Math.max(margin, Math.min(position.top, window.innerHeight - rect.height - margin));

  popover.style.left = `${Math.round(left)}px`;
  popover.style.top = `${Math.round(top)}px`;
  popover.style.visibility = 'visible';
}

// ── Object browser ──────────────────────────────────────────────────────────

export interface ObjectBrowserItem {
  table: TableMetadata;
  isSelected: boolean;
  isDimmedBySearch: boolean;
  isExcludedByFilter: boolean;
  isHiddenByGroup: boolean;
}

export interface ObjectBrowser {
  /** Shows `items` in order, reusing each table's button across renders. */
  render(items: ObjectBrowserItem[], totalCount: number): void;
}

export function createObjectBrowser(
  listEl: HTMLElement,
  countEl: HTMLElement,
  emptyEl: HTMLElement,
  onSelect: (tableId: string) => void,
): ObjectBrowser {
  const buttons = new Map<string, HTMLButtonElement>();
  return {
    render(items: ObjectBrowserItem[], totalCount: number): void {
      countEl.textContent = `${items.length}/${totalCount}`;
      emptyEl.toggleAttribute('hidden', items.length > 0);
      listEl.replaceChildren(
        ...items.map((item) => {
          let button = buttons.get(item.table.id);
          if (button === undefined) {
            button = buildObjectBrowserButton(item.table, onSelect);
            buttons.set(item.table.id, button);
          }
          button.classList.toggle('selected', item.isSelected);
          button.classList.toggle('filtered-out', item.isDimmedBySearch || item.isExcludedByFilter);
          button.classList.toggle('hidden-item', item.isHiddenByGroup);
          return button;
        }),
      );
    },
  };
}

function buildObjectBrowserButton(
  table: TableMetadata,
  onSelect: (tableId: string) => void,
): HTMLButtonElement {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'object-browser-item';

  const header = document.createElement('div');
  header.className = 'object-browser-item-header';

  const name = document.createElement('span');
  name.className = 'object-browser-item-name';
  name.textContent = table.label || table.table_name || table.id;

  const kind = document.createElement('span');
  kind.className = 'object-browser-kind';
  kind.textContent = table.kind;

  const issueBadge = riskBadge(table.issues ?? [], 'object-browser-issue-badge');
  if (issueBadge !== undefined) {
    header.append(name, issueBadge, kind);
  } else {
    header.append(name, kind);
  }

  const meta = document.createElement('div');
  meta.className = 'object-browser-item-meta';

  const counts = document.createElement('span');
  counts.textContent = `${table.columns.length} cols`;

  const relations = document.createElement('span');
  relations.textContent = `${table.inbound_count} in / ${table.outbound_count} out`;

  meta.append(counts, relations);
  button.append(header, meta);

  button.addEventListener('click', () => {
    onSelect(table.id);
  });

  return button;
}
