// One grapheme segmenter for the whole module — constructing an ICU Segmenter per call is
// a measurable allocation on the hover / selection / buffer-read path. Extracted from the
// worker-backed term to keep it under the line cap.
const GRAPHEME_SEGMENTER =
  typeof Intl !== 'undefined' && 'Segmenter' in Intl ? new Intl.Segmenter() : null

/** Reconstruct per-column graphemes from a row's text + width digits ('2' = wide lead,
 *  '1' = normal; the trailing spacer column stays empty) so cell_text is served from the
 *  snapshot. Best-effort segmentation — matches the prior 1:1 char→column assumption when
 *  Intl.Segmenter is unavailable. */
export function buildAtermRowCells(text: string, widths: string, cols: number): string[] {
  const graphemes = GRAPHEME_SEGMENTER
    ? Array.from(GRAPHEME_SEGMENTER.segment(text), (s) => s.segment)
    : Array.from(text)
  const cells: string[] = Array.from({ length: cols }, () => '')
  let col = 0
  for (const g of graphemes) {
    if (col >= cols) {
      break
    }
    cells[col] = g
    // Advance by the cell's width (2 = wide lead + spacer); default 1.
    col += widths[col] === '2' ? 2 : 1
  }
  return cells
}
