import {
  ATERM_RENDERER_FONT_PX,
  type AtermPaneControllerOptions
} from './aterm-pane-controller-types'
import { buildDefaultTerminalOptions } from '../pane-terminal-options'
import { useAppStore } from '@/store'

/** The live settings readers the wiring uses (font size / line-height / family /
 *  ligatures). Each falls back to the engine default when its callback is unset, so a
 *  pane built without controller options still renders correctly. Grouped here to keep
 *  the wiring focused. */
export type AtermControllerOptionReaders = {
  /** Base CSS cell font size (terminalFontSize); default ATERM_RENDERER_FONT_PX. */
  getFontPx: () => number
  /** Cell line-height multiplier (terminalLineHeight); default 1 (engine default). */
  getLineHeight: () => number
  /** Primary font family (terminalFontFamily); undefined keeps the bundled face. */
  getFontFamily: () => string | undefined
  /** Numeric font weight (terminalFontWeight); undefined → the shared default (500). */
  getFontWeight: () => number | undefined
  /** Ligatures enabled (resolved terminalLigatures); default true (engine default ON). */
  getLigatures: () => boolean
  /** Scrollback history line limit; default 100_000 (the engine default → a no-op set). */
  getScrollbackLines: () => number
  /** DEFAULT cursor style as a DECSCUSR param (1–6); default 1 (engine default → no-op). */
  getCursorStyleParam: () => number
  /** Per-cell WCAG contrast floor (xterm's minimumContrastRatio); <= 1 disables it. */
  getMinimumContrastRatio: () => number
  /** Double-click word separators (terminalWordSeparator); null = engine default. */
  getWordSeparators: () => string | null
}

export function createAtermControllerOptionReaders(
  options: AtermPaneControllerOptions | undefined
): AtermControllerOptionReaders {
  return {
    getFontPx: () => options?.getFontPx?.() ?? ATERM_RENDERER_FONT_PX,
    getLineHeight: () => options?.getLineHeight?.() ?? 1,
    getFontFamily: () => options?.getFontFamily?.(),
    getFontWeight: () => options?.getFontWeight?.(),
    getLigatures: () => options?.getLigatures?.() ?? true,
    // Default to the engine's own defaults so an unset callback makes the apply a no-op
    // (100_000-line scrollback; DECSCUSR 1 = blinking block).
    getScrollbackLines: () => options?.getScrollbackLines?.() ?? 100_000,
    getCursorStyleParam: () => options?.getCursorStyleParam?.() ?? 1,
    // Not user-tunable, so read the canonical facade default (4.5) straight from the
    // options builder instead of threading a per-pane callback; 1 = floor off.
    getMinimumContrastRatio: () => buildDefaultTerminalOptions().minimumContrastRatio ?? 1,
    // Unset/empty terminalWordSeparator meant "xterm's built-in default" on the old
    // facade option (wordSeparator) — map it to null so the engine restores its own
    // default word logic instead of treating "" as "nothing separates".
    getWordSeparators: () => useAppStore.getState().settings?.terminalWordSeparator || null
  }
}
