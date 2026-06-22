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

  helpers.appendChild(textarea)
  screen.appendChild(canvas)
  screen.appendChild(helpers)
  wrapper.appendChild(screen)

  return { wrapper, textarea }
}
