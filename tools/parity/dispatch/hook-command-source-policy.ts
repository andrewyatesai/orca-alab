// TS dispatch for the hook-command-source-policy parity module: maps the shared
// vector function names to the real `src/shared/hook-command-source-policy.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  normalizeHookCommandSourcePolicy,
  resolveHookCommandSourcePolicy
} from '../../../src/shared/hook-command-source-policy'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'normalizeHookCommandSourcePolicy':
      // Single-arg pure function: `input` is the raw `policy` value.
      return normalizeHookCommandSourcePolicy(input)
    case 'resolveHookCommandSourcePolicy': {
      const { policy, hasLocalScript } = input as { policy?: unknown; hasLocalScript: boolean }
      // Tri-state: JSON has no `undefined`, so both an absent key and a literal
      // null must map to `undefined` to take the absent-setting branch (which
      // can default to local-only). A present invalid string stays a string, so
      // it never enables local-only — matching the Rust port's `None` vs
      // `Some(invalid)` distinction.
      return resolveHookCommandSourcePolicy(policy ?? undefined, { hasLocalScript })
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
