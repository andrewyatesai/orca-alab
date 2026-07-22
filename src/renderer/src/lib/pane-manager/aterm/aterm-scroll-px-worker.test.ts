/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it, vi } from 'vitest'
import { createWorkerTerminal } from './aterm-worker-terminal'
import { createWorkerBackedTerm } from './aterm-worker-term'
import { dispatchPaneCommand, type PaneRuntime } from './aterm-worker-pane-dispatch'
import type { EngineHandle } from './aterm-worker-engine-build'
import type {
  AtermWorkerPaneCommand,
  AtermWorkerPaneRuntimeCommand,
  AtermWorkerState
} from './aterm-render-worker-protocol'
import type { AtermTerminal } from './aterm_wasm.js'

// P3 — pixel-true trackpad scrolling across the shared-worker seam. The engine's
// scroll_px banks the sub-row residual, so the raw fractional device-px delta must
// survive the facade → protocol → dispatch → engine chain UNROUNDED.

// A minimal engine the worker-terminal constructors tolerate (they probe lazily);
// only the scroll surface is spied.
function makeScrollEngine(opts: { withScrollPx?: boolean } = {}): {
  handle: EngineHandle
  scroll_px: ReturnType<typeof vi.fn>
  scroll_lines: ReturnType<typeof vi.fn>
} {
  const scroll_px = vi.fn()
  const scroll_lines = vi.fn()
  const engine: Record<string, unknown> = {
    cell_width: 8,
    cell_height: 16,
    scroll_lines
  }
  if (opts.withScrollPx !== false) {
    engine.scroll_px = scroll_px
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
  return { handle, scroll_px, scroll_lines }
}

function makePane(term: ReturnType<typeof createWorkerTerminal> | null): {
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

describe('P3 pixel-scroll seam — facade posts the scrollPx command', () => {
  it('scroll_px posts the fractional device-px delta unrounded', () => {
    const posted: AtermWorkerPaneCommand[] = []
    const backed = createWorkerBackedTerm({
      post: (cmd) => posted.push(cmd),
      initial: makeWorkerState()
    })
    const term = backed.term as unknown as Pick<AtermTerminal, 'scroll_px'>
    term.scroll_px(-12.5)
    term.scroll_px(0.25)
    expect(posted).toEqual([
      { type: 'scrollPx', deltaPx: -12.5 },
      { type: 'scrollPx', deltaPx: 0.25 }
    ])
  })
})

describe('P3 pixel-scroll seam — worker dispatch routes to the engine', () => {
  it('scrollPx reaches engine.scroll_px unrounded and schedules a draw', () => {
    const { handle, scroll_px, scroll_lines } = makeScrollEngine()
    const { pane, scheduleDraw } = makePane(createWorkerTerminal(handle))
    dispatchPaneCommand(pane, { type: 'scrollPx', deltaPx: -12.5 })
    expect(scroll_px).toHaveBeenCalledWith(-12.5)
    expect(scroll_lines).not.toHaveBeenCalled()
    expect(scheduleDraw).toHaveBeenCalled()
  })

  it('no-ops safely while the pane engine is still building', () => {
    const { pane, scheduleDraw } = makePane(null)
    expect(() => dispatchPaneCommand(pane, { type: 'scrollPx', deltaPx: -8 })).not.toThrow()
    expect(scheduleDraw).toHaveBeenCalled()
  })

  it('round-trips facade → protocol → dispatch → engine with the residual intact', () => {
    const { handle, scroll_px } = makeScrollEngine()
    const { pane } = makePane(createWorkerTerminal(handle))
    const backed = createWorkerBackedTerm({
      // The real manager stamps a paneId and the worker entry dispatches; the pane
      // envelope never touches the payload, so posting straight in is wire-faithful.
      post: (cmd) => dispatchPaneCommand(pane, cmd as AtermWorkerPaneRuntimeCommand),
      initial: makeWorkerState()
    })
    const term = backed.term as unknown as Pick<AtermTerminal, 'scroll_px'>
    term.scroll_px(7.75)
    expect(scroll_px).toHaveBeenCalledWith(7.75)
  })
})

describe('P3 pixel-scroll seam — blob-skew fallback (engine without scroll_px)', () => {
  it('banks the sub-row remainder and flips whole lines via scroll_lines', () => {
    const { handle, scroll_lines } = makeScrollEngine({ withScrollPx: false })
    const term = createWorkerTerminal(handle)
    term.scrollPx(-8) // -0.5 rows on a 16px cell → banked, no flip
    expect(scroll_lines).not.toHaveBeenCalled()
    term.scrollPx(-8) // banked -1.0 → one whole line
    expect(scroll_lines).toHaveBeenCalledWith(-1)
    term.scrollPx(24) // +1.5 rows → flip +1, bank +0.5
    expect(scroll_lines).toHaveBeenLastCalledWith(1)
    term.scrollPx(8) // banked +1.0 → flip the carried half row
    expect(scroll_lines).toHaveBeenLastCalledWith(1)
    expect(scroll_lines).toHaveBeenCalledTimes(3)
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
    searchMatchRects: [],
    spillExportCapable: false,
    dirtyRows: [],
    predictOverlay: new Uint32Array(0),
    predictDeadlineMs: null,
    ...overrides
  }
}
