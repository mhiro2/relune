export type WasmInitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;
export type WasmInitSource = WasmInitInput | Promise<WasmInitInput>;

export interface WasmDiagnosticCode {
  prefix: string;
  number: number;
}

export interface WasmDiagnostic {
  severity: "error" | "warning" | "info" | "hint";
  code: WasmDiagnosticCode;
  message: string;
}

export interface WasmDuration {
  secs: number;
  nanos: number;
}

export interface WasmRenderStats {
  table_count: number;
  column_count: number;
  edge_count: number;
  view_count: number;
  parse_time: WasmDuration;
  graph_time: WasmDuration;
  layout_time: WasmDuration;
  render_time: WasmDuration;
  total_time: WasmDuration;
}

export interface WasmRenderResult {
  content: string;
  diagnostics: WasmDiagnostic[];
  stats: WasmRenderStats;
}

export interface WasmRenderRequest {
  sql: string;
  format: "html" | "svg";
  focusTable?: string;
  depth?: number;
  includeTables?: string[];
  excludeTables?: string[];
  groupBy?: "none" | "schema" | "prefix";
  layoutDirection?: "top-to-bottom" | "left-to-right" | "right-to-left" | "bottom-to-top";
  layoutAlgorithm?: "hierarchical" | "force-directed";
  edgeStyle?: "straight" | "orthogonal" | "curved";
  theme?: "light" | "dark";
  showLegend?: boolean;
  showStats?: boolean;
}

export interface WasmErrorShape {
  message: string;
  code?: string;
}

export interface WasmInitOutput {
  readonly memory: WebAssembly.Memory;
}

export default function init(
  input?: WasmInitSource | { module_or_path: WasmInitSource },
): Promise<WasmInitOutput>;
export function set_panic_hook(): void;
export function render_from_sql(input: WasmRenderRequest): WasmRenderResult;
export function version(): string;
