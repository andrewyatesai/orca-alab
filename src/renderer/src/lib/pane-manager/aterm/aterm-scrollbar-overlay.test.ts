/**
 * @vitest-environment happy-dom
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createAtermScrollbarOverlay } from './aterm-scrollbar-overlay'
import {
  markTerminalPinnedViewport,
  syncTerminalScrollIntentFromViewport
} from '../terminal-scroll-intent'
import type { AtermSearchMarkerModel } from './aterm-search-marker-model'
import type { TerminalScrollIntentTarget } from '../terminal-scroll-intent-types'
import type { AtermTerminal } from './aterm_wasm.js'

// The thumb has no .xterm class and the canvas emits no DOM scroll event, so
// dom-tracking's pointer gate never arms for a thumb-drag — this overlay is the ONLY
// place the drag can record scroll intent. Spy the seam to prove it does.
vi.mock('../terminal-scroll-intent', () => ({
  markTerminalPinnedViewport: vi.fn(),
  syncTerminalScrollIntentFromViewport: vi.fn()
}))

afterEach(() => {
  vi.mocked(markTerminalPinnedViewport).mockClear()
  vi.mocked(syncTerminalScrollIntentFromViewport).mockClear()
})

type FakeEngine = {
  term: Pick<AtermTerminal, 'display_offset' | 'base_y' | 'is_alt_screen' | 'scroll_lines'>
  scrollLines: ReturnType<typeof vi.fn>
}

function fakeEngine(displayOffset = 100): FakeEngine {
  const scrollLines = vi.fn()
  const term = {
    display_offset: displayOffset,
    base_y: 100,
    is_alt_screen: false,
    scroll_lines: scrollLines
  } as unknown as FakeEngine['term']
  return { term, scrollLines }
}

function mountOverlay(
  engine: FakeEngine,
  getScrollIntentTarget?: () => TerminalScrollIntentTarget | null,
  getSearchMarkers?: () => AtermSearchMarkerModel
): {
  host: HTMLElement
  thumb: HTMLElement
  markerLayer: HTMLElement
  refreshSearchMarkers: () => void
  dispose: () => void
} {
  const host = document.createElement('div')
  // happy-dom does no layout: give the track a height so the thumb geometry is real.
  Object.defineProperty(host, 'clientHeight', { value: 200, configurable: true })
  const canvas = document.createElement('canvas')
  host.appendChild(canvas)
  document.body.appendChild(host)
  const overlay = createAtermScrollbarOverlay(canvas, {
    term: engine.term,
    getRows: () => 24,
    redraw: vi.fn(),
    isDisposed: () => false,
    getScrollIntentTarget,
    getSearchMarkers
  })
  const thumb = host.querySelector('[data-testid="aterm-scrollbar-thumb"]') as HTMLElement
  const markerLayer = host.querySelector(
    '[data-testid="aterm-scrollbar-search-markers"]'
  ) as HTMLElement
  return {
    host,
    thumb,
    markerLayer,
    refreshSearchMarkers: overlay.refreshSearchMarkers,
    dispose: overlay.dispose
  }
}

function startDrag(thumb: HTMLElement, clientY: number): void {
  thumb.dispatchEvent(
    new MouseEvent('mousedown', { button: 0, clientY, bubbles: true, cancelable: true })
  )
}

function dragMoveTo(clientY: number): void {
  window.dispatchEvent(new MouseEvent('mousemove', { clientY, bubbles: true, cancelable: true }))
}

function endDrag(): void {
  window.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))
}

describe('createAtermScrollbarOverlay thumb-drag scroll intent', () => {
  it('records pinned-viewport intent on the facade after a thumb-drag scroll', () => {
    const engine = fakeEngine(100)
    const target = {} as TerminalScrollIntentTarget
    const { thumb, dispose } = mountOverlay(engine, () => target)

    startDrag(thumb, 0)
    dragMoveTo(100) // moves the thumb → a non-zero engine scroll

    expect(engine.scrollLines).toHaveBeenCalled()
    // mark-then-sync, mirroring keyboard-handlers' Cmd+Up path and the wheel path.
    expect(markTerminalPinnedViewport).toHaveBeenCalledWith(target)
    expect(syncTerminalScrollIntentFromViewport).toHaveBeenCalledWith(target, {
      userInteraction: true
    })
    dispose()
  })

  it('settles the final intent on drag release (mouseup)', () => {
    const engine = fakeEngine(100)
    const target = {} as TerminalScrollIntentTarget
    const { thumb, dispose } = mountOverlay(engine, () => target)

    startDrag(thumb, 0)
    dragMoveTo(100)
    vi.mocked(markTerminalPinnedViewport).mockClear()
    vi.mocked(syncTerminalScrollIntentFromViewport).mockClear()

    endDrag()
    // onDragEnd re-records so a final no-delta move still commits the reading position.
    expect(markTerminalPinnedViewport).toHaveBeenCalledWith(target)
    expect(syncTerminalScrollIntentFromViewport).toHaveBeenCalledWith(target, {
      userInteraction: true
    })
    dispose()
  })

  it('scrolls without recording intent when no intent target is wired', () => {
    const engine = fakeEngine(100)
    const { thumb, dispose } = mountOverlay(engine, undefined)

    startDrag(thumb, 0)
    dragMoveTo(100)

    expect(engine.scrollLines).toHaveBeenCalled()
    expect(markTerminalPinnedViewport).not.toHaveBeenCalled()
    expect(syncTerminalScrollIntentFromViewport).not.toHaveBeenCalled()
    dispose()
  })
})

describe('createAtermScrollbarOverlay search match markers', () => {
  it('paints one %-positioned tick per fraction plus a distinct active tick', () => {
    let model: AtermSearchMarkerModel = { fractions: [0.25, 0.75], activeFraction: 0.75 }
    const { markerLayer, refreshSearchMarkers, dispose } = mountOverlay(
      fakeEngine(0),
      undefined,
      () => model
    )

    expect(markerLayer.children).toHaveLength(0) // nothing until a refresh lands
    refreshSearchMarkers()

    const ticks = Array.from(markerLayer.children) as HTMLElement[]
    expect(ticks).toHaveLength(3) // two fraction ticks + the active one on top
    expect(ticks[0].style.top).toBe('25.0000%')
    expect(ticks[1].style.top).toBe('75.0000%')
    expect(ticks[2].dataset.active).toBe('true')
    expect(ticks[2].style.top).toBe('75.0000%')
    // Percent positioning is the resize story: no px math to go stale.
    expect(ticks[0].style.transform).toBe('translateY(-50%)')
    // The strip must never intercept thumb drags or canvas clicks.
    expect(markerLayer.style.pointerEvents).toBe('none')

    // Clearing the search (empty model) empties the strip.
    model = { fractions: [], activeFraction: null }
    refreshSearchMarkers()
    expect(markerLayer.children).toHaveLength(0)
    dispose()
  })

  it('skips the DOM rebuild when the model is value-equal', () => {
    const { markerLayer, refreshSearchMarkers, dispose } = mountOverlay(
      fakeEngine(0),
      undefined,
      () => ({
        fractions: [0.5],
        activeFraction: null
      })
    )
    refreshSearchMarkers()
    const first = markerLayer.children[0]
    refreshSearchMarkers() // fresh (equal) model object → no repaint
    expect(markerLayer.children[0]).toBe(first)
    dispose()
  })

  it('removes the marker layer on dispose', () => {
    const { host, markerLayer, dispose } = mountOverlay(fakeEngine(0), undefined, () => ({
      fractions: [],
      activeFraction: null
    }))
    expect(host.contains(markerLayer)).toBe(true)
    dispose()
    expect(host.contains(markerLayer)).toBe(false)
  })
})
