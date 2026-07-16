/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { SYNC_FIT_PANES_EVENT } from '@/constants/terminal'
import { makePaneKey } from '../../../../../shared/stable-pane-id'
import { registerTabPaneManager, unregisterTabPaneManager } from '../pane-manager-registry'
import {
  chromeFrameOrigin,
  chromeOutsideRects,
  chromeStripRects,
  type AtermDeviceRect
} from './aterm-chrome-box'
import { startAtermSpillGeometryTracker } from './aterm-spill-geometry'
import { createAtermSpillOverlay, type AtermSpillOverlay } from './aterm-spill-overlay'

// The tracker contract: ONE rAF-coalesced measure pass no matter how many
// triggers land in a frame (gBCR only inside it), geometry derived through
// aterm-chrome-box in integer device px, visibility from the aria-hidden/inert
// DOM projection (never rect size), and fully symmetric teardown.

type ObserverCallback = (...args: never[]) => void

class MockObserver {
  observe = vi.fn()
  unobserve = vi.fn()
  disconnect = vi.fn()

  constructor(readonly callback: ObserverCallback) {
    mockObserverInstances.push(this)
  }

  trigger(): void {
    ;(this.callback as () => void)()
  }
}

let mockObserverInstances: MockObserver[] = []
let pendingRafs = new Map<number, FrameRequestCallback>()
let nextRafId = 1
let mediaQueryLists: {
  addEventListener: ReturnType<typeof vi.fn>
  removeEventListener: ReturnType<typeof vi.fn>
}[] = []

function flushAnimationFrames(timestamp = 16): void {
  const callbacks = [...pendingRafs.values()]
  pendingRafs = new Map()
  for (const callback of callbacks) {
    callback(timestamp)
  }
}

function fakeRect(left: number, top: number, width: number, height: number): DOMRect {
  return {
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    x: left,
    y: top,
    toJSON: () => ({})
  } as DOMRect
}

function setRect(el: Element, rect: DOMRect): void {
  ;(el as { getBoundingClientRect: () => DOMRect }).getBoundingClientRect = vi.fn(() => rect)
}

const TAB_ID = 'tab-spill-test'
const LEAF_ID = '11111111-1111-4111-8111-111111111111'
const PANE_KEY = makePaneKey(TAB_ID, LEAF_ID)

// dpr 2 with CSS boxes chosen so every device value is exact: grid device box
// (100,80,400,300) with pad 13 / head 34 → frame device (87,33,426,360).
const DPR = 2
const CHROME = { chromePadPx: 13, chromeHeadPx: 34 }
const GRID_DEVICE: AtermDeviceRect = { x: 100, y: 80, width: 400, height: 300 }
const CLIP_DEVICE: AtermDeviceRect = { x: 96, y: 76, width: 408, height: 308 }

function buildPaneDom(parent: HTMLElement): { paneEl: HTMLElement; canvasEl: HTMLCanvasElement } {
  const paneEl = document.createElement('div')
  const canvasEl = document.createElement('canvas')
  canvasEl.setAttribute('data-testid', 'aterm-canvas')
  paneEl.appendChild(canvasEl)
  parent.appendChild(paneEl)
  setRect(paneEl, fakeRect(48, 38, 204, 154))
  setRect(canvasEl, fakeRect(43.5, 16.5, 213, 180))
  return { paneEl, canvasEl }
}

type TrackerHarness = {
  overlay: AtermSpillOverlay
  container: HTMLElement
  paneEl: HTMLElement
  canvasEl: HTMLCanvasElement
  updateGeometry: ReturnType<typeof vi.fn>
  setOverlayBox: ReturnType<typeof vi.fn>
  start: () => { dispose: () => void }
}

function buildHarness(): TrackerHarness {
  const overlay = createAtermSpillOverlay()
  const updateGeometry = vi.spyOn(overlay, 'updateGeometry') as unknown as ReturnType<typeof vi.fn>
  const setOverlayBox = vi.spyOn(overlay, 'setOverlayBox') as unknown as ReturnType<typeof vi.fn>
  const container = document.createElement('div')
  document.body.appendChild(container)
  setRect(container, fakeRect(0, 0, 800, 600))
  const { paneEl, canvasEl } = buildPaneDom(container)
  const manager = { getPanes: () => [{ leafId: LEAF_ID, container: paneEl }] }
  registerTabPaneManager(TAB_ID, manager)
  cleanups.push(() => unregisterTabPaneManager(TAB_ID, manager))
  overlay.register(PANE_KEY, CHROME)
  return {
    overlay,
    container,
    paneEl,
    canvasEl,
    updateGeometry,
    setOverlayBox,
    start: () => {
      const tracker = startAtermSpillGeometryTracker({ container, overlay, getDpr: () => DPR })
      cleanups.push(() => tracker.dispose())
      return tracker
    }
  }
}

