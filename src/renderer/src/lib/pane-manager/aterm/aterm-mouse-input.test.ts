/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it, vi } from 'vitest'
import { attachAtermMouseInput, shouldForwardMouse } from './aterm-mouse-input'
import { attachAtermSelectionInput } from './aterm-selection-input'
import type { AtermTerminal } from './aterm_wasm.js'

// A configurable stand-in for the wasm terminal: the mouse + selection modules
// only touch the tracking getters and the encoders / selection_* methods.
type FakeTermState = {
  tracking: boolean
  wantsMotion?: boolean
  wantsAnyMotion?: boolean
}

function fakeTerm(state: FakeTermState): {
  term: AtermTerminal
  selectionStart: ReturnType<typeof vi.fn>
} {
  const selectionStart = vi.fn()
  const term = {
    get is_mouse_tracking() {
      return state.tracking
    },
    get mouse_wants_motion() {
      return state.wantsMotion ?? false
    },
    get mouse_wants_any_motion() {
      return state.wantsAnyMotion ?? false
    },
    // SGR-shaped report bytes so the test asserts a recognizable payload.
    encode_mouse_press: (col: number, row: number, button: number) =>
      new TextEncoder().encode(`\x1b[<${button};${col + 1};${row + 1}M`),
    encode_mouse_release: (col: number, row: number, button: number) =>
      new TextEncoder().encode(`\x1b[<${button};${col + 1};${row + 1}m`),
    encode_mouse_motion: (col: number, row: number, button: number) =>
      new TextEncoder().encode(`\x1b[<${button + 32};${col + 1};${row + 1}M`),
    encode_mouse_wheel: (col: number, row: number, up: boolean) =>
      new TextEncoder().encode(`\x1b[<${up ? 64 : 65};${col + 1};${row + 1}M`),
    // Selection methods the selection module calls; only start matters here.
    selection_clear: vi.fn(),
    selection_start: selectionStart,
    selection_extend: vi.fn(),
    selection_finish: vi.fn(),
    selection_text: () => ''
  } as unknown as AtermTerminal
  return { term, selectionStart }
}

type Harness = {
  canvas: HTMLCanvasElement
  inputSink: ReturnType<typeof vi.fn>
  selectionStart: ReturnType<typeof vi.fn>
  dispose: () => void
}

// Mount the mouse forwarder AND the selection input over one canvas so the test
// proves the gate end-to-end: a forwarded press must NOT start a selection.
function mount(state: FakeTermState): Harness {
  const canvas = document.createElement('canvas')
  document.body.appendChild(canvas)
  // happy-dom gives a 0x0 rect; cell sizes are arbitrary (col/row land at 0).
  const { term, selectionStart } = fakeTerm(state)
  const inputSink = vi.fn()
  const mouse = attachAtermMouseInput({
    canvas,
    term,
    metrics: { dpr: 1, cellWidth: 8, cellHeight: 16 },
    getRows: () => 24,
    inputSink,
    isDisposed: () => false
  })
  const selection = attachAtermSelectionInput({
    canvas,
    term,
    dpr: 1,
    cellWidth: 8,
    cellHeight: 16,
    redraw: () => {},
    isDisposed: () => false,
    onCopy: () => {}
  })
  return {
    canvas,
    inputSink,
    selectionStart,
    dispose: () => {
      mouse.dispose()
      selection.dispose()
      canvas.remove()
    }
  }
}

// ESC built from a char code so no control-char literal appears in a regex
// (no-control-regex). The SGR mouse report shape is ESC [ < Cb ; Cx ; Cy {M|m}.
const ESC = String.fromCharCode(27)
// Assert an SGR report with button code `cb` and final byte `final` ('M'/'m'),
// using string ops (no control-char regex literal).
function expectSgrReport(sent: unknown, cb: number, final: 'M' | 'm'): void {
  expect(typeof sent).toBe('string')
  const s = sent as string
  expect(s.startsWith(`${ESC}[<${cb};`)).toBe(true)
  expect(s.endsWith(final)).toBe(true)
  // Body between the prefix and final byte is "Cx;Cy" (two decimal coords).
  const body = s.slice(`${ESC}[<${cb};`.length, -1)
  expect(body).toMatch(/^\d+;\d+$/)
}

