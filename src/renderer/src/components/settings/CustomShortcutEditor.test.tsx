// @vitest-environment happy-dom

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ResolvedCustomKeybinding } from '../../../../shared/custom-keybindings'
import type { TerminalQuickCommand } from '../../../../shared/types'

vi.mock('@/lib/git-wasm/terminal-quick-commands', () => ({
  // Why: the wasm-op helper falls back to false before startGitWasm(); tests need the real discriminant.
  isTerminalAgentQuickCommand: (command: TerminalQuickCommand) => command.action === 'agent-prompt'
}))

import { CustomShortcutEditor } from './CustomShortcutEditor'
import { useAppStore } from '../../store'

const upsertMock = vi.fn(() => Promise.resolve())

function setStoreState(overrides: {
  customKeybindings?: ResolvedCustomKeybinding[]
  terminalQuickCommands?: TerminalQuickCommand[]
}): void {
  useAppStore.setState({
    keybindings: {},
    customKeybindings: overrides.customKeybindings ?? [],
    upsertCustomKeybinding: upsertMock,
    settings: { terminalQuickCommands: overrides.terminalQuickCommands ?? [] }
  } as never)
}

function renderEditor(entry: ResolvedCustomKeybinding | null = null) {
  return render(
    <CustomShortcutEditor open platform="darwin" entry={entry} onClose={vi.fn()} />
  )
}

describe('CustomShortcutEditor', () => {
  beforeEach(() => {
    upsertMock.mockClear()
    setStoreState({})
  })
  afterEach(() => {
    cleanup()
  })

  it('renders a live hex preview of the decoded sendText escapes', () => {
    renderEditor()
    const input = screen.getByLabelText('Text to send')
    fireEvent.change(input, { target: { value: '\\x1b[13;2u' } })
    expect(screen.getByTestId('hex-preview').textContent).toBe('1b 5b 31 33 3b 32 75')
  })

  it('shows the decode error instead of a preview for an escape typo', () => {
    renderEditor()
    fireEvent.change(screen.getByLabelText('Text to send'), { target: { value: '\\q' } })
    expect(screen.queryByTestId('hex-preview')).toBeNull()
    expect(screen.getByRole('alert').textContent).toMatch(/Unknown escape/)
  })

  it('shows the bare-key shadow warning for a chord without modifiers', () => {
    renderEditor({
      id: 'custom.bare00000001',
      title: 'Bare period',
      action: { type: 'sendText', text: '.' },
      bindings: ['Period'],
      decodedText: '.'
    })
    expect(
      screen.getAllByRole('alert').some((node) =>
        /will no longer type its character/.test(node.textContent ?? '')
      )
    ).toBe(true)
  })

  it('blocks saving a conflicting chord and names the counterpart', async () => {
    renderEditor({
      id: 'custom.clash0000001',
      title: 'Clash',
      action: { type: 'sendText', text: 'x' },
      bindings: ['Mod+Shift+C'],
      decodedText: 'x'
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => {
      expect(
        screen
          .getAllByRole('alert')
          .some((node) => /Copy terminal selection/.test(node.textContent ?? ''))
      ).toBe(true)
    })
    expect(upsertMock).not.toHaveBeenCalled()
  })

  it('saves a valid entry through the store action', async () => {
    renderEditor({
      id: 'custom.valid0000001',
      title: 'Valid macro',
      action: { type: 'sendText', text: '\\e[13;2u' },
      bindings: ['Mod+Alt+M'],
      decodedText: '\x1b[13;2u'
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(upsertMock).toHaveBeenCalledTimes(1))
    expect(upsertMock).toHaveBeenCalledWith(
      expect.objectContaining({
        id: 'custom.valid0000001',
        title: 'Valid macro',
        action: { type: 'sendText', text: '\\e[13;2u' },
        bindings: ['Mod+Alt+M']
      })
    )
  })

  it('filters agent-prompt quick commands out of the picker', () => {
    setStoreState({
      terminalQuickCommands: [
        {
          id: 'qc-agent',
          label: 'Ask agent',
          action: 'agent-prompt',
          agent: 'claude',
          prompt: 'p'
        } as TerminalQuickCommand
      ]
    })
    renderEditor()
    fireEvent.click(screen.getByRole('radio', { name: 'Run quick command' }))
    // The only quick command is agent-scoped, so the picker shows the filtered empty state.
    expect(screen.getByText(/No terminal quick commands yet/).textContent).toMatch(
      /Agent-prompt commands are not supported/
    )
  })

  it('offers terminal-command quick commands in the picker', () => {
    setStoreState({
      terminalQuickCommands: [
        { id: 'qc-build', label: 'Build', command: 'make', appendEnter: true },
        {
          id: 'qc-agent',
          label: 'Ask agent',
          action: 'agent-prompt',
          agent: 'claude',
          prompt: 'p'
        } as TerminalQuickCommand
      ]
    })
    renderEditor()
    fireEvent.click(screen.getByRole('radio', { name: 'Run quick command' }))
    expect(screen.queryByText(/No terminal quick commands yet/)).toBeNull()
    expect(screen.getByLabelText('Quick command')).toBeTruthy()
  })
})
