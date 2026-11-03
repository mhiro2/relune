import { parseReluneMetadata, type RiskSeverity, type TableMetadata } from './metadata';
import {
  REVIEW_SEVERITIES,
  definitionExcerpt,
  findingColumns,
  findingTarget,
  parseReviewReport,
  sourceLabel,
  stepFinding,
  type DefinitionDiff,
  type ReportStatement,
  type ReviewFinding,
  type ReviewReport,
} from './review_report';
import { getViewerRuntime, persistDetailsOpen } from './viewer_api';

// The review panel lists the findings of an HTML review report. Choosing a
// finding selects its table, marks its columns on the diagram, and opens
// its reasoning: why the rule fired, the definition before and after, the
// tables it affects, and what to do.
{
  const report = parseReviewReport();
  const panel = document.getElementById('review-panel');
  const meta = document.getElementById('review-panel-meta');
  const dialect = document.getElementById('review-dialect');
  const coverage = document.getElementById('review-coverage');
  const severityFilter = document.getElementById('review-severity-filter');
  const list = document.getElementById('review-findings');
  const empty = document.getElementById('review-empty');

  if (
    report !== null &&
    panel instanceof HTMLDetailsElement &&
    meta instanceof HTMLElement &&
    dialect instanceof HTMLElement &&
    coverage instanceof HTMLElement &&
    severityFilter instanceof HTMLElement &&
    list instanceof HTMLElement &&
    empty instanceof HTMLElement
  ) {
    mountReviewPanel(report, {
      panel,
      meta,
      dialect,
      coverage,
      severityFilter,
      list,
      empty,
    });
  }
}

interface ReviewPanelElements {
  panel: HTMLDetailsElement;
  meta: HTMLElement;
  dialect: HTMLElement;
  coverage: HTMLElement;
  severityFilter: HTMLElement;
  list: HTMLElement;
  empty: HTMLElement;
}

function mountReviewPanel(report: ReviewReport, elements: ReviewPanelElements): void {
  const runtime = getViewerRuntime();
  const tables = new Map<string, TableMetadata>(
    (parseReluneMetadata()?.tables ?? []).map((table) => [table.id, table]),
  );
  const tableLabel = (id: string): string => tables.get(id)?.label ?? id;

  persistDetailsOpen(elements.panel, 'relune-review-panel-open');
  elements.meta.textContent = summaryText(report);
  const notes = [report.dialect_note, report.migration_note].filter(
    (note): note is string => note != null,
  );
  if (notes.length > 0) {
    elements.dialect.textContent = notes.join(' · ');
    elements.dialect.removeAttribute('hidden');
  }
  const statements = new Map(
    (report.statements ?? []).map((statement) => [sourceLabel(statement), statement]),
  );
  renderCoverage(elements.coverage, report);

  const findings = report.findings;
  const items = findings.map((finding, index) =>
    buildFindingItem(
      finding,
      index,
      report.definitions?.[finding.table_id ?? ''],
      statements,
      tableLabel,
    ),
  );
  elements.list.append(...items.map((item) => item.root));
  elements.empty.toggleAttribute('hidden', findings.length > 0);

  // Severities shown in the list; every severity starts visible.
  const shown = new Set<RiskSeverity>(REVIEW_SEVERITIES);
  const visible = (): boolean[] => findings.map((finding) => shown.has(finding.severity));
  let selected: number | null = null;

  const markSelected = (index: number | null): void => {
    if (selected !== null) items[selected]?.root.classList.remove('selected');
    selected = index;
    if (index !== null) items[index]?.root.classList.add('selected');
  };

  const setExpanded = (index: number, expanded: boolean): void => {
    const item = items[index];
    if (item === undefined) return;
    item.toggle.setAttribute('aria-expanded', String(expanded));
    item.detail.toggleAttribute('hidden', !expanded);
  };

  /** Shows a finding on the diagram and opens its reasoning. */
  const selectFinding = (index: number, focusToggle = false): void => {
    const finding = findings[index];
    const item = items[index];
    if (finding === undefined || item === undefined) return;
    items.forEach((_item, other) => {
      if (other !== index) setExpanded(other, false);
    });
    setExpanded(index, true);
    markSelected(index);
    if (finding.table_id != null) {
      runtime.selection?.focus(finding.table_id, findingColumns(finding));
    }
    item.root.scrollIntoView?.({ block: 'nearest' });
    if (focusToggle) item.toggle.focus();
  };

  items.forEach((item, index) => {
    item.toggle.addEventListener('click', () => {
      if (selected === index && item.toggle.getAttribute('aria-expanded') === 'true') {
        setExpanded(index, false);
        return;
      }
      selectFinding(index);
    });
    for (const link of item.links) {
      link.button.addEventListener('click', () => {
        runtime.selection?.focus(link.tableId, link.columns);
      });
    }
  });

  // A selection made on the diagram itself no longer follows a finding.
  document.addEventListener('relune:node-selected', (event: Event) => {
    const nodeId = (event as CustomEvent<{ nodeId: string }>).detail.nodeId;
    if (selected !== null && findings[selected]?.table_id !== nodeId) markSelected(null);
  });
  document.addEventListener('relune:node-cleared', () => {
    markSelected(null);
  });

  renderSeverityFilter(elements.severityFilter, report, shown, () => {
    const flags = visible();
    items.forEach((item, index) => {
      item.root.toggleAttribute('hidden', flags[index] !== true);
    });
    if (selected !== null && flags[selected] !== true) markSelected(null);
  });

  runtime.review = {
    step(delta: 1 | -1): void {
      const next = stepFinding(visible(), selected, delta);
      if (next === null) return;
      runtime.sidebar?.setCollapsed(false);
      elements.panel.open = true;
      selectFinding(next, true);
    },
    togglePanel(): void {
      const reveal = runtime.sidebar?.isCollapsed() === true;
      runtime.sidebar?.setCollapsed(false);
      elements.panel.open = reveal || !elements.panel.open;
    },
  };
}

