import { describe, expect, it } from 'vitest'
import { createWorkerTerminal } from './aterm-worker-terminal'
import { createWorkerBackedTerm } from './aterm-worker-term'
import { dispatchPaneCommand, type PaneRuntime } from './aterm-worker-pane-dispatch'
import type { EngineHandle } from './aterm-worker-engine-build'
import type { AtermWorkerPaneEvent, AtermWorkerState } from './aterm-render-worker-protocol'

// The worker keyboard-mode push: worker-hosted panes encode keys on the MAIN
// thread from the last STATE snapshot's keyboard_mode_bits. STATE frames are
// coalesced with rendering, so a kitty app that flips its keyboard mode and
// then idles (no output → no frames) would leave the mirror stale for an
// UNBOUNDED window — every keystroke in it encodes under the wrong mode. The
// fix posts a small dedicated {type:'keyboardModeBits'} message the moment a
// processed chunk changes the bits, and the main side applies it to the
// synchronous snapshot immediately.

function makeModeBitsHandle(): { handle: EngineHandle; engine: { keyboard_mode_bits: number } } {
  const engine = {
    keyboard_mode_bits: 0,
    display_offset: 0,
    cell_width: 8,
    cell_height: 16,
    cursor_color: undefined,
    take_response: () => undefined,
    take_osc_events: () => undefined,
    take_notifications: () => undefined,
    drain_bell: () => false,
    scroll_to_bottom: () => undefined
  }
  const handle = {
    kind: 'cpu',
    engine: engine as unknown as EngineHandle['engine'],
    // Spill compositor byte source; unused — these fakes export no spill surface.
    memory: { buffer: new ArrayBuffer(0) } as unknown as WebAssembly.Memory,
    // The chunk itself drives the mode flip, like a parsed CSI = 1 u would.
    process: (data: string) => {
      if (data === '\x1b[=1u') {
        engine.keyboard_mode_bits = 0x1
      }
      if (data === '\x1b[=0u') {
        engine.keyboard_mode_bits = 0
      }
    },
    render: () => undefined,
    framebuffer: () => ({ width: 0, height: 0 }),
    search: () => new Uint32Array(0),
    dispose: () => undefined
  } as EngineHandle
  return { handle, engine }
}

describe('worker terminal keyboard-mode flip detection (mock engine)', () => {
  it('reports the new bits only for the chunk that changed them', () => {
    const { handle } = makeModeBitsHandle()
    const term = createWorkerTerminal(handle)
    expect(term.processBytes('plain output').keyboardModeBits).toBeUndefined()
    expect(term.processBytes('\x1b[=1u').keyboardModeBits).toBe(0x1)
    // Unchanged on the next chunk: no re-post (the STATE snapshot carries it).
    expect(term.processBytes('more output').keyboardModeBits).toBeUndefined()
    expect(term.processBytes('\x1b[=0u').keyboardModeBits).toBe(0)
  })
})

describe('worker pane dispatch posts the mode flip immediately', () => {
  it('posts a dedicated keyboardModeBits event from the process command', () => {
    const { handle } = makeModeBitsHandle()
    const posted: AtermWorkerPaneEvent[] = []
    const pane = {
      paneId: 1,
      term: createWorkerTerminal(handle),
      engineSetters: null,
      storedInit: null,
      canvas: null,
      fellBackToCpu: false,
      disposed: false,
      frameScheduler: {
        schedule: () => undefined,
        presentNow: () => undefined,
        setSuspended: () => undefined
      },
      serializeCache: { schedule: () => undefined, dispose: () => undefined },
      post: (event: AtermWorkerPaneEvent) => posted.push(event)
    } as unknown as PaneRuntime

    dispatchPaneCommand(pane, { type: 'process', data: 'plain output' })
    expect(posted.filter((e) => e.type === 'keyboardModeBits')).toEqual([])

    dispatchPaneCommand(pane, { type: 'process', data: '\x1b[=1u' })
    expect(posted.filter((e) => e.type === 'keyboardModeBits')).toEqual([
      { type: 'keyboardModeBits', bits: 0x1 }
    ])
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

describe('worker-backed term applies pushed mode bits to the sync snapshot', () => {
  it('refreshes keyboard_mode_bits immediately, before any new STATE frame', () => {
    const backed = createWorkerBackedTerm({ post: () => undefined, initial: makeWorkerState() })
    const term = backed.term as unknown as { keyboard_mode_bits: number }
    expect(term.keyboard_mode_bits).toBe(0)
    backed.applyKeyboardModeBits(0x1)
    expect(term.keyboard_mode_bits).toBe(0x1)
    // A later full STATE frame carries the same value and must not regress it.
    backed.applyState(makeWorkerState({ keyboardModeBits: 0x1 }))
    expect(term.keyboard_mode_bits).toBe(0x1)
  })
})
