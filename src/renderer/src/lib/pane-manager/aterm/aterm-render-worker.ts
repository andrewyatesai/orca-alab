// Single-engine aterm render worker entry (plan: aterm-single-engine-worker.md). The
// worker owns the ONLY engine for a pane + its transferred OffscreenCanvas: it parses
// PTY bytes, renders, runs search/selection/hover/cursor-blink, drains the engine's
// side channels (reply→PTY / OSC / bell) and pushes a per-frame STATE snapshot the
// main thread reads synchronously. The main thread keeps NO engine.
//
// Draws are coalesced to one rAF frame (state posted on the draw); side-channel events
// are posted immediately per processed chunk so none are dropped. GPU init failure
// posts an 'error' so the main side asks for a CPU rebuild on the same canvas.

import {
  buildCpuEngine,
  buildGpuEngine,
  type EngineHandle,
  type StoredInit
} from './aterm-worker-engine-build'
import { createWorkerTerminal } from './aterm-worker-terminal'
import { createWorkerSerializeCache } from './aterm-worker-serialize-cache'
import { createWorkerFrameScheduler } from './aterm-worker-frame-scheduler'
import type {
  AtermWorkerInit,
  AtermWorkerMessage,
  AtermWorkerRequest
} from './aterm-render-worker-protocol'

// DedicatedWorkerGlobalScope without the WebWorker lib (it clashes with the DOM lib
// this project compiles against): cast the minimal surface used.
const ctx = self as unknown as {
  onmessage: ((event: { data: AtermWorkerRequest }) => void) | null
  postMessage: (message: AtermWorkerMessage) => void
  requestAnimationFrame?: (cb: () => void) => number
}

type WorkerTerminal = ReturnType<typeof createWorkerTerminal>

// Both engine bindings ship these (aterm_wasm/aterm_gpu_web), but the WorkerEngine
// Pick + worker terminal predate them — cast here, surgically, until the planned
// worker refactor folds them into aterm-worker-terminal.
type EngineSettingSetters = {
  set_minimum_contrast: (ratio: number) => void
  set_word_separators: (separators?: string | null) => void
  set_background_opacity: (opacity: number) => void
  set_cursor_opacity: (opacity: number) => void
  set_kitty_keyboard_enabled: (enabled: boolean) => void
}

let term: WorkerTerminal | null = null
let engineSetters: EngineSettingSetters | null = null
let storedInit: StoredInit | null = null
let storedCanvas: OffscreenCanvas | null = null
// Don't fall back twice if the worker posts more than one init error.
let fellBackToCpu = false
// Serialized-buffer cache (throttle-with-max-wait) so the main thread can read recent
// scrollback synchronously at shutdown; the throttle logic lives in its own module.
const serializeCache = createWorkerSerializeCache({
  getTerm: () => term,
  post: (message) => ctx.postMessage(message)
})

// Draw coalescing + STATE-post decisions (render-only blink frames, hidden-pane gating,
// dimension-change posts) live in the frame scheduler.
const frameScheduler = createWorkerFrameScheduler({
  getTerm: () => term,
  post: (state) => ctx.postMessage(state),
  raf: ctx.requestAnimationFrame ? (cb) => ctx.requestAnimationFrame?.(cb) : undefined
})
const scheduleDraw = frameScheduler.schedule

function buildStoredInit(msg: AtermWorkerInit): StoredInit {
  return {
    fontBytes: msg.fontBytes,
    fallbackFonts: msg.fallbackFonts,
    emojiFont: msg.emojiFont,
    rows: msg.rows,
    cols: msg.cols,
    fontPx: msg.fontPx,
    lineHeight: msg.lineHeight,
    themeColors: msg.themeColors
  }
}

/** Wrap a built engine in the worker terminal and size it. Cursor focus/blink state
 *  arrives shortly after via commands from the main-thread blink timer. */
function startTerminal(handle: EngineHandle): void {
  term = createWorkerTerminal(handle)
  engineSetters = handle.engine as unknown as EngineSettingSetters
  if (storedInit) {
    term.resize(storedInit.rows, storedInit.cols)
  }
  // The first frame MUST post: the loader awaits this initial STATE for the cell metrics.
  frameScheduler.postNow()
}

