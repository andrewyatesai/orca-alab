/** The DOM shim that mirrors xterm's structure so the rest of the app's focus,
 *  paste, IME, and clipboard logic — which keys off `.xterm-helper-textarea` and
 *  `closest('.xterm')`/`.xterm-screen` — keeps working when the aterm canvas owns
 *  the pixels. The canvas keeps [data-testid=aterm-canvas] for e2e and remains the
 *  selection/scroll surface; keyboard focus lives on the hidden helper textarea. */
export type AtermInputDom = {
  /** `.xterm` wrapper appended to the pane container (satisfies closest('.xterm')). */
  wrapper: HTMLElement
  /** Hidden focus/keyboard/IME/paste sink (class 'xterm-helper-textarea'). */
  textarea: HTMLTextAreaElement
  /** Off-screen ARIA live region; screen readers read terminal output from here
   *  (the canvas itself is opaque to them). Mirrored from the engine grid on draw
   *  by `aterm-a11y-mirror`. */
  liveRegion: HTMLElement
}

/** Build `div.xterm > div.xterm-screen > (canvas + div.xterm-helpers >
 *  textarea.xterm-helper-textarea)` around the already-created canvas. */
export function buildAtermInputDom(canvas: HTMLCanvasElement): AtermInputDom {
  const wrapper = document.createElement('div')
  wrapper.className = 'xterm'
  wrapper.style.position = 'relative'
  wrapper.style.width = '100%'
  wrapper.style.height = '100%'

  const screen = document.createElement('div')
  screen.className = 'xterm-screen'
  screen.style.position = 'relative'
  screen.style.width = '100%'
  screen.style.height = '100%'

  const helpers = document.createElement('div')
  helpers.className = 'xterm-helpers'

  const textarea = document.createElement('textarea')
  textarea.className = 'xterm-helper-textarea'
  textarea.tabIndex = 0
  // autocapitalize/autocorrect aren't typed on HTMLTextAreaElement; set via attr.
  textarea.setAttribute('autocapitalize', 'off')
  textarea.setAttribute('autocorrect', 'off')
  textarea.autocomplete = 'off'
  textarea.spellcheck = false
  textarea.setAttribute('aria-label', 'Terminal input')
  // Invisible but focusable — mirrors xterm's helper-textarea styling so it can
  // receive keyboard/IME/paste without being seen or affecting layout.
  Object.assign(textarea.style, {
    position: 'absolute',
    opacity: '0',
    left: '-9999em',
    top: '0',
    width: '0',
    height: '0',
    zIndex: '-5',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    resize: 'none',
    padding: '0',
    border: '0'
  } satisfies Partial<CSSStyleDeclaration>)

  // Off-screen ARIA live region: the canvas is invisible to screen readers (and
  // xterm's AccessibilityManager never runs under aterm because terminal.open()
  // is never called), so without this NO terminal output is announced. role="log"
  // + aria-live="polite" makes assistive tech read appended rows; aria-atomic
  // false so only the changed text is announced, not the whole grid each update.
  // Positioned off-screen (NOT display:none — screen readers ignore display:none).
  const liveRegion = document.createElement('div')
  liveRegion.setAttribute('role', 'log')
  liveRegion.setAttribute('aria-live', 'polite')
  liveRegion.setAttribute('aria-atomic', 'false')
  liveRegion.setAttribute('aria-label', 'Terminal output')
  Object.assign(liveRegion.style, {
    position: 'absolute',
    left: '-9999em',
    top: '0',
    width: '1px',
    height: '1px',
    overflow: 'hidden',
    whiteSpace: 'pre-wrap'
  } satisfies Partial<CSSStyleDeclaration>)

  helpers.appendChild(textarea)
  helpers.appendChild(liveRegion)
  screen.appendChild(canvas)
  screen.appendChild(helpers)
  wrapper.appendChild(screen)

  return { wrapper, textarea, liveRegion }
}
