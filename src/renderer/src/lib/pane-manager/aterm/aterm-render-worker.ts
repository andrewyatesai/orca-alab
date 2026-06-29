// Off-main-thread aterm render worker (plan §9, stage 2 — NOT yet wired into pane
// creation; the opt-in strategy that drives it lands in a later stage gated behind
// an explicit flag, so production keeps using the proven main-thread path until
// this is validated in Electron/CDP).
//
// Owns an aterm engine + the pane's transferred OffscreenCanvas and does the
// per-frame work HERE, so heavy terminal output no longer competes with the
// renderer main thread. Two engines share this worker:
//   - 'cpu' (aterm-wasm): rasterize → zero-copy 2d blit (identical to the main-
//     thread painter).
//   - 'gpu' (aterm-gpu-web): WebGL2 present straight to the OffscreenCanvas
//     swapchain — NO rgba readback/blit (the universal off-main win).
// After each frame it posts a small cacheable STATE snapshot so the main thread's
// draw/follow-bottom logic stays synchronous without an RPC round-trip.

import init, { AtermTerminal } from './aterm_wasm.js'
import wasmUrl from './aterm_wasm_bg.wasm?url'
import gpuInit, { AtermGpuTerminal } from './aterm_gpu_web.js'
import gpuWasmUrl from './aterm_gpu_web_bg.wasm?url'
import { seedAtermPalette, seedAtermReplyDefaults } from './aterm-theme-colors'
import type { AtermThemeColors } from './aterm-theme-colors'
import type {
  AtermWorkerInit,
  AtermWorkerMessage,
  AtermWorkerRequest
} from './aterm-render-worker-protocol'

// DedicatedWorkerGlobalScope without pulling in the WebWorker lib (which clashes
// with the DOM lib this project compiles against): cast the minimal surface used.
const ctx = self as unknown as {
  onmessage: ((event: { data: AtermWorkerRequest }) => void) | null
  postMessage: (message: AtermWorkerMessage) => void
}

/** Init params kept after the first message so a GPU→CPU fallback can rebuild on the
 *  SAME canvas: the canvas + font bytes were transferred and can't be re-sent. */
type StoredInit = {
  fontBytes: Uint8Array
  fallbackFonts: Uint8Array[]
  rows: number
  cols: number
  fontPx: number
  themeColors: AtermThemeColors
}

/** The per-frame surface the message loop drives, independent of which engine backs
 *  it. `renderAndPost` renders the frame (CPU blit or GPU present) then posts state. */
type RenderEngine = {
  process: (data: string) => void
  renderAndPost: () => void
  resize: (rows: number, cols: number) => void
  setPx: (px: number) => void
  scrollLines: (delta: number) => void
  scrollToBottom: () => void
  dispose: () => void
}

/** The state getters BOTH engines expose identically (cursor/grid/title/etc.); the
 *  framebuffer width/height differ per engine and are passed in (see callers). */
type SnapshotSource = {
  cell_width: number
  cell_height: number
  display_offset: number
  cursor_x: number
  cursor_y: number
  base_y: number
  is_alt_screen: boolean
  title: () => string | undefined
}

/** The construction-time surface BOTH engines expose for font + theme seeding. */
type SeedTarget = {
  cell_width: number
  cell_height: number
  set_fallback_font: (bytes: Uint8Array) => void
  add_fallback_font: (bytes: Uint8Array) => void
  set_palette_color: (index: number, r: number, g: number, b: number) => void
  set_selection_fg: (fg?: number | null) => void
  set_selection_inactive_bg: (bg?: number | null) => void
  set_default_foreground: (r: number, g: number, b: number) => void
  set_default_background: (r: number, g: number, b: number) => void
  set_cell_pixel_size: (width: number, height: number) => void
}

// Reused for the GPU engine's byte feed (it has no process_str); encoding off-main
// is free, and one encoder avoids a per-chunk allocation.
const textEncoder = new TextEncoder()

let engine: RenderEngine | null = null
let storedInit: StoredInit | null = null
let storedCanvas: OffscreenCanvas | null = null
// Don't fall back twice if the worker posts more than one init/render error.
let fellBackToCpu = false

/** Inject fallback faces + seed palette/selection/reply defaults — byte-for-byte the
 *  same setup the main-thread CPU/GPU drawers do, so both worker engines match. */
