import initWasm, {
  diff_from_sql,
  export_from_sql,
  inspect_from_sql,
  lint_from_sql,
  render_from_sql,
  set_panic_hook,
  version,
} from "../pkg/relune_wasm.js";
import { createSqlEditor } from "./editor.js";

type ExampleId = "simple-blog" | "ecommerce" | "multi-schema" | "custom";
type Theme = "light" | "dark";
type LayoutAlgorithm = "hierarchical" | "force-directed";
type LayoutDirection = "top-to-bottom" | "left-to-right" | "right-to-left" | "bottom-to-top";
type EdgeStyle = "curved" | "orthogonal" | "straight";
type GroupBy = "none" | "schema" | "prefix";
type WorkbenchMode = "render" | "inspect" | "export" | "lint" | "compare";
type ExportFormat = "schema-json" | "graph-json" | "layout-json" | "mermaid" | "d2" | "dot";
type CompareView = "visual" | "text" | "markdown" | "json";
type ViewpointId = string;
type WasmSeverity = "error" | "warning" | "info" | "hint";

type WasmDiagnosticCode = {
  prefix: string;
  number: number;
};

type WasmDiagnostic = {
  severity: WasmSeverity;
  code: WasmDiagnosticCode;
  message: string;
};

type WasmDuration = {
  secs: number;
  nanos: number;
};

