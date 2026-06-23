import type { AtermTerminal } from './aterm_wasm.js'
import type { AtermSearchMatch } from './aterm-search'

// Search-highlight overlay tones — reuse the EXACT yellow/orange the xterm search
// path uses (TerminalSearch.tsx decorations) so aterm search looks identical; the
// styleguide reserves its tokens for chrome, not hosted-tool find highlights.
// Translucent so the glyph underneath stays legible through the rect.
const SEARCH_MATCH_FILL = 'rgba(92, 74, 0, 0.55)' // #5c4a00 @ ~55%
const SEARCH_ACTIVE_FILL = 'rgba(196, 88, 14, 0.6)' // #c4580e @ ~60%

export type AtermSearchOverlayGeometry = {
  term: AtermTerminal
  cellWidth: number
  cellHeight: number
  rows: number
}

/** Device-pixel rect of one match's highlight band using the SAME absolute-row →
 *  display-row mapping paintAtermSearchHighlights uses, or null when the match is
 *  scrolled off-screen. Lets callers locate the painted highlight (e.g. tests). */
export function atermSearchMatchRect(
  match: AtermSearchMatch,
  geometry: AtermSearchOverlayGeometry
): { x: number; y: number; width: number; height: number } | null {
  const { term, cellWidth, cellHeight, rows } = geometry
  const displayRow = match.line - term.search_display_origin + term.display_offset
  if (displayRow < 0 || displayRow >= rows) {
    return null
  }
  return {
    x: match.startCol * cellWidth,
    y: displayRow * cellHeight,
    width: match.length * cellWidth,
    height: cellHeight
  }
}

/** Paint a translucent rect over each visible search match. Called AFTER the
 *  glyph framebuffer is blitted so the text reads through the highlight; the
 *  active match gets a stronger tone. Absolute match lines map to display rows
 *  via the engine's `search_display_origin` + current `display_offset`, so
 *  highlights track the viewport as it scrolls; off-screen matches are skipped. */
export function paintAtermSearchHighlights(
  ctx: CanvasRenderingContext2D,
  matches: AtermSearchMatch[],
  activeIndex: number,
  geometry: AtermSearchOverlayGeometry
): void {
  if (matches.length === 0) {
    return
  }
  const { term, cellWidth, cellHeight, rows } = geometry
  const origin = term.search_display_origin
  const offset = term.display_offset
  for (let i = 0; i < matches.length; i++) {
    const m = matches[i]
    const displayRow = m.line - origin + offset
    if (displayRow < 0 || displayRow >= rows) {
      continue
    }
    ctx.fillStyle = i === activeIndex ? SEARCH_ACTIVE_FILL : SEARCH_MATCH_FILL
    ctx.fillRect(m.startCol * cellWidth, displayRow * cellHeight, m.length * cellWidth, cellHeight)
  }
}
