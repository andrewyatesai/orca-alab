// TS dispatch for the tailnet-address parity module: maps the shared vector
// function names to the real `src/shared/tailnet-address.ts` exports so the
// harness compares the live TS reference against the Rust port.

import { isTailnetIPv4Address } from '../../../src/shared/tailnet-address'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'isTailnetIPv4Address':
      return isTailnetIPv4Address(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
