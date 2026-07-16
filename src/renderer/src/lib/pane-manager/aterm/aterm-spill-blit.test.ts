/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AtermDeviceRect } from './aterm-chrome-box'
import { createAtermSpillBlit, hasAtermSpillExports } from './aterm-spill-blit'
import { createAtermSpillOverlay, type AtermSpillOverlay } from './aterm-spill-overlay'
import type { SpillPaneGeometry, SpillStripSlot } from './aterm-spill-pane-scratch'

// The in-process spill read contract (stage 3): rev-gated (an unchanged
// spill_rev never touches a spill byte), wasm views rebuilt per read (growth
// detaches them), retained ImageData/scratches (zero steady-frame allocations),
// and the live reader feeding the overlay's clear-union + intersect-expansion
// pass so a neighbor's settled ring survives this pane's animation clear.

/** Minimal ImageData stand-in (happy-dom ships none) with a constructor spy. */
class FakeImageData {
  readonly width: number
  readonly height: number
  readonly data: Uint8ClampedArray
  constructor(width: number, height: number) {
    this.width = width
    this.height = height
    this.data = new Uint8ClampedArray(width * height * 4)
    imageDataConstructions.push(this)
  }
}
let imageDataConstructions: FakeImageData[] = []

const rect = (x: number, y: number, width: number, height: number): AtermDeviceRect => ({
  x,
  y,
  width,
  height
})

type FakeTermState = {
  rev: number
  len: number
  ptr: number
  rectsPtr: number
  rectCount: number
  chromePad: number
  chromeHead: number
}

function makeTerm(overrides: Partial<FakeTermState> = {}): {
  state: FakeTermState
  term: {
    chrome_pad: number
    chrome_head: number
    spill_rev: () => number
    spill_rect_count: () => number
    spill_rects_ptr: () => number
    spill_ptr: () => number
    spill_len: () => number
  }
} {
  const state: FakeTermState = {
    rev: 1,
    len: 32,
    ptr: 0,
    rectsPtr: 0,
    rectCount: 0,
    chromePad: 13,
    chromeHead: 34,
    ...overrides
  }
  return {
    state,
    term: {
      get chrome_pad() {
        return state.chromePad
      },
      get chrome_head() {
        return state.chromeHead
      },
      spill_rev: () => state.rev,
      spill_rect_count: () => state.rectCount,
      spill_rects_ptr: () => state.rectsPtr,
      spill_ptr: () => state.ptr,
      spill_len: () => state.len
    }
  }
}

/** A one-strip pane: 4×2 device px at overlay (100, 50) → 32 spill bytes. */
const ONE_STRIP: SpillStripSlot[] = [
  { overlayRect: rect(100, 50, 4, 2), scratchOrigin: { x: 0, y: 0 } }
]

type ScratchCall = { image: FakeImageData; x: number; y: number }
type SpillReader = Parameters<AtermSpillOverlay['runSpillPassInProcess']>[1]

type OverlayHarness = {
  overlay: Pick<AtermSpillOverlay, 'runSpillPassInProcess'>
  passes: number
  puts: ScratchCall[]
  dirtySnapshots: (readonly AtermDeviceRect[] | null)[]
}

/** Overlay stub that always invokes the reader against fixed strip slots and
 *  records what the reader wrote + returned. */
function makeOverlayHarness(strips: readonly SpillStripSlot[] = ONE_STRIP): OverlayHarness {
  const harness: OverlayHarness = {
    passes: 0,
    puts: [],
    dirtySnapshots: [],
    overlay: {
      runSpillPassInProcess: (_paneKey: string, reader: SpillReader): void => {
        harness.passes++
        const ctx = {
          putImageData: (image: FakeImageData, x: number, y: number) => {
            harness.puts.push({ image, x, y })
          }
        } as unknown as CanvasRenderingContext2D
        const dirty = reader({ ctx, strips })
        // Snapshot the reused records: the blit mutates them on the next pass.
        harness.dirtySnapshots.push(dirty ? dirty.map((d) => ({ ...d })) : dirty)
      }
    }
  }
  return harness
}

