/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { attachAtermGridReflow, type AtermMetrics } from './aterm-grid-reflow'

// These specs lock in the fix for blurry Retina text: an aterm pane built before the
// window settles onto its Retina backing store is rasterized at dpr=1, and the glyph
// atlas is then upscaled 2× to a dpr=2 panel ("looks like shit"). The draw loop calls
// reconcileIfNeeded() every frame; it must re-invoke set_px at the LIVE dpr (the
// engine re-rasterizes on a px change) so glyphs reach full resolution. The whole e2e
// suite ran at dpr=1, so this path was previously never asserted.

/** Fake engine: cell metrics scale with the rasterization px, like the real one. */
function makeTerm(): {
  set_px: (px: number) => void
  set_line_height: (scale: number) => void
  readonly cell_width: number
  readonly cell_height: number
  setPxCalls: number[]
} {
  let px = 14
  let lineHeight = 1
  const setPxCalls: number[] = []
  return {
    set_px: (next: number) => {
      px = next
      setPxCalls.push(next)
    },
    set_line_height: (scale: number) => {
      lineHeight = scale
    },
    get cell_width() {
      return px * 0.6
    },
    get cell_height() {
      return px * 1.2 * lineHeight
    },
    setPxCalls
  }
}

const container = (clientWidth: number, clientHeight: number): HTMLElement =>
  ({ clientWidth, clientHeight }) as unknown as HTMLElement

const setDpr = (value: number): void => {
  Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value })
}

beforeEach(() => {
  // happy-dom lacks ResizeObserver; matchMedia must be a function returning an
  // event-target-like object for the DPR tracker to arm without throwing.
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
  )
  vi.stubGlobal('matchMedia', () => ({
    matches: false,
    addEventListener(): void {},
    removeEventListener(): void {}
  }))
})

afterEach(() => {
  vi.unstubAllGlobals()
  setDpr(1)
})

function attach(
  term: ReturnType<typeof makeTerm>,
  metrics: AtermMetrics,
  getFontPx: () => number
): ReturnType<typeof attachAtermGridReflow> {
  let grid = { cols: 80, rows: 24 }
  return attachAtermGridReflow({
    term,
    container: container(800, 600),
    metrics,
    getFontPx,
    getLineHeight: () => 1,
    getGrid: () => grid,
    setGrid: (cols, rows) => {
      grid = { cols, rows }
    },
    isDisposed: () => false,
    syncDependents: () => {},
    scheduleDraw: () => {}
  })
}

describe('attachAtermGridReflow.reconcileIfNeeded', () => {
  it('re-rasterizes to the live dpr when a pane built at dpr=1 lands on a dpr=2 display', () => {
    setDpr(1)
    const term = makeTerm()
    const metrics: AtermMetrics = {
      dpr: 1,
      cellWidth: term.cell_width,
      cellHeight: term.cell_height
    }
    const reflow = attach(term, metrics, () => 14)

    // Window settles to Retina; the next frame's reconcile must catch it.
    setDpr(2)
    const reconciled = reflow.reconcileIfNeeded()

    expect(reconciled).toBe(true) // signals the draw loop to skip the same-turn GPU present
    expect(term.setPxCalls).toContain(28) // round(14 * 2) — full Retina resolution
    expect(metrics.dpr).toBe(2)
    reflow.dispose()
  })

  it('does no work and returns false when the live dpr and font size already match', () => {
    setDpr(2)
    const term = makeTerm()
    const metrics: AtermMetrics = {
      dpr: 2,
      cellWidth: term.cell_width,
      cellHeight: term.cell_height
    }
    const reflow = attach(term, metrics, () => 14)

    const reconciled = reflow.reconcileIfNeeded()

    expect(reconciled).toBe(false) // steady state: draw loop presents normally
    expect(term.setPxCalls).toEqual([]) // cheap guard: no re-rasterize, no layout
    reflow.dispose()
  })

  it('re-rasterizes when the user changes terminalFontSize, with no pane rebuild', () => {
    setDpr(2)
    const term = makeTerm()
    let fontPx = 14
    const metrics: AtermMetrics = {
      dpr: 2,
      cellWidth: term.cell_width,
      cellHeight: term.cell_height
    }
    const reflow = attach(term, metrics, () => fontPx)

    fontPx = 18 // user bumps the size in settings
    reflow.reconcileIfNeeded()

    expect(term.setPxCalls).toContain(36) // round(18 * 2)
    reflow.dispose()
  })
})
