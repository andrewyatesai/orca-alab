/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it, vi } from 'vitest'
import { attachAtermTextareaInput } from './aterm-textarea-input'
import {
  setAtermCursorGlowActivity,
  setAtermMatrixRainActivity
} from './aterm-effects-activity-gate'
import {
  markTerminalPinnedViewport,
  syncTerminalScrollIntentFromViewport
} from '../terminal-scroll-intent'
import { encode_key_with_mode } from './aterm_wasm.js'
import type { AtermTerminal } from './aterm_wasm.js'
import type { TerminalScrollIntentTarget } from '../terminal-scroll-intent-types'

// The module under test imports the wasm glue for the worker-path free-function
// encoder; keep these DOM tests off the real (uninitialized) wasm module.
vi.mock('./aterm_wasm.js', () => ({
  encode_key_with_mode: vi.fn(() => new Uint8Array([0x1b, 0x4f, 0x41]))
}))

// Spy the scroll-intent seam so we can assert Shift+PageUp/Down records intent on the
// facade (the direct engine scroll must not skip the seam a keyed remount restores).
vi.mock('../terminal-scroll-intent', () => ({
  markTerminalPinnedViewport: vi.fn(),
  syncTerminalScrollIntentFromViewport: vi.fn()
}))

type FakeTermOverrides = {
  /** Omit to model the worker-backed term (no engine on this thread). */
  encodeKey?: ReturnType<typeof vi.fn>
  isAltScreen?: boolean
  keyboardModeBits?: number
  matrixRainEnabled?: boolean
  cursorGlowEnabled?: boolean
}

// Minimal engine stand-in: the keydown path reads encode_key / keyboard_mode_bits /
// is_alt_screen / scroll_lines, the composition view reads the cursor.
function fakeTerm(overrides: FakeTermOverrides = {}): {
  term: AtermTerminal
  scrollLines: ReturnType<typeof vi.fn>
  noteKeystroke: ReturnType<typeof vi.fn>
  noteAltScroll: ReturnType<typeof vi.fn>
  noteRainSignal: ReturnType<typeof vi.fn>
} {
  const scrollLines = vi.fn()
  const noteKeystroke = vi.fn()
  const noteAltScroll = vi.fn()
  const noteRainSignal = vi.fn()
  const term = {
    ...(overrides.encodeKey ? { encode_key: overrides.encodeKey } : {}),
    is_alt_screen: overrides.isAltScreen ?? false,
    keyboard_mode_bits: overrides.keyboardModeBits ?? 0,
    scroll_lines: scrollLines,
    note_keystroke: noteKeystroke,
    note_matrix_rain_alt_scroll: noteAltScroll,
    note_matrix_rain_signal: noteRainSignal,
    cursor_x: 4,
    cursor_y: 2
  } as unknown as AtermTerminal
  return { term, scrollLines, noteKeystroke, noteAltScroll, noteRainSignal }
}

type Harness = {
  textarea: HTMLTextAreaElement
  inputSink: ReturnType<typeof vi.fn>
  pasteSink: ReturnType<typeof vi.fn>
  copySelection: ReturnType<typeof vi.fn>
  redraw: ReturnType<typeof vi.fn>
  scrollLines: ReturnType<typeof vi.fn>
  noteKeystroke: ReturnType<typeof vi.fn>
  noteAltScroll: ReturnType<typeof vi.fn>
  noteRainSignal: ReturnType<typeof vi.fn>
  dispose: () => void
}

