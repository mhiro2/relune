import type { EdgeMetadata, IssueMetadata, TableMetadata } from './metadata';
import type { HighlightState } from './highlight_state';
import type { NeighborHighlight } from './highlight_actions';

// ── Shared helpers ──────────────────────────────────────────────────────────

function clearChildren(element: HTMLElement): void {
  element.replaceChildren();
}

function diffBadge(kind: string): HTMLDivElement {
  const badge = document.createElement('div');
  badge.className = `detail-diff-badge detail-diff-badge-${kind}`;
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

export function clearHighlightClasses(svgRoot: Element, getNodes: NodeQuery): void {
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
}

export function applyHighlightClasses(
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

export function renderDrawer(
  table: TableMetadata | undefined,
  state: HighlightState,
  elements: DrawerElements,
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

  // Metrics
  clearChildren(elements.metrics);
  if (table.diff_kind) {
    elements.metrics.append(diffBadge(table.diff_kind));
  }
  elements.metrics.append(
    metricCard('Columns', String(table.columns.length)),
    metricCard('Inbound', String(table.inbound_count)),
    metricCard('Outbound', String(table.outbound_count)),
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
        buildRelationElement(relation.edge, relation.node, state.tableById),
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

function buildColumnElement(column: {
  name: string;
  data_type: string;
  nullable: boolean;
  is_primary_key: boolean;
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
    diffPill.className = `detail-column-pill detail-column-pill-diff detail-column-pill-diff-${column.diff_kind}`;
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
): HTMLDivElement {
  const relationEl = document.createElement('div');
  relationEl.className = 'detail-relation';

  const targetTable = tableById.get(targetNodeId);
  const label = document.createElement('span');
  label.className = 'detail-relation-label';
  label.textContent = edge.name ?? `${edge.from} → ${edge.to}`;

  const meta = document.createElement('span');
  meta.className = 'detail-relation-meta';
  const targetName = targetTable?.label ?? targetNodeId;
  const columnMap =
    edge.from_columns.length > 0 && edge.to_columns.length > 0
      ? ` · ${edge.from_columns.join(', ')} → ${edge.to_columns.join(', ')}`
      : '';
  meta.textContent = `${edge.kind} · ${targetName}${columnMap}`;

  relationEl.append(label, meta);
  return relationEl;
}

function buildIssueElement(issue: IssueMetadata): HTMLDivElement {
  const issueEl = document.createElement('div');
  issueEl.className = `detail-issue detail-issue-${issue.severity}`;

  const header = document.createElement('div');
  header.className = 'detail-issue-header';

  const badge = document.createElement('span');
  badge.className = `detail-issue-badge detail-issue-badge-${issue.severity}`;
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

// ── Object browser ──────────────────────────────────────────────────────────

export interface ObjectBrowserItem {
  table: TableMetadata;
  isSelected: boolean;
  isDimmedBySearch: boolean;
  isDimmedByTypeFilter: boolean;
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
  button.classList.toggle('filtered-out', item.isDimmedBySearch || item.isDimmedByTypeFilter);
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
    issueBadge.className = `object-browser-issue-badge object-browser-issue-badge-${maxSeverity}`;
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
