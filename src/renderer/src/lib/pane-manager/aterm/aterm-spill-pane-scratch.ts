import type { AtermDeviceRect } from './aterm-chrome-box'

// Per-pane retained spill state for the window-space compositor: the packed
// scratch canvas holding a pane's last-exported chrome-band pixels, plus the
// geometry adoption and blit math over it. Retention is load-bearing — neighbor
// recovery after another pane's clear is a pure drawImage from here, never a
// forced engine re-render. Packing stores only band pixels (~0.28MP/pane worst
// case), never a frame-sized buffer. Canvas types are unions so the SAME math
// serves the main-thread compositor (stage 3) and the worker one (stage 4).

/** Registration input: the pane's window-chrome extents in device px. */
export type SpillPaneRecord = {
  chromePadPx: number
  chromeHeadPx: number
}

/** A retained scratch surface: DOM canvas in-process, OffscreenCanvas worker-side. */
export type SpillScratchCanvas = HTMLCanvasElement | OffscreenCanvas
/** Either surface's 2d context (identical drawing surface for this module's ops). */
export type SpillCanvas2D = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D
export type CreateSpillScratchCanvas = (width: number, height: number) => SpillScratchCanvas

/** Both canvas kinds expose getContext('2d'); the union confuses TS's overload
 *  resolution, so route through one narrowed signature. */
export function getSpill2dContext(canvas: SpillScratchCanvas): SpillCanvas2D | null {
  return (canvas.getContext as (contextId: '2d') => SpillCanvas2D | null)('2d')
}

/** Overlay-space integer-device-px geometry pushed by the geometry tracker. */
export type SpillPaneGeometry = {
  /** Origin of the chrome-padded frame (engine spill rects are frame-absolute). */
  frameOrigin: { x: number; y: number }
  /** The pane's own visible box; in-clip chrome stays pane-sourced. */
  clipRect: AtermDeviceRect
  /** The full chrome band as disjoint strips (what the scratch retains). */
  stripRects: readonly AtermDeviceRect[]
  /** Strips minus clip: the only rects this pane may paint on the overlay. */
  outsideRects: readonly AtermDeviceRect[]
  /** Projected isVisible/isWorktreeActive flags; hidden panes paint nothing. */
  visible: boolean
}

/** One chrome strip's slot in the pane's packed scratch canvas. */
export type SpillStripSlot = {
  overlayRect: AtermDeviceRect
  scratchOrigin: { x: number; y: number }
}

/** Refreshes a pane's retained scratch from the engine's spill export and
 *  returns overlay-space dirty rects (null/[] = band unchanged, skip the blit).
 *  Built by createAtermSpillScratchReader; both compositors share the shape. */
export type SpillScratchReader = (target: {
  ctx: SpillCanvas2D
  strips: readonly SpillStripSlot[]
}) => readonly AtermDeviceRect[] | null

export type SpillPaneState = {
  record: SpillPaneRecord
  geometry: SpillPaneGeometry | null
  scratch: SpillScratchCanvas | null
  scratchCtx: SpillCanvas2D | null
  stripSlots: SpillStripSlot[]
  /** Owning strip slot per outsideRect (each lies wholly inside one strip). */
  outsideStripIndex: number[]
  /** Overlay rects this pane last painted — the clear-union input. */
  prevDrawnRects: readonly AtermDeviceRect[]
}

export const EMPTY_SPILL_RECTS: readonly AtermDeviceRect[] = []

export function createSpillPaneState(record: SpillPaneRecord): SpillPaneState {
  return {
    record: { ...record },
    geometry: null,
    scratch: null,
    scratchCtx: null,
    stripSlots: [],
    outsideStripIndex: [],
    prevDrawnRects: EMPTY_SPILL_RECTS
  }
}

function rectEquals(a: AtermDeviceRect, b: AtermDeviceRect | undefined): boolean {
  return (
    b !== undefined && a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height
  )
}

