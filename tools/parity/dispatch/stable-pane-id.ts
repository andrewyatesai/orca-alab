// TS dispatch for the stable-pane-id parity module: maps the shared vector
// function names to the real `src/shared/stable-pane-id.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  isStablePaneId,
  isTerminalLeafId,
  parseLegacyNumericPaneKey,
  parsePaneKey
} from '../../../src/shared/stable-pane-id'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'isStablePaneId':
      return isStablePaneId(input as string)
    case 'isTerminalLeafId':
      return isTerminalLeafId(input as string)
    case 'parsePaneKey':
      return parsePaneKey(input as string)
    case 'parseLegacyNumericPaneKey':
      // Takes `unknown` and validates the type itself, so pass input straight through.
      return parseLegacyNumericPaneKey(input)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
