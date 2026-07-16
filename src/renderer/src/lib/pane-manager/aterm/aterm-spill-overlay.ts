import {
  adoptSpillPaneGeometry,
  createSpillPaneState,
  ensureSpillScratch,
  reblitAllSpillPanes,
  runSpillClearUnionPass,
  spillGeometryEquals,
  type SpillPaneGeometry,
  type SpillPaneRecord,
  type SpillPaneState,
  type SpillScratchReader
} from './aterm-spill-pane-scratch'

// The window-space cross-pane effects compositor (spill stage 2). One overlay
// canvas spans the terminal-surfaces container; each registered pane may paint
// ONLY its chrome band OUTSIDE its own clip box (the outsideRects decomposition
// from aterm-chrome-box), so in-pane pixels stay single-sourced from the pane
// canvas and brightness is exact everywhere. This module composites the
// IN-PROCESS panes and holds the shared registry; worker-backed panes register
// here too (geometry measurement + pane counting) but are delegated to the
// render-worker compositor, keeping this canvas dormant unless an in-process
// pane paints. Zero registrations = no canvas, zero idle cost.

export type {
  SpillPaneGeometry,
  SpillPaneRecord,
  SpillScratchReader,
  SpillStripSlot
} from './aterm-spill-pane-scratch'

/** Device-px backing size of the overlay canvas (the container box × dpr). */
export type SpillOverlayBox = { widthPx: number; heightPx: number }

/** A worker-path pane's geometry sink (stage 4): the geometry tracker keeps
 *  measuring the pane through this registry, but its pixels composite in the
 *  SHARED render worker, so geometry is forwarded instead of adopted here. */
export type SpillWorkerPaneDelegate = {
  pushGeometry: (geometry: SpillPaneGeometry) => void
  /** Fired when the pane leaves the registry (chrome 0 / pane close), so the
   *  worker bridge can clear the pane's strips on ITS canvas. */
  onUnregister: () => void
}

export type AtermSpillOverlay = {
  register: (paneKey: string, record: SpillPaneRecord) => void
  unregister: (paneKey: string) => void
  updateGeometry: (paneKey: string, geometry: SpillPaneGeometry) => void
  /** The in-process (stage 3) per-paint pass: refresh the pane's scratch, then
   *  run the clear-union + intersect-expansion re-blit over the overlay. */
  runSpillPassInProcess: (paneKey: string, readSpill: SpillScratchReader) => void
  /** Route a registered pane's geometry to the worker bridge (stage 4). The
   *  delegation dies with the registration, so a crash-rebuilt in-process pane
   *  re-registering under the same key composites main-side again. */
  delegatePaneToWorker: (paneKey: string, delegate: SpillWorkerPaneDelegate) => void
  /** React layer seams: the canvas mounts only while panes are registered. */
  attachCanvas: (canvas: HTMLCanvasElement) => () => void
  setOverlayBox: (box: SpillOverlayBox) => void
  getOverlayBox: () => SpillOverlayBox
  /** Box-change feed for the worker bridge (its canvas mirrors this box). */
  subscribeOverlayBox: (listener: (box: SpillOverlayBox) => void) => () => void
  /** Fires on register/unregister/chrome-change (React re-render + re-measure). */
  subscribe: (listener: () => void) => () => void
  getPaneCount: () => number
  getPaneKeys: () => readonly string[]
  getPaneChrome: (paneKey: string) => SpillPaneRecord | null
}

