import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  definitionExcerpt,
  findingColumns,
  findingTarget,
  stepFinding,
  type DefinitionLine,
  type ReviewFinding,
  type ReviewReport,
} from './review_report';
import { metadataScript, resetViewerRuntime, table } from './test_fixtures';
import { getViewerRuntime } from './viewer_api';

function finding(overrides: Partial<ReviewFinding> = {}): ReviewFinding {
  return {
    rule_id: 'risk/drop-column',
    severity: 'breaking',
    message: 'Column users.email is dropped.',
    rule_description: 'Column is being dropped',
    table_id: 'users',
    table_name: 'users',
    column_name: 'email',
    ...overrides,
  };
}

function line(text: string, change: DefinitionLine['change'] = 'unchanged'): DefinitionLine {
  return { text, change };
}

describe('review report helpers', () => {
  it('names a finding target like the text report', () => {
    expect(findingTarget(finding())).toBe('users.email');
    expect(findingTarget(finding({ column_name: null, fk_name: 'orders_user_fk' }))).toBe(
      'users.orders_user_fk',
    );
    expect(findingTarget(finding({ column_name: null }))).toBe('users');
    expect(findingTarget(finding({ table_name: null, column_name: null }))).toBe('(schema)');
  });

  it('marks every key column, or the single column', () => {
    expect(findingColumns(finding({ columns: ['tenant_id', 'email'] }))).toEqual([
      'tenant_id',
      'email',
    ]);
    expect(findingColumns(finding())).toEqual(['email']);
    expect(findingColumns(finding({ column_name: null }))).toEqual([]);
  });

  it('keeps changed and focused lines with context and folds the rest', () => {
    const lines = [
      line('TABLE users'),
      ...Array.from({ length: 8 }, (_, index) => line(`  c${index} int`)),
      line('  email text', 'removed'),
      line('  email varchar(120)', 'added'),
      ...Array.from({ length: 6 }, (_, index) => line(`  d${index} int`)),
    ];
    const excerpt = definitionExcerpt(lines, [], 1);
    expect(excerpt).toEqual([
      { kind: 'line', index: 0 },
      { kind: 'gap', count: 7 },
      { kind: 'line', index: 8 },
      { kind: 'line', index: 9 },
      { kind: 'line', index: 10 },
      { kind: 'line', index: 11 },
      { kind: 'gap', count: 5 },
    ]);
    expect(definitionExcerpt(lines, [3], 0).slice(0, 3)).toEqual([
      { kind: 'line', index: 0 },
      { kind: 'gap', count: 2 },
      { kind: 'line', index: 3 },
    ]);
  });

  it('steps through visible findings and wraps around', () => {
    const visible = [true, false, true];
    expect(stepFinding(visible, null, 1)).toBe(0);
    expect(stepFinding(visible, 0, 1)).toBe(2);
    expect(stepFinding(visible, 2, 1)).toBe(0);
    expect(stepFinding(visible, null, -1)).toBe(2);
    expect(stepFinding([false, false], null, 1)).toBeNull();
  });
});

const PANEL_HTML = `
  <details id="review-panel" open>
    <summary><span id="review-panel-meta"></span></summary>
    <p id="review-dialect" hidden></p>
    <div id="review-coverage" hidden></div>
    <div id="review-severity-filter"></div>
    <ol id="review-findings"></ol>
    <p id="review-empty" hidden></p>
  </details>
`;

function reportScript(report: ReviewReport): string {
  return `<script type="application/json" id="relune-review">${JSON.stringify(report)}</script>`;
}

const REPORT: ReviewReport = {
  summary: { breaking: 1, caution: 0, warning: 1, info: 0 },
  suppressed: 0,
  dialect_note: 'Effective dialect: postgres (resolved from auto)',
  coverage_notes: ['1 unsupported SQL construct in the after input was skipped.'],
  findings: [
    finding({ focus_lines: [1] }),
    finding({
      rule_id: 'risk/add-unique-on-existing',
      severity: 'warning',
      message: 'UNIQUE index on (tenant_id,email) is being added.',
      rule_description: 'UNIQUE added',
      table_id: 'accounts',
      table_name: 'accounts',
      column_name: null,
      columns: ['tenant_id', 'email'],
      related_table_id: 'users',
      mitigation: 'Deduplicate first.',
    }),
  ],
  definitions: {
    users: { lines: [line('TABLE users'), line('  email text', 'removed')] },
  },
};

