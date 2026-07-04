// Why: the PTY input seam (pane.routePtyInput / onData) mixes real user input
// (keyboard, IME, paste, mouse reports) with the emulator's auto-replies —
// focus in/out reports (aterm-focus-input) and drained DA/DSR/CPR/OSC-color
// responses (aterm-reply-drain). aterm has no xterm-style core classification
// service, but every real-input byte starts as a user gesture on the pane's
// DOM (helper-textarea keys/paste, canvas pointer/wheel) while auto-replies
// never do — so the pane DOM is the honest classification point.

/** The live terminal mode reads the gesture classifier needs (a structural
 *  subset of the aterm terminal facade). */
type TerminalUserInputModeReads = {
  hasSelection(): boolean
  modes: { readonly mouseTrackingMode: string }
  buffer: { readonly active: { readonly type: string } }
}

/** A DOM target that can host the gesture listeners (the pane's `.xterm`
 *  wrapper or its container; both contain the helper textarea and canvas). */
export type TerminalUserInputEventTarget = {
  addEventListener(type: string, listener: EventListener, options?: AddEventListenerOptions): void
  removeEventListener(type: string, listener: EventListener, options?: EventListenerOptions): void
}

// Capture-phase so gestures on the async-attached textarea/canvas are seen from
// the stable pane target; passive because the signal only records activity.
const USER_INPUT_LISTENER_OPTIONS: AddEventListenerOptions = { capture: true, passive: true }

// Mirrors connectPanePty's onTerminalKeyDown filter: modifier-only presses and
// copy chords over an active selection produce no terminal input bytes.
const MODIFIER_ONLY_KEYS = new Set(['Alt', 'AltGraph', 'Control', 'Meta', 'Shift'])

function isTerminalInputProducingKeydown(
  terminal: TerminalUserInputModeReads,
  event: KeyboardEvent
): boolean {
  if (typeof event.key !== 'string') {
    return true
  }
  if (MODIFIER_ONLY_KEYS.has(event.key)) {
    return false
  }
  if (
    (event.metaKey || event.ctrlKey) &&
    event.key.toLowerCase() === 'c' &&
    terminal.hasSelection()
  ) {
    return false
  }
  return true
}

/**
 * Subscribe to the pane's real-user-input signal. Fires only for user gestures
 * that produce PTY bytes (keys, paste, mouse reports, alternate-scroll wheel),
 * never for the emulator's synthetic replies (focus reports, DA/DSR/CPR
 * drains) that also flow to the PTY.
 *
 * Returns null when the target cannot host DOM listeners (e.g. a headless
 * pane fixture) so callers can fall back to accepted-send recording —
 * degrading to the historical behavior instead of losing input tracking.
 */
export function subscribeToTerminalUserInput(
  terminal: TerminalUserInputModeReads,
  target: TerminalUserInputEventTarget | null | undefined,
  listener: () => void
): { dispose: () => void } | null {
  if (
    !target ||
    typeof target.addEventListener !== 'function' ||
    typeof target.removeEventListener !== 'function'
  ) {
    return null
  }
  const onKeyDown: EventListener = (event) => {
    if (isTerminalInputProducingKeydown(terminal, event as KeyboardEvent)) {
      listener()
    }
  }
  const onPaste: EventListener = () => {
    listener()
  }
  const onMouseDown: EventListener = () => {
    // Clicks send bytes only while an app tracks the mouse; plain clicks are
    // selection/focus gestures, not terminal input.
    if (terminal.modes.mouseTrackingMode !== 'none') {
      listener()
    }
  }
  const onWheel: EventListener = () => {
    // Wheel produces bytes under mouse tracking (wheel reports) or on the
    // alternate screen (alternate-scroll arrow synthesis, aterm-scroll-input);
    // a scrollback wheel only moves the viewport.
    if (
      terminal.modes.mouseTrackingMode !== 'none' ||
      terminal.buffer.active.type === 'alternate'
    ) {
      listener()
    }
  }
  const bindings: readonly (readonly [string, EventListener])[] = [
    ['keydown', onKeyDown],
    ['paste', onPaste],
    ['mousedown', onMouseDown],
    ['wheel', onWheel]
  ]
  const attached: (readonly [string, EventListener])[] = []
  const detach = (): void => {
    for (const [type, handler] of attached) {
      target.removeEventListener(type, handler, USER_INPUT_LISTENER_OPTIONS)
    }
    attached.length = 0
  }
  try {
    for (const binding of bindings) {
      target.addEventListener(binding[0], binding[1], USER_INPUT_LISTENER_OPTIONS)
      attached.push(binding)
    }
  } catch {
    // Why: a half-attached signal must read as unavailable WITHOUT leaving
    // listeners behind — callers re-enable their accepted-send fallback on a
    // null return, and a live remnant would double-record activity.
    try {
      detach()
    } catch {
      return null
    }
    return null
  }
  return { dispose: detach }
}
