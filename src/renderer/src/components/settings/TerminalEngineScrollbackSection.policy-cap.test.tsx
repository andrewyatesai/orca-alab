// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { I18nextProvider } from 'react-i18next'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { i18n } from '@/i18n/i18n'
import { getDefaultSettings } from '../../../../shared/constants'
import {
  DESKTOP_TERMINAL_SCROLLBACK_ROWS_DEFAULT,
  DESKTOP_TERMINAL_SCROLLBACK_ROWS_MAX,
  DESKTOP_TERMINAL_SCROLLBACK_ROWS_MIN
} from '../../../../shared/terminal-scrollback-policy'

// The Scrollback Rows field must advertise exactly what the canonical policy
// keeps: normalizeDesktopTerminalScrollbackRows clamps to 50k, so a UI max above
// it (the old 100k) silently discarded half the typed value.

const mocks = vi.hoisted(() => ({
  state: { settingsSearchQuery: '' }
}))

vi.mock('../../store', () => {
  const useAppStore = (selector: (state: typeof mocks.state) => unknown): unknown =>
    selector(mocks.state)
  useAppStore.getState = () => mocks.state
  return { useAppStore }
})

import { TerminalEngineScrollbackSection } from './TerminalEngineBehaviorSections'

const mountedRoots: Root[] = []

afterEach(() => {
  for (const root of mountedRoots) {
    act(() => root.unmount())
  }
  mountedRoots.length = 0
  document.body.innerHTML = ''
})

async function renderScrollbackSection(
  updateSettings: (updates: Record<string, unknown>) => void
): Promise<HTMLInputElement> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  mountedRoots.push(root)

  await act(async () => {
    root.render(
      <I18nextProvider i18n={i18n}>
        <TerminalEngineScrollbackSection
          settings={getDefaultSettings('/tmp')}
          updateSettings={updateSettings}
        />
      </I18nextProvider>
    )
  })

  const input = container.querySelector('input[type="number"]')
  expect(input).not.toBeNull()
  return input as HTMLInputElement
}

describe('TerminalEngineScrollbackSection scrollback-rows policy cap', () => {
  it('advertises exactly the policy bounds (UI max == policy max)', async () => {
    const input = await renderScrollbackSection(vi.fn())
    expect(Number(input.max)).toBe(DESKTOP_TERMINAL_SCROLLBACK_ROWS_MAX)
    expect(Number(input.min)).toBe(DESKTOP_TERMINAL_SCROLLBACK_ROWS_MIN)
  })

  it('shows the policy default, not a made-up one', async () => {
    const container = await renderScrollbackSection(vi.fn())
    expect(container.ownerDocument.body.textContent).toContain(
      String(DESKTOP_TERMINAL_SCROLLBACK_ROWS_DEFAULT)
    )
  })

  it('commits an over-max entry clamped to the policy max', async () => {
    const updateSettings = vi.fn()
    const input = await renderScrollbackSection(updateSettings)

    await act(async () => {
      // Set through the prototype setter so React's value tracker sees the change.
      const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
      setValue?.call(input, '100000')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await act(async () => {
      // Enter commits like blur; React's synthetic onBlur (focusout) is flaky in happy-dom.
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    })

    expect(updateSettings).toHaveBeenCalledWith({
      terminalScrollbackRows: DESKTOP_TERMINAL_SCROLLBACK_ROWS_MAX
    })
  })
})
