// TS dispatch for the pi-agent-kind parity module: maps the shared vector
// function names to the real `src/shared/pi-agent-kind.ts` export so the
// harness compares the live TS reference against the Rust port.

import { detectPiAgentKindFromCommand } from '../../../src/shared/pi-agent-kind'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'detectPiAgentKindFromCommand': {
      // TS `undefined` round-trips through JSON as `null`; map it back so the
      // bare-shell (no-command) default case is exercised identically.
      const command = input == null ? undefined : (input as string)
      return detectPiAgentKindFromCommand(command)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
