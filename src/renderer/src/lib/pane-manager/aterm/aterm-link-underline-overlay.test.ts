import { describe, it, expect, vi } from 'vitest'
import { paintAtermLinkUnderline } from './aterm-link-underline-overlay'

// A minimal CanvasRenderingContext2D stub recording fillRect calls + the fillStyle
// in effect at each call — enough to assert WHERE and WHAT the underline paints.
function fakeCtx(): {
  ctx: CanvasRenderingContext2D
  rects: { x: number; y: number; w: number; h: number; style: string }[]
} {
  const rects: { x: number; y: number; w: number; h: number; style: string }[] = []
  let fillStyle = ''
  const ctx = {
    get fillStyle() {
      return fillStyle
    },
    set fillStyle(v: string) {
      fillStyle = v
    },
    fillRect: vi.fn((x: number, y: number, w: number, h: number) => {
      rects.push({ x, y, w, h, style: fillStyle })
    })
  } as unknown as CanvasRenderingContext2D
  return { ctx, rects }
}

const GEOM = { cellWidth: 8, cellHeight: 16, dpr: 1 }

describe('paintAtermLinkUnderline', () => {
  it('paints a thin rule across the hovered span at the cell bottom', () => {
    const { ctx, rects } = fakeCtx()
    paintAtermLinkUnderline(ctx, { row: 2, startCol: 3, endCol: 7 }, 0x3399ff, GEOM)
    expect(rects).toHaveLength(1)
    const r = rects[0]
    // x = startCol*cellWidth, width = (endCol-startCol)*cellWidth.
    expect(r.x).toBe(3 * 8)
    expect(r.w).toBe(4 * 8)
    // 1px thick at dpr=1, sat on the bottom edge of row 2 ((row+1)*cellHeight - 1).
    expect(r.h).toBe(1)
    expect(r.y).toBe(3 * 16 - 1)
    // The rule uses the theme fg color.
    expect(r.style).toBe('rgb(51, 153, 255)')
  })

  it('uses a 2px rule on HiDPI', () => {
    const { ctx, rects } = fakeCtx()
    paintAtermLinkUnderline(ctx, { row: 0, startCol: 0, endCol: 2 }, 0xffffff, {
      ...GEOM,
      dpr: 2
    })
    expect(rects[0].h).toBe(2)
  })

  it('no-ops when nothing is hovered (so a cleared hover leaves no underline)', () => {
    const { ctx, rects } = fakeCtx()
    paintAtermLinkUnderline(ctx, null, 0xffffff, GEOM)
    expect(rects).toHaveLength(0)
  })

  it('no-ops on a degenerate (empty) span', () => {
    const { ctx, rects } = fakeCtx()
    paintAtermLinkUnderline(ctx, { row: 1, startCol: 5, endCol: 5 }, 0xffffff, GEOM)
    expect(rects).toHaveLength(0)
  })
})
