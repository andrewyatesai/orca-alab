/**
 * @vitest-environment happy-dom
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { AtermSharedWorkerPane } from './aterm-shared-render-worker'
import type { AtermDrawerBuildConfig } from './aterm-drawer-config'
import type {
  AtermWorkerPaneCommand,
  AtermWorkerPaneEvent,
  AtermWorkerState
} from './aterm-render-worker-protocol'
import { attachAtermCursorBlink, type AtermCursorTarget } from './aterm-cursor-blink'

vi.mock('./aterm-shared-render-worker', () => ({
  acquireAtermSharedWorkerPane: vi.fn()
}))
vi.mock('./aterm-worker-prewarm', () => ({
  noteRealAtermWorkerPaneAcquired: vi.fn()
}))
vi.mock('./aterm-gpu-auto-policy', () => ({
  decideAtermGpu: vi.fn(() => ({ useGpu: false, reason: 'test' }))
}))

import { loadAtermWorkerEngine } from './aterm-worker-loader'
import { acquireAtermSharedWorkerPane } from './aterm-shared-render-worker'

// Worker QoS focus regression (R4): cursor-blink prefers the tri-state
// set_effects_visibility when present — which it ALWAYS is on the worker path
// (rain facade) — so a setFocused post piggybacked only on set_effects_focused
// never fires and the worker scheduler treats the focused pane's keystroke echo
// as background bulk. Pin that driving focus through the REAL cursor-blink /
// visibility path lands setFocused on the worker message channel, both ways.

type FakePane = AtermSharedWorkerPane & {
  post: ReturnType<typeof vi.fn>
  emit: (event: AtermWorkerPaneEvent) => void
}

function makeFakePane(): FakePane {
  let handler: ((event: AtermWorkerPaneEvent) => void) | null = null
  return {
    paneId: 1,
    post: vi.fn(),
    onEvent: (h: (event: AtermWorkerPaneEvent) => void) => {
      handler = h
    },
    onCrash: vi.fn(),
    isBooted: () => true,
    reportBootWedged: vi.fn(),
    release: vi.fn(),
    emit: (event: AtermWorkerPaneEvent) => handler?.(event)
  } as unknown as FakePane
}

function makeConfig(): AtermDrawerBuildConfig {
  const holder = document.createElement('div')
  const canvas = document.createElement('canvas')
  holder.appendChild(canvas)
  document.body.appendChild(holder)
  ;(canvas as { transferControlToOffscreen?: () => OffscreenCanvas }).transferControlToOffscreen =
    () => ({}) as OffscreenCanvas
  return {
    canvas,
    themeColors: {
      fg: 0xfafafa,
      bg: 0x0a0a0a,
      cursor: 0xfafafa,
      selection: 0x64748b,
      selectionForeground: null,
      selectionInactive: null,
      palette: []
    },
    fontPx: 14,
    lineHeight: 1
  } as AtermDrawerBuildConfig
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
    searchMatchRects: [],
    spillExportCapable: false,
    dirtyRows: [],
    predictOverlay: new Uint32Array(0),
    predictDeadlineMs: null
  }
}

async function flushMicrotasks(rounds = 8): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    await Promise.resolve()
  }
}

function setFocusedPosts(pane: FakePane): boolean[] {
  return pane.post.mock.calls
    .map((call) => call[0] as AtermWorkerPaneCommand)
    .filter((command) => command.type === 'setFocused')
    .map((command) => (command as { focused: boolean }).focused)
}

async function loadWithFirstFrame(pane: FakePane): Promise<AtermCursorTarget> {
  vi.mocked(acquireAtermSharedWorkerPane).mockResolvedValue(pane)
  const pending = loadAtermWorkerEngine(makeConfig())
  await flushMicrotasks()
  pane.emit(makeWorkerState())
  const strategy = await pending
  return strategy.term as unknown as AtermCursorTarget
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('loadAtermWorkerEngine worker-QoS focus signal', () => {
  it('posts setFocused through the cursor-blink tri-state visibility path, both ways', async () => {
    const pane = makeFakePane()
    const term = await loadWithFirstFrame(pane)
    // Precondition of the regression: the worker facade DOES expose the tri-state
    // setter, so cursor-blink never falls back to set_effects_focused.
    expect(term.set_effects_visibility).toBeDefined()

    const textarea = document.createElement('textarea')
    document.body.appendChild(textarea)
    const blink = attachAtermCursorBlink({
      term,
      textarea,
      redraw: () => undefined,
      isDisposed: () => false,
      getCursorBlink: () => false
    })

    // Unfocused seed → the scheduler learns this pane is background.
    expect(setFocusedPosts(pane)).toEqual([false])
    expect(pane.post).toHaveBeenCalledWith({
      type: 'setEffectsVisibility',
      state: 'visible_unfocused'
    })

    textarea.dispatchEvent(new FocusEvent('focus'))
    expect(setFocusedPosts(pane)).toEqual([false, true])
    expect(pane.post).toHaveBeenCalledWith({ type: 'setEffectsVisibility', state: 'focused' })

    textarea.dispatchEvent(new FocusEvent('blur'))
    expect(setFocusedPosts(pane)).toEqual([false, true, false])
    blink.dispose()
  })

  it('reports unfocused for a hidden pane and keeps the effects fallback posting too', async () => {
    const pane = makeFakePane()
    const term = await loadWithFirstFrame(pane)

    // Hidden always wins over DOM focus (bounded rain drain) — QoS must agree.
    term.set_effects_visibility?.('hidden')
    expect(setFocusedPosts(pane)).toEqual([false])
    expect(pane.post).toHaveBeenCalledWith({ type: 'setEffectsVisibility', state: 'hidden' })

    // The bool fallback still carries the QoS signal for targets without the
    // tri-state; same-value double posts stay idempotent for the scheduler.
    term.set_effects_focused?.(true)
    expect(setFocusedPosts(pane)).toEqual([false, true])
    expect(pane.post).toHaveBeenCalledWith({ type: 'setEffectsFocused', focused: true })
  })
})
