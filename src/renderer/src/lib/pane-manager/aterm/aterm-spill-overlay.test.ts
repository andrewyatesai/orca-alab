/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AtermDeviceRect } from './aterm-chrome-box'
import {
  createAtermSpillOverlay,
  type AtermSpillOverlay,
  type SpillPaneGeometry
} from './aterm-spill-overlay'

// The compositor contract from the spill plan: zero cost at zero registrations
// (dormant 0×0 display:none canvas, no rAF/timer bookings), idempotent
// registration, and the clear-union + intersect-expansion re-blit — the
// erasure tripwire where pane A's settled ring must survive pane B's
// animation clear via a pure drawImage from A's retained scratch.

type CtxOp = { op: string; args: unknown[] }

type FakeCtx = {
  canvas: HTMLCanvasElement
  ops: CtxOp[]
  clearRect: (...args: unknown[]) => void
  drawImage: (...args: unknown[]) => void
  save: () => void
  restore: () => void
  beginPath: () => void
  rect: (...args: unknown[]) => void
  clip: () => void
}

function makeFakeCtx(canvas: HTMLCanvasElement): FakeCtx {
  const ops: CtxOp[] = []
  const record =
    (op: string) =>
    (...args: unknown[]): void => {
      ops.push({ op, args })
    }
  return {
    canvas,
    ops,
    clearRect: record('clearRect'),
    drawImage: record('drawImage'),
    save: record('save'),
    restore: record('restore'),
    beginPath: record('beginPath'),
    rect: record('rect'),
    clip: record('clip')
  }
}

const fakeCtxByCanvas = new WeakMap<HTMLCanvasElement, FakeCtx>()
let originalGetContext: typeof HTMLCanvasElement.prototype.getContext

function ctxOf(canvas: HTMLCanvasElement): FakeCtx {
  const ctx = fakeCtxByCanvas.get(canvas)
  if (!ctx) {
    throw new Error('no context was created for this canvas')
  }
  return ctx
}

const rect = (x: number, y: number, width: number, height: number): AtermDeviceRect => ({
  x,
  y,
  width,
  height
})

// Two glowing panes whose chrome bands overlap across the shared gap: A's
// right strip spans x 300..340, B's left strip x 320..360 (overlap 320..340).
const GEOM_A: SpillPaneGeometry = {
  frameOrigin: { x: 60, y: 100 },
  clipRect: rect(95, 95, 210, 210),
  stripRects: [rect(300, 100, 40, 200)],
  outsideRects: [rect(305, 100, 35, 200)],
  visible: true
}
const GEOM_B: SpillPaneGeometry = {
  frameOrigin: { x: 320, y: 100 },
  clipRect: rect(355, 95, 210, 210),
  stripRects: [rect(320, 100, 40, 200)],
  outsideRects: [rect(320, 100, 35, 200)],
  visible: true
}

const flushMicrotasks = async (): Promise<void> => {
  await Promise.resolve()
}

function attachOverlayCanvas(overlay: AtermSpillOverlay): {
  canvas: HTMLCanvasElement
  detach: () => void
} {
  const canvas = document.createElement('canvas')
  const detach = overlay.attachCanvas(canvas)
  return { canvas, detach }
}

beforeEach(() => {
  originalGetContext = HTMLCanvasElement.prototype.getContext
  HTMLCanvasElement.prototype.getContext = function (this: HTMLCanvasElement) {
    let ctx = fakeCtxByCanvas.get(this)
    if (!ctx) {
      ctx = makeFakeCtx(this)
      fakeCtxByCanvas.set(this, ctx)
    }
    return ctx as never
  } as never
})

