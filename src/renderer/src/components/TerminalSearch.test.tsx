// @vitest-environment happy-dom

// P1: find-as-you-type is debounced (~75ms) with request generations — stale results
// are discarded, Enter bypasses the debounce, and a visible pending state covers the
// round-trip. P6 surfaces the streaming cost-gate's stale flag as the ~approx label.

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import TerminalSearch, { SEARCH_DEBOUNCE_MS, type AtermSearchSurface } from './TerminalSearch'
import type { SearchState } from '@/components/terminal-pane/keyboard-handlers'

vi.mock('@/i18n/i18n', () => ({
  translate: (_key: string, fallback: string) => fallback
}))
// The styleguide Tooltip needs a radix provider tree irrelevant to these behaviors.
vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
  TooltipContent: () => null
}))

type FindResult = { count: number; activeIndex: number } | null

function makeSurface(): {
  surface: AtermSearchSurface
  findCalls: [string, boolean, boolean][]
  resolveFind: (index: number, result: FindResult) => Promise<void>
  listeners: (() => void)[]
  setSnapshot: (count: number, activeIndex: number, stale: boolean, incomplete?: boolean) => void
} {
  const findCalls: [string, boolean, boolean][] = []
  const resolvers: ((r: FindResult) => void)[] = []
  const listeners: (() => void)[] = []
  let snapshot = { count: 0, activeIndex: 0, stale: false, incomplete: false }
  const surface: AtermSearchSurface = {
    findMatches: vi.fn(() => 0),
    findMatchesAsync: (query, caseSensitive, isRegex) => {
      findCalls.push([query, caseSensitive, isRegex])
      return new Promise<FindResult>((resolve) => resolvers.push(resolve))
    },
    findNextMatch: vi.fn(),
    findPreviousMatch: vi.fn(),
    clearSearch: vi.fn(),
    searchMatchCount: () => snapshot.count,
    searchActiveMatchIndex: () => snapshot.activeIndex,
    searchResultsStale: () => snapshot.stale,
    searchResultsIncomplete: () => snapshot.incomplete,
    searchIsPending: () => false,
    onSearchStateChange: (handler) => {
      listeners.push(handler)
      return () => undefined
    }
  }
  return {
    surface,
    findCalls,
    resolveFind: async (index, result) => {
      await act(async () => {
        resolvers[index](result)
      })
    },
    listeners,
    setSnapshot: (count, activeIndex, stale, incomplete = false) => {
      snapshot = { count, activeIndex, stale, incomplete }
    }
  }
}

let root: Root | null = null
let container: HTMLDivElement | null = null

function renderSearch(
  surface: AtermSearchSurface,
  seedQueryRef?: React.RefObject<string | null>
): void {
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  const searchStateRef = {
    current: { query: '', caseSensitive: false, regex: false } as SearchState
  }
  act(() => {
    root!.render(
      <TerminalSearch
        isOpen
        onClose={() => undefined}
        atermSearch={surface}
        searchStateRef={searchStateRef}
        seedQueryRef={seedQueryRef}
      />
    )
  })
}

