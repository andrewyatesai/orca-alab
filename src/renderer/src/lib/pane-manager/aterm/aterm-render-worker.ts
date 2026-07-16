// SHARED aterm render worker entry: ONE worker hosts the engines for ALL worker-path
// panes, keyed by paneId. Per pane it owns the transferred OffscreenCanvas + engine:
// parses PTY bytes, renders, runs search/selection/hover/cursor-blink, drains the
// engine's side channels (reply→PTY / OSC / bell) and pushes a per-frame STATE
// snapshot the main thread reads synchronously. The main thread keeps NO engine.
//
// Fonts arrive ONCE per worker lifetime (the 'fonts' message, always first) and stay
// resident; every engine build seeds from them, and the engine-side content-keyed
// intern registry dedupes the bytes across engines within each wasm module — so pane
// N+1 costs an engine, not another copy of the multi-MB font payload.
//
// Draws are coalesced onto ONE shared rAF loop driving all dirty panes; side-channel
// events post immediately per processed chunk so none are dropped. A PANE-scoped
// engine-build failure posts a pane 'error' (→ CPU rebuild on the same canvas); an
// exception escaping dispatch posts the worker-scoped 'crash' — the wasm module state
// is suspect for EVERY engine in it, so the manager retires the whole worker.

import {
  buildCpuEngine,
  buildGpuEngine,
  type EngineHandle,
  type StoredInit
} from './aterm-worker-engine-build'
import { applyResidentFontClass, type WorkerResidentFonts } from './aterm-worker-font-registry'
import { createWorkerTerminal } from './aterm-worker-terminal'
import { createWorkerSerializeCache } from './aterm-worker-serialize-cache'
import {
  createSharedWorkerRafLoop,
  createWorkerFrameScheduler
} from './aterm-worker-frame-scheduler'
import {
  dispatchPaneCommand,
  type EngineSettingSetters,
  type PaneRuntime
} from './aterm-worker-pane-dispatch'
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

// The worker-resident font faces (set by the 'fonts' message before any pane init;
// E1: primary-only at boot, fallback classes stream in via 'fontClass' on demand).
let fonts: WorkerResidentFonts | null = null

// Classes already announced to the manager (bit 1 = text, 2 = emoji). Latched for
// the worker's lifetime: a class is requested at most once — if the delivered
// faces still miss a char the engine re-fires the bit, and re-requesting would
// loop (the engine docs require the host to latch per class).
let announcedFontClasses = 0

const panes = new Map<number, PaneRuntime>()

/** Post-frame drain (E1): collect the engine's missing-font class bits and ask the
 *  manager for any class not yet announced. Cheap — one wasm call returning a u8. */
function reportMissingFontClasses(pane: PaneRuntime): void {
  if (!pane.engine) {
    return
  }
  let bits = 0
  try {
    bits = pane.engine.take_missing_font_classes()
  } catch {
    return
  }
  const fresh = bits & ~announcedFontClasses
  if (!fresh) {
    return
  }
  announcedFontClasses |= fresh
  const classes: ('text' | 'emoji')[] = []
  if (fresh & 1) {
    classes.push('text')
  }
  if (fresh & 2) {
    classes.push('emoji')
  }
  ctx.postMessage({ type: 'missingFontClasses', classes })
}

// ONE rAF loop for every pane's frame scheduler (see createSharedWorkerRafLoop).
const sharedRaf = createSharedWorkerRafLoop(
  ctx.requestAnimationFrame ? (cb) => ctx.requestAnimationFrame?.(cb) : undefined
)

function createPane(paneId: number): PaneRuntime {
  const pane: PaneRuntime = {
    paneId,
    term: null,
    engineSetters: null,
    engine: null,
    engineKind: null,
    storedInit: null,
    canvas: null,
    fellBackToCpu: false,
    disposed: false,
    chrome: { pad: 0, head: 0 },
    // Both per-pane by design: dirty/suspend state and the serialize-cache timers
    // must be isolated so one pane's dispose/suspend can't touch another's.
    frameScheduler: createWorkerFrameScheduler({
      getTerm: () => pane.term,
      post: (state) => {
        pane.post(state)
        // After the frame that exposed them: any `.notdef` misses this render
        // recorded reach the manager while the pane is still on glass (E1).
        reportMissingFontClasses(pane)
      },
      raf: sharedRaf
    }),
    serializeCache: createWorkerSerializeCache({
      getTerm: () => pane.term,
      post: (message) => pane.post(message)
    }),
    post: (event) => ctx.postMessage({ ...event, paneId })
  }
  return pane
}

/** Wrap a built engine in the worker terminal and size it. Cursor focus/blink state
 *  arrives shortly after via commands from the main-thread blink timer. */
function startTerminal(pane: PaneRuntime, handle: EngineHandle): void {
  pane.term = createWorkerTerminal(handle, () => pane.chrome)
  pane.engineSetters = handle.engine as unknown as EngineSettingSetters
  pane.engine = handle.engine
  pane.engineKind = handle.kind
  // A rebuild (GPU→CPU fallback) constructs a fresh chrome-less engine; re-apply
  // the pane's stored chrome so the effect frame survives the swap.
  if (pane.chrome.pad !== 0 || pane.chrome.head !== 0) {
    pane.engineSetters.set_chrome(pane.chrome.pad, pane.chrome.head)
  }
  if (pane.storedInit) {
    pane.term.resize(pane.storedInit.rows, pane.storedInit.cols)
  }
  // The first frame MUST post: the loader awaits this initial STATE for the cell metrics.
  pane.frameScheduler.postNow()
}

