/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { attachAtermCursorBlink, type AtermCursorTarget } from './aterm-cursor-blink'

// Proves pane focus drives the engine's inactive-selection flag (xterm's
// selectionInactiveBackground behavior) on the SAME focus/blur transition that
// drives the hollow cursor: blur → set_selection_inactive(true), focus → false.

type RecordingTerm = AtermCursorTarget & {
  hollow: boolean[]
  inactive: boolean[]
}

function makeTerm(): RecordingTerm {
  const term: RecordingTerm = {
    hollow: [],
    inactive: [],
    set_cursor_blink_phase: () => undefined,
    set_cursor_hollow: (h: boolean) => {
      term.hollow.push(h)
    },
    set_selection_inactive: (i: boolean) => {
      term.inactive.push(i)
    }
  }
  return term
}

describe('attachAtermCursorBlink — inactive-selection focus wiring', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    vi.useRealTimers()
    document.body.innerHTML = ''
  })

  it('seeds inactive selection when the pane starts UNFOCUSED', () => {
    const term = makeTerm()
    const textarea = document.createElement('textarea')
    document.body.appendChild(textarea) // not focused (activeElement !== textarea)

    const blink = attachAtermCursorBlink({
      term,
      textarea,
      redraw: () => undefined,
      isDisposed: () => false,
      getCursorBlink: () => false
    })

    // Unfocused seed: hollow cursor AND inactive selection both turned on.
    expect(term.hollow.at(-1)).toBe(true)
    expect(term.inactive.at(-1)).toBe(true)
    blink.dispose()
  })

  it('toggles the inactive-selection flag with focus/blur, alongside the hollow cursor', () => {
    const term = makeTerm()
    const textarea = document.createElement('textarea')
    document.body.appendChild(textarea)
    textarea.focus()

    const blink = attachAtermCursorBlink({
      term,
      textarea,
      redraw: () => undefined,
      isDisposed: () => false,
      getCursorBlink: () => false
    })

    // Focused seed → selection active (inactive=false), cursor solid (hollow=false).
    expect(term.inactive.at(-1)).toBe(false)
    expect(term.hollow.at(-1)).toBe(false)

    // Blur → selection dims (inactive=true), cursor hollow.
    textarea.dispatchEvent(new FocusEvent('blur'))
    expect(term.inactive.at(-1)).toBe(true)
    expect(term.hollow.at(-1)).toBe(true)

    // Re-focus → selection active again (inactive=false), cursor solid.
    textarea.dispatchEvent(new FocusEvent('focus'))
    expect(term.inactive.at(-1)).toBe(false)
    expect(term.hollow.at(-1)).toBe(false)

    blink.dispose()
  })
})
