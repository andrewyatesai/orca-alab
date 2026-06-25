/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it } from 'vitest'
import { computeGrid, MIN_GRID_COLS, MIN_GRID_ROWS } from './aterm-grid-size'

// computeGrid is the renderer<->DOM measurement seam that lives OUTSIDE the Rust
// engine's verification boundary (dpr + container.clientWidth are not inputs to any
// engine proof). These are the TS-side gates documented in
// rust/PROOF_CARRYING_PERFORMANCE.md that own that band.

const container = (clientWidth: number, clientHeight: number): HTMLElement =>
  ({ clientWidth, clientHeight }) as HTMLElement

describe('computeGrid', () => {
  it('never returns a 0×0 grid for an unlaid-out container (the "zero dimensions" regression)', () => {
    // A pane mounted before layout (hidden/background/pre-mount) reports clientWidth 0.
    // It must fall back to a usable 80×24, never 0×0 — the reflow corrects it once the
    // container has real dimensions. This is the invariant the 0×0 banner violated.
    expect(computeGrid(container(0, 0), 2, 8, 16)).toEqual({ cols: 80, rows: 24 })
    expect(computeGrid(container(0, 600), 2, 8, 16)).toEqual({ cols: 80, rows: 24 })
    expect(computeGrid(container(800, 0), 2, 8, 16)).toEqual({ cols: 80, rows: 24 })
  })

  it('scales cols/rows with devicePixelRatio for the same CSS size + cell metrics', () => {
    // deviceWidth = clientWidth * dpr. With device-px cell metrics held fixed, a 2×
    // dpr yields a 2× grid — proving dpr is honored (a dpr=1 pane on a dpr=2 display
    // would otherwise under-resolve).
    expect(computeGrid(container(800, 600), 1, 8, 16)).toEqual({ cols: 100, rows: 37 })
    expect(computeGrid(container(800, 600), 2, 8, 16)).toEqual({ cols: 200, rows: 75 })
  })

  it('floors to whole cells and clamps to the minimum for a tiny laid-out container', () => {
    expect(computeGrid(container(10, 20), 1, 8, 16)).toEqual({
      cols: MIN_GRID_COLS,
      rows: MIN_GRID_ROWS
    })
  })
})