export function createAtermSpillOverlay(): AtermSpillOverlay {
  // The per-pane records are RETAINED across geometry passes (fields updated in
  // place); only changed rect lists are adopted, so steady-state passes allocate
  // nothing here.
  const panes = new Map<string, SpillPaneState>()
  // Worker-delegated paneKeys: measured here, composited in the render worker.
  const workerDelegates = new Map<string, SpillWorkerPaneDelegate>()
  const listeners = new Set<() => void>()
  const boxListeners = new Set<(box: SpillOverlayBox) => void>()
  let canvas: HTMLCanvasElement | null = null
  let ctx: CanvasRenderingContext2D | null = null
  let box: SpillOverlayBox = { widthPx: 0, heightPx: 0 }
  let appliedBox: SpillOverlayBox = { widthPx: -1, heightPx: -1 }
  let recompositeQueued = false

  // Only NON-delegated panes draw on THIS canvas; a worker-only population must
  // keep the main canvas dormant (0×0, display:none) — the backing-store memory
  // rule is load-bearing at 4K (risk 13), not polish.
  const inProcessPaneCount = (): number => panes.size - workerDelegates.size

  const notify = (): void => {
    // Snapshot: a listener may (un)subscribe re-entrantly (React store swaps).
    for (const listener of Array.from(listeners)) {
      listener()
    }
  }

  // Zero in-process registrations: one clear (the 0×0 resize) then a dormant
  // element. Belt-and-braces with the React layer unmounting it — the module
  // contract must hold even before the unmount flushes.
  const applyIdleState = (): void => {
    if (!canvas) {
      return
    }
    canvas.width = 0
    canvas.height = 0
    canvas.style.display = 'none'
    appliedBox = { widthPx: 0, heightPx: 0 }
  }

  const applyLiveBox = (): void => {
    if (!canvas || inProcessPaneCount() === 0) {
      return
    }
    canvas.style.display = ''
    if (appliedBox.widthPx !== box.widthPx || appliedBox.heightPx !== box.heightPx) {
      // Resizing the backing store implicitly clears it; callers follow with a
      // full recomposite from the retained scratches.
      canvas.width = box.widthPx
      canvas.height = box.heightPx
      appliedBox = { widthPx: box.widthPx, heightPx: box.heightPx }
    }
  }

  // Any geometry/box push invalidates layered state wholesale: full clear, then
  // re-blit every visible pane from its retained scratch (architecture graft #1's
  // recovery rule — pure drawImage, no engine re-render).
  const recompositeNow = (): void => {
    recompositeQueued = false
    if (!canvas || !ctx) {
      return
    }
    if (inProcessPaneCount() === 0) {
      applyIdleState()
      return
    }
    applyLiveBox()
    ctx.clearRect(0, 0, canvas.width, canvas.height)
    reblitAllSpillPanes(ctx, panes.values())
  }

  // Microtask coalescing (NOT a rAF/timer booking): one recomposite per measure
  // batch no matter how many panes moved; spill passes flush it first for
  // deterministic ordering.
  const scheduleRecomposite = (): void => {
    if (recompositeQueued) {
      return
    }
    recompositeQueued = true
    queueMicrotask(() => {
      if (recompositeQueued) {
        recompositeNow()
      }
    })
  }

  const register = (paneKey: string, record: SpillPaneRecord): void => {
    const existing = panes.get(paneKey)
    if (existing) {
      if (
        existing.record.chromePadPx === record.chromePadPx &&
        existing.record.chromeHeadPx === record.chromeHeadPx
      ) {
        return
      }
      // Chrome extents shape the strip decomposition, so stale geometry would
      // blit mis-sized strips: drop it and let the tracker re-measure.
      existing.record = { ...record }
      existing.geometry = null
      existing.stripSlots = []
      existing.outsideStripIndex = []
      scheduleRecomposite()
      notify()
      return
    }
    panes.set(paneKey, createSpillPaneState(record))
    if (inProcessPaneCount() === 1) {
      applyLiveBox()
    }
    notify()
  }

  const unregister = (paneKey: string): void => {
    if (!panes.delete(paneKey)) {
      return
    }
    // A worker-delegated pane's strips live on the WORKER canvas: hand the
    // clear to the bridge with the registration.
    const delegate = workerDelegates.get(paneKey)
    if (delegate) {
      workerDelegates.delete(paneKey)
      delegate.onUnregister()
    }
    if (inProcessPaneCount() === 0) {
      // The departing pane's strips die with the whole surface: clear once now
      // (idle supersedes any queued recomposite) and go dormant.
      recompositeQueued = false
      applyIdleState()
    } else {
      scheduleRecomposite()
    }
    notify()
  }

  const updateGeometry = (paneKey: string, geometry: SpillPaneGeometry): void => {
    const pane = panes.get(paneKey)
    if (!pane || spillGeometryEquals(pane.geometry, geometry)) {
      return
    }
    const delegate = workerDelegates.get(paneKey)
    if (delegate) {
      // Worker pane: keep the geometry for the equality dedup above but never
      // adopt scratch slots here — the worker compositor owns its pixels.
      pane.geometry = geometry
      delegate.pushGeometry(geometry)
      return
    }
    adoptSpillPaneGeometry(pane, geometry)
    scheduleRecomposite()
  }

  const delegatePaneToWorker = (paneKey: string, delegate: SpillWorkerPaneDelegate): void => {
    if (!panes.has(paneKey) || workerDelegates.has(paneKey)) {
      return
    }
    workerDelegates.set(paneKey, delegate)
    // The register that preceded this may have gone live on the main canvas;
    // re-settle so a worker-only population returns it to the dormant state.
    scheduleRecomposite()
  }

  const runSpillPassInProcess = (paneKey: string, readSpill: SpillScratchReader): void => {
    // Pending geometry lands before the pass so the blit targets fresh rects.
    if (recompositeQueued) {
      recompositeNow()
    }
    const pane = panes.get(paneKey)
    const geometry = pane?.geometry
    if (!pane || !geometry?.visible || geometry.outsideRects.length === 0) {
      return
    }
    if (!ensureSpillScratch(pane) || !pane.scratchCtx) {
      return
    }
    const dirty = readSpill({ ctx: pane.scratchCtx, strips: pane.stripSlots })
    if (!dirty || dirty.length === 0 || !ctx) {
      // No overlay canvas yet: the scratch stays refreshed and the attach-time
      // recomposite blits the latest state.
      return
    }
    // Clear-union + intersect-expansion (architecture graft #1; shared with the
    // worker compositor): a neighbor's settled ring is restored from its
    // retained scratch, never erased by this pane's clear.
    runSpillClearUnionPass(ctx, pane, geometry, dirty, panes.values())
  }

  const attachCanvas = (next: HTMLCanvasElement): (() => void) => {
    canvas = next
    ctx = next.getContext('2d')
    appliedBox = { widthPx: -1, heightPx: -1 }
    if (panes.size === 0) {
      applyIdleState()
    } else {
      applyLiveBox()
      scheduleRecomposite()
    }
    return () => {
      if (canvas === next) {
        canvas = null
        ctx = null
      }
    }
  }

  const setOverlayBox = (next: SpillOverlayBox): void => {
    if (box.widthPx === next.widthPx && box.heightPx === next.heightPx) {
      return
    }
    box = { widthPx: next.widthPx, heightPx: next.heightPx }
    // The worker bridge mirrors this box onto ITS canvas (change-fed, so a
    // steady measure pass posts nothing across the worker seam).
    for (const listener of Array.from(boxListeners)) {
      listener(box)
    }
    if (inProcessPaneCount() === 0 || !canvas) {
      return
    }
    applyLiveBox()
    scheduleRecomposite()
  }

  const subscribe = (listener: () => void): (() => void) => {
    listeners.add(listener)
    return () => {
      listeners.delete(listener)
    }
  }

  return {
    register,
    unregister,
    updateGeometry,
    runSpillPassInProcess,
    delegatePaneToWorker,
    attachCanvas,
    setOverlayBox,
    getOverlayBox: () => ({ ...box }),
    subscribeOverlayBox: (listener) => {
      boxListeners.add(listener)
      return () => {
        boxListeners.delete(listener)
      }
    },
    subscribe,
    getPaneCount: () => panes.size,
    getPaneKeys: () => [...panes.keys()],
    getPaneChrome: (paneKey) => {
      const pane = panes.get(paneKey)
      return pane ? { ...pane.record } : null
    }
  }
}

/** The window-level compositor instance every seam shares (registration in
 *  aterm-effects-settings, the geometry tracker, the React overlay layer). */
export const atermSpillOverlay: AtermSpillOverlay = createAtermSpillOverlay()
