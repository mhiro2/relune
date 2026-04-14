import {
  tableDisplayName,
  type EdgeMetadata,
  type IssueMetadata,
  type TableMetadata,
} from './metadata';
import type { HighlightState } from './highlight_state';
import type { HoverPreview, NeighborHighlight } from './highlight_actions';

// ── Shared helpers ──────────────────────────────────────────────────────────

const ALLOWED_DIFF_KINDS = new Set(['added', 'removed', 'modified']);
const ALLOWED_SEVERITIES = new Set(['error', 'warning', 'info', 'hint']);
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

function diffBadge(kind: string): HTMLDivElement {
  const badge = document.createElement('div');
  const safe = safeCssToken(kind, ALLOWED_DIFF_KINDS);
  badge.className =
    safe !== '' ? `detail-diff-badge detail-diff-badge-${safe}` : 'detail-diff-badge';
  badge.textContent = kind;
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

export type NodeQuery = () => NodeListOf<Element>;
export type NodeIdFn = (node: Element) => string | null;

const SELECTED_NODE_CLASSES = [
  'highlighted-neighbor',
  'dimmed-by-highlight',
  'selected-node',
  'inbound',
  'outbound',
];
const HOVER_NODE_CLASSES = [
  'hover-preview-node',
  'hover-preview-neighbor',
  'hover-inbound',
  'hover-outbound',
];

export function clearHighlightClasses(svgRoot: Element, getNodes: NodeQuery): void {
  getNodes().forEach((node) => {
    node.classList.remove(...SELECTED_NODE_CLASSES, ...HOVER_NODE_CLASSES);
  });
  svgRoot.querySelectorAll('.edge').forEach((edge) => {
    edge.classList.remove('highlighted-neighbor', 'dimmed-by-highlight', 'hover-preview-edge');
  });
}

export function applySelectedHighlightClasses(
  svgRoot: Element,
  getNodes: NodeQuery,
  getNodeId: NodeIdFn,
  highlight: NeighborHighlight,
): void {
  getNodes().forEach((node) => {
    const id = getNodeId(node);
    if (id === highlight.selectedId) {
      node.classList.add('selected-node');
      node.classList.remove('dimmed-by-highlight');
    } else if (id !== null && highlight.neighborIds.has(id)) {
      node.classList.add('highlighted-neighbor');
      const isInbound = highlight.inboundNodeIds.has(id);
      const isOutbound = highlight.outboundNodeIds.has(id);
      node.classList.toggle('inbound', isInbound && !isOutbound);
      node.classList.toggle('outbound', isOutbound && !isInbound);
      node.classList.remove('dimmed-by-highlight');
    } else {
      node.classList.add('dimmed-by-highlight');
      node.classList.remove('highlighted-neighbor', 'selected-node', 'inbound', 'outbound');
    }
  });

  svgRoot.querySelectorAll('.edge').forEach((edgeElement, index) => {
    edgeElement.classList.toggle('highlighted-neighbor', highlight.connectedEdgeIndices.has(index));
    edgeElement.classList.toggle('dimmed-by-highlight', !highlight.connectedEdgeIndices.has(index));
  });
}

export function applyHoverPreviewClasses(
  svgRoot: Element,
  getNodes: NodeQuery,
  getNodeId: NodeIdFn,
  preview: HoverPreview,
): void {
  getNodes().forEach((node) => {
    const id = getNodeId(node);
    if (id === preview.hoveredId) {
      node.classList.add('hover-preview-node');
    } else if (id !== null && preview.neighborIds.has(id)) {
      node.classList.add('hover-preview-neighbor');
      const isInbound = preview.inboundNodeIds.has(id);
      const isOutbound = preview.outboundNodeIds.has(id);
      node.classList.toggle('hover-inbound', isInbound && !isOutbound);
      node.classList.toggle('hover-outbound', isOutbound && !isInbound);
    }
  });

  svgRoot.querySelectorAll('.edge').forEach((edgeElement, index) => {
    edgeElement.classList.toggle('hover-preview-edge', preview.connectedEdgeIndices.has(index));
  });
}

// ── Detail drawer ───────────────────────────────────────────────────────────

export interface DrawerElements {
  drawer: HTMLElement;
  title: HTMLElement;
  kind: HTMLElement;
  subtitle: HTMLElement;
  metrics: HTMLElement;
  columns: HTMLElement;
  columnsEmpty: HTMLElement;
  relations: HTMLElement;
  relationsEmpty: HTMLElement;
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
  onNavigate?: (tableId: string) => void,
): void {
  if (table === undefined) {
    elements.drawer.setAttribute('hidden', '');
    clearChildren(elements.metrics);
    clearChildren(elements.columns);
    clearChildren(elements.relations);
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

  // Badges (join table candidate, diff)
  clearChildren(elements.metrics);
  if (table.is_join_table_candidate) {
    elements.metrics.append(joinTableBadge());
  }
  if (table.diff_kind) {
    elements.metrics.append(diffBadge(table.diff_kind));
  }

  // Metrics
  const totalRelations = table.inbound_count + table.outbound_count;
  elements.metrics.append(
    metricCard('Columns', String(table.columns.length)),
    metricCard('Relations', String(totalRelations)),
    metricCard('\u2190 In', String(table.inbound_count)),
    metricCard('Out \u2192', String(table.outbound_count)),
  );

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
        buildRelationElement(relation.edge, relation.node, state.tableById, onNavigate),
      );
    }
  }

  // Issues
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

  const issues = table.issues ?? [];
  if (issues.length > 0) {
    elements.badges.appendChild(issueCountBadge(issues));
  }

  placePopover(elements.popover, position);
}