afterEach(() => {
  HTMLCanvasElement.prototype.getContext = originalGetContext
  vi.useRealTimers()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('registry idempotence', () => {
  it('double register with identical chrome is a single registration and one notify', () => {
    const overlay = createAtermSpillOverlay()
    const listener = vi.fn()
    overlay.subscribe(listener)
    overlay.register('tab:a', { chromePadPx: 13, chromeHeadPx: 34 })
    overlay.register('tab:a', { chromePadPx: 13, chromeHeadPx: 34 })
    expect(overlay.getPaneCount()).toBe(1)
    expect(listener).toHaveBeenCalledTimes(1)
    expect(overlay.getPaneChrome('tab:a')).toEqual({ chromePadPx: 13, chromeHeadPx: 34 })
  })

  it('re-register with new chrome updates the record and notifies again', () => {
    const overlay = createAtermSpillOverlay()
    const listener = vi.fn()
    overlay.subscribe(listener)
    overlay.register('tab:a', { chromePadPx: 13, chromeHeadPx: 34 })
    overlay.register('tab:a', { chromePadPx: 15, chromeHeadPx: 40 })
    expect(overlay.getPaneCount()).toBe(1)
    expect(listener).toHaveBeenCalledTimes(2)
    expect(overlay.getPaneChrome('tab:a')).toEqual({ chromePadPx: 15, chromeHeadPx: 40 })
  })

  it('double unregister is safe and notifies only once', () => {
    const overlay = createAtermSpillOverlay()
    overlay.register('tab:a', { chromePadPx: 13, chromeHeadPx: 34 })
    const listener = vi.fn()
    overlay.subscribe(listener)
    overlay.unregister('tab:a')
    overlay.unregister('tab:a')
    expect(overlay.getPaneCount()).toBe(0)
    expect(listener).toHaveBeenCalledTimes(1)
    expect(overlay.getPaneChrome('tab:a')).toBeNull()
  })
})

describe('zero work at idle', () => {
  it('books no rAF or timers and keeps the canvas dormant at zero registrations', () => {
    vi.useFakeTimers()
    const raf = vi.fn()
    vi.stubGlobal('requestAnimationFrame', raf)
    const overlay = createAtermSpillOverlay()
    const { canvas } = attachOverlayCanvas(overlay)
    expect(canvas.style.display).toBe('none')
    expect(canvas.width).toBe(0)
    expect(canvas.height).toBe(0)
    expect(raf).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('returns to the dormant 0×0 display:none state after the last unregister', async () => {
    vi.useFakeTimers()
    const raf = vi.fn()
    vi.stubGlobal('requestAnimationFrame', raf)
    const overlay = createAtermSpillOverlay()
    const { canvas } = attachOverlayCanvas(overlay)
    overlay.register('tab:a', { chromePadPx: 13, chromeHeadPx: 34 })
    overlay.setOverlayBox({ widthPx: 800, heightPx: 600 })
    expect(canvas.style.display).toBe('')
    expect(canvas.width).toBe(800)
    expect(canvas.height).toBe(600)
    overlay.updateGeometry('tab:a', GEOM_A)
    overlay.unregister('tab:a')
    // The clear-once is the synchronous 0×0 resize; the queued recomposite was
    // superseded and must not revive the canvas.
    expect(canvas.style.display).toBe('none')
    expect(canvas.width).toBe(0)
    expect(canvas.height).toBe(0)
    await flushMicrotasks()
    expect(canvas.width).toBe(0)
    expect(raf).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0)
  })
})

describe('spill pass clear-union + intersect-expansion', () => {
  function paintPane(
    overlay: AtermSpillOverlay,
    paneKey: string,
    dirty: readonly AtermDeviceRect[]
  ): HTMLCanvasElement {
    let scratch: HTMLCanvasElement | null = null
    overlay.runSpillPassInProcess(paneKey, (target) => {
      scratch = target.ctx.canvas as HTMLCanvasElement
      return dirty
    })
    if (!scratch) {
      throw new Error(`spill pass for ${paneKey} never invoked its reader`)
    }
    return scratch
  }

  async function setUpTwoPanes(): Promise<{
    overlay: AtermSpillOverlay
    overlayCtx: FakeCtx
    scratchA: HTMLCanvasElement
  }> {
    const overlay = createAtermSpillOverlay()
    const { canvas } = attachOverlayCanvas(overlay)
    overlay.setOverlayBox({ widthPx: 800, heightPx: 600 })
    overlay.register('tab:a', { chromePadPx: 40, chromeHeadPx: 0 })
    overlay.register('tab:b', { chromePadPx: 40, chromeHeadPx: 0 })
    overlay.updateGeometry('tab:a', GEOM_A)
    overlay.updateGeometry('tab:b', GEOM_B)
    await flushMicrotasks()
    const overlayCtx = ctxOf(canvas)
    const scratchA = paintPane(overlay, 'tab:a', GEOM_A.stripRects)
    overlayCtx.ops.length = 0
    return { overlay, overlayCtx, scratchA }
  }

  it("pane A's static ring survives pane B's animation clear (erasure tripwire)", async () => {
    const { overlay, overlayCtx, scratchA } = await setUpTwoPanes()
    const scratchB = paintPane(overlay, 'tab:b', GEOM_B.stripRects)

    // B's pass cleared its own dirty outside band...
    const clears = overlayCtx.ops.filter((op) => op.op === 'clearRect')
    expect(clears).toEqual([{ op: 'clearRect', args: [320, 100, 35, 200] }])
    // ...clipped the re-blit to exactly the cleared union...
    const clipRects = overlayCtx.ops.filter((op) => op.op === 'rect')
    expect(clipRects).toEqual([{ op: 'rect', args: [320, 100, 35, 200] }])
    expect(overlayCtx.ops.some((op) => op.op === 'clip')).toBe(true)
    // ...and re-blitted BOTH panes from their retained scratches: A's rect
    // overlaps the cleared union (x 320..340), so its ring is restored.
    const drawSources = overlayCtx.ops.filter((op) => op.op === 'drawImage').map((op) => op.args[0])
    expect(drawSources).toContain(scratchA)
    expect(drawSources).toContain(scratchB)
    // The clear happens before every re-blit (clear phase, then blit phase).
    const lastClear = overlayCtx.ops.findLastIndex((op) => op.op === 'clearRect')
    const firstDraw = overlayCtx.ops.findIndex((op) => op.op === 'drawImage')
    expect(lastClear).toBeLessThan(firstDraw)
  })

  it('re-blits map overlay rects back through the packed scratch slots', async () => {
    const overlay = createAtermSpillOverlay()
    const { canvas } = attachOverlayCanvas(overlay)
    overlay.setOverlayBox({ widthPx: 800, heightPx: 600 })
    overlay.register('tab:a', { chromePadPx: 40, chromeHeadPx: 0 })
    overlay.updateGeometry('tab:a', GEOM_A)
    await flushMicrotasks()
    const overlayCtx = ctxOf(canvas)
    overlayCtx.ops.length = 0
    let scratch: HTMLCanvasElement | null = null
    overlay.runSpillPassInProcess('tab:a', (target) => {
      scratch = target.ctx.canvas as HTMLCanvasElement
      expect(target.strips).toEqual([
        { overlayRect: rect(300, 100, 40, 200), scratchOrigin: { x: 0, y: 0 } }
      ])
      return GEOM_A.stripRects
    })
    // A's outsideRect {305,100} lives at x offset 5 inside its packed strip
    // slot (strip overlay origin x=300 → scratch origin (0,0)).
    const draw = overlayCtx.ops.find((op) => op.op === 'drawImage')
    expect(draw?.args).toEqual([scratch, 5, 0, 35, 200, 305, 100, 35, 200])
  })

  it('an unchanged export (null dirty) does zero overlay work', async () => {
    const { overlay, overlayCtx } = await setUpTwoPanes()
    overlay.runSpillPassInProcess('tab:a', () => null)
    overlay.runSpillPassInProcess('tab:b', () => [])
    expect(overlayCtx.ops).toEqual([])
  })

  it('invisible or unmeasured panes never invoke the reader', async () => {
    const { overlay } = await setUpTwoPanes()
    overlay.updateGeometry('tab:a', { ...GEOM_A, visible: false })
    await flushMicrotasks()
    const reader = vi.fn(() => null)
    overlay.runSpillPassInProcess('tab:a', reader)
    overlay.runSpillPassInProcess('tab:never-registered', reader)
    expect(reader).not.toHaveBeenCalled()
  })
})

describe('geometry and box pushes', () => {
  it('an unchanged geometry push schedules no recomposite', async () => {
    const overlay = createAtermSpillOverlay()
    const { canvas } = attachOverlayCanvas(overlay)
    overlay.setOverlayBox({ widthPx: 800, heightPx: 600 })
    overlay.register('tab:a', { chromePadPx: 40, chromeHeadPx: 0 })
    overlay.updateGeometry('tab:a', GEOM_A)
    await flushMicrotasks()
    const overlayCtx = ctxOf(canvas)
    overlayCtx.ops.length = 0
    overlay.updateGeometry('tab:a', { ...GEOM_A })
    await flushMicrotasks()
    expect(overlayCtx.ops).toEqual([])
  })

  it('a geometry change coalesces into ONE full clear + re-blit-all recomposite', async () => {
    const overlay = createAtermSpillOverlay()
    const { canvas } = attachOverlayCanvas(overlay)
    overlay.setOverlayBox({ widthPx: 800, heightPx: 600 })
    overlay.register('tab:a', { chromePadPx: 40, chromeHeadPx: 0 })
    overlay.register('tab:b', { chromePadPx: 40, chromeHeadPx: 0 })
    overlay.updateGeometry('tab:a', GEOM_A)
    overlay.updateGeometry('tab:b', GEOM_B)
    await flushMicrotasks()
    const overlayCtx = ctxOf(canvas)
    // Give both panes scratch content so the recomposite re-blits them.
    overlay.runSpillPassInProcess('tab:a', () => GEOM_A.stripRects)
    overlay.runSpillPassInProcess('tab:b', () => GEOM_B.stripRects)
    overlayCtx.ops.length = 0
    // A drag moves both panes in one measure batch → one recomposite.
    const movedA = {
      ...GEOM_A,
      stripRects: [rect(310, 100, 40, 200)],
      outsideRects: [rect(315, 100, 35, 200)]
    }
    const movedB = {
      ...GEOM_B,
      stripRects: [rect(330, 100, 40, 200)],
      outsideRects: [rect(330, 100, 35, 200)]
    }
    overlay.updateGeometry('tab:a', movedA)
    overlay.updateGeometry('tab:b', movedB)
    await flushMicrotasks()
    const fullClears = overlayCtx.ops.filter(
      (op) => op.op === 'clearRect' && op.args[2] === 800 && op.args[3] === 600
    )
    expect(fullClears).toHaveLength(1)
    expect(overlayCtx.ops.filter((op) => op.op === 'drawImage')).toHaveLength(2)
  })

  it('an overlay box change resizes the backing store and re-blits', async () => {
    const overlay = createAtermSpillOverlay()
    const { canvas } = attachOverlayCanvas(overlay)
    overlay.setOverlayBox({ widthPx: 800, heightPx: 600 })
    overlay.register('tab:a', { chromePadPx: 40, chromeHeadPx: 0 })
    overlay.updateGeometry('tab:a', GEOM_A)
    await flushMicrotasks()
    overlay.runSpillPassInProcess('tab:a', () => GEOM_A.stripRects)
    const overlayCtx = ctxOf(canvas)
    overlayCtx.ops.length = 0
    overlay.setOverlayBox({ widthPx: 1000, heightPx: 700 })
    await flushMicrotasks()
    expect(canvas.width).toBe(1000)
    expect(canvas.height).toBe(700)
    expect(overlayCtx.ops.filter((op) => op.op === 'drawImage')).toHaveLength(1)
  })
})
