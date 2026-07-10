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
  phases: boolean[]
  visibilities: ('focused' | 'visible_unfocused' | 'hidden')[]
}

function makeTerm(): RecordingTerm {
  const term: RecordingTerm = {
    hollow: [],
    inactive: [],
    phases: [],
    visibilities: [],
    set_cursor_blink_phase: (on: boolean) => {
      term.phases.push(on)
    },
    set_cursor_hollow: (h: boolean) => {
      term.hollow.push(h)
    },
    set_selection_inactive: (i: boolean) => {
      term.inactive.push(i)
    },
    set_effects_visibility: (state) => {
      term.visibilities.push(state)
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
    expect(term.visibilities.at(-1)).toBe('visible_unfocused')
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
    expect(term.visibilities.at(-1)).toBe('focused')

    // Blur → selection dims (inactive=true), cursor hollow.
    textarea.dispatchEvent(new FocusEvent('blur'))
    expect(term.inactive.at(-1)).toBe(true)
    expect(term.hollow.at(-1)).toBe(true)
    expect(term.visibilities.at(-1)).toBe('visible_unfocused')

    // Re-focus → selection active again (inactive=false), cursor solid.
    textarea.dispatchEvent(new FocusEvent('focus'))
    expect(term.inactive.at(-1)).toBe(false)
    expect(term.hollow.at(-1)).toBe(false)
    expect(term.visibilities.at(-1)).toBe('focused')

    blink.dispose()
  })

  it('reports hidden while drawing is suspended, regardless of DOM focus', () => {
    const term = makeTerm()
    const textarea = document.createElement('textarea')
    document.body.appendChild(textarea)
    textarea.focus()
    let suspended = false
    const blink = attachAtermCursorBlink({
      term,
      textarea,
      redraw: () => undefined,
      isDisposed: () => false,
      getCursorBlink: () => false,
      isDrawSuspended: () => suspended
    })

    expect(term.visibilities.at(-1)).toBe('focused')
    suspended = true
    blink.refreshEffectsVisibility()
    expect(term.visibilities.at(-1)).toBe('hidden')
    suspended = false
    blink.refreshEffectsVisibility()
    expect(term.visibilities.at(-1)).toBe('focused')
    blink.dispose()
  })

  it('refresh() applies a live terminalCursorBlink toggle to the FOCUSED pane', () => {
    // The setting is otherwise only read on focus events, so without refresh a
    // toggle skips the focused pane until the next blur/focus round-trip.
    const term = makeTerm()
    const textarea = document.createElement('textarea')
    document.body.appendChild(textarea)
    textarea.focus()
    let cursorBlink = false

    const blink = attachAtermCursorBlink({
      term,
      textarea,
      redraw: () => undefined,
      isDisposed: () => false,
      getCursorBlink: () => cursorBlink
    })

    // Blink off: no timer, phase stays solid-on.
    vi.advanceTimersByTime(2000)
    expect(term.phases.every((p) => p)).toBe(true)

    // Toggle on + refresh → the timer starts alternating the phase.
    cursorBlink = true
    blink.refresh()
    term.phases.length = 0
    vi.advanceTimersByTime(1100)
    expect(term.phases).toContain(false)

    // Toggle off + refresh → steady-on again (no further phase toggles).
    cursorBlink = false
    blink.refresh()
    term.phases.length = 0
    vi.advanceTimersByTime(2000)
    expect(term.phases.every((p) => p)).toBe(true)

    blink.dispose()
  })
})
