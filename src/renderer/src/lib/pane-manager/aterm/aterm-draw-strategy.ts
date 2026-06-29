import type { AtermTerminal } from './aterm_wasm.js'

/** The seam that decouples the aterm controller from HOW a frame is drawn.
 *
 *  A strategy owns BOTH the engine instance (one per pane — bytes are parsed
 *  once) AND the draw surface (the grid `<canvas>`). The controller wires the
 *  engine into every input/search/reply handler and schedules `drawFrame()`;
 *  it never touches the GPU/2d context directly.
 *
 *  Two implementations:
 *   - CPU (`aterm-cpu-drawer`): the default + fallback — `aterm-wasm`'s engine
 *     rasterizes on the CPU, JS `putImageData`s the RGBA frame onto a 2d canvas.
 *   - GPU (`aterm-gpu-drawer`): `aterm-gpu-web`'s engine draws the grid straight
 *     into a WebGL2 canvas surface (no readback) on the present path.
 *
 *  Because `aterm-gpu-web`'s `AtermGpuTerminal` mirrors `AtermTerminal`'s entire
 *  state surface (scroll/selection/search/mouse/link/cursor/focus), the engine
 *  handle is typed as `AtermTerminal` for both — the input handlers are unchanged.
 *  A canvas can hold EITHER a webgl2 OR a 2d context (never both), so the GPU
 *  strategy's grid canvas is webgl2-owned and search highlights paint to a
 *  SEPARATE stacked 2d overlay canvas the controller positions over the grid. */
export type AtermDrawStrategy = {
  /** The single engine for this pane — drawing AND state. Typed as the CPU
   *  engine; the GPU engine is a structural superset, so every input/search/
   *  reply handler binds to it unchanged. */
  term: AtermTerminal
  /** The grid canvas the controller appends to the pane DOM. Owns the draw
   *  context (2d for CPU, webgl2 for GPU); the controller must NOT call
   *  getContext on it for the other kind. */
  getCanvas: () => HTMLCanvasElement
  /** Whether this strategy paints search highlights onto the grid canvas itself
   *  (CPU, via its 2d context) or needs a SEPARATE stacked 2d overlay (GPU,
   *  whose grid canvas is webgl2-only). The controller creates the overlay when
   *  this is true. */
  needsSearchOverlay: boolean
  /** Render ONE frame: re-index coalesced search, present the engine grid, size
   *  the canvas (CSS = device/dpr), and (CPU only) overlay search highlights.
   *  The GPU strategy presents the grid and lets the controller paint search on
   *  the overlay afterwards. */
  drawFrame: () => void
  /** Resize the grid (the controller has already recomputed cols/rows). */
  resize: (rows: number, cols: number) => void
  /** Worker path only: subscribe to the engine's query replies (DA/DSR/CPR/colour)
   *  so the wiring can forward them to the PTY. The engine lives in the worker, so its
   *  replies arrive as pushed events rather than a synchronous take_response() drain;
   *  unset for the in-process CPU/GPU strategies (their replies pull-drain as before). */
  onReply?: (handler: (data: string) => void) => void
  /** Worker path only: subscribe to engine re-rasterization (new cell size) so the
   *  wiring re-reflows the grid — the worker applies set_px/line-height AFTER the first
   *  snapshot, so metrics can arrive a frame late. Unset for in-process strategies
   *  (their set_px is synchronous, so metrics are read directly). */
  onMetricsChange?: (handler: () => void) => void
  /** Tear down the draw surface + engine (free the wasm handle, drop contexts). */
  dispose: () => void
}
