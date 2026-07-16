// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { I18nextProvider } from 'react-i18next'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '@/i18n/i18n'
import { getDefaultSettings } from '../../../../shared/constants'
import type { GlobalSettings } from '../../../../shared/types'

// Truth tests for the Effects section: the rendered copy must describe the
// SHIPPED defaults (effects ON), switches must render ON before settings
// hydrate, and the Scenes row only exists when the engine ships scenes.

const mocks = vi.hoisted(() => ({
  state: { settingsSearchQuery: '' },
  listAtermSceneNames: vi.fn<() => Promise<readonly string[]>>()
}))

vi.mock('../../store', () => {
  const useAppStore = (selector: (state: typeof mocks.state) => unknown): unknown =>
    selector(mocks.state)
  useAppStore.getState = () => mocks.state
  return { useAppStore }
})

// The demo card boots a real wasm engine; the section under test doesn't need it.
vi.mock('./TerminalEngineEffectsDemo', () => ({
  TerminalEngineEffectsDemo: () => null
}))

vi.mock('./terminal-engine-scene-availability', () => ({
  listAtermSceneNames: mocks.listAtermSceneNames
}))

import { TerminalEngineEffectsSection } from './TerminalEngineEffectsSection'

const mountedRoots: Root[] = []

async function renderEffectsSection(settings: GlobalSettings): Promise<HTMLDivElement> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  mountedRoots.push(root)

  await act(async () => {
    root.render(
      <I18nextProvider i18n={i18n}>
        <TerminalEngineEffectsSection
          settings={settings}
          updateSettings={vi.fn()}
          systemPrefersDark={false}
        />
      </I18nextProvider>
    )
  })

  return container
}

beforeEach(() => {
  mocks.listAtermSceneNames.mockReset().mockResolvedValue([])
})

afterEach(() => {
  for (const root of mountedRoots.splice(0)) {
    act(() => root.unmount())
  }
  document.body.innerHTML = ''
})

describe('TerminalEngineEffectsSection', () => {
  it('describes the shipped ON defaults instead of claiming effects default off', async () => {
    const container = await renderEffectsSection(getDefaultSettings('/tmp'))

    // Pins the localization CATALOG copy (which beats inline fallbacks), so the
    // pane can't drift back into contradicting constants.ts defaults.
    expect(container.textContent).toContain('Word art and the water cursor trail are on by default')
    expect(container.textContent).not.toContain('All default off')
  })

  it('renders every shipped-ON effect switch ON for pre-hydration (empty) settings', async () => {
    const container = await renderEffectsSection({} as GlobalSettings)

    const switches = [...container.querySelectorAll('[role="switch"]')]
    expect(switches.length).toBeGreaterThanOrEqual(6)
    // Matrix Rain is the one opt-in effect (ships OFF); everything else must
    // render ON before settings hydrate — no OFF-then-ON flicker.
    const shippedOn = switches.filter((el) => el.getAttribute('aria-label') !== 'Matrix Rain')
    expect(shippedOn.length).toBe(switches.length - 1)
    for (const el of shippedOn) {
      expect(el.getAttribute('aria-checked')).toBe('true')
    }
  })

  it('hides the Scenes row while the engine registry ships no scenes', async () => {
    const container = await renderEffectsSection(getDefaultSettings('/tmp'))

    expect(container.textContent).not.toContain('Scenes')
  })

  it('shows the Scenes row with the shipped scene names once the registry has art', async () => {
    mocks.listAtermSceneNames.mockResolvedValue(['aurora', 'tidepool'])

    const container = await renderEffectsSection(getDefaultSettings('/tmp'))

    expect(container.textContent).toContain('Scenes')
    expect(container.textContent).toContain('aurora, tidepool')
  })
})
