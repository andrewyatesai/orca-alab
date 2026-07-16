import { describe, expect, it } from 'vitest'
import {
  chromeCssMargins,
  chromeFrameOrigin,
  chromeOutsideRects,
  chromeStripRects,
  type AtermDeviceRect
} from './aterm-chrome-box'

// aterm-chrome-box is the SINGLE source of the window-chrome box math; any drift
// from what the three drawer sites previously inlined shows up as 1-device-px
// seams at the pane clip line, so the margin parity here is byte-for-byte.

const rect = (x: number, y: number, width: number, height: number): AtermDeviceRect => ({
  x,
  y,
  width,
  height
})

const area = (r: AtermDeviceRect): number => r.width * r.height
const sumArea = (rects: AtermDeviceRect[]): number => rects.reduce((t, r) => t + area(r), 0)
const intersectArea = (a: AtermDeviceRect, b: AtermDeviceRect): number => {
  const w = Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x)
  const h = Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y)
  return w > 0 && h > 0 ? w * h : 0
}
const expectPairwiseDisjoint = (rects: AtermDeviceRect[]): void => {
  for (let i = 0; i < rects.length; i++) {
    for (let j = i + 1; j < rects.length; j++) {
      expect(intersectArea(rects[i], rects[j])).toBe(0)
    }
  }
}

// The dpr matrix from the spill plan's rounding-parity mandate, plus the chrome
// values the engine actually emits (pad ≈13, head ≈34 CSS px × dpr).
const DPRS = [1, 1.25, 1.5, 2]
const CHROMES: [number, number][] = [
  [0, 0],
  [13, 34],
  [13, 0],
  [0, 34],
  [1, 1],
  [7, 33],
  [26, 68]
]

describe('chromeCssMargins', () => {
  it('matches the exact margin strings the three drawer sites previously computed', () => {
    // Verbatim copies of the inline expressions removed from aterm-frame-painter,
    // aterm-gpu-drawer, and aterm-worker-loader (syncPaneCanvasCssBox).
    const legacyMarginLeft = (pad: number, dpr: number): string => `${-(pad / dpr)}px`
    const legacyMarginTop = (pad: number, head: number, dpr: number): string =>
      `${-((pad + head) / dpr)}px`
    for (const dpr of DPRS) {
      for (const [pad, head] of CHROMES) {
        const margins = chromeCssMargins(pad, head, dpr)
        expect(margins.marginLeft).toBe(legacyMarginLeft(pad, dpr))
        expect(margins.marginTop).toBe(legacyMarginTop(pad, head, dpr))
      }
    }
  })

  it('emits the expected literal values for representative dprs', () => {
    expect(chromeCssMargins(13, 34, 1)).toEqual({ marginLeft: '-13px', marginTop: '-47px' })
    expect(chromeCssMargins(13, 34, 2)).toEqual({ marginLeft: '-6.5px', marginTop: '-23.5px' })
    expect(chromeCssMargins(13, 34, 1.25)).toEqual({ marginLeft: '-10.4px', marginTop: '-37.6px' })
  })

  it("zero chrome yields '0px' both ways at every dpr (no '-0px')", () => {
    for (const dpr of DPRS) {
      expect(chromeCssMargins(0, 0, dpr)).toEqual({ marginLeft: '0px', marginTop: '0px' })
    }
  })
})

describe('chromeFrameOrigin', () => {
  it('places the frame pad left and pad+head above the grid box', () => {
    expect(chromeFrameOrigin({ x: 100, y: 80 }, 13, 34)).toEqual({ x: 87, y: 33 })
  })

  it('is the identity at zero chrome', () => {
    expect(chromeFrameOrigin({ x: 100, y: 80 }, 0, 0)).toEqual({ x: 100, y: 80 })
  })
})

