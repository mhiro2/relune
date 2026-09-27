import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { parseReluneMetadata, tableDisplayName } from './metadata';
import { metadataScript, resetViewerRuntime, table } from './test_fixtures';
import {
  getViewerRuntime,
  isEditableTarget,
  markViewerModuleReady,
  reportSessionStorageError,
  waitForViewerModules,
} from './viewer_api';

beforeEach(() => {
  resetViewerRuntime();
  document.body.innerHTML = '';
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('viewer runtime', () => {
  it('shares one runtime object across callers', () => {
    const runtime = getViewerRuntime();
    runtime.search = {} as never;
    expect(getViewerRuntime()).toBe(runtime);
  });

  it('runs waiters immediately when their modules are ready', () => {
    markViewerModuleReady('search');
    const callback = vi.fn();
    waitForViewerModules(['search'], callback);
    waitForViewerModules([], callback);
    expect(callback).toHaveBeenCalledTimes(2);
  });

  it('defers waiters until every module is ready and runs them once', () => {
    const callback = vi.fn();
    waitForViewerModules(['search', 'viewport', 'search'], callback);
    markViewerModuleReady('search');
    expect(callback).not.toHaveBeenCalled();
    markViewerModuleReady('viewport');
    expect(callback).toHaveBeenCalledTimes(1);
    markViewerModuleReady('filters');
    expect(callback).toHaveBeenCalledTimes(1);
  });
});

describe('isEditableTarget', () => {
  it('recognises form fields and contenteditable regions', () => {
    document.body.innerHTML = `
      <input id="input"><textarea id="textarea"></textarea><select id="select"></select>
      <div contenteditable="true"><span id="inside"></span></div>
      <button id="button"></button>`;
    for (const id of ['input', 'textarea', 'select', 'inside']) {
      expect(isEditableTarget(document.getElementById(id))).toBe(true);
    }
    expect(isEditableTarget(document.getElementById('button'))).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
    expect(isEditableTarget(window)).toBe(false);
  });
});

describe('reportSessionStorageError', () => {
  it('shows a temporary notice when storage is full', () => {
    vi.useFakeTimers();
    reportSessionStorageError('saving state', new DOMException('full', 'QuotaExceededError'));
    const notice = document.querySelector('#relune-viewer-notices .viewer-notice-warning');
    expect(notice?.getAttribute('role')).toBe('alert');
    expect(notice?.textContent).toContain('saving state');
    vi.advanceTimersByTime(4500);
    expect(document.querySelector('.viewer-notice')).toBeNull();
  });

  it('ignores blocked storage and logs other failures', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    reportSessionStorageError('reading', new DOMException('denied', 'SecurityError'));
    expect(warn).not.toHaveBeenCalled();
    reportSessionStorageError('reading', new Error('boom'));
    expect(warn).toHaveBeenCalledOnce();
    expect(document.querySelector('.viewer-notice')).toBeNull();
  });
});

describe('metadata', () => {
  it('parses the embedded metadata and tolerates missing or invalid JSON', () => {
    expect(parseReluneMetadata()).toBeNull();
    document.body.innerHTML = metadataScript({ tables: [table('users')] });
    expect(parseReluneMetadata()?.tables.map((t) => t.id)).toEqual(['users']);
    document.body.innerHTML = '<script type="application/json" id="relune-metadata">{</script>';
    expect(parseReluneMetadata()).toBeNull();
  });

  it('prefers the label, then the table name, then the id for display', () => {
    expect(tableDisplayName(table('s.users', { label: 'Users', table_name: 'users' }))).toBe(
      'Users',
    );
    expect(tableDisplayName(table('s.users', { label: '', table_name: 'users' }))).toBe('users');
    expect(tableDisplayName(table('s.users', { label: '', table_name: '' }))).toBe('s.users');
  });
});
