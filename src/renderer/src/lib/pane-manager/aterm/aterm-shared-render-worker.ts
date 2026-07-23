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
import { e2eConfig } from '@/lib/e2e-config'
import type {
  AtermFontClass,
  AtermWorkerFederatedCommand,
  AtermWorkerFederatedEvent,
  AtermWorkerMessage,
  AtermWorkerPaneCommand,
  AtermWorkerPaneEvent
} from './aterm-render-worker-protocol'
import type { AtermWorkerSpillCommand } from './aterm-worker-spill-protocol'

/** The boot font payload for the worker engines. E1 LAZY FONTS: primary only
 *  (~264KB bundled JetBrains Mono) — the multi-hundred-MB OS fallback classes are
 *  fetched and delivered only when an engine reports a glyph miss for them, so an
 *  ASCII-only session never pays them in ANY process (the main-side read is
 *  class-scoped too). */
type SharedWorkerFonts = {
  primary: Uint8Array
}

/** Fetch one missed font CLASS from the main process ('text' = CJK-first chain +
 *  symbol, monochrome; 'emoji' = the colour face), shaped for the worker's
 *  'fontClass' message. Injected for tests. */
type LoadFontClass = (cls: AtermFontClass) => Promise<{
  fallbacks?: Uint8Array[]
  symbol?: Uint8Array
  emoji?: Uint8Array
}>

/** A view whose buffer can be transferred whole (postMessage transfer lists move
 *  ArrayBuffers, not views): IPC-delivered bytes normally own their buffer; a view
 *  into a larger one is copied out first so the transfer can't leak siblings. */
function transferableBytes(bytes: Uint8Array): Uint8Array {
  return bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength
    ? bytes
    : bytes.slice()
}

/** One pane's handle onto the shared worker. Every method is a safe no-op once the
 *  pane is released or the worker generation was retired (crash/boot-wedge). */
export type AtermSharedWorkerPane = {
  paneId: number
  /** Post a pane command (or a pane-stamped spill-compositor command); the
   *  manager stamps this pane's paneId on the wire. */
  post: (cmd: AtermWorkerPaneCommand | AtermWorkerSpillCommand, transfer?: Transferable[]) => void
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
  loadFontClass: LoadFontClass
}

export type AtermSharedWorkerHost = {
  acquirePane: () => Promise<AtermSharedWorkerPane>
  /** Post a worker-scoped federated find/cancel to the LIVE worker generation.
   *  Returns false when no worker is alive (nothing to search — the caller's
   *  worker-path panes cannot exist without one); never spawns a worker. */
  postFederated: (cmd: AtermWorkerFederatedCommand) => boolean
  /** Subscribe to worker-scoped federated batch/done events; returns a disposer.
   *  Host-scoped (survives worker generations): stale-gen events are the
   *  subscriber's to drop — every event carries its gen. */
  onFederatedEvent: (handler: (event: AtermWorkerFederatedEvent) => void) => () => void
}

/** Factory (deps injected) so unit tests can drive the lifecycle with a fake Worker;
 *  production uses the module-level singleton below. */
