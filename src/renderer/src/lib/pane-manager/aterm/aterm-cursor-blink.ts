// Cursor blink + focus affordance for the aterm renderer. The engine paints the
// DECSCUSR-shaped cursor but has no timer of its own, so without this the cursor
// is a steady solid block that never blinks and gives no focus cue. This drives
// the engine's blink phase on a ~530ms timer (xterm's interval) while focused, and
// forces a HOLLOW cursor while the pane is unfocused — the standard terminal cue.
// The same focus/blur transition also dims the selection (xterm's
// selectionInactiveBackground) so an unfocused pane's selection reads as inactive.

export type AtermCursorTarget = {
  set_cursor_blink_phase: (on: boolean) => void
  set_cursor_hollow: (hollow: boolean) => void
  set_selection_inactive: (inactive: boolean) => void
  /** Effects focus gate (idle one-shots fire only while focused). Optional so
   *  pre-effects fakes/tests and older targets stay valid. */
  set_effects_focused?: (focused: boolean) => void
  /** Rain's tri-state visibility; supersedes the bool focus gate when present. */
  set_effects_visibility?: (state: 'focused' | 'visible_unfocused' | 'hidden') => void
}

export type AtermCursorBlinkDeps = {
  term: AtermCursorTarget
  /** The helper textarea whose focus/blur mirrors pane focus. */
  textarea: HTMLTextAreaElement
  redraw: () => void
  isDisposed: () => boolean
  /** Live terminalCursorBlink (xterm's cursorBlink); default true. When false the
   *  focused cursor is steady-on (no timer), matching xterm. */
  getCursorBlink?: () => boolean
  /** Hidden-pane state; hidden always wins over DOM focus for bounded rain drain. */
  isDrawSuspended?: () => boolean
}

// xterm's cursor blink interval.
const BLINK_INTERVAL_MS = 530

export type AtermCursorBlink = {
  /** Re-read getCursorBlink for a FOCUSED pane: focus/blur are the only other
   *  triggers, so a live setting toggle needs this to start/stop the timer
   *  without a blur/focus round-trip. No-op while unfocused (no timer runs). */
  refresh: () => void
  /** Re-emit tri-state visibility after a host suspend/resume transition. */
  refreshEffectsVisibility: () => void
  dispose: () => void
}

export function attachAtermCursorBlink(deps: AtermCursorBlinkDeps): AtermCursorBlink {
  const { term, textarea, redraw, isDisposed, getCursorBlink } = deps
  let timer: ReturnType<typeof setInterval> | null = null
  let phase = true

  const stopTimer = (): void => {
    if (timer !== null) {
      clearInterval(timer)
      timer = null
    }
  }

  const setPhase = (on: boolean): void => {
    phase = on
    term.set_cursor_blink_phase(on)
    redraw()
  }

  const setEffectsVisibility = (focused: boolean): void => {
    if (term.set_effects_visibility) {
      term.set_effects_visibility(
        deps.isDrawSuspended?.() ? 'hidden' : focused ? 'focused' : 'visible_unfocused'
      )
      return
    }
    term.set_effects_focused?.(focused)
  }

  // Focused: blink (toggle phase on the timer) when enabled, else steady-on.
  const startFocused = (): void => {
    stopTimer()
    if (isDisposed()) {
      return
    }
    term.set_cursor_hollow(false)
    // Focus → the selection paints with the ACTIVE background.
    term.set_selection_inactive(false)
    // Focus-gate the effects idle one-shots (engine §5.6): only a focused pane blinks.
    setEffectsVisibility(true)
    setPhase(true)
    if (getCursorBlink?.() ?? true) {
      timer = setInterval(() => {
        if (isDisposed()) {
          stopTimer()
          return
        }
        setPhase(!phase)
      }, BLINK_INTERVAL_MS)
    }
  }

  // Unfocused: steady HOLLOW box, no blink.
  const goUnfocused = (): void => {
    stopTimer()
    if (isDisposed()) {
      return
    }
    term.set_cursor_hollow(true)
    // Blur → the selection dims to the INACTIVE background.
    term.set_selection_inactive(true)
    setEffectsVisibility(false)
    setPhase(true)
  }

  const onFocus = (): void => startFocused()
  const onBlur = (): void => goUnfocused()

  // Seed from the current focus state so the cursor is correct before any event.
  if (document.activeElement === textarea) {
    startFocused()
  } else {
    goUnfocused()
  }
  textarea.addEventListener('focus', onFocus)
  textarea.addEventListener('blur', onBlur)

  return {
    refresh: (): void => {
      // startFocused re-reads getCursorBlink, so this restarts (or stops) the
      // timer to match the live setting; it also resets the phase to solid-on.
      if (document.activeElement === textarea) {
        startFocused()
      }
    },
    refreshEffectsVisibility: (): void => {
      setEffectsVisibility(document.activeElement === textarea)
    },
    dispose: (): void => {
      stopTimer()
      textarea.removeEventListener('focus', onFocus)
      textarea.removeEventListener('blur', onBlur)
    }
  }
}
