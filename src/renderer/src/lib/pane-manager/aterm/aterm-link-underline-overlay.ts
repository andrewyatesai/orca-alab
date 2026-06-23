/** The display-row span of the link the pointer is currently hovering, in cells.
 *  `endCol` is EXCLUSIVE (matches LinkHit.end_col), so the span covers the cells
 *  `[startCol, endCol)` on `row`. Null when the pointer isn't over a link. */
export type AtermHoveredLinkSpan = {
  row: number
  startCol: number
  endCol: number
}

export type AtermLinkUnderlineGeometry = {
  cellWidth: number
  cellHeight: number
  dpr: number
}

/** Paint a 1-2px underline across the hovered link's cells. Called AFTER the grid
 *  (CPU: same 2d context as search; GPU: the stacked 2d overlay) so it sits above
 *  the glyphs. The line color is the theme fg (0x00RRGGBB) — the same affordance
 *  xterm/iTerm draw on a hovered hyperlink. No-op when nothing is hovered, so a
 *  cleared hover simply paints nothing on the next frame (no stuck underline). */
export function paintAtermLinkUnderline(
  ctx: CanvasRenderingContext2D,
  span: AtermHoveredLinkSpan | null,
  fgColor: number,
  geometry: AtermLinkUnderlineGeometry
): void {
  if (!span || span.endCol <= span.startCol) {
    return
  }
  const { cellWidth, cellHeight, dpr } = geometry
  // 2px on HiDPI, 1px otherwise — stays a hairline rule, not a bar.
  const thickness = Math.max(1, Math.round(dpr))
  const x = span.startCol * cellWidth
  const width = (span.endCol - span.startCol) * cellWidth
  // Sit the rule on the cell's baseline gutter (just inside the bottom edge).
  const y = (span.row + 1) * cellHeight - thickness
  const r = (fgColor >> 16) & 0xff
  const g = (fgColor >> 8) & 0xff
  const b = fgColor & 0xff
  ctx.fillStyle = `rgb(${r}, ${g}, ${b})`
  ctx.fillRect(x, y, width, thickness)
}
