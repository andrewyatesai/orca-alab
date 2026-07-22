/**
 * @vitest-environment happy-dom
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createWorkerTerminal } from './aterm-worker-terminal'
import { createWorkerBackedTerm } from './aterm-worker-term'
import { createAtermPredictionEcho } from './aterm-prediction-echo'
import { createAtermWorkerOverlay } from './aterm-worker-overlay'
import { dispatchPaneCommand, type PaneRuntime } from './aterm-worker-pane-dispatch'
import type { EngineHandle } from './aterm-worker-engine-build'
import type { AtermWorkerPaneCommand, AtermWorkerState } from './aterm-render-worker-protocol'

// R1 — predictive echo on the DEFAULT off-main worker render path. Before this the
// worker facade exposed no predict_* methods, so the controller's capability probe saw
// engine=null and every seam was an inert no-op (zero local echo → full PTY round-trip,
// the reported SSH typing lag). These prove prediction now fires across the seam:
// command posts, worker dispatch + reconcile, buildState reflection, deadline re-arm,
// and the ghost overlay draw.

// A minimal engine that also survives buildState (rows=0 keeps the dirty-row loop empty;
// an idle search returns 0/null/[]). Predict methods are spies so we can assert the seam.
function makePredictEngine() {
  const set_predictive_echo = vi.fn()
  const predict_char = vi.fn(() => true)
  const predict_backspace = vi.fn(() => true)
  const predict_line_submit = vi.fn()
  const predict_reconcile = vi.fn()
  const predict_reset = vi.fn()
  const predict_overlay = vi.fn(() => new Uint32Array([0, 1, 0x61])) // ghost 'a' at (0,1)
  const predict_next_deadline_ms = vi.fn(() => 250)
  const engine = {
    // scalars buildState reads
    cell_width: 8,
    cell_height: 16,
    cursor_x: 0,
    cursor_y: 0,
    cursor_style: 1,
    cursor_color: undefined,
    base_y: 0,
    display_offset: 0,
    display_origin_absolute: 0,
    is_alt_screen: false,
    bracketed_paste_mode: false,
    is_mouse_tracking: false,
    mouse_wants_motion: false,
    mouse_wants_any_motion: false,
    is_focus_event_mode: false,
    is_color_scheme_updates_mode: false,
    is_app_cursor_mode: false,
    is_alternate_scroll: false,
    keyboard_mode_bits: 0,
    title: () => null,
    selection_range: () => undefined,
    selection_text: () => '',
    take_response: () => undefined,
    take_osc_events: () => undefined,
    take_notifications: () => undefined,
    drain_bell: () => false,
    scroll_to_bottom: () => undefined,
    set_predictive_echo,
    predict_char,
    predict_backspace,
    predict_line_submit,
    predict_reconcile,
    predict_reset,
    predict_overlay,
    predict_next_deadline_ms
  }
  const handle = {
    kind: 'cpu',
    engine: engine as unknown as EngineHandle['engine'],
    memory: { buffer: new ArrayBuffer(0) } as unknown as WebAssembly.Memory,
    process: () => undefined,
    render: () => undefined,
    framebuffer: () => ({ width: 8, height: 16 }),
    search: () => new Uint32Array(0),
    dispose: () => undefined
  } as EngineHandle
  const spies = {
    set_predictive_echo,
    predict_char,
    predict_backspace,
    predict_line_submit,
    predict_reconcile,
    predict_reset,
    predict_overlay
  }
  return { handle, spies }
}

function makePane(term: ReturnType<typeof createWorkerTerminal>): {
  pane: PaneRuntime
  scheduleDraw: ReturnType<typeof vi.fn>
} {
  const scheduleDraw = vi.fn()
  const pane = {
    paneId: 1,
    term,
    engineSetters: null,
    engine: null,
    engineKind: 'cpu',
    engineMemory: null,
    storedInit: null,
    canvas: null,
    fellBackToCpu: false,
    disposed: false,
    chrome: { pad: 0, head: 0 },
    frameScheduler: {
      schedule: scheduleDraw,
      presentNow: vi.fn(),
      setSuspended: vi.fn()
    },
    serializeCache: { schedule: () => undefined, dispose: () => undefined },
    post: () => undefined
  } as unknown as PaneRuntime
  return { pane, scheduleDraw }
}

describe('R1 worker predictive-echo seam — command posts (facade no longer inert)', () => {
  it('exposes predict_* so the controller probe passes and posts each keystroke seam', () => {
    const posted: AtermWorkerPaneCommand[] = []
    const backed = createWorkerBackedTerm({
      post: (cmd) => posted.push(cmd),
      initial: makeWorkerState()
    })
    const prediction = createAtermPredictionEcho({
      term: backed.term,
      requestPaint: () => undefined,
      isDisposed: () => false
    })
    // 'always' proves the probe passed (an inert no-op controller would post nothing).
    prediction.setMode('always')
    prediction.noteChar('l')
    prediction.noteBackspace()
    prediction.noteSubmit()
    prediction.reset()
    expect(posted).toEqual([
      { type: 'predictSetMode', mode: 'always' },
      { type: 'predictChar', ch: 'l' },
      { type: 'predictBackspace' },
      { type: 'predictSubmit' },
      { type: 'predictReset' }
    ])
  })

  it('setMode(off) posts the disable and clears the reflected deadline', () => {
    const posted: AtermWorkerPaneCommand[] = []
    const backed = createWorkerBackedTerm({
      post: (cmd) => posted.push(cmd),
      initial: makeWorkerState()
    })
    const prediction = createAtermPredictionEcho({
      term: backed.term,
      requestPaint: () => undefined,
      isDisposed: () => false
    })
    prediction.setMode('off')
    expect(posted).toContainEqual({ type: 'predictSetMode', mode: 'off' })
  })
})

describe('R1 worker predictive-echo seam — worker dispatch + reconcile + reflection', () => {
  it('routes predict commands to the engine predictor', () => {
    const { handle, spies } = makePredictEngine()
    const term = createWorkerTerminal(handle)
    const { pane } = makePane(term)
    dispatchPaneCommand(pane, { type: 'predictSetMode', mode: 'always' })
    dispatchPaneCommand(pane, { type: 'predictChar', ch: 'l' })
    dispatchPaneCommand(pane, { type: 'predictBackspace' })
    dispatchPaneCommand(pane, { type: 'predictSubmit' })
    dispatchPaneCommand(pane, { type: 'predictReset' })
    expect(spies.set_predictive_echo).toHaveBeenCalledWith('always')
    expect(spies.predict_char).toHaveBeenCalledWith('l')
    expect(spies.predict_backspace).toHaveBeenCalledTimes(1)
    expect(spies.predict_line_submit).toHaveBeenCalledTimes(1)
    expect(spies.predict_reset).toHaveBeenCalledTimes(1)
  })

  it('reconciles guesses AFTER a process chunk once enabled (never while off)', () => {
    const { handle, spies } = makePredictEngine()
    const term = createWorkerTerminal(handle)
    const { pane } = makePane(term)
    // Off by default: a chunk must not cross the wasm boundary to reconcile.
    dispatchPaneCommand(pane, { type: 'process', data: 'x' })
    expect(spies.predict_reconcile).not.toHaveBeenCalled()
    // Enabled: every chunk reconciles so confirmed ghosts retire / divergence flushes.
    dispatchPaneCommand(pane, { type: 'predictSetMode', mode: 'adaptive' })
    dispatchPaneCommand(pane, { type: 'process', data: 'x' })
    expect(spies.predict_reconcile).toHaveBeenCalledTimes(1)
  })

  it('reflects the ghost overlay + glitch deadline in STATE only while enabled', () => {
    const { handle, spies } = makePredictEngine()
    const term = createWorkerTerminal(handle) // rows=0 keeps buildState's dirty-row loop empty
    // Off: no per-frame wasm predictor calls; STATE carries the inert defaults.
    let state = term.buildState()
    expect(state.predictOverlay).toHaveLength(0)
    expect(state.predictDeadlineMs).toBeNull()
    expect(spies.predict_overlay).not.toHaveBeenCalled()
    // Enabled: STATE mirrors predict_overlay() + predict_next_deadline_ms().
    term.predict.setMode('always')
    state = term.buildState()
    expect(Array.from(state.predictOverlay)).toEqual([0, 1, 0x61])
    expect(state.predictDeadlineMs).toBe(250)
  })
})

describe('R1 worker predictive-echo seam — deadline reflection re-arms the ONE timer', () => {
  afterEach(() => vi.useRealTimers())

  it('applyState fires onPredictDeadline with the fresh value (incl. → undefined)', () => {
    const backed = createWorkerBackedTerm({
      post: () => undefined,
      initial: makeWorkerState()
    })
    const seen: (number | undefined)[] = []
    backed.onPredictDeadline((ms) => seen.push(ms))
    backed.applyState(makeWorkerState({ predictDeadlineMs: 250 }))
    backed.applyState(makeWorkerState({ predictDeadlineMs: 250 })) // unchanged → no fire
    backed.applyState(makeWorkerState({ predictDeadlineMs: null })) // healed → fire undefined
    expect(seen).toEqual([250, undefined])
    // The controller's synchronous read returns the last reflected value.
    const term = backed.term as unknown as {
      predict_next_deadline_ms: () => number | undefined
    }
    expect(term.predict_next_deadline_ms()).toBeUndefined()
  })

  it('a reflected deadline arms exactly one repaint at expiry; disable clears it', () => {
    vi.useFakeTimers()
    const backed = createWorkerBackedTerm({
      post: () => undefined,
      initial: makeWorkerState()
    })
    const requestPaint = vi.fn()
    const prediction = createAtermPredictionEcho({
      term: backed.term,
      requestPaint,
      isDisposed: () => false
    })
    prediction.setMode('adaptive')
    // Wire the worker seam exactly as the pane wiring does.
    backed.onPredictDeadline(() => prediction.refreshDeadline())
    backed.applyState(makeWorkerState({ predictDeadlineMs: 40 }))
    vi.advanceTimersByTime(39)
    expect(requestPaint).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(requestPaint).toHaveBeenCalledTimes(1) // the expiry repaint (self-heal)
    // Disable must clear any armed timer (the stranded-deadline 100%-CPU invariant): a
    // stale STATE arriving after 'off' can never re-arm.
    backed.applyState(makeWorkerState({ predictDeadlineMs: 250 })) // re-arm a live timer
    prediction.setMode('off') // clears it
    requestPaint.mockClear()
    backed.applyState(makeWorkerState({ predictDeadlineMs: 300 })) // stale STATE, value changed
    vi.advanceTimersByTime(1000)
    expect(requestPaint).not.toHaveBeenCalled()
  })
})

describe('R1 worker predictive-echo seam — ghost overlay draw', () => {
  const realGetContext = HTMLCanvasElement.prototype.getContext
  afterEach(() => {
    HTMLCanvasElement.prototype.getContext = realGetContext
  })

  it('paints the dim ghost glyph on the stacked overlay from the STATE cells', () => {
    const ctx = {
      clearRect: vi.fn(),
      fillRect: vi.fn(),
      fillText: vi.fn(),
      save: vi.fn(),
      restore: vi.fn(),
      translate: vi.fn(),
      fillStyle: '',
      font: '',
      textBaseline: '' as CanvasTextBaseline
    }
    HTMLCanvasElement.prototype.getContext = vi.fn(
      () => ctx
    ) as unknown as typeof HTMLCanvasElement.prototype.getContext
    const parent = document.createElement('div')
    const paneCanvas = document.createElement('canvas')
    parent.appendChild(paneCanvas)
    const overlay = createAtermWorkerOverlay(
      paneCanvas,
      () => 0xffffff,
      () => 1
    )
    overlay.paint(
      makeWorkerState({
        width: 640,
        height: 384,
        predictOverlay: new Uint32Array([0, 1, 0x61]) // ghost 'a' at (0,1)
      })
    )
    expect(ctx.fillText).toHaveBeenCalledWith('a', expect.any(Number), expect.any(Number))
    // No ghost + no search/link ⇒ idle overlay draws nothing (skips the clearRect).
    ctx.fillText.mockClear()
    ctx.clearRect.mockClear()
    overlay.paint(makeWorkerState({ width: 640, height: 384 }))
    expect(ctx.fillText).not.toHaveBeenCalled()
    overlay.dispose()
  })
})

function makeWorkerState(overrides: Partial<AtermWorkerState> = {}): AtermWorkerState {
  return {
    type: 'state',
    engine: 'cpu',
    wasmHeapBytes: 0,
    width: 0,
    height: 0,
    chromePadPx: 0,
    chromeHeadPx: 0,
    cols: 80,
    rows: 24,
    cellWidth: 8,
    cellHeight: 16,
    displayOffset: 0,
    displayOriginAbsolute: 0,
    cursorX: 0,
    cursorY: 0,
    cursorStyle: 1,
    baseY: 0,
    isAltScreen: false,
    bracketedPasteMode: false,
    isMouseTracking: false,
    mouseWantsMotion: false,
    mouseWantsAnyMotion: false,
    isFocusEventMode: false,
    isColorSchemeUpdatesMode: false,
    isAppCursorMode: false,
    isAlternateScroll: false,
    keyboardModeBits: 0,
    isReady: true,
    title: null,
    cursorColor: null,
    selectionRange: null,
    hoverLink: null,
    hoverCursor: '',
    searchCount: 0,
    searchActiveIndex: 0,
    searchActiveRect: null,
    searchResultsVersion: 0,
    searchResultsStale: false,
    searchResultsIncomplete: false,
    searchGeneration: 0,
    searchMarkers: { fractions: [], activeFraction: null },
    searchMatchRects: [],
    spillExportCapable: false,
    dirtyRows: [],
    predictOverlay: new Uint32Array(0),
    predictDeadlineMs: null,
    ...overrides
  }
}
