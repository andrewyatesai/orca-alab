/* tslint:disable */
/* eslint-disable */
/**
 * A terminal + CPU renderer pair. Feed PTY bytes with [`AtermTerminal::process`],
 * then [`AtermTerminal::render`] to refresh the RGBA framebuffer, then read it
 * back via [`AtermTerminal::rgba`] (+ `width`/`height`) to draw onto a canvas.
 */
export class AtermTerminal {
  free(): void;
  /**
   * Build a `rows`x`cols` terminal rendered with `font_bytes` (a TTF/OTF) at
   * `px` cell font-size. `font_bytes` is injected by the host (fetched in JS),
   * keeping the engine free of filesystem font discovery.
   */
  constructor(rows: number, cols: number, font_bytes: Uint8Array, px: number);
  /**
   * Copy of the last-rendered RGBA8 framebuffer (`width*height*4` bytes),
   * ready for `ctx.putImageData(new ImageData(rgba, width, height), 0, 0)`.
   */
  rgba(): Uint8Array;
  /**
   * Rasterize the current grid into the internal RGBA8 framebuffer.
   */
  render(): void;
  /**
   * Resize the grid (after the host recomputes cols/rows for the canvas).
   */
  resize(rows: number, cols: number): void;
  /**
   * Feed raw PTY output bytes into the engine.
   */
  process(bytes: Uint8Array): void;
  /**
   * Last-rendered framebuffer width in pixels.
   */
  readonly width: number;
  /**
   * Last-rendered framebuffer height in pixels.
   */
  readonly height: number;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly __wbg_atermterminal_free: (a: number, b: number) => void;
  readonly atermterminal_height: (a: number) => number;
  readonly atermterminal_new: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
  readonly atermterminal_process: (a: number, b: number, c: number) => void;
  readonly atermterminal_render: (a: number) => void;
  readonly atermterminal_resize: (a: number, b: number, c: number) => void;
  readonly atermterminal_rgba: (a: number, b: number) => void;
  readonly atermterminal_width: (a: number) => number;
  readonly __wbindgen_export_0: (a: number) => void;
  readonly __wbindgen_export_1: (a: number, b: number, c: number) => void;
  readonly __wbindgen_export_2: (a: number, b: number) => number;
  readonly __wbindgen_export_3: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;
/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
