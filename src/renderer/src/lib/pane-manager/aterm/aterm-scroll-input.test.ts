/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it, vi } from 'vitest'
import { attachAtermScrollInput, type AtermScrollDeps } from './aterm-scroll-input'
import { setAtermMatrixRainActivity } from './aterm-effects-activity-gate'
import type { AtermTerminal } from './aterm_wasm.js'

// A configurable stand-in for the wasm terminal: the scroll module touches only
// the screen/tracking flags, scroll_lines, and the key encoder.
function fakeTerm(state: {
  altScreen: boolean
  tracking?: boolean
  appCursor?: boolean
  matrixRainEnabled?: boolean
}): {
  term: AtermTerminal
  scrollLines: ReturnType<typeof vi.fn>
  noteAltScroll: ReturnType<typeof vi.fn>
} {
  const scrollLines = vi.fn()
  const noteAltScroll = vi.fn()
  const term = {
    get is_alt_screen() {
      return state.altScreen
    },
    get is_mouse_tracking() {
      return state.tracking ?? false
    },
    scroll_lines: scrollLines,
    note_matrix_rain_alt_scroll: noteAltScroll,
    // DECCKM-shaped arrow encodings so the test proves the ENGINE encoder (not a
    // host table) produced the bytes the sink receives.
    encode_key: (key: string) => {
      const final = key === 'ArrowUp' ? 'A' : 'B'
      return new TextEncoder().encode(state.appCursor ? `\x1bO${final}` : `\x1b[${final}`)
    }
  } as unknown as AtermTerminal
  return { term, scrollLines, noteAltScroll }
}

type Harness = {
  canvas: HTMLCanvasElement
  inputSink: ReturnType<typeof vi.fn>
  scrollLines: ReturnType<typeof vi.fn>
  redraw: ReturnType<typeof vi.fn>
  noteAltScroll: ReturnType<typeof vi.fn>
  wheel: (init: WheelEventInit) => WheelEvent
  dispose: () => void
}

function mount(
  state: {
    altScreen: boolean
    tracking?: boolean
    appCursor?: boolean
    matrixRainEnabled?: boolean
  },
  options: Partial<AtermScrollDeps> = {}
): Harness {
  const canvas = document.createElement('canvas')
  document.body.appendChild(canvas)
  const { term, scrollLines, noteAltScroll } = fakeTerm(state)
  setAtermMatrixRainActivity(term, state.matrixRainEnabled ?? true)
  const inputSink = vi.fn()
  const redraw = vi.fn()
  const input = attachAtermScrollInput({
    canvas,
    term,
    metrics: { dpr: 1, cellWidth: 8, cellHeight: 16 },
    getRows: () => 24,
    redraw,
    isDisposed: () => false,
    inputSink,
    ...options
  })
  return {
    canvas,
    inputSink,
    scrollLines,
    redraw,
    noteAltScroll,
    wheel: (init) => {
      const event = new WheelEvent('wheel', { bubbles: true, cancelable: true, ...init })
      // happy-dom drops modifier keys from WheelEvent's init dict; set them so
      // the fast-scroll (Alt) path is exercised (real browser events carry them).
      if (init.altKey) {
        Object.defineProperty(event, 'altKey', { value: true, configurable: true })
      }
      canvas.dispatchEvent(event)
      return event
    },
    dispose: () => {
      input.dispose()
      canvas.remove()
    }
  }
}

const ESC = String.fromCharCode(27)

describe('aterm scroll input: alternate-screen wheel → arrow synthesis', () => {
  it('wheel down on the alt screen sends one ArrowDown per line (no scrollback move)', () => {
    const h = mount({ altScreen: true })
    const event = h.wheel({ deltaY: 2, deltaMode: 1 })
    expect(h.inputSink).toHaveBeenCalledTimes(1)
    expect(h.inputSink.mock.calls[0][0]).toBe(`${ESC}[B${ESC}[B`)
    expect(h.scrollLines).not.toHaveBeenCalled()
    expect(h.noteAltScroll).toHaveBeenCalledTimes(1)
    expect(event.defaultPrevented).toBe(true)
    h.dispose()
  })

  it('wheel up sends ArrowUp in the DECCKM (SS3) form the engine encoder chose', () => {
    const h = mount({ altScreen: true, appCursor: true })
    h.wheel({ deltaY: -1, deltaMode: 1 })
    expect(h.inputSink.mock.calls[0][0]).toBe(`${ESC}OA`)
    h.dispose()
  })

  it('applies the TUI multiplier to discrete wheel notches', () => {
    const h = mount({ altScreen: true }, { getTuiScrollMultiplier: () => 3 })
    h.wheel({ deltaY: 1, deltaMode: 1 })
    expect(h.inputSink.mock.calls[0][0]).toBe(`${ESC}[B`.repeat(3))
    h.dispose()
  })

  it('defers to the mouse forwarder when the TUI tracks the mouse', () => {
    const h = mount({ altScreen: true, tracking: true })
    const event = h.wheel({ deltaY: 1, deltaMode: 1 })
    expect(h.inputSink).not.toHaveBeenCalled()
    expect(h.noteAltScroll).toHaveBeenCalledTimes(1)
    expect(event.defaultPrevented).toBe(false)
    h.dispose()
  })

  it('does not cross the effects seam for alt-screen wheel while rain is off', () => {
    const h = mount({ altScreen: true, matrixRainEnabled: false })
    h.wheel({ deltaY: 1, deltaMode: 1 })
    expect(h.noteAltScroll).not.toHaveBeenCalled()
    h.dispose()
  })
})

describe('aterm scroll input: scrollback sensitivity', () => {
  it('scales scrollback lines by scrollSensitivity', () => {
    const h = mount({ altScreen: false }, { getScrollSensitivity: () => 2 })
    h.wheel({ deltaY: 3, deltaMode: 1 })
    // 3 lines * 2 = 6, wheel down → negative aterm delta.
    expect(h.scrollLines).toHaveBeenCalledWith(-6)
    expect(h.noteAltScroll).not.toHaveBeenCalled()
    expect(h.redraw).toHaveBeenCalled()
    h.dispose()
  })

  it('multiplies in fastScrollSensitivity only while Alt is held', () => {
    const h = mount(
      { altScreen: false },
      { getScrollSensitivity: () => 1, getFastScrollSensitivity: () => 5 }
    )
    h.wheel({ deltaY: 1, deltaMode: 1 })
    expect(h.scrollLines).toHaveBeenLastCalledWith(-1)
    h.wheel({ deltaY: 1, deltaMode: 1, altKey: true })
    expect(h.scrollLines).toHaveBeenLastCalledWith(-5)
    h.dispose()
  })
})