export function createAtermSharedWorkerHost(deps: SharedWorkerHostDeps): AtermSharedWorkerHost {
  type Generation = {
    worker: WorkerLike
    booted: boolean
    retired: boolean
    panes: Map<number, PaneClient>
    /** Font classes already requested for THIS generation (in flight or delivered).
     *  Generation-scoped on purpose: resident faces die with the worker, and a
     *  rebuilt generation self-heals by re-firing the miss signal — the main
     *  process caches the class read, so re-delivery is one IPC copy. */
    requestedFontClasses: Set<AtermFontClass>
  }
  let current: Generation | null = null
  let nextPaneId = 1
  // Host-scoped federated subscribers: a worker retire mid-run simply stops the
  // event stream (subscribers own their timeouts); a fresh generation reuses them.
  const federatedListeners = new Set<(event: AtermWorkerFederatedEvent) => void>()

  // e2e-only worker-restart lever: drives the REAL retire path (same as a wasm
  // crash), so the spill respawn spec can prove every pane rebuilds and the
  // overlay re-establishes on a fresh worker generation + canvas epoch.
  if (e2eConfig.exposeStore && typeof window !== 'undefined') {
    const w = window as unknown as { __atermRetireSharedRenderWorker?: () => void }
    w.__atermRetireSharedRenderWorker = () => {
      if (current) {
        retire(current, 'e2e forced worker retire')
      }
    }
  }

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

  // A missed font CLASS: fetch it (main caches per class) and deliver it to the
  // generation's worker, transferring the buffers — the renderer keeps NO byte
  // cache (pre-E1 it retained the whole ~229MB payload for post-crash re-sends;
  // now a rebuilt generation just re-fires the miss signal).
  const deliverFontClass = async (gen: Generation, cls: AtermFontClass): Promise<void> => {
    if (gen.retired || gen.requestedFontClasses.has(cls)) {
      return
    }
    gen.requestedFontClasses.add(cls)
    let faces: Awaited<ReturnType<LoadFontClass>>
    try {
      faces = await deps.loadFontClass(cls)
    } catch (err) {
      // Permanent for this generation (latched): rendering stays Latin-correct
      // with `.notdef` for the missed scripts, same as a host with no such font.
      console.warn(`[aterm] lazy fallback-font fetch failed (class=${cls})`, err)
      return
    }
    if (gen.retired) {
      return
    }
    const fallbacks = (faces.fallbacks ?? []).map(transferableBytes)
    const symbol = faces.symbol ? transferableBytes(faces.symbol) : undefined
    const emoji = faces.emoji ? transferableBytes(faces.emoji) : undefined
    // Byte count BEFORE the transfer detaches the buffers (the e2e hook below).
    const bytes =
      fallbacks.reduce((sum, f) => sum + f.byteLength, 0) +
      (symbol?.byteLength ?? 0) +
      (emoji?.byteLength ?? 0)
    gen.worker.postMessage({ type: 'fontClass', class: cls, fallbacks, symbol, emoji }, [
      ...fallbacks.map((f) => f.buffer),
      ...(symbol ? [symbol.buffer] : []),
      ...(emoji ? [emoji.buffer] : [])
    ])
    // e2e truth hook: which classes actually crossed, when — the lazy-font gate
    // asserts the emoji face does NOT ship before an emoji renders.
    if (typeof window !== 'undefined') {
      const w = window as unknown as {
        __atermFontClassDeliveries?: { class: AtermFontClass; bytes: number }[]
      }
      ;(w.__atermFontClassDeliveries ??= []).push({ class: cls, bytes })
    }
  }

  const ensureWorker = (fonts: SharedWorkerFonts): Generation => {
    if (current) {
      return current
    }
    const worker = deps.createWorker()
    const gen: Generation = {
      worker,
      booted: false,
      retired: false,
      panes: new Map(),
      requestedFontClasses: new Set()
    }
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
      if (data.type === 'missingFontClasses') {
        for (const cls of data.classes) {
          void deliverFontClass(gen, cls)
        }
        return
      }
      if (data.type === 'federatedBatch' || data.type === 'federatedDone') {
        for (const listener of federatedListeners) {
          listener(data)
        }
        return
      }
      gen.panes.get(data.paneId)?.onEvent?.(data)
    })
    worker.addEventListener('error', (event) => {
      // Nobody else listens for escaped worker exceptions — without this every pane
      // silently freezes at its last frame while keystrokes keep flowing.
      retire(gen, event.message || 'uncaught worker error')
    })
    // The boot 'fonts' message, before any pane init — E1: the ~264KB primary only.
    // Slice the cached asset bytes so the transfer doesn't detach the cache.
    const primary = fonts.primary.slice()
    worker.postMessage({ type: 'fonts', primary, fallbacks: [] }, [primary.buffer])
    current = gen
    return gen
  }

  return {
    postFederated: (cmd) => {
      if (!current || current.retired) {
        return false
      }
      current.worker.postMessage(cmd, [])
      return true
    },
    onFederatedEvent: (handler) => {
      federatedListeners.add(handler)
      return () => federatedListeners.delete(handler)
    },
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

// The boot payload is just the bundled primary (loadAterm caches the asset fetch);
// its failure must be loud — no face, no engine.
async function loadSharedWorkerFonts(): Promise<SharedWorkerFonts> {
  const { fontBytes } = await loadAterm()
  return { primary: fontBytes }
}

// One missed class from the main process (per-class cached there); the bytes are
// transferred straight into the worker — the renderer keeps no copy.
// Exported for tests: the text-class ordering (user stack before CJK) is the
// contract the worker registry applies in array order.
export async function loadSharedWorkerFontClass(cls: AtermFontClass): ReturnType<LoadFontClass> {
  const { user, cjk, emoji, symbol, chain } = await window.api.fonts.getTerminalFallbackFonts([cls])
  if (cls === 'emoji') {
    return { emoji: emoji ? new Uint8Array(emoji) : undefined }
  }
  const fallbacks: Uint8Array[] = []
  // User-configured stack precedes the CJK face. Ordering hazard: the worker
  // registry (aterm-worker-font-registry) appends incrementally by index, so a
  // CHANGED stack cannot reorder an already-delivered class within a live worker
  // generation — a new stack takes effect from the next generation.
  for (const face of user ?? []) {
    fallbacks.push(new Uint8Array(face.bytes))
  }
  if (cjk) {
    fallbacks.push(new Uint8Array(cjk.bytes))
  }
  for (const face of chain ?? []) {
    fallbacks.push(new Uint8Array(face.bytes))
  }
  return { fallbacks, symbol: symbol ? new Uint8Array(symbol) : undefined }
}

const productionHost = createAtermSharedWorkerHost({
  // Vite (renderer worker:{format:'es'}) bundles the worker from this URL.
  createWorker: () =>
    new Worker(new URL('./aterm-render-worker.ts', import.meta.url), { type: 'module' }),
  loadFonts: loadSharedWorkerFonts,
  loadFontClass: loadSharedWorkerFontClass
})

/** Acquire a pane slot on THE shared render worker (created lazily, terminated when
 *  the last pane releases). Throws only on a font/asset failure — before any canvas
 *  transfer — so the caller can still fall back in-process. */
export function acquireAtermSharedWorkerPane(): Promise<AtermSharedWorkerPane> {
  return productionHost.acquirePane()
}

/** Post a federated find/cancel to THE shared render worker (false = no live
 *  worker, so no worker-path panes exist to search). Never spawns a worker. */
export function postAtermFederatedToSharedWorker(cmd: AtermWorkerFederatedCommand): boolean {
  return productionHost.postFederated(cmd)
}

/** Subscribe to the shared worker's federated batch/done events (disposer back). */
export function subscribeAtermSharedWorkerFederatedEvents(
  handler: (event: AtermWorkerFederatedEvent) => void
): () => void {
  return productionHost.onFederatedEvent(handler)
}
