// TS dispatch for the terminal-quick-commands parity module: maps the shared
// vector function names to the real `src/shared/terminal-quick-commands.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  buildTerminalQuickCommandInput,
  getDefaultTerminalQuickCommands,
  getTerminalQuickCommandAction,
  getTerminalQuickCommandBody,
  isTerminalQuickCommandComplete,
  normalizeTerminalQuickCommands,
  supportsTerminalAgentQuickCommand,
  terminalQuickCommandMatchesRepo
} from '../../../src/shared/terminal-quick-commands'
import type {
  TerminalCommandQuickCommand,
  TerminalQuickCommand
} from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'normalizeTerminalQuickCommands':
      return normalizeTerminalQuickCommands(input)
    case 'getDefaultTerminalQuickCommands':
      return getDefaultTerminalQuickCommands()
    case 'supportsTerminalAgentQuickCommand':
      return supportsTerminalAgentQuickCommand(input)
    case 'getTerminalQuickCommandAction':
      return getTerminalQuickCommandAction(input as TerminalQuickCommand)
    case 'getTerminalQuickCommandBody':
      return getTerminalQuickCommandBody(input as TerminalQuickCommand)
    case 'isTerminalQuickCommandComplete':
      return isTerminalQuickCommandComplete(input as TerminalQuickCommand)
    case 'buildTerminalQuickCommandInput':
      return buildTerminalQuickCommandInput(input as TerminalCommandQuickCommand)
    case 'terminalQuickCommandMatchesRepo': {
      const { command, repoId } = input as {
        command: TerminalQuickCommand
        repoId: string | null
      }
      return terminalQuickCommandMatchesRepo(command, repoId)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
