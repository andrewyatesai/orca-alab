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

beforeEach(() => {
  detectMock.mockReset()
  Object.assign(window, { api: { posixShells: { detect: detectMock } } })
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

  it('keeps a hand-edited shell path visible under its display name', async () => {
    detectMock.mockResolvedValue(ZSH_BASH_DETECTION)
    render(<TerminalPosixShellSection updateSettings={vi.fn()} posixShell="/opt/weird/xonsh" />)

    await waitFor(() => expect(screen.getByRole('radio', { name: 'xonsh' })).toBeChecked())
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
