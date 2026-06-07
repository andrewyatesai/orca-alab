// TS dispatch for the hook-command-source-policy parity module: maps the shared
// vector function names to the real `src/shared/hook-command-source-policy.ts`
// exports so the harness compares the live TS reference against the Rust port.

import { normalizeHookCommandSourcePolicy } from '../../../src/shared/hook-command-source-policy'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'normalizeHookCommandSourcePolicy':
      // Single-arg pure function: `input` is the raw `policy` value.
      return normalizeHookCommandSourcePolicy(input)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