async function handleInit(msg: AtermWorkerInit): Promise<void> {
  // Ack BEFORE the engine build: it takes seconds (wasm compile + font parse + GL
  // acquire, stretched further by concurrent pane opens), and without the ack the
  // loader can't tell "alive but building" from "wedged" and would kill a healthy
  // worker at its short first-frame deadline.
  ctx.postMessage({ type: 'booted' })
  storedCanvas = msg.canvas
  storedInit = buildStoredInit(msg)
  let handle: EngineHandle
  try {
    handle =
      msg.engine === 'gpu'
        ? await buildGpuEngine(storedInit, storedCanvas)
        : await buildCpuEngine(storedInit, storedCanvas)
  } catch (err) {
    // GPU acquire (or CPU init) failed — let the main side fall back to a CPU worker on
    // the same canvas rather than crashing/blanking the pane.
    ctx.postMessage({ type: 'error', phase: 'init', message: String(err) })
    return
  }
  startTerminal(handle)
}

/** GPU→CPU fallback: rebuild as CPU on the canvas the worker still holds, reusing the
 *  stored init params, so the pane renders off-main instead of going blank. */
async function handleFallback(): Promise<void> {
  if (term || fellBackToCpu || !storedInit || !storedCanvas) {
    return
  }
  fellBackToCpu = true
  try {
    const handle = await buildCpuEngine(storedInit, storedCanvas)
    startTerminal(handle)
  } catch (err) {
    ctx.postMessage({ type: 'error', phase: 'init', message: String(err) })
  }
}

ctx.onmessage = (event): void => {
  try {
    dispatch(event.data)
  } catch (err) {
    // A wasm RuntimeError escaping here would only fire the worker 'error' event
    // (no structured payload across browsers) and leave the pane frozen — post a
    // runtime error first so the loader rebuilds the pane in-process, then rethrow
    // to keep the worker's own error-event/console semantics.
    ctx.postMessage({ type: 'error', phase: 'runtime', message: String(err) })
    throw err
  }
}

