// TS dispatch for the git-publish-target-status parity module: maps the shared
// vector function names to the real `src/shared/git-publish-target-status.ts`
// exports so the harness compares the live TS reference against the Rust port.
// Only the display-name formatter still has a TS twin — remote-ref + status
// resolution moved fully to Rust (orca-git::publish_target_status), so their
// goldens are pinned by Rust unit tests, not this differential corpus.

import { getPublishTargetDisplayName } from '../../../src/shared/git-publish-target-status'
import type { GitPushTarget } from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'getPublishTargetDisplayName':
      return getPublishTargetDisplayName(input as GitPushTarget)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
