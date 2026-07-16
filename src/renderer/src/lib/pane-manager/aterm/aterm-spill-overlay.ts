import type { AtermDeviceRect } from './aterm-chrome-box'
import {
  adoptSpillPaneGeometry,
  blitSpillOutsideRects,
  createSpillPaneState,
  EMPTY_SPILL_RECTS,
  ensureSpillScratch,
  pushSpillRectIntersection,
  spillGeometryEquals,
  spillRectsOverlap,
  type SpillPaneGeometry,
  type SpillPaneRecord,
  type SpillPaneState,
  type SpillScratchReader
} from './aterm-spill-pane-scratch'

// The window-space cross-pane effects compositor (spill stage 2). One overlay
// canvas spans the terminal-surfaces container; each registered pane may paint
// ONLY its chrome band OUTSIDE its own clip box (the outsideRects decomposition
// from aterm-chrome-box), so in-pane pixels stay single-sourced from the pane
// canvas and brightness is exact everywhere. FEATURE-DARK today: registration is
// gated on an engine spill-export capability no pinned artifact sets yet, so
// nothing registers, no canvas exists, and idle cost is zero.

export type {
  SpillPaneGeometry,
  SpillPaneRecord,
  SpillScratchReader,
  SpillStripSlot
} from './aterm-spill-pane-scratch'

/** Device-px backing size of the overlay canvas (the container box × dpr). */
export type SpillOverlayBox = { widthPx: number; heightPx: number }

/** Reserved for the stage-4 worker-transfer variant: each transfer of a fresh
 *  overlay OffscreenCanvas carries a monotone epoch so the worker drops frames
 *  addressed to a canvas that was since re-transferred (worker respawn). The
 *  main-thread-2d path implemented here is fixed at epoch 0. */
export type SpillOverlayEpoch = number
export type SpillOverlayWorkerTransfer = {
  epoch: SpillOverlayEpoch
  canvas: OffscreenCanvas
  box: SpillOverlayBox
}

export type AtermSpillOverlay = {
  register: (paneKey: string, record: SpillPaneRecord) => void
  unregister: (paneKey: string) => void
  updateGeometry: (paneKey: string, geometry: SpillPaneGeometry) => void
  /** The in-process (stage 3) per-paint pass: refresh the pane's scratch, then
   *  run the clear-union + intersect-expansion re-blit over the overlay. */
  runSpillPassInProcess: (paneKey: string, readSpill: SpillScratchReader) => void
  /** React layer seams: the canvas mounts only while panes are registered. */
  attachCanvas: (canvas: HTMLCanvasElement) => () => void
  setOverlayBox: (box: SpillOverlayBox) => void
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
  const listeners = new Set<() => void>()
  let canvas: HTMLCanvasElement | null = null
  let ctx: CanvasRenderingContext2D | null = null
  let box: SpillOverlayBox = { widthPx: 0, heightPx: 0 }
  let appliedBox: SpillOverlayBox = { widthPx: -1, heightPx: -1 }
  let recompositeQueued = false

  const notify = (): void => {
    // Snapshot: a listener may (un)subscribe re-entrantly (React store swaps).
    for (const listener of Array.from(listeners)) {
      listener()
    }
  }

  // Zero registrations: one clear (the 0×0 resize) then a dormant element.
  // Belt-and-braces with the React layer unmounting it — the module contract
  // must hold even before the unmount flushes.
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
    if (!canvas || panes.size === 0) {
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
    if (panes.size === 0) {
      applyIdleState()
      return
    }
    applyLiveBox()
    ctx.clearRect(0, 0, canvas.width, canvas.height)
    for (const pane of panes.values()) {
      const geometry = pane.geometry
      if (!geometry?.visible || !pane.scratch || geometry.outsideRects.length === 0) {
        pane.prevDrawnRects = EMPTY_SPILL_RECTS
        continue
      }
      blitSpillOutsideRects(ctx, pane, geometry)
      pane.prevDrawnRects = geometry.outsideRects
    }
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
    if (panes.size === 1) {
      applyLiveBox()
    }
    notify()
  }

  const unregister = (paneKey: string): void => {
    if (!panes.delete(paneKey)) {
      return
    }
    if (panes.size === 0) {
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
    adoptSpillPaneGeometry(pane, geometry)
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
    const dirtyOutside: AtermDeviceRect[] = []
    for (const d of dirty) {
      for (const outside of geometry.outsideRects) {
        pushSpillRectIntersection(d, outside, dirtyOutside)
      }
    }
    const clearUnion =
      pane.prevDrawnRects.length > 0 ? [...pane.prevDrawnRects, ...dirtyOutside] : dirtyOutside
    if (clearUnion.length === 0) {
      return
    }
    // Clear-union + intersect-expansion (architecture graft #1): clear every
    // union rect (overlapping clears are idempotent), then re-blit EVERY
    // intersecting pane clipped to the union — a neighbor's settled ring is
    // restored from its retained scratch, never erased by this pane's clear.
    for (const u of clearUnion) {
      ctx.clearRect(u.x, u.y, u.width, u.height)
    }
    ctx.save()
    ctx.beginPath()
    for (const u of clearUnion) {
      ctx.rect(u.x, u.y, u.width, u.height)
    }
    ctx.clip()
    for (const other of panes.values()) {
      const otherGeometry = other.geometry
      if (!otherGeometry?.visible || !other.scratch || otherGeometry.outsideRects.length === 0) {
        continue
      }
      if (
        !otherGeometry.outsideRects.some((rect) =>
          clearUnion.some((u) => spillRectsOverlap(rect, u))
        )
      ) {
        continue
      }
      blitSpillOutsideRects(ctx, other, otherGeometry)
      other.prevDrawnRects = otherGeometry.outsideRects
    }
    ctx.restore()
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
    if (panes.size === 0 || !canvas) {
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
    attachCanvas,
    setOverlayBox,
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