function seedEngine(t: SeedTarget, p: StoredInit): void {
  // CJK first RESETS the chain to it, the rest append (parity with the main path).
  if (p.fallbackFonts.length > 0) {
    t.set_fallback_font(p.fallbackFonts[0])
    for (let i = 1; i < p.fallbackFonts.length; i++) {
      t.add_fallback_font(p.fallbackFonts[i])
    }
  }
  seedAtermPalette(t, p.themeColors)
  t.set_selection_fg(p.themeColors.selectionForeground ?? undefined)
  t.set_selection_inactive_bg(p.themeColors.selectionInactive ?? undefined)
  seedAtermReplyDefaults(t, p.themeColors, t.cell_width, t.cell_height)
}

/** Post the cacheable state snapshot. width/height are the framebuffer device-pixel
 *  size, computed differently per engine (CPU: the rgba framebuffer; GPU: the
 *  presented swapchain) and passed in by the caller. */
function postState(
  source: SnapshotSource,
  engineTag: 'cpu' | 'gpu',
  width: number,
  height: number,
  rows: number,
  cols: number
): void {
  ctx.postMessage({
    type: 'state',
    engine: engineTag,
    width,
    height,
    cols,
    rows,
    cellWidth: source.cell_width,
    cellHeight: source.cell_height,
    displayOffset: source.display_offset,
    cursorX: source.cursor_x,
    cursorY: source.cursor_y,
    baseY: source.base_y,
    isAltScreen: source.is_alt_screen,
    title: source.title() ?? null
  })
}

/** CPU engine: rasterize the grid into the OffscreenCanvas via a zero-copy view over
 *  wasm memory (identical to the main-thread painter), then post the snapshot. */
async function buildCpuEngine(p: StoredInit, canvas: OffscreenCanvas): Promise<RenderEngine> {
  const out = await init(wasmUrl)
  const memory = out.memory
  let rows = p.rows
  let cols = p.cols
  const t = new AtermTerminal(
    p.rows,
    p.cols,
    p.fontBytes,
    p.fontPx,
    p.themeColors.fg,
    p.themeColors.bg,
    p.themeColors.cursor,
    p.themeColors.selection
  )
  seedEngine(t, p)
  const canvasCtx = canvas.getContext('2d')
  if (!canvasCtx) {
    t.free()
    throw new Error('OffscreenCanvas 2d context unavailable')
  }
  const renderAndPost = (): void => {
    t.render()
    const width = t.width
    const height = t.height
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width
      canvas.height = height
    }
    const view = new Uint8ClampedArray(memory.buffer, t.rgba_ptr(), width * height * 4)
    canvasCtx.putImageData(new ImageData(view, width, height), 0, 0)
    postState(t, 'cpu', width, height, rows, cols)
  }
  return {
    process: (data) => t.process_str(data),
    renderAndPost,
    resize: (r, c) => {
      rows = r
      cols = c
      t.resize(r, c)
      renderAndPost()
    },
    setPx: (px) => {
      t.set_px(px)
      renderAndPost()
    },
    scrollLines: (delta) => {
      t.scroll_lines(delta)
      renderAndPost()
    },
    scrollToBottom: () => {
      t.scroll_to_bottom()
      renderAndPost()
    },
    dispose: () => {
      try {
        t.free()
      } catch {
        /* ignore */
      }
    }
  }
}

/** GPU engine: present straight into the OffscreenCanvas WebGL2 swapchain — NO rgba
 *  blit (unlike the CPU path). `init_offscreen` is async and MUST finish before any
 *  render; it throws (JS string) if WebGL is unavailable in the worker → the caller
 *  posts an init error so the main side falls back to a CPU worker. */