function mount(
  termOverrides: FakeTermOverrides = {},
  getMacOptionIsMeta?: () => boolean,
  getCustomKeyEventHandler?: () => ((event: KeyboardEvent) => boolean) | null,
  getScrollIntentTarget?: () => TerminalScrollIntentTarget | null,
  predictionEcho?: Parameters<typeof attachAtermTextareaInput>[0]['predictionEcho']
): Harness {
  const wrapper = document.createElement('div')
  const screen = document.createElement('div')
  const canvas = document.createElement('canvas')
  // happy-dom does no layout, so give the grid canvas a CSS box (the composition
  // view derives the viewport from it to clamp the cursor anchor).
  Object.defineProperty(canvas, 'clientWidth', { value: 400 })
  Object.defineProperty(canvas, 'clientHeight', { value: 240 })
  const textarea = document.createElement('textarea')
  screen.appendChild(canvas)
  screen.appendChild(textarea)
  wrapper.appendChild(screen)
  document.body.appendChild(wrapper)
  const inputSink = vi.fn()
  const pasteSink = vi.fn()
  const copySelection = vi.fn(() => false)
  const redraw = vi.fn()
  const { term, scrollLines, noteKeystroke, noteAltScroll, noteRainSignal } =
    fakeTerm(termOverrides)
  setAtermMatrixRainActivity(term, termOverrides.matrixRainEnabled ?? true)
  setAtermCursorGlowActivity(term, termOverrides.cursorGlowEnabled ?? false)
  const { dispose } = attachAtermTextareaInput({
    textarea,
    term,
    canvas,
    metrics: { dpr: 2, cellWidth: 10, cellHeight: 20 },
    themeColors: { fg: 0xffffff, bg: 0x000000 },
    getRows: () => 24,
    redraw,
    inputSink,
    pasteSink,
    copySelection,
    getMacOptionIsMeta,
    getCustomKeyEventHandler,
    getScrollIntentTarget,
    predictionEcho
  })
  return {
    textarea,
    inputSink,
    pasteSink,
    copySelection,
    redraw,
    scrollLines,
    noteKeystroke,
    noteAltScroll,
    noteRainSignal,
    dispose
  }
}

type FireKeyModifiers = Partial<
  Pick<KeyboardEvent, 'ctrlKey' | 'altKey' | 'metaKey' | 'shiftKey' | 'isComposing'>
>

function fireKey(
  textarea: HTMLTextAreaElement,
  type: 'keydown' | 'keyup',
  key: string,
  modifiers: FireKeyModifiers = {}
): KeyboardEvent {
  const event = new KeyboardEvent(type, { key, bubbles: true, cancelable: true, ...modifiers })
  textarea.dispatchEvent(event)
  return event
}

function fireKeydown(
  textarea: HTMLTextAreaElement,
  key: string,
  modifiers: FireKeyModifiers = {}
): KeyboardEvent {
  return fireKey(textarea, 'keydown', key, modifiers)
}

function fireKeyup(
  textarea: HTMLTextAreaElement,
  key: string,
  modifiers: FireKeyModifiers = {}
): KeyboardEvent {
  return fireKey(textarea, 'keyup', key, modifiers)
}

function fireInput(textarea: HTMLTextAreaElement, data: string | null, inputType: string): void {
  const event = new InputEvent('input', { data: data ?? undefined, inputType, bubbles: true })
  // happy-dom coerces a null `data` init to ''; force the real-browser null so the
  // textarea.value fallback path is exercised (chunked-paste tail).
  if (data === null) {
    Object.defineProperty(event, 'data', { value: null, configurable: true })
  }
  textarea.dispatchEvent(event)
}

// happy-dom drops CompositionEvent.data from the init dict, so define it on the
// dispatched event to mirror the real browser's committed-string delivery.
function fireComposition(
  textarea: HTMLTextAreaElement,
  type: 'compositionstart' | 'compositionupdate' | 'compositionend',
  data?: string
): void {
  const event = new CompositionEvent(type, { bubbles: true })
  if (data !== undefined) {
    Object.defineProperty(event, 'data', { value: data, configurable: true })
  }
  textarea.dispatchEvent(event)
}

