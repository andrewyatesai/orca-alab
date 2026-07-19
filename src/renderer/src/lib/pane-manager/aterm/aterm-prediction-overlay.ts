export type AtermPredictionOverlayGeometry = {
  cellWidth: number
  cellHeight: number
  dpr: number
  /** Theme fg (0x00RRGGBB) — the ghost glyph + underline colour. */
  fgColor: number
}

// The ghost is drawn dim so it reads as speculative, not committed. 0.55 matches
// the legibility posture of the search-highlight overlay (a translucent mark the
// real content shows through) so predictions look native to the app's overlays.
const PREDICTION_GLYPH_ALPHA = 0.55

/** Paint the speculative predictive-echo ghosts for THIS frame. `cells` is the
 *  engine's flat `[row, col, codepoint]` triple stream from `predict_overlay()`
 *  (no style bits — the host chooses the tentative styling). Called AFTER the
 *  grid + search/link overlays so the ghost sits above the glyphs, on the SAME 2d
 *  context (CPU: grid canvas; GPU: the stacked overlay). Rendered dim + underlined
 *  (the mosh convention) so it reads as unconfirmed; `predict_reconcile()` removes
 *  it once the real echo lands (~1 RTT) and it NEVER mutates the real grid. */
export function paintAtermPredictionOverlay(
  ctx: CanvasRenderingContext2D,
  cells: Uint32Array,
  geometry: AtermPredictionOverlayGeometry
): void {
  // Triples: anything shorter carries no complete cell.
  if (cells.length < 3) {
    return
  }
  const { cellWidth, cellHeight, dpr, fgColor } = geometry
  const r = (fgColor >> 16) & 0xff
  const g = (fgColor >> 8) & 0xff
  const b = fgColor & 0xff
  ctx.save()
  // Cell-sized monospace, reusing the IME preedit view's proven sizing so ghost
  // glyphs align with the engine's rasterized cells without threading the font face
  // into the painter (predictions are single-width ASCII/space by engine contract).
  ctx.font = `${Math.max(1, Math.round(cellHeight * 0.75))}px monospace`
  ctx.textBaseline = 'middle'
  const underline = Math.max(1, Math.round(dpr))
  ctx.fillStyle = `rgba(${r}, ${g}, ${b}, ${PREDICTION_GLYPH_ALPHA})`
  for (let i = 0; i + 2 < cells.length; i += 3) {
    const x = cells[i + 1] * cellWidth
    const y = cells[i] * cellHeight
    // codepoint is a Rust `char` widened to u32 — always a valid Unicode scalar.
    ctx.fillText(String.fromCodePoint(cells[i + 2]), x, y + cellHeight / 2)
    ctx.fillRect(x, y + cellHeight - underline, cellWidth, underline)
  }
  ctx.restore()
}
