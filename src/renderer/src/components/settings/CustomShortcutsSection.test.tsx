// @vitest-environment happy-dom

import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ResolvedCustomKeybinding } from '../../../../shared/custom-keybindings'
import type { TerminalQuickCommand } from '../../../../shared/types'

vi.mock('@/lib/git-wasm/terminal-quick-commands', () => ({
  isTerminalAgentQuickCommand: (command: TerminalQuickCommand) => command.action === 'agent-prompt'
}))

import { CustomShortcutsSection } from './CustomShortcutsSection'
import { useAppStore } from '../../store'

const removeMock = vi.fn(() => Promise.resolve())

const quickCommandEntry: ResolvedCustomKeybinding = {
  id: 'custom.quickcmd0001',
  title: 'Run rebuild',
  action: { type: 'runQuickCommand', quickCommandId: 'qc-rebuild' },
  bindings: ['Mod+Alt+B']
}

function setStoreState(args: {
  customKeybindings: ResolvedCustomKeybinding[]
  terminalQuickCommands?: TerminalQuickCommand[]
}): void {
  useAppStore.setState({
    keybindings: {},
    customKeybindings: args.customKeybindings,
    removeCustomKeybinding: removeMock,
    settings: { terminalQuickCommands: args.terminalQuickCommands ?? [] }
  } as never)
}

function renderSection(query = ''): ReturnType<typeof render> {
  return render(
    <CustomShortcutsSection platform="darwin" conflictByAction={new Map()} query={query} />
  )
}

describe('CustomShortcutsSection', () => {
  beforeEach(() => {
    removeMock.mockClear()
  })
  afterEach(() => {
    cleanup()
  })

  it('warns when a referenced quick command no longer exists', () => {
    setStoreState({ customKeybindings: [quickCommandEntry], terminalQuickCommands: [] })
    renderSection()
    expect(
      screen
        .getAllByRole('alert')
        .some((node) => /quick command no longer exists/.test(node.textContent ?? ''))
    ).toBe(true)
  })

  it('shows the quick command label when the reference is live', () => {
    setStoreState({
      customKeybindings: [quickCommandEntry],
      terminalQuickCommands: [{ id: 'qc-rebuild', label: 'Rebuild', command: 'make', appendEnter: true }]
    })
    renderSection()
    expect(screen.getByText('Runs "Rebuild"')).toBeTruthy()
    expect(screen.queryByText(/quick command no longer exists/)).toBeNull()
  })

  it('deletes an entry through the store action', () => {
    setStoreState({ customKeybindings: [quickCommandEntry] })
    renderSection()
    fireEvent.click(screen.getByRole('button', { name: 'Delete Run rebuild' }))
    expect(removeMock).toHaveBeenCalledWith('custom.quickcmd0001')
  })

  it('filters rows with the shared shortcut search query', () => {
    setStoreState({
      customKeybindings: [
        quickCommandEntry,
        {
          id: 'custom.period000001',
          title: 'ASCII period',
          action: { type: 'sendText', text: '.' },
          bindings: ['Period'],
          matchPhysicalKey: true,
          decodedText: '.'
        }
      ]
    })
    renderSection('period')
    expect(screen.queryByText('Run rebuild')).toBeNull()
    expect(screen.getByText('ASCII period')).toBeTruthy()
  })

  it('shows the empty state when there are no custom shortcuts', () => {
    setStoreState({ customKeybindings: [] })
    renderSection()
    expect(screen.getByText('No custom shortcuts yet.')).toBeTruthy()
  })
})
