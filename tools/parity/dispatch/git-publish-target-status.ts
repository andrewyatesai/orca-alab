// TS dispatch for the git-publish-target-status parity module: maps the shared
// vector function names to the real `src/shared/git-publish-target-status.ts`
// exports so the harness compares the live TS reference against the Rust port.
// Only the pure target-naming helpers are covered; getPublishTargetStatus is
// io-injected (takes a GitCommandRunner) and is out of scope for parity.

import {
  getPublishTargetDisplayName,
  getPublishTargetRemoteRef
} from '../../../src/shared/git-publish-target-status'
import type { GitPushTarget } from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'getPublishTargetDisplayName':
      return getPublishTargetDisplayName(input as GitPushTarget)
    case 'getPublishTargetRemoteRef':
      return getPublishTargetRemoteRef(input as GitPushTarget)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
