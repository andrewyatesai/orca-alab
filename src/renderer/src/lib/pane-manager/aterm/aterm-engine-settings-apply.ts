import { attachAtermColorSchemeSync } from './aterm-color-scheme-sync'
import type { AtermControllerOptionReaders } from './aterm-controller-option-readers'

// Apply the user's FIXED terminal settings to a freshly built engine and wire the live
// OS color-scheme sync. "Fixed" = settled for the pane's life (like the font family): a
// change applies to the next opened terminal, not retroactively. Works on both render
// paths — the worker-backed term posts each as a command. Kept out of the wiring to keep
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
}): { dispose: () => void } {
  const { term, readers } = deps
  // None of these change cell metrics, so they can apply after the grid is sized.
  // Defaults match the engine's own, so an unset reader is a no-op.
  term.set_ligatures(readers.getLigatures())
  term.set_scrollback_limit(readers.getScrollbackLines())
  term.set_default_cursor_style(readers.getCursorStyleParam())
  // Seed + live-sync the OS color scheme (DEC 2031 / DSR 996); returns its disposer.
  return attachAtermColorSchemeSync({
    term,
    inputSink: deps.inputSink,
    isDisposed: deps.isDisposed
  })
}
