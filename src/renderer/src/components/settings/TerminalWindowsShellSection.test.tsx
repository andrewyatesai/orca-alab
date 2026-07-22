// @vitest-environment happy-dom

import '@testing-library/jest-dom/vitest'

import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import { TerminalWindowsShellSection } from './TerminalWindowsShellSection'

vi.mock('../../store', () => ({
  useAppStore: (selector: (state: { settingsSearchQuery: string }) => unknown) =>
    selector({ settingsSearchQuery: '' })
}))

afterEach(cleanup)

describe('TerminalWindowsShellSection', () => {
  it('offers Nushell when nu.exe is detected and stores the sentinel', async () => {
    const user = userEvent.setup()
    const updateSettings = vi.fn()
    render(
      <TerminalWindowsShellSection
        updateSettings={updateSettings}
        windowsShell="powershell.exe"
        gitBashAvailable={false}
        nushellAvailable={true}
      />
    )

    expect(screen.queryByRole('radio', { name: 'Git Bash' })).not.toBeInTheDocument()
    const nushellOption = screen.getByRole('radio', { name: 'Nushell' })
    expect(nushellOption).toBeEnabled()

    await user.click(nushellOption)
    expect(updateSettings).toHaveBeenCalledWith({ terminalWindowsShell: 'nushell' })
  })

  it('hides Nushell when nu.exe is missing and not selected', () => {
    render(
      <TerminalWindowsShellSection
        updateSettings={vi.fn()}
        windowsShell="powershell.exe"
        gitBashAvailable={true}
        nushellAvailable={false}
      />
    )

    expect(screen.queryByRole('radio', { name: 'Nushell' })).not.toBeInTheDocument()
    expect(screen.getByRole('radio', { name: 'Git Bash' })).toBeEnabled()
  })

  it('keeps a selected-but-missing Nushell visible and disabled', () => {
    render(
      <TerminalWindowsShellSection
        updateSettings={vi.fn()}
        windowsShell="nushell"
        gitBashAvailable={false}
        nushellAvailable={false}
      />
    )

    const nushellOption = screen.getByRole('radio', { name: 'Nushell' })
    expect(nushellOption).toBeDisabled()
    expect(nushellOption).toBeChecked()
  })
})
