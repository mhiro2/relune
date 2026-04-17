import initWasm, {
  render_from_sql,
  set_panic_hook,
  version,
  type WasmDiagnostic,
  type WasmDuration,
  type WasmErrorShape,
  type WasmRenderRequest,
  type WasmRenderStats,
} from "../pkg/relune_wasm.js";
import { createSqlEditor } from "./editor.js";

type ExampleId = "simple-blog" | "ecommerce" | "multi-schema" | "custom";
type Theme = "light" | "dark";
type LayoutAlgorithm = "hierarchical" | "force-directed";
type LayoutDirection = "top-to-bottom" | "left-to-right" | "right-to-left" | "bottom-to-top";
type EdgeStyle = "curved" | "orthogonal" | "straight";
type GroupBy = "none" | "schema" | "prefix";
type ViewpointId = string;

type PersistedState = {
  example: ExampleId;
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
  sql: string;
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

const STORAGE_KEY = "relune-playground:v1";
const CUSTOM_EXAMPLE_ID = "custom";
const DEFAULT_EXAMPLE_ID: Exclude<ExampleId, "custom"> = "simple-blog";
const MANUAL_VIEWPOINT_ID = "";
const MANUAL_VIEWPOINT_LABEL = "Manual controls";
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

const DEFAULT_STATE: PersistedState = {
  example: DEFAULT_EXAMPLE_ID,
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
  sql: "",
};

const exampleSelect = getElement<HTMLSelectElement>("example-select");
const themeSelect = getElement<HTMLSelectElement>("theme-select");
const layoutSelect = getElement<HTMLSelectElement>("layout-select");
const directionSelect = getElement<HTMLSelectElement>("direction-select");
const edgeStyleSelect = getElement<HTMLSelectElement>("edge-style-select");
const viewpointSelect = getElement<HTMLSelectElement>("viewpoint-select");
const viewpointHint = getElement<HTMLElement>("viewpoint-hint");
const groupBySelect = getElement<HTMLSelectElement>("group-by-select");
const focusTableInput = getElement<HTMLInputElement>("focus-table-input");
const depthInput = getElement<HTMLInputElement>("depth-input");
const includeTablesInput = getElement<HTMLInputElement>("include-tables-input");
const excludeTablesInput = getElement<HTMLInputElement>("exclude-tables-input");
const sqlEditorMount = getElement<HTMLDivElement>("sql-input");
const sqlEditor = createSqlEditor(sqlEditorMount);
const resetExampleButton = getElement<HTMLButtonElement>("reset-example");
const renderNowButton = getElement<HTMLButtonElement>("render-now");
const downloadHtmlButton = getElement<HTMLButtonElement>("download-html");
const downloadSvgButton = getElement<HTMLButtonElement>("download-svg");
const renderStatus = getElement<HTMLElement>("render-status");
const versionPill = getElement<HTMLElement>("version-pill");
const errorBox = getElement<HTMLElement>("error-box");
const statsGrid = getElement<HTMLElement>("stats-grid");
const diagnosticCount = getElement<HTMLElement>("diagnostic-count");
const diagnosticList = getElement<HTMLUListElement>("diagnostic-list");
const previewFrame = getElement<HTMLIFrameElement>("preview-frame");
const sidebar = getElement<HTMLElement>("sidebar");
const sidebarHandle = getElement<HTMLElement>("sidebar-handle");
const sidebarCollapseButton = getElement<HTMLButtonElement>("sidebar-collapse");
const sidebarExpandButton = getElement<HTMLButtonElement>("sidebar-expand");
const editorExpandButton = getElement<HTMLButtonElement>("editor-expand");
const sidebarScroll = document.querySelector<HTMLElement>(".sidebar__scroll")!;

const exampleSql = new Map<Exclude<ExampleId, "custom">, string>();

const SIDEBAR_DEFAULT = 380;

let renderTimer = 0;
let renderSerial = 0;
let lastHtmlOutput = "";
let isApplyingViewpoint = false;
let previousViewpointId = MANUAL_VIEWPOINT_ID;
const manualViewStateByExample = new Map<ExampleId, ManualViewState>();

populateExampleOptions();

void bootstrap();

async function bootstrap(): Promise<void> {
  try {
    setStatus("Loading WASM runtime…");
    renderNowButton.disabled = true;
    downloadHtmlButton.disabled = true;
    downloadSvgButton.disabled = true;
    await loadExamples();
    restoreInitialState();
    await initWasm();
    set_panic_hook();
    versionPill.textContent = `relune-wasm ${version()}`;
    renderNowButton.disabled = false;
    scheduleRender(0);
  } catch (error) {
    showError(normalizeError(error));
    setStatus("Runtime failed");
  }
}

function populateExampleOptions(): void {
  const options = EXAMPLES.map(
    (example) => `<option value="${example.id}">${example.label}</option>`,
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
  }

  if (!initialState.sql) {
    initialState.sql = exampleSql.get(DEFAULT_EXAMPLE_ID) ?? "";
  }

  applyState(initialState);
  attachEventListeners();
  persistState();
}

function attachEventListeners(): void {
  exampleSelect.addEventListener("change", handleExampleChange);
  viewpointSelect.addEventListener("change", handleViewpointChange);
  resetExampleButton.addEventListener("click", resetExample);
  renderNowButton.addEventListener("click", () => {
    void renderDiagram();
  });
  downloadHtmlButton.addEventListener("click", downloadHtml);
  downloadSvgButton.addEventListener("click", () => {
    void downloadSvg();
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
  ];

  for (const control of controls) {
    control.addEventListener("change", handleControlChange);
    control.addEventListener("input", handleControlChange);
  }

  sqlEditor.onUpdate(() => {
    syncExampleSelectionWithEditor();
    handleControlChange();
  });

  sidebarCollapseButton.addEventListener("click", collapseSidebar);
  sidebarExpandButton.addEventListener("click", expandSidebar);
  editorExpandButton.addEventListener("click", toggleEditorExpand);
  initSidebarResize();
}

function collapseSidebar(): void {
  sidebar.classList.add("is-collapsed");
}

function toggleEditorExpand(): void {
  const expanded = sidebarScroll.classList.toggle("is-editor-expanded");
  editorExpandButton.setAttribute("aria-label", expanded ? "Collapse editor" : "Expand editor");
  editorExpandButton.setAttribute("title", expanded ? "Collapse editor" : "Expand editor");
}

function expandSidebar(): void {
  sidebar.classList.remove("is-collapsed");
  sidebar.style.width = `${SIDEBAR_DEFAULT}px`;
}

function initSidebarResize(): void {
  let dragging = false;
  let startX = 0;
  let startWidth = 0;

  function onPointerDown(event: PointerEvent): void {
    // If collapsed, restore on click
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

function handleExampleChange(): void {
  populateViewpointOptions(exampleSelect.value as ExampleId, viewpointSelect.value);
  const selectedViewpoint = getSelectedViewpoint();
  if (selectedViewpoint) {
    applyViewpoint(selectedViewpoint);
  } else {
    restoreManualViewState(exampleSelect.value as ExampleId);
  }

  if (exampleSelect.value === CUSTOM_EXAMPLE_ID) {
    previousViewpointId = viewpointSelect.value;
    persistState();
    scheduleRender();
    return;
  }

  const sql = exampleSql.get(exampleSelect.value as Exclude<ExampleId, "custom">);
  if (sql) {
    sqlEditor.setValue(sql);
  }

  previousViewpointId = viewpointSelect.value;
  persistState();
  scheduleRender();
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
  scheduleRender();
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
  sqlEditor.setValue(exampleSql.get(effectiveExample) ?? "");
  previousViewpointId = viewpointSelect.value;
  persistState();
  scheduleRender(0);
}

function handleControlChange(): void {
  syncViewpointSelectionWithControls();
  if (viewpointSelect.value === MANUAL_VIEWPOINT_ID) {
    storeManualViewState(exampleSelect.value as ExampleId, readManualViewStateFromControls());
  }
  previousViewpointId = viewpointSelect.value;
  applyTheme(themeSelect.value as Theme);
  persistState();
  scheduleRender();
}

function syncExampleSelectionWithEditor(): void {
  if (exampleSelect.value === CUSTOM_EXAMPLE_ID) {
    return;
  }

  const selectedSql = exampleSql.get(toBuiltinExampleId(exampleSelect.value as ExampleId));
  if (selectedSql && sqlEditor.getValue().trim() !== selectedSql.trim()) {
    exampleSelect.value = CUSTOM_EXAMPLE_ID;
    populateViewpointOptions(CUSTOM_EXAMPLE_ID, MANUAL_VIEWPOINT_ID);
    previousViewpointId = MANUAL_VIEWPOINT_ID;
  }
}

function applyState(state: PersistedState): void {
  exampleSelect.value = state.example;
  themeSelect.value = state.theme;
  layoutSelect.value = state.layout;
  directionSelect.value = state.direction;
  edgeStyleSelect.value = state.edgeStyle;
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
  applyTheme(state.theme);
  previousViewpointId = viewpointSelect.value;
}

function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
}

function scheduleRender(delay = 250): void {
  window.clearTimeout(renderTimer);
  renderTimer = window.setTimeout(() => {
    void renderDiagram();
  }, delay);
}

async function renderDiagram(): Promise<void> {
  const sql = sqlEditor.getValue().trim();
  if (!sql) {
    showError({ message: "SQL input is empty." });
    setStatus("Waiting for SQL");
    return;
  }

  const currentSerial = ++renderSerial;
  clearError();
  setStatus("Rendering…");

  try {
    const request = buildRenderRequest("html");
    const result = render_from_sql(request);

    if (currentSerial !== renderSerial) {
      return;
    }

    lastHtmlOutput = result.content;
    previewFrame.srcdoc = result.content;
    renderStats(result.stats);
    renderDiagnostics(result.diagnostics);
    downloadHtmlButton.disabled = false;
    downloadSvgButton.disabled = false;
    setStatus(`Rendered in ${formatDuration(result.stats.total_time)}`);
  } catch (error) {
    if (currentSerial !== renderSerial) {
      return;
    }

    renderDiagnostics([]);
    renderStats(null);
    showError(normalizeError(error));
    setStatus("Render failed");
  }
}

function buildRenderRequest(format: WasmRenderRequest["format"]): WasmRenderRequest {
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

function renderStats(stats: WasmRenderStats | null): void {
  if (!stats) {
    statsGrid.innerHTML = "";
    return;
  }

  const entries: readonly [string, string][] = [
    ["Tables", `${stats.table_count}`],
    ["Columns", `${stats.column_count}`],
    ["Edges", `${stats.edge_count}`],
    ["Views", `${stats.view_count}`],
    ["Parse", formatDuration(stats.parse_time)],
    ["Graph", formatDuration(stats.graph_time)],
    ["Layout", formatDuration(stats.layout_time)],
    ["Render", formatDuration(stats.render_time)],
  ];

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

function formatDiagnosticCode(diagnostic: WasmDiagnostic): string {
  return `${diagnostic.code.prefix}${diagnostic.code.number.toString().padStart(3, "0")}`;
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

function setStatus(text: string): void {
  renderStatus.textContent = text;
}

function showError(error: WasmErrorShape): void {
  errorBox.hidden = false;
  errorBox.innerHTML = `
    <strong>${escapeHtml(error.code ?? "PLAYGROUND_ERROR")}</strong>
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

function downloadHtml(): void {
  if (!lastHtmlOutput) {
    return;
  }

  downloadText("relune-playground.html", lastHtmlOutput, "text/html;charset=utf-8");
}

async function downloadSvg(): Promise<void> {
  try {
    setStatus("Preparing SVG…");
    const svgResult = render_from_sql(buildRenderRequest("svg"));
    downloadText("relune-playground.svg", svgResult.content, "image/svg+xml;charset=utf-8");
    setStatus("SVG downloaded");
  } catch (error) {
    showError(normalizeError(error));
    setStatus("SVG export failed");
  }
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

function readStoredState(): Partial<PersistedState> {
  const rawValue = localStorage.getItem(STORAGE_KEY);
  if (!rawValue) {
    return {};
  }

  try {
    const parsed = JSON.parse(rawValue) as Partial<PersistedState>;
    return sanitizeState(parsed);
  } catch {
    return {};
  }
}

function readQueryState(): Partial<PersistedState> {
  const params = new URLSearchParams(window.location.search);
  return sanitizeState({
    example: (params.get("example") as ExampleId | null) ?? undefined,
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
  });
}

function sanitizeState(state: Partial<PersistedState>): Partial<PersistedState> {
  const sanitized: Partial<PersistedState> = {};

  if (isExampleId(state.example)) {
    sanitized.example = state.example;
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
  if (typeof state.sql === "string") {
    sanitized.sql = state.sql;
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
    sql: sqlEditor.getValue(),
  };
}

function syncQueryString(state: PersistedState): void {
  const params = new URLSearchParams();
  params.set("example", state.example);
  params.set("theme", state.theme);
  params.set("layout", state.layout);
  params.set("direction", state.direction);
  params.set("edges", state.edgeStyle);
  if (state.viewpoint) {
    params.set("viewpoint", state.viewpoint);
  }
  params.set("group", state.groupBy);

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

  const nextQuery = params.toString();
  const nextUrl = nextQuery ? `?${nextQuery}` : window.location.pathname;
  window.history.replaceState(null, "", nextUrl);
}

function isExampleId(value: unknown): value is ExampleId {
  return (
    value === "simple-blog" ||
    value === "ecommerce" ||
    value === "multi-schema" ||
    value === CUSTOM_EXAMPLE_ID
  );
}

function toBuiltinExampleId(value: ExampleId): Exclude<ExampleId, "custom"> {
  return value === CUSTOM_EXAMPLE_ID ? DEFAULT_EXAMPLE_ID : value;
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
