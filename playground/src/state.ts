import type {
  CompareView,
  EdgeStyle,
  ExampleId,
  ExportFormat,
  GroupBy,
  LayoutAlgorithm,
  LayoutDirection,
  PersistedState,
  ReviewDialect,
  Theme,
  WorkbenchMode,
} from './types.js';

export const CUSTOM_EXAMPLE_ID = 'custom';
export const DEFAULT_EXAMPLE_ID: Exclude<ExampleId, 'custom'> = 'simple-blog';
export const MANUAL_VIEWPOINT_ID = '';

export const DEFAULT_STATE: PersistedState = {
  example: DEFAULT_EXAMPLE_ID,
  mode: 'render',
  theme: 'light',
  layout: 'hierarchical',
  direction: 'top-to-bottom',
  edgeStyle: 'curved',
  viewpoint: MANUAL_VIEWPOINT_ID,
  groupBy: 'none',
  focusTable: '',
  depth: '1',
  includeTables: '',
  excludeTables: '',
  exportFormat: 'schema-json',
  inspectTable: '',
  lintRules: '',
  compareView: 'visual',
  compareReviewDialect: 'auto',
  sql: '',
  compareBeforeSql: '',
  compareAfterSql: '',
};

export function splitPatterns(rawValue: string): string[] {
  return rawValue
    .split(',')
    .map((value) => value.trim())
    .filter((value) => value.length > 0);
}

export function serializePatterns(patterns: readonly string[]): string {
  return patterns.join(', ');
}