describe('chromeStripRects', () => {
  const grid = rect(100, 80, 400, 300)

  it('decomposes the chrome band into 4 disjoint strips covering frame minus grid', () => {
    const strips = chromeStripRects(grid, 13, 34)
    expect(strips).toEqual([
      { x: 87, y: 33, width: 426, height: 47 }, // top, incl. head
      { x: 87, y: 380, width: 426, height: 13 }, // bottom
      { x: 87, y: 80, width: 13, height: 300 }, // left
      { x: 500, y: 80, width: 13, height: 300 } // right
    ])
    expectPairwiseDisjoint(strips)
    // Exact cover: frame area minus grid area, no gaps and no overlaps.
    const frameArea = (400 + 2 * 13) * (300 + 2 * 13 + 34)
    expect(sumArea(strips)).toBe(frameArea - area(grid))
    for (const s of strips) {
      expect(intersectArea(s, grid)).toBe(0)
    }
  })

  it('omits empty strips (head-only chrome leaves just the top strip)', () => {
    expect(chromeStripRects(grid, 0, 34)).toEqual([{ x: 100, y: 46, width: 400, height: 34 }])
  })

  it('returns no strips at zero chrome', () => {
    expect(chromeStripRects(grid, 0, 0)).toEqual([])
  })

  it('handles a zero-size grid: side strips vanish, top/bottom keep 2*pad width', () => {
    const strips = chromeStripRects(rect(200, 150, 0, 0), 13, 34)
    expect(strips).toEqual([
      { x: 187, y: 103, width: 26, height: 47 },
      { x: 187, y: 150, width: 26, height: 13 }
    ])
  })

  it('clamps a negative-size grid box like a zero-size one', () => {
    expect(chromeStripRects(rect(200, 150, -5, -7), 13, 34)).toEqual(
      chromeStripRects(rect(200, 150, 0, 0), 13, 34)
    )
  })
})

describe('chromeOutsideRects', () => {
  const grid = rect(100, 80, 400, 300)
  const pad = 13
  const head = 34

  /** Asserts the decomposition invariants and returns the sub-rects: disjoint,
   *  clip-free, inside the strip band, and area-conserving (outside + clip∩strips
   *  == strips — nothing lost, nothing double-counted). */
  const expectValidDecomposition = (
    gridBox: AtermDeviceRect,
    clip: AtermDeviceRect
  ): AtermDeviceRect[] => {
    const strips = chromeStripRects(gridBox, pad, head)
    const outside = chromeOutsideRects(gridBox, pad, head, clip)
    expectPairwiseDisjoint(outside)
    for (const r of outside) {
      expect(area(r)).toBeGreaterThan(0)
      expect(intersectArea(r, clip)).toBe(0)
      expect(strips.reduce((t, s) => t + intersectArea(r, s), 0)).toBe(area(r))
    }
    const clippedArea = strips.reduce((t, s) => t + intersectArea(s, clip), 0)
    expect(sumArea(outside) + clippedArea).toBe(sumArea(strips))
    return outside
  }

  it('yields ≤8 sub-rects for the pane geometry (clip containing the grid box)', () => {
    // The real shape: the pane's visible box contains the grid plus a few px of
    // in-pane chrome ring on each side.
    const clip = rect(96, 76, 408, 308)
    const outside = expectValidDecomposition(grid, clip)
    expect(outside.length).toBeLessThanOrEqual(8)
    expect(outside.length).toBeGreaterThan(0)
  })

  it('returns the strips unchanged when the clip is disjoint from the band', () => {
    const clip = rect(1000, 1000, 50, 50)
    expect(chromeOutsideRects(grid, pad, head, clip)).toEqual(chromeStripRects(grid, pad, head))
    expectValidDecomposition(grid, clip)
  })

  it('returns nothing when the clip covers the whole frame', () => {
    expect(chromeOutsideRects(grid, pad, head, rect(0, 0, 1000, 1000))).toEqual([])
    expectValidDecomposition(grid, rect(0, 0, 1000, 1000))
  })

  it('splits a strip crossed through its middle into both remainders', () => {
    // A vertical band through the frame: top/bottom strips split left+right,
    // side strips (outside the band's x-range) survive whole.
    const clip = rect(250, 0, 60, 500)
    const outside = expectValidDecomposition(grid, clip)
    expect(outside).toContainEqual({ x: 87, y: 33, width: 163, height: 47 })
    expect(outside).toContainEqual({ x: 310, y: 33, width: 203, height: 47 })
    expect(outside).toContainEqual({ x: 87, y: 80, width: 13, height: 300 })
    expect(outside).toContainEqual({ x: 500, y: 80, width: 13, height: 300 })
  })

  it('conserves area for a clip overlapping one corner of the band', () => {
    expectValidDecomposition(grid, rect(87, 33, 100, 100))
  })

  it('handles zero chrome (no strips, no sub-rects) and a zero-size grid', () => {
    expect(chromeOutsideRects(grid, 0, 0, rect(0, 0, 1000, 1000))).toEqual([])
    expectValidDecomposition(rect(200, 150, 0, 0), rect(190, 140, 40, 40))
  })
})
