import { attachAtermColorSchemeSync } from './aterm-color-scheme-sync'
import {
  applyAtermCursorGlowConfig,
  applyAtermEffectsConfig,
  type AtermEffectsTarget
} from './aterm-effects-settings'
import type { AtermControllerOptionReaders } from './aterm-controller-option-readers'

// Apply the user's terminal settings (ligatures, scrollback depth, default cursor shape,
// minimum contrast, word separators, background/cursor opacity) plus the per-pane kitty
// keyboard policy to the engine + wire the live OS color-scheme sync. The readers read the store live, so
// `reapply()` re-applies them on a settings change to an OPEN pane (the setters are cheap +
// don't change cell metrics — parity with how theme/size live-apply). Works on both render
// paths (the worker-backed term posts each as a command). Kept out of the wiring to keep
// it focused.

type EngineSettingsTarget = AtermEffectsTarget & {
  set_ligatures: (on: boolean) => void
  set_scrollback_limit: (lines: number) => void
  set_default_cursor_style: (param: number) => void
  set_minimum_contrast: (ratio: number) => void
  set_word_separators: (separators?: string | null) => void
  set_background_opacity: (opacity: number) => void
  set_cursor_opacity: (opacity: number) => void
  set_kitty_keyboard_enabled: (enabled: boolean) => void
  set_color_scheme: (dark: boolean) => void
  take_response: () => Uint8Array | undefined
  /** Live OSC 12 cursor colour (0x00RRGGBB), undefined while unset / after OSC 112. */
  readonly cursor_color: number | undefined
}

export function applyAtermEngineSettings(deps: {
  term: EngineSettingsTarget
  /** The live settings readers; ligatures/scrollback/cursor/contrast/word-separators
   *  are consumed here. */
  readers: AtermControllerOptionReaders
  inputSink: (data: string) => void
  isDisposed: () => boolean
  /** Schedule a repaint after a live re-apply (the cursor-style change needs one). */
  scheduleDraw: () => void
  /** Restart the focused pane's blink timer on reapply: terminalCursorBlink is
   *  otherwise only read on focus events, so a live toggle would skip the focused
   *  pane until the next blur/focus. */
  refreshCursorBlink: () => void
  /** Apply the live predictive-echo mode to the pane's prediction controller (it
   *  owns the engine `set_predictive_echo` call + the glitch-expiry timer, so 'off'
   *  disarms cleanly). Runs on init + every reapply, so a live toggle takes hold. */
  setPredictiveEcho: (mode: ReturnType<AtermControllerOptionReaders['getPredictiveEcho']>) => void
}): { dispose: () => void; reapply: () => void; syncCursorColor: () => void } {
  const { term, readers } = deps
  // The cursor colour last folded into the glow config, so the per-chunk follow
  // (syncCursorColor) only re-applies on a real OSC 12/112 transition.
  let appliedCursorColor: number | undefined
  // None of these change cell metrics, so they can apply after the grid is sized.
  // Defaults match the engine's own, so an unset reader is a no-op.
  const apply = (): void => {
    term.set_ligatures(readers.getLigatures())
    term.set_scrollback_limit(readers.getScrollbackLines())
    term.set_default_cursor_style(readers.getCursorStyleParam())
    // Per-cell WCAG fg floor (engine-side since set_minimum_contrast landed) — the
    // seeded default fg is additionally floored host-side (enforceDefaultContrast).
    term.set_minimum_contrast(readers.getMinimumContrastRatio())
    // null clears to the engine's default word logic (see the reader's mapping).
    term.set_word_separators(readers.getWordSeparators())
    // DEFAULT-bg / cursor-fill alpha (engine-side compositing; 1 = opaque no-op).
    // The pane DOM behind the canvas carries the theme background, so a translucent
    // default bg reveals it on both the 2d (putImageData) and WebGL2 paths.
    term.set_background_opacity(readers.getBackgroundOpacity())
    term.set_cursor_opacity(readers.getCursorOpacity())
    // Effects (sparkle words / cursor glow) — everything-off is byte-identical, so
    // panes with effects disabled render exactly as before. The glow colour follows
    // the live OSC 12 cursor colour (unset → theme-derived, native-app parity).
    appliedCursorColor = term.cursor_color
    applyAtermEffectsConfig(term, readers.getEffectsConfig(), appliedCursorColor)
    // Mosh-style predictive echo (default 'adaptive'). Routed through the pane's
    // prediction controller so the engine mode + its expiry timer stay in lockstep.
    deps.setPredictiveEcho(readers.getPredictiveEcho())
  }
  apply()
  // Kitty keyboard capability: per-pane STATIC policy (local Windows ConPTY panes
  // disable it), so apply once at engine construction — deliberately NOT part of
  // apply()/reapply(), which re-reads live user settings.
  term.set_kitty_keyboard_enabled(readers.getKittyKeyboardEnabled())
  // Seed + live-sync the OS color scheme (DEC 2031 / DSR 996); returns its disposer.
  const colorScheme = attachAtermColorSchemeSync({
    term,
    inputSink: deps.inputSink,
    isDisposed: deps.isDisposed
  })
  // OS reduce-motion changes live-apply: the effects config reads matchMedia at
  // apply time, so an OS toggle only needs a re-apply + repaint.
  const reduceMotion =
    typeof window.matchMedia === 'function'
      ? window.matchMedia('(prefers-reduced-motion: reduce)')
      : null
  const onReduceMotionChange = (): void => {
    if (!deps.isDisposed()) {
      apply()
      deps.scheduleDraw()
    }
  }
  reduceMotion?.addEventListener('change', onReduceMotionChange)
  return {
    dispose: () => {
      reduceMotion?.removeEventListener('change', onReduceMotionChange)
      colorScheme.dispose()
    },
    // Re-read the live settings + re-apply, so toggling ligatures / cursor style /
    // scrollback / cursor blink updates an already-open pane (color scheme already
    // live-syncs itself).
    reapply: () => {
      if (deps.isDisposed()) {
        return
      }
      apply()
      deps.refreshCursorBlink()
      deps.scheduleDraw()
    },
    // Per-drain-tick OSC 12 follow: when the app moved the cursor colour and the
    // glow runs theme-derived, re-fold the live colour into the glow config (OSC 112
    // resets cursor_color to undefined → back to theme-derived), per the engine's
    // cursor_color contract. Cheap getter compare; no-op while unchanged.
    syncCursorColor: () => {
      if (deps.isDisposed()) {
        return
      }
      const live = term.cursor_color
      if (live === appliedCursorColor) {
        return
      }
      appliedCursorColor = live
      const cfg = readers.getEffectsConfig()
      // Glow off (or host-gated off under reduced motion): nothing consumes the
      // colour; the next apply()/reapply() folds it in if the glow turns on.
      if (!cfg.cursorGlow || cfg.reducedMotion) {
        return
      }
      applyAtermCursorGlowConfig(term, cfg, live)
      deps.scheduleDraw()
    }
  }
}
