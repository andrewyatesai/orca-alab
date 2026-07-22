/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it } from 'vitest'
import { createWorkerBackedTerm } from './aterm-worker-term'
import { createAtermPredictionEcho } from './aterm-prediction-echo'
import { shouldUseWorkerRender } from './aterm-strategy-select'
import type { AtermWorkerPaneCommand, AtermWorkerState } from './aterm-render-worker-protocol'

// WIRED-ON-DEFAULT-PATH contract for mosh-style predictive echo — the code side of
// the EchoLiveness.tla / check-echo-liveness.mjs Trust gate. The typing-lag bug was a
// SILENT capability death: the off-main worker term facade stopped exposing predict_*,
// so createAtermPredictionEcho's runtime probe saw engine=null and EVERY echo seam
// degraded to an inert no-op — predictions simply stopped painting, with no crash and
// correct output, so nothing flagged the regression. This test pins the seam CONNECTED
// on the DEFAULT (worker) render strategy: the facade implements the full predict_*
// shape AND the capability probe passes against it. A future refactor that drops
// predict_* from the facade fails HERE instead of silently shipping the lag.

// The default render strategy is the off-main worker: loadAtermStrategy takes the worker
// path whenever `window.__atermWorkerRender !== false` (aterm-strategy-select.ts) — the
// term the pane wiring hands createAtermPredictionEcho is this worker-backed facade.
// The exact predict_* surface the controller drives (mirrors AtermPredictEngine, the Pick
// over AtermTerminal in aterm-prediction-echo.ts). The facade MUST implement all of these.
const PREDICT_METHODS = [
  'set_predictive_echo',
  'predict_char',
  'predict_backspace',
  'predict_line_submit',
  'predict_reconcile',
  'predict_overlay',
  'predict_next_deadline_ms',
  'predict_reset'
] as const

// The exact capability probe from aterm-prediction-echo.ts:70-76 — engine is non-null
// (predictions armed) only when these four are callable. Replicated here so this test
// fails if the probe's required surface and the facade's implemented surface ever drift.
function probePasses(term: Record<string, unknown>): boolean {
  return (
    typeof term.set_predictive_echo === 'function' &&
    typeof term.predict_char === 'function' &&
    typeof term.predict_overlay === 'function' &&
    typeof term.predict_next_deadline_ms === 'function'
  )
}

function makeDefaultWorkerTerm(post: (cmd: AtermWorkerPaneCommand) => void) {
  return createWorkerBackedTerm({ post, initial: makeWorkerState() })
}

describe('predictive echo is wired on the default worker render path (silent-death guard)', () => {
  it('the default render strategy is the worker path', () => {
    // The REAL predicate loadAtermStrategy uses to pick the worker term (the facade under
    // test): worker by default (flag unset), only an explicit `false` opts out.
    expect(shouldUseWorkerRender(undefined)).toBe(true)
    expect(shouldUseWorkerRender(false)).toBe(false)
    expect(shouldUseWorkerRender(true)).toBe(true)
  })

  it('the worker term facade implements the full predict_* shape', () => {
    const backed = makeDefaultWorkerTerm(() => undefined)
    const term = backed.term as unknown as Record<string, unknown>
    for (const method of PREDICT_METHODS) {
      expect(typeof term[method], `worker facade must expose ${method}`).toBe('function')
    }
  })

  it("createAtermPredictionEcho's capability probe passes against the worker facade (engine !== null)", () => {
    const backed = makeDefaultWorkerTerm(() => undefined)
    // Direct pin of the probe predicate (aterm-prediction-echo.ts:70-76).
    expect(probePasses(backed.term as unknown as Record<string, unknown>)).toBe(true)

    // Behavioural proof that the controller's own probe resolved engine!==null: an inert
    // (engine=null) controller posts NOTHING; a wired one drives every seam across the
    // worker boundary. If the facade ever drops predict_*, these posts vanish.
    const posted: AtermWorkerPaneCommand[] = []
    const prediction = createAtermPredictionEcho({
      term: makeDefaultWorkerTerm((cmd) => posted.push(cmd)).term,
      requestPaint: () => undefined,
      isDisposed: () => false
    })
    prediction.setMode('adaptive')
    prediction.noteChar('x')
    prediction.noteBackspace()
    prediction.noteSubmit()
    prediction.reset()
    expect(posted).toEqual([
      { type: 'predictSetMode', mode: 'adaptive' },
      { type: 'predictChar', ch: 'x' },
      { type: 'predictBackspace' },
      { type: 'predictSubmit' },
      { type: 'predictReset' }
    ])
  })

  it('genuinely discriminates: stripping predict_* makes the probe fail and the controller inert', () => {
    const posted: AtermWorkerPaneCommand[] = []
    const push = (cmd: AtermWorkerPaneCommand): number => posted.push(cmd)

    // Control: an intact worker facade DOES post — so the empty result below is a genuine
    // inert probe, not a dead spy.
    const armed = createAtermPredictionEcho({
      term: makeDefaultWorkerTerm(push).term,
      requestPaint: () => undefined,
      isDisposed: () => false
    })
    armed.setMode('adaptive')
    armed.noteChar('a')
    expect(posted.length).toBeGreaterThan(0)

    // Reconstruct the facade with its predict_* surface removed — the EXACT shape of the
    // silent-death regression. It still shares `push`, so an armed controller here WOULD
    // be observable; the emptiness proves the probe resolved engine=null and went inert.
    posted.length = 0
    const stripped = { ...(makeDefaultWorkerTerm(push).term as unknown as Record<string, unknown>) }
    for (const method of PREDICT_METHODS) {
      delete stripped[method]
    }
    expect(probePasses(stripped)).toBe(false)

    const inert = createAtermPredictionEcho({
      term: stripped as never,
      requestPaint: () => undefined,
      isDisposed: () => false
    })
    inert.setMode('adaptive')
    inert.noteChar('a')
    inert.noteBackspace()
    inert.noteSubmit()
    expect(posted).toEqual([]) // engine=null ⇒ every seam is an inert no-op (the bug)
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
