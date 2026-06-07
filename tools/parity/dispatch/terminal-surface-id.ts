// TS dispatch for the terminal-surface-id parity module: maps the shared vector
// function names to the real `src/shared/terminal-surface-id.ts` exports so the
// harness compares the live TS reference against the Rust port. All three
// functions take a single string argument, so `input` is the raw string.

import {
  isWebTerminalSurfaceTabId,
  toHostSessionTabId,
  toWebTerminalSurfaceTabId
} from '../../../src/shared/terminal-surface-id'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'toWebTerminalSurfaceTabId':
      return toWebTerminalSurfaceTabId(input as string)
    case 'toHostSessionTabId':
      return toHostSessionTabId(input as string)
    case 'isWebTerminalSurfaceTabId':
      return isWebTerminalSurfaceTabId(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
