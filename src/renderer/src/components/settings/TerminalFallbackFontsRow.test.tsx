// @vitest-environment happy-dom

import '@testing-library/jest-dom/vitest'

import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import type { GlobalSettings } from '../../../../shared/types'
import { TerminalFallbackFontsRow } from './TerminalFallbackFontsRow'

vi.mock('../../store', () => ({
  useAppStore: (selector: (state: { settingsSearchQuery: string }) => unknown) =>
    selector({ settingsSearchQuery: '' })
}))

afterEach(cleanup)

function renderRow(families: string[]): { updateSettings: ReturnType<typeof vi.fn> } {
  const updateSettings = vi.fn()
  render(
    <TerminalFallbackFontsRow
      settings={{ terminalFontFallbackFamilies: families } as unknown as GlobalSettings}
      updateSettings={updateSettings}
      suggestions={['Fira Code', 'Iosevka', 'Menlo']}
    />
  )
  return { updateSettings }
}

describe('TerminalFallbackFontsRow', () => {
  it('adds a staged family to the end of terminalFontFallbackFamilies', async () => {
    const user = userEvent.setup()
    const { updateSettings } = renderRow(['Fira Code'])

    await user.type(screen.getByPlaceholderText('Add fallback font'), 'Iosevka')
    await user.click(screen.getByRole('button', { name: 'Add fallback font' }))

    expect(updateSettings).toHaveBeenCalledWith({
      terminalFontFallbackFamilies: ['Fira Code', 'Iosevka']
    })
  })

  it('ignores a duplicate family (case-insensitive) instead of re-adding it', async () => {
    const user = userEvent.setup()
    const { updateSettings } = renderRow(['Fira Code'])

    await user.type(screen.getByPlaceholderText('Add fallback font'), 'fira code')
    await user.click(screen.getByRole('button', { name: 'Add fallback font' }))

    expect(updateSettings).not.toHaveBeenCalled()
  })

  it('reorders via the arrow buttons and preserves the rest of the list', async () => {
    const user = userEvent.setup()
    const { updateSettings } = renderRow(['Fira Code', 'Iosevka', 'Menlo'])

    await user.click(screen.getByRole('button', { name: 'Move Iosevka earlier' }))

    expect(updateSettings).toHaveBeenCalledWith({
      terminalFontFallbackFamilies: ['Iosevka', 'Fira Code', 'Menlo']
    })
  })

  it('disables moving the first entry earlier and the last entry later', () => {
    renderRow(['Fira Code', 'Iosevka'])

    expect(screen.getByRole('button', { name: 'Move Fira Code earlier' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Move Iosevka later' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Move Fira Code later' })).toBeEnabled()
  })

  it('removes an entry, keeping the order of the others', async () => {
    const user = userEvent.setup()
    const { updateSettings } = renderRow(['Fira Code', 'Iosevka', 'Menlo'])

    await user.click(screen.getByRole('button', { name: 'Remove Iosevka' }))

    expect(updateSettings).toHaveBeenCalledWith({
      terminalFontFallbackFamilies: ['Fira Code', 'Menlo']
    })
  })
})