export function arraysEqual(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

export function parsePositiveInteger(rawValue: string): number | undefined {
  const parsed = Number.parseInt(rawValue, 10);
  if (!Number.isFinite(parsed) || parsed < 1) {
    return undefined;
  }
  return parsed;
}

/** Parses the persisted localStorage payload, dropping anything malformed. */
export function parseStoredState(rawValue: string | null): Partial<PersistedState> {
  if (!rawValue) {
    return {};
  }

  try {
    return sanitizeState(JSON.parse(rawValue) as Partial<PersistedState>);
  } catch {
    return {};
  }
}

/** Reads shareable settings from a `location.search` string. */
export function parseQueryState(search: string): Partial<PersistedState> {
  const params = new URLSearchParams(search);
  return sanitizeState({
    example: (params.get('example') as ExampleId | null) ?? undefined,
    mode: (params.get('mode') as WorkbenchMode | null) ?? undefined,
    theme: (params.get('theme') as Theme | null) ?? undefined,
    layout: (params.get('layout') as LayoutAlgorithm | null) ?? undefined,
    direction: (params.get('direction') as LayoutDirection | null) ?? undefined,
    edgeStyle: (params.get('edges') as EdgeStyle | null) ?? undefined,
    viewpoint: params.get('viewpoint') ?? undefined,
    groupBy: (params.get('group') as GroupBy | null) ?? undefined,
    focusTable: params.get('focus') ?? undefined,
    depth: params.get('depth') ?? undefined,
    includeTables: params.get('include') ?? undefined,
    excludeTables: params.get('exclude') ?? undefined,
    exportFormat: (params.get('export') as ExportFormat | null) ?? undefined,
    inspectTable: params.get('table') ?? undefined,
    lintRules: params.get('rules') ?? undefined,
    compareView: (params.get('compare') as CompareView | null) ?? undefined,
    compareReviewDialect: (params.get('reviewDialect') as ReviewDialect | null) ?? undefined,
  });
}

/** Serializes the shareable subset of `state` into a query string (without `?`). */
export function buildQueryString(state: PersistedState): string {
  const params = new URLSearchParams();
  params.set('example', state.example);
  params.set('mode', state.mode);
  params.set('theme', state.theme);
  params.set('layout', state.layout);
  params.set('direction', state.direction);
  params.set('edges', state.edgeStyle);
  params.set('group', state.groupBy);

  if (state.viewpoint) {
    params.set('viewpoint', state.viewpoint);
  }
  if (state.focusTable) {
    params.set('focus', state.focusTable);
  }
  if (state.depth && state.depth !== DEFAULT_STATE.depth) {
    params.set('depth', state.depth);
  }
  if (state.includeTables) {
    params.set('include', state.includeTables);
  }
  if (state.excludeTables) {
    params.set('exclude', state.excludeTables);
  }
  if (state.mode === 'export') {
    params.set('export', state.exportFormat);
  }
  if (state.mode === 'inspect' && state.inspectTable) {
    params.set('table', state.inspectTable);
  }
  if (state.mode === 'lint' && state.lintRules) {
    params.set('rules', state.lintRules);
  }
  if (state.mode === 'compare') {
    params.set('compare', state.compareView);
    if (
      state.compareView === 'review' &&
      state.compareReviewDialect !== DEFAULT_STATE.compareReviewDialect
    ) {
      params.set('reviewDialect', state.compareReviewDialect);
    }
  }

  return params.toString();
}

export function sanitizeState(state: Partial<PersistedState>): Partial<PersistedState> {
  const sanitized: Partial<PersistedState> = {};

  if (isExampleId(state.example)) {
    sanitized.example = state.example;
  }
  if (isWorkbenchMode(state.mode)) {
    sanitized.mode = state.mode;
  }
  if (isTheme(state.theme)) {
    sanitized.theme = state.theme;
  }
  if (isLayoutAlgorithm(state.layout)) {
    sanitized.layout = state.layout;
  }
  if (isLayoutDirection(state.direction)) {
    sanitized.direction = state.direction;
  }
  if (isEdgeStyle(state.edgeStyle)) {
    sanitized.edgeStyle = state.edgeStyle;
  }
  if (typeof state.viewpoint === 'string') {
    sanitized.viewpoint = state.viewpoint.trim();
  }
  if (isGroupBy(state.groupBy)) {
    sanitized.groupBy = state.groupBy;
  }
  if (typeof state.focusTable === 'string') {
    sanitized.focusTable = state.focusTable;
  }
  if (typeof state.depth === 'string') {
    sanitized.depth = state.depth;
  }
  if (typeof state.includeTables === 'string') {
    sanitized.includeTables = state.includeTables;
  }
  if (typeof state.excludeTables === 'string') {
    sanitized.excludeTables = state.excludeTables;
  }
  if (isExportFormat(state.exportFormat)) {
    sanitized.exportFormat = state.exportFormat;
  }
  if (typeof state.inspectTable === 'string') {
    sanitized.inspectTable = state.inspectTable;
  }
  if (typeof state.lintRules === 'string') {
    sanitized.lintRules = state.lintRules;
  }
  if (isCompareView(state.compareView)) {
    sanitized.compareView = state.compareView;
  }
  if (isReviewDialect(state.compareReviewDialect)) {
    sanitized.compareReviewDialect = state.compareReviewDialect;
  }
  if (typeof state.sql === 'string') {
    sanitized.sql = state.sql;
  }
  if (typeof state.compareBeforeSql === 'string') {
    sanitized.compareBeforeSql = state.compareBeforeSql;
  }
  if (typeof state.compareAfterSql === 'string') {
    sanitized.compareAfterSql = state.compareAfterSql;
  }

  return sanitized;
}

export function isExampleId(value: unknown): value is ExampleId {
  return (
    value === 'simple-blog' ||
    value === 'ecommerce' ||
    value === 'multi-schema' ||
    value === CUSTOM_EXAMPLE_ID
  );
}

export function isWorkbenchMode(value: unknown): value is WorkbenchMode {
  return (
    value === 'render' ||
    value === 'inspect' ||
    value === 'export' ||
    value === 'lint' ||
    value === 'compare'
  );
}

function isTheme(value: unknown): value is Theme {
  return value === 'light' || value === 'dark';
}

function isLayoutAlgorithm(value: unknown): value is LayoutAlgorithm {
  return value === 'hierarchical' || value === 'force-directed';
}

function isLayoutDirection(value: unknown): value is LayoutDirection {
  return (
    value === 'top-to-bottom' ||
    value === 'left-to-right' ||
    value === 'right-to-left' ||
    value === 'bottom-to-top'
  );
}

function isEdgeStyle(value: unknown): value is EdgeStyle {
  return value === 'curved' || value === 'orthogonal' || value === 'straight';
}

function isGroupBy(value: unknown): value is GroupBy {
  return value === 'none' || value === 'schema' || value === 'prefix';
}

function isExportFormat(value: unknown): value is ExportFormat {
  return (
    value === 'schema-json' ||
    value === 'graph-json' ||
    value === 'layout-json' ||
    value === 'mermaid' ||
    value === 'd2' ||
    value === 'dot'
  );
}

function isCompareView(value: unknown): value is CompareView {
  return (
    value === 'visual' ||
    value === 'text' ||
    value === 'markdown' ||
    value === 'json' ||
    value === 'review'
  );
}

function isReviewDialect(value: unknown): value is ReviewDialect {
  return value === 'auto' || value === 'postgres' || value === 'mysql' || value === 'sqlite';
}

export function toBuiltinExampleId(value: ExampleId): Exclude<ExampleId, 'custom'> {
  return value === CUSTOM_EXAMPLE_ID ? DEFAULT_EXAMPLE_ID : value;
}