function rectListEquals(a: readonly AtermDeviceRect[], b: readonly AtermDeviceRect[]): boolean {
  return a.length === b.length && a.every((r, i) => rectEquals(r, b[i]))
}

export function spillGeometryEquals(a: SpillPaneGeometry | null, b: SpillPaneGeometry): boolean {
  return (
    a !== null &&
    a.visible === b.visible &&
    a.frameOrigin.x === b.frameOrigin.x &&
    a.frameOrigin.y === b.frameOrigin.y &&
    rectEquals(a.clipRect, b.clipRect) &&
    rectListEquals(a.stripRects, b.stripRects) &&
    rectListEquals(a.outsideRects, b.outsideRects)
  )
}

function rectContains(outer: AtermDeviceRect, inner: AtermDeviceRect): boolean {
  return (
    inner.x >= outer.x &&
    inner.y >= outer.y &&
    inner.x + inner.width <= outer.x + outer.width &&
    inner.y + inner.height <= outer.y + outer.height
  )
}

export function spillRectsOverlap(a: AtermDeviceRect, b: AtermDeviceRect): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

export function pushSpillRectIntersection(
  a: AtermDeviceRect,
  b: AtermDeviceRect,
  out: AtermDeviceRect[]
): void {
  const x = Math.max(a.x, b.x)
  const y = Math.max(a.y, b.y)
  const right = Math.min(a.x + a.width, b.x + b.width)
  const bottom = Math.min(a.y + a.height, b.y + b.height)
  if (right > x && bottom > y) {
    out.push({ x, y, width: right - x, height: bottom - y })
  }
}

function packedScratchSize(pane: SpillPaneState): { width: number; height: number } {
  let width = 0
  let height = 0
  for (const slot of pane.stripSlots) {
    width = Math.max(width, slot.overlayRect.width)
    height += slot.overlayRect.height
  }
  return { width, height }
}

/** Lazily (re)builds the packed scratch to the current strip layout. Returns
 *  false when there is nothing to draw into (no strips or no 2d context).
 *  `createCanvas` defaults to a DOM canvas; the worker passes OffscreenCanvas. */
export function ensureSpillScratch(
  pane: SpillPaneState,
  createCanvas?: CreateSpillScratchCanvas
): boolean {
  const size = packedScratchSize(pane)
  if (size.width <= 0 || size.height <= 0) {
    return false
  }
  if (!pane.scratch) {
    pane.scratch = createCanvas
      ? createCanvas(size.width, size.height)
      : document.createElement('canvas')
    pane.scratch.width = size.width
    pane.scratch.height = size.height
    pane.scratchCtx = getSpill2dContext(pane.scratch)
  } else if (pane.scratch.width !== size.width || pane.scratch.height !== size.height) {
    pane.scratch.width = size.width
    pane.scratch.height = size.height
  }
  return pane.scratchCtx !== null
}

/** Adopts freshly-measured geometry: repacks strip slots and re-indexes the
 *  outsideRects. Equal strip sizes keep the scratch pixels (pure-move drags stay
 *  drawImage-only); a size change reallocates and the engine re-exports. */
export function adoptSpillPaneGeometry(pane: SpillPaneState, geometry: SpillPaneGeometry): void {
  const prevSlots = pane.stripSlots
  const slots: SpillStripSlot[] = []
  let packedY = 0
  for (const strip of geometry.stripRects) {
    slots.push({ overlayRect: strip, scratchOrigin: { x: 0, y: packedY } })
    packedY += strip.height
  }
  pane.stripSlots = slots
  pane.outsideStripIndex = geometry.outsideRects.map((rect) =>
    slots.findIndex((slot) => rectContains(slot.overlayRect, rect))
  )
  pane.geometry = geometry
  if (pane.scratch) {
    const sizesChanged =
      slots.length !== prevSlots.length ||
      slots.some((slot, i) => {
        const prev = prevSlots[i]
        return (
          !prev ||
          slot.overlayRect.width !== prev.overlayRect.width ||
          slot.overlayRect.height !== prev.overlayRect.height
        )
      })
    if (sizesChanged) {
      ensureSpillScratch(pane)
    }
  }
}

