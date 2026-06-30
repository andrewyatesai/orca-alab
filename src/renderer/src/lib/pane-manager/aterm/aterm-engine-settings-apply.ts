import { attachAtermColorSchemeSync } from './aterm-color-scheme-sync'
import type { AtermControllerOptionReaders } from './aterm-controller-option-readers'

// Apply the user's terminal settings (ligatures, scrollback depth, default cursor shape)
// to the engine + wire the live OS color-scheme sync. The readers read the store live, so
// `reapply()` re-applies them on a settings change to an OPEN pane (the setters are cheap +
// don't change cell metrics — parity with how theme/size live-apply). Works on both render
// paths (the worker-backed term posts each as a command). Kept out of the wiring to keep
// it focused.

type EngineSettingsTarget = {
  set_ligatures: (on: boolean) => void
  set_scrollback_limit: (lines: number) => void
  set_default_cursor_style: (param: number) => void
  set_color_scheme: (dark: boolean) => void
  take_response: () => Uint8Array | undefined
}

export function applyAtermEngineSettings(deps: {
  term: EngineSettingsTarget
  /** The live settings readers; ligatures/scrollback/cursor are consumed here. */
  readers: AtermControllerOptionReaders
  inputSink: (data: string) => void
  isDisposed: () => boolean
  /** Schedule a repaint after a live re-apply (the cursor-style change needs one). */
  scheduleDraw: () => void
}): { dispose: () => void; reapply: () => void } {
  const { term, readers } = deps
  // None of these change cell metrics, so they can apply after the grid is sized.
  // Defaults match the engine's own, so an unset reader is a no-op.
  const apply = (): void => {
    term.set_ligatures(readers.getLigatures())
    term.set_scrollback_limit(readers.getScrollbackLines())
    term.set_default_cursor_style(readers.getCursorStyleParam())
  }
  apply()
  // Seed + live-sync the OS color scheme (DEC 2031 / DSR 996); returns its disposer.
  const colorScheme = attachAtermColorSchemeSync({
    term,
    inputSink: deps.inputSink,
    isDisposed: deps.isDisposed
  })
  return {
    dispose: colorScheme.dispose,
    // Re-read the live settings + re-apply, so toggling ligatures / cursor style /
    // scrollback updates an already-open pane (color scheme already live-syncs itself).
    reapply: () => {
      if (deps.isDisposed()) {
        return
      }
      apply()
      deps.scheduleDraw()
    }
  }
}