function summaryText(report: ReviewReport): string {
  const parts = REVIEW_SEVERITIES.filter((severity) => report.summary[severity] > 0).map(
    (severity) => `${report.summary[severity]} ${severity}`,
  );
  return parts.length === 0 ? 'no findings' : parts.join(' · ');
}

function renderCoverage(container: HTMLElement, report: ReviewReport): void {
  const notes = report.coverage_notes ?? [];
  const suppressed =
    report.suppressed > 0
      ? [`${report.suppressed} finding(s) on excepted tables are not listed.`]
      : [];
  if (notes.length === 0 && suppressed.length === 0) return;
  if (notes.length > 0) {
    const heading = document.createElement('strong');
    heading.textContent = 'Not fully reviewed';
    container.appendChild(heading);
  }
  const list = document.createElement('ul');
  for (const note of [...notes, ...suppressed]) {
    const item = document.createElement('li');
    item.textContent = note;
    list.appendChild(item);
  }
  container.appendChild(list);
  container.removeAttribute('hidden');
}

function renderSeverityFilter(
  container: HTMLElement,
  report: ReviewReport,
  shown: Set<RiskSeverity>,
  onChange: () => void,
): void {
  for (const severity of REVIEW_SEVERITIES) {
    const count = report.summary[severity];
    if (count === 0) continue;
    const chip = document.createElement('button');
    chip.type = 'button';
    chip.className = 'review-severity-chip';
    chip.setAttribute('aria-pressed', 'true');
    const dot = document.createElement('span');
    dot.className = `review-severity-dot review-severity-dot-${severity}`;
    dot.setAttribute('aria-hidden', 'true');
    chip.append(dot, `${severity} ${count}`);
    chip.addEventListener('click', () => {
      const show = !shown.has(severity);
      if (show) shown.add(severity);
      else shown.delete(severity);
      chip.setAttribute('aria-pressed', String(show));
      onChange();
    });
    container.appendChild(chip);
  }
}

interface FindingLink {
  button: HTMLButtonElement;
  tableId: string;
  columns: string[];
}

interface FindingItem {
  root: HTMLLIElement;
  toggle: HTMLButtonElement;
  detail: HTMLDivElement;
  links: FindingLink[];
}