async function buildAndStart(pane: PaneRuntime, build: () => Promise<EngineHandle>): Promise<void> {
  let handle: EngineHandle
  try {
    handle = await build()
  } catch (err) {
    // PANE-scoped: GPU acquire (or CPU init) failed — the loader falls this pane back
    // to a CPU rebuild on the same canvas rather than crashing/blanking it.
    pane.post({ type: 'error', phase: 'init', message: String(err) })
    return
  }
  // Disposed while the engine was building (pane closed / loader gave up on it):
  // free the engine now — nothing will ever drive it.
  if (pane.disposed) {
    handle.dispose()
    return
  }
  try {
    startTerminal(pane, handle)
  } catch (err) {
    // A wasm panic on the FIRST resize/render poisons the module for every engine in
    // it — escalate as a worker-fatal crash so all panes rebuild in-process.
    ctx.postMessage({
      type: 'crash',
      message: err instanceof Error && err.stack ? err.stack : String(err)
    })
    throw err
  }
}

function handleInit(msg: AtermWorkerInit & { paneId: number }): void {
  const pane = createPane(msg.paneId)
  panes.set(msg.paneId, pane)
  pane.canvas = msg.canvas
  if (!fonts) {
    // The manager always posts 'fonts' before the first init; reaching here means the
    // contract broke — fail the pane (loader falls back in-process) instead of
    // building an engine with no faces.
    pane.post({ type: 'error', phase: 'init', message: 'no resident fonts before pane init' })
    return
  }
  const stored: StoredInit = {
    fonts,
    rows: msg.rows,
    cols: msg.cols,
    fontPx: msg.fontPx,
    lineHeight: msg.lineHeight,
    themeColors: msg.themeColors
  }
  pane.storedInit = stored
  void buildAndStart(pane, () =>
    msg.engine === 'gpu' ? buildGpuEngine(stored, msg.canvas) : buildCpuEngine(stored, msg.canvas)
  )
}

/** GPU→CPU fallback: rebuild this pane as CPU on the canvas the worker still holds,
 *  reusing its stored init params, so it renders off-main instead of going blank. */
function handleFallback(paneId: number): void {
  const pane = panes.get(paneId)
  if (!pane || pane.term || pane.fellBackToCpu || !pane.storedInit || !pane.canvas) {
    return
  }
  pane.fellBackToCpu = true
  const stored = pane.storedInit
  const canvas = pane.canvas
  void buildAndStart(pane, () => buildCpuEngine(stored, canvas))
}

/** Free ONE pane's engine + worker-side state; every other pane is untouched. */
function handleDispose(paneId: number): void {
  const pane = panes.get(paneId)
  if (!pane) {
    return
  }
  pane.disposed = true // a still-building engine is freed when its build resolves
  pane.frameScheduler.dispose()
  pane.serializeCache.dispose()
  pane.term?.dispose()
  pane.term = null
  pane.engineSetters = null
  pane.engine = null
  pane.engineKind = null
  pane.storedInit = null
  pane.canvas = null
  panes.delete(paneId)
}

ctx.onmessage = (event): void => {
  try {
    dispatch(event.data)
  } catch (err) {
    // A wasm RuntimeError escaping here poisons the module state for EVERY engine in
    // it, and the bare worker 'error' event carries no structured payload — post a
    // worker-scoped crash first so the manager retires the worker and each pane
    // rebuilds in-process, then rethrow to keep the error-event/console semantics.
    ctx.postMessage({
      type: 'crash',
      message: err instanceof Error && err.stack ? err.stack : String(err)
    })
    throw err
  }
}

function dispatch(msg: AtermWorkerRequest): void {
  // Worker-scoped + lifecycle first (narrowing the union), then the per-pane runtime
  // commands go to the pane's dispatcher.
  if (msg.type === 'fonts') {
    fonts = {
      primary: msg.primary,
      fallbacks: msg.fallbacks,
      emoji: msg.emoji,
      symbol: msg.symbol
    }
    // Ack BEFORE any engine build: builds take seconds (wasm compile + font parse +
    // GL acquire), and without the ack the manager/loader can't tell "alive but
    // building" from "wedged".
    ctx.postMessage({ type: 'booted' })
    return
  }
  if (msg.type === 'fontClass') {
    if (!fonts) {
      return
    }
    // Grow the resident faces (future engine builds seed them via ensureModuleHandles)…
    if (msg.class === 'text') {
      fonts.fallbacks = msg.fallbacks ?? []
      fonts.symbol = msg.symbol
    } else {
      fonts.emoji = msg.emoji
    }
    // …and retrofit every LIVE engine: the installers clear the per-char memos and
    // force a full repaint, so `.notdef` cells re-render through the new faces.
    for (const pane of panes.values()) {
      if (pane.engine && pane.engineKind) {
        applyResidentFontClass(pane.engine, pane.engineKind, msg.class, fonts)
        // Render-only: content is unchanged, the glyphs just resolve now.
        pane.frameScheduler.schedule(false)
      }
    }
    return
  }
  if (msg.type === 'init') {
    handleInit(msg)
    return
  }
  if (msg.type === 'fallback') {
    handleFallback(msg.paneId)
    return
  }
  if (msg.type === 'dispose') {
    handleDispose(msg.paneId)
    return
  }
  const pane = panes.get(msg.paneId)
  if (pane) {
    dispatchPaneCommand(pane, msg)
  }
}
