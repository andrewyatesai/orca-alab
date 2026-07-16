// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { I18nextProvider } from 'react-i18next'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '@/i18n/i18n'
import { getDefaultSettings } from '../../../../shared/constants'
import type { TerminalEngineRendererStatus } from './terminal-engine-renderer-status'

// The Rendering section's renderer-status row must surface the live policy
// decision (path + why + presentation), never a hardcoded claim.

const mocks = vi.hoisted(() => ({
  state: { settingsSearchQuery: '' },
  readStatus: vi.fn<() => TerminalEngineRendererStatus>()
}))

vi.mock('../../store', () => {
  const useAppStore = (selector: (state: typeof mocks.state) => unknown): unknown =>
    selector(mocks.state)
  useAppStore.getState = () => mocks.state
  return { useAppStore }
})

vi.mock('./terminal-engine-renderer-status', () => ({
  readTerminalEngineRendererStatus: mocks.readStatus
}))

import { TerminalEngineRenderingSection } from './TerminalEngineRenderingSection'

const mountedRoots: Root[] = []

async function renderRenderingSection(): Promise<HTMLDivElement> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  mountedRoots.push(root)

  await act(async () => {
    root.render(
      <I18nextProvider i18n={i18n}>
        <TerminalEngineRenderingSection
          settings={getDefaultSettings('/tmp')}
          updateSettings={vi.fn()}
        />
      </I18nextProvider>
    )
  })

  return container
}

function status(overrides: Partial<TerminalEngineRendererStatus>): TerminalEngineRendererStatus {
  return {
    path: 'gpu',
    reason: 'auto-allowed',
    adapter: null,
    workerPresentation: true,
    ...overrides
  }
}

beforeEach(() => {
  mocks.readStatus.mockReset().mockReturnValue(status({}))
})

afterEach(() => {
  for (const root of mountedRoots.splice(0)) {
    act(() => root.unmount())
  }
  document.body.innerHTML = ''
})

describe('TerminalEngineRenderingSection renderer status row', () => {
  it('shows the GPU path, worker presentation, and the probed adapter', async () => {
    mocks.readStatus.mockReturnValue(
      status({ adapter: 'ANGLE (Apple, ANGLE Metal Renderer: Apple M2, Unspecified Version)' })
    )

    const container = await renderRenderingSection()

    expect(container.textContent).toContain('GPU (WebGL2) · render worker')
    expect(container.textContent).toContain('Auto: this GPU passed the renderer safety checks.')
    expect(container.textContent).toContain('ANGLE Metal Renderer: Apple M2')
  })

  it('shows the CPU path and in-process presentation without an adapter line', async () => {
    mocks.readStatus.mockReturnValue(
      status({ path: 'cpu', reason: 'auto-unsafe-renderer', workerPresentation: false })
    )

    const container = await renderRenderingSection()

    expect(container.textContent).toContain('CPU · in-process')
    expect(container.textContent).toContain('a software or unidentified GPU was detected')
    expect(container.querySelector('.font-mono.block')).toBeNull()
  })

  it('renders a distinct explanation for every policy reason', async () => {
    const reasons: TerminalEngineRendererStatus['reason'][] = [
      'forced-on',
      'forced-off',
      'setting-on',
      'setting-off',
      'auto-allowed',
      'auto-no-webgl2',
      'auto-unsafe-renderer'
    ]
    const seen = new Set<string>()

    for (const reason of reasons) {
      mocks.readStatus.mockReturnValue(
        status({ reason, path: reason === 'auto-no-webgl2' ? 'cpu' : 'gpu' })
      )
      const container = await renderRenderingSection()
      const row = container.textContent ?? ''
      expect(row.length).toBeGreaterThan(0)
      seen.add(row)
    }

    expect(seen.size).toBe(reasons.length)
  })
})
