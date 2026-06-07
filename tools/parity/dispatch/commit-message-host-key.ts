// TS dispatch for the commit-message-host-key parity module: maps the shared
// vector function names to the real `src/shared/commit-message-host-key.ts`
// exports so the harness compares the live TS reference against the Rust port.

import { getCommitMessageModelDiscoveryHostKeyForScope } from '../../../src/shared/commit-message-host-key'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'getCommitMessageModelDiscoveryHostKeyForScope': {
      // Absent `scope` key destructures to undefined (TS `undefined` → unknown);
      // explicit JSON null stays null (TS falsy → local).
      const { scope } = input as { scope?: string | null }
      return getCommitMessageModelDiscoveryHostKeyForScope(scope)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
