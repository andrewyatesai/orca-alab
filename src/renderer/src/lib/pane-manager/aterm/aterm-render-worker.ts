// Off-main-thread aterm render worker (plan §9, stage 2 — NOT yet wired into pane
// creation; the opt-in strategy that drives it lands in a later stage gated behind
// an explicit flag, so production keeps using the proven main-thread path until
// this is validated in Electron/CDP).
//
// Owns an aterm engine + the pane's transferred OffscreenCanvas and does the
// per-frame work (process → render → zero-copy blit) here, so heavy terminal
// output no longer competes with the renderer main thread. After each frame it
// posts a small cacheable STATE snapshot so the main thread's draw/follow-bottom
// logic stays synchronous without an RPC round-trip.

import init, { AtermTerminal } from './aterm_wasm.js'
import wasmUrl from './aterm_wasm_bg.wasm?url'
import { seedAtermPalette, seedAtermReplyDefaults } from './aterm-theme-colors'
import type {
  AtermWorkerInit,
  AtermWorkerRequest,
  AtermWorkerState
} from './aterm-render-worker-protocol'

// DedicatedWorkerGlobalScope without pulling in the WebWorker lib (which clashes
// with the DOM lib this project compiles against): cast the minimal surface used.
const ctx = self as unknown as {
  onmessage: ((event: { data: AtermWorkerRequest }) => void) | null
  postMessage: (message: AtermWorkerState) => void
}

let term: AtermTerminal | null = null
let canvasCtx: OffscreenCanvasRenderingContext2D | null = null
let memory: WebAssembly.Memory | null = null
let rows = 0
let cols = 0

async function handleInit(msg: AtermWorkerInit): Promise<void> {
  const out = await init(wasmUrl)
  memory = out.memory
  rows = msg.rows
  cols = msg.cols
  const t = new AtermTerminal(
    msg.rows,
    msg.cols,
    msg.fontBytes,
    msg.fontPx,
    msg.themeColors.fg,
    msg.themeColors.bg,
    msg.themeColors.cursor,
    msg.themeColors.selection
  )
  // Fallback faces (CJK first resets the chain, the rest append) — bytes are sent
  // from the main thread since the worker has no window.api to fetch them.
  if (msg.fallbackFonts.length > 0) {
    t.set_fallback_font(msg.fallbackFonts[0])
    for (let i = 1; i < msg.fallbackFonts.length; i++) {
      t.add_fallback_font(msg.fallbackFonts[i])
    }
  }
  seedAtermPalette(t, msg.themeColors)
  t.set_selection_fg(msg.themeColors.selectionForeground ?? undefined)
  t.set_selection_inactive_bg(msg.themeColors.selectionInactive ?? undefined)
  const cellWidth = t.cell_width
  const cellHeight = t.cell_height
  seedAtermReplyDefaults(t, msg.themeColors, cellWidth, cellHeight)
  term = t
  canvasCtx = msg.canvas.getContext('2d')
  renderAndPost()
}

/** Rasterize the current grid into the OffscreenCanvas (zero-copy view over wasm
 *  memory, identical to the main-thread painter) and post the state snapshot. */
function renderAndPost(): void {
  if (!term || !canvasCtx || !memory) {
    return
  }
  term.render()
  const width = term.width
  const height = term.height
  const offscreen = canvasCtx.canvas
  if (offscreen.width !== width || offscreen.height !== height) {
    offscreen.width = width
    offscreen.height = height
  }
  const view = new Uint8ClampedArray(memory.buffer, term.rgba_ptr(), width * height * 4)
  canvasCtx.putImageData(new ImageData(view, width, height), 0, 0)
  ctx.postMessage({
    type: 'state',
    width,
    height,
    cols,
    rows,
    cellWidth: term.cell_width,
    cellHeight: term.cell_height,
    displayOffset: term.display_offset,
    cursorX: term.cursor_x,
    cursorY: term.cursor_y,
    baseY: term.base_y,
    isAltScreen: term.is_alt_screen,
    title: term.title() ?? null
  })
}

ctx.onmessage = (event): void => {
  const msg = event.data
  switch (msg.type) {
    case 'init':
      void handleInit(msg)
      return
    case 'process':
      term?.process_str(msg.data)
      return
    case 'draw':
      renderAndPost()
      return
    case 'resize':
      rows = msg.rows
      cols = msg.cols
      term?.resize(msg.rows, msg.cols)
      renderAndPost()
      return
    case 'setPx':
      term?.set_px(msg.px)
      renderAndPost()
      return
    case 'scrollLines':
      term?.scroll_lines(msg.delta)
      renderAndPost()
      return
    case 'scrollToBottom':
      term?.scroll_to_bottom()
      renderAndPost()
      return
    case 'dispose':
      term?.free()
      term = null
      canvasCtx = null
      memory = null
  }
}