function buildFindingItem(
  finding: ReviewFinding,
  index: number,
  definition: DefinitionDiff | undefined,
  statements: ReadonlyMap<string, ReportStatement>,
  tableLabel: (id: string) => string,
): FindingItem {
  const root = document.createElement('li');
  root.className = `review-finding review-finding-${finding.severity}`;

  const detailId = `review-finding-detail-${index}`;
  const toggle = document.createElement('button');
  toggle.type = 'button';
  toggle.className = 'review-finding-toggle';
  toggle.setAttribute('aria-expanded', 'false');
  toggle.setAttribute('aria-controls', detailId);
  const badge = document.createElement('span');
  badge.className = `review-badge review-badge-${finding.severity}`;
  badge.textContent = finding.severity;
  const target = document.createElement('span');
  target.className = 'review-finding-target';
  target.textContent = findingTarget(finding);
  const summary = document.createElement('span');
  summary.className = 'review-finding-summary';
  summary.textContent = finding.rule_description;
  toggle.append(badge, target, summary);

  const detail = document.createElement('div');
  detail.className = 'review-finding-detail';
  detail.id = detailId;
  detail.setAttribute('hidden', '');

  const ruleId = document.createElement('p');
  ruleId.className = 'review-rule-id';
  ruleId.textContent = finding.rule_id;
  detail.append(ruleId, section('Finding', paragraph(finding.message)));

  if (finding.execution != null) {
    const execution = document.createElement('p');
    execution.className = `review-execution review-execution-${finding.execution.verdict}`;
    const verdict = document.createElement('span');
    verdict.className = 'review-execution-verdict';
    verdict.textContent = finding.execution.verdict;
    execution.append(verdict, finding.execution.detail);
    detail.append(section('How the migration runs it', execution));
  }

  const sources = finding.sources ?? [];
  if (sources.length > 0) {
    detail.append(section('Migration SQL', sourceList(sources.map(sourceLabel), statements)));
  }

  if (definition !== undefined && definition.lines.length > 0) {
    detail.append(section('Definition', definitionBlock(definition, finding.focus_lines ?? [])));
  }

  const links: FindingLink[] = [];
  if (finding.table_id != null) {
    links.push(
      linkButton('table', tableLabel(finding.table_id), finding.table_id, findingColumns(finding)),
    );
  }
  if (finding.related_table_id != null && finding.related_table_id !== finding.table_id) {
    links.push(
      linkButton('related', tableLabel(finding.related_table_id), finding.related_table_id, []),
    );
  }
  if (links.length > 0) {
    const row = document.createElement('div');
    row.className = 'review-links';
    row.append(...links.map((link) => link.button));
    detail.append(section('Affects', row));
  }

  if (finding.mitigation != null) {
    detail.append(section('What to do', paragraph(finding.mitigation)));
  }

  root.append(toggle, detail);
  return { root, toggle, detail, links };
}

function sourceList(
  labels: readonly string[],
  statements: ReadonlyMap<string, ReportStatement>,
): HTMLOListElement {
  const list = document.createElement('ol');
  list.className = 'review-sources';
  for (const label of labels) {
    const item = document.createElement('li');
    const location = document.createElement('span');
    location.className = 'review-source-location';
    location.textContent = label;
    item.appendChild(location);
    const statement = statements.get(label);
    if (statement !== undefined) {
      if (statement.in_transaction) {
        const badge = document.createElement('span');
        badge.className = 'review-source-transaction';
        badge.textContent = 'in transaction';
        item.appendChild(badge);
      }
      const sql = document.createElement('pre');
      sql.className = 'review-source-sql';
      sql.textContent = statement.sql;
      item.appendChild(sql);
    }
    list.appendChild(item);
  }
  return list;
}

function section(title: string, body: HTMLElement): DocumentFragment {
  const fragment = document.createDocumentFragment();
  const heading = document.createElement('h4');
  heading.textContent = title;
  fragment.append(heading, body);
  return fragment;
}

function paragraph(text: string): HTMLParagraphElement {
  const element = document.createElement('p');
  element.textContent = text;
  return element;
}

function linkButton(role: string, label: string, tableId: string, columns: string[]): FindingLink {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'review-link';
  const roleEl = document.createElement('span');
  roleEl.className = 'review-link-role';
  roleEl.textContent = role;
  button.append(roleEl, label);
  return { button, tableId, columns };
}

function definitionBlock(definition: DefinitionDiff, focus: readonly number[]): HTMLPreElement {
  const pre = document.createElement('pre');
  pre.className = 'review-definition';
  const focused = new Set(focus);
  for (const item of definitionExcerpt(definition.lines, focus)) {
    if (item.kind === 'gap') {
      const gap = document.createElement('span');
      gap.className = 'review-definition-gap';
      gap.textContent = `… ${item.count} unchanged line${item.count === 1 ? '' : 's'}`;
      pre.appendChild(gap);
      continue;
    }
    const line = definition.lines[item.index];
    if (line === undefined) continue;
    const span = document.createElement('span');
    const classes = ['review-definition-line', `review-definition-line-${line.change}`];
    if (focused.has(item.index)) classes.push('review-definition-line-focus');
    span.className = classes.join(' ');
    span.textContent = line.text;
    pre.appendChild(span);
  }
  return pre;
}