function dispatch(msg: AtermWorkerRequest): void {
  switch (msg.type) {
    case 'init':
      void handleInit(msg)
      return
    case 'fallback':
      void handleFallback()
      return
    case 'process': {
      if (!term) {
        return
      }
      const side = term.processBytes(msg.data)
      // Post the edge-triggered side channels immediately (NOT coalesced) so none are
      // dropped: replies → PTY, OSC app-events → dispatch, bell → re-emit.
      if (side.reply) {
        ctx.postMessage({ type: 'reply', data: side.reply })
      }
      if (side.osc) {
        ctx.postMessage({ type: 'osc', events: side.osc })
      }
      if (side.bell) {
        ctx.postMessage({ type: 'bell' })
      }
      scheduleDraw()
      serializeCache.schedule()
      return
    }
    case 'draw':
      scheduleDraw()
      return
    case 'resize':
      term?.resize(msg.rows, msg.cols)
      scheduleDraw()
      return
    case 'setPx':
      term?.setPx(msg.px)
      scheduleDraw()
      return
    case 'setLineHeight':
      term?.setLineHeight(msg.lineHeight)
      scheduleDraw()
      return
    case 'setLigatures':
      term?.setLigatures(msg.on)
      scheduleDraw()
      return
    case 'setScrollbackLimit':
      term?.setScrollbackLimit(msg.lines)
      return
    case 'setMinimumContrast':
      // Appearance-only: repaint so the floored fg shows without waiting for output.
      engineSetters?.set_minimum_contrast(msg.ratio)
      scheduleDraw()
      return
    case 'setWordSeparators':
      // Selection-behavior only (next double-click) — no repaint needed.
      engineSetters?.set_word_separators(msg.separators ?? undefined)
      return
    case 'setBackgroundOpacity':
      // Appearance-only: repaint so the translucent default bg shows immediately.
      engineSetters?.set_background_opacity(msg.opacity)
      scheduleDraw()
      return
    case 'setCursorOpacity':
      engineSetters?.set_cursor_opacity(msg.opacity)
      scheduleDraw()
      return
    case 'setKittyKeyboardEnabled':
      // Protocol capability only (affects future CSI ? u replies) — no repaint.
      engineSetters?.set_kitty_keyboard_enabled(msg.enabled)
      return
    case 'setDefaultCursorStyle':
      term?.setDefaultCursorStyle(msg.param)
      scheduleDraw()
      return
    case 'setColorScheme': {
      // set_color_scheme may queue a CSI ?997 push (when the scheme changed AND the app
      // enabled DEC 2031); forward it through the reply channel → main → PTY.
      const reply = term ? term.setColorScheme(msg.dark) : ''
      if (reply) {
        ctx.postMessage({ type: 'reply', data: reply })
      }
      return
    }
    case 'scrollLines':
      term?.scrollLines(msg.delta)
      scheduleDraw()
      return
    case 'scrollToBottom':
      term?.scrollToBottom()
      scheduleDraw()
      return
    case 'scrollToTop':
      term?.scrollToTop()
      scheduleDraw()
      return
    case 'scrollToLine':
      term?.scrollToLine(msg.line)
      scheduleDraw()
      return
    case 'selectionStart':
      term?.selectionStart(msg.row, msg.col)
      scheduleDraw()
      return
    case 'selectionExtend':
      term?.selectionExtend(msg.row, msg.col)
      scheduleDraw()
      return
    case 'selectionFinish':
      term?.selectionFinish()
      scheduleDraw()
      return
    case 'selectionWord':
      term?.selectionWord(msg.row, msg.col)
      scheduleDraw()
      return
    case 'selectionLine':
      term?.selectionLine(msg.row, msg.col)
      scheduleDraw()
      return
    case 'selectionClear':
      term?.selectionClear()
      scheduleDraw()
      return
    case 'themeSet':
      term?.themeSet(msg)
      scheduleDraw()
      return
    case 'setSelectionInactive':
      term?.setSelectionInactive(msg.inactive)
      scheduleDraw()
      return
    case 'setSelectionInactiveBg':
      term?.setSelectionInactiveBg(msg.bg)
      scheduleDraw()
      return
    case 'setClipboardWriteAuthorized':
      term?.setClipboardWriteAuthorized(msg.allowed)
      return
    case 'setDrawSuspended':
      frameScheduler.setSuspended(msg.suspended)
      return
    case 'setCursorBlinkPhase':
      // Render-only: repaint the cursor cell, but post NO state (no snapshot field tracks
      // blink phase, so the STATE would be byte-identical).
      term?.setCursorBlinkPhase(msg.on)
      scheduleDraw(false)
      return
    case 'setCursorHollow':
      term?.setCursorHollow(msg.hollow)
      scheduleDraw(false)
      return
    case 'setHover':
      term?.setHover('clear' in msg ? null : { row: msg.row, col: msg.col })
      scheduleDraw()
      return
    case 'searchFind':
      term?.searchFind(msg.query, msg.caseSensitive, msg.isRegex)
      scheduleDraw()
      return
    case 'searchNext':
      term?.searchNext()
      scheduleDraw()
      return
    case 'searchPrev':
      term?.searchPrev()
      scheduleDraw()
      return
    case 'searchClear':
      term?.searchClear()
      scheduleDraw()
      return
    case 'setPrimaryFont':
      term?.setPrimaryFont(msg.bytes)
      scheduleDraw()
      return
    case 'setBoldFont':
      term?.setBoldFont(msg.bytes)
      scheduleDraw()
      return
    case 'mouseEncode': {
      // The encoded mouse report is PTY input — forward it through the reply channel
      // (→ main onReply → inputSink), same as engine query replies.
      const data = term
        ? term.mouseEncode(msg.kind, msg.col, msg.row, msg.button, msg.mods, msg.up ?? false)
        : ''
      if (data) {
        ctx.postMessage({ type: 'reply', data })
      }
      return
    }
    case 'query': {
      // 'flush' is a parse fence, not an engine read: reaching it means every
      // earlier message (process bytes + their posted replies) was handled, so
      // answer directly — even with no engine yet.
      const value =
        msg.kind === 'flush' ? true : term ? term.query(msg.kind, msg.arg, msg.arg2) : null
      ctx.postMessage({ type: 'queryResult', id: msg.id, value })
      return
    }
    case 'dispose':
      serializeCache.dispose()
      term?.dispose()
      term = null
      engineSetters = null
      storedInit = null
      storedCanvas = null
  }
}
