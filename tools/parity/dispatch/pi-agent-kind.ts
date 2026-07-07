// TS dispatch for the pi-agent-kind parity module. The shared TS detector was
// DELETED (the Rust orca-text core is the sole impl — napi in main, wasm in
// the relay), so this adapter drives the napi binding: the vectors' recorded
// goldens now pin the napi surface absolutely. Requires the built addon.

import { requireRustGitBinding } from '../../../src/main/daemon/rust-git-addon'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'detectPiAgentKindFromCommand': {
      // TS `undefined` round-trips through JSON as `null`; map it back so the
      // bare-shell (no-command) default case is exercised identically.
      const command = input == null ? undefined : (input as string)
      return requireRustGitBinding().detectPiAgentKindFromCommand(command)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