function mousedown(canvas: HTMLCanvasElement, init: MouseEventInit = {}): MouseEvent {
  const event = new MouseEvent('mousedown', {
    button: 0,
    bubbles: true,
    cancelable: true,
    ...init
  })
  canvas.dispatchEvent(event)
  return event
}

describe('aterm mouse forwarding gate', () => {
  it('tracking + no Shift: forwards a mouse report and suppresses selection', () => {
    const h = mount({ tracking: true })
    const event = mousedown(h.canvas)
    // The press was encoded and sent to the PTY.
    expect(h.inputSink).toHaveBeenCalledTimes(1)
    expectSgrReport(h.inputSink.mock.calls[0][0], 0, 'M')
    // Selection must NOT start for a forwarded press.
    expect(h.selectionStart).not.toHaveBeenCalled()
    // The event is consumed so nothing else acts on it.
    expect(event.defaultPrevented).toBe(true)
    h.dispose()
  })

  it('tracking + Shift held: does NOT forward; selection runs (user override)', () => {
    const h = mount({ tracking: true })
    mousedown(h.canvas, { shiftKey: true })
    expect(h.inputSink).not.toHaveBeenCalled()
    expect(h.selectionStart).toHaveBeenCalledTimes(1)
    h.dispose()
  })

  it('not tracking: does NOT forward; selection runs', () => {
    const h = mount({ tracking: false })
    mousedown(h.canvas)
    expect(h.inputSink).not.toHaveBeenCalled()
    expect(h.selectionStart).toHaveBeenCalledTimes(1)
    h.dispose()
  })

  it('wheel while tracking forwards a wheel report instead of scrolling', () => {
    const h = mount({ tracking: true })
    // deltaMode 1 (line) so one whole line accumulates from a single event.
    const wheel = new WheelEvent('wheel', {
      deltaY: -1,
      deltaMode: 1,
      bubbles: true,
      cancelable: true
    })
    // happy-dom drops clientX/Y from WheelEvent's init dict; set them so
    // pointToCell yields a real cell (real browser events carry these).
    Object.defineProperty(wheel, 'clientX', { value: 0, configurable: true })
    Object.defineProperty(wheel, 'clientY', { value: 0, configurable: true })
    h.canvas.dispatchEvent(wheel)
    expect(h.inputSink).toHaveBeenCalledTimes(1)
    // Wheel-up (negative deltaY) → button 64.
    expectSgrReport(h.inputSink.mock.calls[0][0], 64, 'M')
    expect(wheel.defaultPrevented).toBe(true)
    h.dispose()
  })

  it('Normal mode (no motion wanted) does not forward bare mousemove', () => {
    const h = mount({ tracking: true, wantsMotion: false })
    const move = new MouseEvent('mousemove', { bubbles: true, cancelable: true })
    window.dispatchEvent(move)
    expect(h.inputSink).not.toHaveBeenCalled()
    h.dispose()
  })

  it('AnyEvent mode forwards a button-less mousemove report', () => {
    const h = mount({ tracking: true, wantsMotion: true, wantsAnyMotion: true })
    const move = new MouseEvent('mousemove', { bubbles: true, cancelable: true })
    window.dispatchEvent(move)
    expect(h.inputSink).toHaveBeenCalledTimes(1)
    // No button held → button code 3 → encoded as 3+32 = 35 by the fake.
    expectSgrReport(h.inputSink.mock.calls[0][0], 35, 'M')
    h.dispose()
  })

  it('shouldForwardMouse: true only when tracking AND no Shift', () => {
    const { term } = fakeTerm({ tracking: true })
    expect(shouldForwardMouse(term, { shiftKey: false })).toBe(true)
    expect(shouldForwardMouse(term, { shiftKey: true })).toBe(false)
    const { term: off } = fakeTerm({ tracking: false })
    expect(shouldForwardMouse(off, { shiftKey: false })).toBe(false)
  })
})
