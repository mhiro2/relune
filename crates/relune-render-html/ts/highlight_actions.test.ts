import { describe, expect, it } from 'vitest';

import {
  computeHoverPreview,
  computeNeighborHighlights,
  matchesBrowserQuery,
} from './highlight_actions';
import { createHighlightState } from './highlight_state';
import { column, edge, table } from './test_fixtures';

// users <- posts <- comments -> users, plus tags <- post_tags -> posts
const tables = ['users', 'posts', 'comments', 'tags', 'post_tags'].map((id) => table(id));
const edges = [
  edge('posts', 'users', { name: 'posts_author_fk' }),
  edge('comments', 'posts'),
  edge('comments', 'users'),
  edge('post_tags', 'posts'),
  edge('post_tags', 'tags'),
];

describe('createHighlightState', () => {
  it('indexes tables and both edge directions', () => {
    const state = createHighlightState(tables, edges);
    expect(state.tableById.get('posts')?.id).toBe('posts');
    expect(state.outboundMap.comments?.map((r) => r.node)).toEqual(['posts', 'users']);
    expect(state.inboundMap.posts?.map((r) => r.node)).toEqual(['comments', 'post_tags']);
    expect(state.inboundMap.comments).toBeUndefined();
    expect(state).toMatchObject({ hoveredNode: null, selectedNode: null, traversalDepth: 1 });
  });
});

describe('computeNeighborHighlights', () => {
  const state = createHighlightState(tables, edges);

  it('collects direct neighbours and their edges at depth 1', () => {
    const result = computeNeighborHighlights('posts', state);
    expect(result.selectedId).toBe('posts');
    expect([...result.neighborIds].toSorted()).toEqual(['comments', 'post_tags', 'users']);
    expect([...result.outboundNodeIds]).toEqual(['users']);
    expect([...result.inboundNodeIds].toSorted()).toEqual(['comments', 'post_tags']);
    expect([...result.connectedEdgeIndices].toSorted((a, b) => a - b)).toEqual([0, 1, 3]);
  });

  it('walks further hops while keeping direction sets to direct neighbours', () => {
    const result = computeNeighborHighlights('users', state, 2);
    // The cycle back through posts -> users does not make users its own neighbour.
    expect([...result.neighborIds].toSorted()).toEqual(['comments', 'post_tags', 'posts']);
    expect([...result.inboundNodeIds].toSorted()).toEqual(['comments', 'posts']);
    expect(result.outboundNodeIds.size).toBe(0);
    // post_tags -> tags is three hops away from users.
    expect(result.connectedEdgeIndices.has(4)).toBe(false);
    expect([...result.connectedEdgeIndices].toSorted((a, b) => a - b)).toEqual([0, 1, 2, 3]);
  });

  it('keeps self-references out of the neighbour sets but highlights their edge', () => {
    const selfRef = createHighlightState(tables, [
      edge('users', 'users', { name: 'users_manager_fk' }),
      edge('posts', 'users'),
    ]);
    const result = computeNeighborHighlights('users', selfRef);
    expect([...result.neighborIds]).toEqual(['posts']);
    expect([...result.inboundNodeIds]).toEqual(['posts']);
    expect(result.outboundNodeIds.size).toBe(0);
    expect([...result.connectedEdgeIndices].toSorted((a, b) => a - b)).toEqual([0, 1]);
  });

  it('returns an empty neighbourhood for isolated or unknown nodes', () => {
    const result = computeNeighborHighlights('missing', state, 3);
    expect(result.neighborIds.size).toBe(0);
    expect(result.connectedEdgeIndices.size).toBe(0);
  });

  it('highlights every edge that shares a traversed endpoint pair and name', () => {
    const duplicated = createHighlightState(tables, [
      edge('posts', 'users'),
      edge('posts', 'users'),
    ]);
    const result = computeNeighborHighlights('users', duplicated);
    expect([...result.connectedEdgeIndices]).toEqual([0, 1]);
  });
});

describe('computeHoverPreview', () => {
  it('previews only the one-hop neighbourhood', () => {
    const state = createHighlightState(tables, edges);
    const preview = computeHoverPreview('tags', state);
    expect(preview.hoveredId).toBe('tags');
    expect([...preview.neighborIds]).toEqual(['post_tags']);
    expect([...preview.connectedEdgeIndices]).toEqual([4]);
  });
});

describe('matchesBrowserQuery', () => {
  const users = table('public.users', {
    label: 'Users',
    table_name: 'users',
    columns: [column('email', 'citext')],
  });

  it('matches ids, labels, table names, columns and column types', () => {
    for (const query of ['public.', 'USERS', 'email', 'citext', '  Email  ']) {
      expect(matchesBrowserQuery(users, query)).toBe(true);
    }
    expect(matchesBrowserQuery(users, 'orders')).toBe(false);
  });

  it('treats a blank query as matching everything', () => {
    expect(matchesBrowserQuery(users, '   ')).toBe(true);
  });
});
