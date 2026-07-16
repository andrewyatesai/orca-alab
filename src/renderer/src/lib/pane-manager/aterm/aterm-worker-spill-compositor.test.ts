import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AtermDeviceRect } from './aterm-chrome-box'
import type { SpillPaneGeometry } from './aterm-spill-pane-scratch'
import {
  createAtermWorkerSpillCompositor,
  type WorkerSpillPaneSource
} from './aterm-worker-spill-compositor'

// The worker-side compositor contract (stage 4): the swap-cleared dirty array
// makes the flush-epilogue + eager-tail double invocation exactly ONE real
// pass; dead-epoch canvas messages are dropped; a departing pane clears its
// strips and neighbors recover from their retained scratches.

type CtxOp = { op: string; args: unknown[] }

type FakeCanvas = {
  width: number
  height: number
  ctx: FakeCtx
  getContext: (id: string) => FakeCtx
}

type FakeCtx = {
  canvas: FakeCanvas
  ops: CtxOp[]
  clearRect: (...args: unknown[]) => void
  drawImage: (...args: unknown[]) => void
  putImageData: (...args: unknown[]) => void
  save: () => void
  restore: () => void
  beginPath: () => void
  rect: (...args: unknown[]) => void
  clip: () => void
}

function makeFakeCanvas(width = 0, height = 0): FakeCanvas {
  const ops: CtxOp[] = []
  const record =
    (op: string) =>
    (...args: unknown[]): void => {
      ops.push({ op, args })
    }
  const canvas: FakeCanvas = {
    width,
    height,
    ctx: undefined as unknown as FakeCtx,
    getContext: () => canvas.ctx
  }
  canvas.ctx = {
    canvas,
    ops,
    clearRect: record('clearRect'),
    drawImage: record('drawImage'),
    putImageData: record('putImageData'),
    save: record('save'),
    restore: record('restore'),
    beginPath: record('beginPath'),
    rect: record('rect'),
    clip: record('clip')
  }
  return canvas
}

// Minimal ImageData for the node env (the shared reader constructs them).
class FakeImageData {
  width: number
  height: number
  data: Uint8ClampedArray
  constructor(width: number, height: number) {
    this.width = width
    this.height = height
    this.data = new Uint8ClampedArray(width * height * 4)
  }
}

/** A spill-exporting fake engine over a real ArrayBuffer "linear memory". */
function makeSpillEngine(spillBytes: number): {
  engine: object
  memory: { buffer: ArrayBufferLike }
  bumpRev: () => void
  reads: () => number
} {
  const memory = { buffer: new ArrayBuffer(4096) }
  let rev = 0
  let reads = 0
  const engine = {
    spill_rev: () => rev,
    spill_rect_count: () => 0,
    spill_rects_ptr: () => 0,
    spill_ptr: () => {
      reads++
      return 0
    },
    spill_len: () => spillBytes
  }
  return { engine, memory, bumpRev: () => rev++, reads: () => reads }
}

const rect = (x: number, y: number, width: number, height: number): AtermDeviceRect => ({
  x,
  y,
  width,
  height
})

// One 4×2 strip above the pane's clip box → the whole strip is outside.
const GEOM: SpillPaneGeometry = {
  frameOrigin: { x: 0, y: 0 },
  clipRect: rect(0, 2, 4, 2),
  stripRects: [rect(0, 0, 4, 2)],
  outsideRects: [rect(0, 0, 4, 2)],
  visible: true
}
const GEOM_BYTES = 4 * 2 * 4

function makeHarness(): {
  compositor: ReturnType<typeof createAtermWorkerSpillCompositor>
  sources: Map<number, WorkerSpillPaneSource>
  retries: number[]
  scratches: FakeCanvas[]
  overlay: FakeCanvas
  initCanvas: (epoch: number) => void
} {
  const sources = new Map<number, WorkerSpillPaneSource>()
  const retries: number[] = []
  const scratches: FakeCanvas[] = []
  const compositor = createAtermWorkerSpillCompositor({
    resolvePane: (paneId) => sources.get(paneId) ?? null,
    requestRenderRetry: (paneId) => retries.push(paneId),
    createScratchCanvas: (width, height) => {
      const scratch = makeFakeCanvas(width, height)
      scratches.push(scratch)
      return scratch as unknown as OffscreenCanvas
    }
  })
  const overlay = makeFakeCanvas()
  const initCanvas = (epoch: number): void => {
    compositor.dispatch({
      type: 'spillCanvasInit',
      epoch,
      canvas: overlay as unknown as OffscreenCanvas,
      box: { widthPx: 800, heightPx: 600 },
      dpr: 2,
      paneId: 0
    })
  }
  return { compositor, sources, retries, scratches, overlay, initCanvas }
}

function addPane(
  harness: ReturnType<typeof makeHarness>,
  paneId: number,
  paneKey: string,
  geometry: SpillPaneGeometry = GEOM
): ReturnType<typeof makeSpillEngine> {
  const eng = makeSpillEngine(GEOM_BYTES)
  harness.sources.set(paneId, {
    engine: eng.engine,
    memory: eng.memory,
    chrome: { pad: 1, head: 1 }
  })
  harness.compositor.dispatch({ type: 'spillPaneRects', paneKey, geometry, paneId })
  return eng
}

