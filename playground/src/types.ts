export type ExampleId = 'simple-blog' | 'ecommerce' | 'multi-schema' | 'custom';
export type Theme = 'light' | 'dark';
export type LayoutAlgorithm = 'hierarchical' | 'force-directed';
export type LayoutDirection = 'top-to-bottom' | 'left-to-right' | 'right-to-left' | 'bottom-to-top';
export type EdgeStyle = 'curved' | 'orthogonal' | 'straight';
export type GroupBy = 'none' | 'schema' | 'prefix';
export type WorkbenchMode = 'render' | 'inspect' | 'export' | 'lint' | 'compare';
export type ExportFormat = 'schema-json' | 'graph-json' | 'layout-json' | 'mermaid' | 'd2' | 'dot';
export type CompareView = 'visual' | 'text' | 'markdown' | 'json' | 'review';
export type ReviewDialect = 'auto' | 'postgres' | 'mysql' | 'sqlite';
export type ViewpointId = string;
export type WasmSeverity = 'error' | 'warning' | 'info' | 'hint';

export type WasmDiagnosticCode = {
  prefix: string;
  number: number;
};

export type WasmDiagnostic = {
  severity: WasmSeverity;
  code: WasmDiagnosticCode;
  message: string;
};

export type WasmDuration = {
  secs: number;
  nanos: number;
};

export type WasmRenderStats = {
  table_count: number;
  column_count: number;
  edge_count: number;
  view_count: number;
  parse_time: WasmDuration;
  graph_time: WasmDuration;
  layout_time: WasmDuration;
  render_time: WasmDuration;
  total_time: WasmDuration;
};

export type WasmRenderResult = {
  content: string;
  diagnostics: WasmDiagnostic[];
  stats: WasmRenderStats;
};

export type SchemaStats = {
  table_count: number;
  column_count: number;
  foreign_key_count: number;
  view_count: number;
};

export type TableSummary = {
  name: string;
  column_count: number;
  foreign_key_count: number;
  incoming_fk_count: number;
  index_count: number;
  has_primary_key: boolean;
};

export type SchemaSummary = {
  table_count: number;
  column_count: number;
  foreign_key_count: number;
  index_count: number;
  view_count: number;
  enum_count: number;
  tables_without_pk: number;
  orphan_table_count: number;
  tables: TableSummary[];
};

export type ColumnDetails = {
  name: string;
  data_type: string;
  nullable: boolean;
  is_primary_key: boolean;
  comment?: string | null;
};

export type ForeignKeyDetails = {
  name?: string | null;
  from_columns: string[];
  to_table: string;
  to_columns: string[];
  on_delete?: string | null;
  on_update?: string | null;
};

export type IndexDetails = {
  name?: string | null;
  columns: string[];
  is_unique: boolean;
};

export type TableDetails = {
  name: string;
  comment?: string | null;
  columns: ColumnDetails[];
  foreign_keys: ForeignKeyDetails[];
  indexes: IndexDetails[];
};

export type WasmInspectResult = {
  summary: SchemaSummary;
  table?: TableDetails | null;
  diagnostics: WasmDiagnostic[];
};

export type WasmExportResult = {
  content: string;
  diagnostics: WasmDiagnostic[];
  stats: SchemaStats;
};

export type LintStats = {
  total: number;
  errors: number;
  warnings: number;
  infos: number;
  hints: number;
};

export type LintIssue = {
  rule_id: string;
  category: string;
  severity: WasmSeverity;
  message: string;
  table_id?: string | null;
  table_name?: string | null;
  column_name?: string | null;
  hint?: string | null;
};

export type WasmLintResult = {
  issues: LintIssue[];
  stats: LintStats;
  diagnostics: WasmDiagnostic[];
};

export type DiffSummary = {
  tables_added: number;
  tables_removed: number;
  tables_modified: number;
  columns_changed: number;
  foreign_keys_changed: number;
  indexes_changed: number;
  views_added: number;
  views_removed: number;
  views_modified: number;
  view_columns_changed: number;
  view_definitions_changed: number;
  enums_added: number;
  enums_removed: number;
  enums_modified: number;
  enum_values_changed: number;
};

export type TableDiff = {
  table_name: string;
  column_diffs: unknown[];
  fk_diffs: unknown[];
  index_diffs: unknown[];
};

export type ViewDiff = {
  view_name: string;
  column_diffs: unknown[];
};

export type EnumDiff = {
  enum_name: string;
  value_diffs: unknown[];
};

export type SchemaDiff = {
  added_tables: string[];
  removed_tables: string[];
  modified_tables: TableDiff[];
  added_views: string[];
  removed_views: string[];
  modified_views: ViewDiff[];
  added_enums: string[];
  removed_enums: string[];
  modified_enums: EnumDiff[];
  summary: DiffSummary;
};

export type WasmDiffResult = {
  diff: SchemaDiff;
  diagnostics: WasmDiagnostic[];
  rendered?: string | null;
  content?: string | null;
};

export type ReviewSeverity = 'breaking' | 'caution' | 'warning' | 'info';

export type ReviewFinding = {
  rule_id: string;
  severity: ReviewSeverity;
  message: string;
  mitigation?: string | null;
  table_id?: string | null;
  table_name?: string | null;
  column_name?: string | null;
  fk_name?: string | null;
  related_table_id?: string | null;
};

export type ReviewSummary = {
  breaking: number;
  caution: number;
  warning: number;
  info: number;
};

export type ReviewRuleMetadata = {
  rule_id: string;
  default_severity: ReviewSeverity;
  description: string;
};

export type ReviewInputCoverage = {
  empty: boolean;
  unsupported_constructs: number;
};

export type WasmReviewResult = {
  review: {
    findings: ReviewFinding[];
    suppressed: ReviewFinding[];
    summary: ReviewSummary;
    applied_rules: string[];
  };
  diagnostics: WasmDiagnostic[];
  denied: boolean;
  content?: string | null;
  applied_rule_details: ReviewRuleMetadata[];
  requested_dialect: ReviewDialect;
  effective_dialect: ReviewDialect;
  inputs: {
    before: ReviewInputCoverage;
    after: ReviewInputCoverage;
  };
};

export type WasmErrorShape = {
  message: string;
  code?: string;
};

export type PersistedState = {
  example: ExampleId;
  mode: WorkbenchMode;
  theme: Theme;
  layout: LayoutAlgorithm;
  direction: LayoutDirection;
  edgeStyle: EdgeStyle;
  viewpoint: ViewpointId;
  groupBy: GroupBy;
  focusTable: string;
  depth: string;
  includeTables: string;
  excludeTables: string;
  exportFormat: ExportFormat;
  inspectTable: string;
  lintRules: string;
  compareView: CompareView;
  compareReviewDialect: ReviewDialect;
  sql: string;
  compareBeforeSql: string;
  compareAfterSql: string;
};

export type ExampleDefinition = {
  id: Exclude<ExampleId, 'custom'>;
  label: string;
  path: string;
};

export type ViewpointDefinition = {
  id: ViewpointId;
  label: string;
  description: string;
  groupBy: GroupBy;
  focusTable: string;
  depth: number;
  includeTables: readonly string[];
  excludeTables: readonly string[];
};

export type ManualViewState = {
  groupBy: GroupBy;
  focusTable: string;
  depth: string;
  includeTables: string;
  excludeTables: string;
};

export type ButtonAction = {
  label: string;
  run: () => void | Promise<void>;
};
