/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it, vi } from 'vitest'
import { attachAtermScrollInput, type AtermScrollDeps } from './aterm-scroll-input'
import { setAtermMatrixRainActivity } from './aterm-effects-activity-gate'
import type { AtermTerminal } from './aterm_wasm.js'

// A configurable stand-in for the wasm terminal: the scroll module touches only
// the screen/tracking flags, scroll_lines/scroll_px, and the key encoder.
function fakeTerm(state: {
  altScreen: boolean
  tracking?: boolean
  appCursor?: boolean
  matrixRainEnabled?: boolean
  /** Simulate an engine artifact WITHOUT the scroll_px export (blob skew). */
  noScrollPx?: boolean
}): {
  term: AtermTerminal
  scrollLines: ReturnType<typeof vi.fn>
  scrollPx: ReturnType<typeof vi.fn>
  noteAltScroll: ReturnType<typeof vi.fn>
} {
  const scrollLines = vi.fn()
  const scrollPx = vi.fn()
  const noteAltScroll = vi.fn()
  const term = {
    get is_alt_screen() {
      return state.altScreen
    },
    get is_mouse_tracking() {
      return state.tracking ?? false
    },
    scroll_lines: scrollLines,
    ...(state.noScrollPx ? {} : { scroll_px: scrollPx }),
    note_matrix_rain_alt_scroll: noteAltScroll,
    // DECCKM-shaped arrow encodings so the test proves the ENGINE encoder (not a
    // host table) produced the bytes the sink receives.
    encode_key: (key: string) => {
      const final = key === 'ArrowUp' ? 'A' : 'B'
      return new TextEncoder().encode(state.appCursor ? `\x1bO${final}` : `\x1b[${final}`)
    }
  } as unknown as AtermTerminal
  return { term, scrollLines, scrollPx, noteAltScroll }
}

type Harness = {
  canvas: HTMLCanvasElement
  inputSink: ReturnType<typeof vi.fn>
  scrollLines: ReturnType<typeof vi.fn>
  scrollPx: ReturnType<typeof vi.fn>
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
    noScrollPx?: boolean
  },
  options: Partial<AtermScrollDeps> = {}
): Harness {
  const canvas = document.createElement('canvas')
  document.body.appendChild(canvas)
  const { term, scrollLines, scrollPx, noteAltScroll } = fakeTerm(state)
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
    scrollPx,
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

describe('aterm scroll input: pixel-mode scrollback → engine scroll_px (P3)', () => {
  it('routes device px (deltaY × dpr) to scroll_px, sign-flipped, without line rounding', () => {
    const h = mount({ altScreen: false }, { metrics: { dpr: 2, cellWidth: 8, cellHeight: 16 } })
    const event = h.wheel({ deltaY: 24, deltaMode: 0 })
    expect(h.scrollPx).toHaveBeenCalledWith(-48)
    expect(h.scrollLines).not.toHaveBeenCalled()
    expect(h.redraw).toHaveBeenCalled()
    expect(event.defaultPrevented).toBe(true)
    h.dispose()
  })

  it('forwards every fractional sub-line delta unrounded — no JS remainder bank', () => {
    const h = mount({ altScreen: false })
    h.wheel({ deltaY: 5, deltaMode: 0 })
    h.wheel({ deltaY: 5, deltaMode: 0 })
    h.wheel({ deltaY: 5, deltaMode: 0 })
    // 3×5px on a 16px cell: the old JS accumulator would have swallowed all three.
    expect(h.scrollPx.mock.calls.map((c) => c[0])).toEqual([-5, -5, -5])
    expect(h.scrollLines).not.toHaveBeenCalled()
    h.dispose()
  })

  it('wheel up (negative deltaY) reveals older lines → positive engine delta', () => {
    const h = mount({ altScreen: false })
    h.wheel({ deltaY: -8, deltaMode: 0 })
    expect(h.scrollPx).toHaveBeenCalledWith(8)
    h.dispose()
  })

  it('applies scrollSensitivity, times fastScrollSensitivity while Alt is held', () => {
    const h = mount(
      { altScreen: false },
      { getScrollSensitivity: () => 2, getFastScrollSensitivity: () => 5 }
    )
    h.wheel({ deltaY: 10, deltaMode: 0 })
    expect(h.scrollPx).toHaveBeenLastCalledWith(-20)
    h.wheel({ deltaY: 10, deltaMode: 0, altKey: true })
    expect(h.scrollPx).toHaveBeenLastCalledWith(-100)
    h.dispose()
  })

  it('a zero delta neither scrolls nor redraws', () => {
    const h = mount({ altScreen: false })
    h.wheel({ deltaY: 0, deltaMode: 0 })
    expect(h.scrollPx).not.toHaveBeenCalled()
    expect(h.redraw).not.toHaveBeenCalled()
    h.dispose()
  })

  it('defers to the mouse forwarder while the app tracks the mouse', () => {
    const h = mount({ altScreen: false, tracking: true })
    const event = h.wheel({ deltaY: 8, deltaMode: 0 })
    expect(h.scrollPx).not.toHaveBeenCalled()
    expect(event.defaultPrevented).toBe(false)
    h.dispose()
  })

  it('alt-screen pixel deltas still accumulate to whole arrow presses (JS remainder kept)', () => {
    const h = mount({ altScreen: true })
    h.wheel({ deltaY: 8, deltaMode: 0 })
    expect(h.inputSink).not.toHaveBeenCalled() // 0.5 line banked in JS
    h.wheel({ deltaY: 8, deltaMode: 0 })
    expect(h.inputSink).toHaveBeenCalledTimes(1)
    expect(h.inputSink.mock.calls[0][0]).toBe(`${ESC}[B`)
    expect(h.scrollPx).not.toHaveBeenCalled()
    h.dispose()
  })

  it('line mode still rounds through scroll_lines with the JS remainder', () => {
    const h = mount({ altScreen: false })
    h.wheel({ deltaY: 0.5, deltaMode: 1 })
    expect(h.scrollLines).not.toHaveBeenCalled() // 0.5 banked
    h.wheel({ deltaY: 0.5, deltaMode: 1 })
    expect(h.scrollLines).toHaveBeenCalledWith(-1)
    expect(h.scrollPx).not.toHaveBeenCalled()
    h.dispose()
  })

  it('page mode still scales by rows through scroll_lines', () => {
    const h = mount({ altScreen: false })
    h.wheel({ deltaY: 1, deltaMode: 2 })
    expect(h.scrollLines).toHaveBeenCalledWith(-24)
    expect(h.scrollPx).not.toHaveBeenCalled()
    h.dispose()
  })

  it('fails closed to line-rounding when the engine lacks the scroll_px export', () => {
    const h = mount({ altScreen: false, noScrollPx: true })
    h.wheel({ deltaY: 8, deltaMode: 0 })
    expect(h.scrollLines).not.toHaveBeenCalled() // 0.5 line banked in JS
    h.wheel({ deltaY: 8, deltaMode: 0 })
    expect(h.scrollLines).toHaveBeenCalledWith(-1)
    h.dispose()
  })
})
