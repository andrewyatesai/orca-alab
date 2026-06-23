import type { AtermTerminal } from './aterm_wasm.js'

/** Sends focus-report bytes to the PTY (same input seam as keys/mouse). */
export type AtermFocusInputSink = (data: string) => void

export type AtermFocusDeps = {
  /** The hidden helper textarea that actually owns keyboard focus for the pane. */
  textarea: HTMLTextAreaElement
  term: AtermTerminal
  inputSink: AtermFocusInputSink
  isDisposed: () => boolean
}

export type AtermFocusInput = {
  dispose: () => void
}

// DECSET 1004 focus reports: CSI I on focus-in, CSI O on focus-out.
const FOCUS_IN = '\x1b[I'
const FOCUS_OUT = '\x1b[O'

/** Send terminal focus reports when DECSET 1004 is active. Keyboard focus lives
 *  on the helper textarea, so its focus/blur transitions are the pane's true
 *  focus signal; gate on the live engine mode so reports only flow when an app
 *  (vim, tmux) asked for them. */
export function attachAtermFocusInput(deps: AtermFocusDeps): AtermFocusInput {
  const { textarea, term, inputSink, isDisposed } = deps

  const onFocus = (): void => {
    if (isDisposed() || !term.is_focus_event_mode) {
      return
    }
    inputSink(FOCUS_IN)
  }

  const onBlur = (): void => {
    if (isDisposed() || !term.is_focus_event_mode) {
      return
    }
    inputSink(FOCUS_OUT)
  }

  textarea.addEventListener('focus', onFocus)
  textarea.addEventListener('blur', onBlur)

  return {
    dispose: () => {
      textarea.removeEventListener('focus', onFocus)
      textarea.removeEventListener('blur', onBlur)
    }
  }
}
