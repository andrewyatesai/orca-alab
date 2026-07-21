/**
 * @vitest-environment happy-dom
 */
// Repro pins for upstream #6698 (Vietnamese Telex loses the composed syllable:
// "chinhs" → "ch") and the #9235 family (duplicate/lost IME commits). macOS
// Telex and Korean IMEs deliver the committed string in several orderings
// around compositionend; each ordering must reach the PTY exactly once.
import { describe, expect, it, vi } from 'vitest'
import {
  ATERM_IME_DUPLICATE_COMMIT_ABSORB_WINDOW_MS,
  attachAtermTextareaInput
} from './aterm-textarea-input'
import type { AtermTerminal } from './aterm_wasm.js'

// Keep these DOM tests off the real (uninitialized) wasm module.
vi.mock('./aterm_wasm.js', () => ({
  encode_key_with_mode: vi.fn(() => new Uint8Array(0))
}))

type Harness = {
  textarea: HTMLTextAreaElement
  inputSink: ReturnType<typeof vi.fn>
  pasteSink: ReturnType<typeof vi.fn>
  dispose: () => void
}

function mount(): Harness {
  const wrapper = document.createElement('div')
  const canvas = document.createElement('canvas')
  // happy-dom does no layout; the composition view derives its viewport from
  // the canvas CSS box.
  Object.defineProperty(canvas, 'clientWidth', { value: 400 })
  Object.defineProperty(canvas, 'clientHeight', { value: 240 })
  const textarea = document.createElement('textarea')
  wrapper.appendChild(canvas)
  wrapper.appendChild(textarea)
  document.body.appendChild(wrapper)
  const inputSink = vi.fn()
  const pasteSink = vi.fn()
  const term = {
    is_alt_screen: false,
    keyboard_mode_bits: 0,
    scroll_lines: vi.fn(),
    cursor_x: 0,
    cursor_y: 0
  } as unknown as AtermTerminal
  const { dispose } = attachAtermTextareaInput({
    textarea,
    term,
    canvas,
    metrics: { dpr: 2, cellWidth: 10, cellHeight: 20 },
    themeColors: { fg: 0xffffff, bg: 0x000000 },
    getRows: () => 24,
    redraw: vi.fn(),
    inputSink,
    pasteSink,
    copySelection: vi.fn(() => false)
  })
  return { textarea, inputSink, pasteSink, dispose }
}

function fireInput(
  textarea: HTMLTextAreaElement,
  data: string,
  inputType: string,
  options: { isComposing?: boolean } = {}
): void {
  const event = new InputEvent('input', { data, inputType, bubbles: true })
  // happy-dom does not reliably carry isComposing through the init dict.
  Object.defineProperty(event, 'isComposing', {
    value: options.isComposing ?? false,
    configurable: true
  })
  textarea.dispatchEvent(event)
}

// happy-dom drops CompositionEvent.data from the init dict; define it directly.
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

function sentStrings(inputSink: ReturnType<typeof vi.fn>): string[] {
  return inputSink.mock.calls.map((call) => call[0] as string)
}