describe('attachAtermTextareaInput', () => {
  it('delivers input to the PTY even when predictive echo throws (input is not gated behind speculative echo)', () => {
    // A predict-path throw (worker seam under load) must NEVER swallow the
    // keystroke: inputSink runs BEFORE noteChar and noteChar is guarded.
    const noteChar = vi.fn(() => {
      throw new Error('predict seam blew up')
    })
    const predictionEcho = {
      noteChar,
      noteBackspace: vi.fn(),
      noteSubmit: vi.fn(),
      reconcile: vi.fn(),
      overlayCells: vi.fn(() => new Uint32Array(0)),
      setMode: vi.fn(),
      refreshDeadline: vi.fn(),
      reset: vi.fn(),
      dispose: vi.fn()
    }
    const h = mount({}, undefined, undefined, undefined, predictionEcho)
    expect(() => fireInput(h.textarea, 'x', 'insertText')).not.toThrow()
    expect(h.inputSink).toHaveBeenCalledWith('x') // the char still reached the PTY
    expect(noteChar).toHaveBeenCalledWith('x') // and prediction was attempted
    h.dispose()
  })

  it('sends the InputEvent data for an insertText (typed character) via inputSink', () => {
    const h = mount()
    fireInput(h.textarea, 'x', 'insertText')
    expect(h.inputSink).toHaveBeenCalledWith('x')
    expect(h.noteKeystroke).toHaveBeenCalledTimes(1)
    expect(h.pasteSink).not.toHaveBeenCalled() // typing must NOT go through paste
    expect(h.textarea.value).toBe('') // cleared so it never accumulates
    h.dispose()
  })

  it('does not cross the effects seam for typing while matrix rain is off', () => {
    const h = mount({ matrixRainEnabled: false })
    fireInput(h.textarea, 'x', 'insertText')
    expect(h.inputSink).toHaveBeenCalledWith('x')
    expect(h.noteKeystroke).not.toHaveBeenCalled()
    h.dispose()
  })

  it('keeps cursor momentum live when glow is on and matrix rain is off', () => {
    const h = mount({ matrixRainEnabled: false, cursorGlowEnabled: true })
    fireInput(h.textarea, 'x', 'insertText')
    expect(h.noteKeystroke).toHaveBeenCalledTimes(1)
    h.dispose()
  })

  it('routes a paste through pasteSink and typing through inputSink (M1)', () => {
    const h = mount()
    // A paste must reach the paste sink (which wraps with DECSET 2004 markers),
    // never the raw input sink that would let an app auto-indent/run it.
    fireInput(h.textarea, 'PASTED', 'insertFromPaste')
    expect(h.pasteSink).toHaveBeenCalledWith('PASTED')
    expect(h.inputSink).not.toHaveBeenCalled()
    expect(h.noteKeystroke).not.toHaveBeenCalled()
    // A typed character goes the other way: input sink, not the paste sink.
    fireInput(h.textarea, 'y', 'insertText')
    expect(h.inputSink).toHaveBeenCalledWith('y')
    expect(h.pasteSink).toHaveBeenCalledTimes(1)
    expect(h.noteKeystroke).toHaveBeenCalledTimes(1)
    h.dispose()
  })

  it('routes insertReplacementText through pasteSink (M1)', () => {
    const h = mount()
    // The clipboard paste path can fire insertReplacementText; it is a paste too.
    fireInput(h.textarea, 'REPLACED', 'insertReplacementText')
    expect(h.pasteSink).toHaveBeenCalledWith('REPLACED')
    expect(h.inputSink).not.toHaveBeenCalled()
    h.dispose()
  })

  it('delivers an insertFromPaste InputEvent even while an IME composition is open (M1)', () => {
    const h = mount()
    // Open a local composition (sets the module's composing flag).
    fireComposition(h.textarea, 'compositionstart')
    // A programmatic paste fires insertFromPaste with isComposing=false; it must
    // still reach the PTY (via the paste sink) despite the open composition.
    fireInput(h.textarea, 'PASTED', 'insertFromPaste')
    expect(h.pasteSink).toHaveBeenCalledWith('PASTED')
    expect(h.inputSink).not.toHaveBeenCalled()
    h.dispose()
  })

  it('suppresses a genuine composing insertText input until compositionend', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    // An in-progress IME keystroke while composing carries isComposing=true in
    // Chromium and must NOT be sent. (A NON-composing insertText mid-composition
    // is a macOS Telex commit instead — covered in aterm-ime-commit-ordering.)
    const event = new InputEvent('input', { data: 'n', inputType: 'insertText', bubbles: true })
    Object.defineProperty(event, 'isComposing', { value: true, configurable: true })
    h.textarea.dispatchEvent(event)
    expect(h.inputSink).not.toHaveBeenCalled()
    h.dispose()
  })

  it('falls back to textarea.value (via pasteSink) when InputEvent data is null (chunked paste tail)', () => {
    const h = mount()
    // Chunked paste mutates value then fires a null-data insertFromPaste; the
    // value-fallback path is a paste, so it must reach the paste sink.
    h.textarea.value = 'chunked-tail'
    fireInput(h.textarea, null, 'insertFromPaste')
    expect(h.pasteSink).toHaveBeenCalledWith('chunked-tail')
    expect(h.inputSink).not.toHaveBeenCalled()
    h.dispose()
  })

  it('sends the committed string exactly once on compositionend (no double-send)', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    // compositionupdate only renders the preedit; only compositionend commits.
    fireComposition(h.textarea, 'compositionupdate', 'にほ')
    fireComposition(h.textarea, 'compositionend', 'にほん')
    expect(h.inputSink).toHaveBeenCalledTimes(1)
    expect(h.inputSink).toHaveBeenCalledWith('にほん')
    h.dispose()
  })

  it('anchors the textarea to the cursor cell while composing and re-parks it after', () => {
    const h = mount()
    // cursor (4,2) × cell 10×20 device px at dpr 2 → CSS (20px, 20px).
    fireComposition(h.textarea, 'compositionstart')
    expect(h.textarea.style.left).toBe('20px')
    expect(h.textarea.style.top).toBe('20px')
    fireComposition(h.textarea, 'compositionend', 'ん')
    // Restored to the parked position so the candidate window can't linger.
    expect(h.textarea.style.left).toBe('-9999em')
    expect(h.textarea.style.width).toBe('0px')
    h.dispose()
  })

  it('does not double-send a printable: keydown returns null and only input sends', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x61]))
    const h = mount({ encodeKey })
    // A plain printable keydown must not reach the engine encoder (not
    // preventDefault'd / not sent); the character arrives via input only.
    const keydown = fireKeydown(h.textarea, 'a')
    expect(encodeKey).not.toHaveBeenCalled()
    expect(h.inputSink).not.toHaveBeenCalled()
    expect(keydown.defaultPrevented).toBe(false)
    fireInput(h.textarea, 'a', 'insertText')
    expect(h.inputSink).toHaveBeenCalledTimes(1)
    expect(h.inputSink).toHaveBeenCalledWith('a')
    h.dispose()
  })

  it('report-all: printable keydown engine-encodes + preventDefaults (never double-sends)', () => {
    // Kitty REPORT_ALL_KEYS_AS_ESC (mode bit 0x100): the app negotiated escape
    // reports for EVERY key, so a plain printable press must be engine-encoded
    // on keydown instead of flowing out as raw text.
    const encodeKey = vi.fn(() => new Uint8Array([0x1b, 0x5b, 0x39, 0x37, 0x75])) // ESC[97u
    const h = mount({ encodeKey, keyboardModeBits: 0x100 })
    const keydown = fireKeydown(h.textarea, 'a')
    expect(encodeKey).toHaveBeenCalledWith('a', 0, 0, undefined)
    expect(h.inputSink).toHaveBeenCalledTimes(1)
    expect(h.inputSink).toHaveBeenCalledWith('\x1b[97u')
    // CRITICAL never-double-send invariant: bytes on keydown ⇒ preventDefault,
    // so the browser fires NO input event for this press — the report is the
    // only thing sent. (No manual input dispatch here on purpose: a real
    // browser cannot produce one after a cancelled keydown.)
    expect(keydown.defaultPrevented).toBe(true)
    h.dispose()
  })

  it('report-all: engine returning nothing falls back to the text path (key never dead)', () => {
    const encodeKey = vi.fn(() => new Uint8Array(0))
    const h = mount({ encodeKey, keyboardModeBits: 0x100 })
    const keydown = fireKeydown(h.textarea, 'a')
    expect(keydown.defaultPrevented).toBe(false)
    expect(h.inputSink).not.toHaveBeenCalled()
    // The un-cancelled keydown lets the browser deliver the text input event.
    fireInput(h.textarea, 'a', 'insertText')
    expect(h.inputSink).toHaveBeenCalledTimes(1)
    expect(h.inputSink).toHaveBeenCalledWith('a')
    h.dispose()
  })

  it('sends a non-text key (Enter) via the ENGINE encoder and preventDefaults it', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x0d]))
    const h = mount({ encodeKey })
    const keydown = fireKeydown(h.textarea, 'Enter')
    expect(encodeKey).toHaveBeenCalledWith('Enter', 0, 0, undefined)
    expect(h.inputSink).toHaveBeenCalledWith('\r')
    expect(h.noteRainSignal).toHaveBeenCalledWith(10, 4)
    expect(keydown.defaultPrevented).toBe(true)
    h.dispose()
  })

  it('does not send a turn boundary while Matrix Rain is disabled', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x0d]))
    const h = mount({ encodeKey, matrixRainEnabled: false })
    fireKeydown(h.textarea, 'Enter')
    expect(h.inputSink).toHaveBeenCalledWith('\r')
    expect(h.noteRainSignal).not.toHaveBeenCalled()
    h.dispose()
  })

  it('treats Shift+Enter as input, not an agent turn boundary', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x0d]))
    const h = mount({ encodeKey })
    fireKeydown(h.textarea, 'Enter', { shiftKey: true })
    expect(h.inputSink).toHaveBeenCalledWith('\r')
    expect(h.noteRainSignal).not.toHaveBeenCalled()
    h.dispose()
  })

  it.each([
    ['Alt', { altKey: true }],
    ['Ctrl', { ctrlKey: true }],
    ['Meta', { metaKey: true }]
  ] as const)('does not treat %s+Enter as an agent turn boundary', (_name, modifiers) => {
    const encodeKey = vi.fn(() => new Uint8Array([0x0d]))
    const h = mount({ encodeKey })
    fireKeydown(h.textarea, 'Enter', modifiers)
    expect(h.noteRainSignal).not.toHaveBeenCalled()
    h.dispose()
  })

  it('passes Shift+arrow to the engine with the SHIFT modifier bit', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x32, 0x44]))
    const h = mount({ encodeKey })
    fireKeydown(h.textarea, 'ArrowLeft', { shiftKey: true })
    expect(encodeKey).toHaveBeenCalledWith('ArrowLeft', 1, 0, undefined)
    expect(h.inputSink).toHaveBeenCalledWith('\x1b[1;2D')
    h.dispose()
  })

  it('encodes via the free function + snapshot mode bits on the worker path', () => {
    // No encode_key on the term = worker-backed (the engine lives off-thread).
    const h = mount({ keyboardModeBits: 0x4 })
    fireKeydown(h.textarea, 'ArrowUp')
    expect(vi.mocked(encode_key_with_mode)).toHaveBeenCalledWith('ArrowUp', 0, 0, undefined, 0x4)
    expect(h.inputSink).toHaveBeenCalledWith('\x1bOA')
    h.dispose()
  })

  it('swallows Ctrl+Shift+C as the explicit copy chord (never ^C) on non-Mac', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x03]))
    const h = mount({ encodeKey })
    const keydown = fireKeydown(h.textarea, 'C', { ctrlKey: true, shiftKey: true })
    expect(h.copySelection).toHaveBeenCalled()
    expect(encodeKey).not.toHaveBeenCalled()
    expect(h.inputSink).not.toHaveBeenCalled()
    expect(keydown.defaultPrevented).toBe(true)
    h.dispose()
  })

  it('pages the scrollback for Shift+PageUp/PageDown on the main screen', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x1b]))
    const h = mount({ encodeKey })
    const up = fireKeydown(h.textarea, 'PageUp', { shiftKey: true })
    // 24 rows → 23-line page; positive aterm delta reveals older history.
    expect(h.scrollLines).toHaveBeenCalledWith(23)
    expect(h.redraw).toHaveBeenCalled()
    expect(up.defaultPrevented).toBe(true)
    fireKeydown(h.textarea, 'PageDown', { shiftKey: true })
    expect(h.scrollLines).toHaveBeenLastCalledWith(-23)
    // The chord never reaches the engine encoder / PTY on the main screen.
    expect(encodeKey).not.toHaveBeenCalled()
    expect(h.inputSink).not.toHaveBeenCalled()
    h.dispose()
  })

  it('lets the engine encode Shift+PageUp on the alternate screen (TUIs own paging)', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x1b, 0x5b, 0x35, 0x3b, 0x32, 0x7e]))
    const h = mount({ encodeKey, isAltScreen: true })
    fireKeydown(h.textarea, 'PageUp', { shiftKey: true })
    expect(h.scrollLines).not.toHaveBeenCalled()
    expect(encodeKey).toHaveBeenCalledWith('PageUp', 1, 0, undefined)
    expect(h.inputSink).toHaveBeenCalledWith('\x1b[5;2~')
    expect(h.noteAltScroll).toHaveBeenCalledTimes(1)
    expect(h.noteKeystroke).toHaveBeenCalledTimes(1)
    h.dispose()
  })

  it('consults the custom key handler BEFORE encoding: false suppresses the send', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x0d]))
    const handler = vi.fn(() => false)
    const h = mount({ encodeKey }, undefined, () => handler)
    const keydown = fireKeydown(h.textarea, 'Enter')
    expect(handler).toHaveBeenCalledWith(keydown)
    // The consumer handled/suppressed it: nothing encoded, nothing sent, and no
    // preventDefault (native paste/copy events may still need to fire).
    expect(encodeKey).not.toHaveBeenCalled()
    expect(h.inputSink).not.toHaveBeenCalled()
    expect(keydown.defaultPrevented).toBe(false)
    h.dispose()
  })

  it('encodes normally when the custom key handler returns true', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x0d]))
    const handler = vi.fn(() => true)
    const h = mount({ encodeKey }, undefined, () => handler)
    const keydown = fireKeydown(h.textarea, 'Enter')
    expect(handler).toHaveBeenCalledWith(keydown)
    expect(encodeKey).toHaveBeenCalledWith('Enter', 0, 0, undefined)
    expect(h.inputSink).toHaveBeenCalledWith('\r')
    expect(keydown.defaultPrevented).toBe(true)
    h.dispose()
  })

  it('runs the copy chord before the custom key handler (aterm owns the copy pipeline)', () => {
    // The lifecycle handler returns false for clipboard chords (its xterm-era
    // "bypass to the native copy event" rule); the aterm copy pipeline is the
    // chord itself, so it must win or Ctrl+Shift+C would copy nothing.
    const handler = vi.fn(() => false)
    const h = mount({}, undefined, () => handler)
    fireKeydown(h.textarea, 'C', { ctrlKey: true, shiftKey: true })
    expect(h.copySelection).toHaveBeenCalled()
    expect(handler).not.toHaveBeenCalled()
    h.dispose()
  })

  it('sends CSI-u release bytes for a keyup under kitty event-type reporting', () => {
    // In-process engine with REPORT_EVENT_TYPES negotiated: release of 'a'
    // encodes as CSI-u with event-type :3 (byte production proven in Rust).
    const releaseBytes = new Uint8Array([0x1b, 0x5b, 0x39, 0x37, 0x3b, 0x31, 0x3a, 0x33, 0x75])
    const encodeKey = vi.fn(() => releaseBytes)
    const h = mount({ encodeKey })
    const keyup = fireKeyup(h.textarea, 'a')
    expect(encodeKey).toHaveBeenCalledWith('a', 0, 2, undefined)
    expect(h.inputSink).toHaveBeenCalledWith('\x1b[97;1:3u')
    expect(keyup.defaultPrevented).toBe(true)
    h.dispose()
  })

  it('sends nothing for a legacy-mode release (engine emits no bytes) and leaves the event alone', () => {
    // The engine drops releases outside kitty event-type reporting; the keyup
    // listener must be free in legacy mode: no send, no preventDefault.
    const encodeKey = vi.fn(() => new Uint8Array(0))
    const h = mount({ encodeKey })
    const keyup = fireKeyup(h.textarea, 'a')
    expect(encodeKey).toHaveBeenCalledWith('a', 0, 2, undefined)
    expect(h.inputSink).not.toHaveBeenCalled()
    expect(keyup.defaultPrevented).toBe(false)
    h.dispose()
  })

  it('encodes releases via the free function + snapshot mode bits on the worker path', () => {
    // No encode_key on the term = worker-backed; keyups use the same snapshot
    // keyboard_mode_bits seam as keydowns. 0x2 = kitty REPORT_EVENT_TYPES.
    const h = mount({ keyboardModeBits: 0x2 })
    fireKeyup(h.textarea, 'ArrowUp')
    expect(vi.mocked(encode_key_with_mode)).toHaveBeenLastCalledWith(
      'ArrowUp',
      0,
      2,
      undefined,
      0x2
    )
    h.dispose()
  })

  it('suppresses keyups while an IME composition is open', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x61]))
    const h = mount({ encodeKey })
    fireComposition(h.textarea, 'compositionstart')
    fireKeyup(h.textarea, 'a')
    // Browsers also flag composition keyups directly; both gates must hold.
    fireKeyup(h.textarea, 'n', { isComposing: true })
    expect(encodeKey).not.toHaveBeenCalled()
    expect(h.inputSink).not.toHaveBeenCalled()
    h.dispose()
  })

  it('never encodes a Cmd+key release (Mac app shortcuts own press AND release)', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x63]))
    const h = mount({ encodeKey })
    const keyup = fireKeyup(h.textarea, 'c', { metaKey: true })
    expect(encodeKey).not.toHaveBeenCalled()
    expect(h.inputSink).not.toHaveBeenCalled()
    expect(keyup.defaultPrevented).toBe(false)
    h.dispose()
  })

  it('consults the custom key handler on keyup: false suppresses the release', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x61]))
    const handler = vi.fn(() => false)
    const h = mount({ encodeKey }, undefined, () => handler)
    const keyup = fireKeyup(h.textarea, 'a')
    expect(handler).toHaveBeenCalledWith(keyup)
    expect(encodeKey).not.toHaveBeenCalled()
    expect(h.inputSink).not.toHaveBeenCalled()
    h.dispose()
  })

  it('encodes a keyup with no matching keydown (focus gained mid-hold)', () => {
    const encodeKey = vi.fn(() => new Uint8Array([0x1b, 0x5b, 0x75]))
    const h = mount({ encodeKey })
    // No prior keydown fired; the release still reaches the engine, which
    // decides whether the negotiated mode reports it.
    fireKeyup(h.textarea, 'Enter')
    expect(encodeKey).toHaveBeenCalledWith('Enter', 0, 2, undefined)
    expect(h.inputSink).toHaveBeenCalledWith('\x1b[u')
    h.dispose()
  })

  it('pages scrollback for Shift+PageUp only when the handler lets the key through', () => {
    const handler = vi.fn(() => false)
    const h = mount({}, undefined, () => handler)
    const up = fireKeydown(h.textarea, 'PageUp', { shiftKey: true })
    expect(handler).toHaveBeenCalledWith(up)
    expect(h.scrollLines).not.toHaveBeenCalled()
    h.dispose()
    const allow = mount({}, undefined, () => () => true)
    fireKeydown(allow.textarea, 'PageUp', { shiftKey: true })
    expect(allow.scrollLines).toHaveBeenCalledWith(23)
    allow.dispose()
  })

  it('records scroll intent on the facade after a Shift+PageUp/Down page (keyed-remount restore)', () => {
    // The Shift+PageUp/Down page scrolls the engine directly; without recording intent
    // through the facade seam, a keyed remount / workspace-switch snaps the viewport to
    // the bottom and loses the reading position (the sibling of the Cmd+Up/Down bug).
    vi.mocked(markTerminalPinnedViewport).mockClear()
    vi.mocked(syncTerminalScrollIntentFromViewport).mockClear()
    const target = {} as TerminalScrollIntentTarget
    const h = mount({}, undefined, undefined, () => target)

    fireKeydown(h.textarea, 'PageUp', { shiftKey: true })
    expect(h.scrollLines).toHaveBeenCalledWith(23)
    // mark-then-sync, mirroring keyboard-handlers' Cmd+Up path — the exact same seam.
    expect(markTerminalPinnedViewport).toHaveBeenCalledWith(target)
    expect(syncTerminalScrollIntentFromViewport).toHaveBeenCalledWith(target, {
      userInteraction: true
    })

    fireKeydown(h.textarea, 'PageDown', { shiftKey: true })
    expect(markTerminalPinnedViewport).toHaveBeenCalledTimes(2)
    expect(syncTerminalScrollIntentFromViewport).toHaveBeenCalledTimes(2)
    h.dispose()
  })

  it('does not record scroll intent when the page is handled by the engine (alt screen)', () => {
    // On the alternate screen the chord falls through to the engine encoder (TUIs own
    // paging), so there is no scrollback move and no intent to record.
    vi.mocked(markTerminalPinnedViewport).mockClear()
    vi.mocked(syncTerminalScrollIntentFromViewport).mockClear()
    const target = {} as TerminalScrollIntentTarget
    const encodeKey = vi.fn(() => new Uint8Array([0x1b, 0x5b, 0x35, 0x3b, 0x32, 0x7e]))
    const h = mount({ encodeKey, isAltScreen: true }, undefined, undefined, () => target)

    fireKeydown(h.textarea, 'PageUp', { shiftKey: true })
    expect(h.scrollLines).not.toHaveBeenCalled()
    expect(markTerminalPinnedViewport).not.toHaveBeenCalled()
    expect(syncTerminalScrollIntentFromViewport).not.toHaveBeenCalled()
    h.dispose()
  })
})