beforeEach(() => {
  vi.stubGlobal('ImageData', FakeImageData)
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('worker spill compositor', () => {
  it('swap-clears the dirty array: epilogue + eager-tail double invoke = ONE real pass', () => {
    const h = makeHarness()
    h.initCanvas(1)
    const eng = addPane(h, 7, 'tab:a')
    // The bind-time flush consumed rev 0; a new burn frame advances it.
    eng.bumpRev()
    const readsBefore = eng.reads()
    h.compositor.markPaneDirty(7)
    h.compositor.markPaneDirty(7) // second render in the same frame dedupes
    h.compositor.runSpillPass() // the eager presentNow tail
    h.compositor.runSpillPass() // the shared-rAF flush epilogue
    expect(eng.reads() - readsBefore).toBe(1)
    // Unchanged rev never re-marks (idle-to-zero).
    h.compositor.markPaneDirty(7)
    h.compositor.runSpillPass()
    expect(eng.reads() - readsBefore).toBe(1)
  })

  it('binds + composites at geometry arrival, so a settled burn still lands its final band', () => {
    const h = makeHarness()
    h.initCanvas(1)
    const eng = addPane(h, 3, 'tab:a')
    // The bind-time mark+flush already read rev 0 (the settled band) and blitted.
    expect(eng.reads()).toBe(1)
    const blits = h.overlay.ctx.ops.filter((op) => op.op === 'drawImage')
    expect(blits.length).toBeGreaterThan(0)
    // Scratch holds exact straight-alpha strip bytes via putImageData.
    expect(h.scratches[0]?.ctx.ops.some((op) => op.op === 'putImageData')).toBe(true)
  })

  it('drops dead-epoch canvas inits and mismatched box/release messages', () => {
    const h = makeHarness()
    h.initCanvas(2)
    addPane(h, 1, 'tab:a')
    const stale = makeFakeCanvas()
    h.compositor.dispatch({
      type: 'spillCanvasInit',
      epoch: 1, // dead: a retired generation's late transfer
      canvas: stale as unknown as OffscreenCanvas,
      box: { widthPx: 10, heightPx: 10 },
      dpr: 1,
      paneId: 0
    })
    expect(stale.ctx.ops.length).toBe(0)
    expect(h.overlay.width).toBe(800)
    // A stale-epoch box resize is dropped too.
    h.compositor.dispatch({
      type: 'spillOverlayBox',
      epoch: 1,
      box: { widthPx: 5, heightPx: 5 },
      paneId: 0
    })
    expect(h.overlay.width).toBe(800)
    // A stale release keeps the live canvas; the matching one clears + drops it.
    h.compositor.dispatch({ type: 'spillRelease', epoch: 1, paneId: 0 })
    h.overlay.ctx.ops.length = 0
    h.compositor.dispatch({ type: 'spillRelease', epoch: 2, paneId: 0 })
    expect(h.overlay.ctx.ops.some((op) => op.op === 'clearRect')).toBe(true)
    // Canvas is gone: a burn now refreshes scratches but blits nothing.
    h.overlay.ctx.ops.length = 0
    const eng = addPane(h, 2, 'tab:b')
    eng.bumpRev()
    h.compositor.markPaneDirty(2)
    h.compositor.runSpillPass()
    expect(h.overlay.ctx.ops.length).toBe(0)
  })

  it('pane close mid-burn clears its strips and re-blits intersecting neighbors', () => {
    const h = makeHarness()
    h.initCanvas(1)
    addPane(h, 1, 'tab:a')
    // Neighbor whose outside band overlaps A's (same strip region shifted 2px).
    const geomB: SpillPaneGeometry = {
      frameOrigin: { x: 2, y: 0 },
      clipRect: rect(2, 2, 4, 2),
      stripRects: [rect(2, 0, 4, 2)],
      outsideRects: [rect(2, 0, 4, 2)],
      visible: true
    }
    addPane(h, 2, 'tab:b', geomB)
    h.overlay.ctx.ops.length = 0
    h.compositor.handlePaneDisposed(1)
    const ops = h.overlay.ctx.ops.map((op) => op.op)
    // A's prevDrawn strip cleared…
    expect(ops).toContain('clearRect')
    // …and B (intersecting) restored from its retained scratch under a clip.
    expect(ops).toContain('clip')
    expect(ops).toContain('drawImage')
    // A is gone: marking its old paneId is inert.
    h.compositor.markPaneDirty(1)
    h.overlay.ctx.ops.length = 0
    h.compositor.runSpillPass()
    expect(h.overlay.ctx.ops.length).toBe(0)
  })

  it('re-arms a bounded render retry when the export/geometry bytes mismatch', () => {
    const h = makeHarness()
    h.initCanvas(1)
    const eng = makeSpillEngine(GEOM_BYTES * 2) // engine resized ahead of the measure
    h.sources.set(9, { engine: eng.engine, memory: eng.memory, chrome: { pad: 1, head: 1 } })
    h.compositor.dispatch({ type: 'spillPaneRects', paneKey: 'tab:z', geometry: GEOM, paneId: 9 })
    expect(h.retries).toEqual([9])
    // Retries are bounded per revision: repeated skipped passes stop re-arming.
    for (let i = 0; i < 6; i++) {
      h.compositor.markPaneDirty(9)
      h.compositor.runSpillPass()
    }
    expect(h.retries.length).toBeLessThanOrEqual(4)
  })
})