async function loadPanel(report: ReviewReport): Promise<ReturnType<typeof vi.fn>> {
  vi.resetModules();
  document.body.innerHTML =
    PANEL_HTML +
    metadataScript({ tables: [table('users'), table('accounts', { label: 'app.accounts' })] }) +
    reportScript(report);
  const focus = vi.fn(() => true);
  getViewerRuntime().selection = {
    clear: vi.fn(),
    select: vi.fn(),
    getSelected: () => null,
    selectRelation: () => false,
    getSelectedRelation: () => null,
    focus,
  };
  await import('./review_panel');
  return focus;
}

function findingItems(): HTMLLIElement[] {
  return Array.from(document.querySelectorAll<HTMLLIElement>('.review-finding'));
}

beforeEach(() => {
  resetViewerRuntime();
  window.sessionStorage.clear();
});

describe('review panel', () => {
  it('lists findings with the summary, dialect, and coverage gaps', async () => {
    await loadPanel(REPORT);
    expect(document.getElementById('review-panel-meta')?.textContent).toBe(
      '1 breaking · 1 warning',
    );
    expect(document.getElementById('review-dialect')?.hidden).toBe(false);
    expect(document.getElementById('review-coverage')?.textContent).toContain('unsupported SQL');
    expect(
      findingItems().map((item) => item.querySelector('.review-finding-target')?.textContent),
    ).toEqual(['users.email', 'accounts']);
    expect(document.getElementById('review-empty')?.hidden).toBe(true);
  });

  it('shows a finding on the diagram and opens its reasoning', async () => {
    const focus = await loadPanel(REPORT);
    const [, unique] = findingItems();
    unique?.querySelector<HTMLButtonElement>('.review-finding-toggle')?.click();

    expect(focus).toHaveBeenCalledWith('accounts', ['tenant_id', 'email']);
    expect(unique?.classList.contains('selected')).toBe(true);
    const detail = unique?.querySelector('.review-finding-detail');
    expect(detail?.hasAttribute('hidden')).toBe(false);
    expect(detail?.textContent).toContain('Deduplicate first.');

    const related = Array.from(detail?.querySelectorAll<HTMLButtonElement>('.review-link') ?? []);
    expect(related.map((button) => button.textContent)).toEqual([
      'tableapp.accounts',
      'relatedusers',
    ]);
    related[1]?.click();
    expect(focus).toHaveBeenLastCalledWith('users', []);
  });

  it('renders the definition diff with the focused line', async () => {
    await loadPanel(REPORT);
    findingItems()[0]?.querySelector<HTMLButtonElement>('.review-finding-toggle')?.click();
    const lines = Array.from(document.querySelectorAll('.review-definition-line'));
    expect(lines.map((element) => element.className)).toEqual([
      'review-definition-line review-definition-line-unchanged',
      'review-definition-line review-definition-line-removed review-definition-line-focus',
    ]);
  });

  it('filters by severity and steps through the visible findings', async () => {
    const focus = await loadPanel(REPORT);
    const chips = Array.from(document.querySelectorAll<HTMLButtonElement>('.review-severity-chip'));
    expect(chips.map((chip) => chip.textContent)).toEqual(['breaking 1', 'warning 1']);

    chips[0]?.click();
    expect(chips[0]?.getAttribute('aria-pressed')).toBe('false');
    expect(findingItems()[0]?.hidden).toBe(true);

    getViewerRuntime().review?.step(1);
    expect(focus).toHaveBeenLastCalledWith('accounts', ['tenant_id', 'email']);
    getViewerRuntime().review?.step(1);
    expect(focus).toHaveBeenCalledTimes(2);
  });

  it('drops the selected mark when the diagram selects another table', async () => {
    await loadPanel(REPORT);
    findingItems()[0]?.querySelector<HTMLButtonElement>('.review-finding-toggle')?.click();
    document.dispatchEvent(
      new CustomEvent('relune:node-selected', { detail: { nodeId: 'accounts' } }),
    );
    expect(findingItems()[0]?.classList.contains('selected')).toBe(false);
  });

  it('says so when there are no findings', async () => {
    await loadPanel({
      ...REPORT,
      summary: { breaking: 0, caution: 0, warning: 0, info: 0 },
      findings: [],
      coverage_notes: [],
      dialect_note: null,
    });
    expect(document.getElementById('review-empty')?.hidden).toBe(false);
    expect(document.getElementById('review-panel-meta')?.textContent).toBe('no findings');
    expect(document.getElementById('review-coverage')?.hidden).toBe(true);
  });
});
