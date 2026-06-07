// TS dispatch for the git-upstream-status parity module: maps the shared vector
// function names to the real `src/shared/git-upstream-status.ts` exports so the
// harness compares the live TS reference against the Rust port.

import { shouldForcePushWithLeaseForUpstream } from '../../../src/shared/git-upstream-status'
import type { GitUpstreamStatus } from '../../../src/shared/git-status-types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'shouldForcePushWithLeaseForUpstream':
      // Single-arg `status`: null/undefined inputs short-circuit via optional chaining.
      return shouldForcePushWithLeaseForUpstream(input as GitUpstreamStatus | undefined)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
