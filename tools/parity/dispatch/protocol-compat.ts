// TS dispatch for the protocol-compat parity module: maps the shared vector
// function names to the real `src/shared/protocol-compat.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  describeRuntimeCompatBlock,
  evaluateCompat,
  evaluateRuntimeCompat,
  type RuntimeCompatVerdict
} from '../../../src/shared/protocol-compat'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'evaluateRuntimeCompat':
      return evaluateRuntimeCompat(input as Parameters<typeof evaluateRuntimeCompat>[0])
    case 'describeRuntimeCompatBlock':
      return describeRuntimeCompatBlock(input as RuntimeCompatVerdict)
    case 'evaluateCompat':
      return evaluateCompat(input as Parameters<typeof evaluateCompat>[0])
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
