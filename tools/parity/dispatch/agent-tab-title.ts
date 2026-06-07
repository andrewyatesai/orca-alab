// TS dispatch for the agent-tab-title parity module: maps the shared vector
// function names to the real `src/shared/agent-tab-title.ts` exports so the
// harness compares the live TS reference against the Rust port.

import { deriveGeneratedTabTitle } from '../../../src/shared/agent-tab-title'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'deriveGeneratedTabTitle':
      return deriveGeneratedTabTitle(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