function typeQuery(text: string): void {
  const input = container!.querySelector('input')!
  const setValue = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')!.set!
  act(() => {
    setValue.call(input, text)
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

function pressEnter(shift = false): void {
  const input = container!.querySelector('input')!
  act(() => {
    input.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', shiftKey: shift, bubbles: true })
    )
  })
}

const advance = (ms: number): void => {
  act(() => {
    vi.advanceTimersByTime(ms)
  })
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  act(() => root?.unmount())
  container?.remove()
  root = null
  container = null
  vi.useRealTimers()
})

describe('TerminalSearch find-as-you-type', () => {
  it('debounces keystrokes into one find per settled window', () => {
    const { surface, findCalls } = makeSurface()
    renderSearch(surface)

    typeQuery('a')
    advance(SEARCH_DEBOUNCE_MS - 25)
    typeQuery('ab')
    advance(SEARCH_DEBOUNCE_MS - 25)
    typeQuery('abc')
    expect(findCalls).toHaveLength(0) // nothing until a window settles

    advance(SEARCH_DEBOUNCE_MS)
    expect(findCalls).toEqual([['abc', false, false]]) // ONE find, the latest text
  })

  it('shows the pending state until THIS request resolves, then the exact count', async () => {
    const { surface, findCalls, resolveFind } = makeSurface()
    renderSearch(surface)

    typeQuery('abc')
    advance(SEARCH_DEBOUNCE_MS)
    expect(findCalls).toHaveLength(1)
    expect(container!.querySelector('[data-terminal-search-pending]')).not.toBeNull()
    expect(container!.querySelector('[data-terminal-search-count]')).toBeNull()

    await resolveFind(0, { count: 12, activeIndex: 3 })
    expect(container!.querySelector('[data-terminal-search-pending]')).toBeNull()
    expect(container!.querySelector('[data-terminal-search-count]')!.textContent).toBe('3 / 12')
  })

  it('discards a superseded find result (request generations)', async () => {
    const { surface, findCalls, resolveFind } = makeSurface()
    renderSearch(surface)

    typeQuery('a')
    advance(SEARCH_DEBOUNCE_MS)
    typeQuery('ab')
    advance(SEARCH_DEBOUNCE_MS)
    expect(findCalls).toHaveLength(2)

    // The OLD request resolves late — its result must never reach the label.
    await resolveFind(0, { count: 99, activeIndex: 1 })
    expect(container!.querySelector('[data-terminal-search-pending]')).not.toBeNull()

    await resolveFind(1, { count: 2, activeIndex: 2 })
    expect(container!.querySelector('[data-terminal-search-count]')!.textContent).toBe('2 / 2')
  })

  it('Enter bypasses the debounce (runs the armed find NOW, no navigation)', () => {
    const { surface, findCalls } = makeSurface()
    renderSearch(surface)

    typeQuery('abc')
    pressEnter()
    expect(findCalls).toEqual([['abc', false, false]]) // ran immediately
    expect(surface.findNextMatch).not.toHaveBeenCalled()

    // The flushed timer must not double-fire the same find later.
    advance(SEARCH_DEBOUNCE_MS * 2)
    expect(findCalls).toHaveLength(1)
  })

  it('Enter with no armed find navigates (next / shift = previous)', async () => {
    const { surface, resolveFind } = makeSurface()
    renderSearch(surface)
    typeQuery('abc')
    advance(SEARCH_DEBOUNCE_MS)
    await resolveFind(0, { count: 2, activeIndex: 2 })

    pressEnter()
    expect(surface.findNextMatch).toHaveBeenCalledTimes(1)
    pressEnter(true)
    expect(surface.findPreviousMatch).toHaveBeenCalledTimes(1)
  })

  it('clears immediately (never debounced) when the query empties', () => {
    const { surface, findCalls } = makeSurface()
    renderSearch(surface)
    typeQuery('abc')
    advance(SEARCH_DEBOUNCE_MS)
    expect(findCalls).toHaveLength(1)

    typeQuery('')
    expect(surface.clearSearch).toHaveBeenCalled() // no 75ms wait to clear highlights
    expect(container!.querySelector('[data-terminal-search-pending]')).toBeNull()
  })

  it('seeds the query from seedQueryRef on open and runs an immediate find (CM-A1)', () => {
    const { surface, findCalls } = makeSurface()
    const seedQueryRef = { current: 'npm ERR! code 1' as string | null }
    renderSearch(surface, seedQueryRef)

    // The seed bypasses the keystroke debounce — the user already committed to it.
    expect(findCalls).toEqual([['npm ERR! code 1', false, false]])
    expect(seedQueryRef.current).toBeNull() // one-shot: consumed on open
    expect(container!.querySelector('input')!.value).toBe('npm ERR! code 1')

    // Typing afterwards behaves exactly as today: debounced, not immediate.
    typeQuery('npm')
    expect(findCalls).toHaveLength(1)
    advance(SEARCH_DEBOUNCE_MS)
    expect(findCalls).toEqual([
      ['npm ERR! code 1', false, false],
      ['npm', false, false]
    ])
  })

  it('renders the ~approximate indicator while streaming results are stale (P6)', async () => {
    const { surface, resolveFind, listeners, setSnapshot } = makeSurface()
    renderSearch(surface)
    typeQuery('abc')
    advance(SEARCH_DEBOUNCE_MS)
    setSnapshot(5, 5, true) // the worker's cost gate flagged the results stale
    await resolveFind(0, { count: 5, activeIndex: 5 })

    const count = container!.querySelector('[data-terminal-search-count]')!
    expect(count.textContent).toBe('~5 / 5')
    expect(count.getAttribute('data-stale')).toBe('true')

    // Guaranteed final refresh lands → the worker pushes fresh state → indicator clears.
    setSnapshot(6, 6, false)
    act(() => listeners.forEach((fn) => fn()))
    const fresh = container!.querySelector('[data-terminal-search-count]')!
    expect(fresh.textContent).toBe('6 / 6')
    expect(fresh.getAttribute('data-stale')).toBeNull()
  })

  it('renders the incomplete/stale label matrix: N+ / ~N / ~N+ (E9a)', async () => {
    const { surface, resolveFind, listeners, setSnapshot } = makeSurface()
    renderSearch(surface)
    typeQuery('abc')
    advance(SEARCH_DEBOUNCE_MS)

    // incomplete only: the engine truncated the index — the count is a floor, "N+".
    setSnapshot(5, 5, false, true)
    await resolveFind(0, { count: 5, activeIndex: 5 })
    const count = container!.querySelector('[data-terminal-search-count]')!
    expect(count.textContent).toBe('5 / 5+')
    expect(count.getAttribute('data-incomplete')).toBe('true')
    expect(count.getAttribute('data-stale')).toBeNull()

    // stale only: the cost gate serves older results — "~N" (unchanged behavior).
    setSnapshot(6, 6, true, false)
    act(() => listeners.forEach((fn) => fn()))
    expect(container!.querySelector('[data-terminal-search-count]')!.textContent).toBe('~6 / 6')

    // both: older results AND a truncated index — the markers compose, "~N+".
    setSnapshot(7, 7, true, true)
    act(() => listeners.forEach((fn) => fn()))
    const both = container!.querySelector('[data-terminal-search-count]')!
    expect(both.textContent).toBe('~7 / 7+')
    expect(both.getAttribute('data-stale')).toBe('true')
    expect(both.getAttribute('data-incomplete')).toBe('true')

    // neither: the exact label, no markers.
    setSnapshot(8, 8, false, false)
    act(() => listeners.forEach((fn) => fn()))
    const exact = container!.querySelector('[data-terminal-search-count]')!
    expect(exact.textContent).toBe('8 / 8')
    expect(exact.getAttribute('data-stale')).toBeNull()
    expect(exact.getAttribute('data-incomplete')).toBeNull()
  })
})