function buildColumnElement(column: {
  name: string;
  data_type: string;
  nullable: boolean;
  is_primary_key: boolean;
  is_foreign_key: boolean;
  is_indexed: boolean;
  diff_kind?: string | null;
}): HTMLDivElement {
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
  typePill.textContent = column.data_type || 'unknown';
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
    diffPill.textContent = column.diff_kind;
    pills.appendChild(diffPill);
  }

  columnEl.append(name, pills);
  return columnEl;
}

function buildRelationElement(
  edge: EdgeMetadata,
  targetNodeId: string,
  tableById: Map<string, TableMetadata>,
  onNavigate?: (tableId: string) => void,
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

  if (onNavigate) {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'detail-relation detail-relation-navigable';
    btn.addEventListener('click', () => {
      onNavigate(targetNodeId);
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

function issueCountBadge(issues: IssueMetadata[]): HTMLSpanElement {
  const badge = document.createElement('span');
  const safeSeverity = safeCssToken(highestIssueSeverity(issues), ALLOWED_SEVERITIES);
  badge.className =
    safeSeverity !== ''
      ? `hover-popover-badge hover-popover-badge-${safeSeverity}`
      : 'hover-popover-badge';
  badge.textContent = `${issues.length} issue${issues.length === 1 ? '' : 's'}`;
  return badge;
}

function highestIssueSeverity(issues: IssueMetadata[]): string {
  const severityRank: Record<string, number> = { error: 3, warning: 2, info: 1, hint: 0 };
  return issues.reduce((max, issue) => {
    return (severityRank[issue.severity] ?? 0) > (severityRank[max] ?? 0) ? issue.severity : max;
  }, 'hint');
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

export function renderObjectBrowser(
  items: ObjectBrowserItem[],
  totalCount: number,
  listEl: HTMLElement,
  countEl: HTMLElement,
  emptyEl: HTMLElement,
  onSelect: (tableId: string) => void,
): void {
  listEl.replaceChildren();
  countEl.textContent = `${items.length}/${totalCount}`;
  emptyEl.toggleAttribute('hidden', items.length > 0);

  for (const item of items) {
    const button = buildObjectBrowserButton(item, onSelect);
    listEl.appendChild(button);
  }
}

function buildObjectBrowserButton(
  item: ObjectBrowserItem,
  onSelect: (tableId: string) => void,
): HTMLButtonElement {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'object-browser-item';
  button.classList.toggle('selected', item.isSelected);
  button.classList.toggle('filtered-out', item.isDimmedBySearch || item.isExcludedByFilter);
  button.classList.toggle('hidden-item', item.isHiddenByGroup);

  const header = document.createElement('div');
  header.className = 'object-browser-item-header';

  const name = document.createElement('span');
  name.className = 'object-browser-item-name';
  name.textContent = item.table.label || item.table.table_name || item.table.id;

  const kind = document.createElement('span');
  kind.className = 'object-browser-kind';
  kind.textContent = item.table.kind;

  const tableIssues = item.table.issues ?? [];
  if (tableIssues.length > 0) {
    const severityRank: Record<string, number> = { error: 3, warning: 2, info: 1, hint: 0 };
    const maxSeverity = tableIssues.reduce((max, issue) => {
      return (severityRank[issue.severity] ?? 0) > (severityRank[max] ?? 0) ? issue.severity : max;
    }, 'hint' as string);
    const issueBadge = document.createElement('span');
    const safeSev = safeCssToken(maxSeverity, ALLOWED_SEVERITIES);
    issueBadge.className =
      safeSev !== ''
        ? `object-browser-issue-badge object-browser-issue-badge-${safeSev}`
        : 'object-browser-issue-badge';
    issueBadge.textContent = String(tableIssues.length);
    issueBadge.title = `${tableIssues.length} issue${tableIssues.length === 1 ? '' : 's'}`;
    header.append(name, issueBadge, kind);
  } else {
    header.append(name, kind);
  }

  const meta = document.createElement('div');
  meta.className = 'object-browser-item-meta';

  const counts = document.createElement('span');
  counts.textContent = `${item.table.columns.length} cols`;

  const relations = document.createElement('span');
  relations.textContent = `${item.table.inbound_count} in / ${item.table.outbound_count} out`;

  meta.append(counts, relations);
  button.append(header, meta);

  button.addEventListener('click', () => {
    onSelect(item.table.id);
  });

  return button;
}