beforeEach(() => {
  imageDataConstructions = []
  vi.stubGlobal('ImageData', FakeImageData)
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('hasAtermSpillExports', () => {
  it('requires the whole surface (worker facade terms expose none of it)', () => {
    const { term } = makeTerm()
    expect(hasAtermSpillExports(term)).toBe(true)
    expect(hasAtermSpillExports({})).toBe(false)
    expect(hasAtermSpillExports({ ...term, spill_rev: undefined } as object)).toBe(false)
  })
})

describe('rev gate', () => {
  it('an unchanged spill_rev skips the pass entirely (no scratch refresh)', () => {
    const { state, term } = makeTerm()
    const harness = makeOverlayHarness()
    const blit = createAtermSpillBlit({
      term,
      memory: { buffer: new ArrayBuffer(32) },
      getPaneKey: () => 'tab:a',
      isDisposed: () => false,
      scheduleDraw: vi.fn(),
      overlay: harness.overlay
    })
    blit()
    expect(harness.passes).toBe(1)
    expect(harness.puts).toHaveLength(1)
    blit()
    blit()
    expect(harness.passes).toBe(1)
    expect(harness.puts).toHaveLength(1)
    state.rev = 2
    blit()
    expect(harness.passes).toBe(2)
    expect(harness.puts).toHaveLength(2)
  })

  it('zero chrome and an unbound pane key never read the engine', () => {
    const revSpy = vi.fn(() => 1)
    const { term } = makeTerm({ chromePad: 0, chromeHead: 0 })
    const spiedTerm = { ...term, spill_rev: revSpy }
    const harness = makeOverlayHarness()
    let paneKey: string | undefined
    const blit = createAtermSpillBlit({
      term: spiedTerm,
      memory: { buffer: new ArrayBuffer(32) },
      getPaneKey: () => paneKey,
      isDisposed: () => false,
      scheduleDraw: vi.fn(),
      overlay: harness.overlay
    })
    blit() // no pane key yet
    paneKey = 'tab:a'
    blit() // chrome still 0/0
    expect(revSpy).not.toHaveBeenCalled()
    expect(harness.passes).toBe(0)
  })
})

describe('wasm view discipline', () => {
  it('rebuilds the view over spill_ptr on EVERY read (growth detaches old views)', () => {
    const { state, term } = makeTerm()
    const harness = makeOverlayHarness()
    const firstBuffer = new ArrayBuffer(32)
    new Uint8Array(firstBuffer).fill(0x11)
    const memory = { buffer: firstBuffer as ArrayBufferLike }
    const blit = createAtermSpillBlit({
      term,
      memory,
      getPaneKey: () => 'tab:a',
      isDisposed: () => false,
      scheduleDraw: vi.fn(),
      overlay: harness.overlay
    })
    blit()
    expect(harness.puts[0].image.data.every((byte) => byte === 0x11)).toBe(true)
    // Simulate wasm memory growth: the module swaps in a NEW backing buffer
    // (the old view is dead). A cached view would still read the old bytes.
    const grownBuffer = new ArrayBuffer(64)
    new Uint8Array(grownBuffer).fill(0x77)
    memory.buffer = grownBuffer
    state.rev = 2
    blit()
    expect(harness.puts[1].image.data.every((byte) => byte === 0x77)).toBe(true)
  })
})

describe('zero-allocation reuse', () => {
  it('keeps ONE ImageData per strip across frames (same identity, no per-frame alloc)', () => {
    const { state, term } = makeTerm()
    const harness = makeOverlayHarness()
    const blit = createAtermSpillBlit({
      term,
      memory: { buffer: new ArrayBuffer(32) },
      getPaneKey: () => 'tab:a',
      isDisposed: () => false,
      scheduleDraw: vi.fn(),
      overlay: harness.overlay
    })
    blit()
    state.rev = 2
    blit()
    state.rev = 3
    blit()
    expect(imageDataConstructions).toHaveLength(1)
    expect(harness.puts[1].image).toBe(harness.puts[0].image)
    expect(harness.puts[2].image).toBe(harness.puts[0].image)
    // The dirty list is the same retained record set each pass, not fresh objects.
    expect(harness.dirtySnapshots[1]).toEqual(harness.dirtySnapshots[2])
  })
})

describe('dirty-rect mapping', () => {
  it('first blit and rev gaps use the full band; a contiguous rev maps engine rects to overlay space', () => {
    // Buffer layout: 32 pixel bytes then one packed i32 dirty rect at offset 32.
    const buffer = new ArrayBuffer(48)
    new Int32Array(buffer, 32, 4).set([1, 0, 2, 1])
    const { state, term } = makeTerm({ rectsPtr: 32 })
    const harness = makeOverlayHarness()
    const blit = createAtermSpillBlit({
      term,
      memory: { buffer },
      getPaneKey: () => 'tab:a',
      isDisposed: () => false,
      scheduleDraw: vi.fn(),
      overlay: harness.overlay
    })
    state.rectCount = 1
    blit() // first blit: engine rects untrusted → whole band
    expect(harness.dirtySnapshots[0]).toEqual([rect(100, 50, 4, 2)])
    state.rev = 2
    blit() // contiguous: frame-absolute (1,0,2,1) + frame origin (100,50)
    expect(harness.dirtySnapshots[1]).toEqual([rect(101, 50, 2, 1)])
    state.rev = 5
    blit() // missed revisions: fall back to the whole band again
    expect(harness.dirtySnapshots[2]).toEqual([rect(100, 50, 4, 2)])
  })
})

describe('skipped-pass retry', () => {
  it('a byte-length mismatch keeps the rev unconsumed and re-arms a BOUNDED draw retry', () => {
    const { state, term } = makeTerm({ len: 999 })
    const harness = makeOverlayHarness()
    const scheduleDraw = vi.fn()
    const blit = createAtermSpillBlit({
      term,
      memory: { buffer: new ArrayBuffer(4096) },
      getPaneKey: () => 'tab:a',
      isDisposed: () => false,
      scheduleDraw,
      overlay: harness.overlay
    })
    for (let i = 0; i < 6; i++) {
      blit()
    }
    // Reader was offered each time (the overlay pass ran) but never landed…
    expect(harness.puts).toHaveLength(0)
    // …and the re-arm is bounded per revision (no permanent rAF loop).
    expect(scheduleDraw).toHaveBeenCalledTimes(3)
    // Once the engine/geometry agree again, the SAME revision still lands.
    state.len = 32
    blit()
    expect(harness.puts).toHaveLength(1)
    blit()
    expect(harness.puts).toHaveLength(1) // now rev-gated as usual
  })
})

// ── Live reader against the REAL overlay: the erasure tripwire ──────────────

type CtxOp = { op: string; args: unknown[] }
type FakeCtx = Record<string, unknown> & { canvas: HTMLCanvasElement; ops: CtxOp[] }

const fakeCtxByCanvas = new WeakMap<HTMLCanvasElement, FakeCtx>()
let originalGetContext: typeof HTMLCanvasElement.prototype.getContext

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
    putImageData: record('putImageData'),
    save: record('save'),
    restore: record('restore'),
    beginPath: record('beginPath'),
    rect: record('rect'),
    clip: record('clip')
  }
}

