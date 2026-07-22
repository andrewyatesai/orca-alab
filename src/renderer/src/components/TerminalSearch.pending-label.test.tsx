// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { I18nextProvider } from 'react-i18next'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '@/i18n/i18n'
import { TooltipProvider } from '@/components/ui/tooltip'
import TerminalSearch, { type AtermSearchSurface } from './TerminalSearch'
import type { SearchState } from '@/components/terminal-pane/keyboard-handlers'

// While a posted find is in flight on the worker path, the snapshot count is the
// PREVIOUS query's — the label must say so ("~N, searching…"), then resolve to the
// exact "active / total" when the worker's search-state change lands.

type FakeSurface = {
  surface: AtermSearchSurface
  set: (state: { count: number; active: number; pending: boolean }) => void
  emitChange: () => void
}

function makeSurface(initial: { count: number; active: number; pending: boolean }): FakeSurface {
  let state = initial
  const listeners = new Set<() => void>()
  return {
    surface: {
      findMatches: vi.fn(() => 0),
      // Never resolves within a test: the pending label must come from the snapshot
      // (searchIsPending), not from a completed round-trip.
      findMatchesAsync: () => new Promise(() => undefined),
      findNextMatch: vi.fn(),
      findPreviousMatch: vi.fn(),
      clearSearch: vi.fn(),
      searchMatchCount: () => state.count,
      searchActiveMatchIndex: () => state.active,
      searchResultsStale: () => false,
      searchIsPending: () => state.pending,
      onSearchStateChange: (handler) => {
        listeners.add(handler)
        return () => listeners.delete(handler)
      }
    },
    set: (next) => {
      state = next
    },
    emitChange: () => listeners.forEach((fn) => fn())
  }
}

const mountedRoots: Root[] = []

afterEach(() => {
  for (const root of mountedRoots) {
    act(() => root.unmount())
  }
  mountedRoots.length = 0
  document.body.innerHTML = ''
})

async function renderSearch(surface: AtermSearchSurface): Promise<HTMLElement> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  mountedRoots.push(root)
  const searchStateRef = {
    current: { query: '', caseSensitive: false, regex: false }
  } as React.RefObject<SearchState>

  await act(async () => {
    root.render(
      <I18nextProvider i18n={i18n}>
        <TooltipProvider>
          <TerminalSearch
            isOpen
            onClose={vi.fn()}
            atermSearch={surface}
            searchStateRef={searchStateRef}
          />
        </TooltipProvider>
      </I18nextProvider>
    )
  })
  return container
}

async function typeQuery(container: HTMLElement, query: string): Promise<void> {
  const input = container.querySelector('input[type="text"]') as HTMLInputElement
  await act(async () => {
    // Set through the prototype setter so React's value tracker sees the change.
    const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
    setValue?.call(input, query)
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

const label = (container: HTMLElement): string | null =>
  container.querySelector('[data-terminal-search-count]')?.textContent ?? null

describe('TerminalSearch pending match-count label', () => {
  it('shows "~N, searching…" while pending, then the exact count on the async change', async () => {
    const fake = makeSurface({ count: 5, active: 5, pending: true })
    const container = await renderSearch(fake.surface)

    await typeQuery(container, 'foo')
    expect(label(container)).toBe('~5, searching…')

    // The worker echoes the find: results land via the change subscription.
    fake.set({ count: 3, active: 3, pending: false })
    await act(async () => fake.emitChange())
    expect(label(container)).toBe('3 / 3')
  })

  it('shows a bare "searching…" when there is no prior count to approximate', async () => {
    const fake = makeSurface({ count: 0, active: 0, pending: true })
    const container = await renderSearch(fake.surface)
    await typeQuery(container, 'foo')
    expect(label(container)).toBe('searching…')
  })

  it('keeps the exact "active / total" label when nothing is pending', async () => {
    const fake = makeSurface({ count: 4, active: 2, pending: false })
    const container = await renderSearch(fake.surface)
    await typeQuery(container, 'foo')
    expect(label(container)).toBe('2 / 4')
  })
})
