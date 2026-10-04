import { beforeEach, describe, expect, it, vi } from 'vitest';

import { edge, metadataScript, resetViewerRuntime, table } from './test_fixtures';
import { getViewerRuntime } from './viewer_api';

// Mirrors the viewer shell around the drawer, hover popover, and relation card.
function renderViewer(): void {
  document.body.innerHTML = `
    ${metadataScript({
      tables: [table('posts'), table('users')],
      edges: [edge('posts', 'users', { from_columns: ['user_id'], to_columns: ['id'] })],
    })}
    <aside id="hover-popover" hidden>
      <p id="hover-popover-kind"></p><h2 id="hover-popover-title"></h2>
      <p id="hover-popover-subtitle"></p><div id="hover-popover-metrics"></div>
      <div id="hover-popover-badges"></div>
    </aside>
    <aside id="relation-card" tabindex="-1" hidden>
      <p id="relation-card-kind"></p><h2 id="relation-card-title"></h2>
      <button id="relation-card-close"></button><p id="relation-card-name" hidden></p>
      <ul id="relation-card-pairs"></ul>
      <button id="relation-card-open-from"></button><button id="relation-card-open-to"></button>
    </aside>
    <aside id="detail-drawer" tabindex="-1" hidden>
      <p id="detail-kind"></p><h2 id="detail-title"></h2><div id="detail-title-badges"></div>
      <p id="detail-subtitle"></p><div id="detail-metrics"></div>
      <div id="detail-columns-empty"></div><div id="detail-columns"></div>
      <div id="detail-relationships-empty"></div><div id="detail-relations"></div>
    </aside>
    <div id="viewport"><div id="canvas"><svg>
      <g class="node" data-id="posts"><rect class="table-body"/></g>
      <g class="node" data-id="users"><rect class="table-body"/></g>
      <g class="edge" data-from="posts" data-to="users"><path class="edge-path" d="M 0 0 L 10 10"/></g>
    </svg></div></div>`;
}

const byId = (id: string): HTMLElement => {
  const element = document.getElementById(id);
  if (element === null) throw new Error(`missing #${id}`);
  return element;
};

describe('relationship keyboard path', () => {
  beforeEach(async () => {
    resetViewerRuntime();
    renderViewer();
    vi.resetModules();
    await import('./highlight');
  });

  it('reaches the mapping from a table drawer and returns there', () => {
    getViewerRuntime().selection?.select('posts');
    const relationButton = byId('detail-relations').querySelector('button');
    expect(relationButton?.getAttribute('aria-label')).toBe(
      'Show relationship posts.user_id → users.id',
    );

    relationButton?.click();
    const card = byId('relation-card');
    expect(card.hasAttribute('hidden')).toBe(false);
    expect(document.activeElement).toBe(card);
    expect(byId('relation-card-pairs').textContent).toBe('posts.user_id → users.id');
    expect(byId('detail-drawer').hasAttribute('hidden')).toBe(true);
    expect(document.querySelector('.edge')?.classList.contains('selected-edge')).toBe(true);

    // Closing the card goes back to the drawer it was opened from.
    byId('relation-card-close').click();
    expect(card.hasAttribute('hidden')).toBe(true);
    expect(byId('detail-title').textContent).toBe('posts');
    expect(document.activeElement).toBe(byId('detail-drawer'));
  });

  it('opens either end of the relationship in the drawer', () => {
    (document.querySelector('.edge') as Element).dispatchEvent(
      new MouseEvent('click', { bubbles: true }),
    );
    expect(byId('relation-card').hasAttribute('hidden')).toBe(false);

    byId('relation-card-open-to').click();
    expect(byId('relation-card').hasAttribute('hidden')).toBe(true);
    expect(getViewerRuntime().selection?.getSelected()).toBe('users');
    expect(document.activeElement).toBe(byId('detail-drawer'));
  });

  it('clears the relationship with the selection runtime', () => {
    (document.querySelector('.edge') as Element).dispatchEvent(
      new MouseEvent('click', { bubbles: true }),
    );
    getViewerRuntime().selection?.clear();
    expect(byId('relation-card').hasAttribute('hidden')).toBe(true);
    expect(document.querySelectorAll('.selected-edge, .relation-port')).toHaveLength(0);
  });
});
