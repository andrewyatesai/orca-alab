import {
  ATERM_RENDERER_FONT_PX,
  type AtermPaneControllerOptions
} from './aterm-pane-controller-types'
import { buildDefaultTerminalOptions } from '../pane-terminal-options'
import { readAtermEffectsConfig, type AtermEffectsConfig } from './aterm-effects-settings'
import type { AtermPredictionEchoMode } from './aterm-prediction-echo'
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
  /** DEFAULT-bg alpha (terminalBackgroundOpacity), clamped 0..1; default 1 (opaque). */
  getBackgroundOpacity: () => number
  /** Cursor-fill alpha (terminalCursorOpacity), clamped 0..1; default 1 (opaque). */
  getCursorOpacity: () => number
  /** Kitty keyboard capability (per-pane static policy: local Windows ConPTY panes
   *  disable it — see terminal-keyboard-protocol); default true (engine default ON). */
  getKittyKeyboardEnabled: () => boolean
  /** Live effects config (sparkle words / cursor glow / reduced motion); defaults
   *  keep every effect OFF, matching the engine's byte-identical default. */
  getEffectsConfig: () => AtermEffectsConfig
  /** Predictive-echo display mode (terminalPredictiveEcho); default 'adaptive'. */
  getPredictiveEcho: () => AtermPredictionEchoMode
}

/** Clamp a stored opacity setting to the engine's 0..=1 domain; anything unset or
 *  non-finite means "opaque" (1, the engine default → a no-op set). */
export function normalizeTerminalOpacity(value: number | undefined): number {
  return typeof value === 'number' && Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 1
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
    getWordSeparators: () => useAppStore.getState().settings?.terminalWordSeparator || null,
    // Read the store live (like word separators) so a slider change re-applies to
    // open panes via reapplyEngineSettings without a pane rebuild.
    getBackgroundOpacity: () =>
      normalizeTerminalOpacity(useAppStore.getState().settings?.terminalBackgroundOpacity),
    getCursorOpacity: () =>
      normalizeTerminalOpacity(useAppStore.getState().settings?.terminalCursorOpacity),
    getKittyKeyboardEnabled: () => options?.getKittyKeyboardEnabled?.() ?? true,
    // Read the store live (like word separators) so effect toggles re-apply to
    // open panes via reapplyEngineSettings without a pane rebuild.
    getEffectsConfig: readAtermEffectsConfig,
    // Read live (like the effects config) so a mode change re-applies to open
    // panes via reapplyEngineSettings; default 'adaptive' (the self-gating mode).
    getPredictiveEcho: () => useAppStore.getState().settings?.terminalPredictiveEcho ?? 'adaptive'
  }
}