/** Blits the pane's outsideRects from its packed scratch onto the overlay.
 *  Each outsideRect maps into its owning strip's slot; anything the caller
 *  wants excluded must already be clipped on `target`. */
export function blitSpillOutsideRects(
  target: SpillCanvas2D,
  pane: SpillPaneState,
  geometry: SpillPaneGeometry
): void {
  if (!pane.scratch) {
    return
  }
  for (let i = 0; i < geometry.outsideRects.length; i++) {
    const rect = geometry.outsideRects[i]
    const slot = pane.stripSlots[pane.outsideStripIndex[i] ?? -1]
    if (!rect || !slot) {
      continue
    }
    target.drawImage(
      pane.scratch,
      slot.scratchOrigin.x + (rect.x - slot.overlayRect.x),
      slot.scratchOrigin.y + (rect.y - slot.overlayRect.y),
      rect.width,
      rect.height,
      rect.x,
      rect.y,
      rect.width,
      rect.height
    )
  }
}

/** Clear `rects`, then re-blit every pane whose outsideRects intersect them,
 *  clipped to the cleared region (architecture graft #1: overlapping clears are
 *  idempotent, and a neighbor's settled ring is restored from its retained
 *  scratch — never erased by another pane's clear). */
export function clearSpillRectsAndReblit(
  ctx: SpillCanvas2D,
  rects: readonly AtermDeviceRect[],
  panes: Iterable<SpillPaneState>
): void {
  if (rects.length === 0) {
    return
  }
  for (const u of rects) {
    ctx.clearRect(u.x, u.y, u.width, u.height)
  }
  ctx.save()
  ctx.beginPath()
  for (const u of rects) {
    ctx.rect(u.x, u.y, u.width, u.height)
  }
  ctx.clip()
  for (const other of panes) {
    const otherGeometry = other.geometry
    if (!otherGeometry?.visible || !other.scratch || otherGeometry.outsideRects.length === 0) {
      continue
    }
    if (!otherGeometry.outsideRects.some((rect) => rects.some((u) => spillRectsOverlap(rect, u)))) {
      continue
    }
    blitSpillOutsideRects(ctx, other, otherGeometry)
    other.prevDrawnRects = otherGeometry.outsideRects
  }
  ctx.restore()
}

/** One pane's post-scratch-refresh pass: intersect its dirty rects with its
 *  outsideRects, union with its previous paint, clear + intersect-expansion
 *  re-blit. Shared by the in-process and worker compositors. */
export function runSpillClearUnionPass(
  ctx: SpillCanvas2D,
  pane: SpillPaneState,
  geometry: SpillPaneGeometry,
  dirty: readonly AtermDeviceRect[],
  panes: Iterable<SpillPaneState>
): void {
  const dirtyOutside: AtermDeviceRect[] = []
  for (const d of dirty) {
    for (const outside of geometry.outsideRects) {
      pushSpillRectIntersection(d, outside, dirtyOutside)
    }
  }
  const clearUnion =
    pane.prevDrawnRects.length > 0 ? [...pane.prevDrawnRects, ...dirtyOutside] : dirtyOutside
  clearSpillRectsAndReblit(ctx, clearUnion, panes)
}

/** Full recomposite (canvas adopt / geometry push / box resize): re-blit every
 *  visible pane from its retained scratch onto an already-cleared overlay. */
export function reblitAllSpillPanes(ctx: SpillCanvas2D, panes: Iterable<SpillPaneState>): void {
  for (const pane of panes) {
    const geometry = pane.geometry
    if (!geometry?.visible || !pane.scratch || geometry.outsideRects.length === 0) {
      pane.prevDrawnRects = EMPTY_SPILL_RECTS
      continue
    }
    blitSpillOutsideRects(ctx, pane, geometry)
    pane.prevDrawnRects = geometry.outsideRects
  }
}
