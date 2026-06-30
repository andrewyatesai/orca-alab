import {
  ATERM_RENDERER_FONT_PX,
  type AtermPaneControllerOptions
} from './aterm-pane-controller-types'

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
  /** Ligatures enabled (resolved terminalLigatures); default true (engine default ON). */
  getLigatures: () => boolean
}

export function createAtermControllerOptionReaders(
  options: AtermPaneControllerOptions | undefined
): AtermControllerOptionReaders {
  return {
    getFontPx: () => options?.getFontPx?.() ?? ATERM_RENDERER_FONT_PX,
    getLineHeight: () => options?.getLineHeight?.() ?? 1,
    getFontFamily: () => options?.getFontFamily?.(),
    getLigatures: () => options?.getLigatures?.() ?? true
  }
}
