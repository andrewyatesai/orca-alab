// @vitest-environment happy-dom

import '@testing-library/jest-dom/vitest'

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import type { PosixTerminalShellDetection } from '../../../../shared/posix-terminal-shell'
import { TerminalPosixShellSection } from './TerminalPosixShellSection'

vi.mock('../../store', () => ({
  useAppStore: (selector: (state: { settingsSearchQuery: string }) => unknown) =>
    selector({ settingsSearchQuery: '' })
}))

const detectMock = vi.fn<() => Promise<PosixTerminalShellDetection>>()
const validatePathMock = vi.fn()

beforeEach(() => {
  detectMock.mockReset()
  validatePathMock.mockReset()
  validatePathMock.mockResolvedValue({ ok: true, resolvedPath: '/bin/zsh' })
  Object.assign(window, {
    api: {
      posixShells: { detect: detectMock },
      terminalShell: { validatePath: validatePathMock }
    }
  })
})

afterEach(cleanup)

const ZSH_BASH_DETECTION: PosixTerminalShellDetection = {
  shells: [
    { shell: 'zsh', path: '/bin/zsh' },
    { shell: 'bash', path: '/bin/bash' }
  ],
  systemShellName: 'zsh'
}

describe('TerminalPosixShellSection', () => {
  it('offers System plus the detected shells and stores a picked shell name', async () => {
    detectMock.mockResolvedValue(ZSH_BASH_DETECTION)
    const user = userEvent.setup()
    const updateSettings = vi.fn()
    render(<TerminalPosixShellSection updateSettings={updateSettings} posixShell={null} />)

    expect(screen.getByRole('radio', { name: 'System login shell' })).toBeChecked()
    await waitFor(() => expect(screen.getByRole('radio', { name: 'zsh' })).toBeEnabled())
    expect(screen.getByRole('radio', { name: 'bash' })).toBeEnabled()
    expect(screen.queryByRole('radio', { name: 'fish' })).not.toBeInTheDocument()

    await user.click(screen.getByRole('radio', { name: 'zsh' }))
    expect(updateSettings).toHaveBeenCalledWith({ terminalPosixShell: 'zsh' })
  })

  it('clears the setting when switching back to System', async () => {
    detectMock.mockResolvedValue(ZSH_BASH_DETECTION)
    const user = userEvent.setup()
    const updateSettings = vi.fn()
    render(<TerminalPosixShellSection updateSettings={updateSettings} posixShell="bash" />)

    await waitFor(() => expect(screen.getByRole('radio', { name: 'bash' })).toBeChecked())
    await user.click(screen.getByRole('radio', { name: 'System login shell' }))
    expect(updateSettings).toHaveBeenCalledWith({ terminalPosixShell: null })
  })

  it('keeps a selected-but-missing shell visible and disabled', async () => {
    detectMock.mockResolvedValue(ZSH_BASH_DETECTION)
    render(<TerminalPosixShellSection updateSettings={vi.fn()} posixShell="fish" />)

    await waitFor(() => expect(screen.getByRole('radio', { name: 'fish' })).toBeDisabled())
    expect(screen.getByRole('radio', { name: 'fish' })).toBeChecked()
  })

  it('shows a hand-edited shell path as the selected Custom… choice with the path editable (#7467)', async () => {
    detectMock.mockResolvedValue(ZSH_BASH_DETECTION)
    render(<TerminalPosixShellSection updateSettings={vi.fn()} posixShell="/opt/weird/xonsh" />)

    await waitFor(() =>
      expect(screen.getByRole('radio', { name: 'Custom shell path' })).toBeChecked()
    )
    expect(screen.getByDisplayValue('/opt/weird/xonsh')).toBeInTheDocument()
  })

  it('keeps a hand-edited bare shell name visible under its display name', async () => {
    detectMock.mockResolvedValue(ZSH_BASH_DETECTION)
    render(<TerminalPosixShellSection updateSettings={vi.fn()} posixShell="xonsh" />)

    await waitFor(() => expect(screen.getByRole('radio', { name: 'xonsh' })).toBeChecked())
  })

  it('persists a custom path typed into the Custom… input (#7467)', async () => {
    detectMock.mockResolvedValue(ZSH_BASH_DETECTION)
    const user = userEvent.setup()
    const updateSettings = vi.fn()
    render(<TerminalPosixShellSection updateSettings={updateSettings} posixShell={null} />)

    await user.click(screen.getByRole('radio', { name: 'Custom shell path' }))
    // Why: selecting Custom… must not clobber the stored setting before a path is committed.
    expect(updateSettings).not.toHaveBeenCalled()

    const input = screen.getByPlaceholderText('/usr/local/bin/fish')
    await user.type(input, '/usr/local/bin/fish')
    await user.tab()
    expect(updateSettings).toHaveBeenCalledWith({ terminalPosixShell: '/usr/local/bin/fish' })
  })

  it('surfaces validation errors for a bad custom path (#7467)', async () => {
    detectMock.mockResolvedValue(ZSH_BASH_DETECTION)
    validatePathMock.mockResolvedValue({ ok: false, reason: 'not-found' })
    const user = userEvent.setup()
    render(<TerminalPosixShellSection updateSettings={vi.fn()} posixShell={null} />)

    await user.click(screen.getByRole('radio', { name: 'Custom shell path' }))
    await user.type(screen.getByPlaceholderText('/usr/local/bin/fish'), '/gone/fish')

    await waitFor(
      () => expect(screen.getByText('No file exists at this path.')).toBeInTheDocument(),
      { timeout: 3000 }
    )
    expect(validatePathMock).toHaveBeenCalledWith('/gone/fish')
  })

  it('keeps the selected shell usable when detection fails', async () => {
    detectMock.mockRejectedValue(new Error('runtime method missing'))
    render(<TerminalPosixShellSection updateSettings={vi.fn()} posixShell="fish" />)

    await waitFor(() => expect(detectMock).toHaveBeenCalled())
    expect(screen.getByRole('radio', { name: 'fish' })).toBeEnabled()
    expect(screen.getByRole('radio', { name: 'fish' })).toBeChecked()
  })
})

describe('nu choice (#8928 PR1)', () => {
  it('offers nu when detected', async () => {
    detectMock.mockResolvedValue({
      shells: [
        { shell: 'zsh', path: '/bin/zsh' },
        { shell: 'nu', path: '/usr/local/bin/nu' }
      ],
      systemShellName: 'zsh'
    })
    const user = userEvent.setup()
    const updateSettings = vi.fn()
    render(<TerminalPosixShellSection updateSettings={updateSettings} posixShell={null} />)

    await waitFor(() => expect(screen.getByRole('radio', { name: 'nu' })).toBeEnabled())
    await user.click(screen.getByRole('radio', { name: 'nu' }))
    expect(updateSettings).toHaveBeenCalledWith({ terminalPosixShell: 'nu' })
  })

  it('hides nu when not installed', async () => {
    detectMock.mockResolvedValue(ZSH_BASH_DETECTION)
    render(<TerminalPosixShellSection updateSettings={vi.fn()} posixShell={null} />)

    await waitFor(() => expect(screen.getByRole('radio', { name: 'zsh' })).toBeEnabled())
    expect(screen.queryByRole('radio', { name: 'nu' })).not.toBeInTheDocument()
  })
})
