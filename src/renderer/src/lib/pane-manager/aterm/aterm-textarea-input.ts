import { encodeKeyEventToBytes } from './aterm-key-encoding'
import type { AtermTerminal } from './aterm_wasm.js'

/** Inputs for the helper-textarea keyboard/text wiring. The textarea is the
 *  app's focus/paste/IME sink (mirrors xterm's helper textarea); this module owns
 *  the keydown/input/composition handlers that turn it into PTY bytes. */
export type AtermTextareaInputDeps = {
  textarea: HTMLTextAreaElement
  term: AtermTerminal
  /** Send encoded bytes to the PTY. */
  inputSink: (data: string) => void
  /** Copy the current canvas selection; returns true when something was copied. */
  copySelection: () => boolean
}

/** Wire the helper textarea to the PTY following xterm's input model:
 *  - keydown handles ONLY non-text keys (Enter, arrows, Ctrl/Alt chords, …);
 *    plain printable chars return null from the encoder and are NOT sent here.
 *  - the 'input' event handles printable text, paste (setRangeText+InputEvent),
 *    and the IME commit (compositionend) — one route for all text, never doubled.
 *  Returns a disposer that removes every listener.
 *
 *  DECCKM (application cursor keys) is read per-press from the engine so arrows +
 *  Home/End emit SS3 vs CSI to match the live mode. */
export function attachAtermTextareaInput(deps: AtermTextareaInputDeps): { dispose: () => void } {
  const { textarea, term, inputSink, copySelection } = deps
  // Platform-correct copy modifier: Cmd on macOS, Ctrl elsewhere.
  const isMac = typeof navigator !== 'undefined' && navigator.userAgent.includes('Mac')
  let composing = false

  // Copy the canvas selection for the platform's copy chord; returns true when
  // the chord was handled (so the caller swallows it). On Linux/Windows
  // Ctrl+Shift+C is the EXPLICIT copy shortcut and must NEVER send ^C — it is
  // swallowed even with no selection so it can't leak an interrupt.
  const tryCopyChord = (event: KeyboardEvent): boolean => {
    if (event.key.toLowerCase() !== 'c') {
      return false
    }
    if (isMac) {
      // Cmd+C copies; with no selection it falls through (Cmd doesn't send ^C).
      return event.metaKey ? copySelection() : false
    }
    // Ctrl+Shift+C = explicit copy: always swallow, copy when there's a selection.
    if (event.ctrlKey && event.shiftKey) {
      copySelection()
      return true
    }
    // Plain Ctrl+C copies only when there's a selection; otherwise it falls
    // through so the encoder sends ^C (interrupt).
    return event.ctrlKey ? copySelection() : false
  }

  const onKeyDown = (event: KeyboardEvent): void => {
    // Let the IME own keys while a composition is active (checked here AND in the
    // input handler so a composed string is never also sent char-by-char).
    if (event.isComposing || composing) {
      return
    }
    if (tryCopyChord(event)) {
      event.preventDefault()
      return
    }
    // Read DECCKM each press so arrows/Home/End follow the live cursor-key mode.
    const bytes = encodeKeyEventToBytes(event, { appCursor: term.is_app_cursor_mode })
    // Plain printable chars return null here; they flow through onInput instead,
    // so keydown sends ONLY non-text keys and nothing is double-sent.
    if (bytes === null) {
      return
    }
    event.preventDefault()
    inputSink(bytes)
    // Clear so the sink-bound textarea never accumulates the typed characters.
    textarea.value = ''
  }

  // Text input path (typing, paste via setRangeText+InputEvent, IME commit): the
  // helper textarea has no keydown sender for printable chars (see onKeyDown), so
  // the actual character bytes arrive here. Mirrors xterm: keydown = non-text
  // keys, input/compositionend = text.
  const onInput = (event: Event): void => {
    // compositionend handles the committed IME string; ignore inputs fired while
    // composing so a composed run isn't sent twice.
    if (composing || (event as InputEvent).isComposing) {
      return
    }
    const inputEvent = event as InputEvent
    // For insertText/insertFromPaste InputEvent.data carries the inserted text.
    // Chunked/large pastes (text-control-paste.ts) and some browsers fire an
    // InputEvent with null data after mutating value — read textarea.value then.
    const data = inputEvent.data ?? textarea.value
    if (data) {
      inputSink(data)
    }
    // Always clear so the sink-bound textarea never accumulates sent characters.
    textarea.value = ''
  }

  // IME: buffer composing keystrokes, then send the committed string on end.
  const onCompositionStart = (): void => {
    composing = true
  }
  // compositionupdate intentionally has NO handler: sending on update would
  // double-send the in-progress string before compositionend commits it.
  const onCompositionEnd = (event: CompositionEvent): void => {
    composing = false
    if (event.data) {
      inputSink(event.data)
    }
    textarea.value = ''
  }

  textarea.addEventListener('keydown', onKeyDown)
  textarea.addEventListener('input', onInput)
  textarea.addEventListener('compositionstart', onCompositionStart)
  textarea.addEventListener('compositionend', onCompositionEnd)

  return {
    dispose: () => {
      textarea.removeEventListener('keydown', onKeyDown)
      textarea.removeEventListener('input', onInput)
      textarea.removeEventListener('compositionstart', onCompositionStart)
      textarea.removeEventListener('compositionend', onCompositionEnd)
    }
  }
}