type WasmRenderStats = {
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

type WasmRenderResult = {
  content: string;
  diagnostics: WasmDiagnostic[];
  stats: WasmRenderStats;
};

type SchemaStats = {
  table_count: number;
  column_count: number;
  foreign_key_count: number;
  view_count: number;
};

type TableSummary = {
  name: string;
  column_count: number;
  foreign_key_count: number;
  incoming_fk_count: number;
  index_count: number;
  has_primary_key: boolean;
};

type SchemaSummary = {
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

type ColumnDetails = {
  name: string;
  data_type: string;
  nullable: boolean;
  is_primary_key: boolean;
  comment?: string | null;
};

type ForeignKeyDetails = {
  name?: string | null;
  from_columns: string[];
  to_table: string;
  to_columns: string[];
  on_delete?: string | null;
  on_update?: string | null;
};

type IndexDetails = {
  name?: string | null;
  columns: string[];
  is_unique: boolean;
};

type TableDetails = {
  name: string;
  comment?: string | null;
  columns: ColumnDetails[];
  foreign_keys: ForeignKeyDetails[];
  indexes: IndexDetails[];
};

type WasmInspectResult = {
  summary: SchemaSummary;
  table?: TableDetails | null;
  diagnostics: WasmDiagnostic[];
};

type WasmExportResult = {
  content: string;
  diagnostics: WasmDiagnostic[];
  stats: SchemaStats;
};

type LintStats = {
  total: number;
  errors: number;
  warnings: number;
  infos: number;
  hints: number;
};

type LintIssue = {
  rule_id: string;
  category: string;
  severity: WasmSeverity;
  message: string;
  table_id?: string | null;
  table_name?: string | null;
  column_name?: string | null;
  hint?: string | null;
};

type WasmLintResult = {
  issues: LintIssue[];
  stats: LintStats;
  diagnostics: WasmDiagnostic[];
};

type DiffSummary = {
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

type TableDiff = {
  table_name: string;
  column_diffs: unknown[];
  fk_diffs: unknown[];
  index_diffs: unknown[];
};

type ViewDiff = {
  view_name: string;
  column_diffs: unknown[];
};

type EnumDiff = {
  enum_name: string;
  value_diffs: unknown[];
};

type SchemaDiff = {
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

type WasmDiffResult = {
  diff: SchemaDiff;
  diagnostics: WasmDiagnostic[];
  rendered?: string | null;
  content?: string | null;
};

type WasmErrorShape = {
  message: string;
  code?: string;
};

type PersistedState = {
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
  sql: string;
  compareBeforeSql: string;
  compareAfterSql: string;
};

type ExampleDefinition = {
  id: Exclude<ExampleId, "custom">;
  label: string;
  path: string;
};

type ViewpointDefinition = {
  id: ViewpointId;
  label: string;
  description: string;
  groupBy: GroupBy;
  focusTable: string;
  depth: number;
  includeTables: readonly string[];
  excludeTables: readonly string[];
};

type ManualViewState = {
  groupBy: GroupBy;
  focusTable: string;
  depth: string;
  includeTables: string;
  excludeTables: string;
};

type ButtonAction = {
  label: string;
  run: () => void | Promise<void>;
};

const STORAGE_KEY = "relune-schema-workbench:v1";
const CUSTOM_EXAMPLE_ID = "custom";
const DEFAULT_EXAMPLE_ID: Exclude<ExampleId, "custom"> = "simple-blog";
const MANUAL_VIEWPOINT_ID = "";
const MANUAL_VIEWPOINT_LABEL = "Manual controls";
const SIDEBAR_DEFAULT = 380;

const DEFAULT_MANUAL_VIEW_STATE: Readonly<ManualViewState> = {
  groupBy: "none",
  focusTable: "",
  depth: "1",
  includeTables: "",
  excludeTables: "",
};

const EXAMPLES: readonly ExampleDefinition[] = [
  {
    id: "simple-blog",
    label: "Simple Blog",
    path: "./examples/simple_blog.sql",
  },
  {
    id: "ecommerce",
    label: "Ecommerce",
    path: "./examples/ecommerce.sql",
  },
  {
    id: "multi-schema",
    label: "Multi Schema",
    path: "./examples/multi_schema.sql",
  },
] as const;

const VIEWPOINTS: Record<Exclude<ExampleId, "custom">, readonly ViewpointDefinition[]> = {
  "simple-blog": [
    {
      id: "authoring",
      label: "Authoring",
      description: "Posts with their authors and comments.",
      groupBy: "none",
      focusTable: "posts",
      depth: 1,
      includeTables: ["users", "posts", "comments"],
      excludeTables: [],
    },
    {
      id: "community",
      label: "Community",
      description: "Comment moderation around users and posts.",
      groupBy: "none",
      focusTable: "comments",
      depth: 1,
      includeTables: ["comments", "posts", "users"],
      excludeTables: [],
    },
  ],
  ecommerce: [
    {
      id: "billing",
      label: "Billing",
      description: "Orders, payments, and purchased products.",
      groupBy: "none",
      focusTable: "orders",
      depth: 1,
      includeTables: ["orders", "order_items", "payments", "products", "users"],
      excludeTables: [],
    },
    {
      id: "catalog",
      label: "Catalog",
      description: "Products and categories around order lines.",
      groupBy: "none",
      focusTable: "products",
      depth: 1,
      includeTables: ["products", "categories", "order_items"],
      excludeTables: ["payments"],
    },
  ],
  "multi-schema": [
    {
      id: "sales",
      label: "Sales",
      description: "Schema-scoped sales flow with grouped tables.",
      groupBy: "schema",
      focusTable: "sales.orders",
      depth: 1,
      includeTables: ["sales.*"],
      excludeTables: [],
    },
    {
      id: "inventory",
      label: "Inventory",
      description: "Products and stock relationships inside the inventory schema.",
      groupBy: "schema",
      focusTable: "inventory.products",
      depth: 1,
      includeTables: ["inventory.*"],
      excludeTables: [],
    },
  ],
};

const MODE_META: Record<
  WorkbenchMode,
  {
    label: string;
    title: string;
    description: string;
    actionLabel: string;
  }
> = {
  render: {
    label: "Render",
    title: "Schema Preview",
    description: "Render deterministic viewer HTML and SVG with the same WASM engine as the CLI.",
    actionLabel: "Render",
  },
  inspect: {
    label: "Inspect",
    title: "Schema Inventory",
    description: "Explore schema counts, table inventory, and table details from one SQL input.",
    actionLabel: "Inspect",
  },
  export: {
    label: "Export",
    title: "Portable Schema Outputs",
    description: "Export normalized schema data, graph data, and documentation-friendly sources.",
    actionLabel: "Export",
  },
  lint: {
    label: "Lint",
    title: "Schema Review",
    description:
      "Review lint issues and parser diagnostics as a schema workbench, not just a demo viewer.",
    actionLabel: "Lint",
  },
  compare: {
    label: "Compare",
    title: "Before / After Diff",
    description:
      "Keep before and after separate, then inspect visual or structured changes without crowding the viewer mode.",
    actionLabel: "Compare",
  },
};

const DEFAULT_STATE: PersistedState = {
  example: DEFAULT_EXAMPLE_ID,
  mode: "render",
  theme: "light",
  layout: "hierarchical",
  direction: "top-to-bottom",
  edgeStyle: "curved",
  viewpoint: MANUAL_VIEWPOINT_ID,
  groupBy: "none",
  focusTable: "",
  depth: "1",
  includeTables: "",
  excludeTables: "",
  exportFormat: "schema-json",
  inspectTable: "",
  lintRules: "",
  compareView: "visual",
  sql: "",
  compareBeforeSql: "",
  compareAfterSql: "",
};

const exampleSelect = getElement<HTMLSelectElement>("example-select");
const themeSelect = getElement<HTMLSelectElement>("theme-select");
const layoutSelect = getElement<HTMLSelectElement>("layout-select");
const directionSelect = getElement<HTMLSelectElement>("direction-select");
const edgeStyleSelect = getElement<HTMLSelectElement>("edge-style-select");
const groupBySelect = getElement<HTMLSelectElement>("group-by-select");
const viewpointSelect = getElement<HTMLSelectElement>("viewpoint-select");
const viewpointHint = getElement<HTMLElement>("viewpoint-hint");
const focusTableInput = getElement<HTMLInputElement>("focus-table-input");
const depthInput = getElement<HTMLInputElement>("depth-input");
const includeTablesInput = getElement<HTMLInputElement>("include-tables-input");
const excludeTablesInput = getElement<HTMLInputElement>("exclude-tables-input");
const inspectTableSelect = getElement<HTMLSelectElement>("inspect-table-select");
const exportFormatSelect = getElement<HTMLSelectElement>("export-format-select");
const lintRulesInput = getElement<HTMLInputElement>("lint-rules-input");
const compareFormatSelect = getElement<HTMLSelectElement>("compare-format-select");

const appearanceSection = getElement<HTMLElement>("appearance-section");
const layoutSection = getElement<HTMLElement>("layout-section");
const scopeSection = getElement<HTMLElement>("scope-section");
const inspectSection = getElement<HTMLElement>("inspect-section");
const exportSection = getElement<HTMLElement>("export-section");
const lintSection = getElement<HTMLElement>("lint-section");
const compareSection = getElement<HTMLElement>("compare-section");
const viewpointRow = getElement<HTMLElement>("viewpoint-row");
const focusRow = getElement<HTMLElement>("focus-row");

const sqlEditorMount = getElement<HTMLDivElement>("sql-input");
const compareBeforeMount = getElement<HTMLDivElement>("compare-before-input");
const compareAfterMount = getElement<HTMLDivElement>("compare-after-input");
const sqlEditor = createSqlEditor(sqlEditorMount);
const compareBeforeEditor = createSqlEditor(compareBeforeMount);
const compareAfterEditor = createSqlEditor(compareAfterMount);

const singleEditorSection = getElement<HTMLElement>("single-editor-section");
const compareEditorsSection = getElement<HTMLElement>("compare-editors-section");
const modeHint = getElement<HTMLElement>("mode-hint");
const modeTabs = Array.from(document.querySelectorAll<HTMLButtonElement>(".mode-tab"));

const resetExampleButton = getElement<HTMLButtonElement>("reset-example");
const renderNowButton = getElement<HTMLButtonElement>("render-now");
const copyOutputButton = getElement<HTMLButtonElement>("copy-output");
const downloadPrimaryButton = getElement<HTMLButtonElement>("download-html");
const downloadSecondaryButton = getElement<HTMLButtonElement>("download-svg");
const renderStatus = getElement<HTMLElement>("render-status");
const versionPill = getElement<HTMLElement>("version-pill");
const errorBox = getElement<HTMLElement>("error-box");
const statsGrid = getElement<HTMLElement>("stats-grid");
const diagnosticCount = getElement<HTMLElement>("diagnostic-count");
const diagnosticList = getElement<HTMLUListElement>("diagnostic-list");
const previewPanel = getElement<HTMLElement>("preview-panel");
const previewFrame = getElement<HTMLIFrameElement>("preview-frame");

const surfaceEyebrow = getElement<HTMLElement>("surface-eyebrow");
const surfaceTitle = getElement<HTMLElement>("surface-title");
const surfaceDescription = getElement<HTMLElement>("surface-description");
const inspectPanel = getElement<HTMLElement>("inspect-panel");
const inspectTableList = getElement<HTMLUListElement>("inspect-table-list");
const inspectDetail = getElement<HTMLElement>("inspect-detail");
const lintPanel = getElement<HTMLElement>("lint-panel");
const lintIssueList = getElement<HTMLUListElement>("lint-issue-list");
const compareSummaryPanel = getElement<HTMLElement>("compare-summary-panel");
const compareSummary = getElement<HTMLElement>("compare-summary");
const compareSummaryCount = getElement<HTMLElement>("compare-summary-count");
const compareObjectList = getElement<HTMLUListElement>("compare-object-list");
const textOutputPanel = getElement<HTMLElement>("text-output-panel");
const textOutputLabel = getElement<HTMLElement>("text-output-label");
const textOutputMeta = getElement<HTMLElement>("text-output-meta");
const textOutput = getElement<HTMLElement>("text-output");

const sidebar = getElement<HTMLElement>("sidebar");
const sidebarHandle = getElement<HTMLElement>("sidebar-handle");
const sidebarCollapseButton = getElement<HTMLButtonElement>("sidebar-collapse");
const sidebarExpandButton = getElement<HTMLButtonElement>("sidebar-expand");
const editorExpandButton = getElement<HTMLButtonElement>("editor-expand");
const sidebarScroll = document.querySelector<HTMLElement>(".sidebar__scroll")!;

const exampleSql = new Map<Exclude<ExampleId, "custom">, string>();
const manualViewStateByExample = new Map<ExampleId, ManualViewState>();

let currentMode: WorkbenchMode = DEFAULT_STATE.mode;
let renderTimer = 0;
let renderSerial = 0;
let previousViewpointId = MANUAL_VIEWPOINT_ID;
let isApplyingViewpoint = false;
let primaryAction: ButtonAction | null = null;
let secondaryAction: ButtonAction | null = null;
let copyAction: ButtonAction | null = null;

populateExampleOptions();

void bootstrap();

async function bootstrap(): Promise<void> {
  try {
    setStatus("Loading WASM runtime…");
    setActionButtonsDisabled(true);
    await loadExamples();
    restoreInitialState();
    await initWasm();
    set_panic_hook();
    versionPill.textContent = `relune-wasm ${version()}`;
    setActionButtonsDisabled(false);
    scheduleWorkbench(0);
  } catch (error) {
    showError(normalizeError(error));
    setStatus("Runtime failed");
  }
}

function populateExampleOptions(): void {
  const options = EXAMPLES.map(
    (example) => `<option value="${example.id}">${escapeHtml(example.label)}</option>`,
  );
  options.push(`<option value="${CUSTOM_EXAMPLE_ID}">Custom</option>`);
  exampleSelect.innerHTML = options.join("");
}

function getViewpoints(example: ExampleId): readonly ViewpointDefinition[] {
  if (example === CUSTOM_EXAMPLE_ID) {
    return [];
  }

  return VIEWPOINTS[toBuiltinExampleId(example)];
}

function findViewpoint(
  example: ExampleId,
  viewpointId: ViewpointId,
): ViewpointDefinition | undefined {
  return getViewpoints(example).find((viewpoint) => viewpoint.id === viewpointId);
}

function populateViewpointOptions(example: ExampleId, preferredViewpoint: ViewpointId): void {
  const viewpoints = getViewpoints(example);
  const options = [
    `<option value="${MANUAL_VIEWPOINT_ID}">${MANUAL_VIEWPOINT_LABEL}</option>`,
    ...viewpoints.map(
      (viewpoint) => `<option value="${viewpoint.id}">${escapeHtml(viewpoint.label)}</option>`,
    ),
  ];

  viewpointSelect.innerHTML = options.join("");
  viewpointSelect.disabled = viewpoints.length === 0;
  viewpointSelect.value = findViewpoint(example, preferredViewpoint)?.id ?? MANUAL_VIEWPOINT_ID;
  updateViewpointHint();
}

function updateViewpointHint(): void {
  const viewpoints = getViewpoints(exampleSelect.value as ExampleId);
  const selected = getSelectedViewpoint();

  if (selected) {
    viewpointHint.textContent = selected.description;
    return;
  }

  viewpointHint.textContent =
    viewpoints.length === 0
      ? "Manual controls edit focus, filters, and grouping directly for custom SQL."
      : "Built-in presets are scoped to the selected example. Switch to Manual controls to edit focus, filters, and grouping yourself.";
}

function getSelectedViewpoint(): ViewpointDefinition | undefined {
  return findViewpoint(exampleSelect.value as ExampleId, viewpointSelect.value);
}

function serializePatterns(patterns: readonly string[]): string {
  return patterns.join(", ");
}

function cloneManualViewState(state: Readonly<ManualViewState>): ManualViewState {
  return { ...state };
}

function defaultManualViewState(): ManualViewState {
  return cloneManualViewState(DEFAULT_MANUAL_VIEW_STATE);
}

function readManualViewStateFromControls(): ManualViewState {
  return {
    groupBy: groupBySelect.value as GroupBy,
    focusTable: focusTableInput.value.trim(),
    depth: depthInput.value.trim() || DEFAULT_MANUAL_VIEW_STATE.depth,
    includeTables: includeTablesInput.value.trim(),
    excludeTables: excludeTablesInput.value.trim(),
  };
}

function storeManualViewState(example: ExampleId, state: Readonly<ManualViewState>): void {
  manualViewStateByExample.set(example, cloneManualViewState(state));
}

function restoreManualViewState(example: ExampleId): void {
  const state = manualViewStateByExample.get(example) ?? defaultManualViewState();
  isApplyingViewpoint = true;
  groupBySelect.value = state.groupBy;
  focusTableInput.value = state.focusTable;
  depthInput.value = state.depth;
  includeTablesInput.value = state.includeTables;
  excludeTablesInput.value = state.excludeTables;
  isApplyingViewpoint = false;
  updateViewpointHint();
}

function applyViewpoint(viewpoint: ViewpointDefinition): void {
  isApplyingViewpoint = true;
  groupBySelect.value = viewpoint.groupBy;
  focusTableInput.value = viewpoint.focusTable;
  depthInput.value = `${viewpoint.depth}`;
  includeTablesInput.value = serializePatterns(viewpoint.includeTables);
  excludeTablesInput.value = serializePatterns(viewpoint.excludeTables);
  isApplyingViewpoint = false;
  updateViewpointHint();
}

function currentViewSettingsMatch(viewpoint: ViewpointDefinition): boolean {
  return (
    groupBySelect.value === viewpoint.groupBy &&
    focusTableInput.value.trim() === viewpoint.focusTable &&
    depthInput.value.trim() === `${viewpoint.depth}` &&
    arraysEqual(splitPatterns(includeTablesInput.value), [...viewpoint.includeTables]) &&
    arraysEqual(splitPatterns(excludeTablesInput.value), [...viewpoint.excludeTables])
  );
}

function syncViewpointSelectionWithControls(): void {
  if (isApplyingViewpoint) {
    return;
  }

  const selected = getSelectedViewpoint();
  if (!selected) {
    updateViewpointHint();
    return;
  }

  if (!currentViewSettingsMatch(selected)) {
    viewpointSelect.value = MANUAL_VIEWPOINT_ID;
  }

  updateViewpointHint();
}

function arraysEqual(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

async function loadExamples(): Promise<void> {
  const loadedExamples = await Promise.all(
    EXAMPLES.map(async (example) => {
      const response = await fetch(example.path);
      if (!response.ok) {
        throw new Error(`Failed to load example: ${example.label}`);
      }
      return [example.id, await response.text()] as const;
    }),
  );

  for (const [id, sql] of loadedExamples) {
    exampleSql.set(id, sql.trim());
  }
}

function restoreInitialState(): void {
  const storedState = readStoredState();
  const queryState = readQueryState();
  const initialState: PersistedState = {
    ...DEFAULT_STATE,
    ...storedState,
    ...queryState,
  };

  if (initialState.example !== CUSTOM_EXAMPLE_ID) {
    initialState.sql = exampleSql.get(toBuiltinExampleId(initialState.example)) ?? "";
    if (!initialState.compareBeforeSql) {
      initialState.compareBeforeSql = initialState.sql;
    }
    if (!initialState.compareAfterSql) {
      initialState.compareAfterSql = initialState.sql;
    }
  }

  if (!initialState.sql) {
    initialState.sql = exampleSql.get(DEFAULT_EXAMPLE_ID) ?? "";
  }
  if (!initialState.compareBeforeSql) {
    initialState.compareBeforeSql = initialState.sql;
  }
  if (!initialState.compareAfterSql) {
    initialState.compareAfterSql = initialState.compareBeforeSql;
  }

  applyState(initialState);
  attachEventListeners();
  persistState();
}

function attachEventListeners(): void {
  exampleSelect.addEventListener("change", handleExampleChange);
  viewpointSelect.addEventListener("change", handleViewpointChange);
  renderNowButton.addEventListener("click", () => {
    void runWorkbench();
  });
  resetExampleButton.addEventListener("click", resetExample);
  copyOutputButton.addEventListener("click", () => {
    void copyAction?.run();
  });
  downloadPrimaryButton.addEventListener("click", () => {
    void primaryAction?.run();
  });
  downloadSecondaryButton.addEventListener("click", () => {
    void secondaryAction?.run();
  });

  const controls: readonly HTMLElement[] = [
    themeSelect,
    layoutSelect,
    directionSelect,
    edgeStyleSelect,
    groupBySelect,
    focusTableInput,
    depthInput,
    includeTablesInput,
    excludeTablesInput,
    inspectTableSelect,
    exportFormatSelect,
    lintRulesInput,
    compareFormatSelect,
  ];

  for (const control of controls) {
    control.addEventListener("change", handleControlChange);
    control.addEventListener("input", handleControlChange);
  }

  for (const tab of modeTabs) {
    tab.addEventListener("click", () => {
      const nextMode = tab.dataset.mode;
      if (isWorkbenchMode(nextMode)) {
        handleModeChange(nextMode);
      }
    });
  }

  sqlEditor.onUpdate(() => {
    syncExampleSelectionWithActiveEditors();
    handleEditorChange();
  });
  compareBeforeEditor.onUpdate(() => {
    syncExampleSelectionWithActiveEditors();
    handleEditorChange();
  });
  compareAfterEditor.onUpdate(() => {
    syncExampleSelectionWithActiveEditors();
    handleEditorChange();
  });

  sidebarCollapseButton.addEventListener("click", collapseSidebar);
  sidebarExpandButton.addEventListener("click", expandSidebar);
  editorExpandButton.addEventListener("click", toggleEditorExpand);
  initSidebarResize();
}

function handleModeChange(mode: WorkbenchMode): void {
  if (mode === currentMode) {
    return;
  }

  if (
    mode === "compare" &&
    !compareBeforeEditor.getValue().trim() &&
    !compareAfterEditor.getValue().trim()
  ) {
    const seedSql = sqlEditor.getValue().trim() || exampleSql.get(DEFAULT_EXAMPLE_ID) || "";
    compareBeforeEditor.setValue(seedSql);
    compareAfterEditor.setValue(seedSql);
  }

  currentMode = mode;
  applyModeVisibility();
  persistState();
  scheduleWorkbench(0);
}

function handleEditorChange(): void {
  if (currentMode !== "compare") {
    syncViewpointSelectionWithControls();
    if (viewpointSelect.value === MANUAL_VIEWPOINT_ID) {
      storeManualViewState(exampleSelect.value as ExampleId, readManualViewStateFromControls());
    }
    previousViewpointId = viewpointSelect.value;
  }

  persistState();
  scheduleWorkbench();
}

function handleControlChange(): void {
  if (currentMode !== "compare") {
    syncViewpointSelectionWithControls();
    if (viewpointSelect.value === MANUAL_VIEWPOINT_ID) {
      storeManualViewState(exampleSelect.value as ExampleId, readManualViewStateFromControls());
    }
    previousViewpointId = viewpointSelect.value;
  }

  applyTheme(themeSelect.value as Theme);
  applyModeVisibility();
  persistState();
  scheduleWorkbench();
}

function handleExampleChange(): void {
  populateViewpointOptions(exampleSelect.value as ExampleId, viewpointSelect.value);

  const selectedViewpoint = getSelectedViewpoint();
  if (selectedViewpoint) {
    applyViewpoint(selectedViewpoint);
  } else {
    restoreManualViewState(exampleSelect.value as ExampleId);
  }

  if (exampleSelect.value !== CUSTOM_EXAMPLE_ID) {
    const sql = exampleSql.get(exampleSelect.value as Exclude<ExampleId, "custom">);
    if (sql) {
      syncEditorsWithExampleSql(sql);
    }
  }

  previousViewpointId = viewpointSelect.value;
  persistState();
  scheduleWorkbench(0);
}

function handleViewpointChange(): void {
  const selected = getSelectedViewpoint();
  if (selected) {
    if (previousViewpointId === MANUAL_VIEWPOINT_ID) {
      storeManualViewState(exampleSelect.value as ExampleId, readManualViewStateFromControls());
    }
    applyViewpoint(selected);
  } else {
    restoreManualViewState(exampleSelect.value as ExampleId);
  }

  previousViewpointId = viewpointSelect.value;
  persistState();
  scheduleWorkbench();
}

function resetExample(): void {
  const selectedExample = exampleSelect.value as ExampleId;
  const effectiveExample = toBuiltinExampleId(selectedExample);
  exampleSelect.value = effectiveExample;
  populateViewpointOptions(effectiveExample, viewpointSelect.value);
  const selectedViewpoint = getSelectedViewpoint();

  if (selectedViewpoint) {
    applyViewpoint(selectedViewpoint);
  } else {
    restoreManualViewState(effectiveExample);
  }

  const sql = exampleSql.get(effectiveExample) ?? "";
  syncEditorsWithExampleSql(sql);

  previousViewpointId = viewpointSelect.value;
  persistState();
  scheduleWorkbench(0);
}

function collapseSidebar(): void {
  sidebar.classList.add("is-collapsed");
}

function expandSidebar(): void {
  sidebar.classList.remove("is-collapsed");
  sidebar.style.width = `${SIDEBAR_DEFAULT}px`;
}

function toggleEditorExpand(): void {
  const expanded = sidebarScroll.classList.toggle("is-editor-expanded");
  editorExpandButton.setAttribute("aria-label", expanded ? "Collapse editor" : "Expand editor");
  editorExpandButton.setAttribute("title", expanded ? "Collapse editor" : "Expand editor");
}

function initSidebarResize(): void {
  let dragging = false;
  let startX = 0;
  let startWidth = 0;

  function onPointerDown(event: PointerEvent): void {
    if (sidebar.classList.contains("is-collapsed")) {
      sidebar.classList.remove("is-collapsed");
      sidebar.style.width = `${SIDEBAR_DEFAULT}px`;
      return;
    }

    dragging = true;
    startX = event.clientX;
    startWidth = sidebar.getBoundingClientRect().width;

    sidebar.style.transition = "none";
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    previewFrame.style.pointerEvents = "none";

    sidebarHandle.setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: PointerEvent): void {
    if (!dragging) {
      return;
    }

    const delta = event.clientX - startX;
    const newWidth = Math.max(startWidth + delta, 0);
    sidebar.style.width = `${newWidth}px`;
  }

  function onPointerUp(): void {
    if (!dragging) {
      return;
    }

    dragging = false;
    sidebar.style.transition = "";
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    previewFrame.style.pointerEvents = "";
  }

  function onDoubleClick(): void {
    sidebar.classList.remove("is-collapsed");
    sidebar.style.width = `${SIDEBAR_DEFAULT}px`;
  }

  sidebarHandle.addEventListener("pointerdown", onPointerDown);
  sidebarHandle.addEventListener("pointermove", onPointerMove);
  sidebarHandle.addEventListener("pointerup", onPointerUp);
  sidebarHandle.addEventListener("pointercancel", onPointerUp);
  sidebarHandle.addEventListener("dblclick", onDoubleClick);
}

function syncExampleSelectionWithActiveEditors(): void {
  if (exampleSelect.value === CUSTOM_EXAMPLE_ID) {
    return;
  }

  const selectedSql = exampleSql.get(toBuiltinExampleId(exampleSelect.value as ExampleId));
  if (!selectedSql) {
    return;
  }

  const builtinSql = selectedSql.trim();
  const matchesExample =
    currentMode === "compare"
      ? compareBeforeEditor.getValue().trim() === builtinSql &&
        compareAfterEditor.getValue().trim() === builtinSql
      : sqlEditor.getValue().trim() === builtinSql;

  if (!matchesExample) {
    exampleSelect.value = CUSTOM_EXAMPLE_ID;
    populateViewpointOptions(CUSTOM_EXAMPLE_ID, MANUAL_VIEWPOINT_ID);
    previousViewpointId = MANUAL_VIEWPOINT_ID;
  }
}

function applyState(state: PersistedState): void {
  currentMode = state.mode;
  exampleSelect.value = state.example;
  themeSelect.value = state.theme;
  layoutSelect.value = state.layout;
  directionSelect.value = state.direction;
  edgeStyleSelect.value = state.edgeStyle;
  exportFormatSelect.value = state.exportFormat;
  inspectTableSelect.value = state.inspectTable;
  lintRulesInput.value = state.lintRules;
  compareFormatSelect.value = state.compareView;

  populateViewpointOptions(state.example, state.viewpoint);
  const selectedViewpoint = getSelectedViewpoint();
  if (selectedViewpoint) {
    applyViewpoint(selectedViewpoint);
  } else {
    storeManualViewState(state.example, {
      groupBy: state.groupBy,
      focusTable: state.focusTable,
      depth: state.depth || DEFAULT_MANUAL_VIEW_STATE.depth,
      includeTables: state.includeTables,
      excludeTables: state.excludeTables,
    });
    restoreManualViewState(state.example);
  }

  sqlEditor.setValue(state.sql);
  compareBeforeEditor.setValue(state.compareBeforeSql);
  compareAfterEditor.setValue(state.compareAfterSql);
  applyTheme(state.theme);
  previousViewpointId = viewpointSelect.value;
  applyModeVisibility();
}

function applyModeVisibility(): void {
  const modeMeta = MODE_META[currentMode];
  surfaceEyebrow.textContent = modeMeta.label;
  surfaceTitle.textContent = modeMeta.title;
  surfaceDescription.textContent = modeMeta.description;
  modeHint.textContent = modeMeta.description;
  renderNowButton.textContent = modeMeta.actionLabel;

  for (const tab of modeTabs) {
    tab.classList.toggle("is-active", tab.dataset.mode === currentMode);
    tab.setAttribute("aria-selected", `${tab.dataset.mode === currentMode}`);
  }

  const usesVisualControls =
    currentMode === "render" || currentMode === "export" || currentMode === "compare";
  const usesScopeControls =
    currentMode === "render" || currentMode === "export" || currentMode === "compare";

  appearanceSection.hidden = !usesVisualControls;
  layoutSection.hidden = !usesVisualControls;
  scopeSection.hidden = !usesScopeControls;
  inspectSection.hidden = currentMode !== "inspect";
  exportSection.hidden = currentMode !== "export";
  lintSection.hidden = currentMode !== "lint";
  compareSection.hidden = currentMode !== "compare";
  viewpointRow.hidden = !(currentMode === "render" || currentMode === "export");
  focusRow.hidden = currentMode === "compare";

  singleEditorSection.hidden = currentMode === "compare";
  compareEditorsSection.hidden = currentMode !== "compare";

  if (currentMode === "compare") {
    previewFrame.title = "Relune diff preview";
  } else if (currentMode === "render") {
    previewFrame.title = "Relune HTML preview";
  } else {
    previewFrame.title = "Relune workbench output";
  }
}

function scheduleWorkbench(delay = 250): void {
  window.clearTimeout(renderTimer);
  renderTimer = window.setTimeout(() => {
    void runWorkbench();
  }, delay);
}

async function runWorkbench(): Promise<void> {
  const currentSerial = ++renderSerial;
  clearError();
  resetOutputPanels();

  try {
    switch (currentMode) {
      case "render":
        await runRenderMode(currentSerial);
        break;
      case "inspect":
        await runInspectMode(currentSerial);
        break;
      case "export":
        await runExportMode(currentSerial);
        break;
      case "lint":
        await runLintMode(currentSerial);
        break;
      case "compare":
        await runCompareMode(currentSerial);
        break;
    }
  } catch (error) {
    if (currentSerial !== renderSerial) {
      return;
    }

    renderMetricCards([]);
    renderDiagnostics([]);
    resetActions();
    showError(normalizeError(error));
    setStatus(`${MODE_META[currentMode].actionLabel} failed`);
  }
}

async function runRenderMode(currentSerial: number): Promise<void> {
  const sql = sqlEditor.getValue().trim();
  if (!sql) {
    showError({ message: "SQL input is empty." });
    setStatus("Waiting for SQL");
    return;
  }

  setStatus("Rendering…");
  const result = render_from_sql(buildRenderRequest("html")) as WasmRenderResult;
  if (currentSerial !== renderSerial) {
    return;
  }

  previewPanel.hidden = false;
  previewFrame.srcdoc = result.content;
  renderMetricCards([
    ["Tables", `${result.stats.table_count}`],
    ["Columns", `${result.stats.column_count}`],
    ["Edges", `${result.stats.edge_count}`],
    ["Views", `${result.stats.view_count}`],
    ["Parse", formatDuration(result.stats.parse_time)],
    ["Graph", formatDuration(result.stats.graph_time)],
    ["Layout", formatDuration(result.stats.layout_time)],
    ["Render", formatDuration(result.stats.render_time)],
  ]);
  renderDiagnostics(result.diagnostics);
  configureActions({
    copy: {
      label: "Copy HTML",
      run: () => copyText(result.content, "Copied HTML"),
    },
    primary: {
      label: "HTML",
      run: () => downloadText("relune-workbench.html", result.content, "text/html;charset=utf-8"),
    },
    secondary: {
      label: "SVG",
      run: () => {
        const svgResult = render_from_sql(buildRenderRequest("svg")) as { content: string };
        downloadText("relune-workbench.svg", svgResult.content, "image/svg+xml;charset=utf-8");
      },
    },
  });
  setStatus(`Rendered in ${formatDuration(result.stats.total_time)}`);
}

async function runInspectMode(currentSerial: number): Promise<void> {
  const sql = sqlEditor.getValue().trim();
  if (!sql) {
    showError({ message: "SQL input is empty." });
    setStatus("Waiting for SQL");
    return;
  }

  setStatus("Inspecting…");
  const summaryResult = inspect_from_sql({
    sql: sqlEditor.getValue(),
    format: "json",
  }) as WasmInspectResult;
  if (currentSerial !== renderSerial) {
    return;
  }

  const tableNames = summaryResult.summary.tables.map((table) => table.name);
  const selectedTable = tableNames.includes(inspectTableSelect.value)
    ? inspectTableSelect.value
    : "";
  populateInspectTableOptions(tableNames, selectedTable);

  const detailResult =
    selectedTable.length > 0
      ? (inspect_from_sql({
          sql: sqlEditor.getValue(),
          table: selectedTable,
          format: "json",
        }) as WasmInspectResult)
      : summaryResult;
  if (currentSerial !== renderSerial) {
    return;
  }

  inspectPanel.hidden = false;
  renderMetricCards([
    ["Tables", `${summaryResult.summary.table_count}`],
    ["Columns", `${summaryResult.summary.column_count}`],
    ["FKs", `${summaryResult.summary.foreign_key_count}`],
    ["Indexes", `${summaryResult.summary.index_count}`],
    ["Views", `${summaryResult.summary.view_count}`],
    ["Enums", `${summaryResult.summary.enum_count}`],
    ["No PK", `${summaryResult.summary.tables_without_pk}`],
    ["Isolated", `${summaryResult.summary.orphan_table_count}`],
  ]);
  renderDiagnostics(detailResult.diagnostics);
  renderInspectPanel(summaryResult.summary, detailResult.table ?? null, selectedTable);
  const inspectJson = JSON.stringify(detailResult, null, 2);
  configureActions({
    copy: {
      label: "Copy JSON",
      run: () => copyText(inspectJson, "Copied inspect JSON"),
    },
    primary: {
      label: "JSON",
      run: () => downloadText("relune-inspect.json", inspectJson, "application/json;charset=utf-8"),
    },
  });
  setStatus(selectedTable ? `Inspected ${selectedTable}` : "Inspected schema");
}

async function runExportMode(currentSerial: number): Promise<void> {
  const sql = sqlEditor.getValue().trim();
  if (!sql) {
    showError({ message: "SQL input is empty." });
    setStatus("Waiting for SQL");
    return;
  }

  setStatus("Exporting…");
  const result = export_from_sql(buildExportRequest()) as WasmExportResult;
  if (currentSerial !== renderSerial) {
    return;
  }

  renderMetricCards([
    ["Tables", `${result.stats.table_count}`],
    ["Columns", `${result.stats.column_count}`],
    ["FKs", `${result.stats.foreign_key_count}`],
    ["Views", `${result.stats.view_count}`],
  ]);
  renderDiagnostics(result.diagnostics);
  showTextOutput(exportFormatLabel(), result.content);
  configureActions({
    copy: {
      label: "Copy output",
      run: () => copyText(result.content, "Copied export output"),
    },
    primary: {
      label: "Download",
      run: () => downloadText(exportFilename(), result.content, exportMimeType()),
    },
  });
  setStatus(`Exported ${exportFormatLabel()}`);
}

async function runLintMode(currentSerial: number): Promise<void> {
  const sql = sqlEditor.getValue().trim();
  if (!sql) {
    showError({ message: "SQL input is empty." });
    setStatus("Waiting for SQL");
    return;
  }

  setStatus("Linting…");
  const result = lint_from_sql({
    sql: sqlEditor.getValue(),
    format: "json",
    rules: splitPatterns(lintRulesInput.value),
  }) as WasmLintResult;
  if (currentSerial !== renderSerial) {
    return;
  }

  lintPanel.hidden = false;
  renderMetricCards([
    ["Total", `${result.stats.total}`],
    ["Errors", `${result.stats.errors}`],
    ["Warnings", `${result.stats.warnings}`],
    ["Info", `${result.stats.infos}`],
    ["Hints", `${result.stats.hints}`],
  ]);
  renderDiagnostics(result.diagnostics);
  renderLintPanel(result.issues);
  const lintJson = JSON.stringify(result, null, 2);
  configureActions({
    copy: {
      label: "Copy JSON",
      run: () => copyText(lintJson, "Copied lint JSON"),
    },
    primary: {
      label: "JSON",
      run: () => downloadText("relune-lint.json", lintJson, "application/json;charset=utf-8"),
    },
  });
  setStatus(result.stats.total === 0 ? "No lint issues" : `${result.stats.total} issues found`);
}

async function runCompareMode(currentSerial: number): Promise<void> {
  const beforeSql = compareBeforeEditor.getValue().trim();
  const afterSql = compareAfterEditor.getValue().trim();
  if (!beforeSql || !afterSql) {
    showError({ message: "Both before and after SQL inputs are required." });
    setStatus("Waiting for before / after SQL");
    return;
  }

  const compareView = compareFormatSelect.value as CompareView;
  setStatus("Comparing…");
  const result = diff_from_sql(buildDiffRequest(compareView)) as WasmDiffResult;
  if (currentSerial !== renderSerial) {
    return;
  }

  renderMetricCards([
    ["Tables +", `${result.diff.summary.tables_added}`],
    ["Tables -", `${result.diff.summary.tables_removed}`],
    ["Tables ~", `${result.diff.summary.tables_modified}`],
    ["Columns", `${result.diff.summary.columns_changed}`],
    ["FKs", `${result.diff.summary.foreign_keys_changed}`],
    ["Indexes", `${result.diff.summary.indexes_changed}`],
    [
      "Views",
      `${result.diff.summary.views_added + result.diff.summary.views_removed + result.diff.summary.views_modified}`,
    ],
    [
      "Enums",
      `${result.diff.summary.enums_added + result.diff.summary.enums_removed + result.diff.summary.enums_modified}`,
    ],
  ]);
  renderDiagnostics(result.diagnostics);
  renderCompareSummary(result.diff);

  if (compareView === "visual") {
    previewPanel.hidden = false;
    previewFrame.srcdoc = result.rendered ?? "";
    configureActions({
      copy: {
        label: "Copy HTML",
        run: () => copyText(result.rendered ?? "", "Copied diff HTML"),
      },
      primary: {
        label: "HTML",
        run: () =>
          downloadText("relune-diff.html", result.rendered ?? "", "text/html;charset=utf-8"),
      },
    });
  } else if (compareView === "json") {
    const diffJson = result.content ?? JSON.stringify(result, null, 2);
    showTextOutput("Structured diff JSON", diffJson);
    configureActions({
      copy: {
        label: "Copy JSON",
        run: () => copyText(diffJson, "Copied diff JSON"),
      },
      primary: {
        label: "JSON",
        run: () => downloadText("relune-diff.json", diffJson, "application/json;charset=utf-8"),
      },
    });
  } else {
    const diffText = result.content ?? "";
    showTextOutput(compareView === "markdown" ? "Diff markdown" : "Diff text", diffText);
    configureActions({
      copy: {
        label: compareView === "markdown" ? "Copy markdown" : "Copy text",
        run: () =>
          copyText(
            diffText,
            compareView === "markdown" ? "Copied diff markdown" : "Copied diff text",
          ),
      },
      primary: {
        label: compareView === "markdown" ? "MD" : "TXT",
        run: () =>
          downloadText(
            compareView === "markdown" ? "relune-diff.md" : "relune-diff.txt",
            diffText,
            "text/plain;charset=utf-8",
          ),
      },
    });
  }

  const totalChanges = totalDiffChanges(result.diff.summary);
  setStatus(totalChanges === 0 ? "No changes detected" : `${totalChanges} changes detected`);
}

function buildRenderRequest(format: "html" | "svg"): Record<string, unknown> {
  const focusTable = focusTableInput.value.trim();
  const depth = parsePositiveInteger(depthInput.value);

  return {
    sql: sqlEditor.getValue(),
    format,
    theme: themeSelect.value as Theme,
    layoutAlgorithm: layoutSelect.value as LayoutAlgorithm,
    layoutDirection: directionSelect.value as LayoutDirection,
    edgeStyle: edgeStyleSelect.value as EdgeStyle,
    groupBy: groupBySelect.value as GroupBy,
    focusTable: focusTable || undefined,
    depth: focusTable ? depth : undefined,
    includeTables: splitPatterns(includeTablesInput.value),
    excludeTables: splitPatterns(excludeTablesInput.value),
    showLegend: false,
    showStats: false,
  };
}

function buildExportRequest(): Record<string, unknown> {
  const focusTable = focusTableInput.value.trim();
  const depth = parsePositiveInteger(depthInput.value);

  return {
    sql: sqlEditor.getValue(),
    format: exportFormatSelect.value as ExportFormat,
    groupBy: groupBySelect.value as GroupBy,
    focusTable: focusTable || undefined,
    depth: focusTable ? depth : undefined,
    includeTables: splitPatterns(includeTablesInput.value),
    excludeTables: splitPatterns(excludeTablesInput.value),
    layoutAlgorithm: layoutSelect.value as LayoutAlgorithm,
    layoutDirection: directionSelect.value as LayoutDirection,
    edgeStyle: edgeStyleSelect.value as EdgeStyle,
  };
}

function buildDiffRequest(compareView: CompareView): Record<string, unknown> {
  const format =
    compareView === "visual"
      ? "html"
      : compareView === "markdown"
        ? "markdown"
        : compareView === "text"
          ? "text"
          : "json";

  return {
    beforeSql: compareBeforeEditor.getValue(),
    afterSql: compareAfterEditor.getValue(),
    format,
    theme: themeSelect.value as Theme,
    layoutAlgorithm: layoutSelect.value as LayoutAlgorithm,
    layoutDirection: directionSelect.value as LayoutDirection,
    edgeStyle: edgeStyleSelect.value as EdgeStyle,
    groupBy: groupBySelect.value as GroupBy,
    includeTables: splitPatterns(includeTablesInput.value),
    excludeTables: splitPatterns(excludeTablesInput.value),
    showLegend: false,
    showStats: false,
  };
}

function syncEditorsWithExampleSql(sql: string): void {
  sqlEditor.setValue(sql);
  compareBeforeEditor.setValue(sql);
  compareAfterEditor.setValue(sql);
}

function renderInspectPanel(
  summary: SchemaSummary,
  table: TableDetails | null,
  selectedTable: string,
): void {
  inspectTableList.innerHTML =
    summary.tables.length === 0
      ? '<li class="issue-card"><p class="empty-state">No tables detected.</p></li>'
      : summary.tables
          .map(
            (item) => `
              <li>
                <button
                  class="inspect-table-button${item.name === selectedTable ? " is-active" : ""}"
                  type="button"
                  data-table-name="${escapeHtml(item.name)}"
                >
                  <span class="inspect-table-name">${escapeHtml(item.name)}</span>
                  <span class="inspect-table-meta">
                    ${item.column_count} cols · ${item.foreign_key_count} out · ${item.incoming_fk_count} in · ${item.index_count} idx${item.has_primary_key ? " · PK" : ""}
                  </span>
                </button>
              </li>
            `,
          )
          .join("");

  for (const button of inspectTableList.querySelectorAll<HTMLButtonElement>("[data-table-name]")) {
    button.addEventListener("click", () => {
      inspectTableSelect.value = button.dataset.tableName ?? "";
      handleControlChange();
    });
  }

  if (!table) {
    const hubTables = [...summary.tables]
      .sort(
        (left, right) =>
          right.foreign_key_count +
          right.incoming_fk_count -
          (left.foreign_key_count + left.incoming_fk_count),
      )
      .slice(0, 5);

    inspectDetail.innerHTML = `
      <article class="detail-card">
        <h2>Schema overview</h2>
        <p>
          ${summary.table_count} tables, ${summary.column_count} columns, ${summary.foreign_key_count} foreign keys, and ${summary.index_count} indexes parsed from the current SQL input.
        </p>
      </article>
      <article class="detail-card">
        <h3>Highlights</h3>
        <div class="detail-list">
          <div class="detail-item">
            <div class="detail-item__title">Tables without primary key</div>
            <div class="detail-item__meta">${summary.tables_without_pk}</div>
          </div>
          <div class="detail-item">
            <div class="detail-item__title">Isolated tables</div>
            <div class="detail-item__meta">${summary.orphan_table_count}</div>
          </div>
          ${hubTables
            .map(
              (item) => `
                <div class="detail-item">
                  <div class="detail-item__title">${escapeHtml(item.name)}</div>
                  <div class="detail-item__meta">
                    ${item.foreign_key_count} outgoing · ${item.incoming_fk_count} incoming
                  </div>
                </div>
              `,
            )
            .join("")}
        </div>
      </article>
    `;
    return;
  }

  inspectDetail.innerHTML = `
    <article class="detail-card">
      <h2>${escapeHtml(table.name)}</h2>
      <p>${table.comment ? escapeHtml(table.comment) : "No table comment."}</p>
    </article>
    <article class="detail-card">
      <h3>Columns</h3>
      <div class="detail-list">
        ${
          table.columns.length === 0
            ? '<p class="empty-state">No columns.</p>'
            : table.columns
                .map(
                  (column) => `
                    <div class="detail-item">
                      <div class="detail-item__title">
                        ${escapeHtml(column.name)}
                        ${column.is_primary_key ? '<span class="pill pill--warning">PK</span>' : ""}
                        ${column.nullable ? '<span class="pill pill--hint">NULL</span>' : '<span class="pill pill--info">NOT NULL</span>'}
                      </div>
                      <div class="detail-item__meta">${escapeHtml(column.data_type)}</div>
                    </div>
                  `,
                )
                .join("")
        }
      </div>
    </article>
    <article class="detail-card">
      <h3>Foreign keys</h3>
      <div class="detail-list">
        ${
          table.foreign_keys.length === 0
            ? '<p class="empty-state">No foreign keys.</p>'
            : table.foreign_keys
                .map(
                  (foreignKey) => `
                    <div class="detail-item">
                      <div class="detail-item__title">${escapeHtml(
                        foreignKey.name || foreignKey.from_columns.join(", "),
                      )}</div>
                      <div class="detail-item__meta">
                        ${escapeHtml(foreignKey.from_columns.join(", "))} → ${escapeHtml(foreignKey.to_table)}(${escapeHtml(foreignKey.to_columns.join(", "))})
                      </div>
                    </div>
                  `,
                )
                .join("")
        }
      </div>
    </article>
    <article class="detail-card">
      <h3>Indexes</h3>
      <div class="detail-list">
        ${
          table.indexes.length === 0
            ? '<p class="empty-state">No indexes.</p>'
            : table.indexes
                .map(
                  (index) => `
                    <div class="detail-item">
                      <div class="detail-item__title">
                        ${escapeHtml(index.name || index.columns.join(", "))}
                        ${index.is_unique ? '<span class="pill pill--warning">UNIQUE</span>' : ""}
                      </div>
                      <div class="detail-item__meta">${escapeHtml(index.columns.join(", "))}</div>
                    </div>
                  `,
                )
                .join("")
        }
      </div>
    </article>
  `;
}

function renderLintPanel(issues: readonly LintIssue[]): void {
  if (issues.length === 0) {
    lintIssueList.innerHTML =
      '<li class="issue-card"><p class="empty-state">No lint issues found.</p></li>';
    return;
  }

  lintIssueList.innerHTML = issues
    .map(
      (issue) => `
        <li class="issue-card">
          <div class="issue-card__meta">
            <span class="pill pill--${issue.severity}">${issue.severity}</span>
            <span class="pill pill--info">${escapeHtml(issue.category)}</span>
            <span class="issue-card__title">${escapeHtml(issue.rule_id)}</span>
            ${issue.table_name ? `<code>${escapeHtml(issue.table_name)}</code>` : ""}
            ${issue.column_name ? `<code>${escapeHtml(issue.column_name)}</code>` : ""}
          </div>
          <div class="issue-card__body">${escapeHtml(issue.message)}</div>
          ${issue.hint ? `<div class="issue-card__hint">${escapeHtml(issue.hint)}</div>` : ""}
        </li>
      `,
    )
    .join("");
}

function renderCompareSummary(diff: SchemaDiff): void {
  compareSummaryPanel.hidden = false;
  const totalObjects =
    diff.added_tables.length +
    diff.removed_tables.length +
    diff.modified_tables.length +
    diff.added_views.length +
    diff.removed_views.length +
    diff.modified_views.length +
    diff.added_enums.length +
    diff.removed_enums.length +
    diff.modified_enums.length;
  compareSummaryCount.textContent = `${totalObjects}`;
  compareSummary.innerHTML = [
    ["Added tables", `${diff.added_tables.length}`],
    ["Removed tables", `${diff.removed_tables.length}`],
    ["Modified tables", `${diff.modified_tables.length}`],
    ["Added views", `${diff.added_views.length}`],
    ["Removed views", `${diff.removed_views.length}`],
    ["Modified views", `${diff.modified_views.length}`],
    ["Added enums", `${diff.added_enums.length}`],
    ["Removed enums", `${diff.removed_enums.length}`],
  ]
    .map(
      ([label, value]) => `
        <article class="stat-card">
          <span class="stat-card__label">${label}</span>
          <strong class="stat-card__value">${value}</strong>
        </article>
      `,
    )
    .join("");

  const changeCards: string[] = [];
  for (const tableName of diff.added_tables) {
    changeCards.push(buildChangeCard("Added table", tableName, "Table exists only in after."));
  }
  for (const tableName of diff.removed_tables) {
    changeCards.push(buildChangeCard("Removed table", tableName, "Table exists only in before."));
  }
  for (const table of diff.modified_tables) {
    changeCards.push(
      buildChangeCard(
        "Modified table",
        table.table_name,
        `${table.column_diffs.length} column changes · ${table.fk_diffs.length} foreign key changes · ${table.index_diffs.length} index changes`,
      ),
    );
  }
  for (const viewName of diff.added_views) {
    changeCards.push(buildChangeCard("Added view", viewName, "View exists only in after."));
  }
  for (const viewName of diff.removed_views) {
    changeCards.push(buildChangeCard("Removed view", viewName, "View exists only in before."));
  }
  for (const view of diff.modified_views) {
    changeCards.push(
      buildChangeCard(
        "Modified view",
        view.view_name,
        `${view.column_diffs.length} column changes`,
      ),
    );
  }
  for (const enumName of diff.added_enums) {
    changeCards.push(buildChangeCard("Added enum", enumName, "Enum exists only in after."));
  }
  for (const enumName of diff.removed_enums) {
    changeCards.push(buildChangeCard("Removed enum", enumName, "Enum exists only in before."));
  }
  for (const schemaEnum of diff.modified_enums) {
    changeCards.push(
      buildChangeCard(
        "Modified enum",
        schemaEnum.enum_name,
        `${schemaEnum.value_diffs.length} enum value changes`,
      ),
    );
  }

  compareObjectList.innerHTML =
    changeCards.length === 0
      ? '<li class="change-card"><p class="empty-state">No schema changes detected.</p></li>'
      : changeCards.join("");
}

function buildChangeCard(kind: string, name: string, body: string): string {
  return `
    <li class="change-card">
      <div class="change-card__meta">
        <span class="pill pill--info">${escapeHtml(kind)}</span>
        <span class="change-card__title">${escapeHtml(name)}</span>
      </div>
      <div class="change-card__body">${escapeHtml(body)}</div>
    </li>
  `;
}

function showTextOutput(label: string, content: string): void {
  textOutputPanel.hidden = false;
  textOutputLabel.textContent = label;
  textOutputMeta.textContent = `${content.length.toLocaleString()} chars`;
  textOutput.textContent = content;
}

function renderMetricCards(entries: readonly [string, string][]): void {
  statsGrid.innerHTML = entries
    .map(
      ([label, value]) => `
        <article class="stat-card">
          <span class="stat-card__label">${label}</span>
          <strong class="stat-card__value">${value}</strong>
        </article>
      `,
    )
    .join("");
}

function renderDiagnostics(diagnostics: readonly WasmDiagnostic[]): void {
  diagnosticCount.textContent = `${diagnostics.length}`;

  if (diagnostics.length === 0) {
    diagnosticList.innerHTML = '<li class="diagnostic diagnostic--empty">No diagnostics.</li>';
    return;
  }

  diagnosticList.innerHTML = diagnostics
    .map((diagnostic) => {
      const severity = diagnostic.severity;
      return `
        <li class="diagnostic diagnostic--${severity}">
          <div class="diagnostic__meta">
            <span class="pill pill--${severity}">${severity}</span>
            <code>${formatDiagnosticCode(diagnostic)}</code>
          </div>
          <p>${escapeHtml(diagnostic.message)}</p>
        </li>
      `;
    })
    .join("");
}

function configureActions(actions: {
  copy?: ButtonAction;
  primary?: ButtonAction;
  secondary?: ButtonAction;
}): void {
  copyAction = actions.copy ?? null;
  primaryAction = actions.primary ?? null;
  secondaryAction = actions.secondary ?? null;

  copyOutputButton.textContent = copyAction?.label ?? "Copy";
  downloadPrimaryButton.textContent = primaryAction?.label ?? "Download";
  downloadSecondaryButton.textContent = secondaryAction?.label ?? "More";

  copyOutputButton.hidden = copyAction === null;
  downloadPrimaryButton.hidden = primaryAction === null;
  downloadSecondaryButton.hidden = secondaryAction === null;
}

function resetActions(): void {
  configureActions({});
}

function resetOutputPanels(): void {
  previewPanel.hidden = true;
  inspectPanel.hidden = true;
  lintPanel.hidden = true;
  compareSummaryPanel.hidden = true;
  textOutputPanel.hidden = true;
  previewFrame.srcdoc = "";
  inspectTableList.innerHTML = "";
  inspectDetail.innerHTML = "";
  lintIssueList.innerHTML = "";
  compareSummary.innerHTML = "";
  compareObjectList.innerHTML = "";
  textOutput.textContent = "";
  textOutputMeta.textContent = "";
  resetActions();
}

function populateInspectTableOptions(tableNames: readonly string[], selectedTable: string): void {
  inspectTableSelect.innerHTML = [
    '<option value="">Schema summary</option>',
    ...tableNames.map(
      (tableName) => `<option value="${escapeHtml(tableName)}">${escapeHtml(tableName)}</option>`,
    ),
  ].join("");
  inspectTableSelect.value = selectedTable;
}

function splitPatterns(rawValue: string): string[] {
  return rawValue
    .split(",")
    .map((value) => value.trim())
    .filter((value) => value.length > 0);
}

function parsePositiveInteger(rawValue: string): number | undefined {
  const parsed = Number.parseInt(rawValue, 10);
  if (!Number.isFinite(parsed) || parsed < 1) {
    return undefined;
  }
  return parsed;
}

function formatDuration(duration: WasmDuration): string {
  const millis = duration.secs * 1_000 + duration.nanos / 1_000_000;
  if (millis >= 1_000) {
    return `${(millis / 1_000).toFixed(2)} s`;
  }
  if (millis >= 10) {
    return `${millis.toFixed(0)} ms`;
  }
  return `${millis.toFixed(1)} ms`;
}

function formatDiagnosticCode(diagnostic: WasmDiagnostic): string {
  return `${diagnostic.code.prefix}${diagnostic.code.number.toString().padStart(3, "0")}`;
}

function totalDiffChanges(summary: DiffSummary): number {
  return (
    summary.tables_added +
    summary.tables_removed +
    summary.tables_modified +
    summary.columns_changed +
    summary.foreign_keys_changed +
    summary.indexes_changed +
    summary.views_added +
    summary.views_removed +
    summary.views_modified +
    summary.view_columns_changed +
    summary.view_definitions_changed +
    summary.enums_added +
    summary.enums_removed +
    summary.enums_modified +
    summary.enum_values_changed
  );
}

function exportFormatLabel(): string {
  switch (exportFormatSelect.value as ExportFormat) {
    case "schema-json":
      return "Schema JSON";
    case "graph-json":
      return "Graph JSON";
    case "layout-json":
      return "Layout JSON";
    case "mermaid":
      return "Mermaid";
    case "d2":
      return "D2";
    case "dot":
      return "DOT";
  }
}

function exportFilename(): string {
  switch (exportFormatSelect.value as ExportFormat) {
    case "schema-json":
      return "relune-schema.json";
    case "graph-json":
      return "relune-graph.json";
    case "layout-json":
      return "relune-layout.json";
    case "mermaid":
      return "relune-diagram.mmd";
    case "d2":
      return "relune-diagram.d2";
    case "dot":
      return "relune-diagram.dot";
  }
}

function exportMimeType(): string {
  switch (exportFormatSelect.value as ExportFormat) {
    case "schema-json":
    case "graph-json":
    case "layout-json":
      return "application/json;charset=utf-8";
    default:
      return "text/plain;charset=utf-8";
  }
}

function setStatus(text: string): void {
  renderStatus.textContent = text;
}

function showError(error: WasmErrorShape): void {
  errorBox.hidden = false;
  errorBox.innerHTML = `
    <strong>${escapeHtml(error.code ?? "WORKBENCH_ERROR")}</strong>
    <p>${escapeHtml(error.message)}</p>
  `;
}

function clearError(): void {
  errorBox.hidden = true;
  errorBox.innerHTML = "";
}

function normalizeError(error: unknown): WasmErrorShape {
  if (isWasmErrorShape(error)) {
    return error;
  }
  if (
    error &&
    typeof error === "object" &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return { message: error.message };
  }
  return { message: "Unknown playground error." };
}

function isWasmErrorShape(value: unknown): value is WasmErrorShape {
  if (!value || typeof value !== "object") {
    return false;
  }

  return "message" in value && typeof value.message === "string";
}

function downloadText(filename: string, content: string, mimeType: string): void {
  const blob = new Blob([content], { type: mimeType });
  const blobUrl = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = blobUrl;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(blobUrl);
}

async function copyText(content: string, successStatus: string): Promise<void> {
  if (!content) {
    return;
  }

  await navigator.clipboard.writeText(content);
  setStatus(successStatus);
}

function readStoredState(): Partial<PersistedState> {
  const rawValue = localStorage.getItem(STORAGE_KEY);
  if (!rawValue) {
    return {};
  }

  try {
    return sanitizeState(JSON.parse(rawValue) as Partial<PersistedState>);
  } catch {
    return {};
  }
}

function readQueryState(): Partial<PersistedState> {
  const params = new URLSearchParams(window.location.search);
  return sanitizeState({
    example: (params.get("example") as ExampleId | null) ?? undefined,
    mode: (params.get("mode") as WorkbenchMode | null) ?? undefined,
    theme: (params.get("theme") as Theme | null) ?? undefined,
    layout: (params.get("layout") as LayoutAlgorithm | null) ?? undefined,
    direction: (params.get("direction") as LayoutDirection | null) ?? undefined,
    edgeStyle: (params.get("edges") as EdgeStyle | null) ?? undefined,
    viewpoint: params.get("viewpoint") ?? undefined,
    groupBy: (params.get("group") as GroupBy | null) ?? undefined,
    focusTable: params.get("focus") ?? undefined,
    depth: params.get("depth") ?? undefined,
    includeTables: params.get("include") ?? undefined,
    excludeTables: params.get("exclude") ?? undefined,
    exportFormat: (params.get("export") as ExportFormat | null) ?? undefined,
    inspectTable: params.get("table") ?? undefined,
    lintRules: params.get("rules") ?? undefined,
    compareView: (params.get("compare") as CompareView | null) ?? undefined,
  });
}

function sanitizeState(state: Partial<PersistedState>): Partial<PersistedState> {
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
  if (typeof state.viewpoint === "string") {
    sanitized.viewpoint = state.viewpoint.trim();
  }
  if (isGroupBy(state.groupBy)) {
    sanitized.groupBy = state.groupBy;
  }
  if (typeof state.focusTable === "string") {
    sanitized.focusTable = state.focusTable;
  }
  if (typeof state.depth === "string") {
    sanitized.depth = state.depth;
  }
  if (typeof state.includeTables === "string") {
    sanitized.includeTables = state.includeTables;
  }
  if (typeof state.excludeTables === "string") {
    sanitized.excludeTables = state.excludeTables;
  }
  if (isExportFormat(state.exportFormat)) {
    sanitized.exportFormat = state.exportFormat;
  }
  if (typeof state.inspectTable === "string") {
    sanitized.inspectTable = state.inspectTable;
  }
  if (typeof state.lintRules === "string") {
    sanitized.lintRules = state.lintRules;
  }
  if (isCompareView(state.compareView)) {
    sanitized.compareView = state.compareView;
  }
  if (typeof state.sql === "string") {
    sanitized.sql = state.sql;
  }
  if (typeof state.compareBeforeSql === "string") {
    sanitized.compareBeforeSql = state.compareBeforeSql;
  }
  if (typeof state.compareAfterSql === "string") {
    sanitized.compareAfterSql = state.compareAfterSql;
  }

  return sanitized;
}

function persistState(): void {
  const state = collectState();
  localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  syncQueryString(state);
}

function collectState(): PersistedState {
  return {
    example: exampleSelect.value as ExampleId,
    mode: currentMode,
    theme: themeSelect.value as Theme,
    layout: layoutSelect.value as LayoutAlgorithm,
    direction: directionSelect.value as LayoutDirection,
    edgeStyle: edgeStyleSelect.value as EdgeStyle,
    viewpoint: viewpointSelect.value,
    groupBy: groupBySelect.value as GroupBy,
    focusTable: focusTableInput.value.trim(),
    depth: depthInput.value.trim(),
    includeTables: includeTablesInput.value.trim(),
    excludeTables: excludeTablesInput.value.trim(),
    exportFormat: exportFormatSelect.value as ExportFormat,
    inspectTable: inspectTableSelect.value,
    lintRules: lintRulesInput.value.trim(),
    compareView: compareFormatSelect.value as CompareView,
    sql: sqlEditor.getValue(),
    compareBeforeSql: compareBeforeEditor.getValue(),
    compareAfterSql: compareAfterEditor.getValue(),
  };
}

function syncQueryString(state: PersistedState): void {
  const params = new URLSearchParams();
  params.set("example", state.example);
  params.set("mode", state.mode);
  params.set("theme", state.theme);
  params.set("layout", state.layout);
  params.set("direction", state.direction);
  params.set("edges", state.edgeStyle);
  params.set("group", state.groupBy);

  if (state.viewpoint) {
    params.set("viewpoint", state.viewpoint);
  }
  if (state.focusTable) {
    params.set("focus", state.focusTable);
  }
  if (state.depth && state.depth !== DEFAULT_STATE.depth) {
    params.set("depth", state.depth);
  }
  if (state.includeTables) {
    params.set("include", state.includeTables);
  }
  if (state.excludeTables) {
    params.set("exclude", state.excludeTables);
  }
  if (state.mode === "export") {
    params.set("export", state.exportFormat);
  }
  if (state.mode === "inspect" && state.inspectTable) {
    params.set("table", state.inspectTable);
  }
  if (state.mode === "lint" && state.lintRules) {
    params.set("rules", state.lintRules);
  }
  if (state.mode === "compare") {
    params.set("compare", state.compareView);
  }

  const nextQuery = params.toString();
  const nextUrl = nextQuery ? `?${nextQuery}` : window.location.pathname;
  window.history.replaceState(null, "", nextUrl);
}

function setActionButtonsDisabled(disabled: boolean): void {
  renderNowButton.disabled = disabled;
  copyOutputButton.disabled = disabled;
  downloadPrimaryButton.disabled = disabled;
  downloadSecondaryButton.disabled = disabled;
}

function isExampleId(value: unknown): value is ExampleId {
  return (
    value === "simple-blog" ||
    value === "ecommerce" ||
    value === "multi-schema" ||
    value === CUSTOM_EXAMPLE_ID
  );
}

function isWorkbenchMode(value: unknown): value is WorkbenchMode {
  return (
    value === "render" ||
    value === "inspect" ||
    value === "export" ||
    value === "lint" ||
    value === "compare"
  );
}

function isTheme(value: unknown): value is Theme {
  return value === "light" || value === "dark";
}

function isLayoutAlgorithm(value: unknown): value is LayoutAlgorithm {
  return value === "hierarchical" || value === "force-directed";
}

function isLayoutDirection(value: unknown): value is LayoutDirection {
  return (
    value === "top-to-bottom" ||
    value === "left-to-right" ||
    value === "right-to-left" ||
    value === "bottom-to-top"
  );
}

function isEdgeStyle(value: unknown): value is EdgeStyle {
  return value === "curved" || value === "orthogonal" || value === "straight";
}

function isGroupBy(value: unknown): value is GroupBy {
  return value === "none" || value === "schema" || value === "prefix";
}

function isExportFormat(value: unknown): value is ExportFormat {
  return (
    value === "schema-json" ||
    value === "graph-json" ||
    value === "layout-json" ||
    value === "mermaid" ||
    value === "d2" ||
    value === "dot"
  );
}

function isCompareView(value: unknown): value is CompareView {
  return value === "visual" || value === "text" || value === "markdown" || value === "json";
}

function toBuiltinExampleId(value: ExampleId): Exclude<ExampleId, "custom"> {
  return value === CUSTOM_EXAMPLE_ID ? DEFAULT_EXAMPLE_ID : value;
}

function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
}

function getElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) {
    throw new Error(`Missing element #${id}`);
  }
  return element as T;
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
