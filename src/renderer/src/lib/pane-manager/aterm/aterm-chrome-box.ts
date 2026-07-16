// Single source of the window-chrome box-model math. The engine grows the frame
// AROUND the grid (pad on all sides, head above), and every consumer — the three
// drawers' negative-margin pinning, and next the spill geometry tracker — must
// derive identical numbers or 1-device-px seams appear at the clip line. Any new
// chrome geometry derivation belongs here, never inlined at a call site.

/** An axis-aligned rect in device px (shape matches the worker-protocol rects). */
export type AtermDeviceRect = { x: number; y: number; width: number; height: number }

/** The CSS margin pair that pins a chrome-padded canvas so the grid stays put. */
export type AtermChromeCssMargins = { marginLeft: string; marginTop: string }

/** CSS margins pulling the chrome-padded canvas up-left by the grid's in-frame
 *  offset — the grid stays put and only the chrome overhangs. Zero chrome yields
 *  '0px' both ways, so writing them unconditionally restores a chrome-off box. */
export function chromeCssMargins(
  chromePadPx: number,
  chromeHeadPx: number,
  dpr: number
): AtermChromeCssMargins {
  return {
    marginLeft: `${-(chromePadPx / dpr)}px`,
    marginTop: `${-((chromePadPx + chromeHeadPx) / dpr)}px`
  }
}

/** Device-px origin of the chrome-padded frame: the grid sits at (pad, pad+head)
 *  inside the frame, so the frame starts that far up-left of the grid box. */
export function chromeFrameOrigin(
  gridBox: { x: number; y: number },
  chromePadPx: number,
  chromeHeadPx: number
): { x: number; y: number } {
  return { x: gridBox.x - chromePadPx, y: gridBox.y - chromePadPx - chromeHeadPx }
}

/** The chrome band (frame minus grid) as ≤4 disjoint strips in the grid box's
 *  coordinate space: top (incl. head, full frame width), bottom (full width),
 *  left, right (grid height). Empty strips are omitted — zero chrome yields []. */
export function chromeStripRects(
  gridBox: AtermDeviceRect,
  chromePadPx: number,
  chromeHeadPx: number
): AtermDeviceRect[] {
  // Defensive clamp: a degenerate (unmeasured) grid box must not emit negative
  // extents that would corrupt downstream area/dirty-rect math.
  const gridW = Math.max(0, gridBox.width)
  const gridH = Math.max(0, gridBox.height)
  const origin = chromeFrameOrigin(gridBox, chromePadPx, chromeHeadPx)
  const frameW = gridW + 2 * chromePadPx
  const strips: AtermDeviceRect[] = [
    { x: origin.x, y: origin.y, width: frameW, height: chromePadPx + chromeHeadPx },
    { x: origin.x, y: gridBox.y + gridH, width: frameW, height: chromePadPx },
    { x: origin.x, y: gridBox.y, width: chromePadPx, height: gridH },
    { x: gridBox.x + gridW, y: gridBox.y, width: chromePadPx, height: gridH }
  ]
  return strips.filter((r) => r.width > 0 && r.height > 0)
}

/** The chrome strips MINUS a clip rect (the pane's own visible box), as disjoint
 *  sub-rects. In-clip chrome pixels stay single-sourced from the pane canvas, so
 *  a window-space compositor draws ONLY these. When the clip contains the grid
 *  box (the pane geometry) this is ≤8 rects: ≤3 each from top/bottom, ≤1 each
 *  from left/right. */
export function chromeOutsideRects(
  gridBox: AtermDeviceRect,
  chromePadPx: number,
  chromeHeadPx: number,
  clip: AtermDeviceRect
): AtermDeviceRect[] {
  const out: AtermDeviceRect[] = []
  for (const strip of chromeStripRects(gridBox, chromePadPx, chromeHeadPx)) {
    subtractRectInto(strip, clip, out)
  }
  return out
}

/** rect minus clip → disjoint bands pushed onto `out`: full-width above/below the
 *  overlap, then left/right pinched to the overlap's vertical span (≤4 pieces). */
function subtractRectInto(
  rect: AtermDeviceRect,
  clip: AtermDeviceRect,
  out: AtermDeviceRect[]
): void {
  const rectRight = rect.x + rect.width
  const rectBottom = rect.y + rect.height
  const overlapLeft = Math.max(rect.x, clip.x)
  const overlapTop = Math.max(rect.y, clip.y)
  const overlapRight = Math.min(rectRight, clip.x + clip.width)
  const overlapBottom = Math.min(rectBottom, clip.y + clip.height)
  if (overlapLeft >= overlapRight || overlapTop >= overlapBottom) {
    out.push(rect)
    return
  }
  if (overlapTop > rect.y) {
    out.push({ x: rect.x, y: rect.y, width: rect.width, height: overlapTop - rect.y })
  }
  if (overlapBottom < rectBottom) {
    out.push({ x: rect.x, y: overlapBottom, width: rect.width, height: rectBottom - overlapBottom })
  }
  if (overlapLeft > rect.x) {
    out.push({
      x: rect.x,
      y: overlapTop,
      width: overlapLeft - rect.x,
      height: overlapBottom - overlapTop
    })
  }
  if (overlapRight < rectRight) {
    out.push({
      x: overlapRight,
      y: overlapTop,
      width: rectRight - overlapRight,
      height: overlapBottom - overlapTop
    })
  }
}