async function buildGpuEngine(p: StoredInit, canvas: OffscreenCanvas): Promise<RenderEngine> {
  await gpuInit(gpuWasmUrl)
  let rows = p.rows
  let cols = p.cols
  const t = new AtermGpuTerminal(
    p.rows,
    p.cols,
    p.fontBytes,
    p.fontPx,
    p.themeColors.fg,
    p.themeColors.bg,
    p.themeColors.cursor,
    p.themeColors.selection
  )
  // Seed BEFORE init_offscreen so the engine re-applies fonts/theme to the GPU face
  // it builds there (matches aterm-gpu-drawer's seed-then-init ordering).
  seedEngine(t, p)
  try {
    // CRITICAL: acquire the WebGL2 surface on the transferred canvas; must resolve
    // before any render(). Throws if WebGL is unavailable in the worker.
    await t.init_offscreen(canvas)
  } catch (err) {
    try {
      t.free()
    } catch {
      /* ignore */
    }
    throw err
  }
  const renderAndPost = (): void => {
    try {
      // Presents the grid to the swapchain — no readback, no ImageData blit.
      t.render()
    } catch (err) {
      ctx.postMessage({ type: 'error', phase: 'render', message: String(err) })
      return
    }
    // The GPU engine's width/height getters track render_offscreen (unused here), so
    // read the presented framebuffer from the swapchain canvas wgpu sizes; fall back
    // to grid-derived device px if the canvas isn't sized yet.
    const width = canvas.width || Math.round(cols * t.cell_width)
    const height = canvas.height || Math.round(rows * t.cell_height)
    postState(t, 'gpu', width, height, rows, cols)
  }
  return {
    // The GPU engine has no process_str; encode here (off-main, so it's free).
    process: (data) => t.process(textEncoder.encode(data)),
    renderAndPost,
    resize: (r, c) => {
      rows = r
      cols = c
      t.resize(r, c)
      renderAndPost()
    },
    setPx: (px) => {
      t.set_px(px)
      renderAndPost()
    },
    scrollLines: (delta) => {
      t.scroll_lines(delta)
      renderAndPost()
    },
    scrollToBottom: () => {
      t.scroll_to_bottom()
      renderAndPost()
    },
    dispose: () => {
      try {
        t.free()
      } catch {
        /* ignore */
      }
    }
  }
}

async function handleInit(msg: AtermWorkerInit): Promise<void> {
  // Remember everything a CPU fallback would need: the canvas + font bytes were
  // transferred and can't be re-sent from the main thread.
  storedCanvas = msg.canvas
  storedInit = {
    fontBytes: msg.fontBytes,
    fallbackFonts: msg.fallbackFonts,
    rows: msg.rows,
    cols: msg.cols,
    fontPx: msg.fontPx,
    themeColors: msg.themeColors
  }
  if (msg.engine === 'gpu') {
    try {
      engine = await buildGpuEngine(storedInit, storedCanvas)
    } catch (err) {
      // No WebGL in the worker (or acquire failed) — let the main side fall back to a
      // CPU worker on the same canvas instead of crashing/blanking the pane.
      ctx.postMessage({ type: 'error', phase: 'init', message: String(err) })
      return
    }
  } else {
    try {
      engine = await buildCpuEngine(storedInit, storedCanvas)
    } catch (err) {
      ctx.postMessage({ type: 'error', phase: 'init', message: String(err) })
      return
    }
  }
  engine.renderAndPost()
}

/** GPU→CPU fallback: rebuild as a CPU engine on the canvas the worker still holds,
 *  reusing the stored init params, so the pane renders off-main instead of blank. */
async function handleFallback(): Promise<void> {
  if (engine || fellBackToCpu || !storedInit || !storedCanvas) {
    return
  }
  fellBackToCpu = true
  try {
    engine = await buildCpuEngine(storedInit, storedCanvas)
    engine.renderAndPost()
  } catch (err) {
    // The canvas may be poisoned (a WebGL2 context was already acquired on it), so 2d
    // is unavailable — nothing more to recover. Surface it for logging.
    ctx.postMessage({ type: 'error', phase: 'init', message: String(err) })
  }
}

ctx.onmessage = (event): void => {
  const msg = event.data
  switch (msg.type) {
    case 'init':
      void handleInit(msg)
      return
    case 'fallback':
      void handleFallback()
      return
    case 'process':
      engine?.process(msg.data)
      return
    case 'draw':
      engine?.renderAndPost()
      return
    case 'resize':
      engine?.resize(msg.rows, msg.cols)
      return
    case 'setPx':
      engine?.setPx(msg.px)
      return
    case 'scrollLines':
      engine?.scrollLines(msg.delta)
      return
    case 'scrollToBottom':
      engine?.scrollToBottom()
      return
    case 'dispose':
      engine?.dispose()
      engine = null
      storedInit = null
      storedCanvas = null
  }
}
