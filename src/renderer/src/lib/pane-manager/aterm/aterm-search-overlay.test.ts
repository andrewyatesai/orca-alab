// P6: the overlay paint binary-searches the line-sorted match list for the visible
// band instead of scanning ALL matches per frame — output must stay byte-identical
// to the old linear scan (same rects, same active tone by GLOBAL match index).

import { describe, expect, it, vi } from 'vitest'
import {
  paintAtermSearchHighlights,
  SEARCH_ACTIVE_FILL,
  SEARCH_MATCH_FILL
} from './aterm-search-overlay'
import type { AtermSearchMatch } from './aterm-search'
import type { AtermTerminal } from './aterm_wasm.js'

function makeCtx(): {
  ctx: CanvasRenderingContext2D
  rects: { x: number; y: number; w: number; h: number; fill: string }[]
} {
  const rects: { x: number; y: number; w: number; h: number; fill: string }[] = []
  const ctx = {
    fillStyle: '',
    fillRect: (x: number, y: number, w: number, h: number) => {
      rects.push({ x, y, w, h, fill: String(ctx.fillStyle) })
    }
  }
  return { ctx: ctx as unknown as CanvasRenderingContext2D, rects }
}

const geometry = (origin: number, offset: number, rows: number) => ({
  term: { search_display_origin: origin, display_offset: offset } as unknown as AtermTerminal,
  cellWidth: 10,
  cellHeight: 20,
  rows
})

const match = (line: number, startCol = 0, length = 3): AtermSearchMatch => ({
  line,
  startCol,
  length
})

describe('paintAtermSearchHighlights visible-band probe', () => {
  it('paints only the on-screen band of a sorted match list', () => {
    const { ctx, rects } = makeCtx()
    // Viewport shows absolute lines [100, 124) (origin 100, offset 0, 24 rows).
    const matches = [match(2), match(99), match(100), match(110), match(123), match(124)]
    paintAtermSearchHighlights(ctx, matches, -1, geometry(100, 0, 24))

    expect(rects.map((r) => r.y / 20)).toEqual([0, 10, 23]) // display rows of 100/110/123
    expect(rects.every((r) => r.fill === SEARCH_MATCH_FILL)).toBe(true)
  })

  it('keeps the active tone keyed to the GLOBAL match index', () => {
    const { ctx, rects } = makeCtx()
    const matches = [match(2), match(100), match(110)]
    // Active is global index 2 (line 110) — on screen after two off/on-screen others.
    paintAtermSearchHighlights(ctx, matches, 2, geometry(100, 0, 24))

    expect(rects).toHaveLength(2)
    expect(rects[0].fill).toBe(SEARCH_MATCH_FILL)
    expect(rects[1].fill).toBe(SEARCH_ACTIVE_FILL)
  })

  it('paints nothing when every match is off screen (and never scans them)', () => {
    const { ctx, rects } = makeCtx()
    paintAtermSearchHighlights(ctx, [match(2), match(3)], 0, geometry(100, 0, 24))
    expect(rects).toEqual([])
  })

  it('tracks the viewport as display_offset scrolls the band', () => {
    const { ctx, rects } = makeCtx()
    // Scrolled up by 50: visible lines are [50, 74).
    paintAtermSearchHighlights(ctx, [match(49), match(50), match(73), match(74)], -1, {
      ...geometry(100, 50, 24)
    })
    expect(rects.map((r) => r.y / 20)).toEqual([0, 23])
  })

  it('never reads matches outside the band (spy proof for huge lists)', () => {
    const { ctx } = makeCtx()
    const matches: AtermSearchMatch[] = []
    for (let i = 0; i < 1000; i++) {
      matches.push(match(i))
    }
    const reads = vi.fn()
    const spied = new Proxy(matches, {
      get(target, prop) {
        if (typeof prop === 'string' && /^\d+$/.test(prop)) {
          reads(Number(prop))
        }
        return Reflect.get(target, prop)
      }
    })
    // Viewport [500, 524): every indexed read must stay within the probe's bisection
    // path or the visible band — far fewer than the 1000-element scan it replaced.
    paintAtermSearchHighlights(ctx, spied, -1, geometry(500, 0, 24))
    expect(reads.mock.calls.length).toBeLessThan(24 + 2 * Math.ceil(Math.log2(1001)) + 4)
  })
})
