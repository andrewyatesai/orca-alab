/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it } from 'vitest'
import { createWorkerTerminal } from './aterm-worker-terminal'
import { createWorkerBackedTerm } from './aterm-worker-term'
import type { EngineHandle } from './aterm-worker-engine-build'
import type { AtermWorkerPaneCommand, AtermWorkerState } from './aterm-render-worker-protocol'

// The host-minted OSC-8 extra-scheme gate (deep-links #4384): main-side
// worker-term command post + worker-side engine passthrough, incl. the
// feature-detect against a pre-capability wasm blob.

function makeSchemeHandle(withBinding: boolean): { handle: EngineHandle; minted: string[] } {
  const minted: string[] = []
  const engine = {
    display_offset: 0,
    cell_width: 8,
    cell_height: 16,
    cursor_color: undefined,
    take_response: () => undefined,
    take_osc_events: () => undefined,
    take_notifications: () => undefined,
    drain_bell: () => false,
    authorize_notifications: () => undefined,
    scroll_to_bottom: () => undefined,
    ...(withBinding
      ? {
          authorize_hyperlink_scheme: (scheme: string) => {
            minted.push(scheme)
            return true
          }
        }
      : {})
  }
  const handle = {
    kind: 'cpu',
    engine: engine as unknown as EngineHandle['engine'],
    // Spill compositor byte source; unused — these fakes export no spill surface.
    memory: { buffer: new ArrayBuffer(0) } as unknown as WebAssembly.Memory,
    process: () => undefined,
    render: () => undefined,
    framebuffer: () => ({ width: 0, height: 0 }),
    search: () => new Uint32Array(0),
    dispose: () => undefined
  } as EngineHandle
  return { handle, minted }
}

function makeWorkerState(): AtermWorkerState {
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
    predictDeadlineMs: null
  }
}

describe('worker terminal hyperlink-scheme gate (mock engine)', () => {
  it('passes setHyperlinkSchemeAuthorized through to the engine binding', () => {
    const { handle, minted } = makeSchemeHandle(true)
    const term = createWorkerTerminal(handle)
    term.setHyperlinkSchemeAuthorized('orca')
    expect(minted).toEqual(['orca'])
  })

  it('degrades to a no-op on a pre-capability engine build (feature-detect)', () => {
    const { handle, minted } = makeSchemeHandle(false)
    const term = createWorkerTerminal(handle)
    expect(() => term.setHyperlinkSchemeAuthorized('orca')).not.toThrow()
    expect(minted).toEqual([])
  })
})

describe('worker-backed term hyperlink-scheme channel', () => {
  it('authorize_hyperlink_scheme posts the setHyperlinkSchemeAuthorized command', () => {
    const posted: AtermWorkerPaneCommand[] = []
    const backed = createWorkerBackedTerm({
      post: (cmd) => posted.push(cmd),
      initial: makeWorkerState()
    })
    ;(
      backed.term as unknown as { authorize_hyperlink_scheme: (scheme: string) => boolean }
    ).authorize_hyperlink_scheme('orca')
    expect(posted).toContainEqual({ type: 'setHyperlinkSchemeAuthorized', scheme: 'orca' })
  })
})
