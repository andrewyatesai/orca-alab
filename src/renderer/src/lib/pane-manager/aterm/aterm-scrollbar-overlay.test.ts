/**
 * @vitest-environment happy-dom
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createAtermScrollbarOverlay } from './aterm-scrollbar-overlay'
import {
  markTerminalPinnedViewport,
  syncTerminalScrollIntentFromViewport
} from '../terminal-scroll-intent'
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
  getScrollIntentTarget?: () => TerminalScrollIntentTarget | null
): { host: HTMLElement; thumb: HTMLElement; dispose: () => void } {
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
    getScrollIntentTarget
  })
  const thumb = host.querySelector('[data-testid="aterm-scrollbar-thumb"]') as HTMLElement
  return { host, thumb, dispose: overlay.dispose }
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