describe('aterm IME commit ordering (Telex + Korean)', () => {
  it('forwards a Telex commit delivered as a non-composing insertText BEFORE compositionend (#6698 "chinhs" → "chính")', () => {
    const h = mount()
    // 'c' and 'h' commit directly (consonants outside the marked syllable).
    fireInput(h.textarea, 'c', 'insertText')
    fireInput(h.textarea, 'h', 'insertText')
    // The syllable composes as marked text; the tone key rewrites the preedit.
    fireComposition(h.textarea, 'compositionstart')
    for (const preedit of ['i', 'in', 'inh', 'ính']) {
      fireComposition(h.textarea, 'compositionupdate', preedit)
    }
    // macOS Telex commits via a non-composing insertText that outruns the
    // trailing compositionend; dropping it is exactly the reported data loss.
    fireInput(h.textarea, 'ính', 'insertText', { isComposing: false })
    fireComposition(h.textarea, 'compositionend', '')
    expect(sentStrings(h.inputSink)).toEqual(['c', 'h', 'ính'])
    h.dispose()
  })

  it('does not re-send the commit when compositionend repeats the early-forwarded text', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    fireComposition(h.textarea, 'compositionupdate', 'ính')
    fireInput(h.textarea, 'ính', 'insertText', { isComposing: false })
    fireComposition(h.textarea, 'compositionend', 'ính')
    expect(sentStrings(h.inputSink)).toEqual(['ính'])
    h.dispose()
  })

  it('sends a distinct compositionend commit that follows an early-forwarded insertText', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    fireInput(h.textarea, 'ê', 'insertText', { isComposing: false })
    // A different committed string is a second, real commit — not an echo.
    fireComposition(h.textarea, 'compositionend', 'ất')
    expect(sentStrings(h.inputSink)).toEqual(['ê', 'ất'])
    h.dispose()
  })

  it('absorbs the IME echo of a compositionend commit exactly once (Korean IBus restore)', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    fireComposition(h.textarea, 'compositionupdate', '한')
    fireComposition(h.textarea, 'compositionend', '한')
    // IBus restores the same commit as a plain insertText right after end.
    fireInput(h.textarea, '한', 'insertText')
    expect(sentStrings(h.inputSink)).toEqual(['한'])
    h.dispose()
  })

  it('still sends a commit delivered only AFTER an empty compositionend (IBus clears at end)', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    fireComposition(h.textarea, 'compositionupdate', '한')
    fireComposition(h.textarea, 'compositionend', '')
    fireInput(h.textarea, '한', 'insertText')
    expect(sentStrings(h.inputSink)).toEqual(['한'])
    h.dispose()
  })

  it('commits the standard composing flow exactly once', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    fireComposition(h.textarea, 'compositionupdate', '가한')
    // In-progress composition inserts are insertCompositionText/isComposing=true.
    fireInput(h.textarea, '가한', 'insertCompositionText', { isComposing: true })
    fireComposition(h.textarea, 'compositionend', '가한')
    expect(sentStrings(h.inputSink)).toEqual(['가한'])
    h.dispose()
  })

  it('keeps suppressing in-progress composing inserts (isComposing=true) until compositionend', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    fireInput(h.textarea, 'n', 'insertText', { isComposing: true })
    expect(h.inputSink).not.toHaveBeenCalled()
    h.dispose()
  })

  it('disarms the echo absorb on a physical keydown so identical real typing is preserved', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    fireComposition(h.textarea, 'compositionend', '한')
    // A real key press cycle precedes any user-typed insertText; the identical
    // text after it is the user's input, not the IME echo.
    h.textarea.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'a', bubbles: true, cancelable: true })
    )
    fireInput(h.textarea, '한', 'insertText')
    expect(sentStrings(h.inputSink)).toEqual(['한', '한'])
    h.dispose()
  })

  it('expires the echo absorb window so a later identical insertText is sent', () => {
    vi.useFakeTimers()
    try {
      const h = mount()
      fireComposition(h.textarea, 'compositionstart')
      fireComposition(h.textarea, 'compositionend', '한')
      vi.advanceTimersByTime(ATERM_IME_DUPLICATE_COMMIT_ABSORB_WINDOW_MS + 1)
      fireInput(h.textarea, '한', 'insertText')
      expect(sentStrings(h.inputSink)).toEqual(['한', '한'])
      h.dispose()
    } finally {
      vi.useRealTimers()
    }
  })

  it('spends the echo absorb on a different insertText (typing resumed) instead of a later echo', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    fireComposition(h.textarea, 'compositionend', '한')
    fireInput(h.textarea, 'a', 'insertText')
    // The absorb credit is gone: an identical string now is real input.
    fireInput(h.textarea, '한', 'insertText')
    expect(sentStrings(h.inputSink)).toEqual(['한', 'a', '한'])
    h.dispose()
  })

  it('routes a paste fired mid-composition to the paste sink without disturbing the commit', () => {
    const h = mount()
    fireComposition(h.textarea, 'compositionstart')
    fireInput(h.textarea, 'PASTED', 'insertFromPaste')
    fireComposition(h.textarea, 'compositionend', 'ính')
    expect(h.pasteSink).toHaveBeenCalledWith('PASTED')
    expect(sentStrings(h.inputSink)).toEqual(['ính'])
    h.dispose()
  })
})
