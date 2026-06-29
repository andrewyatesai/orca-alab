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

let term: WorkerTerminal | null = null
let storedInit: StoredInit | null = null
let storedCanvas: OffscreenCanvas | null = null
// Don't fall back twice if the worker posts more than one init error.
let fellBackToCpu = false
let suspended = false
let drawScheduled = false
// Serialized-buffer cache: pushed so the main thread has a recent buffer to read
// SYNCHRONOUSLY at shutdown layout-capture. Throttle-with-max-wait, NOT a pure
// debounce: a continuously-busy pane would reset a debounce forever and never cache
// (then the shutdown read gets an empty/stale blob and the pane's scrollback is lost).
let cacheTimer: ReturnType<typeof setTimeout> | null = null
let cacheMaxWaitTimer: ReturnType<typeof setTimeout> | null = null
const SERIALIZE_CACHE_DEBOUNCE_MS = 1000
const SERIALIZE_CACHE_MAX_WAIT_MS = 5000

const flushSerializeCache = (): void => {
  if (cacheTimer !== null) {
    clearTimeout(cacheTimer)
    cacheTimer = null
  }
  if (cacheMaxWaitTimer !== null) {
    clearTimeout(cacheMaxWaitTimer)
    cacheMaxWaitTimer = null
  }
  if (term) {
    const { full, scrollback } = term.serializedCache()
    ctx.postMessage({ type: 'serializedCache', full, scrollback })
  }
}

const scheduleSerializeCache = (): void => {
  // Debounce: refresh ~1s after output settles (the common idle case).
  if (cacheTimer !== null) {
    clearTimeout(cacheTimer)
  }
  cacheTimer = setTimeout(flushSerializeCache, SERIALIZE_CACHE_DEBOUNCE_MS)
  // Max-wait floor: guarantee a refresh at least every MAX_WAIT even while output
  // streams continuously (the debounce above would otherwise never fire).
  if (cacheMaxWaitTimer === null) {
    cacheMaxWaitTimer = setTimeout(flushSerializeCache, SERIALIZE_CACHE_MAX_WAIT_MS)
  }
}

const drawNow = (): void => {
  if (!term) {
    return
  }
  // Suspended (hidden pane): keep state fresh for reads but don't paint a frame.
  if (!suspended) {
    term.render()
  }
  ctx.postMessage(term.buildState())
}

const scheduleDraw = (): void => {
  if (drawScheduled || !term) {
    return
  }
  drawScheduled = true
  const run = (): void => {
    drawScheduled = false
    drawNow()
  }
  // OffscreenCanvas exposes rAF in a worker; fall back to sync if it's missing.
  if (ctx.requestAnimationFrame) {
    ctx.requestAnimationFrame(run)
  } else {
    run()
  }
}

function buildStoredInit(msg: AtermWorkerInit): StoredInit {
  return {
    fontBytes: msg.fontBytes,
    fallbackFonts: msg.fallbackFonts,
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
  if (storedInit) {
    term.resize(storedInit.rows, storedInit.cols)
  }
  drawNow()
}

async function handleInit(msg: AtermWorkerInit): Promise<void> {
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
  const msg = event.data
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
      scheduleSerializeCache()
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
      suspended = msg.suspended
      if (!suspended) {
        scheduleDraw()
      }
      return
    case 'setCursorBlinkPhase':
      term?.setCursorBlinkPhase(msg.on)
      scheduleDraw()
      return
    case 'setCursorHollow':
      term?.setCursorHollow(msg.hollow)
      scheduleDraw()
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
    case 'forceReflow':
      // Metrics are re-read into the next state; just repaint.
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
      const value = term ? term.query(msg.kind, msg.arg, msg.arg2) : null
      ctx.postMessage({ type: 'queryResult', id: msg.id, value })
      return
    }
    case 'dispose':
      if (cacheTimer !== null) {
        clearTimeout(cacheTimer)
        cacheTimer = null
      }
      term?.dispose()
      term = null
      storedInit = null
      storedCanvas = null
  }
}
