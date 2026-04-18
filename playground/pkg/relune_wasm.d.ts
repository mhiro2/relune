export type WasmInitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;
export type WasmInitSource = WasmInitInput | Promise<WasmInitInput>;

export interface WasmInitOutput {
  readonly memory: WebAssembly.Memory;
}

export default function init(
  input?: WasmInitSource | { module_or_path: WasmInitSource },
): Promise<WasmInitOutput>;

export function set_panic_hook(): void;
export function version(): string;

export function render_from_sql(input: unknown): unknown;
export function inspect_from_sql(input: unknown): unknown;
export function export_from_sql(input: unknown): unknown;
export function lint_from_sql(input: unknown): unknown;
export function diff_from_sql(input: unknown): unknown;
