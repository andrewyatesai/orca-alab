/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it, vi } from 'vitest'
import { attachAtermTextareaInput } from './aterm-textarea-input'
import type { AtermTerminal } from './aterm_wasm.js'

// The textarea-input module only reads is_app_cursor_mode off the terminal; a
// minimal stand-in keeps these DOM tests off the wasm engine.
function fakeTerm(): AtermTerminal {
  return { is_app_cursor_mode: false } as unknown as AtermTerminal
}

type Harness = {
  textarea: HTMLTextAreaElement
  inputSink: ReturnType<typeof vi.fn>
  copySelection: ReturnType<typeof vi.fn>
  dispose: () => void
}

function mount(getMacOptionIsMeta?: () => boolean): Harness {
  const textarea = document.createElement('textarea')
  document.body.appendChild(textarea)
  const inputSink = vi.fn()
  const copySelection = vi.fn(() => false)
  const { dispose } = attachAtermTextareaInput({
    textarea,
    term: fakeTerm(),
    inputSink,
    copySelection,
    getMacOptionIsMeta
  })
  return { textarea, inputSink, copySelection, dispose }
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
  it('sends the InputEvent data for an insertText (typed character)', () => {
    const h = mount()
    fireInput(h.textarea, 'x', 'insertText')
    expect(h.inputSink).toHaveBeenCalledWith('x')
    expect(h.textarea.value).toBe('') // cleared so it never accumulates
    h.dispose()
  })

  it('delivers an insertFromPaste InputEvent even while an IME composition is open (M1)', () => {
    const h = mount()
    // Open a local composition (sets the module's composing flag).
    fireComposition(h.textarea, 'compositionstart')
    // A programmatic paste fires insertFromPaste with isComposing=false; it must
    // still reach the PTY despite the open composition.
    fireInput(h.textarea, 'PASTED', 'insertFromPaste')
    expect(h.inputSink).toHaveBeenCalledWith('PASTED')
    h.dispose()
  })

  it('suppresses a genuine composing insertText input until compositionend', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    // An in-progress IME keystroke (insertText) while composing must NOT be sent.
    fireInput(h.textarea, 'n', 'insertText')
    expect(h.inputSink).not.toHaveBeenCalled()
    h.dispose()
  })

  it('falls back to textarea.value when the InputEvent data is null (chunked paste tail)', () => {
    const h = mount()
    // Chunked paste mutates value then fires a null-data InputEvent.
    h.textarea.value = 'chunked-tail'
    fireInput(h.textarea, null, 'insertFromPaste')
    expect(h.inputSink).toHaveBeenCalledWith('chunked-tail')
    h.dispose()
  })

  it('sends the committed string exactly once on compositionend (no double-send)', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    // compositionupdate must not send (no handler); only compositionend commits.
    fireComposition(h.textarea, 'compositionupdate', 'にほ')
    fireComposition(h.textarea, 'compositionend', 'にほん')
    expect(h.inputSink).toHaveBeenCalledTimes(1)
    expect(h.inputSink).toHaveBeenCalledWith('にほん')
    h.dispose()
  })

  it('does not double-send a printable: keydown returns null and only input sends', () => {
    const h = mount()
    // A plain printable keydown returns null from the encoder (not preventDefault'd
    // / not sent); the character arrives via the subsequent input event only.
    const keydown = new KeyboardEvent('keydown', { key: 'a', bubbles: true, cancelable: true })
    h.textarea.dispatchEvent(keydown)
    expect(h.inputSink).not.toHaveBeenCalled()
    expect(keydown.defaultPrevented).toBe(false)
    fireInput(h.textarea, 'a', 'insertText')
    expect(h.inputSink).toHaveBeenCalledTimes(1)
    expect(h.inputSink).toHaveBeenCalledWith('a')
    h.dispose()
  })

  it('sends a non-text key (Enter) via keydown and preventDefaults it', () => {
    const h = mount()
    const keydown = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })
    h.textarea.dispatchEvent(keydown)
    expect(h.inputSink).toHaveBeenCalledWith('\r')
    expect(keydown.defaultPrevented).toBe(true)
    h.dispose()
  })
})
