// The SHARED render-worker manager (audit E1): ONE Web Worker hosts the engines for
// every worker-path pane, instead of one worker per pane.
//
// WHY SHARED: the per-pane design shipped the primary face + the CJK/script fallback
// chain + the colour-emoji face (tens to hundreds of MB) into EVERY pane's worker and
// instantiated a fresh wasm module there — the engine's content-keyed font intern
// registry only dedupes across engines living in the SAME wasm instance. One worker =
// one instance per module (cpu/gpu) = fonts sent once, interned once, and one wasm
// baseline, so pane 2..N cost an engine (a few MB), not another font payload.
//
// OWNERSHIP: this module owns the worker's lifecycle. It is created lazily on the
// first acquired pane, reference-counted, and TERMINATED when the last pane releases
// (memory over warmth: an idle app keeps no worker + no resident fonts; the next pane
// pays one worker spawn + wasm compile + font send again). Fonts are fetched once per
// renderer (cached promise) and posted once per worker generation, before any init.
// Each pane gets a paneId-stamping `post` and per-pane event routing, so the loader
// never sees another pane's traffic.
//
// CRASH SEMANTICS: a wasm RuntimeError in the worker poisons the module state for
// every engine in it, so any crash signal (the worker-scoped 'crash' message, or the
// Worker 'error' event) RETIRES the whole worker: every live pane's onCrash handler
// fires (each rebuilds in-process through its own context-loss seam, seeded from its
// own serialize cache) and the worker is terminated; the next pane open lazily
// recreates a fresh one. Retirement is one-shot per generation.

import { loadAterm } from './load-aterm'
import type {
  AtermWorkerMessage,
  AtermWorkerPaneCommand,
  AtermWorkerPaneEvent
} from './aterm-render-worker-protocol'

/** OS fallback faces for the worker engines: the monochrome fallback chain (CJK first
 *  — set_fallback_font RESETS the chain to it — then the script chain) plus the colour
 *  emoji face (set_emoji_font) and the monochrome symbol face (set_symbol_font), each
 *  kept SEPARATE because the chain + symbol render monochrome and emoji renders colour. */
type SharedWorkerFonts = {
  primary: Uint8Array
  fallbacks: Uint8Array[]
  emoji: Uint8Array | null
  symbol: Uint8Array | null
}

/** One pane's handle onto the shared worker. Every method is a safe no-op once the
 *  pane is released or the worker generation was retired (crash/boot-wedge). */
export type AtermSharedWorkerPane = {
  paneId: number
  /** Post a pane command; the manager stamps this pane's paneId on the wire. */
  post: (cmd: AtermWorkerPaneCommand, transfer?: Transferable[]) => void
  /** Subscribe to this pane's events (state/reply/osc/bell/queryResult/…). */
  onEvent: (handler: (event: AtermWorkerPaneEvent) => void) => void
  /** Subscribe to a worker-fatal crash (fires once; the worker is already retired). */
  onCrash: (handler: (message: string) => void) => void
  /** Whether the worker acked its 'fonts' message (alive vs possibly wedged). */
  isBooted: () => boolean
  /** Boot-deadline escalation: no ack in time — retire the worker, failing every
   *  pane on it through onCrash (they all share the same wedged worker). */
  reportBootWedged: (message: string) => void
  /** Drop this pane; terminates the worker when it was the last one. */
  release: () => void
}

type PaneClient = {
  onEvent: ((event: AtermWorkerPaneEvent) => void) | null
  onCrash: ((message: string) => void) | null
}

type WorkerLike = {
  postMessage: (message: unknown, transfer: Transferable[]) => void
  terminate: () => void
  addEventListener: {
    (type: 'message', listener: (event: MessageEvent<AtermWorkerMessage>) => void): void
    (type: 'error', listener: (event: ErrorEvent) => void): void
  }
}

type SharedWorkerHostDeps = {
  createWorker: () => WorkerLike
  loadFonts: () => Promise<SharedWorkerFonts>
}

export type AtermSharedWorkerHost = {
  acquirePane: () => Promise<AtermSharedWorkerPane>
}

/** Factory (deps injected) so unit tests can drive the lifecycle with a fake Worker;
 *  production uses the module-level singleton below. */
