// TS dispatch for the wsl-paths parity module: maps the shared vector function
// names to the real `src/shared/wsl-paths.ts` exports so the harness compares
// the live TS reference against the Rust port.

import { isWslUncPath, parseWslUncPath } from '../../../src/shared/wsl-paths'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'parseWslUncPath':
      return parseWslUncPath(input as string)
    case 'isWslUncPath':
      return isWslUncPath(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