let cleanups: (() => void)[] = []

beforeEach(() => {
  mockObserverInstances = []
  pendingRafs = new Map()
  nextRafId = 1
  mediaQueryLists = []
  cleanups = []
  vi.stubGlobal('ResizeObserver', MockObserver as never)
  vi.stubGlobal('MutationObserver', MockObserver as never)
  vi.stubGlobal(
    'requestAnimationFrame',
    vi.fn((callback: FrameRequestCallback) => {
      const id = nextRafId++
      pendingRafs.set(id, callback)
      return id
    })
  )
  vi.stubGlobal(
    'cancelAnimationFrame',
    vi.fn((id: number) => {
      pendingRafs.delete(id)
    })
  )
  window.matchMedia = vi.fn(() => {
    const mql = { addEventListener: vi.fn(), removeEventListener: vi.fn() }
    mediaQueryLists.push(mql)
    return mql as never
  }) as never
})

afterEach(() => {
  for (const cleanup of cleanups.toReversed()) {
    cleanup()
  }
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('measure coalescing', () => {
  it('coalesces N triggers within a frame into ONE measure pass (gBCR only inside it)', () => {
    const harness = buildHarness()
    harness.start()
    // Startup books exactly one measure; nothing is read before the flush.
    expect(pendingRafs.size).toBe(1)
    expect(harness.container.getBoundingClientRect).not.toHaveBeenCalled()

    // A storm of triggers while the frame is pending: container RO, layout
    // MutationObserver, window resize, fit-sync event, registry notify.
    for (const observer of mockObserverInstances) {
      observer.trigger()
    }
    window.dispatchEvent(new Event('resize'))
    window.dispatchEvent(new CustomEvent(SYNC_FIT_PANES_EVENT))
    harness.overlay.register(PANE_KEY, { chromePadPx: 13, chromeHeadPx: 40 })
    expect(pendingRafs.size).toBe(1)
    expect(harness.container.getBoundingClientRect).not.toHaveBeenCalled()

    flushAnimationFrames()
    expect(harness.container.getBoundingClientRect).toHaveBeenCalledTimes(1)
    expect(harness.paneEl.getBoundingClientRect).toHaveBeenCalledTimes(1)
    expect(harness.canvasEl.getBoundingClientRect).toHaveBeenCalledTimes(1)
    expect(harness.setOverlayBox).toHaveBeenCalledTimes(1)
    expect(harness.updateGeometry).toHaveBeenCalledTimes(1)
    expect(pendingRafs.size).toBe(0)
  })

  it('a trigger after the flush books a fresh single measure', () => {
    const harness = buildHarness()
    harness.start()
    flushAnimationFrames()
    window.dispatchEvent(new Event('resize'))
    window.dispatchEvent(new Event('resize'))
    expect(pendingRafs.size).toBe(1)
    flushAnimationFrames()
    expect(harness.container.getBoundingClientRect).toHaveBeenCalledTimes(2)
  })
})

describe('measured geometry', () => {
  it('derives integer-device-px geometry through aterm-chrome-box', () => {
    const harness = buildHarness()
    harness.start()
    flushAnimationFrames()
    expect(harness.setOverlayBox).toHaveBeenCalledWith({ widthPx: 1600, heightPx: 1200 })
    const pad = CHROME.chromePadPx
    const head = CHROME.chromeHeadPx
    expect(harness.updateGeometry).toHaveBeenCalledWith(PANE_KEY, {
      frameOrigin: chromeFrameOrigin(GRID_DEVICE, pad, head),
      clipRect: CLIP_DEVICE,
      stripRects: chromeStripRects(GRID_DEVICE, pad, head),
      outsideRects: chromeOutsideRects(GRID_DEVICE, pad, head, CLIP_DEVICE),
      visible: true
    })
  })

  it('projects hidden worktrees from aria-hidden/inert ancestors, never rect size', () => {
    const harness = buildHarness()
    // Re-home the pane under an aria-hidden worktree wrapper with REAL rects
    // (the hidden-but-measurable opacity-0 case).
    const hiddenWrapper = document.createElement('div')
    hiddenWrapper.setAttribute('aria-hidden', 'true')
    harness.container.appendChild(hiddenWrapper)
    hiddenWrapper.appendChild(harness.paneEl)
    harness.start()
    flushAnimationFrames()
    const geometry = harness.updateGeometry.mock.calls.at(-1)?.[1]
    expect(geometry?.visible).toBe(false)
    expect(geometry?.outsideRects.length).toBeGreaterThan(0)
  })

  it('pushes the hidden geometry for an unmeasurable (0-size) pane', () => {
    const harness = buildHarness()
    setRect(harness.canvasEl, fakeRect(0, 0, 0, 0))
    harness.start()
    flushAnimationFrames()
    const geometry = harness.updateGeometry.mock.calls.at(-1)?.[1]
    expect(geometry).toMatchObject({ visible: false, outsideRects: [], stripRects: [] })
  })

  it('pushes the hidden geometry for a registered pane with no live manager pane', () => {
    const harness = buildHarness()
    const orphanKey = makePaneKey(TAB_ID, '22222222-2222-4222-8222-222222222222')
    harness.overlay.register(orphanKey, CHROME)
    harness.start()
    flushAnimationFrames()
    expect(harness.updateGeometry).toHaveBeenCalledWith(
      orphanKey,
      expect.objectContaining({ visible: false, outsideRects: [] })
    )
  })

  it('skips manager panes that never registered with the overlay', () => {
    const harness = buildHarness()
    const strangerLeaf = '33333333-3333-4333-8333-333333333333'
    const strangerEl = document.createElement('div')
    harness.container.appendChild(strangerEl)
    const manager = { getPanes: () => [{ leafId: strangerLeaf, container: strangerEl }] }
    registerTabPaneManager(TAB_ID, manager)
    cleanups.push(() => unregisterTabPaneManager(TAB_ID, manager))
    harness.start()
    flushAnimationFrames()
    const measuredKeys = harness.updateGeometry.mock.calls.map((call) => call[0])
    expect(measuredKeys).toContain(PANE_KEY)
    expect(measuredKeys).not.toContain(makePaneKey(TAB_ID, strangerLeaf))
  })

  it('observes each registered pane container exactly once across passes', () => {
    const harness = buildHarness()
    harness.start()
    flushAnimationFrames()
    const resizeObserver = mockObserverInstances[0]
    expect(resizeObserver?.observe).toHaveBeenCalledWith(harness.container)
    expect(resizeObserver?.observe).toHaveBeenCalledWith(harness.paneEl)
    window.dispatchEvent(new Event('resize'))
    flushAnimationFrames()
    const paneObserveCalls = resizeObserver?.observe.mock.calls.filter(
      (call) => call[0] === harness.paneEl
    )
    expect(paneObserveCalls).toHaveLength(1)
  })
})

describe('teardown symmetry', () => {
  it('disconnects every observer, listener, dpr hook and registry subscription', () => {
    const harness = buildHarness()
    const tracker = harness.start()
    flushAnimationFrames()
    // Book a pending measure so dispose must cancel it.
    window.dispatchEvent(new Event('resize'))
    expect(pendingRafs.size).toBe(1)

    tracker.dispose()
    expect(pendingRafs.size).toBe(0)
    expect(cancelAnimationFrame).toHaveBeenCalled()
    const [resizeObserver, mutationObserver] = mockObserverInstances
    expect(resizeObserver?.disconnect).toHaveBeenCalledTimes(1)
    expect(mutationObserver?.disconnect).toHaveBeenCalledTimes(1)
    for (const mql of mediaQueryLists) {
      expect(mql.removeEventListener.mock.calls.length).toBe(mql.addEventListener.mock.calls.length)
    }

    // Every trigger class is dead: window events, observers, registry churn.
    window.dispatchEvent(new Event('resize'))
    window.dispatchEvent(new CustomEvent(SYNC_FIT_PANES_EVENT))
    for (const observer of mockObserverInstances) {
      observer.trigger()
    }
    harness.overlay.register(PANE_KEY, { chromePadPx: 20, chromeHeadPx: 44 })
    expect(pendingRafs.size).toBe(0)
    expect(requestAnimationFrame).toHaveBeenCalledTimes(2)
  })
})
