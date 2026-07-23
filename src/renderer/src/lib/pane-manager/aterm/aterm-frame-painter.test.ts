import { beforeAll, describe, expect, it, vi } from 'vitest'
import { createAtermFramePainter, type AtermFramePainterDeps } from './aterm-frame-painter'
import type { AtermTerminal } from './aterm_wasm.js'

// Dirty-band present policy (audit E3, Codex-modified): the CPU painter may
// blit only the engine's exported damage bands — but ONLY while the canvas is
// overlay-free. Overlays (search/link/prediction) composite onto the same
// canvas, so any overlay active this frame or last forces the full blit
// (the overlay-triggered full-band policy), and a zero-band overlay-free
// frame skips the canvas entirely.

const WIDTH = 8
const HEIGHT = 4
const BANDS_OFFSET = 4096

beforeAll(() => {
  // node env has no ImageData; the painter only constructs and forwards it.
  if (typeof globalThis.ImageData === 'undefined') {
    vi.stubGlobal(
      'ImageData',
      class {
        constructor(
          public data: Uint8ClampedArray,
          public width: number,
          public height: number
        ) {}
      }
    )
  }
})

type Harness = {
  paint: () => void
  ctx: { putImageData: ReturnType<typeof vi.fn>; fillRect: ReturnType<typeof vi.fn> }
  setBands: (bands: [number, number, number, number][]) => void
  setHoveredSpan: (span: { row: number; startCol: number; endCol: number } | null) => void
}

function makeHarness(): Harness {
  const memory = new WebAssembly.Memory({ initial: 1 })
  let bandCount = 0
  const setBands = (bands: [number, number, number, number][]): void => {
    bandCount = bands.length
    const view = new Int32Array(memory.buffer, BANDS_OFFSET, bands.length * 4)
    bands.forEach((b, i) => view.set(b, i * 4))
  }
  let hoveredSpan: { row: number; startCol: number; endCol: number } | null = null
  const term = {
    render: vi.fn(),
    width: WIDTH,
    height: HEIGHT,
    rgba_ptr: () => 0,
    present_band_count: () => bandCount,
    present_bands_ptr: () => BANDS_OFFSET,
    chrome_pad: 0,
    chrome_head: 0,
    cell_width: 2,
    cell_height: 2
  } as unknown as AtermTerminal
  const ctx = {
    putImageData: vi.fn(),
    fillRect: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    translate: vi.fn(),
    fillStyle: ''
  }
  const canvas = { width: 0, height: 0, style: {} } as unknown as HTMLCanvasElement
  const deps: AtermFramePainterDeps = {
    ctx: ctx as unknown as CanvasRenderingContext2D,
    canvas,
    term,
    memory,
    drawScheduler: { isScheduled: () => true, consume: vi.fn() } as never,
    searchController: { hasActiveQuery: () => false, refresh: vi.fn() } as never,
    isDisposed: () => false,
    getDpr: () => 1,
    getRows: () => 2,
    getSearchMatches: () => [],
    getSearchActiveIndex: () => -1,
    takeSearchRefresh: () => false,
    getHoveredLinkSpan: () => hoveredSpan,
    getFgColor: () => 0xffffff,
    getPredictionCells: () => new Uint32Array()
  }
  return {
    paint: createAtermFramePainter(deps),
    ctx,
    setBands,
    setHoveredSpan: (span) => {
      hoveredSpan = span
    }
  }
}

describe('aterm frame painter dirty-band present (E3)', () => {
  it('full-blits on resize, then skips the canvas on zero-band overlay-free frames', () => {
    const h = makeHarness()
    h.setBands([])
    h.paint() // first frame: canvas 0x0 -> resized -> full blit despite zero bands
    expect(h.ctx.putImageData).toHaveBeenCalledTimes(1)
    expect(h.ctx.putImageData.mock.calls[0]).toHaveLength(3)

    h.paint() // steady state, byte-identical frame: no canvas work at all
    expect(h.ctx.putImageData).toHaveBeenCalledTimes(1)
  })

  it('blits exactly the exported bands when overlay-free', () => {
    const h = makeHarness()
    h.paint() // settle the resize frame
    h.ctx.putImageData.mockClear()

    h.setBands([
      [0, 1, WIDTH, 2],
      [0, 3, WIDTH, 1]
    ])
    h.paint()
    expect(h.ctx.putImageData).toHaveBeenCalledTimes(2)
    expect(h.ctx.putImageData.mock.calls[0].slice(3)).toEqual([0, 1, WIDTH, 2])
    expect(h.ctx.putImageData.mock.calls[1].slice(3)).toEqual([0, 3, WIDTH, 1])
  })

  it('an active overlay forces the full blit and paints above it', () => {
    const h = makeHarness()
    h.paint()
    h.ctx.putImageData.mockClear()

    h.setBands([[0, 0, 2, 2]])
    h.setHoveredSpan({ row: 0, startCol: 0, endCol: 3 })
    h.paint()
    // Full-frame putImageData (3-arg form), never a band blit, then the overlay.
    expect(h.ctx.putImageData).toHaveBeenCalledTimes(1)
    expect(h.ctx.putImageData.mock.calls[0]).toHaveLength(3)
    expect(h.ctx.fillRect).toHaveBeenCalled()
  })

  it('the frame after an overlay clears still full-blits to erase its pixels', () => {
    const h = makeHarness()
    h.paint()
    h.setHoveredSpan({ row: 0, startCol: 0, endCol: 3 })
    h.paint()
    h.ctx.putImageData.mockClear()
    h.ctx.fillRect.mockClear()

    h.setHoveredSpan(null)
    h.setBands([]) // engine sees no grid damage — the stale underline is host-side
    h.paint()
    expect(h.ctx.putImageData).toHaveBeenCalledTimes(1)
    expect(h.ctx.putImageData.mock.calls[0]).toHaveLength(3)
    expect(h.ctx.fillRect).not.toHaveBeenCalled()

    h.paint() // overlay-free steady state resumes band/skip behavior
    expect(h.ctx.putImageData).toHaveBeenCalledTimes(1)
  })
})
