import { atermSpillOverlay, type AtermSpillOverlay } from './aterm-spill-overlay'
import { spillGeometryEquals, type SpillPaneGeometry } from './aterm-spill-pane-scratch'
import type { AtermWorkerSpillCommand } from './aterm-worker-spill-protocol'

// MAIN-side seam of the worker spill compositor (stage 4): tracks which
// registered overlay panes are worker-backed, forwards their measured geometry
// (coalesced, change-only) and the overlay box across the worker seam, and
// owns the overlay-canvas transfer lifecycle. transferControlToOffscreen is
// irreversible per element AND per worker, so each canvas GENERATION is a
// fresh element + a monotone epoch: the worker drops messages addressed to a
// retired generation, and a worker respawn (the manager retires workers on
// crash / terminates on last release) re-initializes exactly like the pane
// canvas rebuild path — new element, higher epoch.

/** A pane-stamped post to the shared worker (set on the effects target by the
 *  worker loader). Spill state is worker-global, so ANY live binding's channel
 *  can carry the canvas-level messages. */
export type AtermSpillWorkerChannel = {
  post: (cmd: AtermWorkerSpillCommand, transfer?: Transferable[]) => void
}

type WorkerPaneBinding = {
  channel: AtermSpillWorkerChannel
  pendingGeometry: SpillPaneGeometry | null
  sentGeometry: SpillPaneGeometry | null
}

export type AtermSpillWorkerBridge = {
  /** Bind a registered overlay pane to the worker (idempotent; the overlay's
   *  unregister tears it down via the delegate). */
  bindPane: (paneKey: string, channel: AtermSpillWorkerChannel) => void
  /** React seam: transfer a freshly mounted overlay canvas for the CURRENT
   *  generation (same element twice = no-op; StrictMode-safe). */
  attachWorkerCanvas: (canvas: HTMLCanvasElement) => void
  /** The current canvas generation — the React layer keys the element by it,
   *  so a bump remounts a fresh (untransferred) element. */
  getCanvasGeneration: () => number
  hasWorkerPanes: () => boolean
  subscribe: (listener: () => void) => () => void
}

export function createAtermSpillWorkerBridge(
  overlay: AtermSpillOverlay = atermSpillOverlay
): AtermSpillWorkerBridge {
  const bindings = new Map<string, WorkerPaneBinding>()
  const listeners = new Set<() => void>()
  let generation = 0
  let attachedCanvas: HTMLCanvasElement | null = null
  let geometryFlushQueued = false

  const notify = (): void => {
    for (const listener of Array.from(listeners)) {
      listener()
    }
  }

  const anyChannel = (): AtermSpillWorkerChannel | null =>
    bindings.values().next().value?.channel ?? null

  // Microtask coalescing (the overlay's recomposite pattern — no rAF/timer
  // booking): a whole measure pass collapses to ONE message per changed pane.
  const flushGeometry = (): void => {
    geometryFlushQueued = false
    for (const [paneKey, binding] of bindings) {
      const geometry = binding.pendingGeometry
      binding.pendingGeometry = null
      if (!geometry || spillGeometryEquals(binding.sentGeometry, geometry)) {
        continue
      }
      binding.sentGeometry = geometry
      binding.channel.post({ type: 'spillPaneRects', paneKey, geometry })
    }
  }

  const queueGeometry = (paneKey: string, geometry: SpillPaneGeometry): void => {
    const binding = bindings.get(paneKey)
    if (!binding) {
      return
    }
    binding.pendingGeometry = geometry
    if (!geometryFlushQueued) {
      geometryFlushQueued = true
      queueMicrotask(() => {
        if (geometryFlushQueued) {
          flushGeometry()
        }
      })
    }
  }

  const unbindPane = (paneKey: string): void => {
    const binding = bindings.get(paneKey)
    if (!binding) {
      return
    }
    bindings.delete(paneKey)
    // Clear the pane's strips on the worker canvas. Posts to a dead worker
    // generation are dropped by the manager — safe on the crash path.
    binding.channel.post({ type: 'spillUnregister', paneKey })
    if (bindings.size === 0) {
      // Last worker pane: release the canvas worker-side and drop the element;
      // the NEXT bind starts a fresh generation (worker respawn ships a new
      // epoch by construction, so no stale-canvas frame can ever land).
      binding.channel.post({ type: 'spillRelease', epoch: generation })
      attachedCanvas = null
      notify()
    }
  }

  const bindPane = (paneKey: string, channel: AtermSpillWorkerChannel): void => {
    const existing = bindings.get(paneKey)
    if (existing) {
      // Same overlay identity on a new pane slot (rebuild): adopt the channel;
      // re-send geometry on the next measure (sentGeometry stays valid — the
      // worker keyed it by paneKey, which survived).
      existing.channel = channel
      return
    }
    // The registration seam registers BEFORE binding, so the pane must exist.
    if (overlay.getPaneChrome(paneKey) === null) {
      return
    }
    if (bindings.size === 0) {
      generation++
    }
    bindings.set(paneKey, { channel, pendingGeometry: null, sentGeometry: null })
    overlay.delegatePaneToWorker(paneKey, {
      pushGeometry: (geometry) => queueGeometry(paneKey, geometry),
      onUnregister: () => unbindPane(paneKey)
    })
    if (bindings.size === 1) {
      notify()
    }
  }

  const attachWorkerCanvas = (canvas: HTMLCanvasElement): void => {
    if (canvas === attachedCanvas || bindings.size === 0) {
      return
    }
    let offscreen: OffscreenCanvas
    try {
      offscreen = canvas.transferControlToOffscreen()
    } catch {
      // Already transferred (a stale element re-attached): never re-init on it.
      return
    }
    attachedCanvas = canvas
    anyChannel()?.post(
      {
        type: 'spillCanvasInit',
        epoch: generation,
        canvas: offscreen,
        box: overlay.getOverlayBox(),
        dpr: typeof window === 'undefined' ? 1 : window.devicePixelRatio || 1
      },
      [offscreen]
    )
    // e2e truth hook (the __atermFontClassDeliveries pattern): which canvas
    // generations actually crossed — the respawn spec asserts the epoch bump.
    if (typeof window !== 'undefined') {
      const w = window as unknown as { __atermSpillCanvasEpochs?: number[] }
      ;(w.__atermSpillCanvasEpochs ??= []).push(generation)
    }
  }

  // The worker canvas mirrors the overlay box; change-fed by the tracker's
  // measure pass, so steady frames post nothing across the seam.
  overlay.subscribeOverlayBox((box) => {
    if (bindings.size === 0 || !attachedCanvas) {
      return
    }
    anyChannel()?.post({ type: 'spillOverlayBox', epoch: generation, box })
  })

  return {
    bindPane,
    attachWorkerCanvas,
    getCanvasGeneration: () => generation,
    hasWorkerPanes: () => bindings.size > 0,
    subscribe: (listener) => {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    }
  }
}

/** The window-level bridge instance shared by the registration seam
 *  (aterm-effects-settings), the worker loader, and the React spill layer. */
export const atermSpillWorkerBridge: AtermSpillWorkerBridge = createAtermSpillWorkerBridge()