// Two glowing panes whose bands overlap across the shared gap (the stage-2
// overlay fixtures): A's strip spans x 300..340, B's x 320..360.
const GEOM_A: SpillPaneGeometry = {
  frameOrigin: { x: 300, y: 100 },
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

describe('live readSpill against the real overlay', () => {
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
  })

  it("pane B's live blit re-blits pane A's overlapping ring from A's retained scratch", async () => {
    const overlay = createAtermSpillOverlay()
    const canvas = document.createElement('canvas')
    overlay.attachCanvas(canvas)
    overlay.setOverlayBox({ widthPx: 800, heightPx: 600 })
    overlay.register('tab:a', { chromePadPx: 40, chromeHeadPx: 0 })
    overlay.register('tab:b', { chromePadPx: 40, chromeHeadPx: 0 })
    overlay.updateGeometry('tab:a', GEOM_A)
    overlay.updateGeometry('tab:b', GEOM_B)
    await Promise.resolve() // flush the queued recomposite

    const stripBytes = 40 * 200 * 4
    const makePane = (paneKey: string): { blit: () => void; state: FakeTermState } => {
      const { state, term } = makeTerm({ len: stripBytes, chromePad: 40, chromeHead: 0 })
      return {
        state,
        blit: createAtermSpillBlit({
          term,
          memory: { buffer: new ArrayBuffer(stripBytes) },
          getPaneKey: () => paneKey,
          isDisposed: () => false,
          scheduleDraw: vi.fn(),
          overlay
        })
      }
    }
    const paneA = makePane('tab:a')
    const paneB = makePane('tab:b')

    const overlayCtx = fakeCtxByCanvas.get(canvas)
    if (!overlayCtx) {
      throw new Error('overlay canvas has no context')
    }
    paneA.blit()
    const aDraw = overlayCtx.ops.find((op) => op.op === 'drawImage')
    expect(aDraw, "A's own blit must draw its outside rect").toBeDefined()
    const scratchA = aDraw?.args[0]
    overlayCtx.ops.length = 0

    paneB.blit()
    // B's pass cleared its dirty outside band (which overlaps A at x 320..340)…
    expect(overlayCtx.ops.some((op) => op.op === 'clearRect')).toBe(true)
    // …and re-blitted BOTH panes from their retained scratches inside the union.
    const sources = overlayCtx.ops.filter((op) => op.op === 'drawImage').map((op) => op.args[0])
    expect(sources).toContain(scratchA)
    expect(sources).toHaveLength(2)

    // A settled (rev unchanged) pane stays rev-gated: B re-blits alone next time.
    overlayCtx.ops.length = 0
    paneA.blit()
    expect(overlayCtx.ops).toHaveLength(0)
    paneB.state.rev = 2
    paneB.blit()
    expect(overlayCtx.ops.filter((op) => op.op === 'drawImage').length).toBeGreaterThan(0)
  })
})