export function createAtermSharedWorkerHost(deps: SharedWorkerHostDeps): AtermSharedWorkerHost {
  type Generation = {
    worker: WorkerLike
    booted: boolean
    retired: boolean
    panes: Map<number, PaneClient>
  }
  let current: Generation | null = null
  let nextPaneId = 1

  const retire = (gen: Generation, message: string): void => {
    // One-shot per generation; a second signal (error event after the crash message)
    // must not re-fire recovery on already-rebuilt panes.
    if (gen.retired) {
      return
    }
    gen.retired = true
    if (current === gen) {
      current = null
    }
    const clients = [...gen.panes.values()]
    gen.panes.clear()
    gen.worker.terminate()
    for (const client of clients) {
      client.onCrash?.(message)
    }
  }

  const ensureWorker = (fonts: SharedWorkerFonts): Generation => {
    if (current) {
      return current
    }
    const worker = deps.createWorker()
    const gen: Generation = { worker, booted: false, retired: false, panes: new Map() }
    worker.addEventListener('message', (event) => {
      const data = event.data
      if (data.type === 'booted') {
        gen.booted = true
        return
      }
      if (data.type === 'crash') {
        retire(gen, data.message)
        return
      }
      gen.panes.get(data.paneId)?.onEvent?.(data)
    })
    worker.addEventListener('error', (event) => {
      // Nobody else listens for escaped worker exceptions — without this every pane
      // silently freezes at its last frame while keystrokes keep flowing.
      retire(gen, event.message || 'uncaught worker error')
    })
    // Fonts ONCE per worker generation, before any pane init. Slice the renderer-side
    // cache so transferring the buffers doesn't detach it (a post-crash generation
    // re-sends from the same cache).
    const primary = fonts.primary.slice()
    const fallbacks = fonts.fallbacks.map((f) => f.slice())
    const emoji = fonts.emoji ? fonts.emoji.slice() : undefined
    const symbol = fonts.symbol ? fonts.symbol.slice() : undefined
    worker.postMessage({ type: 'fonts', primary, fallbacks, emoji, symbol }, [
      primary.buffer,
      ...fallbacks.map((f) => f.buffer),
      ...(emoji ? [emoji.buffer] : []),
      ...(symbol ? [symbol.buffer] : [])
    ])
    current = gen
    return gen
  }

  return {
    acquirePane: async () => {
      // Fetch fonts BEFORE touching the worker: a font/asset failure here throws with
      // the caller's canvas still intact, so it can fall back to the in-process path.
      const fonts = await deps.loadFonts()
      const gen = ensureWorker(fonts)
      const paneId = nextPaneId++
      const client: PaneClient = { onEvent: null, onCrash: null }
      gen.panes.set(paneId, client)
      const live = (): boolean => !gen.retired && gen.panes.has(paneId)
      return {
        paneId,
        post: (cmd, transfer) => {
          if (live()) {
            gen.worker.postMessage({ ...cmd, paneId }, transfer ?? [])
          }
        },
        onEvent: (handler) => {
          client.onEvent = handler
        },
        onCrash: (handler) => {
          client.onCrash = handler
        },
        isBooted: () => gen.booted,
        reportBootWedged: (message) => retire(gen, message),
        release: () => {
          if (!live()) {
            return
          }
          gen.panes.delete(paneId)
          if (gen.panes.size === 0) {
            // Terminate-on-last-close: reclaim the resident fonts + wasm baseline
            // while no pane needs them; the next pane recreates the worker.
            gen.retired = true
            if (current === gen) {
              current = null
            }
            gen.worker.terminate()
          }
        }
      }
    }
  }
}

// The renderer-wide font fetch, shared across worker generations (the OS faces are
// immutable + large; the IPC copies them whole each call — cache ONE fetch).
let sharedWorkerFontsPromise: Promise<SharedWorkerFonts> | null = null

async function loadSharedWorkerFonts(): Promise<SharedWorkerFonts> {
  sharedWorkerFontsPromise ??= (async () => {
    // The primary (bundled JetBrains Mono) fetch must fail loudly — no face, no
    // engine. The OS fallback faces are best-effort (parity with the in-process path).
    const { fontBytes } = await loadAterm()
    try {
      const { cjk, emoji, symbol, chain } = await window.api.fonts.getTerminalFallbackFonts()
      const fallbacks: Uint8Array[] = []
      if (cjk) {
        fallbacks.push(new Uint8Array(cjk.bytes))
      }
      for (const face of chain ?? []) {
        fallbacks.push(new Uint8Array(face.bytes))
      }
      return {
        primary: fontBytes,
        fallbacks,
        emoji: emoji ? new Uint8Array(emoji) : null,
        symbol: symbol ? new Uint8Array(symbol) : null
      }
    } catch {
      return { primary: fontBytes, fallbacks: [], emoji: null, symbol: null }
    }
  })()
  return sharedWorkerFontsPromise
}

const productionHost = createAtermSharedWorkerHost({
  // Vite (renderer worker:{format:'es'}) bundles the worker from this URL.
  createWorker: () =>
    new Worker(new URL('./aterm-render-worker.ts', import.meta.url), { type: 'module' }),
  loadFonts: loadSharedWorkerFonts
})

/** Acquire a pane slot on THE shared render worker (created lazily, terminated when
 *  the last pane releases). Throws only on a font/asset failure — before any canvas
 *  transfer — so the caller can still fall back in-process. */
export function acquireAtermSharedWorkerPane(): Promise<AtermSharedWorkerPane> {
  return productionHost.acquirePane()
}
