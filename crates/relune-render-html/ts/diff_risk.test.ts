import { describe, expect, it } from 'vitest';

import { renderDrawer, type DrawerElements } from './highlight_dom';
import { createHighlightState } from './highlight_state';
import { diffMarker, riskLabel, topRisk, type IssueMetadata } from './metadata';
import { column, table } from './test_fixtures';

const risk = (severity: IssueMetadata['severity'], message: string = severity): IssueMetadata => ({
  severity,
  message,
});

describe('topRisk', () => {
  it('names the highest severity and counts only the risks at it', () => {
    const issues = [risk('warning'), risk('breaking'), risk('info'), risk('breaking')];
    expect(topRisk(issues)).toEqual({ severity: 'breaking', count: 2 });
    expect(riskLabel(issues)).toBe('breaking 2');
  });

  it('returns nothing without risks', () => {
    expect(topRisk([])).toBeUndefined();
    expect(riskLabel([])).toBeUndefined();
  });

  it('marks each change kind like the SVG cards', () => {
    expect(['added', 'removed', 'modified'].map((k) => diffMarker(k as never))).toEqual([
      '+',
      '−',
      '~',
    ]);
  });
});

function drawerElements(): DrawerElements {
  document.body.innerHTML = `
    <aside id="drawer"></aside>
    <h2 id="title"></h2><div id="badges"></div><p id="kind"></p><p id="subtitle"></p>
    <div id="metrics"></div>
    <section id="changes-section" hidden><ul id="changes"></ul></section>
    <div id="columns"></div><div id="columns-empty"></div>
    <div id="relations"></div><div id="relations-empty"></div>
    <div id="issues"></div><div id="issues-empty"></div>`;
  const byId = (id: string) => document.getElementById(id) as HTMLElement;
  return {
    drawer: byId('drawer'),
    title: byId('title'),
    titleBadges: byId('badges'),
    kind: byId('kind'),
    subtitle: byId('subtitle'),
    metrics: byId('metrics'),
    columns: byId('columns'),
    columnsEmpty: byId('columns-empty'),
    relations: byId('relations'),
    relationsEmpty: byId('relations-empty'),
    changesSection: byId('changes-section'),
    changes: byId('changes'),
    issues: byId('issues'),
    issuesEmpty: byId('issues-empty'),
  };
}

describe('renderDrawer for a diff', () => {
  it('shows the change kind, type change, and risks as separate pieces', () => {
    const users = table('users', {
      diff_kind: 'modified',
      diff_details: ['+ status', '~ name: varchar(500) → varchar(120)'],
      columns: [
        column('status', 'text', { diff_kind: 'added' }),
        column('name', 'varchar(120)', {
          diff_kind: 'modified',
          previous_data_type: 'varchar(500)',
        }),
      ],
      issues: [risk('breaking', 'name narrowed')],
    });
    const elements = drawerElements();
    renderDrawer(users, createHighlightState([users], []), elements);

    expect(elements.titleBadges.textContent).toBe('~ modified');
    expect(elements.changesSection?.hasAttribute('hidden')).toBe(false);
    expect([...(elements.changes?.children ?? [])].map((li) => li.textContent)).toEqual([
      '+ status',
      '~ name: varchar(500) → varchar(120)',
    ]);
    expect(elements.columns.textContent).toContain('varchar(500) → varchar(120)');
    expect(elements.columns.textContent).toContain('+ added');
    expect(elements.issues?.querySelector('.detail-issue-breaking')?.textContent).toContain(
      'name narrowed',
    );
  });

  it('hides the change list for an unchanged table', () => {
    const tags = table('tags');
    const elements = drawerElements();
    renderDrawer(tags, createHighlightState([tags], []), elements);

    expect(elements.titleBadges.textContent).toBe('');
    expect(elements.changesSection?.hasAttribute('hidden')).toBe(true);
    expect(elements.issuesEmpty?.hasAttribute('hidden')).toBe(false);
  });
});
