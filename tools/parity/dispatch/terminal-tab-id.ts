// TS dispatch for the terminal-tab-id parity module: maps the shared vector
// function names to the real `src/shared/terminal-tab-id.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  isValidHostTerminalTabId,
  isValidTerminalTabId
} from '../../../src/shared/terminal-tab-id'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'isValidTerminalTabId':
      return isValidTerminalTabId(input as string)
    case 'isValidHostTerminalTabId':
      return isValidHostTerminalTabId(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
