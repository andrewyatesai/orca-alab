// @vitest-environment happy-dom

import '@testing-library/jest-dom/vitest'

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import { TerminalWindowsShellSection } from './TerminalWindowsShellSection'

vi.mock('../../store', () => ({
  useAppStore: (selector: (state: { settingsSearchQuery: string }) => unknown) =>
    selector({ settingsSearchQuery: '' })
}))

const validatePathMock = vi.fn()

beforeEach(() => {
  validatePathMock.mockReset()
  validatePathMock.mockResolvedValue({ ok: true, resolvedPath: 'D:\\tools\\pwsh.exe' })
  Object.assign(window, { api: { terminalShell: { validatePath: validatePathMock } } })
})

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

  it('keeps a persisted WSL default visible as a disabled selected segment (#9779)', () => {
    render(
      <TerminalWindowsShellSection
        updateSettings={vi.fn()}
        windowsShell="wsl.exe"
        gitBashAvailable={false}
        nushellAvailable={false}
      />
    )

    const wslOption = screen.getByRole('radio', { name: 'WSL' })
    expect(wslOption).toBeDisabled()
    expect(wslOption).toBeChecked()
    // Why (#9779): PowerShell must not be shown as selected when the real default is WSL.
    expect(screen.getByRole('radio', { name: 'PowerShell' })).not.toBeChecked()
  })

  it('does not offer WSL when the default shell is not WSL (#9779)', () => {
    render(
      <TerminalWindowsShellSection
        updateSettings={vi.fn()}
        windowsShell="powershell.exe"
        gitBashAvailable={false}
        nushellAvailable={false}
      />
    )

    expect(screen.queryByRole('radio', { name: 'WSL' })).not.toBeInTheDocument()
  })

  it('persists a custom absolute path typed into the Custom… input (#7467)', async () => {
    const user = userEvent.setup()
    const updateSettings = vi.fn()
    render(
      <TerminalWindowsShellSection
        updateSettings={updateSettings}
        windowsShell="powershell.exe"
        gitBashAvailable={false}
        nushellAvailable={false}
      />
    )

    await user.click(screen.getByRole('radio', { name: 'Custom shell path' }))
    // Why: selecting Custom… must not clobber the stored setting before a path is committed.
    expect(updateSettings).not.toHaveBeenCalled()

    const input = screen.getByPlaceholderText('C:\\Program Files\\PowerShell\\7\\pwsh.exe')
    await user.type(input, 'D:\\tools\\pwsh-daily\\pwsh.exe')
    await user.tab()
    expect(updateSettings).toHaveBeenCalledWith({
      terminalWindowsShell: 'D:\\tools\\pwsh-daily\\pwsh.exe'
    })
  })

  it('shows a stored custom path as the selected Custom… choice and surfaces validation errors (#7467)', async () => {
    // Why: JSX string attributes keep backslash pairs literal — pass a JS string so the value matches assertions.
    const stalePath = 'D:\\removed\\pwsh.exe'
    validatePathMock.mockResolvedValue({ ok: false, reason: 'not-found' })
    render(
      <TerminalWindowsShellSection
        updateSettings={vi.fn()}
        windowsShell={stalePath}
        gitBashAvailable={false}
        nushellAvailable={false}
      />
    )

    expect(screen.getByRole('radio', { name: 'Custom shell path' })).toBeChecked()
    expect(screen.getByDisplayValue('D:\\removed\\pwsh.exe')).toBeInTheDocument()
    await waitFor(
      () => expect(screen.getByText('No file exists at this path.')).toBeInTheDocument(),
      { timeout: 3000 }
    )
    expect(validatePathMock).toHaveBeenCalledWith('D:\\removed\\pwsh.exe')
  })

  it('reports the recoverable Store-alias target in the error line (#7467)', async () => {
    const target = 'C:\\Program Files\\WindowsApps\\Microsoft.PowerShell_7\\pwsh.exe'
    const aliasStubPath = 'C:\\Users\\dev\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe'
    validatePathMock.mockResolvedValue({
      ok: false,
      reason: 'not-executable',
      resolvedPath: target
    })
    render(
      <TerminalWindowsShellSection
        updateSettings={vi.fn()}
        windowsShell={aliasStubPath}
        gitBashAvailable={false}
        nushellAvailable={false}
      />
    )

    await waitFor(
      () => expect(screen.getByText(new RegExp('Store app alias'))).toBeInTheDocument(),
      { timeout: 3000 }
    )
    expect(screen.getByText(new RegExp('Microsoft\\.PowerShell_7'))).toBeInTheDocument()
  })
})
