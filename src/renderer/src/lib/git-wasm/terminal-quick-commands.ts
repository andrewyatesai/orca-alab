// Renderer terminal quick-command helpers, driven by the Rust orca-agents core in
// the orca-git wasm module (the shared TS bodies were deleted). Every call goes
// through the single `terminalQuickCommandOp` JSON boundary. All call sites are
// user-interaction time (settings edits, menus, the quick-command dialog), which
// is always after the eager `startGitWasm()` — so the pre-ready fallbacks below
// are defensive only, never hit in practice.
import { terminalQuickCommandOp } from './orca_git_wasm.js'
import { isGitWasmReady } from './git-line-stats'
import type {
  TerminalAgentQuickCommand,
  TerminalCommandQuickCommand,
  TerminalQuickCommand,
  TerminalQuickCommandAction,
  TerminalQuickCommandScope
} from '../../../../shared/types'

function op<T>(fn: string, input: unknown, fallback: T): T {
  if (!isGitWasmReady()) {
    return fallback
  }
  return JSON.parse(terminalQuickCommandOp(fn, JSON.stringify(input ?? null))) as T
}

export function getDefaultTerminalQuickCommands(): TerminalQuickCommand[] {
  return op('getDefaultTerminalQuickCommands', null, [])
}

export function normalizeTerminalQuickCommands(input: unknown): TerminalQuickCommand[] {
  return op('normalizeTerminalQuickCommands', input, [])
}

export function getTerminalQuickCommandScope(
  command: TerminalQuickCommand
): TerminalQuickCommandScope {
  return op('getTerminalQuickCommandScope', command, { type: 'global' })
}

export function terminalQuickCommandMatchesRepo(
  command: TerminalQuickCommand,
  repoId: string | null
): boolean {
  return op('terminalQuickCommandMatchesRepo', { command, repoId }, true)
}

export function getTerminalQuickCommandAction(
  command: TerminalQuickCommand
): TerminalQuickCommandAction {
  return op('getTerminalQuickCommandAction', command, 'terminal-command')
}

export function isTerminalAgentQuickCommand(
  command: TerminalQuickCommand
): command is TerminalAgentQuickCommand {
  return op<boolean>('isTerminalAgentQuickCommand', command, false)
}

export function supportsTerminalAgentQuickCommand(
  agent: unknown
): agent is TerminalAgentQuickCommand['agent'] {
  return op<boolean>('supportsTerminalAgentQuickCommand', agent, false)
}

export function getTerminalQuickCommandBody(command: TerminalQuickCommand): string {
  return op('getTerminalQuickCommandBody', command, '')
}

export function isTerminalQuickCommandComplete(command: TerminalQuickCommand): boolean {
  return op('isTerminalQuickCommandComplete', command, false)
}

export function buildTerminalQuickCommandInput(command: TerminalCommandQuickCommand): string {
  return op(
    'buildTerminalQuickCommandInput',
    command,
    command.appendEnter ? `${command.command}\r` : command.command
  )
}

export function flattenTerminalQuickCommand(
  command: TerminalCommandQuickCommand
): TerminalCommandQuickCommand {
  return op('flattenTerminalQuickCommand', command, command)
}
